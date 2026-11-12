//! HTTP helpers (mirrors `Net.swift`): a shared agent, a per-host circuit breaker, retries,
//! and content sniffing.

use super::{fail, ConvertError, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

pub const USER_AGENT: &str = "Paper2Epub/0.1 (Lunta core)";

fn agent() -> &'static ureq::Agent {
    static A: OnceLock<ureq::Agent> = OnceLock::new();
    A.get_or_init(|| {
        ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(300)))
            .user_agent(USER_AGENT)
            .build()
            .into()
    })
}

pub struct Response {
    pub data: Vec<u8>,
    pub status: u16,
    pub content_type: String,
}

/// Skips hosts that are down: after 3 failures in a row, requests fail fast for 5 minutes.
#[derive(Default)]
struct Breaker {
    failures: HashMap<String, u32>,
    open_until: HashMap<String, Instant>,
}

fn breaker() -> &'static Mutex<Breaker> {
    static B: OnceLock<Mutex<Breaker>> = OnceLock::new();
    B.get_or_init(Default::default)
}

impl Breaker {
    fn check(&self, host: &str) -> Result<()> {
        if let Some(until) = self.open_until.get(host) {
            if *until > Instant::now() {
                return Err(fail(format!("{host} is not responding; skipping it for a few minutes")));
            }
        }
        Ok(())
    }
    fn success(&mut self, host: &str) {
        self.failures.insert(host.into(), 0);
        self.open_until.remove(host);
    }
    fn failure(&mut self, host: &str) {
        let n = self.failures.entry(host.into()).or_insert(0);
        *n += 1;
        if *n >= 3 {
            self.open_until.insert(host.into(), Instant::now() + Duration::from_secs(300));
            self.failures.insert(host.into(), 0);
        }
    }
}

pub fn host(url: &str) -> String {
    url::Url::parse(url).ok().and_then(|u| u.host_str().map(String::from)).unwrap_or_default()
}

/// GET a URL. Errors for transport failures; non-2xx responses are returned so callers can decide.
pub fn fetch(url: &str, accept: Option<&str>, timeout: f64) -> Result<Response> {
    let h = host(url);
    breaker().lock().unwrap().check(&h)?;
    let t = Duration::from_secs_f64(timeout.min(45.0));
    let mut req = agent()
        .get(url)
        .config()
        .timeout_connect(Some(t))
        .timeout_recv_response(Some(t))
        .build();
    if let Some(a) = accept {
        req = req.header("Accept", a);
    }
    match req.call() {
        Ok(mut r) => {
            let status = r.status().as_u16();
            {
                let mut b = breaker().lock().unwrap();
                if status >= 500 {
                    b.failure(&h)
                } else {
                    b.success(&h)
                }
            }
            let content_type = r
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();
            let data = r
                .body_mut()
                .with_config()
                .limit(500 * 1024 * 1024)
                .read_to_vec()
                .map_err(|e| fail(format!("{h}: {e}")))?;
            Ok(Response { data, status, content_type })
        }
        Err(e) => {
            if matches!(
                e,
                ureq::Error::Timeout(_) | ureq::Error::ConnectionFailed | ureq::Error::HostNotFound | ureq::Error::Io(_)
            ) {
                breaker().lock().unwrap().failure(&h);
            }
            Err(ConvertError::Message(format!("{} ({h})", describe(&e))))
        }
    }
}

fn describe(e: &ureq::Error) -> String {
    match e {
        ureq::Error::Timeout(_) => "The request timed out.".into(),
        ureq::Error::HostNotFound => "A server with the specified hostname could not be found.".into(),
        ureq::Error::ConnectionFailed => "Could not connect to the server.".into(),
        other => other.to_string(),
    }
}

/// GET a URL and require a 2xx status.
pub fn get(url: &str, accept: Option<&str>, timeout: f64) -> Result<Vec<u8>> {
    let r = fetch(url, accept, timeout)?;
    if !(200..300).contains(&r.status) {
        let h = host(url);
        return Err(fail(format!("HTTP {} from {}", r.status, if h.is_empty() { url } else { &h })));
    }
    Ok(r.data)
}

pub fn json(url: &str) -> Result<serde_json::Map<String, Value>> {
    let d = get(url, Some("application/json"), 30.0)?;
    match serde_json::from_slice::<Value>(&d) {
        Ok(Value::Object(o)) => Ok(o),
        _ => Err(fail(format!("Unexpected JSON from {}", host(url)))),
    }
}

/// Retries transient failures (5xx) a couple of times; Europe PMC has bad minutes.
/// Timeouts are not retried: a slow service would just stall the queue.
pub fn with_retry<T>(attempts: usize, mut op: impl FnMut() -> Result<T>) -> Result<T> {
    let mut last = None;
    for i in 0..attempts {
        match op() {
            Ok(v) => return Ok(v),
            Err(e) => {
                let transient = e.to_string().contains("HTTP 5");
                last = Some(e);
                if !transient || i == attempts - 1 {
                    break;
                }
                std::thread::sleep(Duration::from_millis(1500 * (i as u64 + 1)));
            }
        }
    }
    Err(last.unwrap())
}

/// Maps `f` over `items` on up to `workers` threads, keeping the order.
pub fn par_map<T: Sync, R: Send>(items: &[T], workers: usize, f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    if items.is_empty() {
        return Vec::new();
    }
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results: Mutex<Vec<Option<R>>> = Mutex::new((0..items.len()).map(|_| None).collect());
    std::thread::scope(|s| {
        for _ in 0..workers.min(items.len()).max(1) {
            s.spawn(|| loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if i >= items.len() {
                    break;
                }
                let r = f(&items[i]);
                results.lock().unwrap()[i] = Some(r);
            });
        }
    });
    results.into_inner().unwrap().into_iter().map(|r| r.unwrap()).collect()
}

/// "pdf", "gzip", "zip", "jats", "html" or "unknown".
pub fn sniff(d: &[u8]) -> &'static str {
    if d.starts_with(b"%PDF-") {
        return "pdf";
    }
    if d.starts_with(&[0x1f, 0x8b]) {
        return "gzip";
    }
    if d.starts_with(&[0x50, 0x4b, 0x03, 0x04]) {
        return "zip";
    }
    let s = String::from_utf8_lossy(&d[..d.len().min(3000)]).to_lowercase();
    if s.contains("<article") || s.contains("jats") || s.contains("<!doctype article") {
        return "jats";
    }
    if s.contains("<html") || s.contains("<!doctype html") {
        return "html";
    }
    "unknown"
}

/// Percent-encodes everything outside Foundation's `urlPathAllowed` set.
pub fn encode_path(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~!$&'()*+,;=:@/".contains(&b) {
            o.push(b as char);
        } else {
            o.push_str(&format!("%{b:02X}"));
        }
    }
    o
}

/// Percent-encodes a query value.
pub fn encode_query(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~:/".contains(&b) {
            o.push(b as char);
        } else {
            o.push_str(&format!("%{b:02X}"));
        }
    }
    o
}

/// Percent-decodes; None if a sequence is malformed or the result isn't UTF-8.
pub fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let h = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(h, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}
