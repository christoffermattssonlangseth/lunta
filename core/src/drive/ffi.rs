//! UniFFI exports of the Drive pieces the Android app needs: sign-in (Custom Tab + PKCE, with the
//! redirect handled by the app) and the Syncer. Types here are thin wrappers over the plain Rust
//! ones in this module tree; the Apple build (no `drive` feature) never sees them.

use super::*;
use crate::model::{CoreError, FileKind, Result};
use crate::ProgressListener;
use std::path::PathBuf;
use std::sync::Arc;

/// What the app opens in a Custom Tab, and what it must keep until the redirect comes back.
#[derive(uniffi::Record, Clone, Debug)]
pub struct SignInRequest {
    pub url: String,
    pub verifier: String,
    pub state: String,
}

/// Google sign-in for a client id with no secret (an "Android" OAuth client). Tokens are kept in
/// `token_file` (create it in the app's private, non-backed-up storage).
#[derive(uniffi::Object)]
pub struct DriveAuth {
    auth: Arc<Authenticator>,
}

#[uniffi::export]
impl DriveAuth {
    /// `full_access`: the `drive` scope (needed to share a library with a Mac that uses Google
    /// Drive for desktop) instead of `drive.file`.
    #[uniffi::constructor]
    pub fn new(client_id: String, token_file: String, full_access: bool) -> Arc<Self> {
        let client = OAuthClient::new(client_id, None).with_scope(if full_access { Scope::Full } else { Scope::File });
        Arc::new(DriveAuth { auth: Authenticator::new(client, Arc::new(FileTokenStore::new(token_file))) })
    }

    pub fn is_connected(&self) -> bool {
        self.auth.is_connected()
    }

    /// The consent URL for `redirect_uri` (e.g. `com.googleusercontent.apps.<id>:/oauth2redirect`).
    pub fn begin_sign_in(&self, redirect_uri: String) -> Result<SignInRequest> {
        let pkce = auth::Pkce::generate()?;
        let state = auth::random_state()?;
        Ok(SignInRequest { url: self.auth.client.authorization_url(&redirect_uri, &pkce, &state), verifier: pkce.verifier, state })
    }

    /// Checks the redirect the browser ended up at and returns the authorisation code.
    /// Errors if the user denied access or the state does not match.
    pub fn code_from_redirect(&self, redirect_url: String, expected_state: String) -> Result<String> {
        let query = redirect_url.split_once('?').map(|(_, q)| q).unwrap_or("");
        let query = query.split('#').next().unwrap_or("");
        auth::parse_redirect(&format!("GET /?{query} HTTP/1.1"), &expected_state)?
            .ok_or_else(|| CoreError::Invalid("the sign-in response had no code".into()))
    }

    /// Exchanges the code for tokens (network) and stores them.
    pub fn complete_sign_in(&self, code: String, verifier: String, redirect_uri: String) -> Result<()> {
        self.auth.complete(&code, &verifier, &redirect_uri)
    }

    /// Revokes the grant (best effort) and forgets the tokens.
    pub fn disconnect(&self) -> Result<()> {
        self.auth.disconnect()
    }
}

#[derive(uniffi::Record, Clone, Debug)]
pub struct SyncSummary {
    pub uploaded: u32,
    pub downloaded: u32,
    pub deleted_local: u32,
    pub deleted_remote: u32,
    pub conflicts: Vec<String>,
    pub errors: Vec<String>,
    pub skipped_lazy: u32,
    /// Something changed in the working folder: call `Library::refresh()`.
    pub changed: bool,
    pub finished_ms: i64,
}

#[derive(uniffi::Record, Clone, Debug)]
pub struct SyncInfo {
    pub remote: String,
    pub last_sync_ms: i64,
    pub tracked_files: u32,
    pub pending_uploads: u32,
    pub pending_downloads: u32,
    pub not_downloaded: u32,
}

#[derive(uniffi::Record, Clone, Debug)]
pub struct PaperRemoteFile {
    pub name: String,
    pub size: u64,
    pub local: bool,
}

/// Syncs a library working folder with a remote (Google Drive, or a plain folder for testing).
#[derive(uniffi::Object)]
pub struct LibrarySync {
    syncer: Syncer,
}

fn options(state_path: String, lazy: bool, progress: Option<Arc<dyn ProgressListener>>) -> SyncOptions {
    let mut o = SyncOptions::new(PathBuf::from(state_path));
    o.lazy = lazy;
    if let Some(p) = progress {
        o.progress = Some(Arc::new(move |line: &str| p.on_progress(line.to_string())));
    }
    o
}

#[uniffi::export]
impl LibrarySync {
    /// Syncs `root` (the folder `Library::open` gets) with the folder `folder_name` in My Drive.
    #[uniffi::constructor]
    pub fn google_drive(root: String, device: String, state_path: String, auth: Arc<DriveAuth>, folder_name: String, lazy: bool, progress: Option<Arc<dyn ProgressListener>>) -> Result<Arc<Self>> {
        let remote = GoogleDrive::new(auth.auth.clone(), DriveRoot::Named { name: folder_name, create: true });
        Ok(Arc::new(LibrarySync { syncer: Syncer::new(root, device, Box::new(remote), options(state_path, lazy, progress))? }))
    }

    /// Syncs with a plain folder acting as the remote (testing; also what a Mac with Google Drive
    /// for desktop looks like).
    #[uniffi::constructor]
    pub fn local_dir(root: String, device: String, state_path: String, remote_dir: String, lazy: bool, progress: Option<Arc<dyn ProgressListener>>) -> Result<Arc<Self>> {
        Ok(Arc::new(LibrarySync { syncer: Syncer::new(root, device, Box::new(LocalDirRemote::new(remote_dir)), options(state_path, lazy, progress))? }))
    }

    /// One sync round (blocking, network). Per-file errors are in the summary.
    pub fn sync_now(&self) -> Result<SyncSummary> {
        let r = self.syncer.sync_now()?;
        Ok(SyncSummary {
            uploaded: r.uploaded,
            downloaded: r.downloaded,
            deleted_local: r.deleted_local,
            deleted_remote: r.deleted_remote,
            conflicts: r.conflicts,
            errors: r.errors,
            skipped_lazy: r.skipped_lazy,
            changed: r.changed,
            finished_ms: r.finished_ms,
        })
    }

    pub fn status(&self) -> SyncInfo {
        let s = self.syncer.status();
        SyncInfo {
            remote: s.remote,
            last_sync_ms: s.last_sync_ms,
            tracked_files: s.tracked_files,
            pending_uploads: s.pending_uploads,
            pending_downloads: s.pending_downloads,
            not_downloaded: s.not_downloaded,
        }
    }

    /// The remote files of a paper as of the last sync.
    pub fn paper_files(&self, paper_id: String) -> Vec<PaperRemoteFile> {
        self.syncer.paper_files(&paper_id).into_iter().map(|(name, size, local)| PaperRemoteFile { name, size, local }).collect()
    }

    /// Downloads the given kinds of a paper if they are not here yet (blocking). Returns how many
    /// files were fetched.
    pub fn fetch_paper(&self, paper_id: String, kinds: Vec<FileKind>) -> Result<u32> {
        self.syncer.fetch_paper(&paper_id, &kinds)
    }

    pub fn reset_state(&self) -> Result<()> {
        self.syncer.reset_state()
    }
}
