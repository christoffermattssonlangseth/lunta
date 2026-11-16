//! Publisher article pages → reading views: a port of `WebPage.swift` (Paper2EpubKit).
//!
//! A page the reader has open in the browser (sent by Lunta's browser extension with the HTML as
//! the server delivered it and the figure images the browser fetched with the reader's access),
//! or saved from Safari (`webarchive`). [`pick`] chooses the reader by the publishing platform:
//! ScienceDirect (`sciencedirect`); Atypon core markup (NEJM, Science, PNAS, Wiley, T&F, SAGE,
//! ACS, …: `atypon`); Springer Nature (nature.com, Springer Link, BMC: `springer`); Silverchair
//! (JAMA, Oxford Academic, ASH, RUP, …: `silverchair`); Cell Press / Lancet (`cellpress`);
//! otherwise the generic reader here, for standard scholarly markup. Pages without a substantial
//! body are refused rather than made into a poor reading view (an abstract-only page — no
//! access — with a message saying so).

use super::html;
use super::xml::{Doc, Node, NodeId};
use super::{esc, fail, images, resolver, Book, BookImage, Chapter, ConvertError, Heading, Options, PaperMeta, Result};
use std::collections::{HashMap, HashSet};

macro_rules! re {
    ($p:expr) => {{
        static R: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        #[allow(clippy::regex_creation_in_loops)] // compiled once
        let r = R.get_or_init(|| regex::Regex::new($p).unwrap());
        r
    }};
}
pub(crate) use re;

/// A figure image (or a linked page, e.g. a table's own page) the browser fetched with the
/// reader's access. `urls`: every address the page uses for it (the high-resolution link, the
/// `src` in the page, …), so the reader finds it whichever one it looks at.
#[derive(Debug, Clone)]
pub struct PageImage {
    pub urls: Vec<String>,
    pub data: Vec<u8>,
}

/// Looks posted images up by any of their addresses, then by file name (`…-gr1.jpg` and
/// `…-gr1_lrg.jpg` are the same figure) when that name is unambiguous.
pub struct PageImages {
    list: Vec<PageImage>,
    by_url: HashMap<String, usize>,
    by_stem: HashMap<String, Vec<usize>>,
    pub base: Option<url::Url>,
}

impl PageImages {
    pub fn new(list: Vec<PageImage>, base: Option<&url::Url>) -> Self {
        let mut by_url = HashMap::new();
        let mut by_stem: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, img) in list.iter().enumerate() {
            let is_page = Self::is_html(&img.data);
            for u in &img.urls {
                by_url.insert(Self::key(u, base), i);
                if is_page {
                    continue; // pages (e.g. a table's own page) only by their address
                }
                let st = Self::stem(u);
                if !st.is_empty() {
                    let v = by_stem.entry(st).or_default();
                    if !v.contains(&i) {
                        v.push(i);
                    }
                }
            }
        }
        PageImages { list, by_url, by_stem, base: base.cloned() }
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// The posted file for the first candidate address that has one (index, bytes).
    pub fn data(&self, cands: &[&str]) -> Option<(usize, &[u8])> {
        for c in cands.iter().filter(|c| !c.is_empty()) {
            if let Some(&i) = self.by_url.get(&Self::key(c, self.base.as_ref())) {
                return Some((i, &self.list[i].data));
            }
        }
        // by name: several files of one figure (thumbnail, full size): the largest
        for c in cands.iter().filter(|c| !c.is_empty()) {
            if let Some(hits) = self.by_stem.get(&Self::stem(c)) {
                if let Some(best) = max_first(hits.iter().copied(), |&i| self.list[i].data.len()) {
                    return Some((best, &self.list[best].data));
                }
            }
        }
        None
    }

    /// A page the browser sent along with the images (e.g. the page of a table that the article
    /// page only links to), by its address.
    pub fn page(&self, cands: &[&str]) -> Option<String> {
        for c in cands.iter().filter(|c| !c.is_empty()) {
            if let Some(&i) = self.by_url.get(&Self::key(c, self.base.as_ref())) {
                if Self::is_html(&self.list[i].data) {
                    return Some(String::from_utf8_lossy(&self.list[i].data).into_owned());
                }
            }
        }
        None
    }

    fn is_html(d: &[u8]) -> bool {
        let head = String::from_utf8_lossy(&d[..d.len().min(512)]).to_lowercase();
        let head = head.trim();
        head.starts_with("<!doctype html") || head.starts_with("<html") || (head.starts_with('<') && head.contains("<head"))
    }

    fn key(u: &str, base: Option<&url::Url>) -> String {
        let mut s = u.trim_matches(|c: char| c == ' ' || c == '\t').to_string();
        if s.starts_with("//") {
            s = format!("https:{s}");
        }
        let abs = match base {
            Some(b) => b.join(&s).ok(),
            None => url::Url::parse(&s).ok(),
        };
        if let Some(a) = abs {
            s = a.to_string();
        }
        if let Some(h) = s.find('#') {
            s.truncate(h);
        }
        s.replace("http://", "https://")
    }

    /// File name without extension and size suffixes, lower-cased; "" for generic names.
    fn stem(u: &str) -> String {
        let s = u.split('?').next().unwrap_or("").split('#').next().unwrap_or("");
        let decoded = super::net::percent_decode(s).unwrap_or_else(|| s.to_string());
        let mut s = decoded.rsplit('/').next().unwrap_or(&decoded).to_string();
        if let Some(dot) = s.rfind('.') {
            if s[dot..].chars().count() <= 5 {
                s.truncate(dot);
            }
        }
        s = s.to_lowercase();
        if s.starts_with("m_") && s.chars().count() > 6 {
            s = s[2..].to_string(); // Silverchair's medium-size copy ("m_gkae1105fig1")
        }
        for suffix in
            ["_lrg", "_large", "-large", "_hires", "_highres", "-highres", "_full", "_med", "_sml", "_small", "_thumb", "-thumb"]
        {
            if s.ends_with(suffix) {
                s.truncate(s.len() - suffix.len());
            }
        }
        if ["default", "image", "img", "figure", "large", "full", "original", "download", "fig", ""].contains(&s.as_str()) {
            String::new()
        } else {
            s
        }
    }
}

/// `href` resolved against `base`, and its address as written when it is already absolute (as
/// Foundation's `URL(string:relativeTo:).absoluteString` gives it; the parsed URL is normalised).
pub(crate) fn absolute(base: Option<&url::Url>, href: &str) -> Option<(url::Url, String)> {
    if let Ok(u) = url::Url::parse(href) {
        return Some((u, href.to_string()));
    }
    let u = base?.join(href).ok()?;
    let s = u.to_string();
    Some((u, s))
}

/// The first item with the largest key (Swift's `max(by:)`; Rust's `max_by_key` keeps the last).
pub(crate) fn max_first<T: Copy, K: PartialOrd>(it: impl IntoIterator<Item = T>, key: impl Fn(&T) -> K) -> Option<T> {
    let mut best: Option<(T, K)> = None;
    for x in it {
        let k = key(&x);
        match &best {
            Some((_, bk)) if !(k > *bk) => {}
            _ => best = Some((x, k)),
        }
    }
    best.map(|b| b.0)
}

// MARK: - Which reader

/// The reader for a page: by the publishing platform its host runs on; for a page without an
/// address (a saved file), by the platform's markup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageReader {
    ScienceDirect,
    Nejm,
    Atypon,
    SpringerNature,
    Silverchair,
    CellPress,
    Generic,
}

impl PageReader {
    pub fn name(self) -> &'static str {
        match self {
            PageReader::ScienceDirect => "ScienceDirect",
            PageReader::Nejm => "NEJM",
            PageReader::Atypon => "Atypon",
            PageReader::SpringerNature => "Springer Nature",
            PageReader::Silverchair => "Silverchair",
            PageReader::CellPress => "Cell Press / Lancet",
            PageReader::Generic => "generic",
        }
    }
}

/// Atypon Literatum sites (journals on their own domains).
const ATYPON_HOSTS: &[&str] = &[
    "science.org", "pnas.org", "onlinelibrary.wiley.com", "tandfonline.com", "journals.sagepub.com", "pubs.acs.org",
    "ahajournals.org", "journals.asm.org", "annualreviews.org", "liebertpub.com", "journals.physiology.org", "ascopubs.org",
    "atsjournals.org", "psychiatryonline.org", "royalsocietypublishing.org", "journals.uchicago.edu", "pubs.rsna.org",
    "ajronline.org", "healthaffairs.org", "pubsonline.informs.org", "epubs.siam.org", "arc.aiaa.org",
    "ajph.aphapublications.org", "cdnsciencepub.com", "futuremedicine.com", "neurology.org", "embopress.org",
];
/// Silverchair sites.
const SILVERCHAIR_HOSTS: &[&str] = &[
    "jamanetwork.com", "academic.oup.com", "ashpublications.org", "rupress.org", "aacrjournals.org", "diabetesjournals.org",
    "direct.mit.edu", "pubs.geoscienceworld.org", "karger.com", "portlandpress.com", "journals.biologists.com",
    "iwaponline.com", "asmedigitalcollection.asme.org", "spiedigitallibrary.org", "meridian.allenpress.com",
    "publications.aap.org", "journals.aai.org", "pubs.asahq.org",
];
const SPRINGER_HOSTS: &[&str] =
    &["nature.com", "link.springer.com", "springer.com", "biomedcentral.com", "springeropen.com", "springermedizin.de"];
const CELL_HOSTS: &[&str] = &["cell.com", "thelancet.com"];

fn on(host: &str, list: &[&str]) -> bool {
    list.iter().any(|h| host == *h || host.ends_with(&format!(".{h}")))
}

pub fn pick(url: Option<&url::Url>, html: &str) -> PageReader {
    let host = url.and_then(|u| u.host_str()).unwrap_or("").to_lowercase();
    if host.ends_with("sciencedirect.com") || (host.is_empty() && super::sciencedirect::looks_like(html)) {
        return PageReader::ScienceDirect;
    }
    if host.ends_with("nejm.org") {
        return PageReader::Nejm;
    }
    if on(&host, CELL_HOSTS) {
        return PageReader::CellPress; // (Atypon core or ScienceDirect markup: decided there)
    }
    // the platform's markup first (a journal on its own domain, a site that moved platform), then the host
    if super::atypon::looks_like(html) {
        return PageReader::Atypon;
    }
    if super::springer::looks_like(html) {
        return PageReader::SpringerNature;
    }
    if super::silverchair::looks_like(html) {
        return PageReader::Silverchair;
    }
    if on(&host, SPRINGER_HOSTS) {
        return PageReader::SpringerNature;
    }
    if on(&host, SILVERCHAIR_HOSTS) {
        return PageReader::Silverchair;
    }
    if on(&host, ATYPON_HOSTS) {
        return PageReader::Atypon;
    }
    PageReader::Generic
}

/// The book for an article page (see the module documentation).
pub fn book(html: &str, url: Option<&url::Url>, images: Vec<PageImage>, opts: &Options) -> Result<Book> {
    let bag = PageImages::new(images, url);
    let mut book = match pick(url, html) {
        PageReader::ScienceDirect => super::sciencedirect::book_with(html, None, url, Some(&bag), opts)?,
        PageReader::Nejm | PageReader::Atypon => super::atypon::book(html, url, &bag, opts)?,
        PageReader::SpringerNature => super::springer::book(html, url, &bag, opts)?,
        PageReader::Silverchair => super::silverchair::book(html, url, &bag, opts)?,
        PageReader::CellPress => super::cellpress::book(html, url, &bag, opts)?,
        PageReader::Generic => {
            let label = format!("Full text from {}", url.and_then(|u| u.host_str()).unwrap_or("the publisher"));
            generic(html, url, &bag, &label, Adapter::none(), opts)?
        }
    };
    if let Some(u) = url {
        if book.meta.url.is_empty() {
            book.meta.url = u.to_string();
        }
        if book.meta.doi.is_empty() {
            if let Some(d) = resolver::doi_in(u.as_str()) {
                book.meta.doi = d;
            }
        }
    }
    Ok(book)
}

// MARK: - MathML

const MATHML_NS: &str = "http://www.w3.org/1998/Math/MathML";

/// MathML written back out (namespace on <math>, attributes EPUB accepts).
pub(crate) fn mathml(m: Node, display: bool) -> String {
    let mut s = format!("<math xmlns=\"{MATHML_NS}\"");
    if display {
        s += " display=\"block\"";
    }
    for (k, v) in m.attrs() {
        if ["id", "display", "xmlns", "class", "style", "altimg", "alttext", "overflow"].contains(&k.as_str())
            || k.starts_with("data-")
        {
            continue;
        }
        s += &format!(" {k}=\"{}\"", esc(v));
    }
    s += ">";
    for c in m.children() {
        s += &mathml_node(c);
    }
    s + "</math>"
}

fn mathml_node(n: Node) -> String {
    if n.is_text() {
        return esc(n.text());
    }
    if n.name() == "annotation" || n.name() == "annotation-xml" {
        return String::new();
    }
    let mut s = format!("<{}", n.name());
    for (k, v) in n.attrs() {
        if k == "id" || k == "class" || k == "style" || k.starts_with("data-") {
            continue;
        }
        if k == "displaystyle" && !["mstyle", "mtable", "math"].contains(&n.name()) {
            continue;
        }
        s += &format!(" {k}=\"{}\"", esc(v));
    }
    if !n.has_children() {
        return s + "/>";
    }
    s += ">";
    for c in n.children() {
        s += &mathml_node(c);
    }
    s + &format!("</{}>", n.name())
}

// MARK: - The generic reader

/// Article pages in general, read from standard scholarly markup: `citation_*` meta tags for the
/// details; the article's main element (article, main, [role=main], .article-body, #body, …) for
/// the text: headings, paragraphs, lists, figures (img + figcaption), tables, MathML and the
/// reference list (the section headed References). Navigation, asides, ads, share bars and
/// scripts are dropped. Conservative: a page without a substantial body (under 1,000 words, or
/// no headings) is refused.
pub const MIN_WORDS: usize = 1000;

/// Elements an adapter marks with this attribute survive `clean` (e.g. a list of links that is
/// the supplementary material).
pub const KEEP_ATTR: &str = "data-lunta-keep";

/// The message for a page with only the abstract (no access to the full text).
pub const ABSTRACT_ONLY: &str =
    "This page only shows the abstract — open it with your institution's access (or sign in) so the full text shows, and try again.";

/// A platform's adjustments before the generic reading: `prepare` rewrites the parsed page in
/// place (platform figures and tables as plain `figure` / table wraps, javascript links as `#id`
/// links, furniture removed); `main` names the article's text when the platform marks it.
pub(crate) struct Adapter<'f> {
    pub prepare: Box<dyn FnMut(&mut Doc) + 'f>,
    pub main: Box<dyn Fn(Node) -> Option<NodeId> + 'f>,
}

impl Adapter<'_> {
    pub fn none() -> Adapter<'static> {
        Adapter { prepare: Box::new(|_| {}), main: Box::new(|_| None) }
    }
}

pub(crate) fn words(s: &str) -> usize {
    s.split_whitespace().count()
}

/// The text of rendered XHTML (tags left out, entities decoded).
pub(crate) fn text_of(h: &str) -> String {
    crate::highlight::dom_text(h)
}

pub(crate) fn is_heading(n: Node) -> bool {
    let b = n.name().as_bytes();
    b.len() == 2 && b[0] == b'h' && (b'1'..=b'6').contains(&b[1])
}

fn heading_level(n: Node) -> Option<i64> {
    if is_heading(n) {
        n.name()[1..].parse().ok()
    } else {
        None
    }
}

fn is_punct(c: char) -> bool {
    c.is_ascii_punctuation() || matches!(c, '–' | '—' | '…' | '“' | '”' | '‘' | '’' | '·' | '•' | '¶' | '§' | '¿' | '¡' | '«' | '»')
}

fn trimmed_lower(t: &str) -> String {
    t.to_lowercase().trim_matches(|c: char| c.is_whitespace() || is_punct(c)).to_string()
}

pub(crate) fn is_abstract(t: &str) -> bool {
    ["abstract", "summary", "structured abstract"].contains(&trimmed_lower(t).as_str())
}

pub(crate) fn is_references(t: &str) -> bool {
    let s = trimmed_lower(t);
    let s = re!(r"^[0-9.\s]+").replace(&s, "");
    [
        "references", "reference", "references and notes", "bibliography", "literature cited", "works cited",
        "cited literature", "reference list", "references cited", "notes and references",
    ]
    .contains(&s.as_ref())
}

/// "MATERIALS AND METHODS" → "Materials and Methods" (section titles set in capitals); others unchanged.
pub(crate) fn title_case(t: &str) -> String {
    let letters: String = t.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.chars().count() <= 3 || letters != letters.to_uppercase() {
        return t.to_string();
    }
    let small = ["a", "an", "and", "as", "at", "by", "for", "in", "of", "on", "or", "the", "to", "with", "vs", "via"];
    t.to_lowercase()
        .split(' ')
        .enumerate()
        .map(|(i, w)| {
            if i > 0 && small.contains(&w) {
                w.to_string()
            } else {
                let mut cs = w.chars();
                match cs.next() {
                    Some(f) => f.to_uppercase().collect::<String>() + cs.as_str(),
                    None => String::new(),
                }
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn refusal(words: usize, headings: usize, abstract_words: usize) -> ConvertError {
    // an abstract and little else: the page of a reader without access
    if abstract_words >= 40 && words - abstract_words.min(words) < 400 {
        return fail(ABSTRACT_ONLY);
    }
    fail(format!(
        "This page doesn't look like the article's full text (found {words} words of article text{}). If you have access, open the full-text view of the article and try again.",
        if headings == 0 { " and no section headings" } else { "" }
    ))
}

/// Script-borne equations as markup the parser keeps (MathML, TeX source).
fn pre_scripts(html: &str) -> String {
    let s = re!(r#"<script\b[^>]*type="math/mml"[^>]*>([\s\S]*?)</script>"#).replace_all(html, "$1");
    let s = re!(r#"<script\b[^>]*type="math/tex; *mode=display"[^>]*>([\s\S]*?)</script>"#)
        .replace_all(&s, "<code class=\"tex-display\">$1</code>");
    let s = re!(r#"<script\b[^>]*type="math/tex"[^>]*>([\s\S]*?)</script>"#).replace_all(&s, "<code class=\"tex\">$1</code>");
    s.into_owned()
}

pub(crate) fn generic(
    html: &str,
    url: Option<&url::Url>,
    images: &PageImages,
    label: &str,
    mut adapter: Adapter,
    opts: &Options,
) -> Result<Book> {
    opts.log(&format!("Reading the page ({label})"));
    let mut doc = html::parse(&pre_scripts(html));
    (adapter.prepare)(&mut doc);
    let root = doc.root();
    let mut base = url.cloned();
    if let Some(b) = root.first(|n| n.name() == "base").and_then(|b| b.attr("href")) {
        let j = match url {
            Some(u) => u.join(b).ok(),
            None => url::Url::parse(b).ok(),
        };
        if j.is_some() {
            base = j;
        }
    }
    let mut meta = metadata(root, url);
    let mut r = Renderer::new(images, base, &meta.title);
    let body_id = root.first(|n| n.name() == "body").map(|n| n.id).unwrap_or(0);
    // what links in the page point at (those ids are kept)
    for a in root.all(|n| n.name() == "a") {
        if let Some(h) = a.attr("href") {
            if let Some(i) = h.find('#') {
                r.targets.insert(h[i + 1..].to_string());
            }
        }
    }
    // the abstract anywhere on the page (to tell a page without access from a page that isn't an article)
    let page_abstract = doc
        .node(body_id)
        .all(|x| {
            (x.name_tokens().contains("abstract") || x.attr("role") == Some("doc-abstract"))
                && !["meta", "a", "span", "li", "p"].contains(&x.name())
        })
        .into_iter()
        .map(|x| words(&x.plain()))
        .max()
        .unwrap_or(0);
    clean(&mut doc, body_id);
    let body = doc.node(body_id);
    let Some(main_id) = (adapter.main)(body).or_else(|| main_element(body).map(|m| m.id)) else {
        return Err(refusal(0, 0, page_abstract));
    };
    {
        let main = doc.node(main_id);
        opts.log(&format!(
            "Main text: <{}{}{}>",
            main.name(),
            main.attr("id").map(|i| format!(" id={i}")).unwrap_or_default(),
            main.attr("class").map(|c| format!(" class={}", c.chars().take(60).collect::<String>())).unwrap_or_default()
        ));
    }

    // the abstract may sit outside the main text
    let main = doc.node(main_id);
    let main_has_abstract = main.all(is_heading).iter().any(|h| is_abstract(&h.plain()))
        || !main.all(|x| x.name_tokens().contains("abstract")).is_empty();
    let outside_abs = if main_has_abstract {
        None
    } else {
        body.first(|x| {
            x.name_tokens().contains("abstract")
                && !["meta", "a", "span", "li"].contains(&x.name())
                && !x.is_inside(main)
                && !main.is_inside(x)
                && words(&x.plain()) >= 30
        })
        .map(|x| x.id)
    };
    if let Some(abs) = outside_abs {
        r.start_chapter("Abstract", "abstract");
        // its own heading: we have one
        if let Some(h) = doc.node(abs).first(is_heading).map(|h| h.id) {
            doc.clear_children(h);
        }
        r.flow(doc.node(abs), None);
    } else if !main_has_abstract && !meta.abstract_text.is_empty() {
        r.start_chapter("Abstract", "abstract");
        r.emit(&format!("<p>{}</p>", esc(&meta.abstract_text)));
    }
    let main = doc.node(main_id);
    r.top_level = main
        .all(is_heading)
        .into_iter()
        .filter(|h| !r.is_title(*h) && !h.plain().is_empty() && !h.has_ancestor(|a| a.name() == "figure" || a.name() == "table"))
        .filter_map(heading_level)
        .min()
        .unwrap_or(2);
    r.title_ahead = main.all(is_heading).into_iter().any(|h| r.is_title(h));
    r.flow(main, None);
    let mut chapters = r.finish();

    let word_count: usize = chapters
        .iter()
        .filter(|c| !c.headings.iter().any(|h| h.id == "references"))
        .map(|c| words(&text_of(&c.html)))
        .sum();
    let heading_count = r.heading_count;
    opts.log(&format!(
        "{word_count} words, {heading_count} headings, {} figure image(s), {} missing",
        r.images.len(),
        r.missing_images
    ));
    if word_count < MIN_WORDS || heading_count == 0 {
        let abstract_words: usize =
            chapters.iter().filter(|c| is_abstract(&c.title)).map(|c| words(&text_of(&c.html))).sum();
        return Err(refusal(
            word_count,
            heading_count,
            abstract_words.max(words(&meta.abstract_text)).max(page_abstract.min(word_count)),
        ));
    }
    if meta.abstract_text.is_empty() {
        if let Some(abs) = chapters.iter().find(|c| c.headings.first().map(|h| h.id == "abstract").unwrap_or(false)) {
            let t = text_of(&abs.html).split_whitespace().collect::<Vec<_>>().join(" ");
            meta.abstract_text = re!(r"^Abstract\s*").replace(&t, "").into_owned();
        }
    }
    replace_missing(&mut chapters);
    let mut notes = Vec::new();
    if r.missing_images > 0 {
        notes.push(format!("{} figure image(s) were not sent by the browser; captions are kept.", r.missing_images));
    }
    if r.math_count > 0 {
        notes.push(format!("{} equation(s) as MathML.", r.math_count));
    }
    Ok(Book { meta, chapters, images: r.images, source_label: label.to_string(), notes })
}

pub(crate) fn replace_missing(chapters: &mut [Chapter]) {
    for c in chapters.iter_mut() {
        if c.html.contains("[[missing-image]]") {
            c.html = c.html.replace("[[missing-image]]", "<p class=\"missing\"><em>[Figure image not available]</em></p>");
        }
    }
}

// MARK: Metadata

pub(crate) struct MetaTags(HashMap<String, Vec<String>>);

impl MetaTags {
    pub fn of(doc: Node) -> Self {
        let mut tags: HashMap<String, Vec<String>> = HashMap::new();
        for m in doc.all(|n| n.name() == "meta") {
            let Some(n) = m.attr("name").or(m.attr("property")).map(|n| n.to_lowercase()) else { continue };
            let Some(c) = m.attr("content").map(|c| c.trim().to_string()).filter(|c| !c.is_empty()) else { continue };
            tags.entry(n).or_default().push(c);
        }
        MetaTags(tags)
    }
    pub fn first(&self, names: &[&str]) -> String {
        names.iter().find_map(|n| self.0.get(*n).and_then(|v| v.first().cloned())).unwrap_or_default()
    }
    pub fn all(&self, name: &str) -> Option<&Vec<String>> {
        self.0.get(name)
    }
}

pub(crate) fn metadata(doc: Node, url: Option<&url::Url>) -> PaperMeta {
    let tags = MetaTags::of(doc);
    let mut m = PaperMeta { title: tags.first(&["citation_title", "dc.title", "og:title"]), ..Default::default() };
    if m.title.is_empty() {
        m.title = doc.first(|n| n.name() == "title").map(|t| t.plain()).unwrap_or_default();
    }
    m.authors = tags
        .all("citation_author")
        .or(tags.all("dc.creator"))
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|a| {
            let parts: Vec<&str> = a.split(',').map(|p| p.trim_matches(|c: char| c == ' ' || c == '\t')).collect();
            if parts.len() == 2 && !parts[1].is_empty() {
                format!("{} {}", parts[1], parts[0])
            } else {
                a
            }
        })
        .collect();
    m.journal = tags.first(&["citation_journal_title", "prism.publicationname", "dc.source"]);
    m.date = tags
        .first(&["citation_publication_date", "citation_date", "citation_online_date", "dc.date", "prism.publicationdate"])
        .replace('/', "-");
    for d in [
        tags.first(&["citation_doi"]),
        tags.first(&["prism.doi"]),
        tags.first(&["dc.identifier"]),
        tags.first(&["bepress_citation_doi"]),
    ] {
        if m.doi.is_empty() {
            if let Some(x) = resolver::doi_in(&d) {
                m.doi = x;
            }
        }
    }
    if m.doi.is_empty() {
        if let Some(x) = url.and_then(|u| resolver::doi_in(u.as_str())) {
            m.doi = x;
        }
    }
    m.abstract_text = re!(r"<[^>]+>").replace_all(&tags.first(&["citation_abstract", "dc.description"]), "").into_owned();
    m.license = tags.first(&["dc.rights", "citation_license"]);
    m.keywords = tags
        .all("citation_keywords")
        .cloned()
        .unwrap_or_default()
        .iter()
        .flat_map(|k| k.split(';').map(|x| x.trim().to_string()).collect::<Vec<_>>())
        .filter(|k| !k.is_empty())
        .collect();
    m.url = url.map(|u| u.to_string()).unwrap_or_default();
    m
}

// MARK: Cleaning and finding the main text

const DROP_NAMES: &[&str] = &[
    "script", "style", "noscript", "template", "iframe", "nav", "aside", "footer", "form", "button", "input", "select",
    "textarea", "svg", "canvas", "video", "audio", "object", "embed", "link", "meta", "title", "head", "dialog", "label",
    "mjx-assistive-mml",
];
const JUNK_TOKENS: &[&str] = &[
    "ad", "ads", "advert", "advertisement", "adslot", "banner", "promo", "share", "sharing", "social", "toolbar", "toolbox",
    "cookie", "cookies", "consent", "related", "recommended", "recommendations", "newsletter", "sidebar", "breadcrumb",
    "breadcrumbs", "skip", "metrics", "altmetric", "signup", "login", "paywall", "modal", "popup", "popover", "tooltip",
    "hidden", "hide", "sr", "visually", "navigation", "nav", "menu", "toc", "pagination", "comments", "download",
    "downloads", "sticky", "feedback", "survey", "alert", "rightslink", "reprints", "permissions", "jumplinks", "offscreen",
    "print", "tools", "widget",
];
const PROTECTED: &[&str] = &[
    "html", "body", "main", "article", "figure", "table", "h1", "h2", "h3", "h4", "h5", "h6", "p", "math", "tr", "td", "th",
    "thead", "tbody", "caption", "figcaption",
];

fn linked_words(c: Node) -> usize {
    c.all(|x| x.name() == "a").iter().map(|a| words(&a.plain())).sum()
}

/// Removes navigation, asides, ads, share bars, hidden elements and scripts.
pub(crate) fn clean(doc: &mut Doc, id: NodeId) {
    let kids = doc.child_ids(id);
    let mut keep = Vec::new();
    let debug = std::env::var("P2E_DEBUG").is_ok();
    for k in kids {
        let n = doc.node(k);
        if drop_reason(n) {
            if debug && words(&n.plain()) >= 80 {
                eprintln!(
                    "  dropped <{} id={} class={}> {} words",
                    n.name(),
                    n.attr("id").unwrap_or(""),
                    n.attr("class").unwrap_or(""),
                    words(&n.plain())
                );
            }
            doc.remove(k);
        } else {
            keep.push(k);
        }
    }
    for k in keep {
        let n = doc.node(k);
        if !n.is_text() && !n.is_math() {
            clean(doc, k);
        }
    }
}

fn drop_reason(c: Node) -> bool {
    if c.is_text() || c.is_math() {
        return false;
    }
    if c.attr(KEEP_ATTR).is_some() {
        return false;
    }
    // a button or label around a figure (zoomable images): kept, it's only a wrapper
    if (c.name() == "button" || c.name() == "label")
        && (c.first_descendant("img").is_some() || c.first_descendant("figure").is_some() || c.first_descendant("math").is_some())
    {
        return false;
    }
    if DROP_NAMES.contains(&c.name()) {
        return true;
    }
    // lists of links only (in-page contents, "View article / Google Scholar" under a reference)
    if c.name() == "ul" || c.name() == "ol" {
        let all = words(&c.plain());
        let linked = linked_words(c);
        if all > 0 && linked * 10 >= all * 9 && (c.parent().map(|p| p.name()) != Some("li") || all < 12) {
            return true;
        }
    }
    if c.name().starts_with("mjx-") && c.name() != "mjx-container" {
        return false;
    }
    if c.attr("hidden").is_some() {
        return true;
    }
    if c.attr("aria-hidden") == Some("true")
        && c.name() != "mjx-container"
        && c.first_descendant("math").is_none()
        && c.first_descendant("img").is_none()
    {
        return true;
    }
    if let Some(st) = c.attr("style") {
        let st = st.replace(' ', "").to_lowercase();
        if st.contains("display:none") || st.contains("visibility:hidden") {
            return true;
        }
    }
    if matches!(c.attr("role"), Some("navigation") | Some("complementary") | Some("banner")) {
        return true;
    }
    if c.classes().iter().any(|x| x.starts_with("MathJax") || x.starts_with("MJX")) {
        return true;
    }
    if !PROTECTED.contains(&c.name()) && c.attr("role") != Some("main") {
        let tok = c.name_tokens();
        if JUNK_TOKENS.iter().any(|j| tok.contains(*j))
            && c.first_descendant("math").is_none()
            && !tok.contains("abstract")
            && !tok.contains("body")
            // class names are only a hint: anything holding real paragraphs or much unlinked text stays
            && c.all(|x| x.name() == "p").iter().map(|p| words(&p.plain())).sum::<usize>() < 120
        {
            let all = words(&c.plain());
            let linked = linked_words(c);
            if all < 80 || linked * 2 > all {
                return true;
            }
        }
    }
    false
}

fn is_main_candidate(n: Node) -> bool {
    if n.name() == "article" || n.name() == "main" || n.attr("role") == Some("main") || n.attr("itemprop") == Some("articleBody")
    {
        return true;
    }
    if matches!(n.attr("id"), Some("body") | Some("article-body") | Some("artText") | Some("main-content")) {
        return true;
    }
    const CLASSES: &[&str] = &[
        "article-body", "article__body", "c-article-body", "articleBody", "article-text", "fulltext-view", "hlFld-Fulltext",
        "JournalFullText", "html-body", "article-full-text", "main-content", "content-body", "article__fulltext", "fulltext",
    ];
    n.classes().iter().any(|c| CLASSES.contains(c))
}

/// The element with the article's text: of the usual containers, the one holding most of the
/// page's paragraph text (at least half of it); otherwise the smallest element holding 80 %.
pub(crate) fn main_element(body: Node) -> Option<Node> {
    let mut pw: HashMap<NodeId, usize> = HashMap::new();
    fn count(n: Node, pw: &mut HashMap<NodeId, usize>) -> usize {
        let mut w = 0;
        if n.name() == "p" {
            w = words(&n.plain());
            for c in n.elements() {
                count(c, pw);
            }
        } else {
            for c in n.elements() {
                w += count(c, pw);
            }
        }
        pw.insert(n.id, w);
        w
    }
    let total = count(body, &mut pw);
    if total == 0 {
        return None;
    }
    let get = |n: &Node| pw.get(&n.id).copied().unwrap_or(0);
    let cands: Vec<Node> = body.all(is_main_candidate).into_iter().filter(|c| get(c) * 2 >= total).collect();
    // the innermost of them that still holds nearly all of the text the best one holds
    if let Some(most) = cands.iter().map(get).max() {
        let good: Vec<Node> = cands.iter().copied().filter(|c| get(c) * 100 >= most * 85).collect();
        if let Some(best) = max_first(good, |c| c.ancestors().len()) {
            return Some(best);
        }
    }
    let mut cur = body;
    while let Some(next) = max_first(cur.elements(), |c| get(c)) {
        if get(&next) * 10 >= total * 8 {
            cur = next;
        } else {
            break;
        }
    }
    Some(cur)
}

// MARK: Rendering

pub(crate) struct Ch {
    title: String,
    html: String,
    headings: Vec<Heading>,
    refs: Option<Vec<String>>,
    pre: bool,
}

const BLOCK_NAMES: &[&str] = &[
    "address", "article", "aside", "blockquote", "details", "div", "dl", "dt", "dd", "fieldset", "figcaption", "figure",
    "footer", "form", "h1", "h2", "h3", "h4", "h5", "h6", "header", "hr", "li", "main", "nav", "ol", "p", "pre", "section",
    "table", "ul", "summary", "center", "caption", "tbody", "thead", "tfoot", "tr", "td", "th", "menu", "#document", "body",
    "html",
];

pub(crate) struct Renderer<'b> {
    pub chapters: Vec<Ch>,
    pub images: Vec<BookImage>,
    image_names: HashMap<u64, String>,
    pub missing_images: usize,
    pub math_count: usize,
    pub heading_count: usize,
    pub top_level: i64,
    pub targets: HashSet<String>,
    used_ids: HashSet<String>,
    bag: &'b PageImages,
    pub base: Option<url::Url>,
    title: String,
    block_memo: HashMap<NodeId, bool>,
    pending_marker: String,
    /// the article's title heading is still to come (masthead headings before it are skipped)
    pub title_ahead: bool,
}

fn norm(s: &str) -> String {
    s.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect()
}

fn trim(s: &str) -> &str {
    s.trim()
}

impl<'b> Renderer<'b> {
    pub fn new(bag: &'b PageImages, base: Option<url::Url>, title: &str) -> Self {
        Renderer {
            chapters: vec![Ch { title: "Summary".into(), html: String::new(), headings: vec![], refs: None, pre: true }],
            images: vec![],
            image_names: HashMap::new(),
            missing_images: 0,
            math_count: 0,
            heading_count: 0,
            top_level: 2,
            targets: HashSet::new(),
            used_ids: HashSet::new(),
            bag,
            base,
            title: title.to_string(),
            block_memo: HashMap::new(),
            pending_marker: String::new(),
            title_ahead: false,
        }
    }

    pub fn is_title(&self, h: Node) -> bool {
        if h.name() != "h1" {
            return false;
        }
        if self.title.is_empty() {
            return true;
        }
        let a = norm(&h.plain());
        let t = norm(&self.title);
        a == t || (a.chars().count() > 30 && t.starts_with(&a.chars().take(30).collect::<String>()))
    }

    pub fn is_block(&mut self, n: Node) -> bool {
        if n.is_text() || n.is_math() {
            return false;
        }
        if BLOCK_NAMES.contains(&n.name()) {
            return true;
        }
        if n.name() == "mjx-container" || n.name() == "img" || n.name() == "br" {
            return false;
        }
        if let Some(&b) = self.block_memo.get(&n.id) {
            return b;
        }
        let b = n.elements().any(|e| self.is_block(e));
        self.block_memo.insert(n.id, b);
        b
    }

    fn in_refs(&self) -> bool {
        self.chapters.last().map(|c| c.refs.is_some()).unwrap_or(false)
    }

    pub fn emit(&mut self, s: &str) {
        if s.is_empty() {
            return;
        }
        let last = self.chapters.len() - 1;
        if self.chapters[last].refs.is_some() && !s.starts_with("<h") {
            // the reference list holds list items only; anchors go into the next item
            if s.starts_with("<span id=") {
                self.pending_marker += s;
                return;
            }
            let item = if s.starts_with("<li") { s.to_string() } else { format!("<li>{s}</li>") };
            let refs = self.chapters[last].refs.as_mut().unwrap();
            if !self.pending_marker.is_empty() {
                if let Some(gt) = item.find('>') {
                    refs.push(format!("{}{}{}", &item[..=gt], self.pending_marker, &item[gt + 1..]));
                    self.pending_marker.clear();
                    return;
                }
            }
            refs.push(item);
            return;
        }
        self.chapters[last].html += s;
    }

    pub fn unique_id(&mut self, raw: Option<&str>) -> String {
        let mut id = re!(r"[^A-Za-z0-9_.-]").replace_all(raw.unwrap_or(""), "-").into_owned();
        if id.is_empty() {
            id = format!("sec-{}", self.used_ids.len() + 1);
        }
        let f = id.chars().next().unwrap();
        if !(f.is_alphabetic() || f == '_') {
            id = format!("x{id}");
        }
        let mut out = id.clone();
        let mut k = 2;
        while self.used_ids.contains(&out) {
            out = format!("{id}-{k}");
            k += 1;
        }
        self.used_ids.insert(out.clone());
        out
    }

    pub fn uid(&mut self, raw: &str) -> String {
        self.unique_id(Some(raw))
    }

    /// The id an element keeps (if links in the page point at it).
    pub fn kept_id(&mut self, n: Node) -> Option<String> {
        let id = n.attr("id")?;
        if !self.targets.contains(id) || self.used_ids.contains(id) {
            return None;
        }
        (self.uid(id) == id).then(|| id.to_string())
    }

    pub fn start_chapter(&mut self, title: &str, id: &str) {
        let refs = is_references(title);
        self.chapters.push(Ch {
            title: title.to_string(),
            html: String::new(),
            headings: vec![],
            refs: if refs { Some(vec![]) } else { None },
            pre: false,
        });
        let hid = if refs { self.uid("references") } else { self.uid(id) };
        let last = self.chapters.last_mut().unwrap();
        last.html = format!("<section><h1 id=\"{}\">{}</h1>", esc(&hid), esc(title));
        last.headings = vec![Heading { level: 1, id: hid, title: title.to_string() }];
    }

    pub fn finish(&mut self) -> Vec<Chapter> {
        let mut out = Vec::new();
        let has_abstract = self.chapters.iter().any(|c| !c.pre && is_abstract(&c.title));
        let chapters = std::mem::take(&mut self.chapters);
        for c in chapters {
            let mut html = c.html;
            if let Some(refs) = &c.refs {
                html += &format!("<ol class=\"refs\">{}</ol>", refs.concat());
            }
            if c.pre {
                // before the abstract: the masthead (authors, affiliations, notes)
                if has_abstract {
                    continue;
                }
                // text before the first heading: kept only if it reads like a summary or an
                // introduction, not bylines; "Label: value" lines (citation, copyright, funding) left out
                html = re!(r"<p>\s*<(strong|b)>[^<]{1,40}:\s*</(strong|b)>[\s\S]*?</p>").replace_all(&html, "").into_owned();
                let t = text_of(&html);
                if words(&t) < 60 || !html.contains("<p") || !html.split("</p>").any(|p| words(&text_of(p)) >= 40) {
                    continue;
                }
                let name = if has_abstract { "Introduction" } else { "Summary" };
                let id = self.uid(&name.to_lowercase());
                out.push(Chapter {
                    title: name.into(),
                    html: format!("<section><h1 id=\"{id}\">{name}</h1>{html}</section>"),
                    headings: vec![Heading { level: 1, id, title: name.into() }],
                });
                continue;
            }
            out.push(Chapter { title: c.title, html: html + "</section>", headings: c.headings });
        }
        out
    }

    // block level: a container's children, inline runs as paragraphs
    pub fn flow(&mut self, n: Node, paragraph_id: Option<String>) {
        let mut run = String::new();
        let mut pid = paragraph_id;
        let flush = |me: &mut Self, run: &mut String, pid: &mut Option<String>| {
            let t = run.trim();
            if !t.is_empty() {
                let id = pid.take().map(|p| format!(" id=\"{}\"", esc(&p))).unwrap_or_default();
                me.emit(&format!("<p{id}>{t}</p>"));
            }
            run.clear();
        };
        for c in n.children() {
            if self.is_block(c) {
                flush(self, &mut run, &mut pid);
                self.block(c);
            } else {
                run += &self.inline(c);
            }
        }
        flush(self, &mut run, &mut pid);
    }

    fn block(&mut self, n: Node) {
        if is_heading(n) {
            self.heading(n);
            return;
        }
        if self.in_refs() && self.ref_item(n) {
            return;
        }
        match n.name() {
            "p" => {
                let id = self.kept_id(n);
                self.flow(n, id)
            }
            "figure" => {
                // a table set as a figure (LaTeXML, some journals)
                if n.first_descendant("img").is_none() && n.first_descendant("table").is_some() {
                    let tables = n.all(|x| x.name() == "table" && !x.has_ancestor(|a| a.name() == "table"));
                    let s = self.table_wrap(n, &tables);
                    self.emit(&s);
                } else {
                    let s = self.figure(n);
                    self.emit(&s);
                }
            }
            "table" => {
                let s = self.table_wrap(n, &[n]);
                self.emit(&s);
            }
            "ul" | "ol" => {
                let s = self.list(n);
                self.emit(&s);
            }
            "pre" => self.emit(&format!("<pre>{}</pre>", esc(&n.all_text()))),
            "hr" => {}
            "dt" => {
                let t = self.inline(n).trim().to_string();
                if !t.is_empty() {
                    self.emit(&format!("<p><strong>{t}</strong></p>"));
                }
            }
            _ => {
                let tok = n.name_tokens();
                if (n.name() == "div" || n.name() == "section")
                    && (tok.contains("figure") || tok.contains("fig"))
                    && n.first_descendant("img").is_some()
                    && n.first_descendant("table").is_none()
                    && words(&n.plain()) < 400
                {
                    let s = self.figure(n);
                    self.emit(&s);
                    return;
                }
                if (tok.contains("table") || tok.contains("tbl"))
                    && n.first_descendant("table").is_some()
                    && !n.all(is_heading).iter().any(|h| !h.plain().is_empty())
                {
                    let tables = n.all(|x| x.name() == "table" && !x.has_ancestor(|a| a.name() == "table"));
                    if tables.len() <= 3 {
                        let s = self.table_wrap(n, &tables);
                        self.emit(&s);
                        return;
                    }
                }
                if let Some(id) = self.kept_id(n) {
                    self.emit(&format!("<span id=\"{}\"></span>", esc(&id)));
                }
                self.flow(n, None);
            }
        }
    }

    /// In the reference list: list items and leaf paragraphs are references.
    fn ref_item(&mut self, n: Node) -> bool {
        if n.name() == "ul" || n.name() == "ol" {
            for li in n.children().filter(|c| c.name() == "li") {
                let t = self.ref_inline(li).trim().to_string();
                if !t.is_empty() {
                    let id = self.kept_id(li).map(|i| format!(" id=\"{}\"", esc(&i))).unwrap_or_default();
                    self.emit(&format!("<li{id}>{t}</li>"));
                }
            }
            return true;
        }
        // one reference: a paragraph-like element without lists, short enough not to be a list of them
        let w = words(&n.plain());
        let has_list =
            n.first_descendant("ol").is_some() || n.first_descendant("ul").is_some() || n.first_descendant("li").is_some();
        let leaf = !n
            .elements()
            .any(|e| ["p", "div", "section", "table", "figure"].contains(&e.name()) && words(&e.plain()) >= 3);
        let tok = n.name_tokens();
        let ref_like = ["ref", "reference", "citation", "bibitem", "bib", "csl"].iter().any(|t| tok.contains(*t));
        if ["p", "div", "li", "span"].contains(&n.name()) && !has_list && w >= 3 && (leaf || (ref_like && w <= 150)) {
            let t = self.ref_inline(n).trim().to_string();
            if !t.is_empty() {
                let id = self.kept_id(n).map(|i| format!(" id=\"{}\"", esc(&i))).unwrap_or_default();
                self.emit(&format!("<li{id}>{t}</li>"));
            }
            return true;
        }
        false
    }

    /// A reference's text without its rows of service links ("Article · PubMed · Google Scholar").
    fn ref_inline(&mut self, n: Node) -> String {
        let mut out = String::new();
        for c in n.children() {
            if !c.is_text() && c.name() != "a" && (self.is_block(c) || c.name() == "span") {
                let all = words(&c.plain());
                let linked = linked_words(c);
                if all > 0 && all < 15 && linked * 10 >= all * 9 {
                    continue;
                }
            }
            out += &self.inline(c);
        }
        out
    }

    fn heading(&mut self, h: Node) {
        let t = h.plain().trim().to_string();
        if self.is_title(h) {
            self.title_ahead = false;
            return;
        }
        if t.is_empty() || self.title_ahead {
            return;
        }
        let level = heading_level(h).unwrap_or(2) - self.top_level;
        let section_id = h.parent().and_then(|p| {
            ((p.name() == "section" || p.name() == "div") && p.elements().next().map(|f| f.id) == Some(h.id))
                .then(|| p.attr("id"))
                .flatten()
        });
        let raw = h.attr("id").or(section_id);
        let in_refs = self.in_refs();
        if level <= 0 || (is_abstract(&t) && !in_refs) {
            self.heading_count += 1;
            let fallback = format!("sec-{}", self.chapters.len());
            self.start_chapter(&t, raw.unwrap_or(&fallback));
            if let Some(s) = section_id {
                if Some(s) != raw && self.targets.contains(s) && !self.used_ids.contains(s) {
                    self.used_ids.insert(s.to_string());
                    self.emit(&format!("<span id=\"{}\"></span>", esc(s)));
                }
            }
            return;
        }
        if in_refs {
            return;
        }
        self.heading_count += 1;
        let id = self.unique_id(raw);
        let tag = if level == 1 { "h2" } else { "h3" };
        if level == 1 {
            self.chapters.last_mut().unwrap().headings.push(Heading { level: 2, id: id.clone(), title: t.clone() });
        }
        let inner = self.inline(h).trim().to_string();
        self.emit(&format!("<{tag} id=\"{}\">{inner}</{tag}>", esc(&id)));
    }

    // MARK: inline

    pub fn inline(&mut self, n: Node) -> String {
        if n.is_text() {
            return esc(n.text());
        }
        // an element links point at (e.g. an empty <a id> before a reference): keep the anchor
        if !n.is_math() && n.name() != "img" {
            let raw = n.attr("id").or(if n.name() == "a" { n.attr("name") } else { None });
            if let Some(raw) = raw {
                if self.targets.contains(raw) && !self.used_ids.contains(raw) && self.uid(raw) == raw {
                    return format!("<span id=\"{}\"></span>", esc(raw)) + &self.inline_content(n);
                }
            }
        }
        self.inline_content(n)
    }

    fn inline_kids(&mut self, n: Node) -> String {
        let mut s = String::new();
        for c in n.children() {
            s += &self.inline(c);
        }
        s
    }

    fn inline_content(&mut self, n: Node) -> String {
        if n.is_text() {
            return esc(n.text());
        }
        if n.is_math() && n.name() == "math" {
            self.math_count += 1;
            return mathml(n, n.attr("display") == Some("block"));
        }
        match n.name() {
            "mjx-container" => {
                self.math_count += 1;
                if let Some(m) = n.first_descendant("math") {
                    return mathml(m, n.attr("display") == Some("true"));
                }
                super::sciencedirect::mjx_math(n)
            }
            "br" => "<br/>".into(),
            "img" => self.inline_image(n),
            "em" | "i" | "strong" | "b" | "sup" | "sub" | "u" | "small" | "code" | "s" => {
                let inner = self.inline_kids(n);
                if inner.is_empty() {
                    return String::new();
                }
                if n.attr("class").map(|c| c.contains("tex")).unwrap_or(false) {
                    return format!("<code>{inner}</code>");
                }
                format!("<{0}>{inner}</{0}>", n.name())
            }
            "a" => {
                let inner = self.inline_kids(n);
                let Some(href) = n.attr("href") else { return inner };
                if super::trim_ws(&inner).is_empty() {
                    return inner;
                }
                if href.starts_with('#') && href.len() > 1 {
                    return format!("<a href=\"{}\">{inner}</a>", esc(href));
                }
                if let Some((u, abs)) = absolute(self.base.as_ref(), href).filter(|(u, _)| u.scheme() == "http" || u.scheme() == "https") {
                    // a link to a place in this same page
                    if let (Some(base), Some(f)) = (&self.base, u.fragment()) {
                        if abs.split('#').next() == base.as_str().split('#').next() {
                            return format!("<a href=\"#{}\">{inner}</a>", esc(f));
                        }
                    }
                    return format!("<a href=\"{}\">{inner}</a>", esc(&abs));
                }
                inner
            }
            _ => {
                // headings and blocks inside inline content: their text
                if is_heading(n) {
                    return format!("<strong>{}</strong> ", self.inline_kids(n));
                }
                let inner = self.inline_kids(n);
                if BLOCK_NAMES.contains(&n.name()) && !inner.is_empty() {
                    inner + " "
                } else {
                    inner
                }
            }
        }
    }

    pub fn candidates(&self, img: Node) -> Vec<String> {
        let mut c: Vec<String> = Vec::new();
        for k in ["src", "data-src", "data-lazy-src", "data-original", "data-hi-res-src", "data-large-src"] {
            if let Some(v) = img.attr(k) {
                if !v.is_empty() && !v.starts_with("data:") {
                    c.push(v.to_string());
                }
            }
        }
        let srcset = |v: &str| -> Vec<String> {
            v.split(',')
                .filter_map(|p| p.trim_matches(|c: char| c == ' ' || c == '\t').split(' ').next().map(String::from))
                .filter(|s| !s.is_empty())
                .collect()
        };
        for k in ["srcset", "data-srcset"] {
            if let Some(v) = img.attr(k) {
                c.extend(srcset(v).into_iter().rev());
            }
        }
        if let Some(pic) = img.parent().filter(|p| p.name() == "picture") {
            for src in pic.children().filter(|s| s.name() == "source") {
                if let Some(v) = src.attr("srcset") {
                    c.extend(srcset(v));
                }
            }
        }
        if let Some(h) = img.ancestors().into_iter().find(|a| a.name() == "a").and_then(|a| a.attr("href")) {
            c.push(h.to_string());
        }
        c
    }

    /// The image's file in the book, from the posted images; None if the browser didn't send it.
    pub fn image_file(&mut self, cands: &[String]) -> Option<String> {
        let refs: Vec<&str> = cands.iter().map(String::as_str).collect();
        let (_, data) = self.bag.data(&refs)?;
        let key = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            data.hash(&mut h);
            h.finish()
        };
        if let Some(name) = self.image_names.get(&key) {
            return Some(name.clone());
        }
        let n = self.images.len() + 1;
        if let Some(x) = images::normalise(data, 1600) {
            let name = format!("img{n}.{}", x.ext);
            self.images.push(BookImage { file_name: name.clone(), data: x.data, media_type: x.media_type.into() });
            self.image_names.insert(key, name.clone());
            return Some(name);
        }
        if data.starts_with(b"<svg") || data.starts_with(b"<?xml") {
            let name = format!("img{n}.svg");
            self.images.push(BookImage { file_name: name.clone(), data: data.to_vec(), media_type: "image/svg+xml".into() });
            self.image_names.insert(key, name.clone());
            return Some(name);
        }
        None
    }

    fn inline_image(&mut self, img: Node) -> String {
        let c = self.candidates(img);
        let Some(f) = self.image_file(&c) else { return String::new() };
        format!("<img src=\"../images/{f}\" alt=\"{}\"/>", esc(img.attr("alt").unwrap_or("")))
    }

    // MARK: figures, tables, lists

    fn figure(&mut self, n: Node) -> String {
        let id = match self.kept_id(n) {
            Some(i) => i,
            None => {
                let fallback = format!("fig-{}", self.images.len() + self.missing_images + 1);
                self.unique_id(Some(n.attr("id").unwrap_or(&fallback)))
            }
        };
        fn is_caption(x: Node) -> bool {
            let t = x.name_tokens();
            x.name() == "figcaption"
                || x.name() == "caption"
                || ["caption", "figcaption", "legend", "captions"].iter().any(|k| t.contains(*k))
        }
        let caps: Vec<Node> = n.all(is_caption).into_iter().filter(|c| !c.has_ancestor(is_caption)).collect();
        let mut imgs: Vec<Node> = n.all(|x| x.name() == "img").into_iter().filter(|i| !i.has_ancestor(is_caption)).collect();
        // hi-res links inside the figure (e.g. "_lrg" or "download full size")
        let links: Vec<String> = n
            .all(|x| x.name() == "a")
            .iter()
            .filter_map(|a| a.attr("href"))
            .filter(|h| re!(r"(?i)(_lrg|large|full|original|hi-?res)|\.(jpe?g|png|gif|webp|tiff?)(\?|$)").is_match(h))
            .map(String::from)
            .collect();
        if imgs.len() > 1 {
            imgs.retain(|i| !i.attr("width").and_then(|w| w.parse::<i64>().ok()).map(|w| w < 40).unwrap_or(false));
        }
        let mut body = String::new();
        for (k, img) in imgs.iter().enumerate() {
            let mut c = self.candidates(*img);
            if k == 0 {
                c = links.iter().cloned().chain(c).collect();
            }
            if let Some(f) = self.image_file(&c) {
                body += &format!("<img src=\"../images/{f}\" alt=\"{}\"/>", esc(img.attr("alt").unwrap_or("Figure")));
            } else {
                self.missing_images += 1;
                body += "[[missing-image]]";
            }
        }
        let mut parts = Vec::new();
        for c in &caps {
            let has_block = c.elements().any(|e| self.is_block(e));
            let s = if has_block {
                let mut s = String::new();
                for x in c.children() {
                    if self.is_block(x) {
                        s += &format!("<span class=\"cap-p\">{}</span> ", self.inline(x).trim());
                    } else {
                        s += &self.inline(x);
                    }
                }
                s
            } else {
                self.inline(*c)
            };
            parts.push(s);
        }
        let mut caption = parts.join(" ").trim().to_string();
        if caption.is_empty() {
            // no marked caption: the figure's own text (label, title, legend paragraphs)
            let mut v = Vec::new();
            for x in n.children() {
                if x.is_text() && x.text().trim().is_empty() {
                    continue;
                }
                if x.name() == "img" || x.first_descendant("img").is_some() {
                    continue;
                }
                let t = self.inline(x).trim().to_string();
                if !t.is_empty() {
                    v.push(t);
                }
            }
            caption = v.join(" ");
        }
        if body.is_empty() && caption.is_empty() {
            return String::new();
        }
        let cap = if caption.is_empty() { String::new() } else { format!("<figcaption>{caption}</figcaption>") };
        format!("<figure id=\"{}\">{body}{cap}</figure>", esc(&id))
    }

    pub fn table_wrap(&mut self, wrap: Node, tables: &[Node]) -> String {
        let mut out = String::new();
        fn is_cap(x: Node) -> bool {
            let t = x.name_tokens();
            x.name() == "caption" || x.name() == "figcaption" || ["caption", "label", "title", "captions"].iter().any(|k| t.contains(*k))
        }
        let mut caps: Vec<Node> = tables.iter().flat_map(|t| t.children().filter(|c| c.name() == "caption")).collect();
        if wrap.name() != "table" {
            let mut w: Vec<Node> = wrap
                .all(is_cap)
                .into_iter()
                .filter(|c| !c.has_ancestor(is_cap) && !c.has_ancestor(|a| a.name() == "table"))
                .collect();
            w.extend(caps);
            caps = w;
        }
        let mut texts = Vec::new();
        for c in caps {
            let t = self.inline(c).trim().to_string();
            if !t.is_empty() {
                texts.push(t);
            }
        }
        let cap_text = texts.join(" ");
        if !cap_text.is_empty() {
            out += &format!("<p class=\"table-caption\"><strong>{cap_text}</strong></p>");
        }
        for t in tables {
            out += &self.table(*t);
        }
        if wrap.name() != "table" {
            let foot = wrap.first(|x| {
                let t = x.name_tokens();
                ["foot", "footnote", "footnotes", "fn", "notes"].iter().any(|k| t.contains(*k))
                    && !x.has_ancestor(|a| a.name() == "table")
            });
            if let Some(f) = foot {
                let t = self.inline(f).trim().to_string();
                if !t.is_empty() {
                    out += &format!("<p class=\"table-foot\"><small>{t}</small></p>");
                }
            }
        }
        let id = match self.kept_id(wrap) {
            Some(i) => Some(i),
            None => match tables.first() {
                Some(t) => self.kept_id(*t),
                None => None,
            },
        };
        format!("<div class=\"table-wrap\"{}>{out}</div>", id.map(|i| format!(" id=\"{}\"", esc(&i))).unwrap_or_default())
    }

    pub fn table(&mut self, t: Node) -> String {
        fn part(me: &mut Renderer, n: Node) -> String {
            match n.name() {
                "thead" | "tbody" | "tfoot" | "tr" => {
                    let inner: String = n.children().map(|c| part(me, c)).collect();
                    format!("<{0}>{inner}</{0}>", n.name())
                }
                "th" | "td" => {
                    let mut attrs = String::new();
                    for a in ["colspan", "rowspan"] {
                        if let Some(v) = n.attr(a) {
                            if v.parse::<i64>().is_ok() {
                                attrs += &format!(" {a}=\"{v}\"");
                            }
                        }
                    }
                    let inner = me.inline_kids(n);
                    format!("<{0}{attrs}>{1}</{0}>", n.name(), inner.trim())
                }
                "caption" | "colgroup" | "col" => String::new(),
                _ => {
                    if n.is_text() {
                        String::new()
                    } else {
                        n.children().map(|c| part(me, c)).collect()
                    }
                }
            }
        }
        let mut rows: String = t.children().map(|c| part(self, c)).collect();
        // rows directly in <table>: wrap them so the XHTML is valid
        if !rows.starts_with("<thead") && !rows.starts_with("<tbody") && !rows.starts_with("<tfoot") {
            rows = format!("<tbody>{rows}</tbody>");
        }
        format!("<table>{rows}</table>")
    }

    pub fn list(&mut self, n: Node) -> String {
        let mut items = String::new();
        for li in n.children().filter(|c| c.name() == "li") {
            let mut inner = String::new();
            let mut run = String::new();
            for c in li.children() {
                match c.name() {
                    "ul" | "ol" => {
                        inner += &std::mem::take(&mut run);
                        inner += &self.list(c);
                    }
                    "figure" => {
                        inner += &std::mem::take(&mut run);
                        inner += &self.figure(c);
                    }
                    "table" => {
                        inner += &std::mem::take(&mut run);
                        inner += &self.table_wrap(c, &[c]);
                    }
                    _ => run += &self.inline(c),
                }
            }
            inner += &run;
            let inner = trim(&inner).to_string();
            if !inner.is_empty() {
                let id = self.kept_id(li).map(|i| format!(" id=\"{}\"", esc(&i))).unwrap_or_default();
                items += &format!("<li{id}>{inner}</li>");
            }
        }
        if items.is_empty() {
            return String::new();
        }
        let tag = if n.name() == "ol" { "ol" } else { "ul" };
        format!("<{tag}>{items}</{tag}>")
    }
}

/// Helpers for the platform adapters that rebuild markup.
pub(crate) fn el(doc: &mut Doc, name: &str, attrs: &[(&str, &str)], kids: &[NodeId]) -> NodeId {
    let mut a: Vec<(String, String)> = attrs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    a.sort_by(|x, y| x.0.cmp(&y.0));
    let id = doc.new_element(name, a);
    doc.append(id, kids);
    id
}

pub(crate) fn text_node(doc: &mut Doc, t: &str) -> NodeId {
    doc.new_text(t)
}
