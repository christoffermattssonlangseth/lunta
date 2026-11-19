//! Asking Crossref, DataCite, OpenAlex, Europe PMC (PubMed), arXiv and bioRxiv politely: one
//! request at a time per service with a pause between them, the User-Agent naming Lunta and its
//! project page (no e-mail address), "too many requests" answered by waiting (Retry-After), and
//! answers kept for a while in the device's cache so a check run again soon asks nothing.
//!
//! Everything goes through [`Net`], so the tests replay recorded answers (`testdata/checks/`)
//! instead of going online.

use crate::model::*;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const USER_AGENT: &str = "Lunta (https://github.com/christoffermattssonlangseth/lunta)";

/// GET a URL: the body, or None when the service says there is no such thing (404, 410).
pub trait Net: Send + Sync {
    fn get(&self, url: &str) -> Result<Option<String>>;
}

/// The host of a URL ("api.crossref.org").
pub fn host(url: &str) -> &str {
    url.split('/').nth(2).unwrap_or("")
}

/// The pause between two requests to a service: what each asks for (arXiv: one request every
/// three seconds), or less than its limit (Crossref's public pool allows 5 a second, one at a time).
pub fn interval(host: &str) -> Duration {
    Duration::from_millis(match host {
        "export.arxiv.org" => 3000,
        "api.crossref.org" => 250,
        "api.biorxiv.org" => 500,
        "api.openalex.org" => 150,
        _ => 250,
    })
}

/// Hands out request times per host so requests to a service are at least `interval` apart,
/// also across threads (a time slot is reserved before sleeping, the lock isn't held while asleep).
pub struct Pacer {
    next: Mutex<HashMap<String, Instant>>,
    interval: fn(&str) -> Duration,
}

impl Pacer {
    pub fn new(interval: fn(&str) -> Duration) -> Self {
        Pacer { next: Mutex::new(HashMap::new()), interval }
    }

    /// Waits until a request to `url`'s host may go.
    pub fn wait(&self, url: &str) {
        let h = host(url).to_string();
        let slot = {
            let mut next = self.next.lock().unwrap();
            let now = Instant::now();
            let slot = next.get(&h).copied().filter(|t| *t > now).unwrap_or(now);
            next.insert(h.clone(), slot + (self.interval)(&h));
            slot
        };
        let now = Instant::now();
        if slot > now {
            std::thread::sleep(slot - now);
        }
    }
}

fn pacer() -> &'static Pacer {
    static P: OnceLock<Pacer> = OnceLock::new();
    P.get_or_init(|| Pacer::new(interval))
}

fn agent() -> &'static ureq::Agent {
    static A: OnceLock<ureq::Agent> = OnceLock::new();
    A.get_or_init(|| {
        ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(30)))
            .http_status_as_error(false)
            .build()
            .into()
    })
}

/// The services themselves.
pub struct LiveNet;

impl Net for LiveNet {
    fn get(&self, url: &str) -> Result<Option<String>> {
        let mut waited = 0;
        loop {
            pacer().wait(url);
            let mut r = agent()
                .get(url)
                .header("User-Agent", USER_AGENT)
                .header("Accept", "application/json, application/xml;q=0.9, */*;q=0.5")
                .call()
                .map_err(|e| CoreError::Network(format!("{}: {e}", host(url))))?;
            let code = r.status().as_u16();
            match code {
                200..=299 => return r.body_mut().with_config().limit(20 * 1024 * 1024).read_to_string().map(Some).map_err(|e| CoreError::Network(e.to_string())),
                404 | 410 => return Ok(None),
                429 | 502 | 503 | 504 if waited < 3 => {
                    // as long as the service asks (at most a minute), else 2, 5, 10 seconds
                    let asked = r.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|v| v.trim().parse::<u64>().ok());
                    let wait = asked.unwrap_or([2, 5, 10][waited]).min(60);
                    std::thread::sleep(Duration::from_secs(wait));
                    waited += 1;
                }
                429 => return Err(CoreError::Network(format!("{} is busy (too many requests); try again in a minute", host(url)))),
                c => return Err(CoreError::Network(format!("{} answered {c}", host(url)))),
            }
        }
    }
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct CachedAnswer {
    at: u64,
    body: Option<String>,
}

/// Answers kept for `ttl` (in memory, and in a file in the device's cache when given one):
/// a reference list checked again the same day, or the weekly check of a library, asks again only
/// for what's new. Errors aren't kept; "there is no such thing" is.
pub struct Cached<'a> {
    inner: &'a dyn Net,
    ttl: Duration,
    file: Option<PathBuf>,
    map: Mutex<HashMap<String, CachedAnswer>>,
    dirty: Mutex<usize>,
}

/// At most this many answers are kept in the cache file (the oldest go first).
const CACHE_MAX: usize = 4000;

impl<'a> Cached<'a> {
    pub fn new(inner: &'a dyn Net, ttl: Duration, file: Option<PathBuf>) -> Self {
        let map = file
            .as_ref()
            .and_then(|f| std::fs::read_to_string(f).ok())
            .and_then(|s| serde_json::from_str::<HashMap<String, CachedAnswer>>(&s).ok())
            .unwrap_or_default();
        Cached { inner, ttl, file, map: Mutex::new(map), dirty: Mutex::new(0) }
    }

    /// Writes the cache file (also done every 20 new answers and when dropped).
    pub fn flush(&self) {
        let Some(f) = &self.file else { return };
        let mut map = self.map.lock().unwrap().clone();
        if map.len() > CACHE_MAX {
            let mut ages: Vec<(u64, String)> = map.iter().map(|(k, v)| (v.at, k.clone())).collect();
            ages.sort();
            for (_, k) in ages.into_iter().take(map.len() - CACHE_MAX) {
                map.remove(&k);
            }
        }
        if let Some(dir) = f.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(s) = serde_json::to_string(&map) {
            let tmp = f.with_extension("tmp");
            if std::fs::write(&tmp, s).is_ok() {
                let _ = std::fs::rename(&tmp, f);
            }
        }
        *self.dirty.lock().unwrap() = 0;
    }
}

impl Drop for Cached<'_> {
    fn drop(&mut self) {
        if *self.dirty.lock().unwrap() > 0 {
            self.flush();
        }
    }
}

impl Net for Cached<'_> {
    fn get(&self, url: &str) -> Result<Option<String>> {
        let now = now_secs();
        if let Some(a) = self.map.lock().unwrap().get(url) {
            if now.saturating_sub(a.at) < self.ttl.as_secs() {
                return Ok(a.body.clone());
            }
        }
        let body = self.inner.get(url)?;
        self.map.lock().unwrap().insert(url.to_string(), CachedAnswer { at: now, body: body.clone() });
        let n = {
            let mut d = self.dirty.lock().unwrap();
            *d += 1;
            *d
        };
        if n >= 20 {
            self.flush();
        }
        Ok(body)
    }
}

/// Recorded answers (tests): `index.json` maps each URL to a file in the same folder, or to null
/// for "no such thing". A URL that isn't recorded is an error naming it.
pub struct Fixtures {
    dir: PathBuf,
    index: HashMap<String, Option<String>>,
    pub asked: Mutex<Vec<String>>,
}

impl Fixtures {
    pub fn open(dir: PathBuf) -> Self {
        let index = std::fs::read_to_string(dir.join("index.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Fixtures { dir, index, asked: Mutex::new(vec![]) }
    }
}

impl Net for Fixtures {
    fn get(&self, url: &str) -> Result<Option<String>> {
        self.asked.lock().unwrap().push(url.to_string());
        match self.index.get(url) {
            Some(Some(f)) => std::fs::read_to_string(self.dir.join(f)).map(Some).map_err(|e| CoreError::Io(format!("{f}: {e}"))),
            Some(None) => Ok(None),
            None => Err(CoreError::Network(format!("not recorded: {url}"))),
        }
    }
}

/// Records what another `Net` answers into a fixtures folder (for `cargo test -- --ignored`
/// to refresh the recordings).
pub struct Recorder<'a> {
    pub inner: &'a dyn Net,
    pub dir: PathBuf,
}

impl Net for Recorder<'_> {
    fn get(&self, url: &str) -> Result<Option<String>> {
        let body = self.inner.get(url)?;
        let index_path = self.dir.join("index.json");
        let mut index: std::collections::BTreeMap<String, Option<String>> =
            std::fs::read_to_string(&index_path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        let name = body.as_ref().map(|b| {
            let mut h: u64 = 0xcbf29ce484222325;
            for c in url.bytes() {
                h = (h ^ c as u64).wrapping_mul(0x100000001b3);
            }
            let ext = if b.trim_start().starts_with('<') { "xml" } else { "json" };
            let name = format!("{}-{h:016x}.{ext}", host(url).split('.').rev().nth(1).unwrap_or("x"));
            let _ = std::fs::create_dir_all(&self.dir);
            let _ = std::fs::write(self.dir.join(&name), b);
            name
        });
        index.insert(url.to_string(), name);
        let _ = std::fs::write(&index_path, serde_json::to_string_pretty(&index).unwrap_or_default());
        Ok(body)
    }
}
