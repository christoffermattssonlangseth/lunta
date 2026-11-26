//! OAuth 2.0 for installed apps (RFC 8252) against Google: authorization code with PKCE and a
//! loopback redirect (desktop, CLI), token refresh, and pluggable token storage.
//!
//! The pieces are separate so every platform can use what fits: the desktop app and the CLI call
//! `Authenticator::connect_loopback`; the Android app builds the URL with `authorization_url` (with
//! its own redirect URI, no client secret), opens a Custom Tab, and passes the returned `code` to
//! `Authenticator::complete`. Tokens go through a `TokenStore`, so each platform keeps them in its
//! own keychain/keystore; `FileTokenStore` (mode 0600) is the fallback for the CLI.
//!
//! Scope: `drive.file` (see `Scope`): Lunta sees only the files it created and the ones the user
//! picked for it in Google's file picker (`picker_url`). Client id / secret are never compiled in: they come from the
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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

pub const AUTH_URI: &str = "https://accounts.google.com/o/oauth2/v2/auth";
pub const TOKEN_URI: &str = "https://oauth2.googleapis.com/token";
pub const REVOKE_URI: &str = "https://oauth2.googleapis.com/revoke";

/// Begins every error that only signing in again can fix (the grant expired or was removed in the
/// Google account, or Lunta was not allowed to see its files): the apps show **Sign in again** for
/// it (`is_sign_in_needed`, `ffi::drive_needs_sign_in`) instead of a plain sync error.
pub const SIGN_IN_AGAIN: &str = "Sign in to Google Drive again";

/// True if `message` is an error that signing in again fixes (see `SIGN_IN_AGAIN`).
pub fn is_sign_in_needed(message: &str) -> bool {
    message.contains(SIGN_IN_AGAIN)
}

pub(crate) fn sign_in_needed(why: &str) -> CoreError {
    CoreError::Invalid(format!("{SIGN_IN_AGAIN}: {why}"))
}

/// What the app asks Google to let it do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// `https://www.googleapis.com/auth/drive`: all of the user's files. A *restricted* scope (Google's
    /// verification with a yearly security assessment). The apps no longer ask for it; tokens granted
    /// by older versions still carry it, and while they do the Drive client moves files Lunta did not
    /// create into app-created copies (`GoogleDrive`, "adoption"), so nothing is lost when the token is
    /// replaced by a `File` one. The CLI can still ask for it (`--scope full`).
    Full,
    /// `https://www.googleapis.com/auth/drive.file` (the default): only files created by this Google
    /// Cloud project's OAuth clients (the Mac bridge, Android and the CLI share one project) and the
    /// files the user picked for it in Google's picker. Non-sensitive: no security assessment, no
    /// "unverified app" screen once the brand is verified.
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

/// Whether Lunta asks Google for the narrow `drive.file` scope (and adopts other apps' files while
/// an old full-scope token lasts). On since 1.3, when the Windows and Linux app gained its own sync
/// through the Drive API (Library Storage › Google Drive); 1.2 and earlier asked for the full `drive`
/// scope. Off, sign-in would ask for the full scope again and no adoption would run.
pub const NARROW_DRIVE_ACCESS: bool = true;

/// The switch as used: always on in tests, which cover the drive.file behaviour.
pub(crate) fn narrow_drive_access() -> bool {
    cfg!(test) || NARROW_DRIVE_ACCESS
}

impl OAuthClient {
    pub fn new(client_id: impl Into<String>, client_secret: Option<String>) -> OAuthClient {
        OAuthClient {
            client_id: client_id.into(),
            client_secret: client_secret.filter(|s| !s.is_empty()),
            scope: if narrow_drive_access() { Scope::File } else { Scope::Full },
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

    /// Sign-in that shows Google's file picker ("OnePick", the picker for desktop and mobile apps:
    /// <https://developers.google.com/workspace/drive/picker/guides/desktop-mobile-picker>). The user
    /// chooses files or folders while consenting; the redirect carries `picked_file_ids` (see
    /// `picked_file_ids`) next to the usual `code`, and the token then reaches those items. Always
    /// `drive.file` (the picker accepts no other scope). Needs the Google Picker API enabled in the
    /// Cloud project; no API key. `file_ids` narrows what the picker shows (empty: everything).
    ///
    /// Picking a folder grants that folder, not the files already in it (Google grants `drive.file`
    /// per item); files Lunta created inside it are visible anyway.
    pub fn picker_url(&self, redirect_uri: &str, pkce: &Pkce, state: &str, file_ids: &[String]) -> String {
        let mut u = format!(
            "{}?client_id={}&redirect_uri={}&response_type=code&scope={}&code_challenge={}&code_challenge_method=S256&state={}&access_type=offline&prompt=consent&trigger_onepick=true&allow_folder_selection=true&allow_multiple=true",
            self.auth_uri,
            urlenc(&self.client_id),
            urlenc(redirect_uri),
            urlenc(Scope::File.url()),
            pkce.challenge,
            urlenc(state)
        );
        if !file_ids.is_empty() {
            u.push_str(&format!("&file_ids={}", urlenc(&file_ids.join(","))));
        }
        u
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
            .ok_or_else(|| sign_in_needed("Lunta's sign-in on this device has no refresh token."))?;
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
            // invalid_grant: the refresh token was revoked or expired (test-mode projects: 7 days),
            // or a code was used twice
            if err == "invalid_grant" {
                return Err(sign_in_needed(&format!("Google no longer accepts Lunta's sign-in (it expired, or Lunta's access was removed in your Google account). ({err} {desc})")));
            }
            return Err(CoreError::Network(format!("Google sign-in failed: {err} {desc} (HTTP {status})")));
        }
        Ok(Tokens {
            access_token: v["access_token"].as_str().unwrap_or_default().to_string(),
            refresh_token: v["refresh_token"].as_str().map(String::from).or_else(|| keep_refresh.map(String::from)),
            expires_at_ms: now_ms() + v["expires_in"].as_i64().unwrap_or(3600) * 1000,
            scope: v["scope"].as_str().map(String::from),
        })
    }
}

/// Shared HTTP agent: no automatic errors on non-2xx (callers read Google's JSON error bodies).
pub fn agent() -> &'static ureq::Agent {
    static A: OnceLock<ureq::Agent> = OnceLock::new();
    A.get_or_init(|| {
        ureq::Agent::config_builder()
            .tls_config(crate::tls::config())
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
            .tls_config(crate::tls::config())
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
    /// The scopes Google granted (space-separated, from the token response). None in tokens saved
    /// by versions that did not record it: those always asked for the full `drive` scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

impl Tokens {
    /// True if the grant reaches Drive at all (`drive` or `drive.file`). Google lets people untick
    /// permissions on the consent screen; a token saved without scopes came from the full scope.
    pub fn has_drive(&self) -> bool {
        match &self.scope {
            None => true,
            Some(s) => s.split_whitespace().any(|x| x == Scope::Full.url() || x == Scope::File.url()),
        }
    }

    /// True if the grant includes the full `drive` scope (or predates recording scopes, when the
    /// apps always asked for it).
    pub fn has_full_drive(&self) -> bool {
        match &self.scope {
            None => true,
            Some(s) => s.split_whitespace().any(|x| x == Scope::Full.url()),
        }
    }
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
        let t = self.current()?.ok_or_else(|| sign_in_needed("Lunta isn't signed in to Google on this device (lunta-sync: run connect first)."))?;
        if t.expires_at_ms - 60_000 > now_ms() {
            return Ok(t.access_token);
        }
        self.force_refresh()
    }

    /// Refreshes now (after the API answered 401).
    pub fn force_refresh(&self) -> Result<String> {
        let t = self.current()?.ok_or_else(|| sign_in_needed("Lunta isn't signed in to Google on this device."))?;
        let mut n = self.client.refresh(&t)?;
        if n.scope.is_none() {
            n.scope = t.scope.clone();
        }
        self.store.save(&n)?;
        *self.cached.lock().unwrap() = Some(n.clone());
        Ok(n.access_token)
    }

    /// True if the stored grant is the old full `drive` scope (signed in with an older version).
    /// Signing in again replaces it with `drive.file`.
    pub fn has_full_drive_access(&self) -> bool {
        narrow_drive_access() && matches!(self.current(), Ok(Some(t)) if t.has_full_drive())
    }

    /// Second half of the flow for callers that run the browser step themselves (Android).
    /// A grant without access to Drive (the user unticked it on Google's consent screen) is not
    /// kept: it would only fail at every sync.
    pub fn complete(&self, code: &str, verifier: &str, redirect_uri: &str) -> Result<()> {
        let t = self.client.exchange_code(code, verifier, redirect_uri)?;
        if !t.has_drive() {
            return Err(sign_in_needed("Google didn't give Lunta access to its files in your Drive. When Google asks, leave the Google Drive permission ticked."));
        }
        self.store.save(&t)?;
        *self.cached.lock().unwrap() = Some(t);
        Ok(())
    }

    /// The whole desktop flow: listens on 127.0.0.1 (random port), calls `open_url` with the
    /// consent URL (open the browser, or print it), waits for the redirect and stores the tokens.
    pub fn connect_loopback(&self, open_url: &dyn Fn(&str), timeout: Duration) -> Result<()> {
        self.connect_loopback_with(open_url, timeout, None, None).map(|_| ())
    }

    /// `connect_loopback` for an app (Windows/Linux desktop): stops with `SIGN_IN_CANCELLED` as
    /// soon as `cancel` is set (the user closed the dialog), and with `pick` shows Google's file
    /// picker during sign-in ("Choose your Lunta folder", `OAuthClient::picker_url`; `pick` narrows
    /// what it shows, empty: everything). Returns the picked ids (empty without `pick`).
    pub fn connect_loopback_with(&self, open_url: &dyn Fn(&str), timeout: Duration, cancel: Option<&AtomicBool>, pick: Option<&[String]>) -> Result<Vec<String>> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let redirect = format!("http://127.0.0.1:{}", listener.local_addr()?.port());
        let pkce = Pkce::generate()?;
        let state = random_state()?;
        let url = match pick {
            Some(ids) => self.client.picker_url(&redirect, &pkce, &state, ids),
            None => self.client.authorization_url(&redirect, &pkce, &state),
        };
        open_url(&url);
        let (code, target) = wait_for_redirect_with(&listener, &state, timeout, cancel)?;
        self.complete(&code, &pkce.verifier, &redirect)?;
        Ok(if pick.is_some() { picked_file_ids(&target) } else { Vec::new() })
    }

    /// Forgets the tokens on this device. Does **not** revoke the grant at Google: with
    /// `drive.file`, revoking takes away the app's access to every file it created (for every device
    /// signed in to the same account, since they share the Cloud project), and signing in again
    /// would not bring it back. Removing Lunta's access is left to the user's Google account page.
    pub fn disconnect(&self) -> Result<()> {
        *self.cached.lock().unwrap() = None;
        self.store.clear()
    }

    /// Revokes the grant at Google (best effort) and forgets the tokens. See `disconnect` for why
    /// the apps do not use this.
    pub fn revoke(&self) -> Result<()> {
        if let Ok(Some(t)) = self.current() {
            let tok = t.refresh_token.unwrap_or(t.access_token);
            let _ = agent()
                .post(REVOKE_URI)
                .header("Content-Type", "application/x-www-form-urlencoded")
                .send(format!("token={}", urlenc(&tok)).as_bytes());
        }
        self.disconnect()
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

/// The `picked_file_ids` of a picker redirect (`picker_url`), from the URL or its query.
pub fn picked_file_ids(redirect_url: &str) -> Vec<String> {
    let query = redirect_url.split_once('?').map(|(_, q)| q).unwrap_or(redirect_url);
    let query = query.split('#').next().unwrap_or("");
    query
        .split('&')
        .filter_map(|kv| kv.split_once('='))
        .filter(|(k, _)| *k == "picked_file_ids")
        .flat_map(|(_, v)| urldec(v).split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect::<Vec<_>>())
        .collect()
}

/// The error a cancelled `connect_loopback_with` returns.
pub const SIGN_IN_CANCELLED: &str = "Google sign-in was cancelled.";

#[cfg(test)]
fn wait_for_redirect(listener: &TcpListener, state: &str, timeout: Duration) -> Result<String> {
    wait_for_redirect_with(listener, state, timeout, None).map(|(code, _)| code)
}

/// Waits for the browser's redirect: (code, the request target with its query).
fn wait_for_redirect_with(listener: &TcpListener, state: &str, timeout: Duration, cancel: Option<&AtomicBool>) -> Result<(String, String)> {
    listener.set_nonblocking(true)?;
    let start = Instant::now();
    loop {
        if cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
            return Err(CoreError::Invalid(SIGN_IN_CANCELLED.into()));
        }
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
                    Ok(Some(c)) => return Ok((c, first.split_whitespace().nth(1).unwrap_or("").to_string())),
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
        assert!(u.contains("code_challenge_method=S256") && u.contains("scope=https%3A%2F%2Fwww.googleapis.com%2Fauth%2Fdrive.file&"), "{u}");
        let full = c.clone().with_scope(Scope::Full).authorization_url("http://127.0.0.1:5555", &p, "st");
        assert!(full.contains("scope=https%3A%2F%2Fwww.googleapis.com%2Fauth%2Fdrive&"));
        let pick = c.with_scope(Scope::Full).picker_url("app:/cb", &p, "st", &["a".into(), "b".into()]);
        assert!(pick.contains("trigger_onepick=true") && pick.contains("allow_folder_selection=true") && pick.contains("file_ids=a%2Cb"), "{pick}");
        assert!(pick.contains("scope=https%3A%2F%2Fwww.googleapis.com%2Fauth%2Fdrive.file&"), "the picker is always drive.file");
        assert_eq!(picked_file_ids("app:/cb?state=st&code=x&picked_file_ids=id1%2Cid2&scope=s"), vec!["id1", "id2"]);
        assert!(picked_file_ids("app:/cb?code=x").is_empty());
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
    fn loopback_flow_gives_picked_ids_and_can_be_cancelled() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let t = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            write!(s, "GET /?state=S&code=c2&picked_file_ids=f1%2Cf2 HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
            let mut out = String::new();
            s.read_to_string(&mut out).unwrap();
        });
        let (code, target) = wait_for_redirect_with(&l, "S", Duration::from_secs(5), None).unwrap();
        t.join().unwrap();
        assert_eq!(code, "c2");
        assert_eq!(picked_file_ids(&target), vec!["f1", "f2"]);

        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let cancel = AtomicBool::new(false);
        let started = Instant::now();
        std::thread::scope(|sc| {
            sc.spawn(|| {
                std::thread::sleep(Duration::from_millis(150));
                cancel.store(true, Ordering::SeqCst);
            });
            let e = wait_for_redirect_with(&l, "S", Duration::from_secs(30), Some(&cancel)).unwrap_err();
            assert!(e.to_string().contains(SIGN_IN_CANCELLED), "{e}");
        });
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn file_token_store_roundtrip() {
        let d = tempfile::tempdir().unwrap();
        let s = FileTokenStore::new(d.path().join("sub/tokens.json"));
        assert_eq!(s.load().unwrap(), None);
        let t = Tokens { access_token: "a".into(), refresh_token: Some("r".into()), expires_at_ms: 5, scope: Some(Scope::File.url().into()) };
        s.save(&t).unwrap();
        assert_eq!(s.load().unwrap(), Some(t.clone()));
        assert!(!t.has_full_drive());
        // tokens saved before scopes were recorded came from the full scope
        std::fs::write(&s.path, r#"{"access_token":"a","refresh_token":"r","expires_at_ms":5}"#).unwrap();
        let old = s.load().unwrap().unwrap();
        assert!(old.scope.is_none() && old.has_full_drive());
        s.clear().unwrap();
        assert_eq!(s.load().unwrap(), None);
    }
}
