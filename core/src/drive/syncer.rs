//! The Syncer: mirrors a library between a local working folder and a `RemoteStore`.
//!
//! # Rules
//!
//! Every path is classified by where it sits in the library layout (`store.rs`):
//!
//! | path                          | owner        | rule                                              |
//! |-------------------------------|--------------|---------------------------------------------------|
//! | `ops/<this device>.jsonl`     | this device  | uploaded whole when it grew; never overwritten by a download that would lose lines |
//! | `ops/<other>.jsonl`           | other device | download only; **never uploaded or modified here** |
//! | `positions/<this device>.json`| this device  | uploaded; merged per paper by newest `at` if the remote differs |
//! | `positions/<other>.json`      | other device | download only                                     |
//! | `papers/<id>/<file>`          | nobody       | three-way sync, deletes propagate (see below)     |
//! | `<name>.json` at the root     | nobody       | three-way sync, last writer wins by modified time |
//!
//! Anything else (dot files, other folders) is ignored on both sides.
//!
//! **Three-way**: the state file remembers, per path, the remote version (md5/size/time) and the local
//! (size, mtime) as of the last time they were identical. Only-local-changed uploads, only-remote-changed
//! downloads, both-changed is a conflict resolved by newest modification time (the loser's local copy is
//! saved under `.lunta-sync/conflicts/`; Drive keeps its own revisions). A file that was synced and is now
//! missing on one side was deleted there, so the deletion is applied to the other side unless that side has
//! changed it since (an edit beats a delete). A mass-deletion guard refuses to apply more than half of the
//! tracked files (or >10) at once, which protects against a wiped working folder or a bad listing.
//!
//! **Order**: paper files, then positions, then shared JSON, and op logs last, both ways, so a remote
//! reader never sees an op that refers to a paper file that has not arrived yet.
//!
//! **Incremental**: `RemoteStore::list(cursor)` returns only what changed (Drive `changes` API); the
//! remote file table lives in the state file. Local files are compared by (size, mtime) and hashed
//! only when that differs. State is written every few transfers, so an interrupted sync resumes with
//! at most a few files redone (which are recognised as identical by md5 and cost no transfer).
//!
//! **Lazy**: with `SyncOptions::lazy`, big paper files (PDF, EPUB, narration) are not downloaded by a
//! sync; `fetch_paper` / `fetch_file` get them on demand (e.g. when the Android app opens a paper).

use super::remote::*;
use super::util::*;
use crate::model::{now_ms, CoreError, FileKind, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub type ProgressFn = Arc<dyn Fn(&str) + Send + Sync>;

#[derive(Clone)]
pub struct SyncOptions {
    /// Where the sync bookkeeping lives (JSON). Keep it on the device, next to the working folder.
    pub state_path: PathBuf,
    /// Do not download PDFs/EPUBs/narration during a sync; fetch them on demand.
    pub lazy: bool,
    /// Called with a short line per transfer ("upload ops/x.jsonl"), e.g. for a progress display.
    pub progress: Option<ProgressFn>,
}

impl SyncOptions {
    pub fn new(state_path: impl Into<PathBuf>) -> SyncOptions {
        SyncOptions { state_path: state_path.into(), lazy: false, progress: None }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SyncReport {
    pub uploaded: u32,
    pub downloaded: u32,
    pub deleted_local: u32,
    pub deleted_remote: u32,
    /// Paths where both sides changed (resolved, see module docs).
    pub conflicts: Vec<String>,
    pub errors: Vec<String>,
    /// Big files left on the remote because of `lazy`.
    pub skipped_lazy: u32,
    /// Something in the working folder changed: the caller should `Library::refresh()`.
    pub changed: bool,
    pub started_ms: i64,
    pub finished_ms: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SyncStatus {
    pub remote: String,
    pub last_sync_ms: i64,
    pub tracked_files: u32,
    pub pending_uploads: u32,
    pub pending_downloads: u32,
    /// Remote files that exist only on the remote (lazy mode).
    pub not_downloaded: u32,
}

#[derive(Serialize, Deserialize, Default, Clone)]
struct Synced {
    remote: RemoteFile,
    local_size: u64,
    local_mtime: i64,
    /// False for files deliberately not downloaded (lazy).
    local: bool,
}

#[derive(Serialize, Deserialize, Default)]
struct SyncState {
    version: u32,
    cursor: Option<String>,
    last_sync_ms: i64,
    remote: BTreeMap<String, RemoteFile>,
    synced: BTreeMap<String, Synced>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    OwnLog,
    OwnPositions,
    ForeignLog,
    ForeignPositions,
    Paper,
    Shared,
}

impl Kind {
    fn order(self) -> u8 {
        match self {
            Kind::Paper => 0,
            Kind::OwnPositions | Kind::ForeignPositions => 1,
            Kind::Shared => 2,
            Kind::OwnLog | Kind::ForeignLog => 3,
        }
    }
    fn foreign(self) -> bool {
        matches!(self, Kind::ForeignLog | Kind::ForeignPositions)
    }
}

pub struct Syncer {
    root: PathBuf,
    device: String,
    remote: Box<dyn RemoteStore>,
    opts: SyncOptions,
    state: Mutex<SyncState>,
}

fn heavy(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or("");
    [FileKind::Pdf, FileKind::Epub, FileKind::Audio].iter().any(|k| k.file_name() == name)
}

enum Decision {
    Nothing,
    Upload,
    Download,
    DeleteLocal,
    DeleteRemote,
    /// Both sides already identical: just remember it.
    Adopt,
    Conflict { remote_wins: bool },
    MergeLog,
    MergePositions,
    Forget,
}

impl Syncer {
    /// `root`: the working folder (what `Library::open` gets). `device`: this device's id, the same
    /// one the Library uses, which decides which `ops/` and `positions/` files are ours.
    pub fn new(root: impl Into<PathBuf>, device: impl Into<String>, remote: Box<dyn RemoteStore>, opts: SyncOptions) -> Result<Syncer> {
        let device = device.into();
        if device.is_empty() || device.contains('/') || device.starts_with('.') {
            return Err(CoreError::Invalid("invalid device id".into()));
        }
        let state = match fs::read_to_string(&opts.state_path) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
            Err(_) => SyncState::default(),
        };
        let root = root.into();
        fs::create_dir_all(&root)?;
        Ok(Syncer { root, device, remote, opts, state: Mutex::new(state) })
    }

    fn classify(&self, path: &str) -> Option<Kind> {
        let parts: Vec<&str> = path.split('/').collect();
        if parts.iter().any(|p| p.is_empty() || p.starts_with('.') || *p == "..") {
            return None;
        }
        match parts.as_slice() {
            ["ops", n] => n.strip_suffix(".jsonl").map(|d| if d == self.device { Kind::OwnLog } else { Kind::ForeignLog }),
            ["positions", n] => n.strip_suffix(".json").map(|d| if d == self.device { Kind::OwnPositions } else { Kind::ForeignPositions }),
            ["papers", _, _] => Some(Kind::Paper),
            [n] if n.ends_with(".json") => Some(Kind::Shared),
            _ => None,
        }
    }

    fn abs(&self, rel: &str) -> PathBuf {
        rel.split('/').fold(self.root.clone(), |p, c| p.join(c))
    }

    fn say(&self, s: &str) {
        if let Some(f) = &self.opts.progress {
            f(s);
        }
    }

    fn save_state(&self, st: &SyncState) -> Result<()> {
        let json = serde_json::to_vec(st).map_err(|e| CoreError::Invalid(e.to_string()))?;
        write_atomic(&self.opts.state_path, &json)
    }

    fn scan_local(&self) -> BTreeMap<String, (u64, i64)> {
        let mut out = BTreeMap::new();
        let mut visit = |dir: PathBuf, prefix: String| {
            if let Ok(rd) = fs::read_dir(&dir) {
                for e in rd.flatten() {
                    let name = e.file_name().to_string_lossy().to_string();
                    let rel = if prefix.is_empty() { name } else { format!("{prefix}/{name}") };
                    if self.classify(&rel).is_some() {
                        if let Some(sig) = file_sig(&e.path()) {
                            out.insert(rel, sig);
                        }
                    }
                }
            }
        };
        visit(self.root.clone(), String::new());
        visit(self.root.join("ops"), "ops".into());
        visit(self.root.join("positions"), "positions".into());
        if let Ok(rd) = fs::read_dir(self.root.join("papers")) {
            for e in rd.flatten() {
                if e.path().is_dir() {
                    let id = e.file_name().to_string_lossy().to_string();
                    visit(e.path(), format!("papers/{id}"));
                }
            }
        }
        out
    }

    /// Offline summary (no network): what the next sync would transfer.
    pub fn status(&self) -> SyncStatus {
        let st = self.state.lock().unwrap();
        let local = self.scan_local();
        let mut s = SyncStatus { remote: self.remote.describe(), last_sync_ms: st.last_sync_ms, tracked_files: st.synced.len() as u32, ..Default::default() };
        for (p, sig) in &local {
            let k = self.classify(p).unwrap();
            if k.foreign() {
                continue;
            }
            match st.synced.get(p) {
                Some(e) if (e.local_size, e.local_mtime) == *sig => {}
                _ => s.pending_uploads += 1,
            }
        }
        for (p, r) in &st.remote {
            match st.synced.get(p) {
                Some(e) if !e.local => s.not_downloaded += 1,
                Some(e) if !e.remote.differs_from(r) => {}
                None if self.opts.lazy && heavy(p) && !local.contains_key(p) => s.not_downloaded += 1,
                _ => s.pending_downloads += 1,
            }
        }
        s
    }

    /// Runs one sync. Errors on single files go into the report and do not stop the run; the
    /// result is `Err` only if the remote cannot be listed or the state cannot be saved.
    pub fn sync_now(&self) -> Result<SyncReport> {
        let mut st = self.state.lock().unwrap();
        let mut rep = SyncReport { started_ms: now_ms(), ..Default::default() };

        // 1. what is on the remote now
        let listing = self.remote.list(st.cursor.as_deref())?;
        let mut dup_notes = Vec::new();
        if listing.full {
            if listing.files.is_empty() && !st.synced.is_empty() {
                return Err(CoreError::Invalid("the remote folder looks empty although files were synced before; refusing to continue (wrong folder or account?)".into()));
            }
            st.remote.clear();
        } else {
            // removed ids, plus files that came back under a new path (renamed on the remote)
            let mut gone: BTreeSet<&str> = listing.removed_ids.iter().map(|s| s.as_str()).collect();
            let moved: BTreeMap<&str, &str> = listing.files.iter().map(|f| (f.id.as_str(), f.path.as_str())).collect();
            gone.extend(moved.keys().copied());
            st.remote.retain(|path, f| !gone.contains(f.id.as_str()) || moved.get(f.id.as_str()) == Some(&path.as_str()));
        }
        for f in listing.files {
            if self.classify(&f.path).is_none() {
                continue;
            }
            if let Some(old) = st.remote.get(&f.path) {
                if old.id != f.id {
                    dup_notes.push(format!("{}: two files with this name on the remote; using the newer", f.path));
                    if old.modified_ms >= f.modified_ms {
                        continue;
                    }
                }
            }
            st.remote.insert(f.path.clone(), f);
        }
        st.cursor = listing.cursor;
        rep.errors.extend(dup_notes);

        // 2. decide per path
        let local = self.scan_local();
        let mut paths: BTreeSet<String> = local.keys().cloned().collect();
        paths.extend(st.remote.keys().cloned());
        paths.extend(st.synced.keys().filter(|p| self.classify(p).is_some()).cloned());
        let mut order: Vec<(u8, String)> = paths.into_iter().map(|p| (self.classify(&p).unwrap().order(), p)).collect();
        order.sort();

        let mut plan: Vec<(String, Decision)> = Vec::new();
        for (_, p) in order {
            match self.decide(&mut st, &p, local.get(&p).copied(), &mut rep) {
                Ok(Decision::Nothing) => {}
                Ok(d) => plan.push((p, d)),
                Err(e) => rep.errors.push(format!("{p}: {e}")),
            }
        }

        // mass-deletion guard
        let dels = plan.iter().filter(|(_, d)| matches!(d, Decision::DeleteLocal | Decision::DeleteRemote)).count();
        let limit = (st.synced.len() / 2).max(10);
        if dels > limit {
            rep.errors.push(format!("{dels} deletions would be applied at once; skipped as a precaution (working folder or remote wiped?)"));
            plan.retain(|(_, d)| !matches!(d, Decision::DeleteLocal | Decision::DeleteRemote));
        }

        // 3. act
        let mut since_save = 0;
        for (p, d) in plan {
            if let Err(e) = self.act(&mut st, &p, d, &mut rep) {
                rep.errors.push(format!("{p}: {e}"));
            }
            since_save += 1;
            if since_save >= 25 {
                self.save_state(&st)?;
                since_save = 0;
            }
        }
        if rep.errors.iter().all(|e| !e.contains("looks empty")) {
            st.last_sync_ms = now_ms();
        }
        rep.finished_ms = now_ms();
        self.save_state(&st)?;
        Ok(rep)
    }

    fn local_changed(&self, p: &str, s: &mut Synced, l: (u64, i64)) -> bool {
        if (s.local_size, s.local_mtime) == l {
            return false;
        }
        // touched or edited? compare content with the version we last synced
        if let Some(md5) = &s.remote.md5 {
            if l.0 == s.remote.size && md5_file(&self.abs(p)).ok().as_ref() == Some(md5) {
                s.local_size = l.0;
                s.local_mtime = l.1;
                return false;
            }
        }
        true
    }

    fn decide(&self, st: &mut SyncState, p: &str, l: Option<(u64, i64)>, rep: &mut SyncReport) -> Result<Decision> {
        let kind = self.classify(p).unwrap();
        let r = st.remote.get(p).cloned();
        let mut s = st.synced.get(p).cloned();
        let d = self.decide_inner(kind, p, l, r.as_ref(), s.as_mut(), rep)?;
        if let Some(s) = s {
            st.synced.insert(p.to_string(), s); // may carry a refreshed local signature
        }
        Ok(d)
    }

    fn decide_inner(&self, kind: Kind, p: &str, l: Option<(u64, i64)>, r: Option<&RemoteFile>, s: Option<&mut Synced>, rep: &mut SyncReport) -> Result<Decision> {
        let lazy_skip = self.opts.lazy && kind == Kind::Paper && heavy(p);
        Ok(match (l, r, s) {
            (None, None, None) => Decision::Nothing,
            (None, None, Some(_)) => Decision::Forget,
            // ---- only here
            (Some(_), None, None) => {
                if kind.foreign() {
                    Decision::Nothing // never publish another device's log
                } else {
                    Decision::Upload
                }
            }
            (Some(l), None, Some(s)) => {
                if kind.foreign() {
                    Decision::Forget
                } else if matches!(kind, Kind::OwnLog | Kind::OwnPositions) || self.local_changed(p, s, l) {
                    Decision::Upload // our own files are restored; an edit beats a remote delete
                } else {
                    Decision::DeleteLocal
                }
            }
            // ---- only on the remote
            (None, Some(_), None) => {
                if lazy_skip {
                    rep.skipped_lazy += 1;
                    Decision::Nothing
                } else {
                    Decision::Download
                }
            }
            (None, Some(r), Some(s)) => {
                if !s.local {
                    if lazy_skip {
                        Decision::Nothing
                    } else {
                        Decision::Download
                    }
                } else if kind.foreign() || matches!(kind, Kind::OwnLog | Kind::OwnPositions) || r.differs_from(&s.remote) {
                    Decision::Download // restore, or the remote changed after we last saw it
                } else {
                    Decision::DeleteRemote // deleted here
                }
            }
            // ---- on both
            (Some(l), Some(r), None) => {
                let same = r.md5.is_some() && l.0 == r.size && md5_file(&self.abs(p)).ok() == r.md5;
                if same {
                    Decision::Adopt
                } else {
                    match kind {
                        Kind::ForeignLog | Kind::ForeignPositions => Decision::Download,
                        Kind::OwnLog => Decision::MergeLog,
                        Kind::OwnPositions => Decision::MergePositions,
                        _ => Decision::Conflict { remote_wins: r.modified_ms >= l.1 },
                    }
                }
            }
            (Some(l), Some(r), Some(s)) => {
                let rchg = r.differs_from(&s.remote);
                match kind {
                    Kind::ForeignLog | Kind::ForeignPositions => {
                        // the local copy of another device's file only ever mirrors the remote
                        if rchg || !s.local || self.local_changed(p, s, l) {
                            Decision::Download
                        } else {
                            Decision::Nothing
                        }
                    }
                    Kind::OwnLog | Kind::OwnPositions => {
                        let lchg = self.local_changed(p, s, l);
                        match (lchg, rchg) {
                            (false, false) => Decision::Nothing,
                            (true, false) => Decision::Upload,
                            _ => {
                                if kind == Kind::OwnLog {
                                    Decision::MergeLog
                                } else {
                                    Decision::MergePositions
                                }
                            }
                        }
                    }
                    _ => {
                        let lchg = self.local_changed(p, s, l);
                        match (lchg, rchg) {
                            (false, false) => Decision::Nothing,
                            (true, false) => Decision::Upload,
                            (false, true) => Decision::Download,
                            (true, true) => {
                                if r.md5.is_some() && md5_file(&self.abs(p)).ok() == r.md5 {
                                    Decision::Adopt
                                } else {
                                    Decision::Conflict { remote_wins: r.modified_ms >= l.1 }
                                }
                            }
                        }
                    }
                }
            }
        })
    }

    fn act(&self, st: &mut SyncState, p: &str, d: Decision, rep: &mut SyncReport) -> Result<()> {
        match d {
            Decision::Nothing => {}
            Decision::Forget => {
                st.synced.remove(p);
            }
            Decision::Adopt => {
                let r = st.remote[p].clone();
                self.record(st, p, r, true);
            }
            Decision::Upload => self.upload(st, p, rep)?,
            Decision::Download => self.download(st, p, rep)?,
            Decision::DeleteLocal => {
                self.say(&format!("delete local {p}"));
                let _ = fs::remove_file(self.abs(p));
                if let Some(dir) = self.abs(p).parent() {
                    let _ = fs::remove_dir(dir); // only if now empty
                }
                st.synced.remove(p);
                rep.deleted_local += 1;
                rep.changed = true;
            }
            Decision::DeleteRemote => {
                self.say(&format!("delete remote {p}"));
                if let Some(r) = st.remote.get(p).cloned() {
                    self.remote.delete(&r)?;
                }
                st.remote.remove(p);
                st.synced.remove(p);
                rep.deleted_remote += 1;
            }
            Decision::Conflict { remote_wins } => {
                rep.conflicts.push(p.to_string());
                if remote_wins {
                    self.keep_conflict_copy(p);
                    self.download(st, p, rep)?;
                } else {
                    self.upload(st, p, rep)?;
                }
            }
            Decision::MergeLog => self.merge_log(st, p, rep)?,
            Decision::MergePositions => self.merge_positions(st, p, rep)?,
        }
        Ok(())
    }

    fn keep_conflict_copy(&self, p: &str) {
        let dir = self.root.join(".lunta-sync").join("conflicts");
        if fs::create_dir_all(&dir).is_ok() {
            let _ = fs::copy(self.abs(p), dir.join(format!("{}-{}", now_ms(), p.replace('/', "_"))));
        }
    }

    /// Remembers that local and `r` are the same now.
    fn record(&self, st: &mut SyncState, p: &str, r: RemoteFile, local_present: bool) {
        let (size, mtime) = if local_present { file_sig(&self.abs(p)).unwrap_or((0, 0)) } else { (0, 0) };
        st.remote.insert(p.to_string(), r.clone());
        st.synced.insert(p.to_string(), Synced { remote: r, local_size: size, local_mtime: mtime, local: local_present });
    }

    fn upload(&self, st: &mut SyncState, p: &str, rep: &mut SyncReport) -> Result<()> {
        let abs = self.abs(p);
        let sig = file_sig(&abs).ok_or_else(|| CoreError::NotFound(p.into()))?;
        self.say(&format!("upload {p}"));
        let existing = st.remote.get(p).cloned();
        let up = self.remote.upload(p, &abs, sig.1, existing.as_ref())?;
        // signature taken *before* the upload: a log appended meanwhile is picked up next time
        st.remote.insert(p.to_string(), up.clone());
        st.synced.insert(p.to_string(), Synced { remote: up, local_size: sig.0, local_mtime: sig.1, local: true });
        rep.uploaded += 1;
        Ok(())
    }

    /// Downloads `p` next to its destination and renames it into place.
    fn fetch_to(&self, r: &RemoteFile, tmp_name: &str) -> Result<PathBuf> {
        let dest = self.abs(&r.path);
        let tmp = dest.with_file_name(format!(".{tmp_name}.part-{}", self.device));
        if let Some(d) = dest.parent() {
            fs::create_dir_all(d)?;
        }
        let res = (|| {
            self.remote.download(r, &tmp)?;
            let got = fs::metadata(&tmp)?.len();
            if got != r.size {
                return Err(CoreError::Network(format!("{}: got {got} bytes, expected {}", r.path, r.size)));
            }
            if let Some(m) = &r.md5 {
                if &md5_file(&tmp)? != m {
                    return Err(CoreError::Network(format!("{}: checksum mismatch", r.path)));
                }
            }
            Ok(())
        })();
        if let Err(e) = res {
            let _ = fs::remove_file(&tmp);
            return Err(e);
        }
        Ok(tmp)
    }

    fn download(&self, st: &mut SyncState, p: &str, rep: &mut SyncReport) -> Result<()> {
        let r = st.remote.get(p).cloned().ok_or_else(|| CoreError::NotFound(p.into()))?;
        self.say(&format!("download {p}"));
        let name = p.rsplit('/').next().unwrap_or(p);
        let tmp = self.fetch_to(&r, name)?;
        set_mtime(&tmp, r.modified_ms);
        fs::rename(&tmp, self.abs(p))?;
        self.record(st, p, r, true);
        rep.downloaded += 1;
        rep.changed = true;
        Ok(())
    }

    fn merge_log(&self, st: &mut SyncState, p: &str, rep: &mut SyncReport) -> Result<()> {
        let r = st.remote.get(p).cloned().ok_or_else(|| CoreError::NotFound(p.into()))?;
        let name = p.rsplit('/').next().unwrap_or(p);
        let tmp = self.fetch_to(&r, name)?;
        let remote_text = fs::read_to_string(&tmp);
        let _ = fs::remove_file(&tmp);
        let remote_text = remote_text?;
        let abs = self.abs(p);
        let mut local_text = fs::read_to_string(&abs)?;
        let have: BTreeSet<&str> = local_text.lines().collect();
        let missing: Vec<&str> = remote_text.lines().filter(|l| !l.trim().is_empty() && !have.contains(l)).collect();
        if !missing.is_empty() {
            // the remote has lines this install lost (reinstalled with the same device id, restored
            // backup): add them back, never drop anything
            if !local_text.is_empty() && !local_text.ends_with('\n') {
                local_text.push('\n');
            }
            for l in &missing {
                local_text.push_str(l);
                local_text.push('\n');
            }
            write_atomic(&abs, local_text.as_bytes())?;
            rep.changed = true;
            rep.conflicts.push(format!("{p} (merged)"));
        }
        if md5_file(&abs)?.as_str() == r.md5.as_deref().unwrap_or("") {
            self.record(st, p, r, true);
            Ok(())
        } else {
            self.upload(st, p, rep)
        }
    }

    fn merge_positions(&self, st: &mut SyncState, p: &str, rep: &mut SyncReport) -> Result<()> {
        let r = st.remote.get(p).cloned().ok_or_else(|| CoreError::NotFound(p.into()))?;
        let tmp = self.fetch_to(&r, "positions")?;
        let remote_text = fs::read_to_string(&tmp);
        let _ = fs::remove_file(&tmp);
        let abs = self.abs(p);
        let parse = |t: &str| serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(t).ok();
        let at = |v: &serde_json::Value| v.get("at").and_then(|a| a.as_f64()).unwrap_or(0.0);
        let mut merged = parse(&fs::read_to_string(&abs)?).unwrap_or_default();
        if let Some(remote) = parse(&remote_text?) {
            for (k, v) in remote {
                if merged.get(&k).is_none_or(|m| at(&v) > at(m)) {
                    merged.insert(k, v);
                }
            }
        }
        let out = serde_json::to_vec(&merged).unwrap();
        if fs::read(&abs)? != out {
            write_atomic(&abs, &out)?;
            rep.changed = true;
            rep.conflicts.push(format!("{p} (merged)"));
        }
        self.upload(st, p, rep)
    }

    // ---- on-demand access (lazy mode, Android opening a paper) ----

    /// Remote files of one paper, as of the last sync: (file name, size, present locally).
    pub fn paper_files(&self, paper_id: &str) -> Vec<(String, u64, bool)> {
        let st = self.state.lock().unwrap();
        let prefix = format!("papers/{paper_id}/");
        st.remote
            .iter()
            .filter(|(p, _)| p.starts_with(&prefix))
            .map(|(p, r)| (p[prefix.len()..].to_string(), r.size, self.abs(p).exists()))
            .collect()
    }

    /// Downloads one file by its library-relative path (`papers/<id>/paper.pdf`) if it is not
    /// here yet (or is older than the remote). Returns the local path.
    pub fn fetch_file(&self, rel: &str) -> Result<PathBuf> {
        if self.classify(rel) != Some(Kind::Paper) {
            return Err(CoreError::Invalid(format!("not a paper file: {rel}")));
        }
        let mut st = self.state.lock().unwrap();
        let r = st.remote.get(rel).cloned().ok_or_else(|| CoreError::NotFound(format!("{rel} is not on the remote (sync first?)")))?;
        let abs = self.abs(rel);
        let up_to_date = abs.exists() && st.synced.get(rel).is_some_and(|s| s.local && !s.remote.differs_from(&r));
        if !up_to_date {
            let mut rep = SyncReport::default();
            self.download(&mut st, rel, &mut rep)?;
            self.save_state(&st)?;
        }
        Ok(abs)
    }

    /// Makes sure the given kinds of a paper are on this device (all remote files if `kinds` is
    /// empty). Returns how many files were downloaded.
    pub fn fetch_paper(&self, paper_id: &str, kinds: &[FileKind]) -> Result<u32> {
        if paper_id.is_empty() || paper_id.contains('/') || paper_id.starts_with('.') {
            return Err(CoreError::Invalid("bad paper id".into()));
        }
        let want: Vec<String> = self
            .paper_files(paper_id)
            .into_iter()
            .filter(|(name, _, _)| kinds.is_empty() || kinds.iter().any(|k| k.file_name() == name))
            .map(|(name, _, _)| format!("papers/{paper_id}/{name}"))
            .collect();
        let mut n = 0;
        for rel in want {
            let had = self.abs(&rel).exists();
            let before = self.state.lock().unwrap().synced.get(&rel).map(|s| s.remote.clone());
            self.fetch_file(&rel)?;
            let after = self.state.lock().unwrap().synced.get(&rel).map(|s| s.remote.clone());
            if !had || before != after {
                n += 1;
            }
        }
        Ok(n)
    }

    /// Forgets the sync bookkeeping (next sync re-compares everything; nothing is deleted).
    pub fn reset_state(&self) -> Result<()> {
        let mut st = self.state.lock().unwrap();
        *st = SyncState::default();
        self.save_state(&st)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}
