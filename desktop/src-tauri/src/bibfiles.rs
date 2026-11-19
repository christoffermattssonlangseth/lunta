//! .bib files the app keeps up to date for a folder or the whole library ("Keep BibTeX File
//! Updated…"), e.g. in a Dropbox folder Overleaf syncs or a LaTeX project's Git repository. The
//! links are this install's (settings.json: paths differ per device), not the synced library's.
//! As on the Mac (apple/Mac/BibFiles.swift): papers sorted by citation key, the file written
//! atomically and only when its text changed.

use lunta_core::Library;
use std::collections::BTreeMap;
use std::path::Path;

/// The source for the whole library; other sources are folder ids.
pub const ALL_PAPERS: &str = "*";

/// The file's text, or None for an empty library (still opening or syncing: the file is left).
/// Entries kept from a .bib imported with its keys (software, web pages) come after the papers.
pub fn text(lib: &Library, source: &str) -> Option<String> {
    lib.kept_bibtex((source != ALL_PAPERS).then(|| source.to_string()))
}

/// Writes `text` to `path` if it differs from what is there (temp file + rename).
pub fn write_if_changed(path: &Path, text: &str) -> std::io::Result<bool> {
    if std::fs::read_to_string(path).is_ok_and(|old| old == text) {
        return Ok(false);
    }
    let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(".{}.lunta-tmp", path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default()));
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)?;
    Ok(true)
}

/// Updates every linked file; returns the problems (source -> message). Folders that aren't in
/// the library (deleted, or not synced yet) leave their file as it was.
pub fn update_all(lib: &Library, links: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let folders: Vec<String> = lib.collections().into_iter().map(|c| c.id).collect();
    let mut problems = BTreeMap::new();
    for (source, path) in links {
        if source != ALL_PAPERS && !folders.contains(source) {
            continue;
        }
        if let Some(t) = text(lib, source) {
            if let Err(e) = write_if_changed(Path::new(path), &t) {
                problems.insert(source.clone(), e.to_string());
            }
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;
    use lunta_core::model::PaperMeta;

    #[test]
    fn writes_only_when_changed() {
        let dir = std::env::temp_dir().join(format!("lunta-bib-{}", uuid::Uuid::new_v4()));
        let lib = Library::open(dir.join("lib").display().to_string(), dir.join("cache").display().to_string(), "test".into()).unwrap();
        let path = dir.join("out/refs.bib");
        let mut links = BTreeMap::new();
        links.insert(ALL_PAPERS.to_string(), path.display().to_string());
        // an empty library leaves the file alone
        assert!(update_all(&lib, &links).is_empty());
        assert!(!path.exists());
        let meta = |t: &str, a: &str| PaperMeta { title: t.into(), authors: vec![a.into()], date: "2020".into(), ..Default::default() };
        lib.add_paper(meta("Zebra stripes", "Bea Young"), None, None, None).unwrap();
        lib.add_paper(meta("Ant colonies", "Al Adams"), None, None, None).unwrap();
        update_all(&lib, &links);
        let t = std::fs::read_to_string(&path).unwrap();
        let (a, y) = (t.find("{Adams2020ant,").unwrap(), t.find("{Young2020zebra,").unwrap());
        assert!(a < y, "{t}");
        let m1 = std::fs::metadata(&path).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert!(!write_if_changed(&path, &t).unwrap());
        assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), m1);
        let _ = std::fs::remove_dir_all(dir);
    }
}
