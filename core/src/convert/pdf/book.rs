//! Book mode: whether a PDF is a paper or a book, and what a book has that a paper hasn't —
//! running heads that change with the chapter and carry the page number, a printed table of
//! contents, an index, and an outline (bookmarks) that gives the chapters. A port of
//! `DocumentKind.swift` and `PDFBookLayout.swift`; papers never use any of it.

use super::layout::{re, Analyzer, Kind, Para};
use std::collections::{HashMap, HashSet};
use std::ops::Range;

/// What a PDF is converted as. A thesis is converted like a book.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DocumentKind {
    #[default]
    Paper,
    Book,
    Thesis,
}

impl DocumentKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            DocumentKind::Paper => "paper",
            DocumentKind::Book => "book",
            DocumentKind::Thesis => "thesis",
        }
    }
    /// Converted with the book pipeline (chapters from the outline, running heads per chapter, no
    /// printed table of contents, the index as one chapter, no abstract or affiliations).
    pub fn is_book_like(&self) -> bool {
        *self != DocumentKind::Paper
    }
}

/// The signals the classifier weighs (see Swift `KindSignals`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct KindSignals {
    pub pages: usize,
    pub outline_entries: usize,
    pub outline_top: usize,
    pub outline_depth: usize,
    pub isbn: bool,
    pub contents_page: bool,
    pub toc_pages: usize,
    pub chapter_pages: usize,
    pub index_page: bool,
    pub preface: bool,
    /// fraction of pages whose top or bottom line carries the page number with words
    pub numbered_heads: f64,
    pub work_type: String,
    pub abstract_: bool,
    pub doi_line: bool,
    pub received: bool,
    pub thesis_words: bool,
}

/// The kind, with the signals that decided it.
pub fn classify(s: &KindSignals) -> (DocumentKind, Vec<String>) {
    let t = s.work_type.trim().to_lowercase();
    let book_type = ["book", "monograph", "edited-book", "reference-book", "book-set"].contains(&t.as_str());
    let thesis_type = ["dissertation", "thesis", "phdthesis", "mastersthesis"].contains(&t.as_str());
    let paper_type = [
        "journal-article", "article-journal", "article", "proceedings-article", "paper-conference", "posted-content",
        "preprint", "book-chapter", "chapter", "review", "letter",
    ]
    .contains(&t.as_str());
    // short documents are papers, whatever else they show
    if s.pages < 40 && !((book_type || thesis_type) && s.pages >= 20) {
        return (DocumentKind::Paper, vec![format!("{} pages", s.pages)]);
    }
    let mut why: Vec<String> = Vec::new();
    let mut score = 0i32;
    let mut add = |n: i32, r: String| {
        score += n;
        why.push(format!("{}{n} {r}", if n > 0 { "+" } else { "" }));
    };
    if book_type || thesis_type {
        add(4, format!("work type {t}"));
    }
    if paper_type {
        add(-3, format!("work type {t}"));
    }
    if s.pages >= 150 {
        add(2, format!("{} pages", s.pages));
    } else if s.pages >= 80 {
        add(1, format!("{} pages", s.pages));
    }
    if s.outline_top >= 5 && s.outline_entries >= 15 && s.outline_depth >= 2 {
        add(2, format!("outline of {} chapters, {} entries", s.outline_top, s.outline_entries));
    } else if s.outline_top >= 8 {
        add(1, format!("outline of {} entries", s.outline_top));
    }
    if s.isbn {
        add(2, "ISBN".into());
    }
    if s.toc_pages >= 2 {
        add(2, format!("{} table-of-contents pages", s.toc_pages));
    } else if s.toc_pages == 1 || s.contents_page {
        add(1, "table of contents".into());
    }
    if s.chapter_pages >= 3 {
        add(2, format!("{} \"Chapter N\" pages", s.chapter_pages));
    }
    if s.index_page {
        add(1, "index".into());
    }
    if s.preface {
        add(1, "preface".into());
    }
    if s.numbered_heads >= 0.3 {
        add(1, format!("running heads with page numbers on {:.0}% of pages", 100.0 * s.numbered_heads));
    }
    if s.abstract_ {
        add(-2, "abstract".into());
    }
    if s.doi_line {
        add(-1, "DOI line on the first page".into());
    }
    if s.received {
        add(-2, "received/accepted dates".into());
    }
    if score < 4 {
        why.push(format!("score {score} < 4"));
        return (DocumentKind::Paper, why);
    }
    why.push(format!("score {score}"));
    let thesis = thesis_type || (s.thesis_words && !book_type);
    (if thesis { DocumentKind::Thesis } else { DocumentKind::Book }, why)
}

/// One entry of a PDF's outline (bookmarks): `y` from the top of the page (0 when the destination
/// gives no height).
#[derive(Debug, Clone, PartialEq)]
pub struct OutlineEntry {
    pub title: String,
    pub level: usize,
    pub page: usize,
    pub y: f64,
}

/// What the analyzer found out about a book, for the assembly.
#[derive(Debug, Clone, Default)]
pub struct BookInfo {
    pub kind: DocumentKind,
    pub reasons: Vec<String>,
    pub outline: Vec<OutlineEntry>,
    pub index_pages: Vec<Range<usize>>,
}

impl BookInfo {
    pub fn on_index_page(&self, pno: usize) -> bool {
        self.index_pages.iter().any(|r| r.contains(&pno))
    }
}

/// A part ("PART I Analyzing DNA") whose entries are chapters: they stay at its level.
pub fn is_part_title(t: &str) -> bool {
    re!(r"(?i)^part\s+([ivxlc]+|\d+|one|two|three|four|five|six|seven|eight|nine|ten)\b").is_match(t.trim())
}

/// The printed page number a margin line carries at either end ("28 3 Shallow neural networks",
/// "3.1 Neural network example 29"), if it has words besides.
pub fn head_number(t: &str) -> Option<i64> {
    let s = t.trim();
    if s.chars().count() > 150 || !s.chars().any(char::is_alphabetic) {
        return None;
    }
    if let Some(c) = re!(r"^(\d{1,4})\s").captures(s) {
        return c[1].parse().ok();
    }
    re!(r"\s(\d{1,4})$").captures(s).and_then(|c| c[1].parse().ok())
}

pub fn is_toc_line(t: &str) -> bool {
    re!(r"(?:\.\s?){4,}\s*(?:\d{1,4}|[ivxlcdm]{1,7})$").is_match(t.trim())
}

fn ends_in_number(t: &str) -> bool {
    re!(r"[\s,](?:\d{1,4}|[ivxlcdm]{1,7})$").is_match(t.trim())
}

/// "Chapter 3", "Appendix A", "Part II" on a line of its own: the label.
pub fn chapter_label(t: &str) -> Option<String> {
    re!(r"(?i)^(?:chapter|appendix|part|lecture)\s+(\d{1,3}|[A-Z]|[IVXLC]{1,6})\.?$").captures(t.trim()).map(|c| c[1].to_string())
}

pub fn is_contents_title(t: &str) -> bool {
    re!(r"(?i)^((table of )?contents( in brief)?|brief contents|contents at a glance)$").is_match(t.trim())
}

pub fn is_index_title(t: &str) -> bool {
    re!(r"(?i)^(subject |general |author |name )?index$").is_match(t.trim())
}

impl Analyzer<'_> {
    /// Line indices of a page from the top down, without the empty ones.
    fn top_down(&self, pno: usize) -> Vec<usize> {
        let lines = &self.pages[pno].lines;
        let mut ix: Vec<usize> = (0..lines.len()).filter(|&i| !lines[i].text.trim().is_empty()).collect();
        ix.sort_by(|&a, &b| lines[a].bbox.y0.partial_cmp(&lines[b].bbox.y0).unwrap_or(std::cmp::Ordering::Equal));
        ix
    }

    /// The top (or bottom) line of a page with the pieces on its baseline (PDFium gives the page
    /// number of "28 3 Shallow neural networks" as a line of its own): their indices and text.
    fn margin_line(&self, pno: usize, top: bool) -> Option<(Vec<usize>, String)> {
        let order = self.top_down(pno);
        let lines = &self.pages[pno].lines;
        let first = *if top { order.first()? } else { order.last()? };
        let fb = lines[first].bbox;
        let mut same: Vec<usize> = order
            .iter()
            .copied()
            .filter(|&i| {
                let b = lines[i].bbox;
                b.y1.min(fb.y1) - b.y0.max(fb.y0) > 0.5 * b.h().min(fb.h())
            })
            .collect();
        same.sort_by(|&a, &b| lines[a].bbox.x0.partial_cmp(&lines[b].bbox.x0).unwrap_or(std::cmp::Ordering::Equal));
        let text = same.iter().map(|&i| lines[i].text.trim()).collect::<Vec<_>>().join(" ");
        Some((same, text))
    }

    /// A margin line without its page number, roman or arabic.
    fn head_key(t: &str) -> String {
        let n = Self::norm(t);
        re!(r"^[ivxlcdm]{1,7}\s+|\s+[ivxlcdm]{1,7}$").replace_all(&n, "").into_owned()
    }

    /// Reads the signals from the lines of every page and the outline.
    pub fn kind_signals(&self) -> KindSignals {
        let n = self.pages.len();
        let mut s = KindSignals { pages: n, work_type: self.work_type.clone(), ..Default::default() };
        if n < 20 {
            return s;
        }
        let o = &self.book.outline;
        s.outline_entries = o.len();
        s.outline_top = o.iter().filter(|e| e.level == 1).count();
        s.outline_depth = o.iter().map(|e| e.level).max().unwrap_or(0);
        let page_text = |p: usize| self.pages[p].lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join("\n");
        let first: String = (0..n.min(6)).map(page_text).collect::<Vec<_>>().join("\n");
        let last = page_text(n - 1);
        s.isbn = re!(r"\bISBN(?:-1[03])?:?\s*(?:97[89][\s\-‐‑–]?)?\d[\d\s\-‐‑–]{8,15}[\dXx]\b").is_match(&format!("{first}\n{last}"));
        let first2 = format!("{}\n{}", page_text(0), if n > 1 { page_text(1) } else { String::new() });
        s.abstract_ = re!(r"(?im)^\s*(abstract|summary)\b").is_match(&first2) && !re!(r"(?i)table of contents").is_match(&first2);
        s.doi_line = re!(r"(?im)^\s*(doi:|https?://(dx\.)?doi\.org/)").is_match(&first2);
        s.received = re!(r"(?i)\breceived:?\s+\d{1,2}\s+\w+\s+\d{4}|\baccepted:?\s+\d{1,2}\s+\w+\s+\d{4}|\breceived\b[^.\n]{0,40}\baccepted\b")
            .is_match(&first2);
        s.thesis_words = re!(r"(?i)\b(doctoral|phd|master'?s|licentiate)\s+(thesis|dissertation)\b|\bdissertation\b|\bthesis\b[^.\n]{0,60}\b(submitted|presented)\b|\bin partial fulfil+ment\b")
            .is_match(&first);
        for pno in 0..n.min(20) {
            let ls = self.top_down(pno);
            if ls.is_empty() {
                continue;
            }
            let lines = &self.pages[pno].lines;
            let top: Vec<&str> = ls.iter().take(3).map(|&i| lines[i].text.trim()).collect();
            if top.iter().any(|t| re!(r"(?i)^(table of )?contents$").is_match(t)) {
                s.contents_page = true;
            }
            if top.iter().any(|t| re!(r"(?i)^(preface|foreword)$").is_match(t)) {
                s.preface = true;
            }
            if ls.iter().filter(|&&i| is_toc_line(&lines[i].text)).count() >= 3 {
                s.toc_pages += 1;
            }
        }
        for pno in n.min(20).max(n.saturating_sub(30))..n {
            let ls = self.top_down(pno);
            if ls.iter().take(2).any(|&i| re!(r"(?i)^(subject |general )?index$").is_match(self.pages[pno].lines[i].text.trim())) {
                s.index_page = true;
                break;
            }
        }
        let mut numbered = 0;
        for pno in 0..n {
            let ls = self.top_down(pno);
            if ls.is_empty() {
                continue;
            }
            let lines = &self.pages[pno].lines;
            if ls.iter().take(2).any(|&i| lines[i].text.trim().to_lowercase().starts_with("chapter") && chapter_label(&lines[i].text).is_some()) {
                s.chapter_pages += 1;
            }
            let h = self.pages[pno].height;
            let top = self.margin_line(pno, true).filter(|(ix, t)| lines[ix[0]].bbox.y0 < 0.2 * h && head_number(t).is_some());
            let bottom = self.margin_line(pno, false).filter(|(ix, t)| lines[ix[0]].bbox.y1 > 0.85 * h && head_number(t).is_some());
            if top.is_some() || bottom.is_some() {
                numbered += 1;
            }
        }
        s.numbered_heads = numbered as f64 / n.max(1) as f64;
        s
    }

    /// Decides paper or book and in book mode leaves out the running heads and the table of
    /// contents and finds the index.
    pub fn decide_kind(&mut self) {
        let (kind, reasons) = classify(&self.kind_signals());
        (self.log)(&format!("Document kind: {} ({})", kind.as_str(), reasons.join("; ")));
        self.book.kind = kind;
        self.book.reasons = reasons;
        if !kind.is_book_like() {
            self.book.outline.clear();
            return;
        }
        let mut drop: HashSet<(usize, usize)> = HashSet::new();
        self.find_book_running(&mut drop);
        self.find_toc_pages(&mut drop);
        for (pno, page) in self.pages.iter_mut().enumerate() {
            let mut i = 0;
            page.lines.retain(|_| {
                let keep = !drop.contains(&(pno, i));
                i += 1;
                keep
            });
        }
        self.find_index();
    }

    /// Running heads and feet of a book: the top line (or the bottom line) of a page when it
    /// carries the printed page number where the body pages carry it (page index minus a fixed
    /// offset), or when the same words stand there on a page close by. Linear in the pages.
    fn find_book_running(&mut self, drop: &mut HashSet<(usize, usize)>) {
        let n = self.pages.len();
        let mut cands: Vec<Vec<(Vec<usize>, String)>> = vec![Vec::new(); n];
        for (pno, c) in cands.iter_mut().enumerate() {
            let h = self.pages[pno].height;
            let lines = &self.pages[pno].lines;
            if let Some(t) = self.margin_line(pno, true).filter(|(ix, _)| lines[ix[0]].bbox.y0 < 0.2 * h) {
                c.push(t);
            }
            if self.top_down(pno).len() > 1 {
                if let Some(b) = self.margin_line(pno, false).filter(|(ix, _)| lines[ix[0]].bbox.y1 > 0.8 * h) {
                    if c.first().map(|t| t.0 != b.0).unwrap_or(true) {
                        c.push(b);
                    }
                }
            }
        }
        let mut offsets: HashMap<i64, usize> = HashMap::new();
        for (pno, c) in cands.iter().enumerate() {
            for (_, t) in c {
                if let Some(k) = head_number(t) {
                    *offsets.entry(pno as i64 + 1 - k).or_insert(0) += 1;
                }
            }
        }
        let best = offsets.iter().max_by_key(|(k, v)| (**v, -**k)).map(|(k, v)| (*k, *v));
        let offset = best.and_then(|(k, v)| (v >= 4.max(n / 10)).then_some(k));
        let keys: Vec<HashSet<String>> =
            cands.iter().map(|c| c.iter().map(|(_, t)| Self::head_key(t)).filter(|k| !k.is_empty()).collect()).collect();
        let mut count = 0;
        for (pno, cand) in cands.iter().enumerate() {
            for (ix, t) in cand {
                let lines = &self.pages[pno].lines;
                // (a chapter's title and its "Chapter 3" label stand there too, in larger type)
                if t.chars().count() > 150 || ix.iter().any(|&i| lines[i].size > self.body_size + 1.0) || chapter_label(t).is_some() {
                    continue;
                }
                let mut running = offset.is_some() && head_number(t).map(|k| pno as i64 + 1 - k) == offset;
                if !running {
                    let key = Self::head_key(t);
                    if !key.is_empty() {
                        running = [-4i64, -3, -2, -1, 1, 2, 3, 4].iter().any(|d| {
                            let q = pno as i64 + d;
                            q >= 0 && (q as usize) < n && keys[q as usize].contains(&key)
                        });
                    }
                }
                if running {
                    count += 1;
                    for &i in ix {
                        drop.insert((pno, i));
                    }
                    if std::env::var_os("P2E_DEBUG").is_some() {
                        println!("running p{}: {t}", pno + 1);
                    }
                }
            }
        }
        (self.log)(&format!(
            "Book: {count} running heads and feet{}",
            offset.map(|o| format!(", printed page = page − {o}")).unwrap_or_default()
        ));
    }

    /// The printed table of contents (and lists of figures and tables): the outline's "Contents"
    /// entries, and pages near the front with lines in dot leaders and page numbers, and the
    /// pages that go on with them.
    fn find_toc_pages(&mut self, drop: &mut HashSet<(usize, usize)>) {
        let n = self.pages.len();
        let mut toc: HashSet<usize> = HashSet::new();
        let o = &self.book.outline;
        for (k, e) in o.iter().enumerate() {
            if is_contents_title(&e.title) {
                let end = o[k + 1..].iter().find(|x| x.page > e.page).map(|x| x.page).unwrap_or(e.page + 1);
                if end - e.page <= 40.max(n / 10) {
                    toc.extend(e.page..end);
                }
            }
        }
        let mut prev = false;
        for pno in 0..n.min(40.max(n / 8)) {
            let lines = &self.pages[pno].lines;
            let ls: Vec<usize> = self.top_down(pno).into_iter().filter(|i| !drop.contains(&(pno, *i))).collect();
            if ls.len() < 3 {
                prev = false;
                continue;
            }
            let leaders = ls.iter().filter(|&&i| is_toc_line(&lines[i].text)).count();
            let numbered = ls.iter().filter(|&&i| ends_in_number(&lines[i].text)).count() as f64;
            let headed = ls.iter().take(3).any(|&i| is_contents_title(&lines[i].text));
            let is_toc = (leaders >= 3 && numbered >= 0.4 * ls.len() as f64)
                || ((prev || headed) && ls.len() >= 4 && numbered >= 0.5 * ls.len() as f64);
            if is_toc {
                toc.insert(pno);
            }
            prev = is_toc || toc.contains(&pno);
        }
        for &pno in &toc {
            for i in 0..self.pages[pno].lines.len() {
                drop.insert((pno, i));
            }
        }
        if !toc.is_empty() {
            let mut v: Vec<usize> = toc.into_iter().collect();
            v.sort();
            (self.log)(&format!(
                "Book: table of contents on page(s) {} left out",
                v.iter().map(|p| (p + 1).to_string()).collect::<Vec<_>>().join(", ")
            ));
        }
    }

    /// The index: the outline's "Index" entries (to the next entry at their level), or a page
    /// headed "Index" among the last pages to the end.
    fn find_index(&mut self) {
        let n = self.pages.len();
        let o = &self.book.outline;
        let mut ranges = Vec::new();
        for (k, e) in o.iter().enumerate() {
            if is_index_title(&e.title) {
                let end = o[k + 1..].iter().find(|x| x.level <= e.level && x.page > e.page).map(|x| x.page).unwrap_or(n);
                ranges.push(e.page..end.max(e.page + 1));
            }
        }
        if ranges.is_empty() {
            for pno in n.saturating_sub(30.max(n / 6))..n {
                let lines = &self.pages[pno].lines;
                if self.top_down(pno).iter().take(3).any(|&i| is_index_title(&lines[i].text)) {
                    ranges.push(pno..n);
                    break;
                }
            }
        }
        for r in &ranges {
            (self.log)(&format!("Book: index on pages {}–{}", r.start + 1, r.end));
        }
        self.book.index_pages = ranges;
    }
}

// MARK: - Assembly

/// The chapters of a book from its outline: each entry placed where its title is printed, the
/// title as printed ("1.1 Supervised learning", "3 Shallow neural networks"), and the paragraphs
/// that print it (or its "Chapter 3" label), which the entry stands for.
pub struct BookPlan {
    pub entries: Vec<OutlineEntry>,
    pub titles: Vec<String>,
    pub consumed: HashSet<usize>,
}

/// Whether paragraph `p` lies before outline entry `e` in reading order.
pub fn before(p: &Para, e: &OutlineEntry) -> bool {
    p.page < e.page || (p.page == e.page && p.bbox().y1 < e.y + 2.0)
}

fn numbered_title(t: &str) -> Option<String> {
    re!(r"^((?:\d{1,3}|[A-Z])(?:\.\d{1,3})*)\.?\s+(\S.*)$").captures(t).map(|c| c[2].to_string())
}

/// A title as text to compare: compatibility forms ("coeﬀicients"), the number before it left out.
fn title_key(t: &str) -> String {
    use icu_normalizer::ComposingNormalizerBorrowed;
    let s = ComposingNormalizerBorrowed::new_nfkc().normalize(t.trim()).into_owned();
    let s = numbered_title(&s).unwrap_or(s);
    s.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect()
}

/// The same title, allowing a letter or two in long ones ("Acknowledgments" for "Acknowledgements").
pub fn same_title(text: &str, title: &str) -> bool {
    let (a, b): (Vec<char>, Vec<char>) = (title_key(text).chars().collect(), title_key(title).chars().collect());
    if a.is_empty() || b.is_empty() || a.len().abs_diff(b.len()) > 2 {
        return false;
    }
    if a == b {
        return true;
    }
    if b.len() < 8 {
        return false;
    }
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut prev = row[0];
        row[0] = i;
        for j in 1..=b.len() {
            let cur = row[j];
            row[j] = (row[j] + 1).min(row[j - 1] + 1).min(prev + usize::from(a[i - 1] != b[j - 1]));
            prev = cur;
        }
    }
    row[b.len()] <= if b.len() >= 12 { 2 } else { 1 }
}

pub fn plan(paras: &[Para], outline: &[OutlineEntry]) -> BookPlan {
    let mut by_page: HashMap<usize, Vec<usize>> = HashMap::new();
    for (i, p) in paras.iter().enumerate() {
        by_page.entry(p.page).or_default().push(i);
    }
    let printed = |i: usize, title: &str| {
        let p = &paras[i];
        let t = p.text();
        matches!(p.kind, Kind::Heading | Kind::Text | Kind::Title | Kind::Front) && t.trim().chars().count() < 200 && same_title(&t, title)
    };
    // each entry to where its title is printed: on its page (nearest the point it gives), or for
    // a chapter within the next eight pages (hyperref's \chapter* bookmarks point a page early)
    let mut outline = outline.to_vec();
    for e in outline.iter_mut() {
        let last = if e.level == 1 { e.page + 8 } else { e.page + 1 };
        for pg in e.page..=last {
            let hits: Vec<usize> = by_page.get(&pg).map(|v| v.iter().copied().filter(|&i| printed(i, &e.title)).collect()).unwrap_or_default();
            if hits.is_empty() {
                continue;
            }
            let i = if pg == e.page {
                *hits
                    .iter()
                    .min_by(|&&a, &&b| {
                        (paras[a].bbox().y0 - e.y).abs().partial_cmp(&(paras[b].bbox().y0 - e.y).abs()).unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .unwrap()
            } else {
                hits[0]
            };
            let y0 = paras[i].bbox().y0 - 1.0;
            e.y = (if pg == e.page && e.y > 0.0 { e.y.min(y0) } else { y0 }).max(0.0);
            e.page = pg;
            break;
        }
    }
    let mut ord: Vec<usize> = (0..outline.len()).collect();
    ord.sort_by(|&a, &b| {
        (outline[a].page, outline[a].y).partial_cmp(&(outline[b].page, outline[b].y)).unwrap_or(std::cmp::Ordering::Equal).then(a.cmp(&b))
    });
    let outline: Vec<OutlineEntry> = ord.into_iter().map(|i| outline[i].clone()).collect();
    let mut titles: Vec<String> = outline.iter().map(|e| e.title.clone()).collect();
    let mut consumed = HashSet::new();
    let mut k = 0;
    for (ei, e) in outline.iter().enumerate() {
        while k < paras.len() && before(&paras[k], e) {
            k += 1;
        }
        let mut label: Option<String> = None;
        let mut j = k;
        while j < paras.len() && j < k + 8 && paras[j].page <= e.page + 1 {
            if ei + 1 < outline.len() && !before(&paras[j], &outline[ei + 1]) && j > k {
                break;
            }
            let p = &paras[j];
            let text = p.text();
            let t = text.trim();
            if !matches!(p.kind, Kind::Heading | Kind::Text | Kind::Title | Kind::Front) || t.chars().count() >= 200 {
                j += 1;
                continue;
            }
            if let Some(l) = chapter_label(t) {
                label = Some(l);
                consumed.insert(j);
                j += 1;
                continue;
            }
            if numbered_title(t).is_some() && same_title(t, &e.title) {
                use icu_normalizer::ComposingNormalizerBorrowed;
                titles[ei] = ComposingNormalizerBorrowed::new_nfkc().normalize(t).into_owned();
                consumed.insert(j);
                break;
            }
            if same_title(t, &e.title) {
                if let Some(l) = &label {
                    titles[ei] = format!("{l} {}", e.title);
                }
                consumed.insert(j);
                break;
            }
            j += 1;
        }
    }
    BookPlan { entries: outline, titles, consumed }
}

/// The authors of a book from its title page: the short line of a name or two under the title.
pub fn title_page_authors(paras: &[Para]) -> Vec<String> {
    let Some(ti) = paras.iter().position(|p| p.kind == Kind::Title) else { return Vec::new() };
    for p in paras[ti + 1..].iter().take(3).filter(|p| p.page == paras[ti].page) {
        let text = p.text();
        let t = text.trim();
        let name = r"(?:\p{Lu}[\p{L}.'’-]*\.?\s+){1,4}\p{Lu}[\p{L}'’-]+";
        let full = format!(r"^{name}(?:\s*(?:,|and|&)\s*{name})*$");
        static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        let re = RE.get_or_init(|| regex::Regex::new(&full).unwrap());
        if re.is_match(t) {
            return re!(r",|\band\b|&").split(t).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        }
    }
    Vec::new()
}
