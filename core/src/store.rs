//! Sync-safe library storage.
//!
//! Layout (the root can live in iCloud Drive):
//!
//! ```text
//! <root>/papers/<paper-id>/paper.pdf | book.epub | fulltext.txt   write-once blobs
//! <root>/ops/<device-id>.jsonl                                      append-only change log, one per device
//! ```
//!
//! Every mutable fact is an `Op` in the writing device's own log, so two devices never write the
//! same file and iCloud never has to resolve a conflict. State is rebuilt by replaying all logs in
//! timestamp order; the last write wins per field.

use crate::model::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum OpKind {
    AddPaper { paper: String, meta: PaperMeta },
    SetMeta { paper: String, meta: PaperMeta },
    DeletePaper { paper: String },
    SetRead { paper: String, read: bool },
    SetStarred { paper: String, starred: bool },
    /// On the Must Read list (older versions skip this op).
    SetMustRead { paper: String, must_read: bool },
    Tag { paper: String, tag: String, present: bool },
    CollectionUpsert { id: String, name: String, parent: Option<String> },
    CollectionDelete { id: String },
    Member { collection: String, paper: String, present: bool },
    AnnotationUpsert { annotation: Annotation },
    AnnotationDelete { paper: String, id: String },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Op {
    pub at: i64,
    pub device: String,
    #[serde(flatten)]
    pub kind: OpKind,
}

#[derive(Clone, Debug, Default)]
pub struct PaperState {
    pub meta: PaperMeta,
    pub added: i64,
    pub modified: i64,
    pub read: bool,
    pub starred: bool,
    pub must_read: bool,
    pub tags: BTreeSet<String>,
    pub collections: BTreeSet<String>,
}

#[derive(Clone, Debug, Default)]
pub struct State {
    pub papers: BTreeMap<String, PaperState>,
    pub collections: BTreeMap<String, (String, Option<String>)>,
    pub annotations: BTreeMap<String, BTreeMap<String, Annotation>>, // paper -> id -> annotation
    pub deleted: BTreeSet<String>,
}

impl State {
    fn apply(&mut self, op: &Op) {
        match &op.kind {
            OpKind::AddPaper { paper, meta } => {
                if self.deleted.contains(paper) {
                    return;
                }
                let p = self.papers.entry(paper.clone()).or_default();
                if p.added == 0 {
                    p.added = op.at;
                    p.meta = meta.clone();
                }
                p.modified = p.modified.max(op.at);
            }
            OpKind::SetMeta { paper, meta } => {
                if let Some(p) = self.papers.get_mut(paper) {
                    p.meta = meta.clone();
                    p.modified = op.at;
                }
            }
            OpKind::DeletePaper { paper } => {
                self.papers.remove(paper);
                self.annotations.remove(paper);
                self.deleted.insert(paper.clone());
            }
            OpKind::SetRead { paper, read } => {
                if let Some(p) = self.papers.get_mut(paper) {
                    p.read = *read;
                }
            }
            OpKind::SetStarred { paper, starred } => {
                if let Some(p) = self.papers.get_mut(paper) {
                    p.starred = *starred;
                }
            }
            OpKind::SetMustRead { paper, must_read } => {
                if let Some(p) = self.papers.get_mut(paper) {
                    p.must_read = *must_read;
                }
            }
            OpKind::Tag { paper, tag, present } => {
                if let Some(p) = self.papers.get_mut(paper) {
                    if *present {
                        p.tags.insert(tag.clone());
                    } else {
                        p.tags.remove(tag);
                    }
                }
            }
            OpKind::CollectionUpsert { id, name, parent } => {
                self.collections.insert(id.clone(), (name.clone(), parent.clone()));
            }
            OpKind::CollectionDelete { id } => {
                self.collections.remove(id);
                for p in self.papers.values_mut() {
                    p.collections.remove(id);
                }
            }
            OpKind::Member { collection, paper, present } => {
                if let Some(p) = self.papers.get_mut(paper) {
                    if *present {
                        p.collections.insert(collection.clone());
                    } else {
                        p.collections.remove(collection);
                    }
                }
            }
            OpKind::AnnotationUpsert { annotation } => {
                if self.papers.contains_key(&annotation.paper_id) {
                    self.annotations
                        .entry(annotation.paper_id.clone())
                        .or_default()
                        .insert(annotation.id.clone(), annotation.clone());
                }
            }
            OpKind::AnnotationDelete { paper, id } => {
                if let Some(m) = self.annotations.get_mut(paper) {
                    m.remove(id);
                }
            }
        }
    }

    /// Drops memberships pointing at collections that no longer exist.
    fn finish(&mut self) {
        let ids: BTreeSet<String> = self.collections.keys().cloned().collect();
        for p in self.papers.values_mut() {
            p.collections.retain(|c| ids.contains(c));
        }
    }
}

pub struct Store {
    pub root: PathBuf,
    pub device: String,
    /// (size, mtime) of every log at the last replay; used to detect changes from other devices.
    seen: HashMap<PathBuf, (u64, i64)>,
}

impl Store {
    pub fn open(root: &Path, device: &str) -> Result<Store> {
        if device.is_empty() || device.contains('/') || device.starts_with('.') {
            return Err(CoreError::Invalid("invalid device id".into()));
        }
        fs::create_dir_all(root.join("papers"))?;
        fs::create_dir_all(root.join("ops"))?;
        Ok(Store { root: root.to_path_buf(), device: device.to_string(), seen: HashMap::new() })
    }

    pub fn paper_dir(&self, id: &str) -> PathBuf {
        self.root.join("papers").join(id)
    }

    pub fn file_path(&self, id: &str, kind: FileKind) -> PathBuf {
        self.paper_dir(id).join(kind.file_name())
    }

    /// True if the file exists on this device or as an iCloud placeholder (".name.icloud").
    pub fn has_file(&self, id: &str, kind: FileKind) -> bool {
        let dir = self.paper_dir(id);
        dir.join(kind.file_name()).exists() || dir.join(format!(".{}.icloud", kind.file_name())).exists()
    }

    /// Writes a blob atomically (temp file + rename) so a half-written file never syncs.
    pub fn write_file(&self, id: &str, kind: FileKind, data: &[u8]) -> Result<()> {
        let dir = self.paper_dir(id);
        fs::create_dir_all(&dir)?;
        let tmp = dir.join(format!(".{}.tmp-{}", kind.file_name(), self.device));
        fs::write(&tmp, data)?;
        fs::rename(&tmp, dir.join(kind.file_name()))?;
        Ok(())
    }

    pub fn append(&mut self, kind: OpKind) -> Result<Op> {
        let op = Op { at: now_ms(), device: self.device.clone(), kind };
        let line = serde_json::to_string(&op).map_err(|e| CoreError::Invalid(e.to_string()))?;
        let path = self.log_path();
        let mut f = fs::OpenOptions::new().create(true).append(true).open(&path)?;
        writeln!(f, "{line}")?;
        drop(f);
        // our own write is not a "change from elsewhere"
        if let Some(sig) = file_sig(&path) {
            self.seen.insert(path, sig);
        }
        Ok(op)
    }

    fn log_path(&self) -> PathBuf {
        self.root.join("ops").join(format!("{}.jsonl", self.device))
    }

    /// Log files (excluding iCloud placeholders), plus whether any changed since the last call.
    fn logs(&self) -> Result<(Vec<PathBuf>, bool)> {
        let mut out = Vec::new();
        let mut changed = false;
        for e in fs::read_dir(self.root.join("ops"))? {
            let p = e?.path();
            if p.extension().map(|x| x == "jsonl").unwrap_or(false)
                && !p.file_name().unwrap().to_string_lossy().starts_with('.')
            {
                let sig = file_sig(&p);
                if sig.is_none() || self.seen.get(&p).copied() != sig {
                    changed = true;
                }
                out.push(p);
            }
        }
        if out.len() != self.seen.len() {
            changed = true;
        }
        out.sort();
        Ok((out, changed))
    }

    pub fn needs_replay(&self) -> Result<bool> {
        Ok(self.logs()?.1)
    }

    /// Replays every device log into a fresh state.
    pub fn replay(&mut self) -> Result<State> {
        let (logs, _) = self.logs()?;
        let mut ops: Vec<Op> = Vec::new();
        self.seen.clear();
        for p in logs {
            let text = fs::read_to_string(&p)?;
            for line in text.lines() {
                // a partially synced last line is skipped and picked up on the next refresh
                if let Ok(op) = serde_json::from_str::<Op>(line) {
                    ops.push(op);
                }
            }
            if let Some(sig) = file_sig(&p) {
                self.seen.insert(p, sig);
            }
        }
        ops.sort_by(|a, b| (a.at, &a.device).cmp(&(b.at, &b.device)));
        let mut st = State::default();
        for op in &ops {
            st.apply(op);
        }
        st.finish();
        Ok(st)
    }

    /// iCloud placeholders (".name.icloud") for files that have not been downloaded yet.
    pub fn placeholders(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut dirs = vec![self.root.join("ops")];
        if let Ok(rd) = fs::read_dir(self.root.join("papers")) {
            dirs.extend(rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()));
        }
        for d in dirs {
            if let Ok(rd) = fs::read_dir(&d) {
                for e in rd.filter_map(|e| e.ok()) {
                    let n = e.file_name().to_string_lossy().to_string();
                    if n.starts_with('.') && n.ends_with(".icloud") {
                        out.push(e.path().to_string_lossy().to_string());
                    }
                }
            }
        }
        out
    }
}

fn file_sig(p: &Path) -> Option<(u64, i64)> {
    let m = fs::metadata(p).ok()?;
    let mt = m
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_millis() as i64;
    Some((m.len(), mt))
}
