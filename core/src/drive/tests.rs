//! Offline tests: a folder acts as the remote; the "Mac" opens a Library straight on it (what Google
//! Drive for desktop gives), API devices own a working folder and sync through `Syncer`.

use super::*;
use crate::model::*;
use crate::Library;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

fn meta(title: &str, doi: &str) -> PaperMeta {
    PaperMeta { title: title.into(), authors: vec!["A. Author".into()], date: "2024".into(), doi: doi.into(), ..Default::default() }
}

fn note(paper: &str, text: &str) -> Annotation {
    Annotation {
        id: String::new(),
        paper_id: paper.into(),
        kind: AnnotationKind::Highlight,
        color: "yellow".into(),
        location: "epub:ch1".into(),
        position: "{}".into(),
        quote: "quote".into(),
        prefix: String::new(),
        suffix: String::new(),
        note: text.into(),
        created: 0,
        modified: 0,
    }
}

struct World {
    tmp: tempfile::TempDir,
    remote: PathBuf,
}

struct Dev {
    lib: Arc<Library>,
    syncer: Option<Syncer>,
    root: PathBuf,
}

impl World {
    fn new() -> World {
        let tmp = tempfile::tempdir().unwrap();
        let remote = tmp.path().join("remote");
        fs::create_dir_all(&remote).unwrap();
        World { tmp, remote }
    }

    /// A Mac with the library folder inside Google Drive for desktop: no Syncer, writes into the remote.
    fn mac(&self) -> Dev {
        let cache = self.tmp.path().join("cache-mac");
        let lib = Library::open(self.remote.to_string_lossy().into(), cache.to_string_lossy().into(), "mac".into()).unwrap();
        Dev { lib, syncer: None, root: self.remote.clone() }
    }

    fn api(&self, name: &str) -> Dev {
        self.api_with(name, Box::new(LocalDirRemote::new(&self.remote)), false)
    }

    fn api_with(&self, name: &str, remote: Box<dyn RemoteStore>, lazy: bool) -> Dev {
        let root = self.tmp.path().join(format!("lib-{name}"));
        let cache = self.tmp.path().join(format!("cache-{name}"));
        let lib = Library::open(root.to_string_lossy().into(), cache.to_string_lossy().into(), name.into()).unwrap();
        let mut opts = SyncOptions::new(self.tmp.path().join(format!("state-{name}.json")));
        opts.lazy = lazy;
        Dev { lib, syncer: Some(Syncer::new(&root, name, remote, opts).unwrap()), root }
    }
}

impl Dev {
    fn sync(&self) -> SyncReport {
        let r = self.syncer.as_ref().unwrap().sync_now().unwrap();
        assert!(r.errors.is_empty(), "sync errors: {:?}", r.errors);
        self.lib.refresh().unwrap();
        r
    }
    fn titles(&self) -> Vec<String> {
        let mut v: Vec<String> = self.lib.papers(PaperFilter::default(), SortKey::Title).into_iter().map(|p| p.meta.title).collect();
        v.sort();
        v
    }
    fn refresh(&self) {
        self.lib.refresh().unwrap();
    }
}

fn read(p: &Path) -> Vec<u8> {
    fs::read(p).unwrap()
}

fn set_time(p: &Path, ms: i64) {
    util::set_mtime(p, ms);
}

#[test]
fn mac_adds_api_downloads_and_writes_back() {
    let w = World::new();
    let mac = w.mac();
    let a = mac.lib.add_paper(meta("Alpha", "10.1/a"), Some(b"%PDF alpha".to_vec()), Some(b"epub alpha".to_vec()), Some("alpha text".into())).unwrap();
    mac.lib.save_annotation(note(&a.id, "mac note")).unwrap();

    let ph = w.api("phone");
    let r = ph.sync();
    assert!(r.downloaded >= 4 && r.uploaded == 0 && r.changed, "{r:?}");
    assert_eq!(ph.titles(), vec!["Alpha"]);
    assert_eq!(ph.lib.annotations(a.id.clone()).len(), 1);
    assert_eq!(read(&ph.root.join(format!("papers/{}/paper.pdf", a.id))), b"%PDF alpha");

    // phone edits: star, new annotation, new paper with files
    ph.lib.set_starred(a.id.clone(), true).unwrap();
    ph.lib.save_annotation(note(&a.id, "phone note")).unwrap();
    let b = ph.lib.add_paper(meta("Beta", "10.1/b"), Some(b"%PDF beta".to_vec()), None, None).unwrap();
    let r = ph.sync();
    assert_eq!(r.uploaded, 2, "{r:?}"); // Beta's pdf and the phone's log
    assert!(w.remote.join("ops/phone.jsonl").exists());
    assert!(w.remote.join(format!("papers/{}/paper.pdf", b.id)).exists());

    mac.refresh();
    assert_eq!(mac.titles(), vec!["Alpha", "Beta"]);
    assert!(mac.lib.paper(a.id.clone()).unwrap().starred);
    assert_eq!(mac.lib.annotations(a.id.clone()).len(), 2);
}

#[test]
fn nothing_to_do_means_no_transfers() {
    let w = World::new();
    let mac = w.mac();
    mac.lib.add_paper(meta("Alpha", ""), Some(b"pdf".to_vec()), None, None).unwrap();
    let ph = w.api("phone");
    ph.sync();
    let r = ph.sync();
    assert_eq!((r.uploaded, r.downloaded, r.deleted_local, r.deleted_remote), (0, 0, 0, 0));
    assert!(r.conflicts.is_empty() && !r.changed);
    let s = ph.syncer.as_ref().unwrap().status();
    assert_eq!((s.pending_uploads, s.pending_downloads, s.not_downloaded), (0, 0, 0));
    assert!(s.last_sync_ms > 0 && s.tracked_files > 0);
}

#[test]
fn never_touches_another_devices_log() {
    let w = World::new();
    let mac = w.mac();
    mac.lib.add_paper(meta("Alpha", ""), None, None, None).unwrap();
    let mac_log = w.remote.join("ops/mac.jsonl");
    let before = read(&mac_log);

    let ph = w.api("phone");
    ph.sync();
    // a local mishap: the phone's copy of the Mac's log is edited, and a stray log of a third device appears
    fs::write(ph.root.join("ops/mac.jsonl"), b"garbage\n").unwrap();
    fs::write(ph.root.join("ops/other.jsonl"), b"{}\n").unwrap();
    fs::create_dir_all(ph.root.join("positions")).unwrap();
    fs::write(ph.root.join("positions/other.json"), b"{}").unwrap();
    ph.lib.set_read(ph.lib.papers(PaperFilter::default(), SortKey::Added)[0].id.clone(), true).unwrap();
    ph.sync();
    assert_eq!(read(&mac_log), before, "mac's log changed on the remote");
    assert!(!w.remote.join("ops/other.jsonl").exists());
    assert!(!w.remote.join("positions/other.json").exists());
    // and the next sync repairs the local copy from the remote
    ph.sync();
    assert_eq!(read(&ph.root.join("ops/mac.jsonl")), before);
}

#[test]
fn publishes_the_logs_it_is_told_are_its_own() {
    // a Mac whose library syncs through the API: its AI agent writes ops/agent-x.jsonl into the
    // same working folder, and the Mac uploads it like its own log (other foreign logs stay put)
    let w = World::new();
    let root = w.tmp.path().join("lib-macapi");
    let cache = w.tmp.path().join("cache-macapi");
    let lib = Library::open(root.to_string_lossy().into(), cache.to_string_lossy().into(), "macapi".into()).unwrap();
    let mut opts = SyncOptions::new(w.tmp.path().join("state-macapi.json"));
    opts.also_own = vec!["agent-x".into()];
    let mac = Dev { lib, syncer: Some(Syncer::new(&root, "macapi", Box::new(LocalDirRemote::new(&w.remote)), opts).unwrap()), root };
    mac.lib.add_paper(meta("Alpha", ""), None, None, None).unwrap();
    let agent = Library::open(mac.root.to_string_lossy().into(), w.tmp.path().join("cache-agent").to_string_lossy().into(), "agent-x".into()).unwrap();
    let id = agent.papers(PaperFilter::default(), SortKey::Added)[0].id.clone();
    agent.set_starred(id.clone(), true).unwrap();
    fs::write(mac.root.join("ops/stray.jsonl"), b"{}\n").unwrap();
    mac.sync();
    assert!(w.remote.join("ops/agent-x.jsonl").exists());
    assert!(!w.remote.join("ops/stray.jsonl").exists());

    // the agent writes more: the next sync sends it, and a phone sees the star
    agent.set_read(id.clone(), true).unwrap();
    let r = mac.sync();
    assert_eq!(r.uploaded, 1, "{r:?}");
    assert!(r.conflicts.is_empty(), "{r:?}");
    let ph = w.api("phone");
    ph.sync();
    let p = ph.lib.paper(id).unwrap();
    assert!(p.starred && p.read);
}

#[test]
fn three_devices_converge() {
    let w = World::new();
    let mac = w.mac();
    let p1 = w.api("p1");
    let p2 = w.api("p2");
    let a = mac.lib.add_paper(meta("From Mac", ""), Some(b"pdf-m".to_vec()), None, None).unwrap();
    p1.lib.add_paper(meta("From P1", ""), Some(b"pdf-1".to_vec()), None, None).unwrap();
    p2.lib.add_paper(meta("From P2", ""), None, Some(b"epub-2".to_vec()), None).unwrap();
    p1.sync();
    p2.sync();
    p1.sync();
    p2.sync();
    mac.refresh();
    let all = vec!["From Mac", "From P1", "From P2"];
    assert_eq!(mac.titles(), all);
    assert_eq!(p1.titles(), all);
    assert_eq!(p2.titles(), all);
    // concurrent edits of one paper from all three: tags union, everything visible everywhere
    mac.lib.set_tag(a.id.clone(), "m".into(), true).unwrap();
    p1.lib.set_tag(a.id.clone(), "one".into(), true).unwrap();
    p2.lib.set_tag(a.id.clone(), "two".into(), true).unwrap();
    p1.sync();
    p2.sync();
    p1.sync();
    mac.refresh();
    for d in [&mac, &p1, &p2] {
        let mut t = d.lib.paper(a.id.clone()).unwrap().tags;
        t.sort();
        assert_eq!(t, vec!["m", "one", "two"]);
    }
}

#[test]
fn delete_paper_propagates() {
    let w = World::new();
    let mac = w.mac();
    let p1 = w.api("p1");
    let p2 = w.api("p2");
    let a = p1.lib.add_paper(meta("Doomed", ""), Some(b"pdf".to_vec()), Some(b"epub".to_vec()), Some("ft".into())).unwrap();
    p1.lib.add_paper(meta("Survivor", ""), Some(b"pdf2".to_vec()), None, None).unwrap();
    p1.sync();
    p2.sync();
    mac.refresh();
    assert_eq!(p2.titles().len(), 2);
    assert!(p2.root.join(format!("papers/{}/book.epub", a.id)).exists());

    p1.lib.delete_paper(a.id.clone()).unwrap();
    let r = p1.sync();
    assert_eq!(r.deleted_remote, 3, "{r:?}");
    assert!(!w.remote.join(format!("papers/{}", a.id)).exists());
    let r = p2.sync();
    assert_eq!(r.deleted_local, 3, "{r:?}");
    assert!(!p2.root.join(format!("papers/{}/paper.pdf", a.id)).exists());
    mac.refresh();
    assert_eq!(mac.titles(), vec!["Survivor"]);
    assert_eq!(p2.titles(), vec!["Survivor"]);
}

#[test]
fn replaced_paper_file_and_remote_file_removal() {
    let w = World::new();
    let mac = w.mac();
    let ph = w.api("phone");
    let a = mac.lib.add_paper(meta("Alpha", ""), Some(b"pdf v1".to_vec()), Some(b"epub v1".to_vec()), None).unwrap();
    ph.sync();
    // the Mac replaces the epub (e.g. the converter improved) and drops the pdf
    mac.lib.attach_file(a.id.clone(), FileKind::Epub, b"epub version two, longer".to_vec()).unwrap();
    fs::remove_file(w.remote.join(format!("papers/{}/paper.pdf", a.id))).unwrap();
    let r = ph.sync();
    assert_eq!((r.downloaded, r.deleted_local), (1, 1), "{r:?}");
    assert_eq!(read(&ph.root.join(format!("papers/{}/book.epub", a.id))), b"epub version two, longer");
    assert!(!ph.root.join(format!("papers/{}/paper.pdf", a.id)).exists());
}

#[test]
fn local_edit_beats_remote_delete_and_remote_edit_beats_local_delete() {
    let w = World::new();
    let mac = w.mac();
    let ph = w.api("phone");
    let a = mac.lib.add_paper(meta("Alpha", ""), Some(b"pdf".to_vec()), Some(b"epub".to_vec()), Some("ft".into())).unwrap();
    ph.sync();
    let d = format!("papers/{}", a.id);
    // remote deleted, local edited -> re-uploaded
    fs::remove_file(w.remote.join(&d).join("fulltext.txt")).unwrap();
    fs::write(ph.root.join(&d).join("fulltext.txt"), b"edited on the phone").unwrap();
    // local deleted, remote edited -> downloaded again
    fs::remove_file(ph.root.join(&d).join("book.epub")).unwrap();
    fs::write(w.remote.join(&d).join("book.epub"), b"edited on the mac, longer").unwrap();
    let r = ph.sync();
    assert_eq!((r.uploaded, r.downloaded, r.deleted_local, r.deleted_remote), (1, 1, 0, 0), "{r:?}");
    assert_eq!(read(&w.remote.join(&d).join("fulltext.txt")), b"edited on the phone");
    assert_eq!(read(&ph.root.join(&d).join("book.epub")), b"edited on the mac, longer");
}

#[test]
fn shared_json_last_writer_wins_and_keeps_losing_copy() {
    let w = World::new();
    let _mac = w.mac();
    let p1 = w.api("p1");
    let p2 = w.api("p2");
    fs::write(p1.root.join("smart-folders.json"), b"[\"v1\"]").unwrap();
    p1.sync();
    p2.sync();
    assert_eq!(read(&p2.root.join("smart-folders.json")), b"[\"v1\"]");

    // both change it; p2's change is the later one
    let t = util::file_sig(&p1.root.join("smart-folders.json")).unwrap().1;
    fs::write(p1.root.join("smart-folders.json"), b"[\"from p1\"]").unwrap();
    set_time(&p1.root.join("smart-folders.json"), t + 10_000);
    fs::write(p2.root.join("smart-folders.json"), b"[\"from p2, later\"]").unwrap();
    set_time(&p2.root.join("smart-folders.json"), t + 20_000);
    p1.sync(); // uploads p1's version
    assert_eq!(read(&w.remote.join("smart-folders.json")), b"[\"from p1\"]");
    let r = p2.sync(); // conflict: p2 is newer -> wins, uploads
    assert_eq!(r.conflicts, vec!["smart-folders.json"]);
    assert_eq!(read(&w.remote.join("smart-folders.json")), b"[\"from p2, later\"]");
    p1.sync();
    assert_eq!(read(&p1.root.join("smart-folders.json")), b"[\"from p2, later\"]");

    // the other way round: the remote (written by the Mac) is newer than p1's edit -> remote wins, p1's copy is kept aside
    let t = util::file_sig(&p1.root.join("smart-folders.json")).unwrap().1;
    fs::write(p1.root.join("smart-folders.json"), b"[\"old p1 edit\"]").unwrap();
    set_time(&p1.root.join("smart-folders.json"), t + 1_000);
    fs::write(w.remote.join("watches.json"), b"{}").unwrap();
    fs::write(w.remote.join("smart-folders.json"), b"[\"mac edit, newer\"]").unwrap();
    set_time(&w.remote.join("smart-folders.json"), t + 50_000);
    let r = p1.sync();
    assert_eq!(r.conflicts, vec!["smart-folders.json"]);
    assert_eq!(read(&p1.root.join("smart-folders.json")), b"[\"mac edit, newer\"]");
    assert_eq!(read(&p1.root.join("watches.json")), b"{}");
    let kept: Vec<_> = fs::read_dir(p1.root.join(".lunta-sync/conflicts")).unwrap().collect();
    assert_eq!(kept.len(), 1);
    assert_eq!(read(&kept[0].as_ref().unwrap().path()), b"[\"old p1 edit\"]");
}

#[test]
fn positions_are_merged_per_paper_for_own_device_only() {
    let w = World::new();
    fs::create_dir_all(w.remote.join("positions")).unwrap();
    fs::write(w.remote.join("positions/mac.json"), br#"{"p1":{"offset":10,"at":5,"device":"mac"}}"#).unwrap();
    let ph = w.api("phone");
    ph.sync();
    assert!(ph.root.join("positions/mac.json").exists());
    fs::write(ph.root.join("positions/phone.json"), br#"{"p1":{"offset":99,"at":9,"device":"phone"},"p2":{"offset":1,"at":1,"device":"phone"}}"#).unwrap();
    ph.sync();
    // remote copy of the phone's file gets lost/replaced by an older version -> merged by newest `at`
    fs::write(w.remote.join("positions/phone.json"), br#"{"p1":{"offset":50,"at":4,"device":"phone"},"p3":{"offset":7,"at":8,"device":"phone"}}"#).unwrap();
    let r = ph.sync();
    assert_eq!(r.conflicts, vec!["positions/phone.json (merged)"]);
    let merged: serde_json::Value = serde_json::from_slice(&read(&ph.root.join("positions/phone.json"))).unwrap();
    assert_eq!(merged["p1"]["offset"], 99);
    assert_eq!(merged["p2"]["offset"], 1);
    assert_eq!(merged["p3"]["offset"], 7);
    assert_eq!(read(&w.remote.join("positions/phone.json")), read(&ph.root.join("positions/phone.json")));
    assert_eq!(read(&w.remote.join("positions/mac.json")), br#"{"p1":{"offset":10,"at":5,"device":"mac"}}"#);
}

#[test]
fn reinstalled_device_with_same_id_does_not_lose_its_old_log() {
    let w = World::new();
    let ph = w.api("phone");
    let a = ph.lib.add_paper(meta("Old", ""), None, None, None).unwrap();
    ph.lib.set_starred(a.id.clone(), true).unwrap();
    ph.sync();
    let remote_before = read(&w.remote.join("ops/phone.jsonl"));

    // reinstall: fresh working folder and fresh sync state, same device id, one new op before the first sync
    let root2 = w.tmp.path().join("lib-phone2");
    fs::create_dir_all(root2.join("ops")).unwrap();
    fs::write(root2.join("ops/phone.jsonl"), b"{\"at\":1,\"device\":\"phone\",\"op\":\"set_read\",\"paper\":\"zzz\",\"read\":true}\n").unwrap();
    let s = Syncer::new(&root2, "phone", Box::new(LocalDirRemote::new(&w.remote)), SyncOptions::new(w.tmp.path().join("state-new.json"))).unwrap();
    let r = s.sync_now().unwrap();
    assert!(r.errors.is_empty());
    let merged = String::from_utf8(read(&w.remote.join("ops/phone.jsonl"))).unwrap();
    for line in String::from_utf8(remote_before).unwrap().lines() {
        assert!(merged.contains(line), "lost {line}");
    }
    assert!(merged.contains("\"zzz\""));
    assert_eq!(read(&w.remote.join("ops/phone.jsonl")), read(&root2.join("ops/phone.jsonl")));
}

/// Fails every remote call after the first `ok` upload/download calls.
struct Flaky {
    inner: LocalDirRemote,
    budget: AtomicI64,
    calls: Mutex<Vec<String>>,
}

impl Flaky {
    fn new(root: &Path, budget: i64) -> Arc<Flaky> {
        Arc::new(Flaky { inner: LocalDirRemote::new(root), budget: AtomicI64::new(budget), calls: Mutex::new(vec![]) })
    }
    fn spend(&self, what: String) -> Result<()> {
        if self.budget.fetch_sub(1, Ordering::SeqCst) <= 0 {
            return Err(CoreError::Network("connection lost".into()));
        }
        self.calls.lock().unwrap().push(what);
        Ok(())
    }
}

struct Shared(Arc<Flaky>);
impl RemoteStore for Shared {
    fn describe(&self) -> String {
        "flaky".into()
    }
    fn list(&self, c: Option<&str>) -> Result<Listing> {
        self.0.inner.list(c)
    }
    fn download(&self, f: &RemoteFile, d: &Path) -> Result<()> {
        self.0.spend(format!("down {}", f.path))?;
        self.0.inner.download(f, d)
    }
    fn upload(&self, p: &str, l: &Path, m: i64, e: Option<&RemoteFile>) -> Result<RemoteFile> {
        self.0.spend(format!("up {p}"))?;
        self.0.inner.upload(p, l, m, e)
    }
    fn delete(&self, f: &RemoteFile) -> Result<()> {
        self.0.spend(format!("del {}", f.path))?;
        self.0.inner.delete(f)
    }
}

#[test]
fn interrupted_sync_resumes_without_repeating_finished_files() {
    let w = World::new();
    let mac = w.mac();
    for i in 0..6 {
        mac.lib.add_paper(meta(&format!("P{i}"), ""), Some(format!("pdf {i}").into_bytes()), Some(format!("epub {i}").into_bytes()), None).unwrap();
    }
    let flaky = Flaky::new(&w.remote, 5);
    let ph = w.api_with("phone", Box::new(Shared(flaky.clone())), false);
    let first = ph.syncer.as_ref().unwrap().sync_now().unwrap();
    assert_eq!(first.downloaded, 5);
    assert!(!first.errors.is_empty());
    // the op log comes last, so the interrupted state shows no paper whose files might be missing
    assert!(!ph.root.join("ops/mac.jsonl").exists());
    ph.refresh();
    assert!(ph.titles().is_empty());

    // connection is back
    flaky.budget.store(1000, Ordering::SeqCst);
    flaky.calls.lock().unwrap().clear();
    let second = ph.sync();
    let again: Vec<_> = flaky.calls.lock().unwrap().iter().filter(|c| c.starts_with("down")).cloned().collect();
    assert_eq!(again.len() as u32, second.downloaded);
    let total_files = 6 * 2 + 1; // pdf+epub per paper, plus the log
    assert_eq!(first.downloaded + second.downloaded, total_files);
    assert_eq!(ph.titles().len(), 6);
    // no leftovers from the failed run
    let junk: Vec<_> = walk(&ph.root).into_iter().filter(|p| p.contains(".part-")).collect();
    assert!(junk.is_empty(), "{junk:?}");
}

#[test]
fn interrupted_upload_resumes() {
    let w = World::new();
    let flaky = Flaky::new(&w.remote, 2);
    let ph = w.api_with("phone", Box::new(Shared(flaky.clone())), false);
    for i in 0..3 {
        ph.lib.add_paper(meta(&format!("P{i}"), ""), Some(b"pdf".to_vec()), None, None).unwrap();
    }
    let first = ph.syncer.as_ref().unwrap().sync_now().unwrap();
    assert_eq!(first.uploaded, 2);
    assert!(!w.remote.join("ops/phone.jsonl").exists(), "the log must not be published before the files it refers to");
    flaky.budget.store(100, Ordering::SeqCst);
    let second = ph.sync();
    assert_eq!(second.uploaded, 2); // the third pdf and the log
    let mac = w.mac();
    assert_eq!(mac.titles().len(), 3);
}

fn walk(p: &Path) -> Vec<String> {
    let mut out = vec![];
    if let Ok(rd) = fs::read_dir(p) {
        for e in rd.flatten() {
            if e.path().is_dir() {
                out.extend(walk(&e.path()));
            } else {
                out.push(e.path().to_string_lossy().to_string());
            }
        }
    }
    out
}

#[test]
fn lazy_mode_defers_big_files_until_fetched() {
    let w = World::new();
    let mac = w.mac();
    let a = mac.lib.add_paper(meta("Alpha", ""), Some(b"pdf bytes".to_vec()), Some(b"epub bytes".to_vec()), Some("full text".into())).unwrap();
    let ph = w.api_with("phone", Box::new(LocalDirRemote::new(&w.remote)), true);
    let r = ph.sync();
    assert_eq!(r.skipped_lazy, 2);
    assert_eq!(ph.titles(), vec!["Alpha"]);
    let pid = &a.id;
    assert!(ph.root.join(format!("papers/{pid}/fulltext.txt")).exists());
    assert!(!ph.root.join(format!("papers/{pid}/paper.pdf")).exists());
    let s = ph.syncer.as_ref().unwrap();
    assert_eq!(s.status().not_downloaded, 2);
    let files = s.paper_files(pid);
    assert!(files.iter().any(|(n, size, here)| n == "paper.pdf" && *size == 9 && !here));

    // opening the paper on the Android app: fetch what the viewer needs
    assert_eq!(s.fetch_paper(pid, &[FileKind::Epub]).unwrap(), 1);
    assert_eq!(read(&ph.root.join(format!("papers/{pid}/book.epub"))), b"epub bytes");
    assert_eq!(s.fetch_paper(pid, &[FileKind::Epub]).unwrap(), 0);
    assert_eq!(s.status().not_downloaded, 1);
    // a later sync neither re-downloads the fetched file, nor treats the missing pdf as a deletion
    let r = ph.sync();
    assert_eq!((r.downloaded, r.deleted_remote, r.deleted_local), (0, 0, 0));
    assert!(w.remote.join(format!("papers/{pid}/paper.pdf")).exists());
    assert_eq!(s.fetch_file(&format!("papers/{pid}/paper.pdf")).unwrap(), ph.root.join(format!("papers/{pid}/paper.pdf")));
    assert!(s.fetch_file("ops/mac.jsonl").is_err());
    assert!(s.fetch_paper("nope", &[]).unwrap() == 0);
}

/// An iPhone with its library in Google Drive: the files left in Drive count as present (no
/// "missing PDF", no remade reading view), "Remove Download" frees space without a sync taking it
/// for a deletion, and a file deleted any other way still is one.
#[test]
fn remote_only_files_count_as_present_and_removed_downloads_are_not_deletions() {
    let w = World::new();
    let mac = w.mac();
    let a = mac.lib.add_paper(meta("Alpha", ""), Some(b"pdf alpha".to_vec()), Some(b"epub alpha".to_vec()), Some("alpha text".into())).unwrap();
    let b = mac.lib.add_paper(meta("Beta", ""), Some(b"pdf beta".to_vec()), None, None).unwrap();
    let ph = w.api_with("phone", Box::new(LocalDirRemote::new(&w.remote)), true);
    ph.sync();
    let s = ph.syncer.as_ref().unwrap();
    let (aid, bid) = (a.id.clone(), b.id.clone());

    // before the app tells the library, the files look missing
    assert!(!ph.lib.paper(aid.clone()).unwrap().has_pdf);
    let remote_only: Vec<String> = s.remote_paper_files().into_iter().filter(|(_, _, here)| !here).map(|(p, _, _)| p).collect();
    assert_eq!(remote_only.len(), 3, "{remote_only:?}"); // Alpha's pdf and epub, Beta's pdf
    assert!(s.remote_paper_files().iter().any(|(p, _, here)| p == &format!("papers/{aid}/fulltext.txt") && *here));
    assert!(ph.lib.set_remote_files(remote_only.clone()));
    assert!(!ph.lib.set_remote_files(remote_only.clone())); // nothing new
    let pa = ph.lib.paper(aid.clone()).unwrap();
    assert!(pa.has_pdf && pa.has_epub);
    assert!(ph.lib.paper(bid.clone()).unwrap().has_pdf);
    assert_eq!(ph.lib.file_status(aid.clone(), FileKind::Pdf), FileStatus::InCloud);
    assert_eq!(ph.lib.file_status(aid.clone(), FileKind::Fulltext), FileStatus::Local);
    assert_eq!(ph.lib.file_status(bid.clone(), FileKind::Epub), FileStatus::Missing);
    assert!(ph.lib.file_path(aid.clone(), FileKind::Pdf).is_none());
    // Find PDF never replaces a PDF that is only in Drive (it errors before looking online)
    assert!(ph.lib.find_pdf(bid.clone()).is_err());
    assert!(!ph.root.join(format!("papers/{bid}/paper.pdf")).exists());
    // a highlight needs the reading view itself (not the full text, which would put it on the PDF)
    let e = ph.lib.anchor_highlight(aid.clone(), "alpha text".into(), None).unwrap_err().to_string();
    assert!(e.contains("isn't downloaded"), "{e}");
    // still present after a refresh and a reindex
    ph.lib.reindex().unwrap();
    assert!(ph.lib.paper(aid.clone()).unwrap().has_pdf);

    // opening Alpha: fetched, now on the device
    assert_eq!(s.fetch_paper(&aid, &[FileKind::Pdf, FileKind::Epub]).unwrap(), 2);
    assert_eq!(ph.lib.file_status(aid.clone(), FileKind::Pdf), FileStatus::Local);

    // Remove Download: gone here, kept in Drive, and syncs leave it that way
    assert_eq!(s.remove_download(&aid).unwrap(), (2, 0, 19));
    assert!(!ph.root.join(format!("papers/{aid}/paper.pdf")).exists());
    assert!(ph.root.join(format!("papers/{aid}/fulltext.txt")).exists());
    assert_eq!(s.status().not_downloaded, 3);
    for _ in 0..2 {
        let r = ph.sync();
        assert_eq!((r.downloaded, r.uploaded, r.deleted_remote, r.deleted_local), (0, 0, 0, 0), "{r:?}");
    }
    assert_eq!(read(&w.remote.join(format!("papers/{aid}/paper.pdf"))), b"pdf alpha");
    assert_eq!(read(&w.remote.join(format!("papers/{aid}/book.epub"))), b"epub alpha");
    // a never-downloaded file: nothing to remove
    assert_eq!(s.remove_download(&bid).unwrap(), (0, 0, 0));
    // and it can be fetched again
    assert_eq!(s.fetch_paper(&aid, &[FileKind::Pdf]).unwrap(), 1);
    assert_eq!(read(&ph.root.join(format!("papers/{aid}/paper.pdf"))), b"pdf alpha");

    // the Mac replaces Alpha's reading view while the phone has no copy: fetched new, not a conflict
    let r = ph.sync();
    assert_eq!(r.deleted_remote, 0);
    mac.lib.attach_file(aid.clone(), FileKind::Epub, b"epub alpha v2".to_vec()).unwrap();
    let r = ph.sync();
    assert_eq!((r.downloaded, r.deleted_remote), (0, 0), "{r:?}");
    assert_eq!(s.fetch_paper(&aid, &[FileKind::Epub]).unwrap(), 1);
    assert_eq!(read(&ph.root.join(format!("papers/{aid}/book.epub"))), b"epub alpha v2");

    // a copy changed here and not uploaded yet is kept
    fs::write(ph.root.join(format!("papers/{aid}/paper.pdf")), b"pdf alpha, edited here").unwrap();
    set_time(&ph.root.join(format!("papers/{aid}/paper.pdf")), now_ms() + 5_000);
    assert_eq!(s.remove_download(&aid).unwrap(), (1, 1, 13)); // the epub went, the edited pdf stayed
    assert!(ph.root.join(format!("papers/{aid}/paper.pdf")).exists());
    let r = ph.sync();
    assert_eq!((r.uploaded, r.deleted_remote), (1, 0), "{r:?}");
    assert_eq!(read(&w.remote.join(format!("papers/{aid}/paper.pdf"))), b"pdf alpha, edited here");

    // deleting a downloaded file any other way (the app removing a reading view) is a deletion
    s.fetch_paper(&bid, &[]).unwrap();
    fs::remove_file(ph.root.join(format!("papers/{bid}/paper.pdf"))).unwrap();
    let r = ph.sync();
    assert_eq!(r.deleted_remote, 1, "{r:?}");
    assert!(!w.remote.join(format!("papers/{bid}/paper.pdf")).exists());

    // a library that downloads everything has nothing to remove
    let full = w.api("tablet");
    full.sync();
    assert!(full.syncer.as_ref().unwrap().remove_download(&aid).is_err());
    assert!(full.root.join(format!("papers/{aid}/paper.pdf")).exists());
}

/// A library synced on demand: a PDF replaced here while Drive's copy was not downloaded (or an
/// edit not uploaded yet) is not overwritten by Download / opening the paper; the next sync sends it.
#[test]
fn fetching_never_overwrites_a_local_copy_not_uploaded_yet() {
    let w = World::new();
    let mac = w.mac();
    let a = mac.lib.add_paper(meta("Alpha", ""), Some(b"pdf alpha".to_vec()), Some(b"epub alpha".to_vec()), None).unwrap();
    let ph = w.api_with("phone", Box::new(LocalDirRemote::new(&w.remote)), true);
    ph.sync();
    let s = ph.syncer.as_ref().unwrap();
    let aid = a.id.clone();
    let pdf = ph.root.join(format!("papers/{aid}/paper.pdf"));
    assert!(!pdf.exists());

    // Replace PDF while the PDF is only in Drive, then Download (every file of the paper)
    ph.lib.attach_file(aid.clone(), FileKind::Pdf, b"pdf alpha, replaced here".to_vec()).unwrap();
    set_time(&pdf, now_ms() + 5_000);
    assert_eq!(s.fetch_paper(&aid, &[]).unwrap(), 1); // the epub came down, the pdf was left alone
    assert_eq!(read(&pdf), b"pdf alpha, replaced here");
    assert_eq!(read(&ph.root.join(format!("papers/{aid}/book.epub"))), b"epub alpha");
    assert_eq!(s.fetch_file(&format!("papers/{aid}/paper.pdf")).unwrap(), pdf);
    assert_eq!(read(&pdf), b"pdf alpha, replaced here");
    let r = ph.sync();
    assert_eq!(r.downloaded, 0, "{r:?}");
    assert_eq!(read(&w.remote.join(format!("papers/{aid}/paper.pdf"))), b"pdf alpha, replaced here");
    assert_eq!(read(&pdf), b"pdf alpha, replaced here");

    // a file here that is the remote's copy, never recorded: fetched (recorded) as usual
    let b = mac.lib.add_paper(meta("Beta", ""), Some(b"pdf beta".to_vec()), None, None).unwrap();
    ph.sync();
    let bpdf = ph.root.join(format!("papers/{}/paper.pdf", b.id));
    fs::create_dir_all(bpdf.parent().unwrap()).unwrap();
    fs::write(&bpdf, b"pdf beta").unwrap();
    s.fetch_paper(&b.id, &[FileKind::Pdf]).unwrap(); // the same bytes: recorded as here
    assert_eq!(read(&bpdf), b"pdf beta");
    assert_eq!(s.remove_download(&b.id).unwrap().0, 1);
}

#[test]
fn wiped_working_folder_does_not_wipe_the_remote() {
    let w = World::new();
    let ph = w.api("phone");
    for i in 0..15 {
        ph.lib.add_paper(meta(&format!("P{i}"), ""), Some(format!("pdf {i}").into_bytes()), None, None).unwrap();
    }
    ph.sync();
    fs::remove_dir_all(ph.root.join("papers")).unwrap();
    let r = ph.syncer.as_ref().unwrap().sync_now().unwrap();
    assert_eq!(r.deleted_remote, 0);
    assert!(r.errors.iter().any(|e| e.contains("precaution")), "{:?}", r.errors);
    assert_eq!(walk(&w.remote.join("papers")).len(), 15);
}

#[test]
fn empty_remote_after_successful_sync_is_refused() {
    let w = World::new();
    let ph = w.api("phone");
    ph.lib.add_paper(meta("Alpha", ""), Some(b"pdf".to_vec()), None, None).unwrap();
    ph.sync();
    fs::remove_dir_all(&w.remote).unwrap();
    fs::create_dir_all(&w.remote).unwrap();
    let e = ph.syncer.as_ref().unwrap().sync_now().unwrap_err();
    assert!(e.to_string().contains("looks empty"));
    assert!(ph.root.join("ops/phone.jsonl").exists());
}

#[test]
fn identical_files_on_both_sides_are_adopted_without_transfer() {
    let w = World::new();
    let mac = w.mac();
    mac.lib.add_paper(meta("Alpha", ""), Some(b"pdf".to_vec()), None, None).unwrap();
    let ph = w.api("phone");
    ph.sync();
    // lose the sync state (new install of the app keeping the folder)
    ph.syncer.as_ref().unwrap().reset_state().unwrap();
    let r = ph.sync();
    assert_eq!((r.uploaded, r.downloaded, r.deleted_local, r.deleted_remote), (0, 0, 0, 0), "{r:?}");
    assert!(r.conflicts.is_empty());
}

#[test]
fn ignores_dot_files_and_unknown_folders() {
    let w = World::new();
    let ph = w.api("phone");
    fs::create_dir_all(ph.root.join("scratch")).unwrap();
    fs::write(ph.root.join("scratch/x.pdf"), b"x").unwrap();
    fs::write(ph.root.join(".DS_Store"), b"x").unwrap();
    fs::write(ph.root.join("notes.txt"), b"x").unwrap();
    fs::create_dir_all(w.remote.join("papers/abc")).unwrap();
    fs::write(w.remote.join("papers/abc/.paper.pdf.tmp-mac"), b"half").unwrap();
    fs::write(w.remote.join("papers/abc/paper.pdf"), b"whole").unwrap();
    let r = ph.sync();
    assert_eq!((r.uploaded, r.downloaded), (0, 1));
    assert!(!ph.root.join("papers/abc/.paper.pdf.tmp-mac").exists());
}

// ---- Google Drive client against the in-memory mock ----

mod gdrive_mock {
    use super::super::auth::{OAuthClient, Tokens};
    use super::super::mock::Mock;
    use super::*;
    use crate::model::now_ms;

    const FILE: &str = "https://www.googleapis.com/auth/drive.file";

    /// A client whose stored grant is `scope` (None: a token saved by a version that did not record
    /// scopes, i.e. the old full `drive` scope).
    fn drive_with(m: &Mock, expires_in_ms: i64, scope: Option<&str>, root: DriveRoot) -> GoogleDrive {
        let mut client = OAuthClient::new("cid", Some("secret".into()));
        client.token_uri = format!("{}/token", m.addr);
        let store = Arc::new(MemoryTokenStore::default());
        let token = m.inner.lock().unwrap().token.clone();
        store.save(&Tokens { access_token: token, refresh_token: Some("refresh".into()), expires_at_ms: now_ms() + expires_in_ms, scope: scope.map(String::from) }).unwrap();
        let auth = Authenticator::new(client, store);
        GoogleDrive::new(auth, root).with_endpoints(&m.api(), &m.upload_api(), std::time::Duration::from_millis(1))
    }

    /// The protocol tests: a drive.file token, and a mock that shows everything (so `put` stands in
    /// for any other device writing into the folder).
    fn drive_for(m: &Mock, expires_in_ms: i64) -> GoogleDrive {
        drive_with(m, expires_in_ms, Some(FILE), DriveRoot::default())
    }

    fn dev(w: &World, name: &str, m: &Mock) -> Dev {
        w.api_with(name, Box::new(drive_for(m, 3_600_000)), false)
    }

    #[test]
    fn full_flow_with_incremental_changes() {
        let m = Mock::start();
        m.put("ops/mac.jsonl", b"{\"at\":1}\n", 1_700_000_000_000);
        m.put("papers/p1/paper.pdf", b"pdf one", 1_700_000_000_000);
        m.put("smart-folders.json", b"[]", 1_700_000_000_000);
        m.put("somewhere/else.txt", b"ignored", 1_700_000_000_000);
        let w = World::new();
        let ph = dev(&w, "phone", &m);
        let r = ph.sync();
        assert_eq!(r.downloaded, 3, "{r:?}");
        assert_eq!(read(&ph.root.join("papers/p1/paper.pdf")), b"pdf one");
        assert_eq!(util::file_sig(&ph.root.join("papers/p1/paper.pdf")).unwrap().1, 1_700_000_000_000);
        assert_eq!(m.count("GET /changes/startPageToken"), 1);
        let lists = m.count("GET /files");

        // nothing changed: one changes call, no listing, no transfers
        let r = ph.sync();
        assert_eq!((r.uploaded, r.downloaded), (0, 0), "{r:?}");
        assert_eq!(m.count("GET /files"), lists, "incremental sync must not list the tree again");
        assert!(m.count("GET /changes") >= 1);

        // phone uploads a new paper (creates folder) and its op log
        fs::create_dir_all(ph.root.join("papers/p2")).unwrap();
        fs::write(ph.root.join("papers/p2/book.epub"), b"epub two").unwrap();
        fs::write(ph.root.join("ops/phone.jsonl"), b"{\"at\":2}\n").unwrap();
        let r = ph.sync();
        assert_eq!(r.uploaded, 2, "{r:?}");
        assert_eq!(m.read("papers/p2/book.epub").unwrap(), b"epub two");
        assert_eq!(m.read("ops/phone.jsonl").unwrap(), b"{\"at\":2}\n");
        // appended: the same Drive file is replaced, not duplicated
        fs::write(ph.root.join("ops/phone.jsonl"), b"{\"at\":2}\n{\"at\":3}\n").unwrap();
        ph.sync();
        assert_eq!(m.read("ops/phone.jsonl").unwrap(), b"{\"at\":2}\n{\"at\":3}\n");
        assert_eq!(m.paths().iter().filter(|p| *p == "ops/phone.jsonl").count(), 1);

        let lists = m.count("GET /files"); // (creating papers/p2 looked the folder up once)
        // the Mac (Drive for desktop) adds a paper in a brand-new folder, edits a file and deletes one
        m.put("papers/p3/paper.pdf", b"pdf three", 1_700_000_100_000);
        m.put("smart-folders.json", b"[1]", 1_700_000_100_000);
        m.trash("papers/p1/paper.pdf");
        let r = ph.sync();
        assert_eq!((r.downloaded, r.deleted_local), (2, 1), "{r:?}");
        assert_eq!(read(&ph.root.join("papers/p3/paper.pdf")), b"pdf three");
        assert!(!ph.root.join("papers/p1/paper.pdf").exists());
        assert_eq!(read(&ph.root.join("smart-folders.json")), b"[1]");
        assert_eq!(m.count("GET /files"), lists, "still no relisting");

        // a second API device starting from scratch sees the final tree, and never changes the Mac's files
        let mac_log = m.read("ops/mac.jsonl").unwrap();
        let p2 = dev(&w, "tablet", &m);
        let r = p2.sync();
        assert!(r.errors.is_empty() && r.downloaded >= 5, "{r:?}");
        assert_eq!(m.read("ops/mac.jsonl").unwrap(), mac_log);
    }

    #[test]
    fn large_file_uses_resumable_upload_and_survives_a_failed_chunk() {
        let m = Mock::start();
        m.put("ops/mac.jsonl", b"x\n", 1);
        let w = World::new();
        let ph = dev(&w, "phone", &m);
        ph.sync();
        let big: Vec<u8> = (0..9 * 1024 * 1024u32).map(|i| (i % 251) as u8).collect();
        fs::create_dir_all(ph.root.join("papers/big")).unwrap();
        fs::write(ph.root.join("papers/big/paper.pdf"), &big).unwrap();
        m.inner.lock().unwrap().fail_puts = 1;
        let r = ph.sync();
        assert_eq!(r.uploaded, 1, "{r:?}");
        assert_eq!(m.read("papers/big/paper.pdf").unwrap(), big);
        assert!(m.count("PUT /session/ID") >= 2, "9 MiB needs two chunks");
        // and back down on another device, checksum verified
        let p2 = dev(&w, "tablet", &m);
        let r = p2.sync();
        assert!(r.errors.is_empty(), "{r:?}");
        assert_eq!(read(&p2.root.join("papers/big/paper.pdf")), big);
    }

    #[test]
    fn expired_token_is_refreshed_and_401_retried() {
        let m = Mock::start();
        m.put("smart-folders.json", b"[]", 5);
        let w = World::new();
        // token already expired: refreshed before the first call
        let ph = w.api_with("phone", Box::new(drive_for(&m, -1000)), false);
        ph.sync();
        assert_eq!(m.inner.lock().unwrap().tokens_issued, 1);
        // server forgets the token (revoked/rotated): 401 -> refresh -> retry
        m.inner.lock().unwrap().token = "something-else".into();
        fs::write(ph.root.join("watches.json"), b"{}").unwrap();
        let r = ph.sync();
        assert_eq!(r.uploaded, 1, "{r:?}");
        assert_eq!(m.inner.lock().unwrap().tokens_issued, 2);
        assert_eq!(m.read("watches.json").unwrap(), b"{}");
    }

    #[test]
    fn library_folder_by_id_and_missing_folder() {
        let m = Mock::start();
        let w = World::new();
        let mut client = OAuthClient::new("cid", None);
        client.token_uri = format!("{}/token", m.addr);
        let store = Arc::new(MemoryTokenStore::default());
        store.save(&Tokens { access_token: "tok-0".into(), refresh_token: Some("r".into()), expires_at_ms: now_ms() + 3_600_000, scope: Some(FILE.into()) }).unwrap();
        let auth = Authenticator::new(client, store);
        let d = GoogleDrive::new(auth, DriveRoot::Named { name: "Nope".into(), create: false }).with_endpoints(&m.api(), &m.upload_api(), std::time::Duration::from_millis(1));
        let e = d.list(None).unwrap_err();
        assert!(matches!(e, CoreError::NotFound(_)), "{e}");
        // default root is created on first use
        let ph = dev(&w, "phone", &m);
        fs::write(ph.root.join("smart-folders.json"), b"[]").unwrap();
        ph.sync();
        assert_eq!(m.read("smart-folders.json").unwrap(), b"[]");
    }

    // ---- drive.file: only what Lunta created, or the user picked, is visible ----

    fn file_scope_dev(w: &World, name: &str, m: &Mock) -> Dev {
        w.api_with(name, Box::new(drive_with(m, 3_600_000, Some(FILE), DriveRoot::default())), false)
    }

    fn try_sync(d: &Dev) -> Result<SyncReport> {
        d.syncer.as_ref().unwrap().sync_now()
    }

    #[test]
    fn drive_file_new_library_is_all_app_created() {
        let m = Mock::start();
        m.inner.lock().unwrap().file_scope = true;
        let w = World::new();
        let ph = file_scope_dev(&w, "phone", &m);
        let p = ph.lib.add_paper(meta("New", "10.1/new"), Some(b"%PDF new".to_vec()), None, None).unwrap();
        ph.lib.save_annotation(note(&p.id, "hi")).unwrap();
        let r = ph.sync();
        assert!(r.uploaded >= 2 && r.notices.is_empty(), "{r:?}");
        let all = m.everything();
        assert!(all.iter().any(|(p, _)| p == "Lunta/ops/phone.jsonl"), "{all:?}");
        assert!(all.iter().all(|(_, app)| *app), "everything was made through the API: {all:?}");
        // a second device signed in with drive.file finds the folder by name and gets everything
        let tab = file_scope_dev(&w, "tablet", &m);
        tab.sync();
        assert_eq!(tab.titles(), vec!["New"]);
        assert_eq!(tab.lib.annotations(p.id.clone()).len(), 1);
    }

    #[test]
    fn drive_file_existing_library_is_found_by_its_id_even_when_renamed() {
        let m = Mock::start();
        m.inner.lock().unwrap().file_scope = true;
        let w = World::new();
        let a = file_scope_dev(&w, "a", &m);
        a.lib.add_paper(meta("Kept", "10.1/kept"), Some(b"%PDF".to_vec()), None, None).unwrap();
        a.sync();
        let b = file_scope_dev(&w, "b", &m);
        b.sync();
        assert_eq!(b.titles(), vec!["Kept"]);
        // the user renames the folder in Drive: both keep syncing with it (by id, not by name)
        let root = m.id_of("Lunta").unwrap();
        {
            let mut g = m.inner.lock().unwrap();
            g.files.iter_mut().find(|f| f.id == root).unwrap().name = "Lunta library".into();
            g.log.push(root.clone());
        }
        let b2 = w.api_with("b", Box::new(drive_with(&m, 3_600_000, Some(FILE), DriveRoot::default())), false); // restarted app
        b2.lib.add_paper(meta("After rename", "10.1/after"), None, None, None).unwrap();
        let r = b2.sync();
        assert!(r.uploaded >= 1, "{r:?}");
        a.sync();
        assert_eq!(a.titles(), vec!["After rename", "Kept"]);
        assert!(m.everything().iter().all(|(p, _)| !p.starts_with("Lunta/")), "no second \"Lunta\" folder was created");
    }

    #[test]
    fn drive_file_foreign_folder_needs_the_picker_and_picking_a_folder_is_not_enough() {
        let m = Mock::start();
        // a library put into Drive by another app (Drive for desktop, the web)
        m.put("ops/mac.jsonl", b"{\"at\":1}\n", 1_700_000_000_000);
        m.put("papers/p1/paper.pdf", b"pdf one", 1_700_000_000_000);
        let root = m.id_of("Lunta").unwrap();
        m.inner.lock().unwrap().file_scope = true;
        let w = World::new();

        // known by id (synced before, or chosen): not visible -> the "Choose your Lunta folder" error
        let d = drive_with(&m, 3_600_000, Some(FILE), DriveRoot::Id(root.clone()));
        let e = d.list(None).unwrap_err();
        assert!(is_folder_not_visible(&e.to_string()), "{e}");
        assert!(ffi::drive_folder_not_visible(e.to_string()));

        // the user picks the folder: it opens, but the files already in it are still not granted
        m.pick(&[root.clone()]);
        let file = m.id_of("Lunta/ops/mac.jsonl").unwrap();
        assert_eq!(d.first_folder(&[file, root.clone()]).unwrap(), Some((root.clone(), "Lunta".to_string())), "the picked folder among the ids");
        let ph = w.api_with("phone", Box::new(drive_with(&m, 3_600_000, Some(FILE), DriveRoot::Id(root.clone()))), false);
        let r = ph.sync();
        assert_eq!(r.downloaded, 0, "{r:?}");
        // picking the files too (multi-select in the picker) grants them
        let ids: Vec<String> = ["Lunta/ops/mac.jsonl", "Lunta/papers/p1/paper.pdf", "Lunta/ops", "Lunta/papers", "Lunta/papers/p1"].iter().map(|p| m.id_of(p).unwrap()).collect();
        m.pick(&ids);
        let ph = w.api_with("phone2", Box::new(drive_with(&m, 3_600_000, Some(FILE), DriveRoot::Id(root.clone()))), false);
        let r = ph.sync();
        assert_eq!(r.downloaded, 2, "{r:?}");
        assert_eq!(read(&ph.root.join("papers/p1/paper.pdf")), b"pdf one");
    }

    #[test]
    fn drive_file_lost_folder_is_reported_not_replaced() {
        let m = Mock::start();
        m.inner.lock().unwrap().file_scope = true;
        let w = World::new();
        let ph = file_scope_dev(&w, "phone", &m);
        ph.lib.add_paper(meta("One", "10.1/one"), None, None, None).unwrap();
        ph.sync();
        // access to the folder is gone (made by another app / access removed): no new empty folder
        m.inner.lock().unwrap().files.iter_mut().filter(|f| f.app).for_each(|f| f.app = false);
        drop(ph);
        let ph = file_scope_dev(&w, "phone", &m); // the app starts again with its saved sync state
        let e = try_sync(&ph).unwrap_err();
        assert!(is_folder_not_visible(&e.to_string()), "{e}");
        assert_eq!(m.everything().iter().filter(|(p, _)| p == "Lunta").count(), 1);
    }

    #[test]
    fn migration_adopts_foreign_files_with_the_old_token_and_deletes_nothing() {
        let m = Mock::start();
        // the library as Drive for desktop (or the web) made it: nothing in it is app-created
        m.put("ops/mac.jsonl", b"{\"at\":1}\n", 1_700_000_000_000);
        m.put("papers/p1/paper.pdf", b"pdf one", 1_700_000_000_000);
        m.put("smart-folders.json", b"[]", 1_700_000_000_000);
        let w = World::new();

        // an Android phone signed in with the old full scope (token from before scopes were saved)
        let ph = w.api_with("phone", Box::new(drive_with(&m, 3_600_000, None, DriveRoot::default())), false);
        let r = ph.sync();
        assert!(r.errors.is_empty(), "{r:?}");
        assert_eq!(r.downloaded, 3, "{r:?}");
        assert!(r.notices.first().is_some_and(|n| n.contains("3 file(s)") && n.contains("originals from before the update")), "{r:?}");
        let all = m.everything();
        let inside: Vec<_> = all.iter().filter(|(p, _)| p == "Lunta" || p.starts_with("Lunta/")).collect();
        assert!(inside.iter().all(|(_, app)| *app), "everything in the library is app-created now: {all:?}");
        // the originals are kept, same content, in the backup folder (the old folder, renamed)
        let backup = "Lunta (originals from before the update)";
        assert_eq!(m.read_full(&format!("{backup}/papers/p1/paper.pdf")).unwrap(), b"pdf one");
        assert_eq!(m.read_full(&format!("{backup}/ops/mac.jsonl")).unwrap(), b"{\"at\":1}\n");
        assert_eq!(m.read_full(&format!("{backup}/smart-folders.json")).unwrap(), b"[]");
        assert!(m.inner.lock().unwrap().files.iter().all(|f| !f.trashed), "nothing deleted");
        // same names, folders, times and content in the library
        assert_eq!(m.read("papers/p1/paper.pdf").unwrap(), b"pdf one");
        assert_eq!(util::file_sig(&ph.root.join("papers/p1/paper.pdf")).unwrap().1, 1_700_000_000_000);

        // next sync: incremental, nothing to transfer although the ids changed
        let lists = m.count("GET /files");
        let r = ph.sync();
        assert_eq!((r.uploaded, r.downloaded, r.deleted_local, r.deleted_remote), (0, 0, 0, 0), "{r:?}");
        assert_eq!(m.count("GET /files"), lists, "adopted once: no more listing");

        // still on the old scope, another app adds a paper in a new folder: adopted at once
        m.put("papers/p2/paper.pdf", b"pdf two", 1_700_000_100_000);
        let r = ph.sync();
        assert_eq!(r.downloaded, 1, "{r:?}");
        assert!(m.everything().iter().filter(|(p, _)| p.starts_with("Lunta/")).all(|(_, app)| *app), "{:?}", m.everything());
        assert_eq!(m.read_full(&format!("{backup}/papers/p2/paper.pdf")).unwrap(), b"pdf two");

        // the phone writes; then the user signs in again: the token is drive.file from now on
        ph.lib.add_paper(meta("Phone paper", "10.1/ph"), Some(b"%PDF ph".to_vec()), None, None).unwrap();
        ph.sync();
        {
            let mut g = m.inner.lock().unwrap();
            g.file_scope = true;
            g.grant_scope = Some(FILE.into());
            g.token = "rotated".into(); // 401 -> refresh -> a drive.file token
        }
        let r = ph.sync();
        assert!(r.errors.is_empty() && (r.uploaded, r.downloaded, r.deleted_local) == (0, 0, 0), "{r:?}");
        // a new device with drive.file sees the whole library
        let tab = file_scope_dev(&w, "tablet", &m);
        let r = tab.sync();
        assert!(r.errors.is_empty(), "{r:?}");
        assert_eq!(read(&tab.root.join("papers/p1/paper.pdf")), b"pdf one");
        assert_eq!(read(&tab.root.join("papers/p2/paper.pdf")), b"pdf two");
        assert_eq!(read(&tab.root.join("ops/mac.jsonl")), b"{\"at\":1}\n");
        assert_eq!(tab.titles(), vec!["Phone paper"]);
        // the limit: what another app adds now stays invisible to Lunta
        m.put("papers/p3/paper.pdf", b"pdf three", 1_700_000_200_000);
        let r = tab.sync();
        assert_eq!(r.downloaded, 0, "{r:?}");
        assert!(!tab.root.join("papers/p3/paper.pdf").exists());
    }

    #[test]
    fn migration_inside_an_app_created_library_adopts_only_the_foreign_items() {
        let m = Mock::start();
        let w = World::new();
        // the Mac bridge made the library through the API (old full scope)...
        let mac = w.api_with("mac", Box::new(drive_with(&m, 3_600_000, None, DriveRoot::default())), false);
        let p = mac.lib.add_paper(meta("Bridged", "10.1/b"), Some(b"%PDF b".to_vec()), None, None).unwrap();
        let r = mac.sync();
        assert!(r.notices.is_empty(), "nothing to adopt: {r:?}");
        let root = m.id_of("Lunta").unwrap();
        // ...and the user dropped a PDF into the paper's folder by hand
        m.put(&format!("papers/{}/extra.pdf", p.id), b"dropped", 1_700_000_000_000);
        let r = mac.sync();
        assert!(r.notices.first().is_some_and(|n| n.contains("1 file(s)")), "{r:?}");
        assert_eq!(m.id_of("Lunta").unwrap(), root, "an app-created library folder stays");
        assert_eq!(m.read(&format!("papers/{}/extra.pdf", p.id)).unwrap(), b"dropped");
        assert_eq!(m.read_full(&format!("Lunta (originals from before the update)/papers/{}/extra.pdf", p.id)).unwrap(), b"dropped");
        assert!(m.everything().iter().filter(|(p, _)| p.starts_with("Lunta/")).all(|(_, app)| *app));
    }
}

// ---- real Google Drive (needs credentials; run with `cargo test --features drive -- --ignored`) ----
//
// LUNTA_GOOGLE_CLIENT_ID / LUNTA_GOOGLE_CLIENT_SECRET: the OAuth client; LUNTA_TEST_TOKEN_FILE: a tokens
// file made by `lunta-sync connect` (state-dir/google-tokens.json); LUNTA_TEST_DRIVE_FOLDER: id of an
// empty scratch folder in Drive that Lunta can see (all test files are created inside it). With the
// drive.file scope that folder must be made by Lunta (e.g. `lunta-sync sync --folder-name lunta-scratch`
// on an empty library) or chosen in the picker. Never point it at a real library.

mod real_drive {
    use super::*;

    fn setup() -> Option<(Arc<Authenticator>, String)> {
        let folder = std::env::var("LUNTA_TEST_DRIVE_FOLDER").ok()?;
        let tokens = std::env::var("LUNTA_TEST_TOKEN_FILE").ok()?;
        let client = OAuthClient::from_env().ok()?;
        Some((Authenticator::new(client, Arc::new(FileTokenStore::new(tokens))), folder))
    }

    #[test]
    #[ignore = "needs a Google account: see the comment above"]
    fn two_devices_through_real_drive() {
        let Some((auth, folder)) = setup() else { panic!("set LUNTA_GOOGLE_CLIENT_ID, LUNTA_TEST_TOKEN_FILE, LUNTA_TEST_DRIVE_FOLDER") };
        let w = World::new();
        let mk = |n: &str| w.api_with(n, Box::new(GoogleDrive::new(auth.clone(), DriveRoot::Id(folder.clone()))), false);
        let (a, b) = (mk("test-a"), mk("test-b"));
        let p = a.lib.add_paper(meta("Real Drive", "10.1/real"), Some(b"%PDF real".to_vec()), Some(b"epub".to_vec()), Some("text".into())).unwrap();
        a.lib.save_annotation(note(&p.id, "hello")).unwrap();
        fs::write(a.root.join("smart-folders.json"), b"[\"x\"]").unwrap();
        a.sync();
        b.sync();
        assert_eq!(b.titles(), vec!["Real Drive"]);
        assert_eq!(b.lib.annotations(p.id.clone()).len(), 1);
        b.lib.set_starred(p.id.clone(), true).unwrap();
        b.sync();
        a.sync();
        assert!(a.lib.paper(p.id.clone()).unwrap().starred);
        // incremental: nothing to do now
        let r = a.sync();
        assert_eq!((r.uploaded, r.downloaded), (0, 0));
        // delete propagates (trash)
        a.lib.delete_paper(p.id.clone()).unwrap();
        a.sync();
        let r = b.sync();
        assert!(r.deleted_local >= 1);
        assert!(b.titles().is_empty());
    }
}

/// A Mac whose library is in iCloud (shared with an iPhone) bridges it to Drive (an Android phone).
#[test]
fn mac_bridges_icloud_and_drive() {
    let w = World::new();
    // the iCloud folder, written by the Mac and the iPhone
    let icloud = w.tmp.path().join("icloud");
    let open_at = |root: &Path, dev: &str| {
        let cache = w.tmp.path().join(format!("cache-{dev}"));
        Library::open(root.to_string_lossy().into(), cache.to_string_lossy().into(), dev.into()).unwrap()
    };
    let mac = open_at(&icloud, "mac");
    let iphone = open_at(&icloud, "iphone");
    let mut opts = SyncOptions::new(w.tmp.path().join("state-mac.json"));
    opts.bridge = true;
    let bridge = Syncer::new(&icloud, "mac", Box::new(LocalDirRemote::new(&w.remote)), opts).unwrap();
    let android = w.api("android");

    let a = mac.add_paper(meta("From Mac", "10.1/m"), Some(b"%PDF mac".to_vec()), None, None).unwrap();
    let b = iphone.add_paper(meta("From iPhone", "10.1/i"), None, None, None).unwrap();
    iphone.set_starred(a.id.clone(), true).unwrap();
    let r = bridge.sync_now().unwrap();
    assert!(r.errors.is_empty() && r.conflicts.is_empty(), "{r:?}");
    assert!(w.remote.join("ops/iphone.jsonl").exists(), "the iPhone's log is passed on to Drive");

    android.sync();
    assert_eq!(android.titles(), vec!["From Mac", "From iPhone"]);
    assert!(android.lib.paper(a.id.clone()).unwrap().starred, "the iPhone's star reaches Android");

    // Android's changes come back to the iCloud folder, for the iPhone too
    let c = android.lib.add_paper(meta("From Android", "10.1/a"), Some(b"%PDF android".to_vec()), None, None).unwrap();
    android.lib.set_must_read(b.id.clone(), true).unwrap();
    android.sync();
    let r = bridge.sync_now().unwrap();
    assert!(r.errors.is_empty() && r.changed, "{r:?}");
    assert!(icloud.join("ops/android.jsonl").exists());
    assert_eq!(read(&icloud.join(format!("papers/{}/paper.pdf", c.id))), b"%PDF android");
    iphone.refresh().unwrap();
    assert!(iphone.paper(c.id.clone()).is_some(), "Android's paper reaches the iPhone");
    assert!(iphone.paper(b.id.clone()).unwrap().must_read);

    // more iPhone changes later, and a second round both ways
    iphone.set_read(c.id.clone(), true).unwrap();
    bridge.sync_now().unwrap();
    android.sync();
    assert!(android.lib.paper(c.id.clone()).unwrap().read);
    // nothing left to do
    let r = bridge.sync_now().unwrap();
    assert_eq!((r.uploaded, r.downloaded), (0, 0), "{r:?}");
}
/// On a bridge, the Mac's and iPhone's positions files are published to Drive but never rewritten
/// in iCloud (a stale Drive copy must not clobber the iPhone's newer place); Android's are mirrored in.
#[test]
fn bridge_publishes_icloud_positions_without_rewriting_them() {
    let w = World::new();
    let icloud = w.tmp.path().join("icloud");
    fs::create_dir_all(icloud.join("positions")).unwrap();
    let mut opts = SyncOptions::new(w.tmp.path().join("state-mac.json"));
    opts.bridge = true;
    let bridge = Syncer::new(&icloud, "mac-0001", Box::new(LocalDirRemote::new(&w.remote)), opts).unwrap();
    let phone = icloud.join("positions/ios-0002.json");
    fs::write(&phone, br#"{"p1":{"offset":100,"at":1.0,"device":"iPhone"}}"#).unwrap();
    bridge.sync_now().unwrap();
    assert!(w.remote.join("positions/ios-0002.json").exists(), "published to Drive");

    // Drive's copy is older/different (another round-trip), the iPhone meanwhile read further
    fs::write(w.remote.join("positions/ios-0002.json"), br#"{"p1":{"offset":50,"at":0.5,"device":"iPhone"}}"#).unwrap();
    fs::write(&phone, br#"{"p1":{"offset":900,"at":3.0,"device":"iPhone"}}"#).unwrap();
    let r = bridge.sync_now().unwrap();
    assert!(r.errors.is_empty() && r.conflicts.is_empty(), "{r:?}");
    assert!(String::from_utf8(read(&phone)).unwrap().contains("900"), "the iPhone's file is not rewritten");
    assert!(String::from_utf8(read(&w.remote.join("positions/ios-0002.json"))).unwrap().contains("900"), "Drive gets the newer place");

    // evicted to a placeholder in iCloud: neither downloaded back nor deleted on Drive
    fs::remove_file(&phone).unwrap();
    bridge.sync_now().unwrap();
    assert!(!phone.exists() && w.remote.join("positions/ios-0002.json").exists());

    // Android's file comes into iCloud; conflict copies are ignored
    fs::write(w.remote.join("positions/android-0003.json"), br#"{"p1":{"offset":7,"at":4.0,"device":"Android"}}"#).unwrap();
    fs::write(icloud.join("positions/ios-0002 2.json"), b"{}").unwrap();
    bridge.sync_now().unwrap();
    assert!(icloud.join("positions/android-0003.json").exists());
    assert!(!w.remote.join("positions/ios-0002 2.json").exists());
}

/// The reader's Summary card is kept as papers/<id>/summary.json (apple/Shared/Summary.swift, and
/// core/src/llm/summary.rs for the desktop and Android apps): small, so it comes down even in lazy
/// mode, and a regenerated one goes back up and replaces the old, both ways.
#[test]
fn summaries_sync_both_ways_also_in_lazy_mode() {
    let w = World::new();
    let mac = w.mac();
    let a = mac.lib.add_paper(meta("Alpha", ""), Some(b"pdf bytes".to_vec()), Some(b"epub bytes".to_vec()), Some("full text".into())).unwrap();
    let pid = &a.id;
    let on_mac = w.remote.join(format!("papers/{pid}/summary.json"));
    fs::write(&on_mac, br#"{"created":"2026-11-19T09:30:00Z","model":"apple-on-device","source":"s1","text":"From the Mac."}"#).unwrap();
    set_time(&on_mac, 1_000_000);
    let ph = w.api_with("phone", Box::new(LocalDirRemote::new(&w.remote)), true);
    let r = ph.sync();
    assert_eq!(r.skipped_lazy, 2);
    let on_phone = ph.root.join(format!("papers/{pid}/summary.json"));
    assert!(read(&on_phone).ends_with(b"From the Mac.\"}"));
    // regenerated on the phone: replaces the Mac's
    fs::write(&on_phone, br#"{"created":"2026-11-20T09:30:00Z","model":"qwen3-1.7b","source":"s2","text":"From the phone."}"#).unwrap();
    set_time(&on_phone, 2_000_000);
    ph.sync();
    assert!(read(&on_mac).ends_with(b"From the phone.\"}"));
}
