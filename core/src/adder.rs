//! Adding papers and making reading views with the Rust converter (feature "convert"), exported
//! to UniFFI for the Android app. Mirrors LibraryModel.add / makeReadingView / attachPDF in
//! apple/Shared and desktop/src-tauri/src/add.rs, and "Use this page's full text" (pagetext) for
//! the app's journal page. Everything here blocks (network), so call it off the main thread.

use crate::convert::{self, ConversionResult, Options};
use crate::pagetext::{self, PageOutcome, PageOutcomeKind, PageSubmission};
use crate::model::{CoreError, FileKind, Paper, PaperMeta, Result};
use crate::{Library, ProgressListener};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(uniffi::Record, Clone, Debug)]
pub struct AddOutcome {
    pub paper: Paper,
    /// Set when the paper was added but no reading view could be made (the converter's reason).
    pub note: Option<String>,
}

/// What "Use this page's full text" did (pagetext's [`PageOutcome`], for UniFFI).
#[derive(uniffi::Record, Clone, Debug)]
pub struct PageFullText {
    pub kind: PageOutcomeKind,
    pub paper_id: Option<String>,
    /// The core's own sentence (it names the backup folder).
    pub message: String,
    pub words: u32,
    pub images: u32,
    pub notes: Vec<String>,
    /// e.g. "Full text from cell.com"
    pub source: String,
    /// the reader used, e.g. "Atypon"
    pub reader: String,
    /// highlights in the reading view that was there
    pub highlights: u32,
    /// quotes of those the page's text wouldn't have (kept: the reading view stayed)
    pub unmatched: Vec<String>,
    /// the folder the previous reading view (or, when kept, the page's version) was saved in
    pub backup: Option<String>,
    pub dry_run: bool,
}

impl From<PageOutcome> for PageFullText {
    fn from(o: PageOutcome) -> Self {
        PageFullText {
            kind: o.kind,
            paper_id: o.paper_id,
            message: o.message,
            words: o.words as u32,
            images: o.images as u32,
            notes: o.notes,
            source: o.source,
            reader: o.reader,
            highlights: o.highlights as u32,
            unmatched: o.unmatched,
            backup: o.backup,
            dry_run: o.dry_run,
        }
    }
}

/// Where the PDFium library can be found (feature "pdf"); call once at start-up with the app's
/// native library directory. Without it the system library path is searched.
#[uniffi::export]
pub fn set_pdfium_path(path: String) {
    std::env::set_var("PDFIUM_PATH", path);
}

#[derive(Clone)]
struct Say(Option<Arc<dyn ProgressListener>>);

impl Say {
    fn say(&self, m: &str) {
        if let (Some(p), false) = (&self.0, m.trim().is_empty()) {
            p.on_progress(m.trim().to_string());
        }
    }
}

enum Conv {
    Input(String),
    File(PathBuf),
    /// A library paper's PDF, with the paper's DOI (an XML version is looked for by it first).
    PaperPdf(PathBuf, String),
}

type Converted = (Vec<u8>, String, ConversionResult);

fn run(input: Conv, say: &Say) -> std::result::Result<Converted, String> {
    let tmp = std::env::temp_dir().join(format!("lunta-conv-{}", uuid::Uuid::new_v4()));
    let s = say.clone();
    let doi = match &input {
        Conv::PaperPdf(_, d) if !d.is_empty() => Some(d.clone()),
        _ => None,
    };
    let opts = Options { log: Some(Box::new(move |m: &str| s.say(m))), offline: false, doi };
    let r = match &input {
        Conv::Input(s) => convert::convert_with(s, &tmp, &opts),
        Conv::File(p) | Conv::PaperPdf(p, _) => convert::convert_file_with(p, &tmp, &opts),
    };
    let out = match r {
        Ok(r) => std::fs::read(&r.file).map(|d| (d, r.fulltext.clone(), r)).map_err(|e| e.to_string()),
        Err(e) => Err(e.to_string()),
    };
    let _ = std::fs::remove_dir_all(&tmp);
    out
}

fn meta_from(c: &ConversionResult) -> PaperMeta {
    PaperMeta {
        title: c.meta.title.clone(),
        authors: c.meta.authors.clone(),
        date: c.meta.date.clone(),
        journal: c.meta.journal.clone(),
        doi: c.meta.doi.clone(),
        abstract_text: c.meta.abstract_text.clone(),
        license: c.meta.license.clone(),
        keywords: c.meta.keywords.clone(),
        url: c.meta.url.clone(),
        ..Default::default()
    }
}

fn inv(e: impl ToString) -> CoreError {
    CoreError::Invalid(e.to_string())
}

#[uniffi::export]
impl Library {
    /// Adds a paper by DOI, PMCID, PubMed id, arXiv id or link: metadata from the lookup, reading
    /// view from the converter when an open full text exists.
    pub fn add_from_input(&self, input: String, progress: Option<Arc<dyn ProgressListener>>) -> Result<AddOutcome> {
        let say = Say(progress);
        let input = input.trim().to_string();
        if input.is_empty() {
            return Err(inv("Type a DOI, PMCID, arXiv id or link first."));
        }
        // a plain web page or blog post (a page that declares a DOI is added as that paper)
        let mut web_error = None;
        if crate::webmeta::is_web_link(&input) {
            match self.add_web_page(input.clone(), None, say.0.clone()) {
                Ok(o) => return Ok(o),
                Err(e) => web_error = Some(e.to_string()),
            }
        }
        say.say(&format!("Looking up {input}…"));
        let meta = crate::resolve_metadata(input.clone()).ok();
        say.say("Making the reading view…");
        let conv = run(Conv::Input(input.clone()), &say);
        let mut m = meta.unwrap_or_default();
        if m.title.is_empty() {
            if let Ok((_, _, r)) = &conv {
                let from = meta_from(r);
                if m.doi.is_empty() {
                    m = from;
                } else {
                    m.title = from.title;
                    if m.authors.is_empty() {
                        m.authors = from.authors;
                    }
                }
            }
        }
        if m.title.is_empty() && m.doi.is_empty() {
            let why = web_error.unwrap_or_else(|| conv.err().unwrap_or_default());
            return Err(inv(format!("Couldn't find “{input}”. Try a DOI or a link to the article.{}", if why.is_empty() { String::new() } else { format!("\n{why}") })));
        }
        let (epub, text, note) = match conv {
            Ok((e, t, _)) => (Some(e), Some(t), None),
            Err(e) => (None, None, Some(e)),
        };
        let paper = self.add_paper(m, None, epub, text)?;
        Ok(AddOutcome { paper, note })
    }

    /// Adds a web page or blog post by its address: its details from the page (read today), its
    /// reading view from its main text (images fetched). `html`: the page as the browser has it
    /// (the browser extension's Save); without it the page is fetched once. A page that declares
    /// a DOI is added as that paper; one already in the library (same address) is kept, and
    /// gets the reading view if it had none. A page with too little text for a reading view is
    /// added with its details and a note saying so.
    pub fn add_web_page(&self, url: String, html: Option<String>, progress: Option<Arc<dyn ProgressListener>>) -> Result<AddOutcome> {
        let say = Say(progress.clone());
        let url = url.trim().to_string();
        let page = match html.filter(|h| !h.trim().is_empty()) {
            Some(h) => crate::webmeta::read(&h, &url, ""),
            None => {
                say.say(&format!("Reading {}…", url.split('/').nth(2).unwrap_or(&url)));
                let (final_url, h) = crate::webmeta::fetch(&url)?;
                crate::webmeta::read(&h, &final_url, "")
            }
        };
        if !page.scholarly_id.is_empty() {
            say.say(&format!("The page is a paper's: {}", page.scholarly_id));
            return self.add_from_input(page.scholarly_id.clone(), progress);
        }
        if let Some(p) = self.find_by_url(page.url.clone()) {
            if p.has_epub {
                return Ok(AddOutcome { paper: p, note: None });
            }
        }
        say.say("Making the reading view…");
        let tmp = std::env::temp_dir().join(format!("lunta-web-{}", uuid::Uuid::new_v4()));
        let s = say.clone();
        let opts = Options { log: Some(Box::new(move |m: &str| s.say(m))), offline: false, doi: None };
        let conv = convert::convert_web_page_with(&page, &tmp, &opts)
            .map_err(|e| e.to_string())
            .and_then(|r| std::fs::read(&r.file).map(|d| (d, r.fulltext.clone())).map_err(|e| e.to_string()));
        let _ = std::fs::remove_dir_all(&tmp);
        let mut m = page.meta.clone();
        if m.title.is_empty() {
            m.title = page.url.clone();
        }
        let (epub, text, note) = match conv {
            Ok((e, t)) => (Some(e), Some(t), None),
            Err(e) => (None, None, Some(e)),
        };
        let paper = self.add_paper(m, None, epub, text)?;
        Ok(AddOutcome { paper, note })
    }

    /// Adds a PDF file as a new paper (the converter reads its metadata and, via the DOI, an open
    /// full text if there is one); the PDF is kept.
    pub fn add_from_pdf(&self, path: String, progress: Option<Arc<dyn ProgressListener>>) -> Result<AddOutcome> {
        let say = Say(progress);
        let data = std::fs::read(&path).map_err(|e| inv(format!("Couldn't read the file: {e}")))?;
        if !data.starts_with(b"%PDF") {
            return Err(inv("That isn't a readable PDF."));
        }
        say.say("Reading the PDF…");
        let conv = run(Conv::File(PathBuf::from(&path)), &say);
        let mut m = match &conv {
            Ok((_, _, r)) => meta_from(r),
            Err(_) => PaperMeta::default(),
        };
        if !m.doi.is_empty() {
            if let Ok(full) = crate::resolve_metadata(m.doi.clone()) {
                if !full.title.is_empty() {
                    m = full;
                }
            }
        }
        if m.title.trim().is_empty() {
            m.title = Path::new(&path).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Untitled".into());
        }
        let (epub, text, note) = match conv {
            Ok((e, t, _)) => (Some(e), Some(t), None),
            Err(e) => (None, None, Some(e)),
        };
        let paper = self.add_paper(m, Some(data), epub, text)?;
        Ok(AddOutcome { paper, note })
    }

    /// Makes the reading view for a paper that has none: arXiv's HTML for arXiv papers, else the
    /// converter on the PDF, else the PMCID / DOI, else an open-access PDF.
    pub fn make_reading_view(&self, id: String, progress: Option<Arc<dyn ProgressListener>>) -> Result<()> {
        let say = Say(progress);
        let p = self.paper(id.clone()).ok_or(CoreError::NotFound(id.clone()))?;
        let arxiv = if !p.meta.arxiv.is_empty() {
            p.meta.arxiv.clone()
        } else if p.meta.doi.to_lowercase().starts_with("10.48550/arxiv.") {
            p.meta.doi["10.48550/arxiv.".len()..].to_string()
        } else {
            String::new()
        };
        let mut errors: Vec<String> = vec![];
        let mut result = None;
        if !arxiv.is_empty() {
            match run(Conv::Input(format!("arxiv:{arxiv}")), &say) {
                Ok(r) => result = Some(r),
                Err(e) => errors.push(e),
            }
        }
        // a PMC paper without a DOI: its XML beats the PDF
        if result.is_none() && p.meta.doi.is_empty() && !p.meta.pmcid.is_empty() {
            match run(Conv::Input(p.meta.pmcid.clone()), &say) {
                Ok(r) => result = Some(r),
                Err(e) => errors.push(e),
            }
        }
        if result.is_none() {
            if let Some(pdf) = self.file_path(id.clone(), FileKind::Pdf) {
                match run(Conv::PaperPdf(PathBuf::from(pdf), p.meta.doi.clone()), &say) {
                    Ok(r) => result = Some(r),
                    Err(e) => errors.push(e),
                }
            }
        }
        if result.is_none() {
            for input in [&p.meta.pmcid, &p.meta.doi] {
                if input.is_empty() {
                    continue;
                }
                match run(Conv::Input(input.clone()), &say) {
                    Ok(r) => {
                        result = Some(r);
                        break;
                    }
                    Err(e) => errors.push(e),
                }
            }
        }
        if result.is_none() && !p.has_pdf && (!p.meta.doi.is_empty() || !p.meta.arxiv.is_empty()) {
            say.say("Looking for an open-access PDF…");
            if let Ok(Some(_)) = self.find_pdf(id.clone()) {
                if let Some(pdf) = self.file_path(id.clone(), FileKind::Pdf) {
                    match run(Conv::PaperPdf(PathBuf::from(pdf), p.meta.doi.clone()), &say) {
                        Ok(r) => result = Some(r),
                        Err(e) => errors.push(e),
                    }
                }
            }
        }
        let Some((epub, text, _)) = result else {
            errors.dedup();
            return Err(inv(if errors.is_empty() { "No open full text found for this paper.".to_string() } else { errors.join("\n") }));
        };
        self.attach_file(id.clone(), FileKind::Epub, epub)?;
        self.attach_file(id, FileKind::Fulltext, text.into_bytes())?;
        Ok(())
    }

    /// Gives a paper its PDF (replacing any it had), then makes the reading view if it has none.
    /// Returns a note when the PDF went in but no reading view could be made.
    pub fn attach_pdf_file(&self, id: String, path: String, progress: Option<Arc<dyn ProgressListener>>) -> Result<Option<String>> {
        let say = Say(progress.clone());
        let data = std::fs::read(&path).map_err(|e| inv(format!("Couldn't read the file: {e}")))?;
        if !data.starts_with(b"%PDF") {
            return Err(inv("That isn't a readable PDF."));
        }
        say.say("Adding the PDF…");
        self.attach_file(id.clone(), FileKind::Pdf, data)?;
        if self.paper(id.clone()).map_or(false, |p| !p.has_epub) {
            say.say("Making the reading view…");
            if let Err(e) = self.make_reading_view(id, progress) {
                return Ok(Some(e.to_string()));
            }
        }
        Ok(None)
    }

    /// "Use this page's full text": the journal page the reader has open in the app (with their
    /// access), with the figure images it fetched, becomes the paper's reading view: added,
    /// attached, or replaced after backing up the current one (book.epub and its text) into
    /// `backups_dir`, but kept when a highlight wouldn't be found in the page's text. `dry_run`
    /// converts and checks but changes nothing. A page that isn't the full text (only the
    /// abstract, a sign-in page) is an error with the converter's message.
    pub fn use_page_full_text(
        &self,
        page: PageSubmission,
        backups_dir: String,
        dry_run: bool,
        progress: Option<Arc<dyn ProgressListener>>,
    ) -> Result<PageFullText> {
        let say = Say(progress);
        say.say("Making the reading view…");
        let s = say.clone();
        let opts = Options { log: Some(Box::new(move |m: &str| s.say(m))), offline: false, doi: None };
        let o = pagetext::use_page_full_text(self, &page, Path::new(&backups_dir), dry_run, &opts).map_err(inv)?;
        if !dry_run && matches!(o.kind, PageOutcomeKind::Attached | PageOutcomeKind::Replaced) {
            // the unpacked book of the reading view that was there (open_epub's cache)
            if let Some(id) = &o.paper_id {
                let dir = self.inner.lock().unwrap().cache.join("epub").join(id);
                let _ = std::fs::remove_dir_all(dir);
            }
        }
        Ok(o.into())
    }
}
