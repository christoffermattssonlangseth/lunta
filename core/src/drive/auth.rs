//! OAuth 2.0 for installed apps (RFC 8252) against Google: authorization code with PKCE and a
//! loopback redirect (desktop, CLI), token refresh, and pluggable token storage.
//!
//! The pieces are separate so every platform can use what fits: the desktop app and the CLI call
//! `Authenticator::connect_loopback`; the Android app builds the URL with `authorization_url` (with
//! its own redirect URI, no client secret), opens a Custom Tab, and passes the returned `code` to
//! `Authenticator::complete`. Tokens go through a `TokenStore`, so each platform keeps them in its
//! own keychain/keystore; `FileTokenStore` (mode 0600) is the fallback for the CLI.
//!
//! Scope: `drive` (see `Scope`). Client id / secret are never compiled in: they come from the
//! caller, or the environment variables `LUNTA_GOOGLE_CLIENT_ID` / `LUNTA_GOOGLE_CLIENT_SECRET`.

use super::util::*;
use crate::model::{now_ms, CoreError, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

pub const AUTH_URI: &str = "https://accounts.google.com/o/oauth2/v2/auth";
pub const TOKEN_URI: &str = "https://oauth2.googleapis.com/token";
pub const REVOKE_URI: &str = "https://oauth2.googleapis.com/revoke";

/// What the app asks Google to let it do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// `https://www.googleapis.com/auth/drive`: all of the user's files. Needed to share a library
    /// with a Mac that uses Google Drive for desktop, because those files were not created by this
    /// app. A *restricted* scope: fine for personal use and for test users of an unverified Google
    /// Cloud project; publishing the app to the public needs Google's verification and a security
    /// assessment.
    Full,
    /// `https://www.googleapis.com/auth/drive.file`: only files this app created. No verification
    /// needed, but it cannot see a library made by the Mac or by another install/client id, so it
    /// only suits a library used solely through the API.
    File,
}

impl Scope {
    pub fn url(self) -> &'static str {
        match self {
            Scope::Full => "https://www.googleapis.com/auth/drive",
            Scope::File => "https://www.googleapis.com/auth/drive.file",
        }
    }
}

#[derive(Clone, Debug)]
pub struct OAuthClient {
    pub client_id: String,
    /// Google's "Desktop app" clients have a secret (not confidential, but still required by the
    /// token endpoint); Android clients have none.
    pub client_secret: Option<String>,
    pub scope: Scope,
    pub auth_uri: String,
    pub token_uri: String,
}

impl OAuthClient {
    pub fn new(client_id: impl Into<String>, client_secret: Option<String>) -> OAuthClient {
        OAuthClient {
            client_id: client_id.into(),
            client_secret: client_secret.filter(|s| !s.is_empty()),
            scope: Scope::Full,
            auth_uri: AUTH_URI.into(),
            token_uri: TOKEN_URI.into(),
        }
    }

    pub fn from_env() -> Result<OAuthClient> {
        let id = std::env::var("LUNTA_GOOGLE_CLIENT_ID")
            .map_err(|_| CoreError::Invalid("set LUNTA_GOOGLE_CLIENT_ID (and LUNTA_GOOGLE_CLIENT_SECRET); see README, \"Google Drive sync\"".into()))?;
        Ok(OAuthClient::new(id, std::env::var("LUNTA_GOOGLE_CLIENT_SECRET").ok()))
    }

    pub fn with_scope(mut self, scope: Scope) -> OAuthClient {
        self.scope = scope;
        self
    }

    /// The URL to open in the user's browser.
    pub fn authorization_url(&self, redirect_uri: &str, pkce: &Pkce, state: &str) -> String {
        format!(
            "{}?client_id={}&redirect_uri={}&response_type=code&scope={}&code_challenge={}&code_challenge_method=S256&state={}&access_type=offline&prompt=consent",
            self.auth_uri,
            urlenc(&self.client_id),
            urlenc(redirect_uri),
            urlenc(self.scope.url()),
            pkce.challenge,
            urlenc(state)
        )
    }

    pub fn exchange_code(&self, code: &str, verifier: &str, redirect_uri: &str) -> Result<Tokens> {
        let mut form = vec![
            ("grant_type", "authorization_code"),
            ("code", code),
            ("code_verifier", verifier),
            ("redirect_uri", redirect_uri),
            ("client_id", self.client_id.as_str()),
        ];
        if let Some(s) = &self.client_secret {
            form.push(("client_secret", s));
        }
        self.token_request(&form, None)
    }

    /// Gets a fresh access token. Google does not return the refresh token again, so it is kept.
    pub fn refresh(&self, tokens: &Tokens) -> Result<Tokens> {
        let rt = tokens
            .refresh_token
            .as_deref()
            .ok_or_else(|| CoreError::Invalid("not connected to Google Drive (no refresh token); connect again".into()))?;
        let mut form = vec![("grant_type", "refresh_token"), ("refresh_token", rt), ("client_id", self.client_id.as_str())];
        if let Some(s) = &self.client_secret {
            form.push(("client_secret", s));
        }
        self.token_request(&form, Some(rt))
    }

    fn token_request(&self, form: &[(&str, &str)], keep_refresh: Option<&str>) -> Result<Tokens> {
        let body = form.iter().map(|(k, v)| format!("{}={}", urlenc(k), urlenc(v))).collect::<Vec<_>>().join("&");
        let mut resp = agent()
            .post(&self.token_uri)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .send(body.as_bytes())
            .map_err(net)?;
        let status = resp.status().as_u16();
        let text = resp.body_mut().read_to_string().map_err(net)?;
        let v: serde_json::Value = serde_json::from_str(&text).map_err(|_| net(format!("token endpoint answered {status}")))?;
        if status != 200 {
            let err = v["error"].as_str().unwrap_or("error");
            let desc = v["error_description"].as_str().unwrap_or("");
            // invalid_grant: the refresh token was revoked or expired (test-mode projects: 7 days)
            return Err(CoreError::Network(format!("Google sign-in failed: {err} {desc} (HTTP {status})")));
        }
        Ok(Tokens {
            access_token: v["access_token"].as_str().unwrap_or_default().to_string(),
            refresh_token: v["refresh_token"].as_str().map(String::from).or_else(|| keep_refresh.map(String::from)),
            expires_at_ms: now_ms() + v["expires_in"].as_i64().unwrap_or(3600) * 1000,
        })
    }
}

/// Shared HTTP agent: no automatic errors on non-2xx (callers read Google's JSON error bodies).
pub fn agent() -> &'static ureq::Agent {
    static A: OnceLock<ureq::Agent> = OnceLock::new();
    A.get_or_init(|| {
        ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_connect(Some(Duration::from_secs(20)))
            .timeout_recv_response(Some(Duration::from_secs(120)))
            .timeout_recv_body(Some(Duration::from_secs(600)))
            .timeout_send_body(Some(Duration::from_secs(600)))
            .user_agent("Lunta/0.1 (reference manager)")
            .build()
            .into()
    })
}

/// Same, but 3xx answers are returned instead of followed: resumable uploads answer 308
/// "Resume Incomplete" (no Location) after every chunk but the last.
pub fn agent_noredirect() -> &'static ureq::Agent {
    static A: OnceLock<ureq::Agent> = OnceLock::new();
    A.get_or_init(|| {
        ureq::Agent::config_builder()
            .http_status_as_error(false)
            .max_redirects(0)
            .max_redirects_will_error(false)
            .timeout_connect(Some(Duration::from_secs(20)))
            .timeout_recv_response(Some(Duration::from_secs(120)))
            .timeout_recv_body(Some(Duration::from_secs(600)))
            .timeout_send_body(Some(Duration::from_secs(600)))
            .user_agent("Lunta/0.1 (reference manager)")
            .build()
            .into()
    })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at_ms: i64,
}

/// Where the platform keeps the tokens (Keychain, Android Keystore-backed storage, a file).
pub trait TokenStore: Send + Sync {
    fn load(&self) -> Result<Option<Tokens>>;
    fn save(&self, tokens: &Tokens) -> Result<()>;
    fn clear(&self) -> Result<()>;
}

pub struct FileTokenStore {
    pub path: PathBuf,
}

impl FileTokenStore {
    pub fn new(path: impl Into<PathBuf>) -> FileTokenStore {
        FileTokenStore { path: path.into() }
    }
}

impl TokenStore for FileTokenStore {
    fn load(&self) -> Result<Option<Tokens>> {
        match std::fs::read_to_string(&self.path) {
            Ok(s) => Ok(serde_json::from_str(&s).ok()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
    fn save(&self, tokens: &Tokens) -> Result<()> {
        if let Some(d) = self.path.parent() {
            std::fs::create_dir_all(d)?;
        }
        let tmp = self.path.with_extension("tmp");
        {
            let mut opts = std::fs::OpenOptions::new();
            opts.write(true).create(true).truncate(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                opts.mode(0o600);
            }
            let mut f = opts.open(&tmp)?;
            f.write_all(serde_json::to_string(tokens).unwrap().as_bytes())?;
        }
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }
    fn clear(&self) -> Result<()> {
        let _ = std::fs::remove_file(&self.path);
        Ok(())
    }
}

#[derive(Default)]
pub struct MemoryTokenStore(Mutex<Option<Tokens>>);

impl TokenStore for MemoryTokenStore {
    fn load(&self) -> Result<Option<Tokens>> {
        Ok(self.0.lock().unwrap().clone())
    }
    fn save(&self, t: &Tokens) -> Result<()> {
        *self.0.lock().unwrap() = Some(t.clone());
        Ok(())
    }
    fn clear(&self) -> Result<()> {
        *self.0.lock().unwrap() = None;
        Ok(())
    }
}

/// PKCE (RFC 7636) verifier and its S256 challenge.
#[derive(Clone, Debug)]
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

impl Pkce {
    pub fn generate() -> Result<Pkce> {
        let mut b = [0u8; 48];
        getrandom::fill(&mut b).map_err(|e| CoreError::Invalid(format!("no randomness: {e}")))?;
        Ok(Pkce::from_verifier(URL_SAFE_NO_PAD.encode(b)))
    }
    pub fn from_verifier(verifier: String) -> Pkce {
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        Pkce { verifier, challenge }
    }
}

pub fn random_state() -> Result<String> {
    let mut b = [0u8; 18];
    getrandom::fill(&mut b).map_err(|e| CoreError::Invalid(format!("no randomness: {e}")))?;
    Ok(URL_SAFE_NO_PAD.encode(b))
}

/// Hands out valid access tokens, refreshing and persisting them as needed.
pub struct Authenticator {
    pub client: OAuthClient,
    store: Arc<dyn TokenStore>,
    cached: Mutex<Option<Tokens>>,
}

impl Authenticator {
    pub fn new(client: OAuthClient, store: Arc<dyn TokenStore>) -> Arc<Authenticator> {
        Arc::new(Authenticator { client, store, cached: Mutex::new(None) })
    }

    pub fn is_connected(&self) -> bool {
        matches!(self.current(), Ok(Some(t)) if t.refresh_token.is_some() || t.expires_at_ms > now_ms())
    }

    fn current(&self) -> Result<Option<Tokens>> {
        let mut c = self.cached.lock().unwrap();
        if c.is_none() {
            *c = self.store.load()?;
        }
        Ok(c.clone())
    }

    /// A valid access token (refreshed if it expires within a minute).
    pub fn access_token(&self) -> Result<String> {
        let t = self.current()?.ok_or_else(|| CoreError::Invalid("not connected to Google Drive; run connect first".into()))?;
        if t.expires_at_ms - 60_000 > now_ms() {
            return Ok(t.access_token);
        }
        self.force_refresh()
    }

    /// Refreshes now (after the API answered 401).
    pub fn force_refresh(&self) -> Result<String> {
        let t = self.current()?.ok_or_else(|| CoreError::Invalid("not connected to Google Drive".into()))?;
        let n = self.client.refresh(&t)?;
        self.store.save(&n)?;
        *self.cached.lock().unwrap() = Some(n.clone());
        Ok(n.access_token)
    }

    /// Second half of the flow for callers that run the browser step themselves (Android).
    pub fn complete(&self, code: &str, verifier: &str, redirect_uri: &str) -> Result<()> {
        let t = self.client.exchange_code(code, verifier, redirect_uri)?;
        self.store.save(&t)?;
        *self.cached.lock().unwrap() = Some(t);
        Ok(())
    }

    /// The whole desktop flow: listens on 127.0.0.1 (random port), calls `open_url` with the
    /// consent URL (open the browser, or print it), waits for the redirect and stores the tokens.
    pub fn connect_loopback(&self, open_url: &dyn Fn(&str), timeout: Duration) -> Result<()> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let redirect = format!("http://127.0.0.1:{}", listener.local_addr()?.port());
        let pkce = Pkce::generate()?;
        let state = random_state()?;
        open_url(&self.client.authorization_url(&redirect, &pkce, &state));
        let code = wait_for_redirect(&listener, &state, timeout)?;
        self.complete(&code, &pkce.verifier, &redirect)
    }

    /// Revokes the grant at Google (best effort) and forgets the tokens.
    pub fn disconnect(&self) -> Result<()> {
        if let Ok(Some(t)) = self.current() {
            let tok = t.refresh_token.unwrap_or(t.access_token);
            let _ = agent()
                .post(REVOKE_URI)
                .header("Content-Type", "application/x-www-form-urlencoded")
                .send(format!("token={}", urlenc(&tok)).as_bytes());
        }
        *self.cached.lock().unwrap() = None;
        self.store.clear()
    }
}

/// Parses `GET /?code=..&state=.. HTTP/1.1`; returns Ok(Some(code)), Ok(None) for unrelated requests
/// (favicon), or an error for a denied/mismatched redirect.
pub fn parse_redirect(request_line: &str, expect_state: &str) -> Result<Option<String>> {
    let target = request_line.split_whitespace().nth(1).unwrap_or("");
    let Some((_, query)) = target.split_once('?') else { return Ok(None) };
    let mut code = None;
    let mut state = None;
    let mut error = None;
    for kv in query.split('&') {
        let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
        match k {
            "code" => code = Some(urldec(v)),
            "state" => state = Some(urldec(v)),
            "error" => error = Some(urldec(v)),
            _ => {}
        }
    }
    if let Some(e) = error {
        return Err(CoreError::Invalid(format!("Google sign-in was not completed: {e}")));
    }
    match (code, state) {
        (Some(c), Some(s)) if s == expect_state => Ok(Some(c)),
        (Some(_), _) => Err(CoreError::Invalid("sign-in response had the wrong state; try again".into())),
        _ => Ok(None),
    }
}

fn wait_for_redirect(listener: &TcpListener, state: &str, timeout: Duration) -> Result<String> {
    listener.set_nonblocking(true)?;
    let start = Instant::now();
    loop {
        if start.elapsed() > timeout {
            return Err(CoreError::Invalid("timed out waiting for the Google sign-in".into()));
        }
        match listener.accept() {
            Ok((mut s, _)) => {
                s.set_nonblocking(false)?;
                s.set_read_timeout(Some(Duration::from_secs(5)))?;
                let mut buf = [0u8; 4096];
                let n = s.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let first = req.lines().next().unwrap_or("");
                let res = parse_redirect(first, state);
                let msg = match &res {
                    Ok(Some(_)) => "Lunta is connected to Google Drive. You can close this tab.",
                    Ok(None) => "Waiting for Google sign-in.",
                    Err(_) => "Sign-in failed. Close this tab and try again from Lunta.",
                };
                let body = format!("<!doctype html><meta charset=utf-8><title>Lunta</title><body style=\"font:16px system-ui;margin:3em\">{msg}");
                let _ = write!(
                    s,
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                match res {
                    Ok(Some(c)) => return Ok(c),
                    Ok(None) => {}
                    Err(e) => return Err(e),
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => return Err(e.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_rfc7636_vector() {
        let p = Pkce::from_verifier("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk".into());
        assert_eq!(p.challenge, "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
        let g = Pkce::generate().unwrap();
        assert!(g.verifier.len() >= 43 && g.verifier.len() <= 128);
    }

    #[test]
    fn auth_url_and_redirect() {
        let c = OAuthClient::new("id.apps.googleusercontent.com", None);
        let p = Pkce::from_verifier("v".repeat(43));
        let u = c.authorization_url("http://127.0.0.1:5555", &p, "st");
        assert!(u.contains("code_challenge_method=S256") && u.contains("scope=https%3A%2F%2Fwww.googleapis.com%2Fauth%2Fdrive&"));
        assert!(u.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A5555") && u.contains("access_type=offline"));
        assert_eq!(parse_redirect("GET /?code=4%2Fabc&state=st HTTP/1.1", "st").unwrap(), Some("4/abc".into()));
        assert_eq!(parse_redirect("GET /favicon.ico HTTP/1.1", "st").unwrap(), None);
        assert!(parse_redirect("GET /?code=x&state=bad HTTP/1.1", "st").is_err());
        assert!(parse_redirect("GET /?error=access_denied&state=st HTTP/1.1", "st").is_err());
    }

    #[test]
    fn loopback_flow_receives_code() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let t = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            write!(s, "GET /?code=thecode&state=S HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
            let mut out = String::new();
            s.read_to_string(&mut out).unwrap();
            out
        });
        assert_eq!(wait_for_redirect(&l, "S", Duration::from_secs(5)).unwrap(), "thecode");
        assert!(t.join().unwrap().contains("connected"));
    }

    #[test]
    fn file_token_store_roundtrip() {
        let d = tempfile::tempdir().unwrap();
        let s = FileTokenStore::new(d.path().join("sub/tokens.json"));
        assert_eq!(s.load().unwrap(), None);
        let t = Tokens { access_token: "a".into(), refresh_token: Some("r".into()), expires_at_ms: 5 };
        s.save(&t).unwrap();
        assert_eq!(s.load().unwrap(), Some(t));
        s.clear().unwrap();
        assert_eq!(s.load().unwrap(), None);
    }
}
