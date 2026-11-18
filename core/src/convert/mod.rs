//! Paper → EPUB converter: a Rust port of the Swift `Paper2EpubKit` (apple/Paper2EpubKit),
//! so non-Apple apps can use it.
//!
//! Input (DOI, PMCID, PubMed ID, arXiv id, article link, JATS XML file) is resolved into
//! candidate sources (`resolver`), the first that loads is turned into chapters (`jats` for
//! PMC / Europe PMC / eLife XML, `arxiv` for arXiv's LaTeXML HTML) and packaged as EPUB 3
//! (`epub`) with a generated cover (`cover`).
//!
//! PDFs are analysed by `pdf` (feature "pdf"; a port of `PDFAnalyzer.swift` / `PDFContent.swift`
//! on PDFium instead of PDFKit). Without that feature, where the Swift code would convert a PDF
//! this returns [`ConvertError::PdfNotSupported`].
//!
//! Everything is synchronous (ureq), like `metadata.rs`.

pub mod arxiv;
pub mod atypon;
pub mod cellpress;
pub mod cover;
pub mod epub;
pub mod html;
pub mod images;
pub mod jats;
pub mod net;
#[cfg(feature = "pdf")]
pub mod pdf;
pub mod resolver;
pub mod sciencedirect;
pub mod silverchair;
pub mod springer;
pub mod webarchive;
pub mod webpage;
pub mod xml;

use regex::Regex;
use resolver::{Candidate, Source};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
pub use webpage::PageImage;
pub use xml::esc;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PaperMeta {
    pub title: String,
    pub authors: Vec<String>,
    pub date: String,
    pub doi: String,
    pub journal: String,
    pub abstract_text: String,
    pub license: String,
    pub keywords: Vec<String>,
    pub url: String,
    /// Pre-rendered XHTML for affiliations, shown on the title page.
    pub affiliations_html: String,
}

impl PaperMeta {
    /// Fills empty fields from `other`.
    pub fn merge(&mut self, o: &PaperMeta) {
        fn f(a: &mut String, b: &str) {
            if a.is_empty() {
                *a = b.to_string();
            }
        }
        f(&mut self.title, &o.title);
        if self.authors.is_empty() {
            self.authors = o.authors.clone();
        }
        f(&mut self.date, &o.date);
        f(&mut self.doi, &o.doi);
        f(&mut self.journal, &o.journal);
        f(&mut self.abstract_text, &o.abstract_text);
        f(&mut self.license, &o.license);
        if self.keywords.is_empty() {
            self.keywords = o.keywords.clone();
        }
        f(&mut self.url, &o.url);
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Heading {
    /// 1 = chapter, 2 = subsection
    pub level: u32,
    pub id: String,
    /// plain text
    pub title: String,
}

#[derive(Debug, Clone)]
pub struct Chapter {
    pub title: String,
    /// XHTML body content
    pub html: String,
    pub headings: Vec<Heading>,
}

#[derive(Debug, Clone)]
pub struct BookImage {
    /// e.g. "fig1.jpg"
    pub file_name: String,
    pub data: Vec<u8>,
    pub media_type: String,
}

#[derive(Debug, Clone)]
pub struct Book {
    pub meta: PaperMeta,
    pub chapters: Vec<Chapter>,
    pub images: Vec<BookImage>,
    pub source_label: String,
    pub notes: Vec<String>,
}

/// What `convert` produced; mirrors the Swift `ConversionResult`.
#[derive(Debug, Clone)]
pub struct ConversionResult {
    /// The EPUB that was written.
    pub file: PathBuf,
    pub title: String,
    /// Which source was used, e.g. "PMC open-access XML".
    pub source: String,
    pub words: usize,
    pub images: usize,
    pub notes: Vec<String>,
    /// Metadata of the converted paper (for reference managers such as Lunta).
    pub meta: PaperMeta,
    /// Plain text of the whole paper, for full-text search.
    pub fulltext: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ConvertError {
    #[error("{0}")]
    Message(String),
    /// The only usable source is a PDF, and this build has no PDF analysis (feature "pdf" off).
    #[error("PDF conversion not yet supported ({0})")]
    PdfNotSupported(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, ConvertError>;

pub(crate) fn fail(m: impl Into<String>) -> ConvertError {
    ConvertError::Message(m.into())
}

/// Options for [`convert_with`] / [`convert_file_with`].
#[derive(Default)]
pub struct Options {
    /// Progress messages ("Fetching the arXiv HTML version", …).
    pub log: Option<Box<dyn Fn(&str) + Send + Sync>>,
    /// When true, converting a file never goes online (no XML lookup by a PDF's DOI, no
    /// Crossref metadata).
    pub offline: bool,
    /// The paper's DOI when it is known (a library paper's): converting its PDF looks for an XML
    /// version by this DOI, also when the PDF doesn't show it.
    pub doi: Option<String>,
}

impl Options {
    pub(crate) fn log(&self, m: &str) {
        if let Some(l) = &self.log {
            l(m);
        }
    }
}

/// Converts a link / DOI / PMCID / PubMed ID / arXiv id into `out_dir/<slug>.epub`.
pub fn convert(input: &str, out_dir: &Path) -> Result<ConversionResult> {
    convert_with(input, out_dir, &Options::default())
}

/// Converts a file (PDF or JATS XML) into `out_dir/<slug>.epub`.
pub fn convert_file(path: &Path, out_dir: &Path) -> Result<ConversionResult> {
    convert_file_with(path, out_dir, &Options::default())
}

pub fn convert_with(input: &str, out_dir: &Path, opts: &Options) -> Result<ConversionResult> {
    let cands = resolver::candidates(input)?;
    run(cands, out_dir, opts)
}

pub fn convert_file_with(path: &Path, out_dir: &Path, opts: &Options) -> Result<ConversionResult> {
    let cands = resolver::candidates_for_file(path)?;
    run(cands, out_dir, opts)
}

/// Converts a publisher's article page given as HTML — as the browser shows it to a reader with
/// access, e.g. sent by an app or browser extension — into `out_dir/<slug>.epub`. `base_url` is
/// the page's address (image links relative to it are resolved against it; may be empty).
/// Without figure images; see [`convert_page_with`].
pub fn convert_html_with(html: &str, base_url: &str, out_dir: &Path, opts: &Options) -> Result<ConversionResult> {
    let book = html_book(html, base_url, opts)?;
    write(&book, out_dir, opts)
}

/// The book for a publisher's article page given as HTML (see [`convert_html_with`]).
pub fn html_book(html: &str, base_url: &str, opts: &Options) -> Result<Book> {
    page_book(html, base_url, Vec::new(), opts)
}

/// "Use this page's full text": an article page the reader has open in the browser (the HTML as
/// the server delivered it, its address, and the figure images — and linked table pages — the
/// browser fetched with the reader's access, each with every address the page uses for it)
/// becomes `out_dir/<slug>.epub`. The reader is picked by the publishing platform (see
/// [`page_reader`]); a page without a substantial body is refused (an abstract-only page — no
/// access — with a message saying so).
pub fn convert_page_with(
    html: &str,
    url: &str,
    images: Vec<PageImage>,
    out_dir: &Path,
    opts: &Options,
) -> Result<ConversionResult> {
    let book = page_book(html, url, images, opts)?;
    write(&book, out_dir, opts)
}

/// The book for an article page (see [`convert_page_with`]).
pub fn page_book(html: &str, url: &str, images: Vec<PageImage>, opts: &Options) -> Result<Book> {
    let u = url::Url::parse(url).ok();
    webpage::book(html, u.as_ref(), images, opts)
}

/// Which reader [`convert_page_with`] uses for a page: "ScienceDirect", "NEJM", "Atypon",
/// "Springer Nature", "Silverchair", "Cell Press / Lancet" or "generic".
pub fn page_reader(url: &str, html: &str) -> &'static str {
    webpage::pick(url::Url::parse(url).ok().as_ref(), html).name()
}

/// Converts a Safari web archive of an article page (the page and the images it loaded).
pub fn convert_webarchive_with(data: &[u8], out_dir: &Path, opts: &Options) -> Result<ConversionResult> {
    let p = webarchive::read(data)?;
    opts.log(&format!(
        "Web archive: {}, {} image(s)",
        p.url.as_ref().map(|u| u.to_string()).unwrap_or_else(|| "no URL".into()),
        p.images.len()
    ));
    let book = webpage::book(&p.html, p.url.as_ref(), p.images, opts)?;
    write(&book, out_dir, opts)
}

fn run(cands: Vec<Candidate>, out_dir: &Path, opts: &Options) -> Result<ConversionResult> {
    let mut errors: Vec<String> = Vec::new();
    for c in cands {
        let src = match (c.load)(opts) {
            Ok(s) => s,
            Err(e) => {
                errors.push(format!("{}: {}", c.label, e));
                opts.log(&format!("  ✗ {}: {}", c.label, e));
                continue;
            }
        };
        let book = build(src, None, opts, &errors)?;
        return write(&book, out_dir, opts);
    }
    if errors.len() == 1 {
        let parts: Vec<&str> = errors[0].split(": ").collect();
        return Err(fail(parts[1..].join(": ")));
    }
    Err(fail(format!("No usable source found.\n{}", errors.join("\n"))))
}

/// `pdf`: the paper's PDF, when converting from it: figures an XML version lacks are cut from it.
fn build(src: Source, pdf: Option<&[u8]>, opts: &Options, earlier: &[String]) -> Result<Book> {
    match src {
        Source::Book(book) => Ok(*book),
        Source::Pdf { data, meta, label } => {
            // An open-access XML version beats any PDF analysis: check for one via the PDF's DOI.
            if !opts.offline {
                if let Some(doi) = opts.doi.clone().filter(|d| !d.trim().is_empty()).or_else(|| pdf_doi(&data)) {
                    opts.log(&format!("PDF has DOI {doi}; checking for an XML version"));
                    for c in resolver::doi_candidates(&doi) {
                        let Ok(src) = (c.load)(opts) else { continue };
                        match src {
                            Source::Jats { .. } | Source::Html { .. } | Source::Book(_) => {
                                return build(src, Some(&data), opts, earlier)
                            }
                            Source::Pdf { .. } => continue, // the arXiv PDF candidate: we already have this PDF
                        }
                    }
                    opts.log(if cfg!(feature = "pdf") {
                        "No XML version; converting the PDF itself"
                    } else {
                        "No XML version; the PDF itself would have to be converted"
                    });
                }
            }
            build_pdf(&data, &meta, &label, opts, earlier)
        }
        Source::Html { doc, base, meta, label } => {
            opts.log(&format!("Converting {label}"));
            let mut conv = arxiv::ArxivConverter::new(&doc, base);
            let urls = conv.image_urls();
            if !urls.is_empty() {
                opts.log(&format!("Downloading {} figure image(s)", urls.len()));
            }
            let urls: Vec<url::Url> = urls.into_iter().take(80).collect();
            let fetched_raw = net::par_map(&urls, 6, |u| {
                let r = net::fetch(u.as_str(), None, 30.0).ok()?;
                if !(200..300).contains(&r.status) {
                    return None;
                }
                Some(r)
            });
            let mut fetched: Vec<(url::Url, BookImage)> = Vec::new();
            for (i, (u, r)) in urls.iter().zip(fetched_raw).enumerate() {
                let Some(r) = r else { continue };
                let ext = u.path().rsplit('/').next().unwrap_or("").rsplit_once('.').map(|x| x.1.to_lowercase());
                if ext.as_deref() == Some("svg") || r.content_type.contains("svg") {
                    fetched.push((
                        u.clone(),
                        BookImage { file_name: format!("fig{}.svg", i + 1), data: r.data, media_type: "image/svg+xml".into() },
                    ));
                } else if let Some(n) = images::normalise(&r.data, 1600) {
                    fetched.push((
                        u.clone(),
                        BookImage { file_name: format!("fig{}.{}", i + 1, n.ext), data: n.data, media_type: n.media_type.into() },
                    ));
                }
            }
            conv.set_images(&fetched);
            let chapters = conv.chapters(&meta);
            if !chapters.iter().any(|c| c.title != "Abstract") {
                return Err(fail(format!("The {label} page has no full text.")));
            }
            let mut notes = Vec::new();
            if conv.missing_figures > 0 {
                notes.push(format!("{} figure image(s) could not be downloaded.", conv.missing_figures));
            }
            let all: String = chapters.iter().map(|c| c.html.as_str()).collect();
            let imgs = fetched
                .into_iter()
                .map(|x| x.1)
                .filter(|i| all.contains(&format!("../images/{}\"", i.file_name)))
                .collect();
            Ok(Book { meta, chapters, images: imgs, source_label: label, notes })
        }
        Source::Jats { data, assets, meta: extra, label } => {
            opts.log(&format!("Converting {label}"));
            let doc = xml::parse(&data)?;
            let hrefs = jats::graphic_hrefs(doc.root());
            let images = images::fetch_assets(&hrefs, &assets, opts);
            let mut conv = jats::JatsConverter::new(doc.root(), &images);
            let mut chapters = conv.chapters();
            if conv.missing_figures > 0 {
                if let Some(figs) = pdf.map(pdf_figures).filter(|f| !f.is_empty()) {
                    // figure images not available (e.g. a PMC author manuscript): cut them from the PDF
                    opts.log(&format!("{} figure image(s) missing; taking them from the PDF", conv.missing_figures));
                    conv = jats::JatsConverter::new(doc.root(), &images).with_pdf_figures(figs);
                    chapters = conv.chapters();
                }
            }
            let mut meta = conv.meta();
            meta.merge(&extra);
            let has_body = chapters.iter().any(|c| !c.headings.iter().any(|h| h.id.starts_with("abstract")));
            if !has_body {
                return Err(fail("The XML only contains the abstract, not the full text."));
            }
            let has_abstract =
                chapters.iter().any(|c| c.headings.first().map(|h| h.id.starts_with("abstract")).unwrap_or(false));
            if !has_abstract && !meta.abstract_text.is_empty() {
                chapters.insert(
                    0,
                    Chapter {
                        title: "Abstract".into(),
                        html: format!(
                            "<section><h1 id=\"abstract\">Abstract</h1><p>{}</p></section>",
                            esc(&meta.abstract_text)
                        ),
                        headings: vec![Heading { level: 1, id: "abstract".into(), title: "Abstract".into() }],
                    },
                );
            }
            let mut notes = Vec::new();
            if !conv.used_from_pdf.is_empty() {
                notes.push(format!("{} figure image(s) taken from the PDF.", conv.used_from_pdf.len()));
            }
            if conv.missing_figures > 0 {
                notes.push(format!(
                    "{} figure image(s) could not be downloaded; captions are kept.",
                    conv.missing_figures
                ));
            }
            if conv.tex_formulas > 0 {
                notes.push(format!(
                    "{} formula(s) only exist as TeX and are shown as source code.",
                    conv.tex_formulas
                ));
            }
            // keep only images actually referenced
            let all: String = chapters.iter().map(|c| c.html.as_str()).collect();
            let mut imgs: Vec<BookImage> = images
                .values()
                .chain(conv.used_from_pdf.iter())
                .filter(|i| all.contains(&format!("../images/{}\"", i.file_name)))
                .cloned()
                .collect();
            imgs.sort_by(|a, b| a.file_name.cmp(&b.file_name));
            Ok(Book { meta, chapters, images: imgs, source_label: label, notes })
        }
    }
}

/// The DOI printed in the PDF (PDFium: page text and document info), else one in its metadata.
fn pdf_doi(data: &[u8]) -> Option<String> {
    #[cfg(feature = "pdf")]
    if let Ok(Some(d)) = pdf::quick_doi(data) {
        return Some(d);
    }
    resolver::doi_in_pdf_metadata(data)
}

/// The PDF's figures by label (feature "pdf"; none without it).
fn pdf_figures(data: &[u8]) -> std::collections::HashMap<String, BookImage> {
    #[cfg(feature = "pdf")]
    return pdf::figures_by_label(data, &|_| {});
    #[cfg(not(feature = "pdf"))]
    {
        let _ = data;
        std::collections::HashMap::new()
    }
}

#[cfg(feature = "pdf")]
fn build_pdf(data: &[u8], meta: &PaperMeta, _label: &str, opts: &Options, _earlier: &[String]) -> Result<Book> {
    pdf::build(data, meta, opts)
}

#[cfg(not(feature = "pdf"))]
fn build_pdf(_data: &[u8], _meta: &PaperMeta, label: &str, _opts: &Options, earlier: &[String]) -> Result<Book> {
    let mut why = format!("the only usable source is {label}");
    if !earlier.is_empty() {
        why += &format!("; other sources failed: {}", earlier.join("; "));
    }
    Err(ConvertError::PdfNotSupported(why))
}

fn write(book: &Book, dir: &Path, opts: &Options) -> Result<ConversionResult> {
    std::fs::create_dir_all(dir)?;
    let mut name = slug(&book.meta.title, 70);
    if name.is_empty() {
        name = slug(&book.meta.doi, 60);
    }
    if name.is_empty() {
        name = "paper".into();
    }
    let path = dir.join(format!("{name}.epub"));
    opts.log("Writing EPUB");
    let cover = cover::make(&book.meta);
    epub::write(book, cover.as_deref(), &path)?;
    static TAG: OnceLock<Regex> = OnceLock::new();
    let tag = TAG.get_or_init(|| Regex::new(r"<[^>]+>").unwrap());
    static BLOCK_END: OnceLock<Regex> = OnceLock::new();
    let block_end = BLOCK_END.get_or_init(|| Regex::new(r"</(p|h[1-6]|li|figcaption|div)>").unwrap());
    static SPACES: OnceLock<Regex> = OnceLock::new();
    let spaces = SPACES.get_or_init(|| Regex::new(r"[ \t]+").unwrap());
    let words = book.chapters.iter().map(|c| tag.replace_all(&c.html, " ").split_whitespace().count()).sum();
    let text = book
        .chapters
        .iter()
        .map(|c| {
            let t = block_end.replace_all(&c.html, "\n");
            let t = tag.replace_all(&t, " ");
            let t = t.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&amp;", "&");
            spaces.replace_all(&t, " ").into_owned()
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(ConversionResult {
        file: path,
        title: book.meta.title.clone(),
        source: book.source_label.clone(),
        words,
        images: book.images.len(),
        notes: book.notes.clone(),
        meta: book.meta.clone(),
        fulltext: text,
    })
}

/// Lower-case ASCII slug: runs of anything else become one dash.
pub fn slug(s: &str, max: usize) -> String {
    let lowered = s.to_lowercase();
    let mut out = String::new();
    let mut last_dash = false;
    for ch in lowered.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    let cut: String = out.chars().take(max).collect();
    cut.trim_matches('-').to_string()
}

/// Swift's `CharacterSet.newlines`.
fn is_newline(c: char) -> bool {
    matches!(c, '\u{a}'..='\u{d}' | '\u{85}' | '\u{2028}' | '\u{2029}')
}

/// Trims Swift's `CharacterSet.whitespaces` (spaces and tabs, not newlines).
pub(crate) fn trim_ws(s: &str) -> &str {
    s.trim_matches(|c: char| c.is_whitespace() && !is_newline(c))
}

/// Swift's `String.capitalized`: first letter of each word upper-case, the rest lower-case.
pub(crate) fn capitalized(s: &str) -> String {
    let mut out = String::new();
    let mut start = true;
    for ch in s.chars() {
        if ch.is_alphanumeric() {
            if start {
                out.extend(ch.to_uppercase());
            } else {
                out.extend(ch.to_lowercase());
            }
            start = false;
        } else {
            out.push(ch);
            start = ch.is_whitespace();
        }
    }
    out
}

#[cfg(test)]
mod pages_tests;
#[cfg(test)]
mod tests;
