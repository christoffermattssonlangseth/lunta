//! A small in-memory imitation of the parts of the Drive v3 API the client uses, served on 127.0.0.1.
//! Good enough to exercise request shapes, listing, `changes`, multipart/resumable uploads, token
//! refresh and retries without credentials. (Real Drive is covered by the #[ignore]d tests.)

use super::util::*;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

pub const FOLDER: &str = "application/vnd.google-apps.folder";

#[derive(Clone)]
pub struct MFile {
    pub id: String,
    pub name: String,
    pub parents: Vec<String>,
    pub mime: String,
    pub data: Vec<u8>,
    pub modified: i64,
    pub trashed: bool,
}

#[derive(Default)]
pub struct Inner {
    pub files: Vec<MFile>,
    pub log: Vec<String>, // file ids in change order; page token = index
    pub counts: HashMap<String, u32>,
    pub sessions: HashMap<String, (Option<String>, Value, Vec<u8>)>, // session id -> (file id, metadata, bytes so far)
    pub next_id: u32,
    /// fail the next N chunk PUTs with 503
    pub fail_puts: u32,
    /// accepted access token; anything else gets 401
    pub token: String,
    pub tokens_issued: u32,
}

#[derive(Clone)]
pub struct Mock {
    pub inner: Arc<Mutex<Inner>>,
    pub addr: String,
}

impl Mock {
    pub fn start() -> Mock {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = format!("http://{}", l.local_addr().unwrap());
        let inner = Arc::new(Mutex::new(Inner { token: "tok-0".into(), ..Default::default() }));
        inner.lock().unwrap().files.push(MFile { id: "ROOT".into(), name: "My Drive".into(), parents: vec![], mime: FOLDER.into(), data: vec![], modified: 0, trashed: false });
        let m = Mock { inner: inner.clone(), addr: addr.clone() };
        let m2 = m.clone();
        std::thread::spawn(move || {
            for s in l.incoming().flatten() {
                let m = m2.clone();
                std::thread::spawn(move || m.serve(s));
            }
        });
        m
    }

    pub fn api(&self) -> String {
        format!("{}/drive/v3", self.addr)
    }
    pub fn upload_api(&self) -> String {
        format!("{}/upload/drive/v3", self.addr)
    }
    pub fn count(&self, key: &str) -> u32 {
        *self.inner.lock().unwrap().counts.get(key).unwrap_or(&0)
    }

    fn new_id(g: &mut Inner) -> String {
        g.next_id += 1;
        format!("id{}", g.next_id)
    }

    // ---- direct manipulation, like Google Drive for desktop on a Mac ----

    /// Writes `path` (relative to the "Lunta" folder under My Drive), creating folders.
    pub fn put(&self, path: &str, data: &[u8], modified: i64) {
        let mut g = self.inner.lock().unwrap();
        let mut parent = Self::folder(&mut g, "ROOT", "Lunta");
        let parts: Vec<&str> = path.split('/').collect();
        for d in &parts[..parts.len() - 1] {
            parent = Self::folder(&mut g, &parent, d);
        }
        let name = parts[parts.len() - 1];
        let ex = g.files.iter().position(|f| f.name == name && f.parents == vec![parent.clone()] && !f.trashed);
        let id = match ex {
            Some(i) => {
                g.files[i].data = data.to_vec();
                g.files[i].modified = modified;
                g.files[i].id.clone()
            }
            None => {
                let id = Self::new_id(&mut g);
                g.files.push(MFile { id: id.clone(), name: name.into(), parents: vec![parent], mime: "application/octet-stream".into(), data: data.to_vec(), modified, trashed: false });
                id
            }
        };
        g.log.push(id);
    }

    fn folder(g: &mut Inner, parent: &str, name: &str) -> String {
        if let Some(f) = g.files.iter().find(|f| f.name == name && f.parents == vec![parent.to_string()] && f.mime == FOLDER && !f.trashed) {
            return f.id.clone();
        }
        let id = Self::new_id(g);
        g.files.push(MFile { id: id.clone(), name: name.into(), parents: vec![parent.into()], mime: FOLDER.into(), data: vec![], modified: 1, trashed: false });
        g.log.push(id.clone());
        id
    }

    pub fn path_of(g: &Inner, f: &MFile) -> String {
        let mut parts = vec![f.name.clone()];
        let mut cur = f.parents.first().cloned();
        while let Some(p) = cur {
            let pf = g.files.iter().find(|x| x.id == p).unwrap();
            if pf.id == "ROOT" {
                break;
            }
            parts.push(pf.name.clone());
            cur = pf.parents.first().cloned();
        }
        parts.reverse();
        parts[1..].join("/") // drop the "Lunta" level
    }

    pub fn read(&self, path: &str) -> Option<Vec<u8>> {
        let g = self.inner.lock().unwrap();
        g.files.iter().find(|f| f.mime != FOLDER && !f.trashed && Self::path_of(&g, f) == path).map(|f| f.data.clone())
    }

    pub fn trash(&self, path: &str) {
        let mut g = self.inner.lock().unwrap();
        let id = g.files.iter().find(|f| !f.trashed && f.mime != FOLDER && Self::path_of(&g, f) == path).unwrap().id.clone();
        g.files.iter_mut().find(|f| f.id == id).unwrap().trashed = true;
        g.log.push(id);
    }

    pub fn paths(&self) -> Vec<String> {
        let g = self.inner.lock().unwrap();
        let mut v: Vec<String> = g.files.iter().filter(|f| f.mime != FOLDER && !f.trashed).map(|f| Self::path_of(&g, f)).collect();
        v.sort();
        v
    }

    // ---- HTTP ----

    fn meta(f: &MFile) -> Value {
        let mut v = json!({"id": f.id, "name": f.name, "mimeType": f.mime, "parents": f.parents, "trashed": f.trashed, "modifiedTime": format_rfc3339(f.modified)});
        if f.mime != FOLDER {
            v["size"] = json!(f.data.len().to_string());
            v["md5Checksum"] = json!(md5_bytes(&f.data));
        }
        v
    }

    fn serve(&self, mut s: TcpStream) {
        let mut buf = Vec::new();
        let mut tmp = [0u8; 65536];
        let head_end = loop {
            let n = s.read(&mut tmp).unwrap_or(0);
            if n == 0 {
                return;
            }
            buf.extend_from_slice(&tmp[..n]);
            if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                break p + 4;
            }
        };
        let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
        let mut lines = head.lines();
        let first = lines.next().unwrap_or("").to_string();
        let mut it = first.split_whitespace();
        let (method, target) = (it.next().unwrap_or("").to_string(), it.next().unwrap_or("").to_string());
        let hdrs: HashMap<String, String> = lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_lowercase(), v.trim().to_string())).collect();
        let len: usize = hdrs.get("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
        while buf.len() < head_end + len {
            let n = s.read(&mut tmp).unwrap_or(0);
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&tmp[..n]);
        }
        let body = buf[head_end..head_end + len.min(buf.len() - head_end)].to_vec();
        let (path, query) = target.split_once('?').unwrap_or((&target, ""));
        let q: HashMap<String, String> = query.split('&').filter(|x| !x.is_empty()).map(|kv| {
            let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
            (urldec(k), urldec(v))
        }).collect();
        let (status, extra, resp) = self.route(&method, path, &q, &hdrs, &body);
        let mut out = format!("HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n", resp.len());
        for (k, v) in extra {
            out.push_str(&format!("{k}: {v}\r\n"));
        }
        out.push_str("\r\n");
        let _ = s.write_all(out.as_bytes());
        let _ = s.write_all(&resp);
    }

    fn route(&self, method: &str, path: &str, q: &HashMap<String, String>, h: &HashMap<String, String>, body: &[u8]) -> (u16, Vec<(String, String)>, Vec<u8>) {
        let mut g = self.inner.lock().unwrap();
        let norm = path.trim_start_matches("/upload").trim_start_matches("/drive/v3");
        let norm = Regex::new(r"^/(files|session)/[^/?]+").unwrap().replace(norm, "/$1/ID").to_string();
        let key = format!("{method} {norm}");
        *g.counts.entry(key.clone()).or_insert(0) += 1;
        let j = |status: u16, v: Value| (status, vec![], v.to_string().into_bytes());
        if path == "/token" {
            g.tokens_issued += 1;
            g.token = format!("tok-{}", g.tokens_issued);
            return j(200, json!({"access_token": g.token, "expires_in": 3600}));
        }
        if h.get("authorization").map(|a| a.as_str()) != Some(&format!("Bearer {}", g.token)) {
            return j(401, json!({"error": {"message": "Invalid Credentials"}}));
        }
        let path = path.trim_start_matches("/upload").trim_start_matches("/drive/v3");
        let now = crate::model::now_ms();
        match (method, path) {
            ("GET", "/changes/startPageToken") => j(200, json!({"startPageToken": g.log.len().to_string()})),
            ("GET", "/changes") => {
                let from: usize = q["pageToken"].parse().unwrap_or(usize::MAX);
                if from > g.log.len() {
                    return j(400, json!({"error": {"message": "Invalid token"}}));
                }
                let mut seen = std::collections::HashSet::new();
                let changes: Vec<Value> = g.log[from..]
                    .iter()
                    .filter(|id| seen.insert((*id).clone()))
                    .map(|id| {
                        let f = g.files.iter().find(|f| &f.id == id).unwrap();
                        json!({"fileId": id, "removed": false, "file": Self::meta(f)})
                    })
                    .collect();
                j(200, json!({"changes": changes, "newStartPageToken": g.log.len().to_string()}))
            }
            ("GET", "/files") => {
                let query = q.get("q").cloned().unwrap_or_default();
                let parents: Vec<String> = Regex::new(r"'([^']+)' in parents").unwrap().captures_iter(&query).map(|c| c[1].to_string()).collect();
                let name = Regex::new(r"name = '((?:[^'\\]|\\.)*)'").unwrap().captures(&query).map(|c| c[1].replace("\\'", "'"));
                let folder_only = query.contains("mimeType = ");
                let files: Vec<Value> = g
                    .files
                    .iter()
                    .filter(|f| f.id != "ROOT" && !f.trashed)
                    .filter(|f| parents.is_empty() || f.parents.iter().any(|p| parents.contains(p) || (p == "ROOT" && parents.contains(&"root".to_string()))))
                    .filter(|f| name.as_ref().is_none_or(|n| &f.name == n))
                    .filter(|f| !folder_only || f.mime == FOLDER)
                    .map(Self::meta)
                    .collect();
                j(200, json!({"files": files}))
            }
            ("GET", p) if p.starts_with("/files/") => {
                let id = &p[7..];
                match g.files.iter().find(|f| f.id == id) {
                    None => j(404, json!({"error": {"message": "File not found"}})),
                    Some(f) if q.get("alt").map(|a| a.as_str()) == Some("media") => (200, vec![], f.data.clone()),
                    Some(f) => j(200, Self::meta(f)),
                }
            }
            ("POST", "/files") | ("PATCH", _) | ("PUT", _) if q.get("uploadType").is_none() && !path.starts_with("/session") => {
                let v: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
                if method == "POST" {
                    let id = Self::new_id(&mut g);
                    let parents: Vec<String> = v["parents"].as_array().unwrap().iter().map(|p| if p == "root" { "ROOT".to_string() } else { p.as_str().unwrap().to_string() }).collect();
                    g.files.push(MFile { id: id.clone(), name: v["name"].as_str().unwrap().into(), parents, mime: v["mimeType"].as_str().unwrap_or("application/octet-stream").into(), data: vec![], modified: now, trashed: false });
                    g.log.push(id.clone());
                    j(200, Self::meta(g.files.iter().find(|f| f.id == id).unwrap()))
                } else {
                    let id = path[7..].to_string();
                    let f = g.files.iter_mut().find(|f| f.id == id).unwrap();
                    if v["trashed"] == json!(true) {
                        f.trashed = true;
                    }
                    let m = Self::meta(f);
                    g.log.push(id);
                    j(200, m)
                }
            }
            ("POST", "/files") | ("PATCH", _) if q.get("uploadType").map(|s| s.as_str()) == Some("multipart") => {
                let ct = h["content-type"].clone();
                let boundary = ct.split("boundary=").nth(1).unwrap().to_string();
                let text = String::from_utf8_lossy(body).to_string();
                let _ = text;
                // split on the delimiter at byte level
                let delim = format!("--{boundary}").into_bytes();
                let parts = split_bytes(body, &delim);
                let part = |i: usize| -> &[u8] {
                    let p = parts[i];
                    let he = p.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
                    let d = &p[he..];
                    d.strip_suffix(b"\r\n").unwrap_or(d)
                };
                let meta: Value = serde_json::from_slice(part(0)).unwrap();
                let data = part(1).to_vec();
                let id = if method == "PATCH" { Some(path[7..].to_string()) } else { None };
                Self::finish_upload(&mut g, id, &meta, data)
            }
            ("POST", "/files") | ("PATCH", _) if q.get("uploadType").map(|s| s.as_str()) == Some("resumable") => {
                let meta: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
                let sid = format!("s{}", Self::new_id(&mut g));
                let id = if method == "PATCH" { Some(path[7..].to_string()) } else { None };
                g.sessions.insert(sid.clone(), (id, meta, vec![]));
                (200, vec![("Location".into(), format!("{}/session/{sid}", self.addr))], vec![])
            }
            ("PUT", p) if p.starts_with("/session/") => {
                let sid = p[9..].to_string();
                if g.fail_puts > 0 {
                    g.fail_puts -= 1;
                    return j(503, json!({"error": {"message": "backend error"}}));
                }
                let Some((id, meta, have)) = g.sessions.get(&sid).cloned() else { return j(404, json!({})) };
                let cr = h["content-range"].clone(); // "bytes a-b/total" or "bytes */total"
                let (range, total) = cr.trim_start_matches("bytes ").split_once('/').unwrap();
                let total: usize = total.parse().unwrap();
                let mut have = have;
                if range != "*" {
                    let start: usize = range.split('-').next().unwrap().parse().unwrap();
                    assert_eq!(start, have.len(), "chunk out of order");
                    have.extend_from_slice(body);
                }
                if have.len() == total {
                    g.sessions.remove(&sid);
                    return Self::finish_upload(&mut g, id, &meta, have);
                }
                let range_hdr = if have.is_empty() { vec![] } else { vec![("Range".to_string(), format!("bytes=0-{}", have.len() - 1))] };
                g.sessions.insert(sid, (id, meta, have));
                (308, range_hdr, vec![])
            }
            _ => j(404, json!({"error": {"message": format!("mock: no route {method} {path}")}})),
        }
    }

    fn finish_upload(g: &mut Inner, id: Option<String>, meta: &Value, data: Vec<u8>) -> (u16, Vec<(String, String)>, Vec<u8>) {
        let modified = meta["modifiedTime"].as_str().and_then(parse_rfc3339).unwrap_or(0);
        let id = match id {
            Some(id) => {
                let f = g.files.iter_mut().find(|f| f.id == id).unwrap();
                f.data = data;
                f.modified = modified;
                id
            }
            None => {
                let id = Self::new_id(g);
                let parents = meta["parents"].as_array().unwrap().iter().map(|p| p.as_str().unwrap().to_string()).collect();
                g.files.push(MFile { id: id.clone(), name: meta["name"].as_str().unwrap().into(), parents, mime: "application/octet-stream".into(), data, modified, trashed: false });
                id
            }
        };
        g.log.push(id.clone());
        (200, vec![], Self::meta(g.files.iter().find(|f| f.id == id).unwrap()).to_string().into_bytes())
    }
}

fn split_bytes<'a>(data: &'a [u8], delim: &[u8]) -> Vec<&'a [u8]> {
    // pieces between delimiters, without the leading preamble and the closing "--"
    let mut idx = vec![];
    let mut i = 0;
    while i + delim.len() <= data.len() {
        if &data[i..i + delim.len()] == delim {
            idx.push(i);
            i += delim.len();
        } else {
            i += 1;
        }
    }
    idx.windows(2).map(|w| &data[w[0] + delim.len()..w[1]]).map(|p| p.strip_prefix(b"\r\n").unwrap_or(p)).collect()
}
