//! Google Drive REST v3 as a `RemoteStore`.
//!
//! * **Layout**: a folder (default "Lunta" in My Drive, or any folder id) holds the library exactly as
//!   `store.rs` lays it out, so it is the same tree "Google Drive for desktop" produces for a Mac.
//! * **Listing**: the first sync lists the tree (children of each folder, batched 20 parents per
//!   query). From then on the `changes` API returns only what changed; the folder id -> path map
//!   needed to place those changes travels inside the opaque cursor. Why `changes` and not listing
//!   with `modifiedTime > last`: folder `modifiedTime` does not reliably move when children are added,
//!   so a time filter would still need one request per paper folder; `changes` is one request per
//!   1000 changes and also reports deletions. A removed/renamed folder or an expired token falls back
//!   to a full listing.
//! * **Upload**: files up to 5 MB use one multipart request; larger ones a resumable session sent in
//!   8 MiB chunks (a chunk that fails is retried from the offset Drive reports).
//! * **Delete** moves to the trash (recoverable, and Drive for desktop treats it as a deletion).
//! * Requests retry with backoff on 429 / 5xx / network errors and refresh the token on 401.

use super::auth::{agent, agent_noredirect, Authenticator};
use super::remote::*;
use super::util::*;
use crate::model::{CoreError, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const FOLDER_MIME: &str = "application/vnd.google-apps.folder";
const FIELDS: &str = "id,name,mimeType,size,md5Checksum,modifiedTime,parents,trashed";
const MULTIPART_MAX: u64 = 5 * 1024 * 1024;
const CHUNK: u64 = 8 * 1024 * 1024; // multiple of 256 KiB

/// Which Drive folder is the library.
#[derive(Clone, Debug)]
pub enum DriveRoot {
    /// A folder called `name` directly in My Drive (created if `create`).
    Named { name: String, create: bool },
    /// A specific folder id (e.g. picked by the user).
    Id(String),
}

impl Default for DriveRoot {
    fn default() -> Self {
        DriveRoot::Named { name: "Lunta".into(), create: true }
    }
}

#[derive(Default)]
struct Cache {
    root_id: Option<String>,
    /// folder id -> path relative to the library root ("" = the root)
    folders: BTreeMap<String, String>,
}

#[derive(Serialize, Deserialize)]
struct Cursor {
    token: String,
    root: String,
    folders: BTreeMap<String, String>,
}

pub struct GoogleDrive {
    auth: Arc<Authenticator>,
    root: DriveRoot,
    api: String,
    upload_api: String,
    retry_base: Duration,
    cache: Mutex<Cache>,
}

struct Resp {
    status: u16,
    body: String,
    location: Option<String>,
    range: Option<String>,
}

#[derive(Debug, Clone)]
struct DFile {
    id: String,
    name: String,
    mime: String,
    size: u64,
    md5: Option<String>,
    modified_ms: i64,
    parents: Vec<String>,
    trashed: bool,
}

fn parse_file(v: &Value) -> Option<DFile> {
    Some(DFile {
        id: v["id"].as_str()?.to_string(),
        name: v["name"].as_str().unwrap_or("").to_string(),
        mime: v["mimeType"].as_str().unwrap_or("").to_string(),
        size: v["size"].as_str().and_then(|s| s.parse().ok()).or_else(|| v["size"].as_u64()).unwrap_or(0),
        md5: v["md5Checksum"].as_str().map(String::from),
        modified_ms: v["modifiedTime"].as_str().and_then(parse_rfc3339).unwrap_or(0),
        parents: v["parents"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default(),
        trashed: v["trashed"].as_bool().unwrap_or(false),
    })
}

fn q_esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\'', "\\'")
}

fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

fn mime_for(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "pdf" => "application/pdf",
        "epub" => "application/epub+zip",
        "json" => "application/json",
        "txt" => "text/plain",
        "m4a" => "audio/mp4",
        _ => "application/octet-stream",
    }
}

impl GoogleDrive {
    pub fn new(auth: Arc<Authenticator>, root: DriveRoot) -> GoogleDrive {
        GoogleDrive {
            auth,
            root,
            api: "https://www.googleapis.com/drive/v3".into(),
            upload_api: "https://www.googleapis.com/upload/drive/v3".into(),
            retry_base: Duration::from_millis(500),
            cache: Mutex::new(Cache::default()),
        }
    }

    /// Points the client at another server (tests use a local mock).
    pub fn with_endpoints(mut self, api: &str, upload_api: &str, retry_base: Duration) -> GoogleDrive {
        self.api = api.trim_end_matches('/').into();
        self.upload_api = upload_api.trim_end_matches('/').into();
        self.retry_base = retry_base;
        self
    }

    // ---- HTTP ----

    fn request(&self, method: &str, url: &str, headers: &[(&str, String)], body: Option<&[u8]>) -> Result<Resp> {
        self.request_with(agent(), method, url, headers, body)
    }

    fn request_with(&self, agent: &ureq::Agent, method: &str, url: &str, headers: &[(&str, String)], body: Option<&[u8]>) -> Result<Resp> {
        let mut refreshed = false;
        let mut attempt = 0;
        loop {
            let token = self.auth.access_token()?;
            let mut b = ureq::http::Request::builder().method(method).uri(url).header("Authorization", format!("Bearer {token}"));
            for (k, v) in headers {
                b = b.header(*k, v.as_str());
            }
            let res = match body {
                Some(data) => agent.run(b.body(data.to_vec()).map_err(net)?),
                None => agent.run(b.body(()).map_err(net)?),
            };
            match res {
                Ok(mut r) => {
                    let status = r.status().as_u16();
                    let hdr = |n: &str| r.headers().get(n).and_then(|v| v.to_str().ok()).map(String::from);
                    let (location, range) = (hdr("location"), hdr("range"));
                    let text = r.body_mut().read_to_string().unwrap_or_default();
                    let resp = Resp { status, body: text, location, range };
                    if status == 401 && !refreshed {
                        refreshed = true;
                        self.auth.force_refresh()?;
                        continue;
                    }
                    let throttled = status == 429 || (500..600).contains(&status) || (status == 403 && resp.body.contains("ateLimitExceeded"));
                    if throttled && attempt < 5 {
                        attempt += 1;
                        std::thread::sleep(self.retry_base * 2u32.pow(attempt - 1));
                        continue;
                    }
                    return Ok(resp);
                }
                Err(e) => {
                    if attempt < 5 {
                        attempt += 1;
                        std::thread::sleep(self.retry_base * 2u32.pow(attempt - 1));
                        continue;
                    }
                    return Err(net(e));
                }
            }
        }
    }

    fn ok(r: &Resp) -> Result<Value> {
        if (200..300).contains(&r.status) {
            return Ok(serde_json::from_str(&r.body).unwrap_or(Value::Null));
        }
        Err(Self::err(r))
    }

    fn err(r: &Resp) -> CoreError {
        let msg = serde_json::from_str::<Value>(&r.body)
            .ok()
            .and_then(|v| v["error"]["message"].as_str().map(String::from))
            .unwrap_or_else(|| r.body.chars().take(200).collect());
        if r.status == 404 {
            CoreError::NotFound(format!("Drive: {msg}"))
        } else {
            CoreError::Network(format!("Drive HTTP {}: {msg}", r.status))
        }
    }

    fn url(&self, path: &str, q: &[(&str, &str)]) -> String {
        let mut u = format!("{}{}", self.api, path);
        let mut first = true;
        for (k, v) in q.iter().chain([("supportsAllDrives", "true")].iter()) {
            u.push(if first { '?' } else { '&' });
            first = false;
            u.push_str(&format!("{}={}", urlenc(k), urlenc(v)));
        }
        u
    }

    fn get_json(&self, path: &str, q: &[(&str, &str)]) -> Result<Value> {
        Self::ok(&self.request("GET", &self.url(path, q), &[], None)?)
    }

    fn send_json(&self, method: &str, path: &str, q: &[(&str, &str)], body: &Value) -> Result<Value> {
        let r = self.request(method, &self.url(path, q), &[("Content-Type", "application/json; charset=UTF-8".into())], Some(body.to_string().as_bytes()))?;
        Self::ok(&r)
    }

    // ---- folders ----

    fn root_id(&self) -> Result<String> {
        if let Some(id) = self.cache.lock().unwrap().root_id.clone() {
            return Ok(id);
        }
        let id = match &self.root {
            DriveRoot::Id(id) => {
                let v = self.get_json(&format!("/files/{id}"), &[("fields", FIELDS)])?;
                let f = parse_file(&v).ok_or_else(|| CoreError::NotFound("library folder".into()))?;
                if f.mime != FOLDER_MIME || f.trashed {
                    return Err(CoreError::Invalid("that Drive id is not a folder (or is in the trash)".into()));
                }
                f.id
            }
            DriveRoot::Named { name, create } => {
                let q = format!("name = '{}' and 'root' in parents and mimeType = '{FOLDER_MIME}' and trashed = false", q_esc(name));
                let v = self.get_json("/files", &[("q", &q), ("fields", &format!("files({FIELDS})")), ("orderBy", "createdTime"), ("pageSize", "10")])?;
                match v["files"].as_array().and_then(|a| a.first()).and_then(parse_file) {
                    Some(f) => f.id,
                    None if *create => {
                        let v = self.send_json("POST", "/files", &[("fields", FIELDS)], &json!({"name": name, "mimeType": FOLDER_MIME, "parents": ["root"]}))?;
                        parse_file(&v).ok_or_else(|| CoreError::Network("Drive did not return the new folder".into()))?.id
                    }
                    None => return Err(CoreError::NotFound(format!("no folder \"{name}\" in My Drive"))),
                }
            }
        };
        let mut c = self.cache.lock().unwrap();
        c.root_id = Some(id.clone());
        c.folders.insert(id.clone(), String::new());
        Ok(id)
    }

    /// Id of the folder at `dir` (relative, "" = root), creating missing levels.
    fn ensure_folder(&self, dir: &str) -> Result<String> {
        let root = self.root_id()?;
        if dir.is_empty() {
            return Ok(root);
        }
        if let Some((id, _)) = self.cache.lock().unwrap().folders.iter().find(|(_, p)| p.as_str() == dir) {
            return Ok(id.clone());
        }
        let (parent_dir, name) = dir.rsplit_once('/').unwrap_or(("", dir));
        let parent = self.ensure_folder(parent_dir)?;
        let q = format!("name = '{}' and '{}' in parents and mimeType = '{FOLDER_MIME}' and trashed = false", q_esc(name), parent);
        let v = self.get_json("/files", &[("q", &q), ("fields", &format!("files({FIELDS})")), ("orderBy", "createdTime"), ("pageSize", "10")])?;
        let id = match v["files"].as_array().and_then(|a| a.first()).and_then(parse_file) {
            Some(f) => f.id,
            None => {
                let v = self.send_json("POST", "/files", &[("fields", FIELDS)], &json!({"name": name, "mimeType": FOLDER_MIME, "parents": [parent]}))?;
                parse_file(&v).ok_or_else(|| CoreError::Network("Drive did not return the new folder".into()))?.id
            }
        };
        self.cache.lock().unwrap().folders.insert(id.clone(), dir.to_string());
        Ok(id)
    }

    // ---- listing ----

    fn to_remote(&self, f: &DFile, path: String) -> RemoteFile {
        RemoteFile { path, id: f.id.clone(), size: f.size, modified_ms: f.modified_ms, md5: f.md5.clone() }
    }

    fn full_list(&self) -> Result<Listing> {
        let root = self.root_id()?;
        // token first: changes made while we list are then reported by the next incremental call
        let token = self.get_json("/changes/startPageToken", &[])?["startPageToken"].as_str().unwrap_or_default().to_string();
        let mut folders: BTreeMap<String, String> = BTreeMap::new();
        folders.insert(root.clone(), String::new());
        let mut files = Vec::new();
        let mut queue = vec![root.clone()];
        while !queue.is_empty() {
            let batch: Vec<String> = queue.drain(..queue.len().min(20)).collect();
            let q = format!("({}) and trashed = false", batch.iter().map(|i| format!("'{i}' in parents")).collect::<Vec<_>>().join(" or "));
            let mut page: Option<String> = None;
            loop {
                let mut qq = vec![("q", q.as_str()), ("fields", "nextPageToken,files(id,name,mimeType,size,md5Checksum,modifiedTime,parents,trashed)"), ("pageSize", "1000")];
                if let Some(p) = &page {
                    qq.push(("pageToken", p));
                }
                qq.push(("includeItemsFromAllDrives", "true"));
                let v = self.get_json("/files", &qq)?;
                for item in v["files"].as_array().cloned().unwrap_or_default() {
                    let Some(f) = parse_file(&item) else { continue };
                    let Some(dir) = f.parents.iter().find_map(|p| folders.get(p).cloned()) else { continue };
                    let path = join(&dir, &f.name);
                    if f.mime == FOLDER_MIME {
                        folders.insert(f.id.clone(), path);
                        queue.push(f.id);
                    } else if !f.mime.starts_with("application/vnd.google-apps.") {
                        files.push(self.to_remote(&f, path));
                    }
                }
                page = v["nextPageToken"].as_str().map(String::from);
                if page.is_none() {
                    break;
                }
            }
        }
        self.cache.lock().unwrap().folders = folders.clone();
        let cursor = serde_json::to_string(&Cursor { token, root, folders }).unwrap();
        Ok(Listing { files, removed_ids: vec![], full: true, cursor: Some(cursor) })
    }

    /// Path of folder `id` if it lies inside the library tree (fetching unknown ancestors).
    fn folder_path(&self, id: &str, folders: &mut BTreeMap<String, String>, outside: &mut HashSet<String>, depth: u8) -> Result<Option<String>> {
        if let Some(p) = folders.get(id) {
            return Ok(Some(p.clone()));
        }
        if depth == 0 || outside.contains(id) {
            return Ok(None);
        }
        let r = self.request("GET", &self.url(&format!("/files/{id}"), &[("fields", FIELDS)]), &[], None)?;
        if r.status == 404 || r.status == 403 {
            outside.insert(id.to_string());
            return Ok(None);
        }
        let f = parse_file(&Self::ok(&r)?).ok_or_else(|| CoreError::Network("bad folder metadata".into()))?;
        if f.trashed || f.mime != FOLDER_MIME {
            outside.insert(id.to_string());
            return Ok(None);
        }
        for p in &f.parents {
            if let Some(pp) = self.folder_path(p, folders, outside, depth - 1)? {
                let path = join(&pp, &f.name);
                folders.insert(id.to_string(), path.clone());
                return Ok(Some(path));
            }
        }
        outside.insert(id.to_string());
        Ok(None)
    }

    /// Delta since the cursor; None if it cannot be used (expired token, folder removed/renamed).
    fn changes(&self, cur: Cursor) -> Result<Option<Listing>> {
        let mut folders = cur.folders;
        let mut outside = HashSet::new();
        let mut upserts: BTreeMap<String, RemoteFile> = BTreeMap::new();
        let mut removed: Vec<String> = Vec::new();
        let mut token = cur.token;
        let fields = format!("nextPageToken,newStartPageToken,changes(fileId,removed,file({FIELDS}))");
        let new_token = loop {
            let r = self.request(
                "GET",
                &self.url("/changes", &[("pageToken", &token), ("pageSize", "1000"), ("fields", &fields), ("includeRemoved", "true"), ("includeItemsFromAllDrives", "true"), ("spaces", "drive")]),
                &[],
                None,
            )?;
            if r.status == 400 || r.status == 404 || r.status == 410 {
                return Ok(None);
            }
            let v = Self::ok(&r)?;
            for ch in v["changes"].as_array().cloned().unwrap_or_default() {
                let fid = ch["fileId"].as_str().unwrap_or("").to_string();
                let file = ch["file"].as_object().and_then(|_| parse_file(&ch["file"]));
                let gone = ch["removed"].as_bool().unwrap_or(false) || file.as_ref().is_none_or(|f| f.trashed);
                if folders.contains_key(&fid) && fid != cur.root {
                    // a known folder was trashed or renamed/moved: simplest correct answer is a full listing
                    let same = file.as_ref().filter(|_| !gone).and_then(|f| {
                        let parent = f.parents.first()?;
                        Some(join(folders.get(parent)?, &f.name) == folders[&fid])
                    });
                    if same != Some(true) {
                        return Ok(None);
                    }
                    continue;
                }
                if gone {
                    removed.push(fid.clone());
                    upserts.remove(&fid);
                    continue;
                }
                let Some(f) = file else { continue };
                let mut dir = None;
                for p in &f.parents {
                    if let Some(d) = self.folder_path(p, &mut folders, &mut outside, 6)? {
                        dir = Some(d);
                        break;
                    }
                }
                let Some(dir) = dir else {
                    removed.push(fid.clone()); // moved out of the library (or never in it)
                    upserts.remove(&fid);
                    continue;
                };
                let path = join(&dir, &f.name);
                if f.mime == FOLDER_MIME {
                    folders.insert(f.id.clone(), path);
                } else if !f.mime.starts_with("application/vnd.google-apps.") {
                    upserts.insert(f.id.clone(), self.to_remote(&f, path));
                    removed.retain(|x| x != &f.id);
                }
            }
            match v["nextPageToken"].as_str() {
                Some(t) => token = t.to_string(),
                None => break v["newStartPageToken"].as_str().map(String::from).unwrap_or(token),
            }
        };
        self.cache.lock().unwrap().folders = folders.clone();
        let cursor = serde_json::to_string(&Cursor { token: new_token, root: cur.root, folders }).unwrap();
        Ok(Some(Listing { files: upserts.into_values().collect(), removed_ids: removed, full: false, cursor: Some(cursor) }))
    }

    // ---- uploads ----

    fn upload_multipart(&self, id: Option<&str>, meta: &Value, local: &Path, mime: &str) -> Result<Value> {
        let data = fs::read(local)?;
        let boundary = format!("lunta{:x}", crate::model::now_ms());
        let mut body = Vec::with_capacity(data.len() + 512);
        write!(body, "--{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{meta}\r\n--{boundary}\r\nContent-Type: {mime}\r\n\r\n")?;
        body.extend_from_slice(&data);
        write!(body, "\r\n--{boundary}--")?;
        let (method, path) = match id {
            Some(id) => ("PATCH", format!("/files/{id}")),
            None => ("POST", "/files".to_string()),
        };
        let url = format!("{}{}?uploadType=multipart&supportsAllDrives=true&fields={}", self.upload_api, path, urlenc(FIELDS));
        Self::ok(&self.request(method, &url, &[("Content-Type", format!("multipart/related; boundary={boundary}"))], Some(&body))?)
    }

    fn upload_resumable(&self, id: Option<&str>, meta: &Value, local: &Path, mime: &str) -> Result<Value> {
        let total = fs::metadata(local)?.len();
        let (method, path) = match id {
            Some(id) => ("PATCH", format!("/files/{id}")),
            None => ("POST", "/files".to_string()),
        };
        let url = format!("{}{}?uploadType=resumable&supportsAllDrives=true&fields={}", self.upload_api, path, urlenc(FIELDS));
        for _session in 0..3 {
            let r = self.request(
                method,
                &url,
                &[
                    ("Content-Type", "application/json; charset=UTF-8".into()),
                    ("X-Upload-Content-Type", mime.into()),
                    ("X-Upload-Content-Length", total.to_string()),
                ],
                Some(meta.to_string().as_bytes()),
            )?;
            if !(200..300).contains(&r.status) {
                return Err(Self::err(&r));
            }
            let session = r.location.ok_or_else(|| CoreError::Network("Drive gave no upload session".into()))?;
            let mut f = fs::File::open(local)?;
            let mut offset = 0u64;
            let mut fails = 0;
            loop {
                let end = (offset + CHUNK).min(total);
                let mut buf = vec![0u8; (end - offset) as usize];
                f.seek(SeekFrom::Start(offset))?;
                f.read_exact(&mut buf)?;
                let range = if total == 0 { "bytes */0".to_string() } else { format!("bytes {offset}-{}/{total}", end - 1) };
                let r = self.request_with(agent_noredirect(), "PUT", &session, &[("Content-Range", range), ("Content-Type", mime.into())], Some(&buf));
                let r = match r {
                    Ok(r) if r.status == 308 || (200..300).contains(&r.status) => r,
                    Ok(r) if r.status == 404 || r.status == 410 => break, // session gone: start over
                    other => {
                        // unknown outcome: ask where the server is, then continue from there
                        fails += 1;
                        if fails > 4 {
                            return Err(other.err().unwrap_or_else(|| CoreError::Network("upload failed".into())));
                        }
                        std::thread::sleep(self.retry_base * fails);
                        let q = self.request_with(agent_noredirect(), "PUT", &session, &[("Content-Range", format!("bytes */{total}"))], Some(&[]))?;
                        if (200..300).contains(&q.status) {
                            return Ok(serde_json::from_str(&q.body).unwrap_or(Value::Null));
                        }
                        offset = q.range.as_deref().and_then(|r| r.rsplit('-').next()).and_then(|n| n.parse::<u64>().ok()).map_or(0, |n| n + 1);
                        continue;
                    }
                };
                if (200..300).contains(&r.status) {
                    return Ok(serde_json::from_str(&r.body).unwrap_or(Value::Null));
                }
                offset = r.range.as_deref().and_then(|r| r.rsplit('-').next()).and_then(|n| n.parse::<u64>().ok()).map_or(end, |n| n + 1);
            }
        }
        Err(CoreError::Network("Drive upload session kept expiring".into()))
    }
}

impl RemoteStore for GoogleDrive {
    fn describe(&self) -> String {
        match &self.root {
            DriveRoot::Named { name, .. } => format!("Google Drive folder \"{name}\""),
            DriveRoot::Id(id) => format!("Google Drive folder {id}"),
        }
    }

    fn list(&self, cursor: Option<&str>) -> Result<Listing> {
        let root = self.root_id()?;
        if let Some(cur) = cursor.and_then(|c| serde_json::from_str::<Cursor>(c).ok()) {
            if cur.root == root {
                if let Some(l) = self.changes(cur)? {
                    return Ok(l);
                }
            }
        }
        self.full_list()
    }

    fn download(&self, file: &RemoteFile, dest: &Path) -> Result<()> {
        let mut refreshed = false;
        let mut attempt = 0;
        loop {
            let token = self.auth.access_token()?;
            let url = self.url(&format!("/files/{}", file.id), &[("alt", "media")]);
            let req = ureq::http::Request::builder().method("GET").uri(&url).header("Authorization", format!("Bearer {token}")).body(()).map_err(net)?;
            match agent().run(req) {
                Ok(mut r) => {
                    let status = r.status().as_u16();
                    if (200..300).contains(&status) {
                        let mut out = fs::File::create(dest)?;
                        std::io::copy(&mut r.body_mut().as_reader(), &mut out).map_err(net)?;
                        return Ok(());
                    }
                    let text = r.body_mut().read_to_string().unwrap_or_default();
                    let resp = Resp { status, body: text, location: None, range: None };
                    if status == 401 && !refreshed {
                        refreshed = true;
                        self.auth.force_refresh()?;
                        continue;
                    }
                    if (status == 429 || status >= 500) && attempt < 5 {
                        attempt += 1;
                        std::thread::sleep(self.retry_base * 2u32.pow(attempt - 1));
                        continue;
                    }
                    return Err(Self::err(&resp));
                }
                Err(e) => {
                    if attempt < 5 {
                        attempt += 1;
                        std::thread::sleep(self.retry_base * 2u32.pow(attempt - 1));
                        continue;
                    }
                    return Err(net(e));
                }
            }
        }
    }

    fn upload(&self, path: &str, local: &Path, modified_ms: i64, existing: Option<&RemoteFile>) -> Result<RemoteFile> {
        let size = fs::metadata(local)?.len();
        let mime = mime_for(path);
        let (dir, name) = path.rsplit_once('/').unwrap_or(("", path));
        let mut meta = json!({ "modifiedTime": format_rfc3339(modified_ms) });
        let id = existing.map(|e| e.id.as_str());
        if id.is_none() {
            let parent = self.ensure_folder(dir)?;
            meta["name"] = json!(name);
            meta["parents"] = json!([parent]);
        }
        let v = if size <= MULTIPART_MAX { self.upload_multipart(id, &meta, local, mime)? } else { self.upload_resumable(id, &meta, local, mime)? };
        let f = parse_file(&v).ok_or_else(|| CoreError::Network("Drive did not return the uploaded file".into()))?;
        Ok(self.to_remote(&f, path.to_string()))
    }

    fn delete(&self, file: &RemoteFile) -> Result<()> {
        self.send_json("PATCH", &format!("/files/{}", file.id), &[("fields", "id")], &json!({"trashed": true}))?;
        Ok(())
    }
}
