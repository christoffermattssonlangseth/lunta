//! Heuristic PDF → book layout analysis (no LLM): a port of `PDFAnalyzer.swift`.
//!
//! Everything here works on plain data — text lines with fonts, sizes and positions, and the
//! rectangles of images and vector drawings per page — so it does not depend on PDFium and can
//! be tested with synthetic pages. `extract.rs` fills [`Page`]s from a PDF; rendering crops of
//! figures, tables and equations goes through the [`Renderer`] trait.

use super::super::BookImage;
use regex::Regex;
use std::collections::{HashMap, HashSet};

/// A regex compiled once.
macro_rules! re {
    ($p:expr) => {{
        static R: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        #[allow(clippy::regex_creation_in_loops)] // compiled once
        let r = R.get_or_init(|| regex::Regex::new($p).unwrap());
        r
    }};
}
pub(crate) use re;

/// Rectangle in page coordinates with the origin at the top-left of the media box, in points.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct R {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl R {
    pub fn new(x0: f64, y0: f64, x1: f64, y1: f64) -> R {
        R { x0, y0, x1, y1 }
    }
    pub fn w(&self) -> f64 {
        self.x1 - self.x0
    }
    pub fn h(&self) -> f64 {
        self.y1 - self.y0
    }
    pub fn area(&self) -> f64 {
        self.w().max(0.0) * self.h().max(0.0)
    }
    pub fn cx(&self) -> f64 {
        (self.x0 + self.x1) / 2.0
    }
    pub fn union(&self, o: &R) -> R {
        R::new(self.x0.min(o.x0), self.y0.min(o.y0), self.x1.max(o.x1), self.y1.max(o.y1))
    }
    pub fn intersects(&self, o: &R, pad: f64) -> bool {
        !(self.x1 + pad < o.x0 || o.x1 + pad < self.x0 || self.y1 + pad < o.y0 || o.y1 + pad < self.y0)
    }
    /// `i` lies inside, give or take `tol` (Swift's default is 2).
    pub fn contains(&self, i: &R, tol: f64) -> bool {
        i.x0 >= self.x0 - tol && i.y0 >= self.y0 - tol && i.x1 <= self.x1 + tol && i.y1 <= self.y1 + tol
    }
    pub fn intersection(&self, o: &R) -> R {
        R::new(self.x0.max(o.x0), self.y0.max(o.y0), self.x1.min(o.x1), self.y1.min(o.y1))
    }
    /// Union of a non-empty list.
    pub fn union_all<'a>(rs: impl IntoIterator<Item = &'a R>) -> R {
        let mut it = rs.into_iter();
        let first = *it.next().expect("union of no rectangles");
        it.fold(first, |a, b| a.union(b))
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Span {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub sup: bool,
    pub sub: bool,
}

impl Span {
    fn plain(t: &str) -> Span {
        Span { text: t.into(), ..Default::default() }
    }
}

/// One line of text as the PDF lays it out.
#[derive(Debug, Clone)]
pub struct PLine {
    pub text: String,
    pub bbox: R,
    pub size: f64,
    /// fraction of characters in a bold font
    pub bold: f64,
    /// fraction in a math font
    pub math: f64,
    pub spans: Vec<Span>,
    pub page: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Text,
    Heading,
    Caption,
    Front,
    Title,
    Abstract,
    Equation,
    Figure,
    Table,
}

#[derive(Debug, Clone)]
pub struct Para {
    pub lines: Vec<PLine>,
    pub page: usize,
    pub kind: Kind,
    pub level: usize,
    pub asset: Option<BookImage>,
    pub region: Option<R>,
    /// The framed or shaded box (sidebar) the paragraph sits in, numbered per document.
    pub boxed: Option<usize>,
}

impl Para {
    pub fn new(lines: Vec<PLine>, page: usize, kind: Kind) -> Para {
        Para { lines, page, kind, level: 0, asset: None, region: None, boxed: None }
    }
    pub fn text(&self) -> String {
        join_lines(self.lines.iter().map(|l| l.text.as_str()))
    }
    pub fn bbox(&self) -> R {
        self.region.unwrap_or_else(|| R::union_all(self.lines.iter().map(|l| &l.bbox)))
    }
    /// The font size most of the text is set in.
    pub fn size(&self) -> f64 {
        let mut c: Vec<(f64, usize)> = Vec::new();
        for l in &self.lines {
            let n = count(&l.text);
            match c.iter_mut().find(|x| x.0 == l.size) {
                Some(x) => x.1 += n,
                None => c.push((l.size, n)),
            }
        }
        first_max(&c, |x| (x.1, x.0)).map(|x| x.0).unwrap_or(0.0)
    }
    fn char_count(&self) -> usize {
        self.lines.iter().map(|l| count(&l.text)).sum()
    }
}

/// Number of characters (Swift's `String.count`, near enough).
pub(crate) fn count(s: &str) -> usize {
    s.chars().count()
}

/// The first element with the largest key (Swift's `max(by:)` keeps the first of equals).
pub(crate) fn first_max<T, K: PartialOrd>(v: &[T], key: impl Fn(&T) -> K) -> Option<&T> {
    let mut best: Option<(&T, K)> = None;
    for x in v {
        let k = key(x);
        if best.as_ref().map(|(_, bk)| k > *bk).unwrap_or(true) {
            best = Some((x, k));
        }
    }
    best.map(|b| b.0)
}

fn trim(s: &str) -> &str {
    super::super::trim_ws(s)
}

fn first_is_lower(s: &str) -> bool {
    s.chars().next().map(|c| c.is_lowercase()).unwrap_or(false)
}

/// Line-end markers in [`PLine::text`] (and the last span): `GLUE` = the word continues on the
/// next line (a hyphenation hyphen was taken out); `HARD_HYPHEN` = a real hyphen at the end of a
/// line ("sample-|level"), kept when the lines are joined.
pub const GLUE: char = '\u{AD}';
pub const HARD_HYPHEN: char = '\u{2011}';

/// A web address broken across lines ("https://doi.org/10." + "1038/nature21350") goes on
/// without a space.
pub fn url_continues(out: &str, next: &str) -> bool {
    let Some(last_word) = out.split(' ').next_back() else { return false };
    if !re!(r"^\(?(https?://|www\.|doi\.org/)\S*$").is_match(last_word) {
        return false;
    }
    let Some(n) = next.chars().next() else { return false };
    if n == '/' || n == '.' {
        return true;
    }
    last_word.chars().last().is_some_and(|c| ".-_/=?&#".contains(c)) && (n.is_lowercase() || n.is_numeric())
}

/// Turns the line-end markers back into plain text.
pub fn line_ends_cleaned(s: &str) -> String {
    if !s.contains(GLUE) && !s.contains(HARD_HYPHEN) {
        return s.to_string();
    }
    s.replace(GLUE, "").replace(HARD_HYPHEN, "-")
}

/// Joins wrapped lines, undoing end-of-line hyphenation.
pub fn join_lines<'a>(parts: impl IntoIterator<Item = &'a str>) -> String {
    let mut out = String::new();
    for raw in parts {
        let p = trim(raw);
        if p.is_empty() {
            continue;
        }
        if out.is_empty() {
            out = p.to_string();
            continue;
        }
        if out.ends_with(GLUE) {
            out.pop();
            out += p;
        } else if out.ends_with(HARD_HYPHEN) {
            out.pop();
            out.push('-');
            out += p;
        } else if url_continues(&out, p) {
            out += p;
        } else if re!(r"[A-Za-z]-$").is_match(&out) && first_is_lower(p) {
            out.pop();
            out += p;
        } else if (out.ends_with('-') || out.ends_with('/') || out.ends_with('–')) && !out.ends_with(" -") {
            out += p;
        } else {
            out.push(' ');
            out += p;
        }
    }
    line_ends_cleaned(&out)
}

pub fn known_sections() -> &'static Regex {
    re!(concat!(
        r"(?i)^(?:\d+(?:\.\d+)*\.?\s+|[IVX]+\.\s+)?(abstract|summary|introduction|background|results?|discussion|",
        r"results and discussion|methods?|materials and methods|methods and materials|star\s*methods|star\+methods|online methods|",
        r"conclusions?|references|bibliography|acknowledge?ments?|funding|author contributions?|",
        r"competing interests?|declaration of interests?|conflicts? of interest|data availability|code availability|",
        r"data and code availability|supplementary (?:information|material|figures?|tables?)|supporting information|",
        r"appendix|keywords|significance|highlights|limitations|related work|experiments?|",
        r"figure legends|figures|tables|abbreviations|ethics statement|resource availability)\s*[:.]?$"
    ))
}

/// A "math" font that sets the running text (STIX-Regular in some Wiley and Springer journals) is the
/// body font: when a good share of the body-size lines are prose in it, no font in the document marks
/// a formula (and no paragraph is cropped as one).
pub fn clear_math_if_body_font(pages: &mut [Page], body_size: f64) {
    let (mut body, mut in_math) = (0usize, 0usize);
    for l in pages.iter().flat_map(|p| &p.lines).filter(|l| (l.size - body_size).abs() <= 0.5) {
        body += count(&l.text);
        if l.math >= 0.5 && is_prose(&l.text) {
            in_math += count(&l.text);
        }
    }
    if body > 0 && in_math as f64 > 0.3 * body as f64 {
        pages.iter_mut().flat_map(|p| p.lines.iter_mut()).for_each(|l| l.math = 0.0);
    }
}

/// Running text rather than a formula: five or more words of three or more lower-case letters that
/// make up most of it. Such a paragraph is kept as text (never cropped as an image), whatever its
/// fonts say.
pub fn is_prose(t: &str) -> bool {
    let words: Vec<_> = re!(r"\b[a-z]{3,}\b").find_iter(t).collect();
    let word_chars: usize = words.iter().map(|m| m.as_str().len()).sum();
    let chars = t.chars().filter(|c| !c.is_whitespace()).count();
    words.len() >= 5 && word_chars as f64 >= 0.5 * chars.max(1) as f64
}

/// A DOI on its own: "https://doi.org/10.1038/…", "doi: 10.…".
pub fn doi_line(s: &str) -> bool {
    re!(r"(?i)^(https?://(dx\.)?doi\.org/|doi:?\s*)10\.\d{4,9}/\S+$").is_match(trim(s))
}

pub fn caption_re() -> &'static Regex {
    // the label must be followed by punctuation, a capitalised word or nothing: running text that
    // wraps onto a line starting "Figure S2G). Top genes…" or "Fig. 1a and …" is not a caption
    re!(concat!(
        r"(?i)^(?:Supplementary\s+|Extended\s+Data\s+|Supporting\s+)?(Figure|Fig\.?|Scheme|Table|Algorithm|Box)\s*S?\d+[A-Za-z]?",
        // (a dash straight into a letter or digit is a range: "Fig. 4a–c)")
        r"(?:$|[.:|—]|[\-–](?:$|(?-i:[^a-z0-9]))|\s+(?:[.:|\-–—]|(?-i:[A-Z(\[])|$))"
    ))
}

/// Image placements and vector drawings on a page (top-left coordinates).
#[derive(Debug, Clone, Default)]
pub struct PageGraphics {
    pub images: Vec<R>,
    pub drawings: Vec<R>,
}

/// One page as extracted from the PDF.
#[derive(Debug, Clone, Default)]
pub struct Page {
    /// media box size
    pub width: f64,
    pub height: f64,
    pub lines: Vec<PLine>,
    pub graphics: PageGraphics,
}

/// Rasterises part of a page; `rect` is in top-left page points, `scale` pixels per point.
pub trait Renderer {
    fn render(&mut self, page: usize, rect: R, scale: f64) -> Option<image::RgbImage>;
}

/// Document information dictionary entries.
#[derive(Debug, Clone, Default)]
pub struct DocInfo {
    pub title: String,
    pub author: String,
    pub subject: String,
    /// year of /CreationDate
    pub year: Option<i32>,
}

#[derive(Debug, Clone)]
pub struct Region {
    pub rect: R,
    pub kind: Kind,
    pub image: Option<BookImage>,
}

/// A block for the reading order: one line or region, or a box of them (`items`: their indices).
#[derive(Debug, Clone)]
struct Node {
    r: R,
    n: usize,
    text: bool,
    items: Vec<usize>,
}

#[derive(Debug, Clone)]
struct Cluster {
    rect: R,
    has_image: bool,
    n: usize,
}

pub struct Analyzer<'a> {
    pub pages: Vec<Page>,
    pub body_size: f64,
    pub running: HashSet<String>,
    renderer: &'a mut dyn Renderer,
    log: &'a dyn Fn(&str),
}

impl<'a> Analyzer<'a> {
    pub fn new(pages: Vec<Page>, renderer: &'a mut dyn Renderer, log: &'a dyn Fn(&str)) -> Self {
        let mut sizes: Vec<(f64, usize)> = Vec::new();
        for l in pages.iter().flat_map(|p| &p.lines) {
            let n = count(&l.text);
            match sizes.iter_mut().find(|x| x.0 == l.size) {
                Some(x) => x.1 += n,
                None => sizes.push((l.size, n)),
            }
        }
        let body_size = first_max(&sizes, |x| (x.1, x.0)).map(|x| x.0).unwrap_or(10.0);
        let mut pages = pages;
        mark_hard_hyphens(&mut pages);
        clear_math_if_body_font(&mut pages, body_size);
        let mut a = Analyzer { pages, body_size, running: HashSet::new(), renderer, log };
        a.find_running();
        a
    }

    /// A margin line with its numbers left out ("2 of 11", "3of11", "e70999") and, if it has words,
    /// the punctuation at its ends, for finding running heads and feet.
    fn norm(s: &str) -> String {
        let s = s.to_lowercase();
        let s = re!(r"\d+\s*of\s*\d+").replace_all(&s, "");
        let s = re!(r"\b[es]?\d+\b").replace_all(&s, "");
        let s = re!(r"\s+").replace_all(&s, " ");
        let words = s.trim_matches(|c: char| " ,.;:|–-".contains(c));
        if words.chars().any(char::is_alphabetic) { words.to_string() } else { trim(&s).to_string() }
    }

    /// Running heads and feet: lines in the top/bottom margin repeated on many pages.
    fn find_running(&mut self) {
        let n = self.pages.len();
        if n < 2 {
            return;
        }
        let mut c: HashMap<String, usize> = HashMap::new();
        for p in &self.pages {
            let h = p.height;
            let mut seen = HashSet::new();
            for l in &p.lines {
                if l.bbox.y0 < 0.09 * h || l.bbox.y1 > 0.91 * h {
                    seen.insert(Self::norm(&l.text));
                }
            }
            for s in seen {
                *c.entry(s).or_insert(0) += 1;
            }
        }
        let min = 2f64.max(0.3 * n as f64);
        self.running = c.into_iter().filter(|(_, v)| *v as f64 >= min).map(|(k, _)| k).collect();
    }

    pub fn is_noise(&self, l: &PLine) -> bool {
        let t = trim(&l.text);
        let h = self.pages[l.page].height;
        let in_margin = l.bbox.y0 < 0.09 * h || l.bbox.y1 > 0.91 * h;
        if in_margin && (self.running.contains(&Self::norm(t)) || re!(r"^[\divxlcIVXLC\s\-–/of|]+$").is_match(t)) {
            return true;
        }
        // the first page's fuller version of a running foot: "Chemistry – A European Journal, 2026;
        // 32:e70999 https://doi.org/…"
        if in_margin && l.page == 0 {
            let nt = Self::norm(t);
            if self.running.iter().any(|k| count(k) >= 15 && nt.starts_with(k.as_str())) {
                return true;
            }
        }
        if re!(r"^\d{1,4}$").is_match(t) && l.bbox.w() < 25.0 {
            return true;
        }
        if re!(r"(?i)^(bioRxiv|medRxiv) preprint doi").is_match(t) || t.contains("copyright holder for this preprint") {
            return true;
        }
        if in_margin && re!(r"(?i)^(downloaded from|arxiv:\d{4}\.\d+)").is_match(t) {
            return true;
        }
        if t.chars().all(|c| c == '\u{FFFC}' || c.is_whitespace()) {
            return true; // logo placeholder
        }
        if re!(r"(?i)^check for updates$").is_match(t) {
            return true; // Crossmark badge
        }
        if re!(r"(?i)^\((figure )?legend (continued )?on (the )?(next|previous|following) page\)$").is_match(t) {
            return true;
        }
        // publisher footer: "Cell 190, 1–19 … © 2026 Elsevier Inc." / "All rights are reserved, …"
        if l.bbox.y0 > 0.86 * h
            && l.size <= self.body_size + 0.5
            && count(t) < 200
            && re!(r"(?i)©|\bAll rights (are )?reserved\b|^for text and data mining\b").is_match(t)
        {
            return true;
        }
        // first-page citation footer: "Jiang et al., 2027, Cell 190, 1–19"
        if l.page <= 1 && l.bbox.y0 > 0.86 * h && re!(r"^[A-Z][\w'’-]+ et al\., \d{4}, .+\d$").is_match(t) {
            return true;
        }
        false
    }

    // MARK: - Figure / table regions

    pub fn find_regions(&self, pno: usize, lines: &[PLine]) -> Vec<Region> {
        let page = &self.pages[pno];
        let parea = page.width * page.height;
        let g = &page.graphics;
        let mut rects: Vec<(bool, R)> = Vec::new();
        for r in &g.images {
            if (r.w() < 1.0 && r.h() < 1.0) || r.area() > 0.95 * parea {
                continue;
            }
            // a band behind a line of text ("Published online: …" on a shaded strip) is not a picture
            if r.h() < 30.0 && text_cover(r, lines) > 0.5 * r.area() {
                continue;
            }
            rects.push((true, *r));
        }
        let mut thin: Vec<R> = Vec::new();
        for r in &g.drawings {
            if r.w() < 2.5 || r.h() < 2.5 {
                thin.push(*r);
            } else if r.area() < 0.9 * parea {
                rects.push((false, *r));
            }
        }
        rects.sort_by(|a, b| (a.1.y0, a.1.x0).partial_cmp(&(b.1.y0, b.1.x0)).unwrap_or(std::cmp::Ordering::Equal));
        let mut clusters: Vec<Cluster> = Vec::new();
        for (img, r) in rects {
            let target = match clusters.last() {
                Some(c) if c.rect.intersects(&r, 6.0) => Some(clusters.len() - 1),
                _ => clusters.iter().position(|c| c.rect.intersects(&r, 6.0)),
            };
            match target {
                Some(ci) => {
                    let c = &mut clusters[ci];
                    c.rect = c.rect.union(&r);
                    c.has_image |= img;
                    c.n += 1;
                }
                None => clusters.push(Cluster { rect: r, has_image: img, n: 1 }),
            }
        }
        merge_clusters(&mut clusters, |a, b| a.intersects(b, 6.0));
        // join nearby pieces of one figure unless body text sits between them
        let prose: Vec<R> = lines
            .iter()
            .filter(|l| count(&l.text) > 50 && l.size >= self.body_size - 0.5)
            .map(|l| l.bbox)
            .collect();
        merge_clusters(&mut clusters, |a, b| {
            if !a.intersects(b, 22.0) {
                return false;
            }
            let u = a.union(b);
            !prose.iter().any(|p| u.contains(p, 0.0) && !p.intersects(a, 0.0) && !p.intersects(b, 0.0))
        });
        // (with the rules that touch them, so that a table's or a frame's piece stands for all of it)
        for c in clusters.iter_mut() {
            for r in &thin {
                if c.rect.intersects(r, 2.0) {
                    c.rect = c.rect.union(r);
                }
            }
        }
        // the rows and columns of one figure's panels sit further apart, their letters and axis
        // labels in between: join pieces while the joined box holds no running text, caption or
        // heading (only short labels)
        let barrier: Vec<R> = lines
            .iter()
            .filter(|l| {
                let t = trim(&l.text);
                let n = count(t);
                n > 50 || (l.size >= self.body_size + 1.0 && re!(r"\p{L}{4}").is_match(t)) || caption_re().is_match(t)
            })
            .map(|l| l.bbox)
            .collect();
        // (a line of them in the box, if only by a few points)
        let hits = |u: &R, p: &R| {
            let o = u.intersection(p);
            o.w() > 1.0 && o.h() > 0.3 * p.h()
        };
        let clean = |u: &R| !barrier.iter().any(|p| hits(u, p));
        // (and two figures side by side, each over its own caption, stay two)
        let cap_boxes: Vec<R> = lines.iter().filter(|l| caption_re().is_match(trim(&l.text))).map(|l| l.bbox).collect();
        let caption_below = |r: &R| {
            cap_boxes
                .iter()
                .filter(|c| c.y0 >= r.y1 - 2.0 && c.x1 > r.x0 && c.x0 < r.x1)
                .min_by(|a, b| a.y0.partial_cmp(&b.y0).unwrap_or(std::cmp::Ordering::Equal))
                .copied()
        };
        let one_figure = |a: &R, b: &R| match (caption_below(a), caption_below(b)) {
            (Some(x), Some(y)) => x == y,
            _ => true,
        };
        // (not the masthead's or the foot's logos and lines, nor a lone shape such as a frame's
        // corner: only pictures and pieces of several shapes)
        let (m_top, m_bot) = (0.07 * page.height, 0.93 * page.height);
        let lone: Vec<R> = clusters.iter().filter(|c| !c.has_image && c.n < 2).map(|c| c.rect).collect();
        let joins = |r: &R| r.y1 >= m_top && r.y0 <= m_bot && r.area() >= 100.0 && !lone.contains(r);
        merge_clusters(&mut clusters, |a, b| {
            a.intersects(b, 60.0) && joins(a) && joins(b) && clean(&a.union(b)) && one_figure(a, b)
        });
        let mut regions: Vec<Region> = Vec::new();
        for c in &clusters {
            let r = c.rect;
            if r.area() < 2500.0 || r.y1 < m_top || r.y0 > m_bot {
                continue;
            }
            let inside: usize = lines.iter().filter(|l| r.contains(&l.bbox, 2.0)).map(|l| count(&l.text)).sum();
            if !c.has_image && c.n <= 3 && inside > 250 {
                continue; // shaded text box
            }
            if !c.has_image && c.n <= 2 && r.h() < 30.0 {
                continue;
            }
            // the journal's logo in the first page's masthead
            if pno == 0 && r.y1 < 0.12 * page.height && r.h() < 0.08 * page.height {
                continue;
            }
            regions.push(Region { rect: r, kind: Kind::Figure, image: None });
        }

        // tables: caption + horizontal rules, or a borderless grid of short cells
        let caps: Vec<usize> = (0..lines.len())
            .filter(|&i| re!(r"(?i)^(Supplementary\s+)?Table\s*S?\d+").is_match(trim(&lines[i].text)))
            .collect();
        let mut segs: Vec<R> = thin.iter().filter(|r| r.w() > 2.5 && r.h() < 2.5).copied().collect();
        segs.sort_by(|a, b| (a.y0, a.x0).partial_cmp(&(b.y0, b.x0)).unwrap_or(std::cmp::Ordering::Equal));
        // a rule drawn in pieces (one per column of the table) is one rule
        let mut hrules: Vec<R> = Vec::new();
        for r in segs {
            match hrules.last_mut() {
                Some(m) if (m.y0 - r.y0).abs() < 1.0 && r.x0 <= m.x1 + 2.0 => *m = m.union(&r),
                _ => hrules.push(r),
            }
        }
        hrules.retain(|r| r.w() > 80.0);
        hrules.sort_by(|a, b| a.y0.partial_cmp(&b.y0).unwrap_or(std::cmp::Ordering::Equal));
        for ci in caps {
            let cap = &lines[ci];
            if regions.iter().any(|g| cap.bbox.intersects(&g.rect, 30.0)) {
                continue;
            }
            let below: Vec<R> = hrules.iter().filter(|r| r.y0 > cap.bbox.y0 - 2.0).copied().collect();
            let above: Vec<R> = hrules.iter().filter(|r| r.y1 < cap.bbox.y0 + 2.0).copied().collect();
            // the table's rules are about as wide as the one next to its caption: a rule much wider
            // (the running head's rule over both columns of the page) or narrower (a table in the
            // other column) is not one of them, unless that leaves the table a single rule
            let same_table = |a: &R, r: &R| {
                r.w() <= 1.5 * a.w() && r.w() >= 0.6 * a.w() && r.x1.min(a.x1) - r.x0.max(a.x0) > 0.5 * r.w()
            };
            // (and a table ends before another table's or figure's caption)
            let other_caption = |y0: f64, y1: f64| {
                lines.iter().enumerate().any(|(j, l)| j != ci && l.bbox.y0 > y0 && l.bbox.y1 < y1 && caption_re().is_match(trim(&l.text)))
            };
            let group = |only_same: bool| -> Option<Vec<R>> {
                if let Some(f) = below.first().filter(|f| f.y0 - cap.bbox.y1 < 60.0) {
                    let mut gg = vec![*f];
                    for r in below[1..].iter().filter(|r| !only_same || same_table(f, r)) {
                        if r.y0 - gg.last().unwrap().y1 < 250.0 && !other_caption(gg.last().unwrap().y1, r.y0) {
                            gg.push(*r);
                        } else {
                            break;
                        }
                    }
                    Some(gg)
                } else if let Some(l) = above.last().filter(|l| cap.bbox.y0 - l.y1 < 60.0) {
                    let mut gg = vec![*l];
                    for r in above[..above.len() - 1].iter().rev().filter(|r| !only_same || same_table(l, r)) {
                        if gg.last().unwrap().y0 - r.y1 < 250.0 && !other_caption(r.y1, gg.last().unwrap().y0) {
                            gg.push(*r);
                        } else {
                            break;
                        }
                    }
                    Some(gg)
                } else {
                    None
                }
            };
            let grp = group(true).filter(|g| g.len() >= 2).or_else(|| group(false));
            if let Some(grp) = grp.filter(|g| g.len() >= 2) {
                regions.push(Region { rect: R::union_all(&grp), kind: Kind::Table, image: None });
            } else if let Some(tr) = self.text_table_region(ci, lines, page) {
                regions.push(Region { rect: tr, kind: Kind::Table, image: None });
            }
        }
        // expand regions with nearby short labels (axis labels, panel letters); not a caption, nor the
        // line beside one of this region's (its continuation in the other column)
        for g in regions.iter_mut() {
            let mut r = g.rect;
            for _ in 0..2 {
                for l in lines {
                    let t = trim(&l.text);
                    if cap_boxes.iter().any(|c| {
                        c.y1.min(l.bbox.y1) - c.y0.max(l.bbox.y0) > 0.5 * l.bbox.h() && c.x1 > r.x0 && c.x0 < r.x1
                    }) {
                        continue;
                    }
                    let vo = r.y1.min(l.bbox.y1) - r.y0.max(l.bbox.y0) > 0.6 * l.bbox.h();
                    let n = count(t);
                    if r.intersects(&l.bbox, 8.0)
                        && n < 70
                        // (a line of running text beside it is no label)
                        && !(n > 50 && l.size >= self.body_size - 0.5 && is_prose(t))
                        && !r.contains(&l.bbox, 2.0)
                        && (l.size < self.body_size - 0.4 || vo || n <= 12)
                    {
                        r = r.union(&l.bbox);
                    }
                }
            }
            g.rect = r;
        }
        // pieces of a figure that have grown into each other by their labels are one figure
        let mut merged = true;
        while merged {
            merged = false;
            'outer: for i in 0..regions.len() {
                for j in (i + 1)..regions.len() {
                    let (a, b) = (regions[i].rect, regions[j].rect);
                    if regions[i].kind == Kind::Figure
                        && regions[j].kind == Kind::Figure
                        && a.intersects(&b, 0.0)
                        && clean(&a.union(&b))
                        && one_figure(&a, &b)
                    {
                        regions[i].rect = a.union(&b);
                        regions.remove(j);
                        merged = true;
                        break 'outer;
                    }
                }
            }
        }
        // a piece that has grown inside another (by its labels) is part of it
        let rects: Vec<(R, Kind)> = regions.iter().map(|g| (g.rect, g.kind)).collect();
        let mut k = 0;
        regions.retain(|g| {
            let i = k;
            k += 1;
            !rects.iter().enumerate().any(|(j, (o, kind))| {
                j != i && *kind == g.kind && o.contains(&g.rect, 2.0) && (!g.rect.contains(o, 2.0) || j < i)
            })
        });
        regions
    }

    fn text_table_region(&self, cap_i: usize, lines: &[PLine], page: &Page) -> Option<R> {
        let cap = &lines[cap_i];
        let (x0, x1) =
            if cap.bbox.w() < 0.6 * page.width { (cap.bbox.x0 - 5.0, cap.bbox.x1 + 5.0) } else { (0.0, page.width) };
        let mut rows: HashMap<i64, Vec<&PLine>> = HashMap::new();
        for (i, l) in lines.iter().enumerate() {
            if i != cap_i && l.bbox.x0 >= x0 - 40.0 && l.bbox.x1 <= x1 + 40.0 {
                rows.entry((l.bbox.y0 / 3.0).round() as i64).or_default().push(l);
            }
        }
        let mut ys: Vec<i64> = rows.keys().copied().collect();
        ys.sort();
        let run_from = |start_y: f64, down: bool| -> Option<R> {
            let seq: Vec<i64> = if down {
                ys.iter().copied().filter(|&y| y as f64 * 3.0 >= start_y).collect()
            } else {
                ys.iter().copied().filter(|&y| y as f64 * 3.0 <= start_y).rev().collect()
            };
            let mut got: Vec<i64> = Vec::new();
            let mut last: Option<f64> = None;
            let mut n_multi = 0;
            for y in seq {
                let row = &rows[&y];
                let ry = if down {
                    row.iter().map(|l| l.bbox.y0).fold(f64::INFINITY, f64::min)
                } else {
                    row.iter().map(|l| l.bbox.y1).fold(f64::NEG_INFINITY, f64::max)
                };
                if let Some(last) = last {
                    if (ry - last).abs() > 40.0 {
                        break;
                    }
                }
                let multi = row.len() >= 2;
                if !multi && row.iter().map(|l| count(&l.text)).sum::<usize>() > 90 {
                    break;
                }
                got.push(y);
                if multi {
                    n_multi += 1;
                }
                last = Some(if down {
                    row.iter().map(|l| l.bbox.y1).fold(f64::NEG_INFINITY, f64::max)
                } else {
                    row.iter().map(|l| l.bbox.y0).fold(f64::INFINITY, f64::min)
                });
            }
            if got.len() >= 3 && n_multi as f64 >= 0.5 * got.len() as f64 {
                return Some(R::union_all(got.iter().flat_map(|y| rows[y].iter().map(|l| &l.bbox))));
            }
            None
        };
        let up = if cap.bbox.y0 > page.height * 0.25 { run_from(cap.bbox.y0 - 1.0, false) } else { None };
        let down = run_from(cap.bbox.y1 + 1.0, true);
        let both: Vec<R> = [up, down].into_iter().flatten().collect();
        first_max(&both, |r| r.area()).copied()
    }

    // MARK: - Rendering crops

    pub fn render(&mut self, pno: usize, rect: R, photo: bool, name: &str) -> Option<BookImage> {
        let page = &self.pages[pno];
        let r = R::new(rect.x0 - 4.0, rect.y0 - 4.0, rect.x1 + 4.0, rect.y1 + 4.0)
            .intersection(&R::new(0.0, 0.0, page.width, page.height));
        if !(r.w() > 2.0 && r.h() > 2.0) {
            return None;
        }
        let scale = (200.0 / 72.0f64).min(1600.0 / r.w().max(r.h()));
        let img = self.renderer.render(pno, r, scale)?;
        encode(&img, photo, name)
    }

    // MARK: - Assembly

    pub fn analyze(&mut self) -> Vec<Para> {
        let page_lines: Vec<Vec<PLine>> =
            self.pages.iter().map(|p| p.lines.iter().filter(|l| !self.is_noise(l)).cloned().collect()).collect();
        let mut page_regions: Vec<Vec<Region>> =
            page_lines.iter().enumerate().map(|(i, ls)| self.find_regions(i, ls)).collect();
        // logos and banners that sit in the same place on many pages are not figures
        let key = |r: &R| {
            format!(
                "{},{},{},{}",
                (r.x0 / 6.0) as i64,
                (r.y0 / 6.0) as i64,
                (r.x1 / 6.0) as i64,
                (r.y1 / 6.0) as i64
            )
        };
        let mut counts: HashMap<String, usize> = HashMap::new();
        for rs in &page_regions {
            let keys: HashSet<String> = rs.iter().map(|g| key(&g.rect)).collect();
            for k in keys {
                *counts.entry(k).or_insert(0) += 1;
            }
        }
        let limit = 3usize.max((0.3 * page_regions.len() as f64) as usize);
        for rs in page_regions.iter_mut() {
            rs.retain(|g| counts.get(&key(&g.rect)).copied().unwrap_or(0) < limit);
        }
        let mut all: Vec<Para> = Vec::new();
        let n = self.pages.len();
        for pno in 0..n {
            let lines = &page_lines[pno];
            let mut regions = std::mem::take(&mut page_regions[pno]);
            for (i, g) in regions.iter_mut().enumerate() {
                let rr = g.rect;
                let photo =
                    self.pages[pno].graphics.images.iter().any(|g| rr.contains(g, 5.0) && g.area() > 0.3 * rr.area());
                let name =
                    format!("p{}-{}{}", pno + 1, if g.kind == Kind::Table { "table" } else { "figure" }, i + 1);
                g.image = self.render(pno, rr, photo, &name);
            }
            let free: Vec<PLine> = lines
                .iter()
                .filter(|l| {
                    !regions.iter().any(|g| {
                        g.rect.contains(&l.bbox, 3.0)
                            || (l.bbox.area() > 0.0
                                && g.rect.intersects(&l.bbox, 0.0)
                                && g.rect.intersection(&l.bbox).area() > 0.6 * l.bbox.area())
                    })
                })
                .cloned()
                .collect();
            all.extend(self.paragraphs(pno, free, regions));
            if pno % 5 == 4 {
                (self.log)(&format!("Laying out page {} of {}", pno + 1, n));
            }
        }
        self.classify(all)
    }

    pub fn paragraphs(&self, pno: usize, input: Vec<PLine>, regions: Vec<Region>) -> Vec<Para> {
        // merge fragments on the same baseline (the text layer sometimes splits a line)
        let mut sorted = input;
        sorted.sort_by(|a, b| {
            (a.bbox.y0.round(), a.bbox.x0).partial_cmp(&(b.bbox.y0.round(), b.bbox.x0)).unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut merged: Vec<PLine> = Vec::new();
        for l in sorted {
            if let Some(m) = merged.last_mut() {
                if (m.bbox.y0 - l.bbox.y0).abs() < 2.0
                    && l.bbox.x0 >= m.bbox.x1 - 1.0
                    // (PDFium already joins a line's pieces up to 2 em apart; a wider gap here
                    // is usually a narrow column gutter)
                    && l.bbox.x0 - m.bbox.x1 < 1.0 * l.size
                {
                    if !m.text.ends_with(' ') {
                        m.text.push(' ');
                    }
                    m.text += &l.text;
                    m.spans.push(Span::plain(" "));
                    m.spans.extend(l.spans);
                    m.bbox = m.bbox.union(&l.bbox);
                    continue;
                }
            }
            merged.push(l);
        }
        let mut units: Vec<Para> = merged.into_iter().map(|l| Para::new(vec![l], pno, Kind::Text)).collect();
        for g in regions {
            let mut p = Para::new(vec![], pno, g.kind);
            p.region = Some(g.rect);
            p.asset = g.image;
            units.push(p);
        }
        let (units, cols) = self.order_with_columns(pno, units);
        let txt: Vec<(R, (f64, f64))> =
            units.iter().zip(&cols).filter(|(u, _)| u.kind == Kind::Text).map(|(u, k)| (u.lines[0].bbox, *k)).collect();
        let right_edge = |l: &PLine, col: (f64, f64)| -> f64 {
            // the column's right edge: lines of other columns (full-width legends, summary) do not count
            txt.iter()
                .filter(|(_, k)| *k == col)
                .map(|(b, _)| b)
                .filter(|b| b.x1.min(l.bbox.x1) - b.x0.max(l.bbox.x0) > 0.3 * l.bbox.w())
                .map(|b| b.x1)
                .fold(None, |m: Option<f64>, x| Some(m.map_or(x, |m| m.max(x))))
                .unwrap_or(l.bbox.x1)
        };
        let abbrev = re!(r"(\b(e\.g|i\.e|et al|cf|Fig|Figs|Eq|Eqs|vs|approx|ca|resp|Ref|Refs|No|Dr|Prof|al)\.|\b[A-Z]\.)$");
        let w = self.pages[pno].width;
        let mut paras: Vec<Para> = Vec::new();
        let mut cur: Vec<PLine> = Vec::new();
        let mut cur_col = (0.0, 0.0);
        let mut cur_box: Option<usize> = None;
        // where the paragraph's lines in the current column start (after a column continuation)
        let mut seg = 0;
        let flush = |cur: &mut Vec<PLine>, paras: &mut Vec<Para>, boxed: Option<usize>| {
            if !cur.is_empty() {
                let mut p = Para::new(std::mem::take(cur), pno, Kind::Text);
                p.boxed = boxed;
                paras.push(p);
            }
        };
        for (u, col) in units.into_iter().zip(cols) {
            if u.kind != Kind::Text {
                flush(&mut cur, &mut paras, cur_box);
                paras.push(u);
                continue;
            }
            if u.boxed != cur_box {
                // a box's text never runs on into the page's or the other way round
                flush(&mut cur, &mut paras, cur_box);
                cur_box = u.boxed;
            }
            let l = u.lines.into_iter().next().unwrap();
            let a_col = std::mem::replace(&mut cur_col, col);
            let Some(a) = cur.last() else {
                seg = 0;
                cur.push(l);
                continue;
            };
            let left = cur[seg..].iter().map(|c| c.bbox.x0).fold(f64::INFINITY, f64::min);
            let gap = l.bbox.y0 - a.bbox.y1;
            let jumped = l.bbox.y0 < a.bbox.y0 - 2.0 || (l.bbox.x0 - left).abs() > 0.3 * w;
            let re_a = right_edge(a, a_col);
            let width = (re_a - left).max(1.0);
            let at = trim(&a.text);
            let ends_sentence = at.ends_with('.') || at.ends_with(':') || at.ends_with('?') || at.ends_with('!');
            let short_a = (re_a - a.bbox.x1) > 0.18 * width && ends_sentence && !abbrev.is_match(at);
            // (not the hanging indent of a numbered reference: "4. Satija, R., … (2015)." / "Spatial …")
            let numbered_item = re!(r"^(\[\d+\]|\d+\.)\s").is_match(trim(&cur[0].text));
            let indent = l.bbox.x0 - left > 8.0
                && cur[seg.max(1)..].iter().all(|c| (c.bbox.x0 - left).abs() < 3.0)
                && ends_sentence
                && !abbrev.is_match(at)
                && !numbered_item;
            // (a drop cap is not a hanging first line)
            let drop_cap = cur[0].bbox.h() > 2.5 * cur[0].size;
            let hanging =
                cur.len() >= 2 && l.bbox.x0 < a.bbox.x0 - 6.0 && cur[0].bbox.x0 < a.bbox.x0 - 6.0 && !drop_cap;
            let size_change = (l.size - a.size).abs() >= 1.0;
            let style_break = (a.bold > 0.9) != (l.bold > 0.9) && (count(at) < 120 || count(&l.text) < 120);
            let cap = caption_re().is_match(trim(&l.text));
            if jumped
                || gap > 0.75 * l.size.max(a.size)
                || short_a
                || indent
                || hanging
                || size_change
                || style_break
                || cap
                || (cur.len() == 1 && l.bbox.x0 > a.bbox.x0 + 8.0 && gap > 0.3 * l.size && !numbered_item)
            {
                if jumped
                    && gap <= 0.75 * l.size
                    && !short_a
                    && !size_change
                    && !cap
                    && first_is_lower(trim(&l.text))
                    && !doi_line(&l.text)
                {
                    seg = cur.len();
                    cur.push(l); // column continuation
                    continue;
                }
                flush(&mut cur, &mut paras, cur_box);
                seg = 0;
            }
            cur.push(l);
        }
        flush(&mut cur, &mut paras, cur_box);
        paras
    }

    /// Reading order: one column top to bottom, or columns between full-width blocks.
    pub fn order(&self, pno: usize, paras: Vec<Para>) -> Vec<Para> {
        self.order_with_columns(pno, paras).0
    }

    /// Reading order of a page's lines and blocks, with the column each was read in: the
    /// x-interval between the gutters on either side of it (lines of one column share it).
    ///
    /// Recursive: a gutter splits the page into columns, and blocks across it (titles, full-width
    /// figures and legends) cut it into bands that are ordered the same way. So three columns, a
    /// two-column legend under a full-width figure, or columns of different widths above and
    /// below a figure each come out column by column. A framed or shaded box is read as one block.
    pub fn order_with_columns(&self, pno: usize, paras: Vec<Para>) -> (Vec<Para>, Vec<(f64, f64)>) {
        let singles: Vec<Node> = paras
            .iter()
            .enumerate()
            .map(|(i, p)| Node { r: p.bbox(), n: count(&p.text()), text: p.kind == Kind::Text, items: vec![i] })
            .collect();
        let mut nodes: Vec<Node> = Vec::new();
        let panels = self.panels(pno, &singles);
        let in_panel: HashSet<usize> = panels.iter().flatten().copied().collect();
        nodes.extend(singles.iter().filter(|s| !in_panel.contains(&s.items[0])).cloned());
        let mut paras = paras;
        for (k, m) in panels.into_iter().enumerate() {
            for &i in &m {
                paras[i].boxed = Some(pno * 64 + k);
            }
            let r = R::union_all(m.iter().map(|&i| &singles[i].r));
            nodes.push(Node { r, n: m.iter().map(|&i| singles[i].n).sum(), text: true, items: m });
        }
        nodes.sort_by_key(|n| n.items[0]);
        let mut out: Vec<(usize, (f64, f64))> = Vec::new();
        self.order_rec(pno, &singles, nodes, 0, None, (f64::NEG_INFINITY, f64::INFINITY), &mut out);
        if std::env::var("P2E_DEBUG_ORDER").ok().and_then(|v| v.parse::<usize>().ok()) == Some(pno + 1) {
            for &(i, k) in &out {
                let t: String = paras[i].text().chars().take(50).collect();
                eprintln!("  {:?} col=({:.0},{:.0}) {t}", singles[i].r, k.0, k.1);
            }
        }
        let mut slots: Vec<Option<Para>> = paras.into_iter().map(Some).collect();
        let keys = out.iter().map(|o| o.1).collect();
        (out.into_iter().filter_map(|(i, _)| slots[i].take()).collect(), keys)
    }

    /// Framed or shaded boxes (a drawn rectangle around several lines of text that no line
    /// crosses, holding part of the page's text): the members of each, outermost boxes only.
    fn panels(&self, pno: usize, nodes: &[Node]) -> Vec<Vec<usize>> {
        let page = &self.pages[pno];
        let (w, h) = (page.width, page.height);
        let all_n: usize = nodes.iter().filter(|n| n.text).map(|n| n.n).sum();
        let mut rects: Vec<R> = page
            .graphics
            .drawings
            .iter()
            .filter(|r| r.w() >= 0.2 * w && r.h() >= 3.0 * self.body_size && r.area() < 0.9 * w * h)
            .copied()
            .collect();
        rects.sort_by(|a, b| b.area().partial_cmp(&a.area()).unwrap_or(std::cmp::Ordering::Equal));
        let mut out: Vec<Vec<usize>> = Vec::new();
        for r in rects {
            let mut members = Vec::new();
            let mut ok = true;
            for (i, n) in nodes.iter().enumerate() {
                if r.contains(&n.r, 2.0) {
                    if !n.text {
                        ok = false; // a figure inside: left to the figure detection
                        break;
                    }
                    members.push(i);
                } else if n.r.area() > 0.0 && r.intersects(&n.r, 0.0) && r.intersection(&n.r).area() > 0.3 * n.r.area() {
                    ok = false; // text across the frame: not a box
                    break;
                }
            }
            let n: usize = members.iter().map(|&i| nodes[i].n).sum();
            if !ok || members.len() < 3 || n < 100 || n as f64 > 0.6 * all_n as f64 {
                continue;
            }
            if out.iter().any(|m| members.iter().any(|i| m.contains(i))) {
                continue; // inside (or across) a box already found
            }
            out.push(members);
        }
        out
    }

    /// The best column gutter among `nodes`: the x that most often separates two lines side by
    /// side on the same row, decided row by row so that full-width blocks do not hide the columns.
    /// Of several gutters (three columns) the one fewest blocks cross. Also returns how many rows
    /// it separates.
    fn find_gutter(&self, pno: usize, nodes: &[Node], depth: usize) -> (f64, usize) {
        let w = self.pages[pno].width;
        let textual: Vec<&Node> = nodes.iter().filter(|n| n.text && n.r.w() >= 0.15 * w).collect();
        let mut gaps: Vec<(f64, f64)> = Vec::new();
        for (ai, a) in textual.iter().enumerate() {
            let ra = a.r;
            let right_of = textual
                .iter()
                .enumerate()
                .filter(|(bi, b)| {
                    let rb = b.r;
                    *bi != ai
                        && rb.x0 >= ra.x1 + 3.0
                        && rb.x0 - ra.x1 < 0.4 * w
                        && ra.y1.min(rb.y1) - ra.y0.max(rb.y0) > 0.5 * ra.h().min(rb.h())
                })
                .map(|(_, b)| b.r.x0)
                .fold(None, |m: Option<f64>, x| Some(m.map_or(x, |m| m.min(x))));
            if let Some(bx) = right_of {
                gaps.push((ra.x1, bx));
            }
        }
        let (lo, hi) = if depth == 0 {
            (0.25 * w, 0.75 * w)
        } else {
            let x0 = nodes.iter().map(|n| n.r.x0).fold(f64::INFINITY, f64::min);
            let x1 = nodes.iter().map(|n| n.r.x1).fold(f64::NEG_INFINITY, f64::max);
            (x0 + 0.25 * (x1 - x0), x1 - 0.25 * (x1 - x0))
        };
        let mut cover: Vec<(f64, usize)> = Vec::new();
        let mut x = lo;
        while x <= hi {
            cover.push((x, gaps.iter().filter(|g| g.0 < x && x < g.1).count()));
            x += 1.0;
        }
        let max_n = cover.iter().map(|c| c.1).max().unwrap_or(0);
        if cover.is_empty() {
            return (0.5 * w, 0);
        }
        // stretches covered nearly as often as the best one; in each, the middle of its longest
        // most-covered run
        let thr = if max_n == 0 { 0 } else { ((0.7 * max_n as f64).ceil() as usize).max(1) };
        let mut best: Option<(usize, usize, usize, f64)> = None; // (crossing, peak, run, x)
        let mut i = 0;
        while i < cover.len() {
            if cover[i].1 < thr {
                i += 1;
                continue;
            }
            let mut j = i;
            while j + 1 < cover.len() && cover[j + 1].1 >= thr {
                j += 1;
            }
            let peak = cover[i..=j].iter().map(|c| c.1).max().unwrap_or(0);
            let mut run: Option<(usize, usize)> = None;
            let mut k = i;
            while k <= j {
                if cover[k].1 != peak {
                    k += 1;
                    continue;
                }
                let mut m = k;
                while m < j && cover[m + 1].1 == peak {
                    m += 1;
                }
                if run.map(|b| m - k > b.1 - b.0).unwrap_or(true) {
                    run = Some((k, m));
                }
                k = m + 1;
            }
            let (a, b) = run.unwrap_or((i, j));
            let g = (cover[a].0 + cover[b].0) / 2.0;
            let crossing = nodes.iter().filter(|n| self.crosses(pno, n, g)).count();
            let better = match best {
                None => true,
                Some((c, p, l, _)) => (crossing, std::cmp::Reverse(peak), std::cmp::Reverse(b - a)) < (c, std::cmp::Reverse(p), std::cmp::Reverse(l)),
            };
            if better {
                best = Some((crossing, peak, b - a, g));
            }
            i = j + 1;
        }
        best.map(|b| (b.3, b.1)).unwrap_or((0.5 * w, 0))
    }

    /// A block across the gutter at `g` (text never crosses it unless it spans the columns; a
    /// figure's box, grown by its labels, may stick out of its column a little).
    fn crosses(&self, pno: usize, n: &Node, g: f64) -> bool {
        let w = self.pages[pno].width;
        let m = if n.text { 3.0 } else { 0.05 * w };
        (n.r.x0 < g - m && n.r.x1 > g + m) || (!n.text && n.r.w() >= 0.55 * w)
    }

    #[allow(clippy::too_many_arguments)]
    fn order_rec(
        &self,
        pno: usize,
        singles: &[Node],
        nodes: Vec<Node>,
        depth: usize,
        inherited: Option<f64>,
        key: (f64, f64),
        out: &mut Vec<(usize, (f64, f64))>,
    ) {
        let cmp = |a: f64, b: f64| a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal);
        let leaf = |mut nodes: Vec<Node>, out: &mut Vec<(usize, (f64, f64))>| {
            if depth == 0 {
                nodes.sort_by(|a, b| cmp((a.r.y0 / 4.0).round(), (b.r.y0 / 4.0).round()).then(cmp(a.r.x0, b.r.x0)));
            } else {
                nodes.sort_by(|a, b| cmp(a.r.y0, b.r.y0));
            }
            for n in nodes {
                if n.items.len() == 1 {
                    out.push((n.items[0], key));
                } else {
                    // a box: its own lines in their own order
                    let inner: Vec<Node> = n.items.iter().map(|&i| singles[i].clone()).collect();
                    self.order_rec(pno, singles, inner, depth + 1, None, (n.r.x0, n.r.x1), out);
                }
            }
        };
        if nodes.len() <= 1 || depth > 12 {
            return leaf(nodes, out);
        }
        let w = self.pages[pno].width;
        let (mut gutter, max_n) = self.find_gutter(pno, &nodes, depth);
        let columns = if depth == 0 {
            // pages with little text (end matter): fall back on "most text sits in one half or the other"
            let narrow = |n: &&Node| n.text && n.r.w() < 0.55 * w;
            let left_n: usize = nodes.iter().filter(narrow).filter(|n| n.r.x1 <= gutter).map(|n| n.n).sum();
            let right_n: usize = nodes.iter().filter(narrow).filter(|n| n.r.x0 >= gutter).map(|n| n.n).sum();
            let all_n = nodes.iter().filter(|n| n.text).map(|n| n.n).sum::<usize>().max(1);
            let halves = (left_n + right_n) as f64 > 0.5 * all_n as f64
                && left_n.min(right_n) as f64 > 0.2 * (left_n + right_n) as f64;
            if std::env::var("P2E_DEBUG_ORDER").ok().and_then(|v| v.parse::<usize>().ok()) == Some(pno + 1) {
                eprintln!("ORDER gutter={gutter:.1} max_n={max_n} halves={halves}");
            }
            max_n >= 4 || halves
        } else if max_n >= 4 {
            true
        } else if let Some(g) = inherited.filter(|&g| !nodes.iter().any(|n| self.crosses(pno, n, g))) {
            // a band too short to show its columns: the page's
            gutter = g;
            nodes.iter().any(|n| n.r.cx() < g) && nodes.iter().any(|n| n.r.cx() >= g)
        } else {
            false
        };
        if !columns {
            return leaf(nodes, out);
        }
        let g = gutter;
        let (wide, rest): (Vec<Node>, Vec<Node>) = nodes.into_iter().partition(|n| self.crosses(pno, n, g));
        if wide.is_empty() {
            let (left, right): (Vec<Node>, Vec<Node>) = rest.into_iter().partition(|n| n.r.cx() < g);
            if left.is_empty() || right.is_empty() {
                return leaf(left.into_iter().chain(right).collect(), out);
            }
            self.order_rec(pno, singles, left, depth + 1, None, (key.0, g), out);
            self.order_rec(pno, singles, right, depth + 1, None, (g, key.1), out);
            return;
        }
        // blocks across the gutter, one after the other, form full-width zones; text beside their
        // lines (the other half of a two-column legend) belongs to the zone
        let mut wide = wide;
        wide.sort_by(|a, b| cmp(a.r.y0, b.r.y0));
        let tol = 0.8 * self.body_size;
        let line_like = |n: &Node| n.text && n.r.h() <= 3.0 * self.body_size;
        // (top, bottom, extent of its lines, members)
        type Zone = (f64, f64, Option<(f64, f64)>, Vec<Node>);
        let mut zones: Vec<Zone> = Vec::new();
        for n in wide {
            let lines = if line_like(&n) { Some((n.r.y0, n.r.y1)) } else { None };
            match zones.last_mut() {
                Some(z) if n.r.y0 <= z.1 + tol => {
                    z.1 = z.1.max(n.r.y1);
                    if let Some((a, b)) = lines {
                        z.2 = Some(z.2.map_or((a, b), |(c, d)| (c.min(a), d.max(b))));
                    }
                    z.3.push(n);
                }
                _ => zones.push((n.r.y0, n.r.y1, lines, vec![n])),
            }
        }
        let mut band_nodes: Vec<Node> = Vec::new();
        for n in rest {
            let c = (n.r.y0 + n.r.y1) / 2.0;
            match zones.iter_mut().find(|z| line_like(&n) && z.2.is_some_and(|(a, b)| a < c && c < b)) {
                Some(z) => z.3.push(n),
                None => band_nodes.push(n),
            }
        }
        if band_nodes.is_empty() && zones.len() == 1 {
            return leaf(zones.pop().unwrap().3, out);
        }
        let mut prev = f64::NEG_INFINITY;
        for (y0, _, _, members) in zones {
            let (band, later): (Vec<Node>, Vec<Node>) = band_nodes.into_iter().partition(|n| prev <= n.r.y0 && n.r.y0 < y0);
            band_nodes = later;
            self.order_rec(pno, singles, band, depth + 1, Some(g), key, out);
            self.order_rec(pno, singles, members, depth + 1, None, key, out);
            prev = y0;
        }
        self.order_rec(pno, singles, band_nodes, depth + 1, Some(g), key, out);
    }

    pub fn classify(&mut self, input: Vec<Para>) -> Vec<Para> {
        let body = self.body_size;
        // glue wrapped large-font lines (titles, long headings)
        let mut paras: Vec<Para> = Vec::new();
        for p in input {
            if let Some(q) = paras.last_mut() {
                let (ps, qs) = (p.size(), q.size());
                let (pb, qb) = (p.bbox(), q.bbox());
                if p.kind == Kind::Text
                    && q.kind == Kind::Text
                    && p.page == q.page
                    && p.boxed == q.boxed
                    && ps == qs
                    && ps >= body + 1.0
                    && pb.y0 - qb.y1 >= 0.0
                    && pb.y0 - qb.y1 < 0.9 * ps
                    && count(&q.text()) + count(&p.text()) < 400
                {
                    q.lines.extend(p.lines);
                    continue;
                }
            }
            paras.push(p);
        }
        {
            let cands: Vec<(usize, f64)> =
                paras.iter().enumerate().filter(|(_, p)| p.page == 0 && p.kind == Kind::Text && p.boxed.is_none()).map(|(i, p)| (i, p.size())).collect();
            if let Some(&(bi, bs)) = first_max(&cands, |x| x.1) {
                if bs >= body + 2.0 && count(&paras[bi].text()) < 400 {
                    paras[bi].kind = Kind::Title;
                }
            }
        }
        // the masthead above the title (the journal's logo, an "ARTICLE | Open Access" banner) is no figure
        if let Some(ty) = paras.iter().find(|p| p.page == 0 && p.kind == Kind::Title).map(|p| p.bbox().y0) {
            let h = self.pages[0].height;
            paras.retain(|p| !(p.page == 0 && p.kind == Kind::Figure && p.bbox().y1 <= ty + 2.0 && p.bbox().h() < 0.25 * h));
        }
        let mut heading_sizes: Vec<f64> = Vec::new();
        // (a box's own headings and labels stay in the box: plain text)
        for p in paras.iter_mut().filter(|p| p.kind == Kind::Text && p.boxed.is_none()) {
            let text = p.text();
            let t = trim(&text);
            if caption_re().is_match(t) {
                p.kind = Kind::Caption;
                continue;
            }
            let n = count(t);
            let total = p.char_count().max(1) as f64;
            let boldish = p.lines.iter().map(|l| l.bold * count(&l.text) as f64).sum::<f64>() / total;
            let numbered = re!(r"^(\d+(?:\.\d+)*)\.?\s+[A-Z]").is_match(t);
            let ends_sentence = t.ends_with('.') && n > 60;
            let size = p.size();
            if n <= 160
                && p.lines.len() <= 3
                && !ends_sentence
                && (size >= body + 1.0
                    || known_sections().is_match(t)
                    || (boldish > 0.85 && n < 110 && size >= body - 0.6)
                    || (numbered && boldish > 0.5))
            {
                if size < body - 1.5 {
                    continue;
                }
                p.kind = Kind::Heading;
                if !heading_sizes.contains(&size) {
                    heading_sizes.push(size);
                }
            }
        }
        // runs of 3+ "headings" are usually table cells or figure labels
        let mut i = 0;
        while i < paras.len() {
            let mut j = i;
            while j < paras.len() && paras[j].kind == Kind::Heading {
                j += 1;
            }
            if j - i >= 3 {
                for p in &mut paras[i..j] {
                    let text = p.text();
                    let t = trim(&text);
                    if !(known_sections().is_match(t) || re!(r"^[A-Z]?\d+(\.\d+)*\.?\s+[A-Z][a-z]").is_match(t)) {
                        p.kind = Kind::Text;
                    }
                }
            }
            i = j.max(i + 1);
        }
        for p in paras.iter_mut().filter(|p| p.kind == Kind::Heading) {
            let text = p.text();
            let t = trim(&text);
            if re!(r"^[\(\[\d·.,\s]+$").is_match(t) || t.starts_with('(') || t.starts_with('[') {
                p.kind = Kind::Text;
            }
        }
        // front matter: page-1 material before the first real section heading
        let mut boundary: Option<usize> = None;
        for (k, p) in paras.iter().enumerate() {
            if p.page > 1 {
                break;
            }
            let text = p.text();
            let t = trim(&text);
            // a summary page's "Highlights" box is still front matter (Cell: graphical abstract,
            // highlights, in brief — then the article itself with title and authors on the next page)
            if p.kind == Kind::Heading && re!(r"(?i)^(highlights|graphical abstract|in brief)$").is_match(t) {
                continue;
            }
            if p.kind == Kind::Heading && (known_sections().is_match(t) || re!(r"^\d+\.?\s+[A-Z]").is_match(t)) {
                boundary = Some(k);
                break;
            }
        }
        let mut bnd = boundary.unwrap_or_else(|| {
            paras.iter().position(|p| p.page > 1 || (p.page == 1 && p.kind == Kind::Heading)).unwrap_or(paras.len())
        });
        // a piece without section headings (the first one is "References"): the body starts at
        // its first paragraph in body type, not after a guessed abstract
        if let Some(b) = boundary {
            if re!(r"(?i)^(references|bibliography|acknowledge?ments?|funding|author contributions?|competing interests?|declaration of interests?|conflicts? of interest|data availability|code availability)\s*[:.]?$")
                .is_match(trim(&paras[b].text()))
            {
                if let Some(k) = paras[..b]
                    .iter()
                    .position(|p| p.kind == Kind::Text && (p.size() - body).abs() < 0.6 && count(&p.text()) >= 150)
                {
                    bnd = k;
                }
            }
        }
        let mut abstract_started = false;
        for p in &mut paras[..bnd] {
            if matches!(p.kind, Kind::Title | Kind::Figure | Kind::Table | Kind::Caption | Kind::Equation) || p.boxed.is_some() {
                continue;
            }
            let text = p.text();
            let affil = text
                .split(|c: char| c.is_whitespace() && !matches!(c, '\n' | '\r'))
                .filter(|w| re!(r"(?i)universit|institut|department|centre|center|hospital|laborator|faculty").is_match(w))
                .count();
            if abstract_started && p.kind == Kind::Text {
                continue;
            }
            if re!(r"(?i)^abstract\b").is_match(trim(&text)) || (count(&text) >= 500 && affil < 2) {
                p.kind = Kind::Abstract;
                abstract_started = true;
            } else if p.kind == Kind::Text || p.kind == Kind::Heading {
                p.kind = Kind::Front;
            }
        }
        // affiliation / correspondence footnotes on the first page belong to the front matter, and
        // so does a DOI on a line of its own
        for p in paras.iter_mut().filter(|p| p.page <= 1 && p.kind == Kind::Text && p.boxed.is_none()) {
            if doi_line(&p.text()) {
                p.kind = Kind::Front;
            }
        }
        for p in paras.iter_mut().filter(|p| p.page == 0 && p.kind == Kind::Text && p.boxed.is_none()) {
            let text = p.text();
            let affil = re!(r"(?i)universit|institut|department|hospital|laborator|faculty").is_match(&text);
            if affil && (re!(r"(?i)e-?mail:|correspond").is_match(&text) || re!(r"^\d\s?[A-Z]").is_match(&text)) {
                p.kind = Kind::Front;
            }
            // "A and B contributed equally to this work." at the foot of the first page
            if count(&text) < 300 && re!(r"(?i)contributed equally|equal(ly)? contribut|these authors contributed").is_match(&text) {
                p.kind = Kind::Front;
            }
        }
        // the first pages' footnotes: correspondence, e-mail, dates, editor, licence, ORCID
        for p in paras.iter_mut().filter(|p| p.page <= 1 && p.kind == Kind::Text && p.boxed.is_none()) {
            let h = self.pages[p.page].height;
            if front_footnote(&p.text(), p.size(), body, p.bbox().y0 > 0.75 * h) {
                p.kind = Kind::Front;
            }
        }
        heading_sizes.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
        let big: Vec<f64> = heading_sizes.iter().copied().filter(|&s| s >= body + 1.0).collect();
        let title_text = paras.iter().find(|p| p.kind == Kind::Title).map(|p| p.text()).unwrap_or_default();
        for p in paras.iter_mut().filter(|p| p.kind == Kind::Heading) {
            let text = p.text();
            let t = trim(&text);
            if !title_text.is_empty() && similar(t, &title_text) {
                p.kind = Kind::Title;
                continue;
            }
            if known_sections().is_match(t) && !re!(r"^\d+\.\d").is_match(t) {
                p.level = 1;
                continue;
            }
            if let Some(c) = re!(r"^(\d+(?:\.\d+)*)\.?\s").captures(t) {
                p.level = 3.min(c[1].matches('.').count() + 1);
            } else {
                let size = p.size();
                p.level = 3.min(big.iter().position(|&s| s == size).map(|i| i + 1).unwrap_or(big.len() + 1));
            }
        }
        // display equations: mostly math-font lines; merge stacked pieces
        let math_frac = |p: &Para| -> (f64, usize) {
            let n = p.char_count();
            (p.lines.iter().map(|l| l.math * count(&l.text) as f64).sum::<f64>() / n.max(1) as f64, n)
        };
        let mut out: Vec<Para> = Vec::new();
        for mut p in paras {
            if p.kind == Kind::Text && !p.lines.is_empty() && p.boxed.is_none() {
                let (mf, n) = math_frac(&p);
                if mf > 0.3 && n < 400 && !is_prose(&p.text()) {
                    p.kind = Kind::Equation;
                }
            }
            if let Some(prev) = out.last_mut() {
                let gap = p.bbox().y0 - prev.bbox().y1;
                // stacked pieces (not a piece back up the page, at the top of the next column)
                if prev.page == p.page
                    && (p.kind == Kind::Equation || (p.kind == Kind::Text && p.boxed.is_none()))
                    && prev.kind == Kind::Equation
                    && gap < 1.6 * body
                    && p.bbox().y0 > prev.bbox().y0 - body
                    && (p.kind == Kind::Equation || count(&p.text()) < 30)
                {
                    let mut lines = std::mem::take(&mut prev.lines);
                    lines.extend(p.lines);
                    *prev = Para::new(lines, p.page, Kind::Equation);
                    continue;
                }
                if prev.page == p.page
                    && p.kind == Kind::Equation
                    && prev.kind == Kind::Text
                    && prev.boxed.is_none()
                    && count(&prev.text()) < 30
                    && gap < body
                    && !re!(r"[a-z]{4}").is_match(&prev.text())
                {
                    let mut lines = std::mem::take(&mut prev.lines);
                    lines.extend(p.lines);
                    *prev = Para::new(lines, p.page, Kind::Equation);
                    continue;
                }
            }
            out.push(p);
        }
        for p in out.iter_mut() {
            if p.kind == Kind::Equation {
                let (page, b) = (p.page, p.bbox());
                p.asset = self.render(page, b, false, &format!("p{}-eq{}-{}", page + 1, b.y0 as i64, b.x0 as i64));
            }
        }
        out
    }

    // MARK: - Metadata

    pub fn meta_guess(&self, paras: &[Para], info: &DocInfo, quick_doi: impl FnOnce() -> Option<String>) -> super::super::PaperMeta {
        let mut m = super::super::PaperMeta::default();
        let t = trim(&info.title);
        if count(t) > 12 && !re!(r"(?i)microsoft word|untitled|\.docx?$|\.pdf$|^paper$").is_match(t) {
            m.title = t.to_string();
        }
        if m.title.is_empty() {
            if let Some(tp) = paras.iter().find(|p| p.kind == Kind::Title) {
                m.title = tp.text();
            }
        }
        let a = trim(&info.author);
        if !a.is_empty() && !re!(r"(?i)administrator|user|owner|^admin").is_match(a) {
            m.authors = a
                .split([';', ','])
                .flat_map(|s| s.split(" and "))
                .map(|s| trim(s).to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
        if let Some(y) = info.year {
            m.date = y.to_string();
        }
        let first_text = paras.iter().take(60).filter(|p| p.page < 2).map(|p| p.text()).collect::<Vec<_>>().join(" ")
            + " "
            + &info.subject;
        if let Some(d) = super::super::resolver::doi_in(&first_text).or_else(quick_doi) {
            m.doi = d;
        }
        m
    }
}

/// When the PDF tells hyphenation hyphens apart (lines ending in [`GLUE`]), the hyphens left at
/// line ends are real ones ("sample-|level"): mark them [`HARD_HYPHEN`] so joining keeps them.
/// Decided per font size: a layout program may do it for the body text and not for the legends.
pub fn mark_hard_hyphens(pages: &mut [Page]) {
    let key = |s: f64| ((s * 2.0).round() / 2.0 * 10.0) as i64;
    let mut glued: HashMap<i64, usize> = HashMap::new();
    let mut ends: HashMap<i64, usize> = HashMap::new();
    for l in pages.iter().flat_map(|p| &p.lines) {
        if l.text.ends_with(GLUE) {
            *glued.entry(key(l.size)).or_insert(0) += 1;
        } else if re!(r"[A-Za-z]-$").is_match(&l.text) {
            *ends.entry(key(l.size)).or_insert(0) += 1;
        }
    }
    for l in pages.iter_mut().flat_map(|p| p.lines.iter_mut()) {
        let k = key(l.size);
        let g = glued.get(&k).copied().unwrap_or(0);
        if g < 3 || g < ends.get(&k).copied().unwrap_or(0) / 2 || !re!(r"[A-Za-z]-$").is_match(&l.text) {
            continue;
        }
        l.text.pop();
        l.text.push(HARD_HYPHEN);
        if let Some(sp) = l.spans.iter_mut().rev().find(|sp| !sp.text.trim_end().is_empty()) {
            let t = sp.text.trim_end().to_string();
            if let Some(stripped) = t.strip_suffix('-') {
                sp.text = format!("{stripped}{HARD_HYPHEN}");
            }
        }
    }
}

/// A footnote of the first pages that belongs to the front matter, not the body: correspondence
/// and e-mail, "A full list of affiliations…", dates, editor, competing interests, ORCID, licence.
/// In footnote type anywhere on the page, or in body type at its foot if it starts like one; a
/// sentence of the body that merely gives an address ("Requests to x@y") is in body type and does
/// not start like a footnote.
pub fn front_footnote(text: &str, size: f64, body: f64, at_foot: bool) -> bool {
    let t = trim(text);
    if count(t) > 1200 || re!(r"(?i)^(data|code|materials?|software)( and \w+)? availability").is_match(t) {
        return false;
    }
    let starts = re!(concat!(
        r"(?i)^[\W\d]{0,3}\s*(",
        r"corresponding authors?|\*?\s*correspondence\b|(to )?whom (all )?correspondence|address(ed)? (all )?correspondence|",
        r"e-?mail( address(es)?)?\s*:|email\b|edited by\b|reviewed by\b|academic editor\b|handling editor\b|",
        r"(received|accepted|revised|published( online)?)\s*[:\d]|published online\b|available online\b|first published\b|article history\b|",
        r"author contributions?\s*:|competing interests?\s*:|conflicts? of interests?\s*:|the authors? declares?\b|",
        r"orcid\b|these authors contributed|present address|current address|deceased\b|a full list of affiliations|",
        r"©|copyright\b|\(c\)\s*(19|20)\d\d|this (is an )?open access|open access\b|this article (is a pnas|is an open|is licensed|is distributed|contains supporting)|",
        r"https?://doi\.org/|doi:\s*10\.",
        r")"
    ));
    let anywhere = re!(concat!(
        r"(?i)✉|\ba full list of affiliations\b|\breceived in revised form\b|\bPNAS Direct Submission\b|",
        r"\bto whom correspondence\b|\bcorresponding authors?\b|\bcontributed equally\b"
    ));
    let email = re!(r"[\w.+-]+@[\w-]+(\.[\w-]+)+");
    if size <= body - 0.4 {
        starts.is_match(t) || anywhere.is_match(t) || email.is_match(t)
    } else {
        at_foot && (starts.is_match(t) || anywhere.is_match(t))
    }
}

/// How much of `r` the text lines cover (their overlaps added up).
fn text_cover(r: &R, lines: &[PLine]) -> f64 {
    lines
        .iter()
        .map(|l| {
            let o = r.intersection(&l.bbox);
            if o.w() > 0.0 && o.h() > 0.0 { o.area() } else { 0.0 }
        })
        .sum()
}

fn merge_clusters(cs: &mut Vec<Cluster>, should_merge: impl Fn(&R, &R) -> bool) {
    let mut merged = true;
    while merged {
        merged = false;
        'outer: for i in 0..cs.len() {
            for j in (i + 1)..cs.len() {
                if should_merge(&cs[i].rect, &cs[j].rect) {
                    let c = cs.remove(j);
                    let t = &mut cs[i];
                    t.rect = t.rect.union(&c.rect);
                    t.has_image |= c.has_image;
                    t.n += c.n;
                    merged = true;
                    break 'outer;
                }
            }
        }
    }
}

/// JPEG for photos, PNG for line art and equations.
pub fn encode(img: &image::RgbImage, photo: bool, name: &str) -> Option<BookImage> {
    let mut out = Vec::new();
    if photo {
        let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85);
        enc.encode_image(img).ok()?;
        return Some(BookImage { file_name: format!("{name}.jpg"), data: out, media_type: "image/jpeg".into() });
    }
    let enc = image::codecs::png::PngEncoder::new_with_quality(
        &mut out,
        image::codecs::png::CompressionType::Best,
        image::codecs::png::FilterType::Adaptive,
    );
    image::DynamicImage::ImageRgb8(img.clone()).write_with_encoder(enc).ok()?;
    Some(BookImage { file_name: format!("{name}.png"), data: out, media_type: "image/png".into() })
}

/// Same text, ignoring case and punctuation (or one contains the other, for long strings).
pub fn similar(a: &str, b: &str) -> bool {
    let na = re!(r"\W+").replace_all(&a.to_lowercase(), "").into_owned();
    let nb = re!(r"\W+").replace_all(&b.to_lowercase(), "").into_owned();
    !na.is_empty() && !nb.is_empty() && (na == nb || (count(&na) > 20 && (na.contains(&nb) || nb.contains(&na))))
}

/// Paragraph text as XHTML with italics/bold/super- and subscripts.
pub fn styled(p: &Para) -> String {
    use super::super::esc;
    let total = p.char_count().max(1);
    let bold_chars: usize = p.lines.iter().flat_map(|l| &l.spans).filter(|s| s.bold).map(|s| count(&s.text)).sum();
    let para_bold = bold_chars as f64 > 0.85 * total as f64;
    let mut lines_html: Vec<(String, String)> = Vec::new();
    for l in &p.lines {
        let mut h = String::new();
        for s in &l.spans {
            let core = trim(&s.text);
            if core.is_empty() {
                h += &esc(&s.text);
                continue;
            }
            let lead: String = s.text.chars().take_while(|&c| c == ' ').collect();
            let trail: String = s.text.chars().rev().take_while(|&c| c == ' ').collect();
            let mut c = esc(core);
            let n = count(core);
            if s.sup && n < 25 {
                c = format!("<sup>{c}</sup>");
            } else if s.sub && n < 25 {
                c = format!("<sub>{c}</sub>");
            } else {
                if s.bold && !para_bold {
                    c = format!("<strong>{c}</strong>");
                }
                if s.italic {
                    c = format!("<em>{c}</em>");
                }
            }
            h += &lead;
            h += &c;
            h += &trail;
        }
        // the line-end markers: the plain text keeps them to decide the join, the HTML never does
        let h = trim(&h).replace(GLUE, "").replace(HARD_HYPHEN, "-");
        lines_html.push((trim(&l.text).to_string(), h));
    }
    // join with the same de-hyphenation rules as join_lines
    let mut out = String::new();
    let mut out_plain = String::new();
    for (plain, html) in lines_html {
        if plain.is_empty() {
            continue;
        }
        if out_plain.is_empty() {
            out = html;
            out_plain = plain;
            continue;
        }
        let hyph = re!(r"-((</[a-z]+>)*)$");
        if out_plain.ends_with(GLUE) {
            out += &html;
            out_plain.pop();
            out_plain += &plain;
        } else if out_plain.ends_with(HARD_HYPHEN) {
            out += &html;
            out_plain.pop();
            out_plain.push('-');
            out_plain += &plain;
        } else if url_continues(&out_plain, &plain) {
            out += &html;
            out_plain += &plain;
        } else if re!(r"[A-Za-z]-$").is_match(&out_plain) && first_is_lower(&plain) && hyph.is_match(&out) {
            let m = hyph.find(&out).unwrap();
            let tags = out[m.start() + 1..].to_string();
            out.truncate(m.start());
            out += &tags;
            out += &html;
            out_plain.pop();
            out_plain += &plain;
        } else if (out_plain.ends_with('-') || out_plain.ends_with('/') || out_plain.ends_with('–'))
            && !out_plain.ends_with(" -")
        {
            out += &html;
            out_plain += &plain;
        } else {
            out.push(' ');
            out += &html;
            out_plain.push(' ');
            out_plain += &plain;
        }
    }
    out
}
