//! "Use this page's full text" (browser extension): the article page the reader has open, with
//! their access, becomes the paper's reading view. A port of apple/Mac/PageFullText.swift, for
//! the apps on the Rust core (Windows / Linux; later Android).
//!
//! - No such paper: it is added with the page's version.
//! - No reading view yet: the page's version is attached.
//! - A reading view with highlights: every highlight must still be found in the page's text (the
//!   reader's own `locate`, see [`highlight::would_find`]). If one wouldn't be, the current
//!   reading view stays and the page's version is only saved beside it in the backups folder,
//!   with the highlights that didn't match listed. Otherwise the current reading view (and its
//!   full text) is backed up and replaced.
//!
//! Backups: `<backups>/<paper id>-<date>/` (the app picks the folder, outside the library).

use crate::convert::{self, Options, PageImage};
use crate::model::{FileKind, Paper, PaperFilter, PaperMeta, SortKey};
use crate::{highlight, Library};
use std::path::{Path, PathBuf};

/// What the browser sent.
#[derive(Debug, Clone, Default)]
pub struct PageSubmission {
    /// The page as the server sent it.
    pub html: String,
    /// The page as the browser shows it (after scripts ran), tried when `html` can't be converted.
    pub live_html: Option<String>,
    pub url: String,
    pub doi: String,
    pub title: String,
    /// Figure images (and linked table pages) the browser fetched with the reader's access.
    pub images: Vec<PageImage>,
    /// Figures the browser couldn't send (too large, refused).
    pub images_skipped: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageOutcomeKind {
    Added,
    Attached,
    Replaced,
    Kept,
    Unchanged,
}

impl PageOutcomeKind {
    pub fn name(self) -> &'static str {
        match self {
            PageOutcomeKind::Added => "added",
            PageOutcomeKind::Attached => "attached",
            PageOutcomeKind::Replaced => "replaced",
            PageOutcomeKind::Kept => "kept",
            PageOutcomeKind::Unchanged => "unchanged",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PageOutcome {
    pub kind: PageOutcomeKind,
    pub paper_id: Option<String>,
    pub message: String,
    pub words: usize,
    pub images: usize,
    pub notes: Vec<String>,
    /// e.g. "Full text from cell.com"
    pub source: String,
    /// the reader used, e.g. "Atypon" (see `convert::page_reader`)
    pub reader: String,
    /// highlights in the reading view that was there
    pub highlights: usize,
    /// quotes of those the page's text wouldn't have
    pub unmatched: Vec<String>,
    /// the folder the previous reading view (or the page's, when kept) was saved in
    pub backup: Option<String>,
    pub dry_run: bool,
}

impl PageOutcome {
    /// The fields of the extension endpoint's answer (as Lunta for Mac sends them).
    pub fn to_json(&self) -> serde_json::Map<String, serde_json::Value> {
        let mut j = serde_json::Map::new();
        j.insert("outcome".into(), self.kind.name().into());
        j.insert("message".into(), self.message.clone().into());
        j.insert("words".into(), self.words.into());
        j.insert("images".into(), self.images.into());
        j.insert("notes".into(), self.notes.clone().into());
        j.insert("source".into(), self.source.clone().into());
        j.insert("reader".into(), self.reader.clone().into());
        j.insert("highlights".into(), self.highlights.into());
        j.insert("unmatched".into(), self.unmatched.clone().into());
        j.insert("dryRun".into(), self.dry_run.into());
        if let Some(p) = &self.paper_id {
            j.insert("paperId".into(), p.clone().into());
        }
        if let Some(b) = &self.backup {
            j.insert("backup".into(), b.clone().into());
        }
        j
    }
}

/// A paper by DOI, else by exact (normalised) title.
pub fn find_paper(lib: &Library, doi: &str, title: &str) -> Option<Paper> {
    if !doi.is_empty() {
        if let Some(p) = lib.find_by_doi(doi.to_string()) {
            return Some(p);
        }
    }
    if title.chars().count() <= 12 {
        return None;
    }
    let norm = |s: &str| s.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect::<String>();
    let t = norm(title);
    lib.papers(PaperFilter::default(), SortKey::Added).into_iter().find(|p| norm(&p.meta.title) == t)
}

/// The text of a reading view as the reader sees it (the continuous document's body).
pub fn reader_text(epub: &Path, cache: &Path) -> Result<String, String> {
    let book = crate::epub::open(epub, cache).map_err(|e| e.to_string())?;
    let x = std::fs::read_to_string(&book.combined).map_err(|e| e.to_string())?;
    Ok(highlight::dom_text(&x))
}

fn tmp_dir() -> PathBuf {
    std::env::temp_dir().join(format!("lunta-page-{}", uuid::Uuid::new_v4()))
}

/// "yyyy-MM-dd-HHmmss" (UTC).
pub fn backup_stamp() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    // days → civil date (Howard Hinnant's algorithm)
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}-{:02}{:02}{:02}", rem / 3600, rem % 3600 / 60, rem % 60)
}

fn plural(n: usize, one: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {one}s")
    }
}

/// Converts the page and attaches it as described in the module documentation. `backups`: the
/// folder for backups. `dry_run`: converts and checks, but changes nothing (no paper added, no
/// file written to the library, no backup).
pub fn use_page_full_text(
    lib: &Library,
    s: &PageSubmission,
    backups: &Path,
    dry_run: bool,
    opts: &Options,
) -> Result<PageOutcome, String> {
    let tmp = tmp_dir();
    let r = run(lib, s, backups, dry_run, opts, &tmp);
    let _ = std::fs::remove_dir_all(&tmp);
    r
}

fn run(lib: &Library, s: &PageSubmission, backups: &Path, dry_run: bool, opts: &Options, tmp: &Path) -> Result<PageOutcome, String> {
    // 1. convert: the page as the server sent it, else as the browser shows it
    let mut used = &s.html;
    let result = match convert::convert_page_with(&s.html, &s.url, s.images.clone(), &tmp.join("out"), opts) {
        Ok(r) => r,
        Err(e) => {
            let Some(live) = s.live_html.as_ref().filter(|l| !l.is_empty() && **l != s.html) else { return Err(e.to_string()) };
            opts.log(&format!("The page as sent didn't convert ({e}); trying the page as shown"));
            used = live;
            convert::convert_page_with(live, &s.url, s.images.clone(), &tmp.join("out"), opts).map_err(|e| e.to_string())?
        }
    };
    let reader = convert::page_reader(&s.url, used).to_string();
    let epub = std::fs::read(&result.file).map_err(|_| "The reading view couldn't be written.".to_string())?;
    let text = result.fulltext.clone();
    let mut notes = result.notes.clone();
    if s.images_skipped > 0 {
        notes.push(format!("{} figure image(s) were too large to send.", s.images_skipped));
    }
    let mut out = PageOutcome {
        kind: PageOutcomeKind::Attached,
        paper_id: None,
        message: String::new(),
        words: result.words,
        images: result.images,
        notes,
        source: result.source.clone(),
        reader,
        highlights: 0,
        unmatched: vec![],
        backup: None,
        dry_run,
    };
    let figures = plural(result.images, "figure");
    let words = result.words;

    // 2. the paper
    let doi = if result.meta.doi.is_empty() { s.doi.clone() } else { result.meta.doi.clone() };
    let title = if result.meta.title.is_empty() { &s.title } else { &result.meta.title };
    let Some(paper) = find_paper(lib, &doi, title) else {
        out.kind = PageOutcomeKind::Added;
        if dry_run {
            out.message = format!("Dry run: would add this paper with the page's full text ({words} words, {figures}).");
            return Ok(out);
        }
        let mut meta = PaperMeta::default();
        if !doi.is_empty() && !opts.offline {
            meta = crate::resolve_metadata(doi.clone()).unwrap_or_default();
        }
        if meta.title.is_empty() {
            let c = &result.meta;
            meta = PaperMeta {
                title: c.title.clone(),
                authors: c.authors.clone(),
                date: c.date.clone(),
                journal: c.journal.clone(),
                doi: c.doi.clone(),
                abstract_text: c.abstract_text.clone(),
                license: c.license.clone(),
                keywords: c.keywords.clone(),
                url: c.url.clone(),
                ..Default::default()
            };
        }
        if meta.doi.is_empty() {
            meta.doi = doi;
        }
        if meta.url.is_empty() {
            meta.url = s.url.clone();
        }
        let added = lib.add_paper(meta, None, Some(epub), Some(text)).map_err(|e| e.to_string())?;
        out.paper_id = Some(added.id);
        out.message = format!("Added to Lunta with the page's full text ({words} words, {figures}).");
        return Ok(out);
    };
    out.paper_id = Some(paper.id.clone());
    let id = paper.id.clone();

    // 3. no reading view yet: attach
    if !paper.has_epub {
        out.kind = PageOutcomeKind::Attached;
        if dry_run {
            out.message = format!("Dry run: would make the reading view from this page ({words} words, {figures}).");
            return Ok(out);
        }
        lib.attach_file(id.clone(), FileKind::Epub, epub).map_err(|e| e.to_string())?;
        lib.attach_file(id, FileKind::Fulltext, text.into_bytes()).map_err(|e| e.to_string())?;
        out.message = format!("Reading view made from this page ({words} words, {figures}).");
        return Ok(out);
    }

    // 4. a reading view already: its highlights must all be found in the new text
    let anchored: Vec<_> =
        lib.annotations(id.clone()).into_iter().filter(|a| a.location == "epub" && !a.quote.trim().is_empty()).collect();
    let new_text = reader_text(&result.file, &tmp.join("book"))?;
    let unmatched: Vec<_> = anchored.iter().filter(|a| !highlight::would_find(&new_text, &a.quote)).collect();
    out.highlights = anchored.len();
    out.unmatched = unmatched.iter().map(|a| a.quote.chars().take(160).collect()).collect();
    let stamp = backup_stamp();
    let n_all = anchored.len();

    if !unmatched.is_empty() {
        out.kind = PageOutcomeKind::Kept;
        let n = unmatched.len();
        let which = unmatched
            .iter()
            .take(3)
            .map(|a| {
                let q: String = a.quote.chars().take(60).collect();
                format!("“{q}{}”", if a.quote.chars().count() > 60 { "…" } else { "" })
            })
            .collect::<Vec<_>>()
            .join(", ");
        out.message = format!(
            "Kept the current reading view: {n} of {n_all} highlight{} wouldn't be found in this page's text ({which}{}).",
            if n_all == 1 { "" } else { "s" },
            if n > 3 { ", …" } else { "" }
        );
        if dry_run {
            out.message = format!("Dry run: {}", out.message);
            return Ok(out);
        }
        // the page's version is kept beside it, in case it's wanted after all
        let folder = backups.join(format!("{id}-{stamp}-page-not-used"));
        std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        std::fs::write(folder.join("page-book.epub"), &epub).map_err(|e| e.to_string())?;
        std::fs::write(folder.join("page-fulltext.txt"), &text).map_err(|e| e.to_string())?;
        let list = unmatched.iter().map(|a| format!("- {}", a.quote.replace('\n', " "))).collect::<Vec<_>>().join("\n");
        std::fs::write(folder.join("unmatched-highlights.txt"), format!("Highlights not found in the page's text ({}):\n{list}\n", s.url))
            .map_err(|e| e.to_string())?;
        let path = folder.to_string_lossy().to_string();
        out.message += &format!(" The page's version is saved in {path}.");
        out.backup = Some(path);
        return Ok(out);
    }

    out.kind = PageOutcomeKind::Replaced;
    if dry_run {
        out.message = format!(
            "Dry run: would replace the reading view ({n_all} highlight{} all found in the new text) after backing up the current one.",
            if n_all == 1 { "" } else { "s" }
        );
        return Ok(out);
    }
    // back up the current reading view (and its text), then replace it
    let old = lib
        .file_path(id.clone(), FileKind::Epub)
        .and_then(|p| std::fs::read(p).ok())
        .ok_or("The current reading view isn't on this computer yet (it is still syncing), so it can't be backed up. Try again in a moment.")?;
    if old == epub {
        out.kind = PageOutcomeKind::Unchanged;
        out.message = "The reading view already matches this page.".into();
        return Ok(out);
    }
    let folder = backups.join(format!("{id}-{stamp}"));
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    std::fs::write(folder.join("book.epub"), &old).map_err(|e| e.to_string())?;
    if let Some(ft) = lib.file_path(id.clone(), FileKind::Fulltext).and_then(|p| std::fs::read(p).ok()) {
        std::fs::write(folder.join("fulltext.txt"), ft).map_err(|e| e.to_string())?;
    }
    lib.attach_file(id.clone(), FileKind::Epub, epub).map_err(|e| e.to_string())?;
    lib.attach_file(id, FileKind::Fulltext, text.into_bytes()).map_err(|e| e.to_string())?;
    let path = folder.to_string_lossy().to_string();
    let kept = if anchored.is_empty() {
        String::new()
    } else {
        format!(" All {n_all} highlight{} found in the new text.", if n_all == 1 { "" } else { "s" })
    };
    out.message = format!("Reading view updated from this page ({words} words, {figures}).{kept} The previous one is saved in {path}.");
    out.backup = Some(path);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    const CELL: &str = include_str!("convert/testdata/page-atypon-cell.html");
    const URL: &str = "https://www.cell.com/cell/fulltext/S0092-8674(26)00001-1";

    fn page(thanks: &str) -> String {
        let filler = (0..600).map(|i| format!("word{}", i % 97)).collect::<Vec<_>>().join(" ");
        CELL.replace("{{FILLER}}", &filler).replace("We thank the test suite.", thanks)
    }

    fn library(dir: &Path) -> Arc<Library> {
        Library::open(
            dir.join("lib").to_string_lossy().into(),
            dir.join("cache").to_string_lossy().into(),
            "test".into(),
        )
        .unwrap()
    }

    fn sub(html: String) -> PageSubmission {
        PageSubmission { html, url: URL.into(), ..Default::default() }
    }

    #[test]
    fn highlights_are_found_as_the_reader_finds_them() {
        let text = "The quick brown fox — “jumps” over\n the   lazy dog.";
        assert!(highlight::would_find(text, "brown fox"));
        assert!(highlight::would_find(text, "over the lazy"));
        assert!(highlight::would_find(text, "fox - \"jumps\" OVER"));
        assert!(!highlight::would_find(text, "slow red fox"));
        assert!(!highlight::would_find(text, "  "));
    }

    #[test]
    fn page_full_text_adds_replaces_and_keeps() {
        let dir = tempfile::tempdir().unwrap();
        let lib = library(dir.path());
        let backups = dir.path().join("backups");
        let opts = Options { offline: true, ..Default::default() };

        // a dry run changes nothing
        let o = use_page_full_text(&lib, &sub(page("We thank the test suite.")), &backups, true, &opts).unwrap();
        assert_eq!((o.kind, o.dry_run, o.reader.as_str()), (PageOutcomeKind::Added, true, "Cell Press / Lancet"));
        assert!(lib.papers(PaperFilter::default(), SortKey::Added).is_empty());

        // not in the library: added with the page's reading view
        let o = use_page_full_text(&lib, &sub(page("We thank the test suite.")), &backups, false, &opts).unwrap();
        assert_eq!(o.kind, PageOutcomeKind::Added);
        let id = o.paper_id.clone().unwrap();
        let p = lib.paper(id.clone()).unwrap();
        assert!(p.has_epub);
        assert_eq!(p.meta.doi, "10.1016/j.cell.2026.01.001");
        assert!(o.message.starts_with("Added to Lunta with the page's full text"), "{}", o.message);

        // highlights that the new version still has: backed up and replaced
        lib.add_highlight(id.clone(), "We thank the test suite.".into(), None, "yellow".into(), String::new()).unwrap();
        lib.add_highlight(id.clone(), "Cells matter".into(), None, "green".into(), String::new()).unwrap();
        let o = use_page_full_text(&lib, &sub(page("We thank the test suite. And the reviewers.")), &backups, false, &opts).unwrap();
        assert_eq!(o.kind, PageOutcomeKind::Replaced, "{}", o.message);
        assert_eq!(o.highlights, 2);
        let backup = PathBuf::from(o.backup.clone().unwrap());
        assert!(backup.join("book.epub").exists() && backup.join("fulltext.txt").exists());
        assert!(o.message.contains("All 2 highlights found in the new text."), "{}", o.message);

        // a page without one of the highlighted passages: the reading view stays
        let before = std::fs::read(lib.file_path(id.clone(), FileKind::Epub).unwrap()).unwrap();
        let o = use_page_full_text(&lib, &sub(page("We thank nobody in particular.")), &backups, false, &opts).unwrap();
        assert_eq!(o.kind, PageOutcomeKind::Kept, "{}", o.message);
        assert_eq!(o.unmatched, vec!["We thank the test suite."]);
        assert!(o.message.starts_with("Kept the current reading view: 1 of 2 highlights wouldn't be found in this page's text (“We thank the test suite.”)."), "{}", o.message);
        let kept = PathBuf::from(o.backup.unwrap());
        assert!(kept.to_string_lossy().ends_with("-page-not-used"));
        assert!(kept.join("page-book.epub").exists() && kept.join("unmatched-highlights.txt").exists());
        assert_eq!(std::fs::read(lib.file_path(id.clone(), FileKind::Epub).unwrap()).unwrap(), before);

        // a page that isn't the full text: an error, nothing changes
        let e = use_page_full_text(&lib, &sub("<html><body><p>Sign in</p></body></html>".into()), &backups, false, &opts).unwrap_err();
        assert!(e.starts_with("This page doesn't look like the article's full text"), "{e}");
    }

    #[test]
    fn backup_stamp_looks_like_a_date() {
        let s = backup_stamp();
        assert_eq!(s.len(), "2026-11-16-224700".len());
        assert!(s.starts_with("20"));
    }
}
