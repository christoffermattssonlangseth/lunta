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
}

impl Para {
    pub fn new(lines: Vec<PLine>, page: usize, kind: Kind) -> Para {
        Para { lines, page, kind, level: 0, asset: None, region: None }
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
        first_max(&c, |x| x.1).map(|x| x.0).unwrap_or(0.0)
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
        if re!(r"[A-Za-z]-$").is_match(&out) && first_is_lower(p) {
            out.pop();
            out += p;
        } else if (out.ends_with('-') || out.ends_with('/') || out.ends_with('–')) && !out.ends_with(" -") {
            out += p;
        } else {
            out.push(' ');
            out += p;
        }
    }
    out
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

pub fn caption_re() -> &'static Regex {
    re!(r"(?i)^(?:Supplementary\s+|Extended\s+Data\s+|Supporting\s+)?(Figure|Fig\.?|Table|Algorithm|Box)\s*S?\d+[A-Za-z]?\s*[.:|\-–—]?")
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
        let body_size = first_max(&sizes, |x| x.1).map(|x| x.0).unwrap_or(10.0);
        let mut a = Analyzer { pages, body_size, running: HashSet::new(), renderer, log };
        a.find_running();
        a
    }

    fn norm(s: &str) -> String {
        let s = s.to_lowercase();
        let s = re!(r"\b[es]?\d+\b").replace_all(&s, "");
        let s = re!(r"\s+").replace_all(&s, " ");
        trim(&s).to_string()
    }

    /// Running heads and feet: lines in the top/bottom margin repeated on many pages.
    fn find_running(&mut self) {
        let n = self.pages.len();
        if n < 3 {
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
        if re!(r"^\d{1,4}$").is_match(t) && l.bbox.w() < 25.0 {
            return true;
        }
        if re!(r"(?i)^(bioRxiv|medRxiv) preprint doi").is_match(t) || t.contains("copyright holder for this preprint") {
            return true;
        }
        if in_margin && re!(r"(?i)^(downloaded from|arxiv:\d{4}\.\d+)").is_match(t) {
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
        for c in clusters.iter_mut() {
            for r in &thin {
                if c.rect.intersects(r, 2.0) {
                    c.rect = c.rect.union(r);
                }
            }
        }
        let mut regions: Vec<Region> = Vec::new();
        let (m_top, m_bot) = (0.07 * page.height, 0.93 * page.height);
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
            regions.push(Region { rect: r, kind: Kind::Figure, image: None });
        }

        // tables: caption + horizontal rules, or a borderless grid of short cells
        let caps: Vec<usize> = (0..lines.len())
            .filter(|&i| re!(r"(?i)^(Supplementary\s+)?Table\s*S?\d+").is_match(trim(&lines[i].text)))
            .collect();
        let mut hrules: Vec<R> = thin.iter().filter(|r| r.w() > 80.0 && r.h() < 2.5).copied().collect();
        hrules.sort_by(|a, b| a.y0.partial_cmp(&b.y0).unwrap_or(std::cmp::Ordering::Equal));
        for ci in caps {
            let cap = &lines[ci];
            if regions.iter().any(|g| cap.bbox.intersects(&g.rect, 30.0)) {
                continue;
            }
            let below: Vec<R> = hrules.iter().filter(|r| r.y0 > cap.bbox.y0 - 2.0).copied().collect();
            let above: Vec<R> = hrules.iter().filter(|r| r.y1 < cap.bbox.y0 + 2.0).copied().collect();
            let mut grp: Option<Vec<R>> = None;
            if let Some(f) = below.first().filter(|f| f.y0 - cap.bbox.y1 < 60.0) {
                let mut gg = vec![*f];
                for r in &below[1..] {
                    if r.y0 - gg.last().unwrap().y1 < 250.0 {
                        gg.push(*r);
                    } else {
                        break;
                    }
                }
                grp = Some(gg);
            } else if let Some(l) = above.last().filter(|l| cap.bbox.y0 - l.y1 < 60.0) {
                let mut gg = vec![*l];
                for r in above[..above.len() - 1].iter().rev() {
                    if gg.last().unwrap().y0 - r.y1 < 250.0 {
                        gg.push(*r);
                    } else {
                        break;
                    }
                }
                grp = Some(gg);
            }
            if let Some(grp) = grp.filter(|g| g.len() >= 2) {
                regions.push(Region { rect: R::union_all(&grp), kind: Kind::Table, image: None });
            } else if let Some(tr) = self.text_table_region(ci, lines, page) {
                regions.push(Region { rect: tr, kind: Kind::Table, image: None });
            }
        }
        // expand regions with nearby short labels (axis labels, panel letters)
        for g in regions.iter_mut() {
            let mut r = g.rect;
            for _ in 0..2 {
                for l in lines {
                    let t = trim(&l.text);
                    if caption_re().is_match(t) {
                        continue;
                    }
                    let vo = r.y1.min(l.bbox.y1) - r.y0.max(l.bbox.y0) > 0.6 * l.bbox.h();
                    let n = count(t);
                    if r.intersects(&l.bbox, 8.0)
                        && n < 70
                        && !r.contains(&l.bbox, 2.0)
                        && (l.size < self.body_size - 0.4 || vo || n <= 12)
                    {
                        r = r.union(&l.bbox);
                    }
                }
            }
            g.rect = r;
        }
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
                    && l.bbox.x0 - m.bbox.x1 < 1.2 * l.size
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
        let units = self.order(pno, units);
        let txt: Vec<R> = units.iter().filter(|u| u.kind == Kind::Text).map(|u| u.lines[0].bbox).collect();
        let right_edge = |l: &PLine| -> f64 {
            txt.iter()
                .filter(|b| b.x1.min(l.bbox.x1) - b.x0.max(l.bbox.x0) > 0.3 * l.bbox.w())
                .map(|b| b.x1)
                .fold(None, |m: Option<f64>, x| Some(m.map_or(x, |m| m.max(x))))
                .unwrap_or(l.bbox.x1)
        };
        let abbrev = re!(r"(\b(e\.g|i\.e|et al|cf|Fig|Figs|Eq|Eqs|vs|approx|ca|resp|Ref|Refs|No|Dr|Prof|al)\.|\b[A-Z]\.)$");
        let w = self.pages[pno].width;
        let mut paras: Vec<Para> = Vec::new();
        let mut cur: Vec<PLine> = Vec::new();
        let flush = |cur: &mut Vec<PLine>, paras: &mut Vec<Para>| {
            if !cur.is_empty() {
                paras.push(Para::new(std::mem::take(cur), pno, Kind::Text));
            }
        };
        for u in units {
            if u.kind != Kind::Text {
                flush(&mut cur, &mut paras);
                paras.push(u);
                continue;
            }
            let l = u.lines.into_iter().next().unwrap();
            let Some(a) = cur.last() else {
                cur.push(l);
                continue;
            };
            let left = cur.iter().map(|c| c.bbox.x0).fold(f64::INFINITY, f64::min);
            let gap = l.bbox.y0 - a.bbox.y1;
            let jumped = l.bbox.y0 < a.bbox.y0 - 2.0 || (l.bbox.x0 - left).abs() > 0.3 * w;
            let re_a = right_edge(a);
            let width = (re_a - left).max(1.0);
            let at = trim(&a.text);
            let ends_sentence = at.ends_with('.') || at.ends_with(':') || at.ends_with('?') || at.ends_with('!');
            let short_a = (re_a - a.bbox.x1) > 0.18 * width && ends_sentence && !abbrev.is_match(at);
            let indent = l.bbox.x0 - left > 8.0
                && cur[1..].iter().all(|c| (c.bbox.x0 - left).abs() < 3.0)
                && ends_sentence
                && !abbrev.is_match(at);
            let hanging = cur.len() >= 2 && l.bbox.x0 < a.bbox.x0 - 6.0 && cur[0].bbox.x0 < a.bbox.x0 - 6.0;
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
                || (cur.len() == 1 && l.bbox.x0 > a.bbox.x0 + 8.0 && gap > 0.3 * l.size)
            {
                if jumped
                    && gap <= 0.75 * l.size
                    && !short_a
                    && !size_change
                    && !cap
                    && first_is_lower(trim(&l.text))
                {
                    cur.push(l); // column continuation
                    continue;
                }
                flush(&mut cur, &mut paras);
            }
            cur.push(l);
        }
        flush(&mut cur, &mut paras);
        paras
    }

    /// Reading order: one column top to bottom, or two columns between full-width blocks.
    pub fn order(&self, pno: usize, paras: Vec<Para>) -> Vec<Para> {
        let w = self.pages[pno].width;
        let info: Vec<(R, usize, bool)> = paras.iter().map(|p| (p.bbox(), count(&p.text()), p.kind == Kind::Text)).collect();
        let chars_n: usize = info.iter().filter(|x| x.2 && x.0.w() < 0.55 * w).map(|x| x.1).sum();
        let chars_all = info.iter().filter(|x| x.2).map(|x| x.1).sum::<usize>().max(1);
        let left: usize = info.iter().filter(|x| x.2 && x.0.w() < 0.55 * w && x.0.cx() < w / 2.0).map(|x| x.1).sum();
        let right = chars_n - left;
        let two_col = chars_n as f64 > 0.5 * chars_all as f64
            && left as f64 > 0.2 * chars_n as f64
            && right as f64 > 0.2 * chars_n as f64;
        let mut idx: Vec<usize> = (0..paras.len()).collect();
        let cmp = |a: f64, b: f64| a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal);
        if !two_col {
            idx.sort_by(|&a, &b| {
                let (ra, rb) = (info[a].0, info[b].0);
                cmp((ra.y0 / 4.0).round(), (rb.y0 / 4.0).round()).then(cmp(ra.x0, rb.x0))
            });
        } else {
            let mut wide: Vec<usize> = idx.iter().copied().filter(|&i| info[i].0.w() >= 0.55 * w).collect();
            wide.sort_by(|&a, &b| cmp(info[a].0.y0, info[b].0.y0));
            let rest: Vec<usize> = idx.iter().copied().filter(|&i| info[i].0.w() < 0.55 * w).collect();
            let mut out: Vec<usize> = Vec::new();
            let mut prev_y = -1.0;
            for wp in wide.iter().map(Some).chain(std::iter::once(None)) {
                let y = wp.map(|&i| info[i].0.y0).unwrap_or(f64::MAX);
                let band: Vec<usize> =
                    rest.iter().copied().filter(|&i| prev_y <= info[i].0.y0 && info[i].0.y0 < y).collect();
                for left_side in [true, false] {
                    let mut side: Vec<usize> =
                        band.iter().copied().filter(|&i| (info[i].0.cx() < w / 2.0) == left_side).collect();
                    side.sort_by(|&a, &b| cmp(info[a].0.y0, info[b].0.y0));
                    out.extend(side);
                }
                if let Some(&i) = wp {
                    out.push(i);
                    prev_y = info[i].0.y0;
                }
            }
            idx = out;
        }
        let mut slots: Vec<Option<Para>> = paras.into_iter().map(Some).collect();
        idx.into_iter().filter_map(|i| slots[i].take()).collect()
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
                    && ps == qs
                    && ps >= body + 1.0
                    && pb.y0 - qb.y1 >= 0.0
                    && pb.y0 - qb.y1 < 0.9 * ps
                    && count(&q.text()) + count(&p.text()) < 400
                {
                    let mut lines = std::mem::take(&mut q.lines);
                    lines.extend(p.lines);
                    *q = Para::new(lines, q.page, Kind::Text);
                    continue;
                }
            }
            paras.push(p);
        }
        {
            let cands: Vec<(usize, f64)> =
                paras.iter().enumerate().filter(|(_, p)| p.page == 0 && p.kind == Kind::Text).map(|(i, p)| (i, p.size())).collect();
            if let Some(&(bi, bs)) = first_max(&cands, |x| x.1) {
                if bs >= body + 2.0 && count(&paras[bi].text()) < 400 {
                    paras[bi].kind = Kind::Title;
                }
            }
        }
        let mut heading_sizes: Vec<f64> = Vec::new();
        for p in paras.iter_mut().filter(|p| p.kind == Kind::Text) {
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
            if p.kind == Kind::Heading && (known_sections().is_match(t) || re!(r"^\d+\.?\s+[A-Z]").is_match(t)) {
                boundary = Some(k);
                break;
            }
        }
        let bnd = boundary.unwrap_or_else(|| {
            paras.iter().position(|p| p.page > 1 || (p.page == 1 && p.kind == Kind::Heading)).unwrap_or(paras.len())
        });
        let mut abstract_started = false;
        for p in &mut paras[..bnd] {
            if matches!(p.kind, Kind::Title | Kind::Figure | Kind::Table | Kind::Caption | Kind::Equation) {
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
        // affiliation / correspondence footnotes on the first page belong to the front matter
        for p in paras.iter_mut().filter(|p| p.page == 0 && p.kind == Kind::Text) {
            let text = p.text();
            let affil = re!(r"(?i)universit|institut|department|hospital|laborator|faculty").is_match(&text);
            if affil && (re!(r"(?i)e-?mail:|correspond").is_match(&text) || re!(r"^\d\s?[A-Z]").is_match(&text)) {
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
            if p.kind == Kind::Text && !p.lines.is_empty() {
                let (mf, n) = math_frac(&p);
                if mf > 0.3 && n < 400 {
                    p.kind = Kind::Equation;
                }
            }
            if let Some(prev) = out.last_mut() {
                let gap = p.bbox().y0 - prev.bbox().y1;
                if prev.page == p.page
                    && (p.kind == Kind::Equation || p.kind == Kind::Text)
                    && prev.kind == Kind::Equation
                    && gap < 1.6 * body
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
        lines_html.push((trim(&l.text).to_string(), trim(&h).to_string()));
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
        if re!(r"[A-Za-z]-$").is_match(&out_plain) && first_is_lower(&plain) && hyph.is_match(&out) {
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
