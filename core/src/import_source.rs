//! Importing a library on Windows, Linux and Android: what was picked (an export file, the folder
//! Zotero's "Export Files" makes, or a ZIP of it), where the export's PDFs are on this device, and
//! the import itself in batches with progress, then the picked folder's other PDFs matched to the
//! papers by DOI or title. A port of apple/Shared/ImportSource.swift and LibraryModel.runImport /
//! importPDFFolder (the Apple apps keep theirs, with PDFKit). The parsing is `import`'s.

use crate::import::{self, ImportEntry};
use crate::model::{CoreError, FileKind, PaperFilter, Result, SortKey};
use crate::Library;
use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// What was picked to import a library from.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct ImportSource {
    /// The .bib or .ris file (or an EndNote .txt that is RIS).
    pub export: String,
    /// The folder or unpacked ZIP that was picked (none for a single file): the export's PDF paths
    /// are looked for in it too, and its other PDFs are matched to the papers by DOI or title.
    pub root: Option<String>,
    /// An unpacked ZIP, removed when the import is done (or by `discard_import_source`).
    pub temporary: Option<String>,
    /// The folder name to suggest: an exported collection keeps its name ("Thesis.bib" → "Thesis").
    pub suggested_folder: String,
}

/// An export read, with its PDFs found on this device.
#[derive(uniffi::Record, Clone, Debug, Default)]
pub struct ExportRead {
    /// The entries, their PDF paths replaced by the files found (others left as they were).
    pub entries: Vec<ImportEntry>,
    /// The PDFs found (canonical paths), not matched again from the folder.
    pub found: Vec<String>,
    /// Entries whose export names PDFs none of which was found.
    pub missing: u32,
}

/// The counts at the end of an import (the Apple apps' ImportSummary).
#[derive(uniffi::Record, Clone, Debug, Default)]
pub struct ImportSummary {
    pub added: u32,
    pub merged: u32,
    pub skipped: u32,
    /// PDFs the export named and that were found.
    pub pdfs: u32,
    /// More PDFs from the picked folder, matched to papers by DOI or title.
    pub matched: u32,
    /// Papers whose export names a PDF that wasn't found.
    pub missing_pdfs: u32,
    /// The folder the papers were put in, if any.
    pub folder: Option<String>,
    /// Papers added or updated (for reading views afterwards).
    pub paper_ids: Vec<String>,
    /// "Added 312 papers, merged 4 with papers already in Lunta, 280 PDFs."
    pub text: String,
}

impl ImportSummary {
    fn sentence(&self) -> String {
        let n = self.pdfs + self.matched;
        let mut s = format!("Added {} {}", self.added, if self.added == 1 { "paper" } else { "papers" });
        if self.merged > 0 {
            s += &format!(", merged {} with papers already in Lunta", self.merged);
        }
        s += &format!(", {n} {}.", if n == 1 { "PDF" } else { "PDFs" });
        if let Some(f) = self.folder.as_ref().filter(|f| !f.is_empty()) {
            s += &format!(" In the folder “{f}”.");
        }
        if self.skipped > 0 {
            s += &format!(" {} without a title or DOI left out.", self.skipped);
        }
        if self.missing_pdfs > 0 {
            s += &format!(" {} PDFs named in the export weren't found.", self.missing_pdfs);
        }
        s
    }
}

/// What matching a folder of PDFs to the library did.
#[derive(uniffi::Record, Clone, Debug, Default)]
pub struct PdfMatchReport {
    pub by_doi: u32,
    pub by_title: u32,
    /// Matched a paper that already had a PDF (left as it was).
    pub already: u32,
    /// File names of PDFs that matched no paper (not added, to avoid duplicates).
    pub unmatched: Vec<String>,
    /// "PDFs: 12 attached (10 by DOI, 2 by title), 3 not matched to a paper."
    pub text: String,
}

/// Progress of an import: a line for the status bar, and how far it has got (`done` of `total`).
#[uniffi::export(with_foreign)]
pub trait ImportProgress: Send + Sync {
    fn on_import_progress(&self, message: String, done: u32, total: u32);
}

fn say(p: &Option<Arc<dyn ImportProgress>>, m: String, done: usize, total: usize) {
    if let Some(p) = p {
        p.on_import_progress(m, done as u32, total as u32);
    }
}

fn inv(m: impl Into<String>) -> CoreError {
    CoreError::Invalid(m.into())
}

fn no_export(name: &str) -> CoreError {
    inv(format!(
        "“{name}” has no BibTeX (.bib) or RIS (.ris) export in it. In Zotero, use File › Export Library… with the format BibTeX and Export Files ticked, then choose the folder it makes."
    ))
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| p.display().to_string())
}

fn hidden(p: &Path) -> bool {
    p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with('.') || n == "__MACOSX")
}

/// Every file under `dir` (not hidden ones, nor in hidden folders), with its depth below `dir`.
fn walk(dir: &Path) -> Vec<(PathBuf, usize)> {
    let mut out = vec![];
    let mut stack = vec![(dir.to_path_buf(), 0usize)];
    while let Some((d, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        let mut entries: Vec<PathBuf> = rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| !hidden(p)).collect();
        entries.sort();
        for p in entries {
            // symlinked folders are not followed (no loops); symlinked files are fine
            let is_link_dir = std::fs::symlink_metadata(&p).map(|m| m.file_type().is_symlink()).unwrap_or(false) && p.is_dir();
            if p.is_dir() && !is_link_dir {
                stack.push((p, depth + 1));
            } else if p.is_file() {
                out.push((p, depth));
            }
        }
    }
    out
}

/// Opens what was picked: a folder (its export is found in it), a ZIP (unpacked into a new folder
/// in `temp_dir`, then the same) or an export file. Blocks.
#[uniffi::export]
pub fn open_import_source(path: String, temp_dir: String) -> Result<ImportSource> {
    let p = PathBuf::from(&path);
    if p.is_dir() {
        let export = find_export(&p).ok_or_else(|| no_export(&file_name(&p)))?;
        return Ok(source(export, Some(p), None));
    }
    if !p.is_file() {
        return Err(CoreError::NotFound(path));
    }
    let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let is_zip = ext == "zip" || std::fs::File::open(&p).ok().and_then(|mut f| {
        let mut head = [0u8; 4];
        f.read_exact(&mut head).ok().map(|_| head == *b"PK\x03\x04")
    }) == Some(true);
    if is_zip {
        let dir = PathBuf::from(temp_dir).join(format!("import-{}", uuid::Uuid::new_v4()));
        let r = (|| {
            std::fs::create_dir_all(&dir)?;
            let f = std::fs::File::open(&p)?;
            let mut zip = zip::ZipArchive::new(f).map_err(|e| inv(format!("“{}” isn't a ZIP file that can be read: {e}", file_name(&p))))?;
            zip.extract(&dir).map_err(|e| CoreError::Io(e.to_string()))?;
            let export = find_export(&dir).ok_or_else(|| no_export(&file_name(&p)))?;
            Ok(source(export, Some(dir.clone()), Some(dir.clone())))
        })();
        if r.is_err() {
            let _ = std::fs::remove_dir_all(&dir);
        }
        return r;
    }
    Ok(source(p, None, None))
}

fn source(export: PathBuf, root: Option<PathBuf>, temporary: Option<PathBuf>) -> ImportSource {
    let suggested_folder = export.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let s = |p: PathBuf| p.to_string_lossy().to_string();
    ImportSource { export: s(export), root: root.map(s), temporary: temporary.map(s), suggested_folder }
}

/// Removes an unpacked ZIP (when the import is cancelled; `import_library` does it itself).
#[uniffi::export]
pub fn discard_import_source(source: ImportSource) {
    if let Some(t) = source.temporary {
        let _ = std::fs::remove_dir_all(t);
    }
}

/// The export in a folder: a .bib, .ris (or an EndNote .txt that is RIS), the one nearest the top
/// (Zotero puts it next to its "files" folder), BibTeX before RIS, then the largest.
pub fn find_export(folder: &Path) -> Option<PathBuf> {
    let bib_start = regex::Regex::new(r"@\w+\s*[{(]").unwrap();
    let mut found: Vec<(usize, u8, i64, PathBuf)> = vec![];
    for (p, depth) in walk(folder) {
        let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        let kind = match ext.as_str() {
            "bib" | "bibtex" => 0,
            "ris" => 1,
            "txt" => {
                // a .txt only if it starts like an export ("TY  - " or "@article{")
                let mut head = vec![0u8; 4096];
                let n = std::fs::File::open(&p).and_then(|mut f| f.read(&mut head)).unwrap_or(0);
                let s = String::from_utf8_lossy(&head[..n]);
                if !(s.contains("TY  - ") || bib_start.is_match(&s)) {
                    continue;
                }
                2
            }
            _ => continue,
        };
        let size = std::fs::metadata(&p).map(|m| m.len() as i64).unwrap_or(0);
        found.push((depth, kind, -size, p));
    }
    found.into_iter().min().map(|f| f.3)
}

/// Where a PDF path from an export is on this device: the path itself, the path relative to one of
/// `roots` (Zotero: "files/12/paper.pdf"), or its last folders under a root (a path from another
/// computer, "/Users/me/Zotero/storage/AB12CD/paper.pdf" → "storage/AB12CD/paper.pdf", also with
/// Windows' backslashes).
pub fn locate(path: &str, roots: &[PathBuf]) -> Option<PathBuf> {
    let p = Path::new(path);
    if p.is_file() {
        return Some(p.to_path_buf());
    }
    let parts: Vec<&str> = path.split(['/', '\\']).filter(|s| !s.is_empty() && *s != "~" && *s != ".." && *s != ".").collect();
    // a drive ("C:") is never part of a path relative to the export
    let parts: Vec<&str> = parts.into_iter().filter(|s| !(s.len() == 2 && s.ends_with(':'))).collect();
    for k in (1..=parts.len()).rev() {
        let tail = &parts[parts.len() - k..];
        for root in roots {
            let u = tail.iter().fold(root.clone(), |acc, s| acc.join(s));
            if u.is_file() {
                return Some(u);
            }
        }
    }
    None
}

/// The export's text: UTF-8, else Latin-1 (older EndNote and RIS files).
pub fn read_text(path: &Path) -> Result<String> {
    let data = std::fs::read(path)?;
    Ok(match String::from_utf8(data) {
        Ok(s) => s,
        Err(e) => e.into_bytes().iter().map(|&b| b as char).collect(),
    })
}

/// Reads an export and finds its PDFs, looking next to the export and in `source.root`. Blocks.
#[uniffi::export]
pub fn read_import_source(source: &ImportSource) -> Result<ExportRead> {
    let export = PathBuf::from(&source.export);
    let mut roots: Vec<PathBuf> = export.parent().map(|d| vec![d.to_path_buf()]).unwrap_or_default();
    if let Some(r) = &source.root {
        roots.push(PathBuf::from(r));
    }
    let text = read_text(&export)?;
    let mut found = HashSet::new();
    let mut missing = 0;
    let entries = import::parse_any(&text)
        .into_iter()
        .map(|mut e| {
            let mut any = false;
            e.files = e
                .files
                .iter()
                .map(|f| match locate(f, &roots) {
                    Some(u) => {
                        any = true;
                        found.insert(canonical(&u));
                        u.to_string_lossy().to_string()
                    }
                    None => f.clone(),
                })
                .collect();
            if !e.files.is_empty() && !any {
                missing += 1;
            }
            e
        })
        .collect();
    let mut found: Vec<String> = found.into_iter().collect();
    found.sort();
    Ok(ExportRead { entries, found, missing })
}

fn canonical(p: &Path) -> String {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf()).to_string_lossy().to_string()
}

fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect()
}

#[uniffi::export]
impl Library {
    /// Imports what was picked: the export in batches of 20 (progress after each), the papers put
    /// in `folder` when one is given (made if needed; one with the same name is used), then the
    /// picked folder's other PDFs matched to the papers by DOI or title. An unpacked ZIP is removed
    /// at the end. Reading views are left to the app (in the background, afterwards). Blocks.
    pub fn import_library(&self, source: ImportSource, folder: Option<String>, progress: Option<Arc<dyn ImportProgress>>) -> Result<ImportSummary> {
        let r = self.import_library_inner(&source, folder, &progress);
        discard_import_source(source);
        r
    }

    /// Matches a folder of PDFs (e.g. ReadCube Papers' "Export PDFs") to papers in the library: by
    /// the DOI printed in the PDF, else by finding a paper's title on its first pages. PDFs that
    /// match nothing are reported, not added (no duplicates). `skipping`: PDFs already taken from an
    /// export (canonical paths). Blocks.
    pub fn match_pdf_folder(&self, folder: String, skipping: Vec<String>, progress: Option<Arc<dyn ImportProgress>>) -> Result<PdfMatchReport> {
        let dir = PathBuf::from(&folder);
        if !dir.is_dir() {
            return Err(CoreError::NotFound(folder));
        }
        let skip: HashSet<String> = skipping.into_iter().collect();
        let pdfs: Vec<PathBuf> = walk(&dir)
            .into_iter()
            .map(|(p, _)| p)
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf")) && !skip.contains(&canonical(p)))
            .collect();
        let mut rep = PdfMatchReport::default();
        // nothing left once the export's own PDFs are taken (a Zotero export folder)
        if pdfs.is_empty() && !skip.is_empty() {
            return Ok(rep);
        }
        let titles: Vec<(String, String)> = self
            .papers(PaperFilter::default(), SortKey::Added)
            .into_iter()
            .map(|p| (p.id, norm(&p.meta.title)))
            .filter(|(_, t)| t.chars().count() >= 25)
            .collect();
        for (i, path) in pdfs.iter().enumerate() {
            say(&progress, format!("Matching PDFs: {} of {}", i + 1, pdfs.len()), i, pdfs.len());
            let Ok(data) = std::fs::read(path) else { continue };
            if !data.starts_with(b"%PDF") {
                rep.unmatched.push(file_name(path));
                continue;
            }
            let by_doi = crate::convert::pdf::doi_in_pdf(&data).and_then(|d| self.find_by_doi(d)).map(|p| p.id);
            let hit = match by_doi {
                Some(id) => Some((id, true)),
                None => {
                    let text = norm(&crate::convert::pdf::first_pages_text(&data, 2));
                    // the longest title found on the first pages wins (not a short generic one)
                    titles.iter().filter(|(_, t)| text.contains(t.as_str())).max_by_key(|(_, t)| t.len()).map(|(id, _)| (id.clone(), false))
                }
            };
            let Some((id, doi)) = hit else {
                rep.unmatched.push(file_name(path));
                continue;
            };
            if self.paper(id.clone()).is_some_and(|p| p.has_pdf) {
                rep.already += 1;
                continue;
            }
            self.attach_file(id, FileKind::Pdf, data)?;
            if doi {
                rep.by_doi += 1;
            } else {
                rep.by_title += 1;
            }
        }
        let n = rep.by_doi + rep.by_title;
        rep.text = format!("PDFs: {n} attached ({} by DOI, {} by title), {} not matched to a paper.", rep.by_doi, rep.by_title, rep.unmatched.len());
        Ok(rep)
    }
}

impl Library {
    fn import_library_inner(&self, source: &ImportSource, folder: Option<String>, progress: &Option<Arc<dyn ImportProgress>>) -> Result<ImportSummary> {
        let name = file_name(Path::new(&source.export));
        say(progress, format!("Importing {name}…"), 0, 0);
        let read = read_import_source(source)?;
        let total = read.entries.len();
        let mut sum = ImportSummary { missing_pdfs: read.missing, ..Default::default() };
        let mut seen = HashSet::new();
        for (b, batch) in read.entries.chunks(20).enumerate() {
            let start = b * 20;
            say(progress, format!("Importing {name}: {start} of {total} papers"), start, total);
            let r = self.import_entries(batch.to_vec())?;
            sum.added += r.added;
            sum.merged += r.merged;
            sum.skipped += r.skipped;
            sum.pdfs += r.pdfs_copied;
            for id in r.paper_ids {
                if seen.insert(id.clone()) {
                    sum.paper_ids.push(id);
                }
            }
        }
        say(progress, format!("Importing {name}: {total} of {total} papers"), total, total);
        if let Some(f) = folder.map(|f| f.trim().to_string()).filter(|f| !f.is_empty()) {
            if !sum.paper_ids.is_empty() {
                let col = match self.collections().into_iter().find(|c| c.name.to_lowercase() == f.to_lowercase()) {
                    Some(c) => c,
                    None => self.create_collection(f.clone(), None)?,
                };
                for id in &sum.paper_ids {
                    self.set_membership(col.id.clone(), id.clone(), true)?;
                }
                sum.folder = Some(f);
            }
        }
        if let Some(root) = &source.root {
            sum.matched = self.match_pdf_folder(root.clone(), read.found.clone(), progress.clone())?.by_doi_and_title();
        }
        sum.text = sum.sentence();
        Ok(sum)
    }
}

impl PdfMatchReport {
    fn by_doi_and_title(&self) -> u32 {
        self.by_doi + self.by_title
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BIB: &str = r#"
@article{a,
  title = {A first paper about synthetic exports and their files},
  author = {Doe, Jane},
  year = {2021},
  doi = {10.1234/first},
  file = {Full Text PDF:files/1/a.pdf:application/pdf}
}
@article{b,
  title = {A second paper whose PDF lives on another computer},
  author = {Roe, Rick},
  year = {2020},
  file = {Full Text:/Users/someone/Zotero/storage/AB12CD/b.pdf:application/pdf}
}
@article{c,
  title = {A third paper whose PDF went missing somewhere},
  author = {Poe, Pat},
  year = {2019},
  file = {Full Text PDF:files/3/gone.pdf:application/pdf}
}
@article{d,
  title = {A fourth paper with no file at all in the export},
  author = {Moe, Max},
  year = {2018},
  doi = {10.1234/fourth}
}
"#;

    /// A Zotero-style "Export Files" folder: Thesis/Thesis.bib, files/1/a.pdf, storage/AB12CD/b.pdf
    /// (the path from another computer), no files/3/gone.pdf, and extra/unnamed.pdf.
    fn export_folder(dir: &Path) -> PathBuf {
        let root = dir.join("Thesis");
        for (rel, data) in [
            ("Thesis.bib", BIB.as_bytes()),
            ("files/1/a.pdf", b"%PDF-1.4 a".as_slice()),
            ("storage/AB12CD/b.pdf", b"%PDF-1.4 b".as_slice()),
            ("extra/unnamed.pdf", b"%PDF-1.4 x".as_slice()),
            ("notes.txt", b"just notes".as_slice()),
            (".hidden.bib", b"@article{h, title={hidden}}".as_slice()),
        ] {
            let p = root.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, data).unwrap();
        }
        root
    }

    fn zip_folder(root: &Path, out: &Path) {
        let mut z = zip::ZipWriter::new(std::fs::File::create(out).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        for (p, _) in walk(root) {
            let rel = p.strip_prefix(root.parent().unwrap()).unwrap().to_string_lossy().replace('\\', "/");
            z.start_file(rel, opts).unwrap();
            std::io::Write::write_all(&mut z, &std::fs::read(&p).unwrap()).unwrap();
        }
        z.finish().unwrap();
    }

    fn library(dir: &Path) -> Arc<Library> {
        let root = dir.join("lib");
        let cache = dir.join("cache");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&cache).unwrap();
        Library::open(root.to_string_lossy().into(), cache.to_string_lossy().into(), "test".into()).unwrap()
    }

    #[test]
    fn finds_the_export_and_its_pdfs() {
        let tmp = tempfile::tempdir().unwrap();
        let root = export_folder(tmp.path());
        let src = open_import_source(root.to_string_lossy().into(), tmp.path().to_string_lossy().into()).unwrap();
        assert!(src.export.ends_with("Thesis.bib"));
        assert_eq!(src.suggested_folder, "Thesis");
        assert!(src.temporary.is_none());
        let r = read_import_source(&src).unwrap();
        assert_eq!(r.entries.len(), 4);
        assert!(Path::new(&r.entries[0].files[0]).is_file(), "files/1/a.pdf next to the export");
        assert!(r.entries[1].files[0].ends_with("storage/AB12CD/b.pdf") && Path::new(&r.entries[1].files[0]).is_file(), "another computer's path, by its last folders");
        assert_eq!(r.found.len(), 2);
        assert_eq!(r.missing, 1);

        // a path with Windows' backslashes and a drive
        let w = locate("C:\\Users\\me\\Zotero\\storage\\AB12CD\\b.pdf", &[root.clone()]).unwrap();
        assert!(w.ends_with("storage/AB12CD/b.pdf"));
        assert!(locate("/nowhere/at/all.pdf", &[root.clone()]).is_none());
    }

    #[test]
    fn export_file_alone_and_zip() {
        let tmp = tempfile::tempdir().unwrap();
        let root = export_folder(tmp.path());
        // the .bib alone: its PDFs are still found next to it
        let alone = open_import_source(root.join("Thesis.bib").to_string_lossy().into(), tmp.path().to_string_lossy().into()).unwrap();
        assert!(alone.root.is_none());
        assert_eq!(read_import_source(&alone).unwrap().found.len(), 2);

        // the folder zipped (Thesis/… inside)
        let z = tmp.path().join("Thesis.zip");
        zip_folder(&root, &z);
        let temp = tmp.path().join("t");
        std::fs::create_dir_all(&temp).unwrap();
        let src = open_import_source(z.to_string_lossy().into(), temp.to_string_lossy().into()).unwrap();
        let unpacked = PathBuf::from(src.temporary.clone().unwrap());
        assert!(unpacked.starts_with(&temp));
        assert_eq!(src.suggested_folder, "Thesis");
        let r = read_import_source(&src).unwrap();
        assert_eq!((r.entries.len(), r.found.len(), r.missing), (4, 2, 1));
        discard_import_source(src);
        assert!(!unpacked.exists());

        // a folder without an export
        let empty = tmp.path().join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        let e = open_import_source(empty.to_string_lossy().into(), temp.to_string_lossy().into()).unwrap_err();
        assert!(e.to_string().contains("no BibTeX"));
    }

    #[test]
    fn export_txt_is_found_only_when_it_is_an_export() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path().join("e");
        std::fs::create_dir_all(d.join("sub")).unwrap();
        std::fs::write(d.join("readme.txt"), "hello").unwrap();
        std::fs::write(d.join("sub/endnote.txt"), "TY  - JOUR\nTI  - A title\nER  - \n").unwrap();
        assert!(find_export(&d).unwrap().ends_with("sub/endnote.txt"));
        std::fs::write(d.join("sub/lib.ris"), "TY  - JOUR\nER  - \n").unwrap();
        assert!(find_export(&d).unwrap().ends_with("sub/lib.ris"), "RIS before a .txt");
        std::fs::write(d.join("top.bib"), "@article{x, title={T}}").unwrap();
        assert!(find_export(&d).unwrap().ends_with("top.bib"), "nearest the top");
    }

    struct Count(std::sync::Mutex<Vec<(String, u32, u32)>>);
    impl ImportProgress for Count {
        fn on_import_progress(&self, message: String, done: u32, total: u32) {
            self.0.lock().unwrap().push((message, done, total));
        }
    }

    #[test]
    fn imports_into_a_folder_from_a_zip() {
        let tmp = tempfile::tempdir().unwrap();
        let root = export_folder(tmp.path());
        let z = tmp.path().join("Thesis.zip");
        zip_folder(&root, &z);
        let lib = library(tmp.path());
        let src = open_import_source(z.to_string_lossy().into(), tmp.path().to_string_lossy().into()).unwrap();
        let unpacked = PathBuf::from(src.temporary.clone().unwrap());
        let p = Arc::new(Count(Default::default()));
        let s = lib.import_library(src, Some("Thesis".into()), Some(p.clone())).unwrap();
        assert_eq!((s.added, s.merged, s.pdfs, s.missing_pdfs), (4, 0, 2, 1));
        assert_eq!(s.folder.as_deref(), Some("Thesis"));
        assert_eq!(s.paper_ids.len(), 4);
        assert!(s.text.starts_with("Added 4 papers, "), "{}", s.text);
        assert!(s.text.contains("In the folder “Thesis”") && s.text.contains("1 PDFs named in the export weren't found"), "{}", s.text);
        assert!(!unpacked.exists(), "the unpacked ZIP is removed");
        let col = lib.collections().into_iter().find(|c| c.name == "Thesis").unwrap();
        let inside = lib.papers(PaperFilter { collection: Some(col.id.clone()), ..Default::default() }, SortKey::Added);
        assert_eq!(inside.len(), 4);
        assert_eq!(inside.iter().filter(|p| p.has_pdf).count(), 2);
        let msgs = p.0.lock().unwrap();
        assert!(msgs.iter().any(|(m, d, t)| m.contains("4 of 4") && *d == 4 && *t == 4), "{msgs:?}");

        // again, from the folder: merged into the same papers and the same folder
        let src = open_import_source(root.to_string_lossy().into(), tmp.path().to_string_lossy().into()).unwrap();
        let s = lib.import_library(src, Some("thesis".into()), None).unwrap();
        assert_eq!((s.added, s.merged), (0, 4));
        assert_eq!(lib.collections().iter().filter(|c| c.name.eq_ignore_ascii_case("thesis")).count(), 1);
        assert!(root.exists(), "a picked folder is never removed");
    }

    /// Needs PDFium (PDFIUM_PATH): a PDF with a DOI printed in it and one with a paper's title.
    #[test]
    #[ignore]
    fn matches_pdfs_by_doi_and_title() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = library(tmp.path());
        let src = open_import_source(export_folder(tmp.path()).join("Thesis.bib").to_string_lossy().into(), tmp.path().to_string_lossy().into()).unwrap();
        lib.import_library(src, None, None).unwrap();
        let pdfs = tmp.path().join("pdfs");
        std::fs::create_dir_all(&pdfs).unwrap();
        std::fs::write(pdfs.join("x.pdf"), pdf_with_text("Journal of Tests. doi:10.1234/fourth")).unwrap();
        std::fs::write(pdfs.join("y.pdf"), pdf_with_text("A third paper whose PDF went missing somewhere")).unwrap();
        std::fs::write(pdfs.join("z.pdf"), pdf_with_text("Something else entirely")).unwrap();
        let r = lib.match_pdf_folder(pdfs.to_string_lossy().into(), vec![], None).unwrap();
        assert_eq!((r.by_doi, r.by_title, r.unmatched.clone()), (1, 1, vec!["z.pdf".to_string()]), "{}", r.text);
        assert_eq!(lib.papers(PaperFilter::default(), SortKey::Added).iter().filter(|p| p.has_pdf).count(), 4);
    }

    /// A one-page PDF showing `text` in Helvetica.
    fn pdf_with_text(text: &str) -> Vec<u8> {
        let content = format!("BT /F1 10 Tf 40 700 Td ({text}) Tj ET");
        let objs = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".to_string(),
            format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        ];
        let mut out = b"%PDF-1.4\n".to_vec();
        let mut offsets = vec![];
        for (i, o) in objs.iter().enumerate() {
            offsets.push(out.len());
            out.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).bytes());
        }
        let xref = out.len();
        out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).bytes());
        for o in offsets {
            out.extend(format!("{o:010} 00000 n \n").bytes());
        }
        out.extend(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).bytes());
        out
    }
}
