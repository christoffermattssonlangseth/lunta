//! Adding papers and making reading views, with the core's metadata lookup and the Rust
//! converter (core/src/convert). Mirrors LibraryModel.add / makeReadingView / attachPDF in
//! apple/Shared/LibraryModel.swift and AttachPDF.swift. Everything here blocks (network), so
//! call it from a blocking task.

use lunta_core::convert::{self, ConversionResult, Options};
use lunta_core::model::{FileKind, Paper, PaperMeta};
use lunta_core::Library;
use std::path::{Path, PathBuf};

pub struct Added {
    pub paper: Paper,
    /// Set when the paper was added but no reading view could be made (the converter's reason).
    pub note: Option<String>,
}

fn tmp_dir(what: &str) -> PathBuf {
    std::env::temp_dir().join(format!("lunta-{what}-{}", uuid::Uuid::new_v4()))
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

/// Runs the converter and returns (epub bytes, full text, result) or the converter's error.
fn run(input: Conv, log: &Logger) -> Result<(Vec<u8>, String, ConversionResult), String> {
    let tmp = tmp_dir("conv");
    let l = log.clone();
    let opts = Options { log: Some(Box::new(move |m: &str| l.say(m.trim()))), offline: false };
    let r = match &input {
        Conv::Input(s) => convert::convert_with(s, &tmp, &opts),
        Conv::File(p) => convert::convert_file_with(p, &tmp, &opts),
    };
    let out = match r {
        Ok(r) => match std::fs::read(&r.file) {
            Ok(data) => {
                let text = r.fulltext.clone();
                Ok((data, text, r))
            }
            Err(e) => Err(e.to_string()),
        },
        Err(e) => Err(e.to_string()),
    };
    let _ = std::fs::remove_dir_all(&tmp);
    out
}

enum Conv {
    Input(String),
    File(PathBuf),
}

/// Progress messages ("Fetching the arXiv HTML version", …) for the status bar.
#[derive(Clone)]
pub struct Logger(pub std::sync::Arc<dyn Fn(&str) + Send + Sync>);

impl Logger {
    pub fn say(&self, m: &str) {
        if !m.is_empty() {
            (self.0)(m)
        }
    }
}

/// Adds a paper by DOI, PMCID, PubMed id, arXiv id or link: metadata from the core's lookup,
/// reading view from the converter when an open full text exists.
pub fn add(lib: &Library, input: &str, log: &Logger) -> Result<Added, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("Type a DOI, PMCID, arXiv id or link first.".into());
    }
    log.say(&format!("Looking up {input}…"));
    let meta = lunta_core::resolve_metadata(input.to_string()).ok();
    log.say("Making the reading view…");
    let conv = run(Conv::Input(input.to_string()), log);
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
        return Err(format!("Couldn't find “{input}”. Try a DOI or a link to the article.{}", if why.is_empty() { String::new() } else { format!("\n{why}") }));
    }
    let (epub, text, note) = match conv {
        Ok((e, t, _)) => (Some(e), Some(t), None),
        Err(e) => (None, None, Some(e)),
    };
    let paper = lib.add_paper(m, None, epub, text).map_err(|e| e.to_string())?;
    Ok(Added { paper, note })
}

/// Makes the reading view for a paper that has none: arXiv's HTML for arXiv papers, else the
/// converter on the PDF (which looks for an open XML version via the PDF's DOI; PDF analysis
/// itself is not in the Rust converter yet), else the PMCID / DOI, else an open-access PDF.
pub fn make_reading_view(lib: &Library, id: &str, log: &Logger) -> Result<(), String> {
    let p = lib.paper(id.to_string()).ok_or("No such paper")?;
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
        match run(Conv::Input(format!("arxiv:{arxiv}")), log) {
            Ok(r) => result = Some(r),
            Err(e) => errors.push(e),
        }
    }
    if result.is_none() {
        if let Some(pdf) = lib.file_path(id.to_string(), FileKind::Pdf) {
            match run(Conv::File(PathBuf::from(pdf)), log) {
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
            match run(Conv::Input(input.clone()), log) {
                Ok(r) => {
                    result = Some(r);
                    break;
                }
                Err(e) => errors.push(e),
            }
        }
    }
    if result.is_none() && !p.has_pdf && (!p.meta.doi.is_empty() || !p.meta.arxiv.is_empty()) {
        log.say("Looking for an open-access PDF…");
        if let Ok(Some(_)) = lib.find_pdf(id.to_string()) {
            if let Some(pdf) = lib.file_path(id.to_string(), FileKind::Pdf) {
                match run(Conv::File(PathBuf::from(pdf)), log) {
                    Ok(r) => result = Some(r),
                    Err(e) => errors.push(e),
                }
            }
        }
    }
    let Some((epub, text, _)) = result else {
        errors.dedup();
        return Err(if errors.is_empty() { "No open full text found for this paper.".into() } else { errors.join("\n") });
    };
    lib.attach_file(id.to_string(), FileKind::Epub, epub).map_err(|e| e.to_string())?;
    lib.attach_file(id.to_string(), FileKind::Fulltext, text.into_bytes()).map_err(|e| e.to_string())?;
    Ok(())
}

/// Gives a paper its PDF (replacing any it had), then makes the reading view if it has none.
/// Returns a note when the PDF went in but no reading view could be made.
pub fn attach_pdf(lib: &Library, id: &str, path: &Path, log: &Logger) -> Result<Option<String>, String> {
    let data = std::fs::read(path).map_err(|e| format!("Couldn't read {}: {e}", path.display()))?;
    if !data.starts_with(b"%PDF") {
        return Err(format!("“{}” isn't a readable PDF.", path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()));
    }
    log.say("Adding the PDF…");
    lib.attach_file(id.to_string(), FileKind::Pdf, data).map_err(|e| e.to_string())?;
    if lib.paper(id.to_string()).map_or(false, |p| !p.has_epub) {
        log.say("Making the reading view…");
        if let Err(e) = make_reading_view(lib, id, log) {
            return Ok(Some(e));
        }
    }
    Ok(None)
}
