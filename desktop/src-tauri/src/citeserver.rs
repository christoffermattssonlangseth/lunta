//! Citing in Word on Windows and Linux: the HTTPS server on https://localhost:47481 that Lunta for
//! Mac runs (`apple/Mac/CitationServer.swift` around `CitationAPI.swift`), so the same Word add-in
//! (`apple/citations/word`, both manifests) works with this app.
//!
//! It serves the add-in's pages (`apple/citations`, built into the app, at the same paths as the
//! Mac: `/word/index.html`, `/common.js`, `/icons/icon-32.png`…) and the citation API at `/api/…`,
//! answered by `citeapi.rs` (the browser extension's `/cite/…`, the same routes). TLS with rustls
//! and the certificates in `citecerts.rs` (renewed while the app runs).
//!
//! Who may ask, as on the Mac: pages from this server, the hosted Word panel (the store add-in),
//! and the Google Docs sidebar (served from *.googleusercontent.com, so it must also present the
//! pairing key). Requests a web page makes from anywhere else are refused, also the ones a browser
//! sends without an Origin (Sec-Fetch-Site), and so is any other Host (DNS rebinding).

use crate::citecerts::{self, Outcome};
use crate::AppState;
use lunta_core::Library;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

pub const PORT: u16 = 47481;
/// Where the hosted Word panel (manifest-store.xml) lives: getlunta.app, and before the move the
/// GitHub Pages copy (CitationAPI.hostedPanelOrigin on the Mac).
pub const HOSTED_PANEL_ORIGINS: [&str; 2] = ["https://getlunta.app", "https://christoffermattssonlangseth.github.io"];
const MAX_HEADER: usize = 64 << 10;
const MAX_BODY: usize = 8 << 20;

// ---- who may ask ----

/// The access rules (CitationAPI.refusal / corsHeaders on the Mac). `port`: the one served (the
/// tests use another).
pub(crate) struct Access {
    pub port: u16,
    /// the Google Docs sidebar's pairing key
    pub token: String,
}

pub(crate) struct Response {
    pub status: u16,
    pub body: Vec<u8>,
    pub content_type: &'static str,
    pub headers: Vec<(String, String)>,
}

impl Response {
    fn json(status: u16, v: &Value) -> Response {
        Response { status, body: serde_json::to_vec(v).unwrap_or_else(|_| b"{}".to_vec()), content_type: "application/json", headers: vec![] }
    }
    fn error(status: u16, message: &str) -> Response {
        Self::json(status, &json!({"error": message}))
    }
    /// The response as bytes on the wire (the connection is closed after it).
    pub fn encoded(&self) -> Vec<u8> {
        let reason = match self.status {
            200 => "OK",
            204 => "No Content",
            400 => "Bad Request",
            403 => "Forbidden",
            404 => "Not Found",
            413 => "Payload Too Large",
            421 => "Misdirected Request",
            502 => "Bad Gateway",
            503 => "Service Unavailable",
            _ => "Error",
        };
        let mut h = format!("HTTP/1.1 {} {reason}\r\nContent-Type: {}\r\nContent-Length: {}\r\n", self.status, self.content_type, self.body.len());
        h += "Connection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n";
        for (k, v) in &self.headers {
            h += &format!("{k}: {v}\r\n");
        }
        h += "\r\n";
        let mut out = h.into_bytes();
        out.extend_from_slice(&self.body);
        out
    }
}

pub(crate) fn is_google_sidebar(origin: &str) -> bool {
    let Some(rest) = origin.strip_prefix("https://") else { return false };
    // the origin is scheme://host[:port]; nothing after the host
    let host = rest.split(':').next().unwrap_or("").to_lowercase();
    !rest.contains('/') && host.ends_with(".googleusercontent.com") && host.len() > ".googleusercontent.com".len()
}

impl Access {
    fn own_origins(&self) -> [String; 2] {
        [format!("https://localhost:{}", self.port), format!("https://127.0.0.1:{}", self.port)]
    }

    fn is_own(&self, origin: &str) -> bool {
        self.own_origins().iter().any(|o| o == origin)
    }

    /// Origins that are answered (CORS) at all; the Google sidebar still needs the key.
    pub fn is_known_origin(&self, origin: &str) -> bool {
        self.is_own(origin) || HOSTED_PANEL_ORIGINS.contains(&origin) || is_google_sidebar(origin)
    }

    pub fn cors_headers(&self, r: &Request) -> Vec<(String, String)> {
        let Some(origin) = r.headers.get("origin") else { return vec![] };
        if !self.is_known_origin(origin) || self.is_own(origin) {
            return vec![];
        }
        [
            ("Access-Control-Allow-Origin", origin.as_str()),
            ("Vary", "Origin"),
            ("Access-Control-Allow-Methods", "GET, POST, OPTIONS"),
            ("Access-Control-Allow-Headers", "Content-Type, X-Margins-Token, X-Lunta-Token"),
            ("Access-Control-Allow-Private-Network", "true"),
            ("Access-Control-Max-Age", "600"),
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
    }

    /// None when the request may go on; otherwise the refusal to send.
    pub fn refusal(&self, r: &Request) -> Option<Response> {
        // a page on another name that resolves to this computer (DNS rebinding) never gets here
        // with a valid certificate, but don't rely on that alone
        if let Some(host) = r.headers.get("host") {
            let host = host.to_lowercase();
            let ok = [format!("localhost:{}", self.port), format!("127.0.0.1:{}", self.port), "localhost".into(), "127.0.0.1".into()];
            if !ok.contains(&host) {
                return Some(Response::error(421, "Wrong host."));
            }
        }
        if !r.path.starts_with("/api/") {
            return None; // the add-in files are public
        }
        if r.method == "OPTIONS" {
            return None;
        }
        if let Some(origin) = r.headers.get("origin") {
            if self.is_own(origin) || HOSTED_PANEL_ORIGINS.contains(&origin.as_str()) {
                return None;
            }
            if is_google_sidebar(origin) {
                let sent = r.headers.get("x-lunta-token").or_else(|| r.headers.get("x-margins-token")).map(String::as_str).unwrap_or("");
                if !self.token.is_empty() && same(sent, &self.token) {
                    return None;
                }
                return Some(Response::error(403, if sent.is_empty() {
                    "Not paired. Enter the pairing key shown in Lunta’s Citations in Word & Google Docs…"
                } else {
                    "Wrong pairing key. Enter the key shown in Lunta’s Citations in Word & Google Docs… (each computer has its own)."
                }));
            }
            return Some(Response::error(403, "This page may not use your Lunta library."));
        }
        // no Origin: the add-in's own GETs, or another app on this computer (curl); a browser
        // asking for another site (<script src>, no-cors fetch) says so in Sec-Fetch-Site
        if let Some(site) = r.headers.get("sec-fetch-site") {
            if site != "same-origin" && site != "none" {
                return Some(Response::error(403, "This page may not use your Lunta library."));
            }
        }
        None
    }
}

/// Compares without stopping at the first difference.
fn same(a: &str, b: &str) -> bool {
    let (x, y) = (a.as_bytes(), b.as_bytes());
    if x.len() != y.len() {
        return false;
    }
    x.iter().zip(y).fold(0u8, |d, (p, q)| d | (p ^ q)) == 0
}

// ---- requests ----

pub(crate) use crate::extension::Request;

pub(crate) enum Parsed {
    /// More bytes are needed.
    Incomplete,
    /// Not HTTP, or too large: answer 400 / 413 and close.
    Bad(u16),
    Request(Request),
}

/// One request off the connection (CitationHTTP.parse on the Mac).
pub(crate) fn parse(d: &[u8]) -> Parsed {
    let Some(end) = d.windows(4).position(|w| w == b"\r\n\r\n") else {
        return if d.len() > MAX_HEADER { Parsed::Bad(400) } else { Parsed::Incomplete };
    };
    let Some(mut r) = crate::extension::parse_head(&String::from_utf8_lossy(&d[..end])) else { return Parsed::Bad(400) };
    if !r.path.starts_with('/') {
        return Parsed::Bad(400);
    }
    r.method = r.method.to_uppercase();
    if r.headers.contains_key("transfer-encoding") {
        return Parsed::Bad(400); // fetch() sends Content-Length
    }
    let Ok(len) = r.headers.get("content-length").map(|s| s.trim()).unwrap_or("0").parse::<i64>() else { return Parsed::Bad(400) };
    if len < 0 {
        return Parsed::Bad(400);
    }
    if len as usize > MAX_BODY {
        return Parsed::Bad(413);
    }
    let start = end + 4;
    if d.len() - start < len as usize {
        return Parsed::Incomplete;
    }
    r.body = d[start..start + len as usize].to_vec();
    Parsed::Request(r)
}

// ---- answers ----

/// What a request works with: the open library, where journal styles are kept, the key, and what
/// happens when Resolve Citations added a paper.
pub(crate) struct Ctx {
    pub lib: Option<Arc<Library>>,
    pub csl_cache: PathBuf,
    pub access: Access,
    pub added: Arc<dyn Fn(&str) + Send + Sync>,
}

/// The whole answer: the add-in's files, or an /api/ request (refusal, preflight or the API, with
/// CORS headers).
pub(crate) fn respond(cx: &Ctx, r: &Request) -> Response {
    if !r.path.starts_with("/api/") {
        if let Some(no) = cx.access.refusal(r) {
            return no;
        }
        return serve_file(&r.path);
    }
    let mut out = if let Some(no) = cx.access.refusal(r) {
        no
    } else if r.method == "OPTIONS" {
        Response { status: 204, body: vec![], content_type: "text/plain", headers: vec![] }
    } else {
        note_connection(r);
        handle(cx, r)
    };
    out.headers.extend(cx.access.cors_headers(r));
    out
}

fn handle(cx: &Ctx, r: &Request) -> Response {
    let Some(lib) = &cx.lib else { return Response::error(503, "Lunta's library isn't open yet. Try again in a moment.") };
    // /api/… is the extension's /cite/… (citeapi.rs, the same routes as CitationAPI.swift)
    let path = format!("/cite/{}", &r.path["/api/".len()..]);
    let (status, body) = crate::citeapi::route(lib, &cx.csl_cache.to_string_lossy(), &r.method, &path, &r.query, &r.body);
    if path == "/cite/add" && body.get("added").and_then(Value::as_bool) == Some(true) {
        if let Some(id) = body.get("id").and_then(Value::as_str) {
            (cx.added)(id);
        }
    }
    Response::json(status, &body)
}

// ---- the add-in's files (apple/citations, as the Mac app's Resources/citations) ----

macro_rules! files {
    ($($p:literal),* $(,)?) => { &[$(($p, include_bytes!(concat!("../../../apple/citations/", $p)) as &[u8])),*] };
}

/// Every file of `apple/citations`, at the path the Mac serves it (a test checks none is missing).
pub(crate) const FILES: &[(&str, &[u8])] = files![
    "common.js",
    "panel.css",
    "resolve.js",
    "word/index.html",
    "word/app.js",
    "word/fields.js",
    "word/manifest.xml",
    "word/manifest-store.xml",
    "gdocs/Code.gs",
    "gdocs/Sidebar.html",
    "icons/icon-16.png",
    "icons/icon-32.png",
    "icons/icon-64.png",
    "icons/icon-80.png",
    "icons/icon-128.png",
];

pub(crate) fn file(rel: &str) -> Option<&'static [u8]> {
    FILES.iter().find(|(p, _)| *p == rel).map(|(_, d)| *d)
}

fn serve_file(path: &str) -> Response {
    let mut rel = percent_encoding::percent_decode_str(path).decode_utf8_lossy().trim_start_matches('/').to_string();
    if rel.is_empty() || rel.ends_with('/') {
        rel += "index.html";
    }
    let Some(data) = file(&rel) else { return Response::error(404, "Not found.") };
    let content_type = match rel.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "png" => "image/png",
        "xml" => "application/xml",
        "json" => "application/json",
        _ => "application/octet-stream",
    };
    Response { status: 200, body: data.to_vec(), content_type, headers: vec![] }
}

// ---- state for the Citations window ----

#[derive(Default)]
struct Health {
    /// why the add-ins can't connect (None when serving)
    problem: Option<String>,
    serving: bool,
    /// the last time an add-in asked (Word's panel, the hosted panel, the Google sidebar)
    last_request: Option<Instant>,
}

fn health() -> &'static Mutex<Health> {
    static H: OnceLock<Mutex<Health>> = OnceLock::new();
    H.get_or_init(|| Mutex::new(Health::default()))
}

fn note_connection(r: &Request) {
    if r.headers.contains_key("origin") || r.headers.get("sec-fetch-site").is_some_and(|s| s == "same-origin") {
        health().lock().unwrap().last_request = Some(Instant::now());
    }
}

fn set_problem(p: Option<String>) {
    if let Some(p) = &p {
        eprintln!("Lunta: {p}");
    }
    let mut h = health().lock().unwrap();
    h.serving = p.is_none();
    h.problem = p;
}

/// (serving, problem, seconds since an add-in last asked)
pub(crate) fn status() -> (bool, Option<String>, Option<u64>) {
    let h = health().lock().unwrap();
    (h.serving, h.problem.clone(), h.last_request.map(|t| t.elapsed().as_secs()))
}

// ---- the server ----

/// The TLS configuration in use; swapped when the certificate is renewed.
fn tls() -> &'static RwLock<Option<Arc<rustls::ServerConfig>>> {
    static T: OnceLock<RwLock<Option<Arc<rustls::ServerConfig>>>> = OnceLock::new();
    T.get_or_init(|| RwLock::new(None))
}

/// Where the certificates are kept.
pub(crate) fn tls_dir(app: &AppHandle) -> PathBuf {
    app.path().app_data_dir().unwrap_or_else(|_| std::env::temp_dir().join("Lunta")).join("citations").join("tls")
}

/// Makes or renews the certificates and loads them; the outcome.
pub(crate) fn refresh_certificates(dir: &std::path::Path) -> Outcome {
    let out = citecerts::ensure(dir, time::OffsetDateTime::now_utc());
    match citecerts::server_config(dir) {
        Ok(c) => *tls().write().unwrap() = Some(c),
        Err(e) => set_problem(Some(format!("The add-ins’ certificate couldn’t be read ({e})."))),
    }
    out
}

/// Starts the server in the background. On a Mac (where this app runs only for testing) the port
/// is Lunta for Mac's; a debug build may be given another (LUNTA_CITATION_PORT).
pub fn start(app: AppHandle) {
    let chosen: Option<u16> = if cfg!(debug_assertions) { std::env::var("LUNTA_CITATION_PORT").ok().and_then(|p| p.parse().ok()) } else { None };
    if cfg!(target_os = "macos") && chosen.is_none() {
        set_problem(Some("On a Mac, Lunta for Mac serves the Word add-in.".into()));
        return;
    }
    let port = chosen.unwrap_or(PORT);
    let dir = tls_dir(&app);
    std::thread::spawn(move || {
        if let Outcome::Failed(e) = refresh_certificates(&dir) {
            set_problem(Some(format!("Couldn’t make the certificate for the add-ins ({e}).")));
        }
        // the certificate is renewed a month before it runs out; the app may run that long
        let renew_dir = dir.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(24 * 3600));
            if let Outcome::Failed(e) = refresh_certificates(&renew_dir) {
                eprintln!("Lunta: renewing the citation certificate failed: {e}");
            }
        });
        // the port taken (another copy of Lunta?): try again for a while
        let mut tries = 0u64;
        let listener = loop {
            match TcpListener::bind((Ipv4Addr::LOCALHOST, port)) {
                Ok(l) => break l,
                Err(e) => {
                    set_problem(Some(format!("The add-ins can’t connect: port {port} is unavailable ({e}). Is another copy of Lunta running?")));
                    tries += 1;
                    if tries > 20 {
                        return;
                    }
                    std::thread::sleep(Duration::from_secs((tries * 3).min(60)));
                }
            }
        };
        if tls().read().unwrap().is_some() {
            set_problem(None);
        }
        let a = app.clone();
        serve(listener, Arc::new(move || app_context(&a, port)));
    });
}

/// The Google Docs sidebar's pairing key (made once, kept in the settings).
pub(crate) fn token(app: &AppHandle) -> String {
    let state = app.state::<AppState>();
    let mut s = state.settings.lock().unwrap();
    if s.citation_token.is_empty() {
        s.citation_token = uuid::Uuid::new_v4().simple().to_string()[..12].to_string();
        let _ = crate::settings::save(&state.config_dir, &s);
    }
    s.citation_token.clone()
}

fn app_context(app: &AppHandle, port: u16) -> Ctx {
    let state = app.state::<AppState>();
    let a = app.clone();
    Ctx {
        lib: state.lib().ok(),
        csl_cache: app.path().app_cache_dir().unwrap_or_else(|_| std::env::temp_dir().join("Lunta")).join("citations"),
        access: Access { port, token: token(app) },
        // Resolve Citations in Word added a paper: the window shows it (and looks for its PDF)
        added: Arc::new(move |id: &str| {
            let _ = a.emit("extension", "Added a paper you cited (Resolve Citations in Word).");
            let _ = a.emit("cited-paper-added", id.to_string());
        }),
    }
}

/// Answers on `listener`, each connection on its own thread, with the TLS configuration in use.
pub(crate) fn serve(listener: TcpListener, ctx: Arc<dyn Fn() -> Ctx + Send + Sync>) {
    for conn in listener.incoming().flatten() {
        let Some(config) = tls().read().unwrap().clone() else { continue };
        let ctx = ctx.clone();
        std::thread::spawn(move || connection(config, conn, &*ctx));
    }
}

pub(crate) fn connection(config: Arc<rustls::ServerConfig>, tcp: TcpStream, ctx: &dyn Fn() -> Ctx) {
    // a connection that never sends a whole request is closed after a while
    let _ = tcp.set_read_timeout(Some(Duration::from_secs(30)));
    let _ = tcp.set_write_timeout(Some(Duration::from_secs(30)));
    let Ok(conn) = rustls::ServerConnection::new(config) else { return };
    let mut s = rustls::StreamOwned::new(conn, tcp);
    let mut buf = Vec::new();
    let mut chunk = vec![0u8; 1 << 16];
    let out = loop {
        match parse(&buf) {
            Parsed::Request(r) => break respond(&ctx(), &r),
            Parsed::Bad(st) => break Response::error(st, if st == 413 { "Too large." } else { "Bad request." }),
            Parsed::Incomplete => match s.read(&mut chunk) {
                Ok(0) | Err(_) => return, // closed, timed out, or the handshake failed (an untrusted certificate)
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
            },
        }
    };
    let _ = s.write_all(&out.encoded());
    let _ = s.flush();
    s.conn.send_close_notify();
    let _ = s.flush();
}

#[cfg(test)]
pub(crate) fn set_tls_for_tests(c: Arc<rustls::ServerConfig>) {
    *tls().write().unwrap() = Some(c);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn request(method: &str, path: &str, headers: &[(&str, &str)], body: &str) -> Request {
        let (p, q) = path.split_once('?').unwrap_or((path, ""));
        let query: HashMap<String, String> = q.split('&').filter(|x| !x.is_empty()).map(|kv| {
            let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
            (k.to_string(), v.to_string())
        }).collect();
        let mut h: HashMap<String, String> = HashMap::from([("host".to_string(), "localhost:47481".to_string())]);
        for (k, v) in headers {
            h.insert(k.to_lowercase(), v.to_string());
        }
        Request { method: method.into(), path: p.into(), query, headers: h, body: body.as_bytes().to_vec() }
    }

    fn lib() -> (Arc<Library>, PathBuf) {
        let d = std::env::temp_dir().join(format!("lunta-citeserver-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        let lib = Library::open(d.join("lib").to_string_lossy().into(), d.join("cache").to_string_lossy().into(), "test".into()).unwrap();
        (lib, d)
    }

    fn ctx(lib: Option<Arc<Library>>, dir: &std::path::Path) -> Ctx {
        Ctx { lib, csl_cache: dir.join("csl"), access: Access { port: PORT, token: "secret123456".into() }, added: Arc::new(|_| {}) }
    }

    fn send(cx: &Ctx, r: Request) -> (u16, Value, HashMap<String, String>) {
        let out = respond(cx, &r);
        (out.status, serde_json::from_slice(&out.body).unwrap_or(Value::Null), out.headers.into_iter().collect())
    }

    /// CitationAPITests.testOnlyTheAddInsMayAsk, rule for rule.
    #[test]
    fn only_the_add_ins_may_ask() {
        let (lib, dir) = lib();
        let cx = ctx(Some(lib), &dir);
        // the Word panel served by Lunta: no Origin on GET, its own on POST
        assert_eq!(send(&cx, request("GET", "/api/status", &[("Sec-Fetch-Site", "same-origin")], "")).0, 200);
        assert_eq!(send(&cx, request("GET", "/api/status", &[("Origin", "https://localhost:47481")], "")).0, 200);
        assert_eq!(send(&cx, request("GET", "/api/status", &[("Origin", "https://127.0.0.1:47481")], "")).0, 200);
        // the hosted Word panel, at getlunta.app and at its earlier address
        for hosted in HOSTED_PANEL_ORIGINS {
            let r = send(&cx, request("GET", "/api/status", &[("Origin", hosted)], ""));
            assert_eq!(r.0, 200, "{hosted}");
            assert_eq!(r.2.get("Access-Control-Allow-Origin").map(String::as_str), Some(hosted));
            assert_eq!(r.2.get("Access-Control-Allow-Private-Network").map(String::as_str), Some("true"));
        }
        // the Google Docs sidebar: only with the pairing key
        let g = "https://n-abc123-0lu-script.googleusercontent.com";
        let no_key = send(&cx, request("GET", "/api/search?q=x", &[("Origin", g)], ""));
        assert_eq!(no_key.0, 403);
        assert_eq!(no_key.2.get("Access-Control-Allow-Origin").map(String::as_str), Some(g), "the sidebar must be able to read why");
        assert!(no_key.1["error"].as_str().unwrap().contains("pairing key"));
        assert_eq!(send(&cx, request("GET", "/api/search?q=x", &[("Origin", g), ("X-Margins-Token", "wrong")], "")).0, 403);
        assert_eq!(send(&cx, request("GET", "/api/search?q=x", &[("Origin", g), ("X-Margins-Token", "secret123456")], "")).0, 200);
        assert_eq!(send(&cx, request("GET", "/api/search?q=x", &[("Origin", g), ("X-Lunta-Token", "secret123456")], "")).0, 200);
        // any other web page: refused, and no CORS headers to read the answer with
        for origin in ["https://evil.example", "null", "https://googleusercontent.com.evil.example", "http://n-1.googleusercontent.com",
                       "https://localhost:47481.evil.example", "https://christoffermattssonlangseth.github.io.evil.example",
                       "https://getlunta.app.evil.example", "http://getlunta.app", "https://.googleusercontent.com", "http://localhost:47481"] {
            let r = send(&cx, request("POST", "/api/bibliography", &[("Origin", origin)], r#"{"style":"apa","ids":[]}"#));
            assert_eq!(r.0, 403, "{origin}");
            assert!(r.2.get("Access-Control-Allow-Origin").is_none(), "{origin}");
        }
        // a page elsewhere using <script src> or a no-cors fetch (no Origin header)
        assert_eq!(send(&cx, request("GET", "/api/search?q=a", &[("Sec-Fetch-Site", "cross-site")], "")).0, 403);
        assert_eq!(send(&cx, request("GET", "/api/search?q=a", &[("Sec-Fetch-Site", "same-site")], "")).0, 403);
        // another name for this computer (DNS rebinding), also for the add-in's files
        let rebound = request("GET", "/api/status", &[("Host", "evil.example:47481")], "");
        assert_eq!(send(&cx, rebound).0, 421);
        assert_eq!(send(&cx, request("GET", "/word/index.html", &[("Host", "evil.example:47481")], "")).0, 421);
        assert_eq!(send(&cx, request("GET", "/api/status", &[], "")).0, 200, "curl and other local tools");
        // preflight: allowed origins only
        let pre = send(&cx, request("OPTIONS", "/api/bibliography", &[("Origin", g)], ""));
        assert_eq!(pre.0, 204);
        assert!(pre.2["Access-Control-Allow-Headers"].contains("X-Margins-Token"));
        assert_eq!(pre.2["Access-Control-Allow-Private-Network"], "true");
        assert!(send(&cx, request("OPTIONS", "/api/bibliography", &[("Origin", "https://evil.example")], "")).2.get("Access-Control-Allow-Origin").is_none());
        // the add-in's own pages get no CORS headers (same origin)
        assert!(send(&cx, request("GET", "/api/status", &[("Origin", "https://localhost:47481")], "")).2.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_library_yet() {
        let dir = std::env::temp_dir();
        assert_eq!(send(&ctx(None, &dir), request("GET", "/api/status", &[], "")).0, 503);
    }

    #[test]
    fn the_api_is_citeapi() {
        let (lib, dir) = lib();
        let meta = lunta_core::model::PaperMeta { title: "Alpha cells in the islet".into(), authors: vec!["Ada Lee".into()], date: "2020".into(),
                                                  doi: "10.1234/alpha".into(), ..Default::default() };
        let p = lib.add_paper(meta, None, None, None).unwrap();
        let added = Arc::new(Mutex::new(Vec::<String>::new()));
        let seen = added.clone();
        let mut cx = ctx(Some(lib), &dir);
        cx.added = Arc::new(move |id: &str| seen.lock().unwrap().push(id.to_string()));
        let (st, j, _) = send(&cx, request("GET", "/api/status", &[], ""));
        assert_eq!((st, j["papers"].clone(), j["api"].clone()), (200, json!(1), json!(crate::citeapi::VERSION)));
        let (st, j, _) = send(&cx, request("GET", "/api/search?q=10.1234/alpha", &[], ""));
        assert_eq!((st, j[0]["id"].clone()), (200, json!(p.id)));
        let (st, j, _) = send(&cx, request("POST", "/api/bibliography", &[("Origin", "https://localhost:47481")], &format!(r#"{{"style":"apa","ids":["{}"]}}"#, p.id)));
        assert_eq!(st, 200);
        assert!(j["entries"][0]["entry"].as_str().unwrap().contains("Alpha cells"), "{j}");
        // Resolve Citations' add tells the window
        let (st, j, _) = send(&cx, request("POST", "/api/add", &[], r#"{"meta":{"title":"Beta waves","authors":["Kim Ng"],"year":"2021"}}"#));
        assert_eq!((st, j["added"].clone()), (200, json!(true)));
        assert_eq!(added.lock().unwrap().as_slice(), [j["id"].as_str().unwrap().to_string()]);
        assert_eq!(send(&cx, request("GET", "/api/nothing", &[], "")).0, 404);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn serves_the_add_in_files_as_the_mac_does() {
        let dir = std::env::temp_dir();
        let cx = ctx(None, &dir);
        let page = respond(&cx, &request("GET", "/word/index.html", &[], ""));
        assert_eq!((page.status, page.content_type), (200, "text/html; charset=utf-8"));
        assert!(String::from_utf8_lossy(&page.body).contains("app.js"));
        assert_eq!(respond(&cx, &request("GET", "/word/", &[], "")).status, 200);
        // what index.html and the manifest load
        for (p, t) in [("/common.js", "text/javascript; charset=utf-8"), ("/resolve.js", "text/javascript; charset=utf-8"), ("/panel.css", "text/css; charset=utf-8"),
                       ("/word/app.js", "text/javascript; charset=utf-8"), ("/icons/icon-16.png", "image/png"), ("/icons/icon-32.png", "image/png"),
                       ("/icons/icon-80.png", "image/png"), ("/word/manifest.xml", "application/xml")] {
            let r = respond(&cx, &request("GET", p, &[], ""));
            assert_eq!((r.status, r.content_type), (200, t), "{p}");
        }
        for p in ["/../Cargo.toml", "/word/../../secret", "/%2e%2e/x", "/nothing.js"] {
            assert_eq!(respond(&cx, &request("GET", p, &[], "")).status, 404, "{p}");
        }
        // every URL in the sideloaded manifest is served
        let manifest = String::from_utf8_lossy(file("word/manifest.xml").unwrap()).to_string();
        for url in manifest.split('"').filter(|s| s.starts_with("https://localhost:47481/")) {
            let path = url.trim_start_matches("https://localhost:47481").split('?').next().unwrap();
            assert_eq!(respond(&cx, &request("GET", path, &[], "")).status, 200, "{url}");
        }
    }

    #[test]
    fn every_add_in_file_is_built_in() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apple/citations");
        fn walk(dir: &std::path::Path, base: &std::path::Path, out: &mut Vec<String>) {
            for e in std::fs::read_dir(dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, base, out);
                } else if !p.file_name().unwrap().to_string_lossy().starts_with('.') {
                    out.push(p.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/"));
                }
            }
        }
        let mut on_disk = Vec::new();
        walk(&root, &root, &mut on_disk);
        for f in &on_disk {
            assert!(file(f).is_some(), "apple/citations/{f} is not in citeserver::FILES");
        }
    }

    #[test]
    fn parsing() {
        let head = "POST /api/bibliography?x=1 HTTP/1.1\r\nHost: localhost:47481\r\nContent-Type: application/json\r\nContent-Length: 10\r\nX-Margins-Token:  k \r\n\r\n";
        assert!(matches!(parse(head.as_bytes()), Parsed::Incomplete), "body still to come");
        assert!(matches!(parse(format!("{head}{{\"ids\"").as_bytes()), Parsed::Incomplete), "half the body");
        let Parsed::Request(r) = parse(format!("{head}{{\"ids\":[]}}").as_bytes()) else { panic!("complete") };
        assert_eq!((r.method.as_str(), r.path.as_str(), r.query["x"].as_str(), r.headers["x-margins-token"].as_str()), ("POST", "/api/bibliography", "1", "k"));
        assert_eq!(r.body, b"{\"ids\":[]}");
        assert!(matches!(parse(b"garbage\r\n\r\n"), Parsed::Bad(400)), "not HTTP");
        assert!(matches!(parse(b"POST /api/render HTTP/1.1\r\nContent-Length: 999999999\r\n\r\n"), Parsed::Bad(413)), "too big");
        assert!(matches!(parse(b"POST / HTTP/1.1\r\nContent-Length: -1\r\n\r\n"), Parsed::Bad(400)), "negative");
        assert!(matches!(parse(b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n"), Parsed::Bad(400)), "chunked");
        assert!(matches!(parse(&vec![b'A'; MAX_HEADER + 1]), Parsed::Bad(400)), "endless header");
        let wire = String::from_utf8(Response::json(200, &json!({"ok": true})).encoded()).unwrap();
        assert!(wire.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(wire.contains("Content-Length: 11\r\n"));
        assert!(wire.ends_with("\r\n\r\n{\"ok\":true}"));
    }

    /// The server over real TLS: a client that trusts only the authority made in a temporary
    /// folder (nothing added to any system store) reaches the add-in and the API; one that
    /// doesn't trust it can't.
    #[test]
    fn https_end_to_end() {
        let (lib, dir) = lib();
        let tls_dir = dir.join("tls");
        assert_eq!(citecerts::ensure(&tls_dir, time::OffsetDateTime::now_utc()), Outcome::NewAuthority);
        set_tls_for_tests(citecerts::server_config(&tls_dir).unwrap());
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let (l2, d2) = (lib.clone(), dir.clone());
        std::thread::spawn(move || {
            serve(listener, Arc::new(move || Ctx { lib: Some(l2.clone()), csl_cache: d2.join("csl"), access: Access { port, token: "k".into() }, added: Arc::new(|_| {}) }))
        });

        let ca = citecerts::cert_der(&tls_dir.join("ca.pem")).unwrap();
        let get = |roots: Option<&[u8]>, host: &str, path: &str, origin: Option<&str>| -> Result<(u16, String), String> {
            let mut store = rustls::RootCertStore::empty();
            if let Some(r) = roots {
                store.add(rustls::pki_types::CertificateDer::from(r.to_vec())).unwrap();
            }
            let provider = Arc::new(rustls::crypto::ring::default_provider());
            let config = rustls::ClientConfig::builder_with_provider(provider).with_safe_default_protocol_versions().unwrap()
                .with_root_certificates(store).with_no_client_auth();
            let name = rustls::pki_types::ServerName::try_from(host.to_string()).unwrap();
            let conn = rustls::ClientConnection::new(Arc::new(config), name).unwrap();
            let tcp = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
            let mut s = rustls::StreamOwned::new(conn, tcp);
            let o = origin.map(|o| format!("Origin: {o}\r\n")).unwrap_or_default();
            s.write_all(format!("GET {path} HTTP/1.1\r\nHost: {host}:{port}\r\n{o}Connection: close\r\n\r\n").as_bytes()).map_err(|e| e.to_string())?;
            let mut out = Vec::new();
            match s.read_to_end(&mut out) {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof && !out.is_empty() => {}
                Err(e) => return Err(e.to_string()),
            }
            let text = String::from_utf8_lossy(&out).to_string();
            let status = text.split(' ').nth(1).and_then(|s| s.parse().ok()).ok_or_else(|| format!("no status in {text:?}"))?;
            Ok((status, text))
        };
        let (st, page) = get(Some(&ca), "localhost", "/word/index.html", None).unwrap();
        assert_eq!(st, 200);
        assert!(page.contains("text/html") && page.contains("app.js"));
        let (st, body) = get(Some(&ca), "localhost", "/api/status", Some(&format!("https://localhost:{port}"))).unwrap();
        assert_eq!(st, 200);
        assert!(body.contains("\"api\":"), "{body}");
        let (st, body) = get(Some(&ca), "127.0.0.1", "/api/status", Some("https://getlunta.app")).unwrap();
        assert_eq!(st, 200);
        assert!(body.contains("Access-Control-Allow-Origin: https://getlunta.app"), "{body}");
        assert_eq!(get(Some(&ca), "localhost", "/api/status", Some("https://evil.example")).unwrap().0, 403);
        // without the authority trusted, the handshake fails (as in Word before Trust Certificate)
        assert!(get(None, "localhost", "/api/status", None).is_err());
        // another authority (another computer's Lunta) doesn't vouch for this one
        let other = dir.join("other");
        citecerts::ensure(&other, time::OffsetDateTime::now_utc());
        assert!(get(Some(&citecerts::cert_der(&other.join("ca.pem")).unwrap()), "localhost", "/api/status", None).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
