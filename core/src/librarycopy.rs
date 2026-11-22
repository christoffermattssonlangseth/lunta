//! Copying a library into another folder when it moves (Windows/Linux: "Move to Google Drive…",
//! "Move to a Folder on This Computer…"), with the rules of the Apple apps' `LibraryCopyRule`
//! (apple/Shared/StorageDecision.swift) and `LibraryLocation.copyLibrary`:
//!
//! - only the library's own files: `ops/*.jsonl`, `positions/*.json`, `papers/<id>/*` and the root
//!   `*.json` files (smart folders, watches, New for You); nothing hidden (sync bookkeeping);
//! - what the destination lacks is copied;
//! - a change log both have is merged line by line (logs are append-only, nothing is lost);
//! - a positions file both have is merged per paper (the later `at` wins);
//! - any other file both have is replaced only when the source's copy is newer;
//! - nothing in the source is changed or deleted.

use std::path::Path;
use std::time::SystemTime;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyRule {
    Copy,
    /// A change log (ops/<device>.jsonl) present in both places: add the lines the destination lacks.
    MergeLines,
    /// A device's reading positions (positions/<device>.json) present in both places: per paper,
    /// the newer position wins.
    MergePositions,
    /// Present in both places and the source's copy is newer: overwrite the destination's.
    Replace,
    Skip,
}

/// What to do with the library-relative `path` ("papers/<id>/paper.pdf").
pub fn rule(path: &str, destination_has_it: bool, source_is_newer: bool) -> CopyRule {
    let parts: Vec<&str> = path.split('/').collect();
    if parts.iter().any(|p| p.is_empty() || p.starts_with('.') || *p == "..") {
        return CopyRule::Skip;
    }
    let wanted = match parts.len() {
        1 => parts[0].ends_with(".json"),
        2 => (parts[0] == "ops" && parts[1].ends_with(".jsonl")) || (parts[0] == "positions" && parts[1].ends_with(".json") && !parts[1].contains(' ')),
        3 => parts[0] == "papers",
        _ => false,
    };
    if !wanted {
        return CopyRule::Skip;
    }
    if !destination_has_it {
        return CopyRule::Copy;
    }
    match parts[0] {
        "ops" if parts.len() == 2 => CopyRule::MergeLines,
        "positions" if parts.len() == 2 => CopyRule::MergePositions,
        _ if source_is_newer => CopyRule::Replace,
        _ => CopyRule::Skip,
    }
}

/// The destination log with the source's missing lines added at the end, or None if it has them all.
pub fn merged_log(destination: &str, source: &str) -> Option<String> {
    let have: std::collections::HashSet<&str> = destination.split('\n').filter(|l| !l.is_empty()).collect();
    let mut seen = std::collections::HashSet::new();
    let missing: Vec<&str> = source
        .split('\n')
        .filter(|l| !l.trim().is_empty() && !have.contains(l) && seen.insert(*l))
        .collect();
    if missing.is_empty() {
        return None;
    }
    let mut out = destination.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&missing.join("\n"));
    out.push('\n');
    Some(out)
}

/// Two copies of one device's positions file (`{paper: {offset, at, device}}`) merged: every paper
/// from both, the one with the later `at` where both have it. None if the destination already has
/// everything (or the source can't be read); a destination that can't be read is replaced by the source.
pub fn merged_positions(destination: &[u8], source: &[u8]) -> Option<Vec<u8>> {
    type Map = serde_json::Map<String, serde_json::Value>;
    let src: Map = serde_json::from_slice(source).ok()?;
    let Ok(mut out) = serde_json::from_slice::<Map>(destination) else { return Some(source.to_vec()) };
    let at = |v: Option<&serde_json::Value>| v.and_then(|v| v.get("at")).and_then(|a| a.as_f64());
    let mut changed = false;
    for (paper, spot) in src {
        let mine = out.get(&paper);
        if mine.is_some() {
            match (at(mine), at(Some(&spot))) {
                (Some(a), Some(b)) if a >= b => continue,
                (_, None) => continue,
                _ => {}
            }
        }
        out.insert(paper, spot);
        changed = true;
    }
    changed.then(|| serde_json::to_vec(&out).unwrap_or_default())
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CopyResult {
    pub copied: u32,
    pub merged: u32,
    /// Library-relative paths that could not be copied.
    pub failed: Vec<String>,
}

/// The library's files under `src`, library-relative with '/'.
fn library_files(src: &Path) -> Vec<String> {
    let names = |dir: &Path| -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(dir)
            .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect())
            .unwrap_or_default();
        v.sort();
        v
    };
    let mut out = Vec::new();
    for name in names(src) {
        let p = src.join(&name);
        if p.is_dir() {
            match name.as_str() {
                "ops" | "positions" => out.extend(names(&p).into_iter().filter(|n| p.join(n).is_file()).map(|n| format!("{name}/{n}"))),
                "papers" => {
                    for id in names(&p) {
                        let d = p.join(&id);
                        if d.is_dir() {
                            out.extend(names(&d).into_iter().filter(|n| d.join(n).is_file()).map(|n| format!("papers/{id}/{n}")));
                        }
                    }
                }
                _ => {}
            }
        } else {
            out.push(name);
        }
    }
    out
}

fn modified(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

/// What `copy_library` would do: (path, rule) for every file that isn't skipped.
pub fn plan(src: &Path, dst: &Path) -> Vec<(String, CopyRule)> {
    library_files(src)
        .into_iter()
        .filter_map(|rel| {
            let to = dst.join(&rel);
            let there = to.exists();
            let newer = there && matches!((modified(&src.join(&rel)), modified(&to)), (Some(a), Some(b)) if a > b);
            match rule(&rel, there, newer) {
                CopyRule::Skip => None,
                r => Some((rel, r)),
            }
        })
        .collect()
}

/// Copies the library at `src` into `dst` (made if needed) with the rules above. `progress` gets
/// (done, total). Files that fail are listed; the rest still go. `src` is only read.
pub fn copy_library(src: &Path, dst: &Path, progress: &dyn Fn(usize, usize)) -> CopyResult {
    let mut res = CopyResult::default();
    if std::fs::create_dir_all(dst).is_err() {
        res.failed.push(dst.display().to_string());
        return res;
    }
    let plan = plan(src, dst);
    for (i, (rel, rule)) in plan.iter().enumerate() {
        if i % 10 == 0 || i + 1 == plan.len() {
            progress(i + 1, plan.len());
        }
        let from = src.join(rel);
        let to = dst.join(rel);
        // Ok(1): copied, Ok(2): merged, Ok(0): nothing to do
        let ok: std::io::Result<u8> = (|| {
            if let Some(dir) = to.parent() {
                std::fs::create_dir_all(dir)?;
            }
            match rule {
                CopyRule::Copy | CopyRule::Replace => {
                    // copied next to it first, so a failed copy leaves the destination's as it was
                    let tmp = to.with_file_name(format!(".lunta-move-{}", uuid::Uuid::new_v4().simple()));
                    if let Err(e) = std::fs::copy(&from, &tmp) {
                        let _ = std::fs::remove_file(&tmp);
                        return Err(e);
                    }
                    if let Some(t) = modified(&from) {
                        let _ = std::fs::File::options().write(true).open(&tmp).and_then(|f| f.set_modified(t));
                    }
                    std::fs::rename(&tmp, &to).inspect_err(|_| {
                        let _ = std::fs::remove_file(&tmp);
                    })?;
                    Ok(1)
                }
                CopyRule::MergeLines => {
                    let have = std::fs::read_to_string(&to)?;
                    let add = std::fs::read_to_string(&from)?;
                    match merged_log(&have, &add) {
                        Some(m) => write_replacing(&to, m.as_bytes()).map(|_| 2),
                        None => Ok(0),
                    }
                }
                CopyRule::MergePositions => {
                    let have = std::fs::read(&to)?;
                    let add = std::fs::read(&from)?;
                    match merged_positions(&have, &add) {
                        Some(m) => write_replacing(&to, &m).map(|_| 2),
                        None => Ok(0),
                    }
                }
                CopyRule::Skip => Ok(0),
            }
        })();
        match ok {
            Ok(1) => res.copied += 1,
            Ok(2) => res.merged += 1,
            Ok(_) => {}
            Err(_) => res.failed.push(rel.clone()),
        }
    }
    res
}

fn write_replacing(to: &Path, data: &[u8]) -> std::io::Result<()> {
    let tmp = to.with_file_name(format!(".lunta-move-{}", uuid::Uuid::new_v4().simple()));
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, to).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// Bytes in the library's files under `root` (hidden sync bookkeeping left out).
pub fn library_size(root: &Path) -> u64 {
    library_files(root).iter().filter_map(|rel| std::fs::metadata(root.join(rel)).ok()).map(|m| m.len()).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn rules_like_the_apple_apps() {
        assert_eq!(rule("papers/p1/paper.pdf", false, false), CopyRule::Copy);
        assert_eq!(rule("papers/p1/paper.pdf", true, false), CopyRule::Skip);
        assert_eq!(rule("papers/p1/paper.pdf", true, true), CopyRule::Replace);
        assert_eq!(rule("ops/mac-1.jsonl", true, false), CopyRule::MergeLines);
        assert_eq!(rule("positions/mac-1.json", true, false), CopyRule::MergePositions);
        assert_eq!(rule("positions/mac-1 2.json", false, false), CopyRule::Skip, "a sync client's duplicate");
        assert_eq!(rule("smart-folders.json", false, false), CopyRule::Copy);
        assert_eq!(rule("notes.txt", false, false), CopyRule::Skip);
        assert_eq!(rule(".lunta-sync/state.json", false, false), CopyRule::Skip);
        assert_eq!(rule("papers/.x/paper.pdf", false, false), CopyRule::Skip);
        assert_eq!(rule("papers/p1/sub/x", false, false), CopyRule::Skip);
        assert_eq!(rule("ops/x.txt", false, false), CopyRule::Skip);
    }

    #[test]
    fn logs_merge_line_by_line() {
        assert_eq!(merged_log("a\nb\n", "a\nb\n"), None);
        assert_eq!(merged_log("a\nb", "b\nc\n\nc\n").as_deref(), Some("a\nb\nc\n"));
        assert_eq!(merged_log("", "x\n").as_deref(), Some("x\n"));
    }

    #[test]
    fn positions_merge_per_paper() {
        let dst = br#"{"p1":{"offset":10,"at":200,"device":"d"},"p2":{"offset":5,"at":100,"device":"d"}}"#;
        let src = br#"{"p1":{"offset":3,"at":100,"device":"d"},"p2":{"offset":9,"at":300,"device":"d"},"p3":{"offset":1,"at":1,"device":"d"}}"#;
        let m: serde_json::Value = serde_json::from_slice(&merged_positions(dst, src).unwrap()).unwrap();
        assert_eq!(m["p1"]["offset"], 10);
        assert_eq!(m["p2"]["offset"], 9);
        assert_eq!(m["p3"]["offset"], 1);
        assert_eq!(merged_positions(dst, br#"{"p1":{"offset":3,"at":100}}"#), None);
        assert_eq!(merged_positions(b"broken", src).as_deref(), Some(&src[..]));
        assert_eq!(merged_positions(dst, b"broken"), None);
    }

    #[test]
    fn copies_merges_and_leaves_the_source_alone() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        let s = src.path();
        let d = dst.path();
        for (p, c) in [
            ("ops/linux-1.jsonl", "op1\nop2\n"),
            ("ops/mac-1.jsonl", "m1\n"),
            ("positions/linux-1.json", r#"{"p1":{"offset":4,"at":50}}"#),
            ("papers/p1/paper.pdf", "pdf"),
            ("papers/p1/meta.json", "new meta"),
            ("smart-folders.json", "[]"),
            (".lunta-sync/state.json", "{}"),
            ("stray.txt", "x"),
        ] {
            fs::create_dir_all(s.join(p).parent().unwrap()).unwrap();
            fs::write(s.join(p), c).unwrap();
        }
        // the destination already holds part of this library (it was synced before)
        fs::create_dir_all(d.join("ops")).unwrap();
        fs::create_dir_all(d.join("papers/p1")).unwrap();
        fs::create_dir_all(d.join("positions")).unwrap();
        fs::write(d.join("ops/linux-1.jsonl"), "op1\n").unwrap();
        fs::write(d.join("ops/android-1.jsonl"), "a1\n").unwrap();
        fs::write(d.join("positions/linux-1.json"), r#"{"p2":{"offset":7,"at":60}}"#).unwrap();
        fs::write(d.join("papers/p1/meta.json"), "old meta").unwrap();
        let old = SystemTime::now() - std::time::Duration::from_secs(3600);
        fs::File::options().write(true).open(d.join("papers/p1/meta.json")).unwrap().set_modified(old).unwrap();

        let before: Vec<(String, Vec<u8>)> = library_files(s).into_iter().map(|r| (r.clone(), fs::read(s.join(&r)).unwrap())).collect();
        let calls = std::cell::Cell::new(0);
        let r = copy_library(s, d, &|_, _| calls.set(calls.get() + 1));
        assert!(r.failed.is_empty(), "{r:?}");
        assert!(calls.get() > 0);
        assert_eq!(fs::read_to_string(d.join("ops/linux-1.jsonl")).unwrap(), "op1\nop2\n");
        assert_eq!(fs::read_to_string(d.join("ops/mac-1.jsonl")).unwrap(), "m1\n");
        assert_eq!(fs::read_to_string(d.join("ops/android-1.jsonl")).unwrap(), "a1\n", "the destination's other logs stay");
        let pos: serde_json::Value = serde_json::from_slice(&fs::read(d.join("positions/linux-1.json")).unwrap()).unwrap();
        assert_eq!((pos["p1"]["offset"].as_i64(), pos["p2"]["offset"].as_i64()), (Some(4), Some(7)));
        assert_eq!(fs::read_to_string(d.join("papers/p1/meta.json")).unwrap(), "new meta", "the newer copy wins");
        assert_eq!(fs::read_to_string(d.join("papers/p1/paper.pdf")).unwrap(), "pdf");
        assert!(d.join("smart-folders.json").exists());
        assert!(!d.join(".lunta-sync").exists() && !d.join("stray.txt").exists());
        assert!(fs::read_dir(d.join("papers/p1")).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().starts_with('.')), "no temporary files left");
        // the source is untouched
        for (rel, data) in before {
            assert_eq!(fs::read(s.join(&rel)).unwrap(), data, "{rel}");
        }
        assert!(s.join("stray.txt").exists() && s.join(".lunta-sync/state.json").exists());
        // a second run has nothing left to do
        assert!(plan(s, d).iter().all(|(_, r)| matches!(r, CopyRule::MergeLines | CopyRule::MergePositions)));
        let again = copy_library(s, d, &|_, _| {});
        assert_eq!((again.copied, again.failed.len()), (0, 0));
        assert!(library_size(d) > 0);
    }
}
