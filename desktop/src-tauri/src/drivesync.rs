//! Library Storage on Windows and Linux: the library either lives in a folder the user picked
//! (as before: a local folder, or one that Google Drive for desktop, Dropbox or OneDrive syncs), or
//! in the app's data folder (`DriveLibrary`), synced with the "Lunta" folder in Google Drive
//! **through the Drive API** by the core's `Syncer` — like Android and the Apple apps' Google Drive
//! choice. Only files Lunta made through the API are visible to it with the narrower `drive.file`
//! access (`NARROW_DRIVE_ACCESS`, on since 1.3), which is why this exists.
//!
//! Sign-in: OAuth for installed apps with a loopback redirect on 127.0.0.1 and PKCE, in the
//! system browser (`Authenticator::connect_loopback_with`). The OAuth client is Google's
//! "Desktop app" type, compiled in from `LUNTA_DESKTOP_GOOGLE_CLIENT_ID` / `_SECRET` (set by the
//! release workflow from the repository's secrets), or taken from `LUNTA_GOOGLE_CLIENT_ID` /
//! `_SECRET` at run time. Without either, Google Drive is offered greyed out.
//!
//! Syncs: at start, when the window comes to the front (if the last one is over a minute old),
//! every 5 minutes, and 20 s after this device's change log, reading positions or shared settings
//! change; Sync Now. "Download papers when you open them" makes it lazy: PDFs, reading views and
//! narration stay in Drive until opened (`ensure_local`), and count as present meanwhile
//! (`Library::set_remote_files`).
//!
//! Moving the library (`storage_move`): to Google Drive, the Drive library is synced first (what is
//! already there comes down), then this library is copied in with the core's copy rules
//! (`librarycopy`: missing files copied, change logs merged line by line, positions per paper,
//! nothing overwritten unless newer); out of Google Drive, the last changes are synced and every
//! file still only in Drive is downloaded first. The old copy is never changed or deleted.
//!
//! Debug builds: `LUNTA_DEV_DRIVE_DIR=/some/folder` uses that folder as "Google Drive" (no sign-in),
//! for trying the flows without an account.

use crate::{err, settings, AppState, R};
use lunta_core::drive::{self, Authenticator, DriveRoot, FileTokenStore, GoogleDrive, LocalDirRemote, OAuthClient, RemoteStore, SyncOptions, SyncReport, Syncer};
use lunta_core::librarycopy;
use lunta_core::model::{now_ms, FileKind, FileStatus};
use lunta_core::Library;
use serde::Serialize;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

const PERIOD_MS: i64 = 5 * 60 * 1000;
const AFTER_CHANGE_MS: i64 = 20 * 1000;
const FOCUS_MIN_AGE_MS: i64 = 60 * 1000;
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// The desktop OAuth client ("Desktop app" in the Cloud project): compiled in by the release build,
/// else from the environment (developers, self-built copies).
pub fn client() -> Option<OAuthClient> {
    let nonempty = |s: &str| !s.trim().is_empty();
    let (id, secret) = match option_env!("LUNTA_DESKTOP_GOOGLE_CLIENT_ID").filter(|s| nonempty(s)) {
        Some(id) => (id.trim().to_string(), option_env!("LUNTA_DESKTOP_GOOGLE_CLIENT_SECRET").map(|s| s.trim().to_string())),
        None => (
            std::env::var("LUNTA_GOOGLE_CLIENT_ID").ok().filter(|s| nonempty(s))?,
            std::env::var("LUNTA_GOOGLE_CLIENT_SECRET").ok(),
        ),
    };
    // the scope follows the core's switch (`drive.file` since 1.3)
    Some(OAuthClient::new(id, secret))
}

/// Debug builds only: a plain folder standing in for Google Drive.
fn dev_remote() -> Option<PathBuf> {
    if !cfg!(debug_assertions) {
        return None;
    }
    std::env::var("LUNTA_DEV_DRIVE_DIR").ok().filter(|s| !s.trim().is_empty()).map(PathBuf::from)
}

#[derive(Clone)]
struct Active {
    syncer: Arc<Syncer>,
    lib: Arc<Library>,
    root: PathBuf,
    device: String,
    lazy: bool,
}

#[derive(Default, Clone)]
struct Info {
    last_sync_ms: i64,
    error: Option<String>,
    notice: Option<String>,
    folder_not_visible: bool,
    /// The last sync's adoption left items behind: no "Sign In Again" until they are adopted.
    adoption_pending: bool,
    progress: Option<String>,
}

pub struct DriveSync {
    /// The app's local data folder: the Drive library, its sync state and the Google tokens.
    dir: PathBuf,
    active: Mutex<Option<Active>>,
    running: AtomicBool,
    info: Mutex<Info>,
    sign_in: Mutex<Option<Arc<AtomicBool>>>,
    due_ms: AtomicI64,
    last_attempt_ms: AtomicI64,
}

impl DriveSync {
    pub fn new(dir: PathBuf) -> DriveSync {
        DriveSync {
            dir,
            active: Mutex::new(None),
            running: AtomicBool::new(false),
            info: Mutex::new(Info::default()),
            sign_in: Mutex::new(None),
            due_ms: AtomicI64::new(0),
            last_attempt_ms: AtomicI64::new(0),
        }
    }

    /// Where a library kept in Google Drive lives on this computer.
    pub fn root(&self) -> PathBuf {
        self.dir.join("DriveLibrary")
    }
    fn state_path(&self) -> PathBuf {
        self.dir.join("drive-library-state.json")
    }
    fn tokens_path(&self) -> PathBuf {
        self.dir.join("google-tokens.json")
    }

    fn auth(&self) -> Option<Arc<Authenticator>> {
        Some(Authenticator::new(client()?, Arc::new(FileTokenStore::new(self.tokens_path()))))
    }

    pub fn available(&self) -> bool {
        dev_remote().is_some() || client().is_some()
    }

    pub fn signed_in(&self) -> bool {
        dev_remote().is_some() || self.auth().is_some_and(|a| a.is_connected())
    }

    fn active(&self) -> Option<Active> {
        self.active.lock().unwrap().clone()
    }

    fn remote(&self, folder_id: Option<String>) -> R<Box<dyn RemoteStore>> {
        if let Some(d) = dev_remote() {
            return Ok(Box::new(LocalDirRemote::new(d)));
        }
        let auth = self.auth().ok_or("Google sign-in isn’t available in this copy of Lunta.")?;
        if !auth.is_connected() {
            return Err("Sign in to Google Drive first.".into());
        }
        let root = match folder_id.filter(|s| !s.trim().is_empty()) {
            Some(id) => DriveRoot::Id(id),
            None => DriveRoot::default(),
        };
        Ok(Box::new(GoogleDrive::new(auth, root)))
    }

    fn make_syncer(&self, app: &AppHandle, root: &Path, device: &str, lazy: bool, folder_id: Option<String>) -> R<Syncer> {
        let mut o = SyncOptions::new(self.state_path());
        o.lazy = lazy;
        let a = app.clone();
        o.progress = Some(Arc::new(move |line: &str| {
            let ds = a.state::<DriveSync>();
            ds.info.lock().unwrap().progress = Some(line.to_string());
            let _ = a.emit("drive-progress", line.to_string());
        }));
        Syncer::new(root, device, self.remote(folder_id)?, o).map_err(err)
    }

    /// Builds (or drops) the syncer for what is open now: the Drive library while signed in.
    pub fn attach(&self, app: &AppHandle) {
        let st = app.state::<AppState>();
        let s = st.settings.lock().unwrap().clone();
        let open = st.open.lock().unwrap().as_ref().map(|o| (o.lib.clone(), o.root.clone()));
        let mut next = None;
        if let (true, Some((lib, root))) = (s.in_drive(), open) {
            if root == self.root() && self.signed_in() {
                match self.make_syncer(app, &root, &s.device_id, s.download_on_open(), s.drive_folder_id.clone()) {
                    Ok(syncer) => {
                        let a = Active { syncer: Arc::new(syncer), lib, root, device: s.device_id.clone(), lazy: s.download_on_open() };
                        // the files only in Drive count as present from the start (offline, from the last sync)
                        if a.lazy {
                            a.lib.set_remote_files(remote_only_paths(&a.syncer));
                        } else {
                            a.lib.set_remote_files(vec![]);
                        }
                        self.info.lock().unwrap().last_sync_ms = a.syncer.status().last_sync_ms;
                        next = Some(a);
                    }
                    Err(e) => self.info.lock().unwrap().error = Some(e),
                }
            }
        }
        *self.active.lock().unwrap() = next;
    }

    /// Asks the background loop for a sync `delay_ms` from now (or sooner if one is due already).
    pub fn schedule(&self, delay_ms: i64) {
        let at = now_ms() + delay_ms.max(0);
        let mut d = self.due_ms.load(Ordering::SeqCst);
        while d == 0 || d > at {
            match self.due_ms.compare_exchange(d, at, Ordering::SeqCst, Ordering::SeqCst) {
                Ok(_) => break,
                Err(now) => d = now,
            }
        }
    }

    /// One sync round, on the calling thread. None if there is nothing to sync or one is running.
    pub fn sync_once(&self, app: &AppHandle) -> Option<Result<SyncReport, String>> {
        let a = self.active()?;
        if self.running.swap(true, Ordering::SeqCst) {
            return None;
        }
        self.last_attempt_ms.store(now_ms(), Ordering::SeqCst);
        let _ = app.emit("drive-sync", SyncEvent { syncing: true, changed: false });
        let res = a.syncer.sync_now().map_err(err);
        let mut changed = false;
        {
            let mut info = self.info.lock().unwrap();
            info.progress = None;
            match &res {
                Ok(r) => {
                    changed = after_sync(&a, r);
                    info.last_sync_ms = r.finished_ms;
                    // an error only signing in again fixes is the one to show (Sign In Again)
                    let first = r.errors.iter().find(|e| drive::is_sign_in_needed(e)).or(r.errors.first());
                    info.error = match (r.errors.len(), first) {
                        (_, None) => None,
                        (1, Some(e)) => Some(e.clone()),
                        (n, Some(e)) => Some(format!("{n} files couldn’t be synced, e.g. {e}")),
                    };
                    if let Some(n) = r.notices.first() {
                        info.notice = Some(n.clone());
                    }
                    info.adoption_pending = drive::ffi::drive_adoption_incomplete(r.notices.clone());
                    info.folder_not_visible = false;
                }
                Err(e) => {
                    info.folder_not_visible = drive::is_folder_not_visible(e);
                    info.error = Some(e.clone());
                }
            }
        }
        self.running.store(false, Ordering::SeqCst);
        let _ = app.emit("drive-sync", SyncEvent { syncing: false, changed });
        if changed {
            // full texts of new papers into the search index
            while matches!(a.lib.index_fulltext(20), Ok(n) if n > 0) {}
        }
        Some(res)
    }
}

#[derive(Serialize, Clone)]
struct SyncEvent {
    syncing: bool,
    changed: bool,
}

/// Library-relative paths of the paper files that are only in the remote.
fn remote_only_paths(s: &Syncer) -> Vec<String> {
    s.remote_paper_files().into_iter().filter(|(_, _, local)| !local).map(|(p, _, _)| p).collect()
}

/// After a sync: reload the library if files changed, and tell it which files are only in Drive.
/// True if the lists should be reloaded.
fn after_sync(a: &Active, r: &SyncReport) -> bool {
    let mut changed = r.changed;
    if r.changed {
        let _ = a.lib.refresh();
    }
    if a.lazy {
        changed |= a.lib.set_remote_files(remote_only_paths(&a.syncer));
    }
    changed
}

/// Size of this device's change log and positions, and the shared settings' times: what changes
/// when something is done here (`start` syncs 20 s later).
fn local_signature(root: &Path, device: &str) -> u64 {
    let meta = |p: PathBuf| std::fs::metadata(p).ok();
    let stamp = |m: Option<std::fs::Metadata>| {
        m.map(|m| m.len() ^ m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as u64).unwrap_or(0).rotate_left(17)).unwrap_or(0)
    };
    let mut h = stamp(meta(root.join("ops").join(format!("{device}.jsonl"))));
    h = h.wrapping_mul(31).wrapping_add(stamp(meta(root.join("positions").join(format!("{device}.json")))));
    if let Ok(rd) = std::fs::read_dir(root) {
        let mut names: Vec<_> = rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect();
        names.sort();
        for p in names {
            h = h.wrapping_mul(31).wrapping_add(stamp(meta(p)));
        }
    }
    h
}

/// The background loop: syncs at start, every 5 minutes, 20 s after a change made here, and
/// whenever `schedule` asks.
pub fn start(app: AppHandle) {
    app.state::<DriveSync>().schedule(0);
    std::thread::spawn(move || {
        let mut seen: Option<(PathBuf, u64)> = None;
        loop {
            std::thread::sleep(Duration::from_secs(2));
            let ds = app.state::<DriveSync>();
            let Some(a) = ds.active() else {
                seen = None;
                continue;
            };
            let sig = local_signature(&a.root, &a.device);
            if matches!(&seen, Some((r, s)) if *r == a.root && *s != sig) {
                ds.schedule(AFTER_CHANGE_MS);
            }
            seen = Some((a.root.clone(), sig));
            let now = now_ms();
            if now - ds.last_attempt_ms.load(Ordering::SeqCst) >= PERIOD_MS {
                ds.schedule(0);
            }
            let due = ds.due_ms.load(Ordering::SeqCst);
            if due > 0 && now >= due && ds.due_ms.compare_exchange(due, 0, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
                ds.sync_once(&app);
                // what the sync itself wrote (merged positions) is not a change made here
                seen = Some((a.root.clone(), local_signature(&a.root, &a.device)));
            }
        }
    });
}

/// Makes sure a paper's files of these kinds are on this computer, downloading them from Google
/// Drive if they are only there. Call before reading a PDF, an EPUB or narration.
pub(crate) fn ensure_local(app: &AppHandle, id: &str, kinds: &[FileKind]) -> R<()> {
    let ds = app.state::<DriveSync>();
    let Some(a) = ds.active() else {
        // a Drive library while signed out: say why it can't be opened
        let st = app.state::<AppState>();
        let in_drive = st.settings.lock().unwrap().in_drive();
        if in_drive {
            if let Ok(lib) = st.lib() {
                if kinds.iter().any(|k| lib.file_status(id.to_string(), *k) == FileStatus::InCloud) {
                    return Err("This paper is in Google Drive and not downloaded yet. Sign in to Google Drive (Settings ▾ › Library Storage…) to download it.".into());
                }
            }
        }
        return Ok(());
    };
    let names: BTreeSet<String> = a.syncer.paper_files(id).into_iter().filter(|(_, _, local)| !local).map(|(n, _, _)| n).collect();
    let missing: Vec<FileKind> = kinds.iter().copied().filter(|k| names.contains(k.file_name()) && a.lib.file_path(id.to_string(), *k).is_none()).collect();
    if missing.is_empty() {
        return Ok(());
    }
    let _ = app.emit("drive-download", DownloadEvent { id: id.to_string(), active: true });
    let r = a.syncer.fetch_paper(id, &missing);
    let _ = app.emit("drive-download", DownloadEvent { id: id.to_string(), active: false });
    r.map_err(|e| format!("Couldn’t download it from Google Drive: {e}"))?;
    let _ = a.lib.reindex_quick();
    if a.lazy {
        a.lib.set_remote_files(remote_only_paths(&a.syncer));
    }
    Ok(())
}

#[derive(Serialize, Clone)]
struct DownloadEvent {
    id: String,
    active: bool,
}

/// Downloads every paper file that is only in the remote. (downloaded, failures)
fn download_all(s: &Syncer, progress: &dyn Fn(usize, usize)) -> (u32, Vec<String>) {
    let todo = remote_only_paths(s);
    let mut failed = Vec::new();
    let mut n = 0;
    for (i, p) in todo.iter().enumerate() {
        progress(i + 1, todo.len());
        match s.fetch_file(p) {
            Ok(_) => n += 1,
            Err(e) => failed.push(format!("{p}: {e}")),
        }
    }
    (n, failed)
}

// ---- commands ----

#[derive(Serialize)]
pub struct Status {
    /// Google sign-in is built in (or a test folder stands in for Drive).
    available: bool,
    /// "drive" or "folder"
    storage: String,
    library: Option<String>,
    signed_in: bool,
    signing_in: bool,
    syncing: bool,
    last_sync_ms: i64,
    error: Option<String>,
    notice: Option<String>,
    folder_not_visible: bool,
    /// The last sync failed in a way only signing in again fixes (expired or removed grant).
    needs_sign_in: bool,
    /// Signed in with the old full `drive` scope (a build from before 1.3) and nothing left to adopt:
    /// offer Sign In Again.
    full_access: bool,
    /// The narrow `drive.file` access is on (then "Choose your Lunta folder" is offered).
    narrow: bool,
    download_on_open: bool,
    pending_uploads: u32,
    pending_downloads: u32,
    not_downloaded: u32,
    here_bytes: u64,
    drive_only_bytes: u64,
    progress: Option<String>,
    /// The test folder standing in for Google Drive (debug builds).
    dev_remote: Option<String>,
}

fn status_of(app: &AppHandle) -> Status {
    let ds = app.state::<DriveSync>();
    let st = app.state::<AppState>();
    let s = st.settings.lock().unwrap().clone();
    let info = ds.info.lock().unwrap().clone();
    let a = ds.active();
    let (mut pu, mut pd, mut nd, mut here, mut only) = (0, 0, 0, 0, 0);
    if let Some(a) = &a {
        // status() and the file list wait for a running sync's lock: skip them meanwhile
        if !ds.running.load(Ordering::SeqCst) {
            let x = a.syncer.status();
            (pu, pd, nd) = (x.pending_uploads, x.pending_downloads, x.not_downloaded);
            for (_, size, local) in a.syncer.remote_paper_files() {
                if local {
                    here += size;
                } else {
                    only += size;
                }
            }
        }
    }
    let signing_in = ds.sign_in.lock().unwrap().is_some();
    let needs_sign_in = info.error.as_deref().is_some_and(drive::is_sign_in_needed);
    let library = st.open.lock().unwrap().as_ref().map(|o| o.root.to_string_lossy().to_string()).or(s.library.clone());
    Status {
        available: ds.available(),
        storage: if s.in_drive() { "drive".into() } else { "folder".into() },
        library,
        signed_in: ds.signed_in(),
        signing_in,
        syncing: ds.running.load(Ordering::SeqCst),
        last_sync_ms: info.last_sync_ms,
        error: info.error,
        notice: info.notice,
        folder_not_visible: info.folder_not_visible,
        needs_sign_in,
        full_access: dev_remote().is_none() && !info.adoption_pending && ds.auth().is_some_and(|a| a.has_full_drive_access()),
        narrow: drive::ffi::narrow_drive_access(),
        download_on_open: s.download_on_open(),
        pending_uploads: pu,
        pending_downloads: pd,
        not_downloaded: nd,
        here_bytes: here,
        drive_only_bytes: only,
        progress: info.progress,
        dev_remote: dev_remote().map(|p| p.to_string_lossy().to_string()),
    }
}

#[tauri::command]
pub fn drive_status(app: AppHandle) -> Status {
    status_of(&app)
}

/// Signs in to Google in the browser (loopback redirect). `pick`: with Google's folder picker
/// ("Choose your Lunta folder"); the first folder picked becomes the library's folder in Drive.
#[tauri::command]
pub async fn drive_sign_in(app: AppHandle, pick: bool) -> R<()> {
    let ds = app.state::<DriveSync>();
    if dev_remote().is_some() {
        return Ok(());
    }
    let auth = ds.auth().ok_or("Google sign-in isn’t available in this copy of Lunta.")?;
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut g = ds.sign_in.lock().unwrap();
        if let Some(old) = g.take() {
            old.store(true, Ordering::SeqCst);
        }
        *g = Some(cancel.clone());
    }
    let a2 = app.clone();
    let c2 = cancel.clone();
    let res = tauri::async_runtime::spawn_blocking(move || {
        use tauri_plugin_opener::OpenerExt;
        let open = |url: &str| {
            let _ = a2.opener().open_url(url, None::<&str>);
        };
        let ids = auth.connect_loopback_with(&open, SIGN_IN_TIMEOUT, Some(&c2), pick.then_some(&[][..]))?;
        let folder = if pick && !ids.is_empty() { GoogleDrive::new(auth.clone(), DriveRoot::default()).first_folder(&ids)? } else { None };
        Ok::<_, lunta_core::model::CoreError>((ids, folder))
    })
    .await
    .map_err(err)?;
    {
        let mut g = ds.sign_in.lock().unwrap();
        if g.as_ref().is_some_and(|c| Arc::ptr_eq(c, &cancel)) {
            *g = None;
        }
    }
    let (_, folder) = res.map_err(|e| {
        let m = err(e);
        if m.contains(drive::auth::SIGN_IN_CANCELLED) {
            drive::auth::SIGN_IN_CANCELLED.to_string()
        } else {
            format!("Couldn’t sign in to Google: {m}")
        }
    })?;
    if let Some((id, _name)) = folder {
        let st = app.state::<AppState>();
        let mut s = st.settings.lock().unwrap();
        s.drive_folder_id = Some(id);
        settings::save(&st.config_dir, &s).map_err(err)?;
    }
    {
        let mut info = ds.info.lock().unwrap();
        info.error = None;
        info.folder_not_visible = false;
    }
    ds.attach(&app);
    ds.schedule(0);
    Ok(())
}

#[tauri::command]
pub fn drive_cancel_sign_in(app: AppHandle) {
    if let Some(c) = app.state::<DriveSync>().sign_in.lock().unwrap().take() {
        c.store(true, Ordering::SeqCst);
    }
}

/// Forgets the Google tokens on this computer (the grant at Google stays: revoking it would cut
/// every device off from the files Lunta made). A library kept in Google Drive stays here and
/// is synced again after signing in.
#[tauri::command]
pub fn drive_sign_out(app: AppHandle) -> R<()> {
    let ds = app.state::<DriveSync>();
    if let Some(a) = ds.auth() {
        a.disconnect().map_err(err)?;
    }
    *ds.active.lock().unwrap() = None;
    let mut info = ds.info.lock().unwrap();
    info.error = None;
    info.folder_not_visible = false;
    info.progress = None;
    Ok(())
}

/// Syncs now (waits for it). Returns the status afterwards.
#[tauri::command]
pub async fn drive_sync_now(app: AppHandle) -> R<Status> {
    let a2 = app.clone();
    let res = tauri::async_runtime::spawn_blocking(move || a2.state::<DriveSync>().sync_once(&a2)).await.map_err(err)?;
    if let Some(Err(e)) = res {
        return Err(e);
    }
    Ok(status_of(&app))
}

/// The window came to the front: sync if the last one is over a minute old.
#[tauri::command]
pub fn drive_sync_soon(app: AppHandle) {
    let ds = app.state::<DriveSync>();
    if now_ms() - ds.last_attempt_ms.load(Ordering::SeqCst) > FOCUS_MIN_AGE_MS {
        ds.schedule(0);
    }
}

/// "Download papers when you open them". Switching on deletes nothing; switching off downloads
/// the rest at the next sync.
#[tauri::command]
pub fn drive_set_download_on_open(app: AppHandle, on: bool) -> R<()> {
    {
        let st = app.state::<AppState>();
        let mut s = st.settings.lock().unwrap();
        s.drive_download_on_open = Some(on);
        settings::save(&st.config_dir, &s).map_err(err)?;
    }
    let ds = app.state::<DriveSync>();
    ds.attach(&app);
    ds.schedule(0);
    Ok(())
}

/// Ids of the papers with files only in Google Drive (for the list's cloud marks and the
/// Download / Remove Download items).
#[tauri::command]
pub fn drive_remote_only(app: AppHandle) -> Vec<String> {
    let ds = app.state::<DriveSync>();
    let Some(a) = ds.active() else { return vec![] };
    if !a.lazy || ds.running.load(Ordering::SeqCst) {
        return vec![];
    }
    let ids: BTreeSet<String> = remote_only_paths(&a.syncer).into_iter().filter_map(|p| p.split('/').nth(1).map(String::from)).collect();
    ids.into_iter().collect()
}

/// Downloads these papers' files (all papers with none given). Returns how many files came.
#[tauri::command]
pub async fn drive_download(app: AppHandle, ids: Vec<String>) -> R<u32> {
    let a = app.state::<DriveSync>().active().ok_or("The library isn’t synced with Google Drive.")?;
    let app2 = app.clone();
    let n = tauri::async_runtime::spawn_blocking(move || -> R<u32> {
        if ids.is_empty() {
            let (n, failed) = download_all(&a.syncer, &|i, t| {
                let _ = app2.emit("drive-progress", format!("Downloading from Google Drive… {i} of {t}"));
            });
            if let Some(f) = failed.first() {
                return Err(format!("{} file(s) couldn’t be downloaded, e.g. {f}", failed.len()));
            }
            Ok(n)
        } else {
            let mut n = 0;
            for id in &ids {
                n += a.syncer.fetch_paper(id, &[]).map_err(err)?;
            }
            Ok(n)
        }
    })
    .await
    .map_err(err)??;
    if let Some(a) = app.state::<DriveSync>().active() {
        let _ = a.lib.reindex_quick();
        if a.lazy {
            a.lib.set_remote_files(remote_only_paths(&a.syncer));
        }
    }
    let _ = app.emit("drive-sync", SyncEvent { syncing: false, changed: true });
    Ok(n)
}

/// "Remove Download": deletes this computer's copies of the papers' PDFs, reading views and
/// narration that are safely in Google Drive. Returns a sentence for the status line.
#[tauri::command]
pub async fn drive_remove_download(app: AppHandle, ids: Vec<String>) -> R<String> {
    let a = app.state::<DriveSync>().active().ok_or("The library isn’t synced with Google Drive.")?;
    if !a.lazy {
        return Err("Turn on “Download papers when you open them” first (Settings ▾ › Library Storage…); otherwise the next sync downloads them again.".into());
    }
    let a2 = a.clone();
    let (removed, kept, bytes) = tauri::async_runtime::spawn_blocking(move || -> R<(u32, u32, u64)> {
        let mut t = (0, 0, 0);
        for id in &ids {
            let (r, k, b) = a2.syncer.remove_download(id).map_err(err)?;
            t = (t.0 + r, t.1 + k, t.2 + b);
        }
        Ok(t)
    })
    .await
    .map_err(err)??;
    a.lib.set_remote_files(remote_only_paths(&a.syncer));
    let _ = app.emit("drive-sync", SyncEvent { syncing: false, changed: true });
    let mut msg = format!("Removed {removed} downloaded file{} ({}).", if removed == 1 { "" } else { "s" }, size_text(bytes));
    if kept > 0 {
        msg.push_str(&format!(" Kept {kept} with changes not in Google Drive yet."));
    }
    Ok(msg)
}

fn size_text(b: u64) -> String {
    match b {
        b if b >= 1 << 30 => format!("{:.1} GB", b as f64 / (1u64 << 30) as f64),
        b if b >= 1 << 20 => format!("{:.0} MB", b as f64 / (1u64 << 20) as f64),
        b => format!("{} KB", b.div_ceil(1024)),
    }
}

/// Opens the library of the storage now chosen and builds its syncer.
fn switch_to(app: &AppHandle, root: &Path, drive: bool) -> R<()> {
    let st = app.state::<AppState>();
    std::fs::create_dir_all(root).map_err(err)?;
    st.open_library(root)?;
    *st.open_error.lock().unwrap() = None;
    {
        let mut s = st.settings.lock().unwrap();
        s.storage = Some(if drive { "drive" } else { "folder" }.into());
        if !drive {
            s.library = Some(root.to_string_lossy().to_string());
        }
        settings::save(&st.config_dir, &s).map_err(err)?;
    }
    let ds = app.state::<DriveSync>();
    ds.attach(app);
    if drive {
        ds.schedule(0);
    }
    Ok(())
}

/// Moves the library: `to` "drive" (into Google Drive, through the API) or "folder" (into
/// `folder`, a folder on this computer). Without a library open (first run), only switches.
/// Returns what happened, for the user.
#[tauri::command]
pub async fn storage_move(app: AppHandle, to: String, folder: Option<String>) -> R<String> {
    let st = app.state::<AppState>();
    let ds = app.state::<DriveSync>();
    if ds.running.load(Ordering::SeqCst) {
        return Err("A sync is running. Try again in a moment.".into());
    }
    let (in_drive, device, lazy, folder_id) = {
        let s = st.settings.lock().unwrap();
        (s.in_drive(), s.device_id.clone(), s.download_on_open(), s.drive_folder_id.clone())
    };
    let current = st.open.lock().unwrap().as_ref().map(|o| o.root.clone());
    let drive_root = ds.root();
    let progress = {
        let a = app.clone();
        move |line: String| {
            let _ = a.emit("drive-progress", line);
        }
    };
    match to.as_str() {
        "drive" => {
            if !ds.signed_in() {
                return Err("Sign in to Google Drive first.".into());
            }
            if in_drive {
                return Ok("Your library is already in Google Drive.".into());
            }
            let syncer = ds.make_syncer(&app, &drive_root, &device, lazy, folder_id)?;
            let src = current.clone();
            let p2 = progress.clone();
            let (dst, src2) = (drive_root.clone(), src.clone());
            let copied = tauri::async_runtime::spawn_blocking(move || move_into_drive(&syncer, src2.as_deref(), &dst, &p2)).await.map_err(err)??;
            switch_to(&app, &drive_root, true)?;
            Ok(match (src, copied) {
                (Some(src), Some(_)) => format!(
                    "Your library is now in Google Drive; Lunta is uploading it. The old copy is still in {}; Lunta no longer uses it.",
                    src.display()
                ),
                _ => "Your library is in Google Drive.".into(),
            })
        }
        "folder" => {
            let dest = PathBuf::from(folder.unwrap_or_default().trim());
            if dest.as_os_str().is_empty() {
                return Err("Choose a folder first.".into());
            }
            if dest.starts_with(&drive_root) {
                return Err("Choose a folder outside Lunta’s own Google Drive folder.".into());
            }
            if current.as_deref() == Some(dest.as_path()) {
                return Ok("Your library is already in that folder.".into());
            }
            let active = if in_drive { ds.active() } else { None };
            if in_drive && active.is_none() && !lazy_files_absent(&app) {
                return Err("Sign in to Google Drive first: some papers are only in Google Drive and must be downloaded before the library can move.".into());
            }
            let src = current.clone().ok_or("No library is open.")?;
            let p2 = progress.clone();
            let d2 = dest.clone();
            let syncer = active.map(|a| a.syncer);
            tauri::async_runtime::spawn_blocking(move || move_out(syncer.as_deref(), &src, &d2, &p2)).await.map_err(err)??;
            switch_to(&app, &dest, false)?;
            let src = current.unwrap_or_default();
            Ok(if in_drive {
                format!(
                    "Your library is now in {}. The old copy stays on this computer (in {}) and in the “Lunta” folder in your Google Drive, which this computer no longer syncs.",
                    dest.display(),
                    src.display()
                )
            } else {
                format!("Your library is now in {}. The old copy is still in {}; Lunta no longer uses it.", dest.display(), src.display())
            })
        }
        _ => Err(format!("unknown storage {to}")),
    }
}

/// True if no file of the open library is known to be only in Google Drive.
fn lazy_files_absent(app: &AppHandle) -> bool {
    let st = app.state::<AppState>();
    let Ok(lib) = st.lib() else { return true };
    let kinds = [FileKind::Pdf, FileKind::Epub, FileKind::Audio];
    !lib.papers(Default::default(), lunta_core::model::SortKey::Added)
        .iter()
        .any(|p| kinds.iter().any(|k| lib.file_status(p.id.clone(), *k) == FileStatus::InCloud))
}

/// Into Google Drive: sync the Drive library first (whatever is there already comes down), then
/// copy `src` into it. Some(result) if anything was copied, None without a source.
fn move_into_drive(syncer: &Syncer, src: Option<&Path>, drive_root: &Path, progress: &dyn Fn(String)) -> R<Option<librarycopy::CopyResult>> {
    progress("Looking at your Google Drive…".into());
    let r = syncer.sync_now().map_err(|e| format!("Couldn’t sync with Google Drive, so nothing was moved: {e}"))?;
    if let Some(e) = r.errors.first() {
        return Err(format!("Couldn’t sync with Google Drive, so nothing was moved: {e}"));
    }
    let Some(src) = src else { return Ok(None) };
    let res = librarycopy::copy_library(src, drive_root, &|i, n| progress(format!("Copying your library… {i} of {n}")));
    if !res.failed.is_empty() {
        return Err(format!(
            "{} file(s) couldn’t be copied (e.g. {}), so the library stays where it is.",
            res.failed.len(),
            res.failed[0]
        ));
    }
    Ok(Some(res))
}

/// Out of Google Drive (or from one folder to another): the last changes synced and every file
/// still only in Drive downloaded, then copied into `dest`.
fn move_out(syncer: Option<&Syncer>, src: &Path, dest: &Path, progress: &dyn Fn(String)) -> R<librarycopy::CopyResult> {
    if let Some(s) = syncer {
        progress("Syncing with Google Drive…".into());
        let r = s.sync_now().map_err(|e| format!("Couldn’t sync with Google Drive, so nothing was moved: {e}"))?;
        if let Some(e) = r.errors.first() {
            return Err(format!("Couldn’t sync with Google Drive, so nothing was moved: {e}"));
        }
        let (_, failed) = download_all(s, &|i, n| progress(format!("Downloading from Google Drive… {i} of {n}")));
        if let Some(f) = failed.first() {
            return Err(format!("{} file(s) couldn’t be downloaded (e.g. {f}), so the library stays where it is.", failed.len()));
        }
    }
    let res = librarycopy::copy_library(src, dest, &|i, n| progress(format!("Copying your library… {i} of {n}")));
    if !res.failed.is_empty() {
        return Err(format!(
            "{} file(s) couldn’t be copied (e.g. {}), so the library stays where it is.",
            res.failed.len(),
            res.failed[0]
        ));
    }
    Ok(res)
}

/// First run (or "Use Google Drive" without moving anything): open the library kept in Google
/// Drive, downloading it at the first sync.
#[tauri::command]
pub async fn storage_use_drive(app: AppHandle) -> R<()> {
    let ds = app.state::<DriveSync>();
    if !ds.signed_in() {
        return Err("Sign in to Google Drive first.".into());
    }
    let root = ds.root();
    switch_to(&app, &root, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lunta_core::model::PaperMeta;

    fn lib(root: &Path, cache: &Path, device: &str) -> Arc<Library> {
        Library::open(root.to_string_lossy().into(), cache.to_string_lossy().into(), device.into()).unwrap()
    }

    fn syncer(root: &Path, state: &Path, remote: &Path, device: &str, lazy: bool) -> Syncer {
        let mut o = SyncOptions::new(state);
        o.lazy = lazy;
        Syncer::new(root, device, Box::new(LocalDirRemote::new(remote)), o).unwrap()
    }

    fn meta(title: &str) -> PaperMeta {
        PaperMeta { title: title.into(), ..Default::default() }
    }

    /// Another device (Android) has a paper with a PDF in Drive; this computer syncs lazily, sees
    /// it as present, downloads it when opened, and can remove the download again.
    #[test]
    fn lazy_library_downloads_on_open() {
        let t = tempfile::tempdir().unwrap();
        let p = |s: &str| t.path().join(s);
        let remote = p("drive");
        // the other device
        let other = lib(&p("other"), &p("other-cache"), "android-1");
        let paper = other.add_paper(meta("A paper"), Some(b"%PDF-1.4 test".to_vec()), None, Some("full text".into())).unwrap();
        syncer(&p("other"), &p("other-state.json"), &remote, "android-1", false).sync_now().unwrap();

        // this computer
        let root = p("DriveLibrary");
        let mine = lib(&root, &p("cache"), "linux-1");
        let s = syncer(&root, &p("state.json"), &remote, "linux-1", true);
        let a = Active { syncer: Arc::new(s), lib: mine.clone(), root: root.clone(), device: "linux-1".into(), lazy: true };
        let r = a.syncer.sync_now().unwrap();
        assert!(r.skipped_lazy >= 1);
        assert!(after_sync(&a, &r));
        let here = mine.paper(paper.id.clone()).expect("the paper came down");
        assert!(here.has_pdf, "a PDF only in Drive counts as present");
        assert_eq!(mine.file_status(paper.id.clone(), FileKind::Pdf), FileStatus::InCloud);
        assert!(mine.file_path(paper.id.clone(), FileKind::Pdf).is_none());
        assert_eq!(remote_only_paths(&a.syncer), vec![format!("papers/{}/paper.pdf", paper.id)]);

        // opening it
        assert_eq!(a.syncer.fetch_paper(&paper.id, &[FileKind::Pdf]).unwrap(), 1);
        assert_eq!(std::fs::read(mine.file_path(paper.id.clone(), FileKind::Pdf).unwrap()).unwrap(), b"%PDF-1.4 test");
        assert!(remote_only_paths(&a.syncer).is_empty());

        // Remove Download: gone here, still in Drive, still present for the library
        let (removed, kept, _) = a.syncer.remove_download(&paper.id).unwrap();
        assert_eq!((removed, kept), (1, 0));
        mine.set_remote_files(remote_only_paths(&a.syncer));
        assert_eq!(mine.file_status(paper.id.clone(), FileKind::Pdf), FileStatus::InCloud);
        let r = a.syncer.sync_now().unwrap();
        assert_eq!(r.deleted_remote, 0, "a removed download is not a deletion");
        assert!(remote.join(format!("papers/{}/paper.pdf", paper.id)).exists());

        // download everything
        let (n, failed) = download_all(&a.syncer, &|_, _| {});
        assert_eq!((n, failed.len()), (1, 0));
    }

    /// Moving a folder library into Google Drive: what Drive has comes down, this library is copied
    /// in (logs merged), the next sync uploads it; the old folder is untouched.
    #[test]
    fn move_into_drive_merges_and_uploads() {
        let t = tempfile::tempdir().unwrap();
        let p = |s: &str| t.path().join(s);
        let remote = p("drive");
        let other = lib(&p("other"), &p("other-cache"), "android-1");
        other.add_paper(meta("From Android"), None, None, None).unwrap();
        syncer(&p("other"), &p("other-state.json"), &remote, "android-1", false).sync_now().unwrap();

        let folder = p("Library");
        let old = lib(&folder, &p("cache-old"), "linux-1");
        let mine = old.add_paper(meta("From this computer"), Some(b"%PDF mine".to_vec()), None, None).unwrap();
        drop(old);
        let before = std::fs::read(folder.join("ops/linux-1.jsonl")).unwrap();

        let root = p("DriveLibrary");
        let s = syncer(&root, &p("state.json"), &remote, "linux-1", false);
        let res = move_into_drive(&s, Some(&folder), &root, &|_| {}).unwrap().unwrap();
        assert!(res.copied >= 2 && res.failed.is_empty(), "{res:?}");
        assert_eq!(std::fs::read(folder.join("ops/linux-1.jsonl")).unwrap(), before, "the old copy is untouched");
        s.sync_now().unwrap();
        assert!(remote.join("ops/linux-1.jsonl").exists());
        assert!(remote.join(format!("papers/{}/paper.pdf", mine.id)).exists());
        let l = lib(&root, &p("cache"), "linux-1");
        let titles: BTreeSet<String> = l.papers(Default::default(), lunta_core::model::SortKey::Added).into_iter().map(|p| p.meta.title).collect();
        assert_eq!(titles, ["From Android", "From this computer"].into_iter().map(String::from).collect());
    }

    /// Moving out of Google Drive downloads what is only there first, so the new folder is complete.
    #[test]
    fn move_out_of_drive_downloads_first() {
        let t = tempfile::tempdir().unwrap();
        let p = |s: &str| t.path().join(s);
        let remote = p("drive");
        let other = lib(&p("other"), &p("other-cache"), "android-1");
        let paper = other.add_paper(meta("Only in Drive"), Some(b"%PDF remote".to_vec()), None, None).unwrap();
        syncer(&p("other"), &p("other-state.json"), &remote, "android-1", false).sync_now().unwrap();

        let root = p("DriveLibrary");
        let s = syncer(&root, &p("state.json"), &remote, "linux-1", true);
        s.sync_now().unwrap();
        assert!(!root.join(format!("papers/{}/paper.pdf", paper.id)).exists());
        let dest = p("Elsewhere");
        move_out(Some(&s), &root, &dest, &|_| {}).unwrap();
        assert_eq!(std::fs::read(dest.join(format!("papers/{}/paper.pdf", paper.id))).unwrap(), b"%PDF remote");
        assert!(dest.join("ops/android-1.jsonl").exists());
        assert!(!dest.join(".lunta-sync").exists());
    }

    #[test]
    fn a_change_here_changes_the_signature() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path();
        let a = local_signature(root, "linux-1");
        std::fs::create_dir_all(root.join("ops")).unwrap();
        std::fs::write(root.join("ops/linux-1.jsonl"), "x\n").unwrap();
        let b = local_signature(root, "linux-1");
        assert_ne!(a, b);
        std::fs::write(root.join("ops/android-1.jsonl"), "y\n").unwrap();
        assert_eq!(b, local_signature(root, "linux-1"), "other devices' logs don't count");
        std::fs::write(root.join("smart-folders.json"), "[]").unwrap();
        assert_ne!(b, local_signature(root, "linux-1"));
    }

    #[test]
    fn sizes_read_well() {
        assert_eq!(size_text(500), "1 KB");
        assert_eq!(size_text(5 << 20), "5 MB");
        assert_eq!(size_text(3 << 30), "3.0 GB");
    }
}
