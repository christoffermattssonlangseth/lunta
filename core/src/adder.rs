//! Adding papers and making reading views with the Rust converter (feature "convert"), exported
//! to UniFFI for the Android app. Mirrors LibraryModel.add / makeReadingView / attachPDF in
//! apple/Shared and desktop/src-tauri/src/add.rs. Everything here blocks (network), so call it
//! off the main thread.

use crate::convert::{self, ConversionResult, Options};
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
}

type Converted = (Vec<u8>, String, ConversionResult);

fn run(input: Conv, say: &Say) -> std::result::Result<Converted, String> {
    let tmp = std::env::temp_dir().join(format!("lunta-conv-{}", uuid::Uuid::new_v4()));
    let s = say.clone();
    let opts = Options { log: Some(Box::new(move |m: &str| s.say(m))), offline: false };
    let r = match &input {
        Conv::Input(s) => convert::convert_with(s, &tmp, &opts),
        Conv::File(p) => convert::convert_file_with(p, &tmp, &opts),
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
            let why = conv.err().unwrap_or_default();
            return Err(inv(format!("Couldn't find “{input}”. Try a DOI or a link to the article.{}", if why.is_empty() { String::new() } else { format!("\n{why}") })));
        }
        let (epub, text, note) = match conv {
            Ok((e, t, _)) => (Some(e), Some(t), None),
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
        if result.is_none() {
            if let Some(pdf) = self.file_path(id.clone(), FileKind::Pdf) {
                match run(Conv::File(PathBuf::from(pdf)), &say) {
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
                    match run(Conv::File(PathBuf::from(pdf)), &say) {
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
}
