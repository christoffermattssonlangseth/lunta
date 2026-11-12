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

    fn drive_for(m: &Mock, expires_in_ms: i64) -> GoogleDrive {
        let mut client = OAuthClient::new("cid", Some("secret".into()));
        client.token_uri = format!("{}/token", m.addr);
        let store = Arc::new(MemoryTokenStore::default());
        store.save(&Tokens { access_token: "tok-0".into(), refresh_token: Some("refresh".into()), expires_at_ms: now_ms() + expires_in_ms }).unwrap();
        let auth = Authenticator::new(client, store);
        GoogleDrive::new(auth, DriveRoot::default()).with_endpoints(&m.api(), &m.upload_api(), std::time::Duration::from_millis(1))
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
        store.save(&Tokens { access_token: "tok-0".into(), refresh_token: Some("r".into()), expires_at_ms: now_ms() + 3_600_000 }).unwrap();
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
}

// ---- real Google Drive (needs credentials; run with `cargo test --features drive -- --ignored`) ----
//
// LUNTA_GOOGLE_CLIENT_ID / LUNTA_GOOGLE_CLIENT_SECRET: the OAuth client; LUNTA_TEST_TOKEN_FILE: a tokens
// file made by `lunta-sync connect` (state-dir/google-tokens.json); LUNTA_TEST_DRIVE_FOLDER: id of an
// empty scratch folder in Drive (all test files are created inside it).

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
