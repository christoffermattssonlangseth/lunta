//! Sync of a Lunta library through a remote folder (Google Drive), for devices that have no synced
//! folder of their own (Android, Linux, ...) or whose owner prefers the API.
//!
//! ```text
//!  local working folder  <--- Syncer --->  RemoteStore  (GoogleDrive | LocalDirRemote)
//!  (opened by Library)                      a folder named "Lunta" with the same layout
//! ```
//!
//! The remote has exactly the layout described in `store.rs`, so a Mac that keeps its library folder
//! inside "Google Drive for desktop" and API-based devices share one library. See `syncer.rs` for the
//! merge rules, `gdrive.rs` for the Drive client and `auth.rs` for OAuth.
//!
//! Everything here is plain owned types (String, u64, i64, Vec, Arc<dyn Trait>) so it can be exported
//! through UniFFI later; it is deliberately not exported yet.

pub mod auth;
pub mod gdrive;
pub mod remote;
pub mod syncer;
pub mod util;

pub use auth::{Authenticator, FileTokenStore, MemoryTokenStore, OAuthClient, Scope, TokenStore, Tokens};
pub use gdrive::{DriveRoot, GoogleDrive};
pub use remote::{Listing, LocalDirRemote, RemoteFile, RemoteStore};
pub use syncer::{SyncOptions, SyncReport, SyncStatus, Syncer};

#[cfg(test)]
mod mock;
#[cfg(test)]
mod tests;
