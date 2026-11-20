//! Sync of a Lunta library through a remote folder (Google Drive), for devices that have no synced
//! folder of their own (Android, Linux, ...) or whose owner prefers the API.
//!
//! ```text
//!  local working folder  <--- Syncer --->  RemoteStore  (GoogleDrive | LocalDirRemote)
//!  (opened by Library)                      a folder named "Lunta" with the same layout
//! ```
//!
//! The remote has exactly the layout described in `store.rs`. Access is Google's `drive.file` scope:
//! only what this Cloud project's clients created (or the user picked) is visible, so files another
//! app (Google Drive for desktop) adds are not; `gdrive.rs` adopts them while an old full-scope token
//! lasts. See `syncer.rs` for the merge rules, `gdrive.rs` for the Drive client and `auth.rs` for OAuth.
//!
//! The plain types are wrapped for UniFFI in `ffi.rs` (sign-in and the Syncer), which the Android app uses.

pub mod auth;
pub mod ffi;
pub mod gdrive;
pub mod remote;
pub mod syncer;
pub mod util;

pub use auth::{Authenticator, FileTokenStore, MemoryTokenStore, OAuthClient, Scope, TokenStore, Tokens};
pub use gdrive::{is_folder_not_visible, DriveRoot, GoogleDrive, FOLDER_NOT_VISIBLE};
pub use remote::{Listing, LocalDirRemote, RemoteFile, RemoteStore};
pub use syncer::{SyncOptions, SyncReport, SyncStatus, Syncer};

#[cfg(test)]
mod mock;
#[cfg(test)]
mod tests;
