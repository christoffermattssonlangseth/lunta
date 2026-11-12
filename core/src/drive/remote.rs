//! The remote side of the sync: a folder tree that holds the library layout.

use super::util::*;
use crate::model::{CoreError, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// One file on the remote. `path` is relative to the library folder, with `/` separators
/// (`ops/mac.jsonl`, `papers/<id>/paper.pdf`, `smart-folders.json`).
#[derive(Clone, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct RemoteFile {
    pub path: String,
    /// Backend identifier (Drive file id; the path for LocalDirRemote).
    pub id: String,
    pub size: u64,
    /// Modification time, ms since the epoch.
    pub modified_ms: i64,
    pub md5: Option<String>,
}

impl RemoteFile {
    /// True if `other` is a different version of the same file (content hash when both have one,
    /// otherwise size and time).
    pub fn differs_from(&self, other: &RemoteFile) -> bool {
        match (&self.md5, &other.md5) {
            (Some(a), Some(b)) => a != b,
            _ => self.size != other.size || self.modified_ms != other.modified_ms,
        }
    }
}

/// What `RemoteStore::list` found.
#[derive(Clone, Debug, Default)]
pub struct Listing {
    /// Files that are new or changed since `cursor` (every file when `full`).
    pub files: Vec<RemoteFile>,
    /// Ids of files deleted since `cursor` (empty when `full`: absence from `files` means deleted).
    pub removed_ids: Vec<String>,
    /// True if `files` is the complete tree rather than a delta.
    pub full: bool,
    /// Opaque; hand it back to the next `list` call to get only what changed.
    pub cursor: Option<String>,
}

pub trait RemoteStore: Send + Sync {
    /// Human-readable description for status output.
    fn describe(&self) -> String;
    /// Lists the tree, or what changed since `cursor`. A backend that cannot honour a cursor
    /// (expired, unknown) returns a full listing instead.
    fn list(&self, cursor: Option<&str>) -> Result<Listing>;
    /// Downloads to `dest` (the caller renames it into place).
    fn download(&self, file: &RemoteFile, dest: &Path) -> Result<()>;
    /// Creates or replaces `path` with the contents of `local`, stamping `modified_ms` as its time.
    /// `existing` is the known remote file to replace, if any.
    fn upload(&self, path: &str, local: &Path, modified_ms: i64, existing: Option<&RemoteFile>) -> Result<RemoteFile>;
    /// Deletes (Drive: moves to the trash).
    fn delete(&self, file: &RemoteFile) -> Result<()>;
}

/// A plain folder acting as the remote. Also what "Google Drive for desktop" looks like to a Mac
/// device, so tests let a `Library` write straight into it.
pub struct LocalDirRemote {
    pub root: PathBuf,
}

impl LocalDirRemote {
    pub fn new(root: impl Into<PathBuf>) -> LocalDirRemote {
        let root = root.into();
        let _ = fs::create_dir_all(&root);
        LocalDirRemote { root }
    }

    fn abs(&self, rel: &str) -> Result<PathBuf> {
        if rel.split('/').any(|c| c.is_empty() || c == "." || c == "..") {
            return Err(CoreError::Invalid(format!("bad remote path {rel}")));
        }
        Ok(self.root.join(rel))
    }

    fn walk(&self, dir: &Path, prefix: &str, out: &mut Vec<RemoteFile>) -> Result<()> {
        let Ok(rd) = fs::read_dir(dir) else { return Ok(()) };
        for e in rd {
            let e = e?;
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let rel = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
            let p = e.path();
            if p.is_dir() {
                self.walk(&p, &rel, out)?;
            } else if let Some((size, modified_ms)) = file_sig(&p) {
                out.push(RemoteFile { path: rel.clone(), id: rel, size, modified_ms, md5: Some(md5_file(&p)?) });
            }
        }
        Ok(())
    }
}

impl RemoteStore for LocalDirRemote {
    fn describe(&self) -> String {
        format!("folder {}", self.root.display())
    }

    fn list(&self, _cursor: Option<&str>) -> Result<Listing> {
        let mut files = Vec::new();
        self.walk(&self.root, "", &mut files)?;
        files.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(Listing { files, removed_ids: vec![], full: true, cursor: None })
    }

    fn download(&self, file: &RemoteFile, dest: &Path) -> Result<()> {
        let src = self.abs(&file.id)?;
        if let Some(d) = dest.parent() {
            fs::create_dir_all(d)?;
        }
        fs::copy(&src, dest).map_err(|e| CoreError::NotFound(format!("{}: {e}", file.path)))?;
        Ok(())
    }

    fn upload(&self, path: &str, local: &Path, modified_ms: i64, _existing: Option<&RemoteFile>) -> Result<RemoteFile> {
        let dest = self.abs(path)?;
        if let Some(d) = dest.parent() {
            fs::create_dir_all(d)?;
        }
        let tmp = dest.with_file_name(format!(".{}.upload", dest.file_name().unwrap().to_string_lossy()));
        fs::copy(local, &tmp)?;
        set_mtime(&tmp, modified_ms);
        fs::rename(&tmp, &dest)?;
        let (size, modified_ms) = file_sig(&dest).ok_or_else(|| CoreError::Io("upload vanished".into()))?;
        Ok(RemoteFile { path: path.into(), id: path.into(), size, modified_ms, md5: Some(md5_file(&dest)?) })
    }

    fn delete(&self, file: &RemoteFile) -> Result<()> {
        let p = self.abs(&file.id)?;
        fs::remove_file(&p)?;
        let mut d = p.parent().map(|d| d.to_path_buf());
        while let Some(dir) = d {
            if dir == self.root || fs::remove_dir(&dir).is_err() {
                break;
            }
            d = dir.parent().map(|d| d.to_path_buf());
        }
        Ok(())
    }
}
