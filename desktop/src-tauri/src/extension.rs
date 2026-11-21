//! Local endpoint for the Lunta browser extension, on 127.0.0.1:47480 only: the same protocol as
//! Lunta for Mac (apple/Mac/ExtensionServer.swift), so the Chrome extension works with Lunta on
//! Windows and Linux.
//!
//!   GET  /status?doi=…&title=…   → {"inLibrary":bool,"id":…,"hasPdf":bool,"hasReadingView":bool,"title":…}
//!   POST /save   {doi,title,authors,journal,date,url,abstract}  → status JSON of the saved paper
//!   POST /pdf?id=…   (body: PDF bytes)                          → status JSON
//!   POST /pdf?name=… (no id: add / match a standalone PDF)        → status JSON
//!   POST /fulltext[?dryRun=1]  {url, html, liveHtml?, doi?, title?, images:[{urls:[…], base64}],
//!                               imagesSkipped?, dryRun?}
//!        "Use this page's full text": the article page becomes the paper's reading view (see
//!        lunta_core::pagetext: highlights must still be found, the old reading view is backed up
//!        to <app data>/backups) → status JSON + {outcome, message, words, images, highlights,
//!        unmatched, backup?, notes, reader, dryRun}; 422 + {error} if the page can't be made
//!        into a reading view.
//!   GET/POST /cite/…   citing in Google Docs from the extension: the citation API (citeapi.rs)
//!
//! Only requests from a browser extension are accepted: a request with a web page's Origin is
//! refused, and the extension's own header (X-Margins-Client), which a web page can't send
//! without a CORS preflight we never approve, is required.

use crate::{add, AppState};
use lunta_core::convert::{Options, PageImage};
use lunta_core::model::{FileKind, Paper, PaperMeta};
use lunta_core::pagetext::{self, PageSubmission};
use lunta_core::Library;
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};


pub const PORT: u16 = 47480;
/// Requests larger than this (a page with its figures) are dropped.
const MAX_REQUEST: usize = 300 * 1024 * 1024;

/// Starts the endpoint in the background. If the port is taken (e.g. another Lunta is running),
/// the extension talks to that one and this one does without.
pub fn start(app: AppHandle) {
    // a debug build may be given another port (to test beside the real app)
    let chosen: Option<u16> =
        if cfg!(debug_assertions) { std::env::var("LUNTA_EXTENSION_PORT").ok().and_then(|p| p.parse().ok()) } else { None };
    // on a Mac (where this app runs only for testing) the port is Lunta for Mac's
    if cfg!(target_os = "macos") && chosen.is_none() {
        return;
    }
    let port = chosen.unwrap_or(PORT);
    let listener = match TcpListener::bind((Ipv4Addr::LOCALHOST, port)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Lunta: the browser extension endpoint (127.0.0.1:{port}) is not available: {e}");
            return;
        }
    };
    serve(listener, Arc::new(move || app_context(&app)));
}

/// What a request works with: the open library, where backups go, and how the window hears of it.
pub(crate) struct Ctx {
    pub lib: Option<Arc<Library>>,
    pub backups: PathBuf,
    /// where journal styles (CSL) are kept, for citing
    pub csl_cache: PathBuf,
    /// (event, payload) to the window: "status" (progress), "extension" (the library changed)
    pub notify: Arc<dyn Fn(&str, &str) + Send + Sync>,
    /// a paper's reading view changed: forget where it was unpacked
    pub forget_book: Arc<dyn Fn(&str) + Send + Sync>,
    /// no network lookups (tests)
    pub offline: bool,
}

fn app_context(app: &AppHandle) -> Ctx {
    let state = app.state::<AppState>();
    let (a, b) = (app.clone(), app.clone());
    Ctx {
        lib: state.lib().ok(),
        backups: app.path().app_data_dir().unwrap_or_else(|_| std::env::temp_dir().join("Lunta")).join("backups"),
        csl_cache: app.path().app_cache_dir().unwrap_or_else(|_| std::env::temp_dir().join("Lunta")).join("citations"),
        notify: Arc::new(move |event: &str, payload: &str| {
            // the extension's "Open": bring the window to the front too
            if event == "open" {
                if let Some(w) = a.get_webview_window("main") {
                    let _ = w.unminimize();
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
            let _ = a.emit(event, payload.to_string());
        }),
        forget_book: Arc::new(move |id: &str| {
            b.state::<AppState>().books.lock().unwrap().remove(id);
        }),
        offline: false,
    }
}

/// Answers requests on `listener` (each on its own thread) until the app quits.
pub(crate) fn serve(listener: TcpListener, ctx: Arc<dyn Fn() -> Ctx + Send + Sync>) {
    std::thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            let ctx = ctx.clone();
            std::thread::spawn(move || handle(&ctx(), conn));
        }
    });
}

// ---- HTTP plumbing ----

pub(crate) struct Request {
    pub method: String,
    pub path: String,
    pub query: HashMap<String, String>,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

fn handle(cx: &Ctx, mut conn: TcpStream) {
    let _ = conn.set_read_timeout(Some(Duration::from_secs(120)));
    let Some(req) = read_request(&mut conn) else { return };
    let (status, body) = route(cx, &req);
    let _ = respond(&mut conn, status, &body, req.headers.get("origin").map(String::as_str));
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Reads a request: the headers, then the full body (per Content-Length).
fn read_request(conn: &mut impl Read) -> Option<Request> {
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = vec![0u8; 1 << 16];
    let head_end = loop {
        if let Some(i) = find(&buf, b"\r\n\r\n") {
            break i;
        }
        let n = conn.read(&mut chunk).ok()?;
        if n == 0 || buf.len() > 1 << 20 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let mut req = parse_head(&String::from_utf8_lossy(&buf[..head_end]))?;
    let len: usize = req.headers.get("content-length").and_then(|l| l.trim().parse().ok()).unwrap_or(0);
    if len > MAX_REQUEST {
        return None;
    }
    let mut body = buf.split_off(head_end + 4);
    while body.len() < len {
        let n = conn.read(&mut chunk).ok()?;
        if n == 0 {
            return None;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(len);
    req.body = body;
    Some(req)
}

pub(crate) fn parse_head(head: &str) -> Option<Request> {
    let mut lines = head.split("\r\n");
    let first: Vec<&str> = lines.next()?.split(' ').collect();
    if first.len() < 2 {
        return None;
    }
    let target = first[1];
    let (path, q) = target.split_once('?').unwrap_or((target, ""));
    let dec = |s: &str| percent_encoding::percent_decode_str(&s.replace('+', " ")).decode_utf8_lossy().to_string();
    let query = q
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (dec(k), dec(v))
        })
        .collect();
    let headers = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_lowercase(), v.trim().to_string()))
        .collect();
    Some(Request { method: first[0].to_string(), path: path.to_string(), query, headers, body: vec![] })
}

fn respond(conn: &mut TcpStream, status: u16, json: &Value, origin: Option<&str>) -> std::io::Result<()> {
    let body = serde_json::to_vec(json).unwrap_or_else(|_| b"{}".to_vec());
    let mut head = format!(
        "HTTP/1.1 {status} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
        if status == 200 { "OK" } else { "Error" },
        body.len()
    );
    if let Some(o) = origin.filter(|o| o.starts_with("chrome-extension://") || o.starts_with("safari-web-extension://") || o.starts_with("moz-extension://")) {
        head += &format!("Access-Control-Allow-Origin: {o}\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\n");
        head += "Access-Control-Allow-Headers: Content-Type, X-Margins-Client\r\nAccess-Control-Allow-Private-Network: true\r\n";
    }
    head += "\r\n";
    conn.write_all(head.as_bytes())?;
    conn.write_all(&body)?;
    conn.flush()
}

// ---- routes ----

/// Whether the request may use the endpoint: (status, error) if not.
pub(crate) fn gate(r: &Request) -> Option<(u16, Value)> {
    // Web pages cannot send custom headers to another origin without a CORS preflight, which we
    // never approve for web origins; the extension sends this header. Pages with an http(s)
    // Origin are refused outright.
    let origin = r.headers.get("origin").map(String::as_str).unwrap_or("");
    let from_web_page = origin.starts_with("http://") || origin.starts_with("https://") || origin == "null";
    if r.method == "OPTIONS" {
        return Some(if from_web_page { (403, json!({})) } else { (200, json!({})) });
    }
    if from_web_page || r.headers.get("x-margins-client").map(String::as_str) != Some("extension") {
        return Some((403, json!({"error": "only the Lunta browser extension may use this endpoint"})));
    }
    None
}

fn route(cx: &Ctx, r: &Request) -> (u16, Value) {
    if let Some(refused) = gate(r) {
        return refused;
    }
    let Some(lib) = cx.lib.clone() else { return (503, json!({"error": "library not open"})) };
    if r.path.starts_with("/cite/") {
        let out = crate::citeapi::route(&lib, &cx.csl_cache.to_string_lossy(), &r.method, &r.path, &r.query, &r.body);
        if r.path == "/cite/add" && out.1.get("added").and_then(Value::as_bool) == Some(true) {
            changed(cx, "Added a paper you cited (Resolve Citations).");
        }
        return out;
    }
    let q = |k: &str| r.query.get(k).cloned().unwrap_or_default();
    match (r.method.as_str(), r.path.as_str()) {
        ("GET", "/status") => {
            let p = pagetext::find_paper(&lib, &q("doi"), &q("title")).or_else(|| if q("url").is_empty() { None } else { lib.find_by_url(q("url")) });
            (200, status(p.as_ref()))
        }
        // a web page or blog post (no paper on it): saved with the page as the browser has it
        ("POST", "/webpage") => {
            let Ok(Value::Object(j)) = serde_json::from_slice::<Value>(&r.body) else {
                return (400, json!({"error": "bad JSON"}));
            };
            let s = |k: &str| j.get(k).and_then(Value::as_str).unwrap_or("").to_string();
            if !s("url").starts_with("http") {
                return (400, json!({"error": "Expected the page's address."}));
            }
            let html = Some(s("html")).filter(|h| !h.trim().is_empty());
            match add::add_web_page(&lib, &s("url"), html, &logger(cx)) {
                Ok(a) => {
                    changed(cx, "Saved a web page from the browser.");
                    let mut out = status(lib.paper(a.paper.id.clone()).as_ref());
                    out["message"] = json!(a.note.unwrap_or_else(|| "Saved as a web page, with a reading view.".into()));
                    (200, out)
                }
                Err(e) => {
                    (cx.notify)("status", "");
                    (422, json!({"error": e}))
                }
            }
        }
        ("POST", "/save") => {
            let Ok(Value::Object(j)) = serde_json::from_slice::<Value>(&r.body) else {
                return (400, json!({"error": "bad JSON"}));
            };
            let mut p = save(cx, &lib, &j);
            // no PDF link on the page (PubMed): look for a free copy while saving
            if let Some(q) = p.as_ref().filter(|q| !q.has_pdf && j.get("findPdf").and_then(Value::as_bool) == Some(true)) {
                p = find_pdf(cx, &lib, &q.id).or(p);
            }
            if p.is_some() {
                changed(cx, "Saved from the browser.");
            }
            (if p.is_none() { 500 } else { 200 }, status(p.as_ref()))
        }
        // the extension's "Open" (no lunta:// link, which browsers ask about on every site)
        ("POST", "/open") => {
            let id = q("id");
            if lib.paper(id.clone()).is_none() {
                return (404, json!({"error": "unknown paper"}));
            }
            (cx.notify)("open", &id);
            (200, json!({"ok": true}))
        }
        ("POST", "/findpdf") => {
            let id = q("id");
            let Some(p) = lib.paper(id.clone()) else { return (404, json!({"error": "unknown paper"})) };
            let p = if p.has_pdf { Some(p) } else { find_pdf(cx, &lib, &id) };
            if p.as_ref().is_some_and(|p| p.has_pdf) {
                changed(cx, "Found a free PDF.");
            }
            (200, status(p.as_ref()))
        }
        ("POST", "/pdf") => {
            if !r.body.starts_with(b"%PDF") {
                return (400, json!({"error": "not a PDF"}));
            }
            let id = q("id");
            if id.is_empty() {
                // a PDF on its own (e.g. open in Chrome's viewer): add it as a new paper or match it by DOI
                let mut name = q("name");
                if name.is_empty() {
                    name = "paper.pdf".into();
                }
                let p = add_pdf(&lib, &r.body, &name);
                if p.is_some() {
                    changed(cx, "PDF saved from the browser.");
                }
                return (if p.is_none() { 500 } else { 200 }, status(p.as_ref()));
            }
            if lib.paper(id.clone()).is_none() {
                return (404, json!({"error": "unknown paper"}));
            }
            if let Err(e) = lib.attach_file(id.clone(), FileKind::Pdf, r.body.clone()) {
                return (500, json!({"error": e.to_string()}));
            }
            changed(cx, "PDF saved from the browser.");
            if lib.paper(id.clone()).map_or(false, |p| !p.has_epub) {
                let (notify, lib, id) = (cx.notify.clone(), lib.clone(), id.clone());
                std::thread::spawn(move || {
                    let n = notify.clone();
                    let log = add::Logger(Arc::new(move |m: &str| n("status", m)));
                    if add::make_reading_view(&lib, &id, &log).is_ok() {
                        notify("extension", "Reading view made from the PDF.");
                    }
                });
            }
            (200, status(lib.paper(id).as_ref()))
        }
        ("POST", "/fulltext") => full_text(cx, &lib, r),
        _ => (404, json!({"error": "not found"})),
    }
}

fn status(p: Option<&Paper>) -> Value {
    match p {
        None => json!({"inLibrary": false}),
        Some(p) => json!({"inLibrary": true, "id": p.id, "title": p.meta.title, "hasPdf": p.has_pdf, "hasReadingView": p.has_epub}),
    }
}

/// The library changed: the window reloads and says why.
fn changed(cx: &Ctx, message: &str) {
    (cx.notify)("extension", message);
}

fn logger(cx: &Ctx) -> add::Logger {
    let n = cx.notify.clone();
    add::Logger(Arc::new(move |m: &str| n("status", m)))
}

fn save(cx: &Ctx, lib: &Library, j: &Map<String, Value>) -> Option<Paper> {
    let s = |k: &str| j.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let (doi, title) = (s("doi"), s("title"));
    if let Some(p) = pagetext::find_paper(lib, &doi, &title) {
        return Some(p);
    }
    if !doi.is_empty() && !cx.offline {
        let _ = add::add(lib, &doi, &logger(cx));
        if let Some(p) = lib.find_by_doi(doi.clone()) {
            return Some(p);
        }
    }
    // no DOI: the PubMed or PMC id (PubMed's pages), so the paper gets its details and can find a PDF
    let (pmid, pmcid) = (s("pmid"), s("pmcid"));
    if !cx.offline {
        for input in [&pmcid, &pmid] {
            if !input.is_empty() {
                if let Ok(a) = add::add(lib, input, &logger(cx)) {
                    return Some(a.paper);
                }
            }
        }
    }
    // no DOI (or lookup failed): save what the page told us
    let meta = PaperMeta {
        title,
        authors: j
            .get("authors")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
            .unwrap_or_default(),
        journal: s("journal"),
        date: s("date"),
        doi,
        pmid,
        pmcid,
        url: s("url"),
        abstract_text: s("abstract"),
        ..Default::default()
    };
    if meta.title.is_empty() {
        return None;
    }
    lib.add_paper(meta, None, None, None).ok()
}

/// Looks for a free, legal PDF of a paper (as File › Find PDF does) and makes its reading view if it
/// has none; the paper as it is afterwards.
fn find_pdf(cx: &Ctx, lib: &Library, id: &str) -> Option<Paper> {
    if !cx.offline {
        if let Ok(Some(_)) = lib.find_pdf(id.to_string()) {
            if lib.paper(id.to_string()).is_some_and(|p| !p.has_epub) {
                let _ = add::make_reading_view(lib, id, &logger(cx));
            }
            (cx.forget_book)(id);
        }
    }
    lib.paper(id.to_string())
}

/// A PDF on its own: matched to a paper by its DOI, else added (the converter reads it).
fn add_pdf(lib: &Library, data: &[u8], name: &str) -> Option<Paper> {
    let dir = std::env::temp_dir().join(format!("lunta-pdf-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).ok()?;
    let mut file_name: String = name.chars().filter(|c| !matches!(c, '/' | '\\' | ':' | '\0')).collect();
    if file_name.trim().is_empty() {
        file_name = "paper.pdf".into();
    }
    if !file_name.to_lowercase().ends_with(".pdf") {
        file_name += ".pdf";
    }
    let path = dir.join(file_name);
    let r = std::fs::write(&path, data).ok().and_then(|_| lib.add_from_pdf(path.to_string_lossy().to_string(), None).ok());
    let _ = std::fs::remove_dir_all(&dir);
    r.map(|a| a.paper)
}

fn pages_in_progress() -> &'static Mutex<HashSet<String>> {
    static P: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    P.get_or_init(|| Mutex::new(HashSet::new()))
}

/// The JSON the extension posts to /fulltext. None if it has no page.
pub(crate) fn submission(body: &[u8]) -> Option<(PageSubmission, bool)> {
    let Ok(Value::Object(j)) = serde_json::from_slice::<Value>(body) else { return None };
    let s = |k: &str| j.get(k).and_then(Value::as_str).map(String::from);
    let html = s("html").filter(|h| !h.is_empty()).or_else(|| s("liveHtml").filter(|h| !h.is_empty()))?;
    let mut images = Vec::new();
    for i in j.get("images").and_then(Value::as_array).into_iter().flatten() {
        let Some(data) = i.get("base64").and_then(Value::as_str).and_then(base64_decode).filter(|d| !d.is_empty()) else {
            continue;
        };
        let mut urls: Vec<String> =
            i.get("urls").and_then(Value::as_array).into_iter().flatten().filter_map(|u| u.as_str().map(String::from)).collect();
        if let Some(u) = i.get("url").and_then(Value::as_str) {
            urls.push(u.to_string());
        }
        images.push(PageImage { urls, data });
    }
    let sub = PageSubmission {
        paper_id: None,
        html,
        live_html: s("liveHtml"),
        url: s("url").unwrap_or_default(),
        doi: s("doi").unwrap_or_default(),
        title: s("title").unwrap_or_default(),
        images,
        images_skipped: j.get("imagesSkipped").and_then(Value::as_u64).unwrap_or(0) as u32,
    };
    Some((sub, j.get("dryRun").and_then(Value::as_bool).unwrap_or(false)))
}

fn full_text(cx: &Ctx, lib: &Library, r: &Request) -> (u16, Value) {
    let Some((sub, dry_in_body)) = submission(&r.body) else {
        return (400, json!({"error": "Expected JSON with the page's html."}));
    };
    let dry = dry_in_body || r.query.get("dryRun").map(String::as_str) == Some("1");
    let key = if sub.url.is_empty() { sub.doi.clone() } else { sub.url.clone() };
    if !pages_in_progress().lock().unwrap().insert(key.clone()) {
        return (409, json!({"error": "Lunta is already working on this page."}));
    }
    let host = url_host(&sub.url);
    (cx.notify)("status", &format!("Making a reading view from {host}…"));
    let log = logger(cx);
    let opts = Options { log: Some(Box::new(move |m: &str| log.say(m.trim()))), offline: cx.offline, doi: None };
    let result = pagetext::use_page_full_text(lib, &sub, &cx.backups, dry, &opts);
    pages_in_progress().lock().unwrap().remove(&key);
    match result {
        Ok(o) => {
            let mut j = match status(o.paper_id.as_ref().and_then(|id| lib.paper(id.clone())).as_ref()) {
                Value::Object(m) => m,
                _ => Map::new(),
            };
            j.extend(o.to_json());
            if !dry {
                if let Some(id) = &o.paper_id {
                    (cx.forget_book)(id);
                }
                changed(cx, &o.message);
            }
            (200, Value::Object(j))
        }
        Err(e) => {
            (cx.notify)("status", "");
            (422, json!({"error": e}))
        }
    }
}

fn url_host(u: &str) -> String {
    let rest = u.split("://").nth(1).unwrap_or("");
    let h = rest.split(['/', '?', '#']).next().unwrap_or("");
    if h.is_empty() {
        "the page".into()
    } else {
        h.to_string()
    }
}

/// Standard base64 (what the extension's `btoa` gives); None if malformed.
pub(crate) fn base64_decode(s: &str) -> Option<Vec<u8>> {
    fn val(c: u8) -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        } as u32)
    }
    let bytes: Vec<u8> = s.bytes().filter(|c| !c.is_ascii_whitespace() && *c != b'=').collect();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        let mut n = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            n |= val(c)? << (18 - 6 * i);
        }
        match chunk.len() {
            4 => out.extend_from_slice(&[(n >> 16) as u8, (n >> 8) as u8, n as u8]),
            3 => out.extend_from_slice(&[(n >> 16) as u8, (n >> 8) as u8]),
            2 => out.push((n >> 16) as u8),
            _ => return None,
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(head: &str) -> Request {
        parse_head(head).unwrap()
    }

    #[test]
    fn only_the_extension_may_use_it() {
        let ok = req("GET /status?doi=10.1%2Fx&title=A+title HTTP/1.1\r\nOrigin: chrome-extension://abc\r\nX-Margins-Client: extension");
        assert_eq!(ok.query["doi"], "10.1/x");
        assert_eq!(ok.query["title"], "A title");
        assert!(gate(&ok).is_none());
        let page = req("GET /status HTTP/1.1\r\nOrigin: https://evil.example\r\nX-Margins-Client: extension");
        assert_eq!(gate(&page).unwrap().0, 403);
        let no_header = req("POST /save HTTP/1.1\r\nContent-Type: application/json");
        assert_eq!(gate(&no_header).unwrap().0, 403);
        assert_eq!(gate(&req("OPTIONS /fulltext HTTP/1.1\r\nOrigin: chrome-extension://abc")).unwrap().0, 200);
        assert_eq!(gate(&req("OPTIONS /fulltext HTTP/1.1\r\nOrigin: null")).unwrap().0, 403);
    }

    #[test]
    fn reads_a_full_text_submission() {
        let body = json!({"url": "https://www.cell.com/x", "html": "<html></html>", "doi": "10.1/x",
            "images": [{"urls": ["https://a/gr1.jpg"], "base64": "iVBORw0KGgo="}, {"urls": [], "base64": "@@"}],
            "imagesSkipped": 2, "dryRun": true});
        let (s, dry) = submission(&serde_json::to_vec(&body).unwrap()).unwrap();
        assert!(dry);
        assert_eq!((s.url.as_str(), s.doi.as_str(), s.images_skipped), ("https://www.cell.com/x", "10.1/x", 2));
        assert_eq!(s.images.len(), 1);
        assert_eq!(s.images[0].data, vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
        assert!(submission(b"{\"url\":\"x\"}").is_none());
        assert_eq!(base64_decode("aGVsbG8gd29ybGQ=").unwrap(), b"hello world");
    }

    /// The endpoint over HTTP with a library in a temporary folder: "Use this page's full text"
    /// adds the paper, /status finds it, a web page is refused.
    #[test]
    fn serves_the_extension_over_http() {
        let dir = tempfile_dir();
        let lib = Library::open(
            dir.join("lib").to_string_lossy().into(),
            dir.join("cache").to_string_lossy().into(),
            "test".into(),
        )
        .unwrap();
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let events: Arc<Mutex<Vec<String>>> = Arc::default();
        let (l2, ev, backups) = (lib.clone(), events.clone(), dir.join("backups"));
        serve(
            listener,
            Arc::new(move || {
                let ev = ev.clone();
                Ctx {
                    lib: Some(l2.clone()),
                    backups: backups.clone(),
                    csl_cache: backups.join("csl"),
                    notify: Arc::new(move |e: &str, p: &str| ev.lock().unwrap().push(format!("{e}: {p}"))),
                    forget_book: Arc::new(|_: &str| {}),
                    offline: true,
                }
            }),
        );
        let call = |method: &str, path: &str, origin: &str, body: &[u8]| -> (u16, Value) {
            let mut c = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
            let head = format!(
                "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nOrigin: {origin}\r\nX-Margins-Client: extension\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
                body.len()
            );
            c.write_all(head.as_bytes()).unwrap();
            c.write_all(body).unwrap();
            let mut out = Vec::new();
            c.read_to_end(&mut out).unwrap();
            let text = String::from_utf8_lossy(&out).to_string();
            let status: u16 = text[9..12].parse().unwrap();
            let (h, b) = text.split_once("\r\n\r\n").unwrap();
            if origin.starts_with("chrome-extension://") {
                assert!(h.contains(&format!("Access-Control-Allow-Origin: {origin}")), "{h}");
            }
            (status, serde_json::from_str(b).unwrap())
        };
        let filler = (0..600).map(|i| format!("word{}", i % 97)).collect::<Vec<_>>().join(" ");
        let html = include_str!("../../../core/src/convert/testdata/page-atypon-cell.html").replace("{{FILLER}}", &filler);
        let ext = "chrome-extension://abcdefgh";
        let body = json!({"url": "https://www.cell.com/cell/fulltext/S0092-8674(26)00001-1", "html": html, "images": []});
        let (st, j) = call("POST", "/fulltext", ext, &serde_json::to_vec(&body).unwrap());
        assert_eq!(st, 200, "{j}");
        assert_eq!(j["outcome"], "added");
        assert_eq!(j["reader"], "Cell Press / Lancet");
        assert_eq!(j["inLibrary"], true);
        assert_eq!(j["hasReadingView"], true);
        let (st, j) = call("GET", "/status?doi=10.1016%2Fj.cell.2026.01.001", ext, b"");
        assert_eq!((st, j["inLibrary"].clone(), j["hasReadingView"].clone()), (200, json!(true), json!(true)));
        // the same page again, as a dry run: nothing changes
        let (st, j) = call("POST", "/fulltext?dryRun=1", ext, &serde_json::to_vec(&body).unwrap());
        assert_eq!(st, 200, "{j}");
        assert_eq!(j["dryRun"], true);
        // a page that isn't the full text
        let (st, j) = call("POST", "/fulltext", ext, br#"{"url":"https://x.example/a","html":"<p>Sign in</p>"}"#);
        assert_eq!(st, 422);
        assert!(j["error"].as_str().unwrap().contains("full text"), "{j}");
        // a web page may not use it
        assert_eq!(call("GET", "/status?doi=x", "https://evil.example", b"").0, 403);
        // citing in Google Docs (citeapi.rs): the extension only
        let (st, j) = call("GET", "/cite/status", ext, b"");
        assert_eq!((st, j["papers"].clone()), (200, json!(1)));
        assert_eq!(call("POST", "/cite/bibliography", "https://docs.google.com", br#"{"style":"apa","ids":[]}"#).0, 403);
        // saving by title (no DOI, no network)
        let (st, j) = call("POST", "/save", ext, br#"{"title":"A paper saved from its page","authors":["Ada Lovelace"]}"#);
        assert_eq!((st, j["inLibrary"].clone()), (200, json!(true)));
        assert!(events.lock().unwrap().iter().any(|e| e.starts_with("extension: Added to Lunta")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn tempfile_dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!("lunta-ext-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn reads_a_request_with_its_body() {
        let raw = b"POST /fulltext?dryRun=1 HTTP/1.1\r\nContent-Length: 5\r\nX-Margins-Client: extension\r\n\r\nhello";
        let r = read_request(&mut &raw[..]).unwrap();
        assert_eq!((r.method.as_str(), r.path.as_str(), r.body.as_slice()), ("POST", "/fulltext", &b"hello"[..]));
        assert_eq!(r.query["dryRun"], "1");
        // a body cut short: no request
        assert!(read_request(&mut &b"POST / HTTP/1.1\r\nContent-Length: 9\r\n\r\nabc"[..]).is_none());
    }
}
