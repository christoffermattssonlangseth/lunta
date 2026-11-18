//! The reference a citation in a PDF points to, for the citation card in the PDF views.
//!
//! The apps read the PDF's lines (PDFKit on the Mac and iPhone, PDFium on Windows and Linux) and
//! hand them here with what was clicked:
//! - a link into the reference list: [`reference_at`] takes the entry at the link's destination,
//!   from the line there down to where the next entry starts (the next number, a line back at the
//!   hanging indent, an author after a line that ends an entry), on into the next column or page
//!   when the entry is broken there;
//! - a citation without a link ("[12]", "cells¹²", "(Smith et al., 2020)", "Smith and Jones
//!   (2019)"): [`citation_at`] reads it from the clicked line and [`reference_for_citation`] finds
//!   its entry after the reference list's heading (by number, or by first author and year).
//!
//! [`resolve_reference`] then finds the reference's DOI: printed in it, else by Crossref's
//! bibliographic search, taken only when the title found is in the reference and the years agree.
//!
//! Coordinates are page coordinates from the top left (y down), in PDF points.

use crate::metadata;
use crate::model::Result;
use regex::Regex;
use std::sync::OnceLock;

macro_rules! re {
    ($p:expr) => {{
        static R: OnceLock<Regex> = OnceLock::new();
        R.get_or_init(|| Regex::new($p).unwrap())
    }};
}

/// One line of text on a PDF page, from the top left (y down), in points.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct TextLine {
    /// page index, from 0
    pub page: u32,
    pub text: String,
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

impl TextLine {
    fn h(&self) -> f64 {
        (self.bottom - self.top).max(1.0)
    }
    fn mid(&self) -> f64 {
        (self.top + self.bottom) / 2.0
    }
}

/// What a reference is: its DOI or arXiv number, and the title Crossref has for it (when it was
/// looked up).
#[derive(Debug, Clone, Default, PartialEq, uniffi::Record)]
pub struct ReferenceIds {
    pub doi: String,
    pub arxiv: String,
    pub title: String,
}

/// A citation in the text.
#[derive(Debug, Clone, PartialEq)]
pub enum Citation {
    /// reference numbers, the clicked one first
    Numbers(Vec<u32>),
    /// first author's family name and the year ("2020", "2020a")
    AuthorYear { author: String, year: String },
}

// ---- recognising entries ----

/// The number an entry starts with: "12.", "[12]", "(12)", "12)", "12 Smith" (as refs.rs, but
/// not "1216." – the end of a page range carried over to the next line).
fn label(t: &str) -> Option<u32> {
    let c = re!(r"^\s*(?:\[(\d{1,4})\]|\((\d{1,3})\)\s|(\d{1,3})[.)](?:\s|$|\p{Lu}\p{Ll})|(\d{1,3})\s+\p{Lu}[\p{L}.])").captures(t)?;
    (1..=4).find_map(|i| c.get(i)).and_then(|m| m.as_str().parse().ok())
}

/// A line that is only a label ("12.", "[12]"): the entry's text is to its right.
fn bare_label(t: &str) -> bool {
    re!(r"^\s*(\[\d{1,4}\]|\(\d{1,3}\)|\d{1,3}\.?)\s*$").is_match(t)
}

/// Starts with an author: "Smith JA,", "Smith, J. A.,", "J. A. Smith," (as refs.rs).
fn author_start(t: &str) -> bool {
    re!(concat!(
        r"^\s*(?:(?:van|von|de|der|den|di|da|du|del|la|le|dos|das|ten|ter|el|al)\s+)*\p{Lu}[\p{L}'’\-]+(?:[\s\-]\p{Lu}[\p{L}'’\-]+)?",
        r"(?:,\s*(?:\p{Lu}\.\s?-?){1,3}|,?\s+\p{Lu}{1,3}(?:[,.:]|\s+(?:and|&)\s)|,?\s+(?:\p{Lu}\.\s?-?){1,3}[,\s])",
        r"|^\s*(?:\p{Lu}\.\s?-?){1,3}\s*\p{Lu}[\p{L}'’\-]+(?:\s\p{Lu}[\p{L}'’\-]+)?,"
    ))
    .is_match(t)
}

fn entry_start(t: &str) -> bool {
    label(t).is_some() || author_start(t)
}

/// A reference-list heading ("References", "6 REFERENCES", "Literature Cited", …).
fn is_heading(t: &str) -> bool {
    let s = re!(r"^[^\p{L}\p{N}]+").replace(t.trim(), "");
    let s = re!(r"^(\d+(\.\d+)*\.?|[IVX]+\.)\s+").replace(&s, "");
    let s = s.trim();
    let s = if re!(r"^(\p{L} )+\p{L}$").is_match(s) { s.replace(' ', "") } else { s.to_string() };
    re!(concat!(
        r"(?i)^(references?(\s+and\s+notes)?|references\s+cited|cited\s+references|reference\s+list|literature\s+cited|",
        r"bibliography|cited\s+literature|works\s+cited|literatur|literaturverzeichnis|références|referencias|referenser)\s*[:.]?$"
    ))
    .is_match(&s)
}

/// How much a piece of text looks like a reference (as refs.rs's score): a year, a year in
/// brackets or before volume and pages, volume and pages, a DOI or web address, "et al.",
/// initials; less for running text.
fn score(t: &str) -> i32 {
    let mut s = 0;
    if has_year(t) {
        s += 1;
    }
    if re!(r"\((1[89]|20)\d{2}[a-z]?\)|\b(1[89]|20)\d{2}[a-z]?\s*[;:]\s*\d|\b(1[89]|20)\d{2}[a-z]?\s*,\s*\d{1,4}\s*[,:(]").is_match(t) {
        s += 1;
    }
    if re!(r"\b\d{1,4}\s*(\([\dS\s–\-]+\))?\s*[:,]\s*[A-Za-z]?\d{1,6}\s*[–\-]\s*[A-Za-z]?\d{1,6}\b|\b\d{1,4}\s*\(\d{1,4}\)\s*:|\b(1[89]|20)\d{2},\s*\d{1,4},\s*[A-Za-z]?\d{1,6}\b").is_match(t) {
        s += 1;
    }
    if re!(r"(?i)\bdoi\b|https?://|www\.|\bpmid\b|\b10\.\d{4,9}/").is_match(t) {
        s += 1;
    }
    if re!(r"\bet\s+al\b").is_match(t) {
        s += 1;
    }
    let initials = re!(r"\b\p{Lu}\.\s?(\p{Lu}\.\s?)*").find_iter(t).count() + re!(r"\b\p{Lu}[\p{Ll}'’\-]+\s\p{Lu}{1,3}[,.]").find_iter(t).count();
    if initials >= 2 {
        s += 1;
    }
    let words = re!(r"\p{L}+").find_iter(t).count();
    let common = re!(r"(?i)\b(we|our|was|were|is|are|this|these|that|which|have|has|had|been|showed|shown|found|suggest|suggests|however|thus|therefore|could|would|may|might|should|also|than|there)\b")
        .find_iter(t)
        .count();
    if t.chars().count() > 1500 || (words >= 20 && common as f64 > 0.08 * words as f64) {
        s -= 2;
    }
    s
}

/// A reference, not a section heading, a caption or a table a link also goes to.
pub fn looks_like_reference(t: &str) -> bool {
    let s = score(t);
    has_year(t) && (s >= 2 || (s >= 1 && entry_start(t)))
}

fn has_year(t: &str) -> bool {
    re!(r"\b(1[89]|20)\d{2}[a-z]?\b").is_match(t)
}

/// An entry that is not finished: it ends with a comma, a hyphen, "and", or it has no year yet.
fn unfinished(t: &str) -> bool {
    let t = t.trim_end();
    t.ends_with([',', ';', ':', '-', '–', '&', '(', '/']) || re!(r"\b(and|of|in|the|for|on)$").is_match(t) || !has_year(t)
}

/// Joins an entry's lines, undoing hyphenation ("immuno-" + "logy") and keeping broken DOIs and
/// web addresses whole.
fn join(lines: &[&str]) -> String {
    let mut out = String::new();
    for raw in lines {
        let p = raw.trim();
        if p.is_empty() {
            continue;
        }
        if out.is_empty() {
            out.push_str(p);
            continue;
        }
        let last = out.rsplit(' ').next().unwrap_or("");
        let link = re!(r"^\(?(https?://|www\.|doi\.org/|10\.\d{4,9}/|doi:)").is_match(last);
        let lower = p.chars().next().is_some_and(|c| c.is_lowercase());
        if link && (last.ends_with(['/', '.', '-', '_', '=', '?', '&']) || !p.contains(' ') || p.starts_with(['/', '.'])) && !p.starts_with(['(', '[']) {
            out.push_str(p);
        } else if re!(r"\p{L}-$").is_match(&out) && lower {
            out.pop();
            out.push_str(p);
        } else if out.ends_with(['-', '–', '/']) && !out.ends_with(" -") {
            out.push_str(p);
        } else {
            out.push(' ');
            out.push_str(p);
        }
    }
    out
}

fn median(mut v: Vec<f64>) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    Some(v[v.len() / 2])
}

/// The usual line height on a page.
fn line_height(lines: &[TextLine], page: u32) -> f64 {
    median(lines.iter().filter(|l| l.page == page && !l.text.trim().is_empty()).map(|l| l.h()).collect()).unwrap_or(10.0).clamp(4.0, 30.0)
}

/// The entry that starts at line `start`: its lines down the column, and on in the next column or
/// on the next page when it is cut off there.
fn gather(lines: &[TextLine], start: usize) -> String {
    let mut s = &lines[start];
    let h = line_height(lines, s.page);
    let mut prefix = String::new();
    // a label on its own ("[12]" in a margin column): the entry's text is the line to its right
    if bare_label(&s.text) {
        if let Some(r) = lines
            .iter()
            .filter(|l| l.page == s.page && l.left >= s.right - 1.0 && (l.mid() - s.mid()).abs() < 0.5 * h && !bare_label(&l.text))
            .min_by(|a, b| a.left.total_cmp(&b.left))
        {
            prefix = s.text.trim().to_string();
            s = r;
        }
    }
    let number = label(&format!("{prefix} {}", s.text)).or_else(|| label(&s.text));
    let mut parts: Vec<&str> = vec![&s.text];
    let in_band = |l: &TextLine, left: f64| l.left >= left - h && l.left <= left + 4.0 * h;

    // down the column
    let mut below: Vec<&TextLine> = lines
        .iter()
        .filter(|l| l.page == s.page && l.top > s.top + 0.4 * h && in_band(l, s.left) && l.left < s.right && !l.text.trim().is_empty())
        .collect();
    below.sort_by(|a, b| a.top.total_cmp(&b.top));
    let mut prev = s;
    let mut hanging: Option<bool> = None;
    let mut right = s.right;
    let mut ran_out = true;
    for l in below {
        // a gap: the foot of the column (a footnote or the page number below it) or the list's end
        if l.top - prev.bottom > 1.2 * h {
            break;
        }
        if parts.len() >= 15 {
            ran_out = false;
            break;
        }
        // a label to its left in a column of its own: the next entry
        let labelled = lines.iter().any(|o| o.page == l.page && o.right <= l.left + 1.0 && (o.mid() - l.mid()).abs() < 0.5 * h && bare_label(&o.text));
        if labelled || ends_here(l, s, prev, number, &mut hanging, right, h) {
            ran_out = false;
            break;
        }
        parts.push(&l.text);
        right = right.max(l.right);
        prev = l;
    }

    // cut off at the foot of the column (no year yet, or it ends with a comma): on at the top of the
    // next column, or of the next page
    let mut text = join(&parts);
    if ran_out && unfinished(&text) {
        let page_h = lines.iter().filter(|l| l.page == s.page).map(|l| l.bottom).fold(0.0, f64::max);
        let next_col: Vec<&TextLine> = lines.iter().filter(|l| l.page == s.page && l.left >= right - h && l.top > 0.07 * page_h).collect();
        let next: Vec<&TextLine> = if !next_col.is_empty() {
            next_col
        } else {
            let p = s.page + 1;
            let ph = lines.iter().filter(|l| l.page == p).map(|l| l.bottom).fold(0.0, f64::max);
            let min_left = lines.iter().filter(|l| l.page == p && l.top > 0.07 * ph).map(|l| l.left).fold(f64::MAX, f64::min);
            lines.iter().filter(|l| l.page == p && l.top > 0.07 * ph && l.left <= min_left + 4.0 * h).collect()
        };
        if let Some(first) = next.iter().min_by(|a, b| a.top.total_cmp(&b.top)) {
            let mut col: Vec<&TextLine> = next.iter().copied().filter(|l| in_band(l, first.left) && l.left < first.right).collect();
            col.sort_by(|a, b| a.top.total_cmp(&b.top));
            let mut more: Vec<&str> = vec![];
            let mut prev = *col.first().unwrap();
            for l in col {
                if more.len() >= 6 || entry_start(&l.text) || l.top - prev.bottom > 1.2 * h {
                    break;
                }
                more.push(&l.text);
                prev = l;
                if !unfinished(&join(&more)) && l.text.trim_end().ends_with(['.', ')', ']']) {
                    break;
                }
            }
            if !more.is_empty() {
                parts.extend(more);
                text = join(&parts);
            }
        }
    }
    if prefix.is_empty() {
        text
    } else {
        format!("{prefix} {text}")
    }
}

/// Whether line `l` starts the entry after the one that began at `s`.
fn ends_here(l: &TextLine, s: &TextLine, prev: &TextLine, number: Option<u32>, hanging: &mut Option<bool>, right: f64, h: f64) -> bool {
    let at_start = (l.left - s.left).abs() < 0.3 * h;
    if let Some(n) = number {
        if let Some(m) = label(&l.text) {
            if m == n + 1 || (m != n && l.left <= s.left + h) {
                return true;
            }
        }
        // numbered entries can still be set flush ("Smith J, …" lines with the label in a margin)
        return bare_label(&l.text);
    }
    // author–year: a hanging indent shows where entries start
    let hang = *hanging.get_or_insert(l.left > s.left + 0.4 * h);
    if hang {
        return l.left < s.left + 0.3 * h;
    }
    let p = prev.text.trim_end();
    let prev_done = p.ends_with(['.', ')', ']']) || re!(r"\d$").is_match(p) || prev.right < right - 2.0 * h;
    at_start && author_start(&l.text) && prev_done
}

/// Pieces of one line joined: PDFium breaks a line at an accent set as its own character
/// ("Gonz´" + "alez-Barahona").
fn merge_rows(lines: &[TextLine]) -> Vec<TextLine> {
    let mut out: Vec<TextLine> = Vec::with_capacity(lines.len());
    for l in lines {
        if let Some(p) = out.last_mut() {
            let h = p.h().min(l.h());
            if p.page == l.page && (p.mid() - l.mid()).abs() < 0.3 * h && l.left >= p.left && l.left - p.right < 0.5 * h {
                let glue = if p.text.ends_with(['´', '`', '¨', '˜', '^', '-']) || l.left < p.right { "" } else { " " };
                p.text = format!("{}{glue}{}", p.text.trim_end(), l.text.trim_start());
                p.right = p.right.max(l.right);
                p.top = p.top.min(l.top);
                p.bottom = p.bottom.max(l.bottom);
                continue;
            }
        }
        out.push(l.clone());
    }
    out
}

/// The reference at a link's destination: `page` (from 0), `x` (if the destination has one) and
/// `y`, in top-left page coordinates. None when there is no reference there (a link to a section,
/// a figure or a table).
pub fn reference_at(lines: &[TextLine], page: u32, x: Option<f64>, y: f64) -> Option<String> {
    let lines = &merge_rows(lines);
    let h = line_height(lines, page);
    let on: Vec<usize> = (0..lines.len()).filter(|&i| lines[i].page == page && !lines[i].text.trim().is_empty()).collect();
    // the column: lines starting near x (none when x is off, e.g. at the page edge)
    let col: Vec<usize> = match x {
        Some(x) => {
            let c: Vec<usize> = on.iter().copied().filter(|&i| lines[i].left >= x - 1.5 * h && lines[i].left <= x + 4.0 * h).collect();
            if c.is_empty() {
                on.clone()
            } else {
                c
            }
        }
        None => on.clone(),
    };
    let near = |i: &usize| {
        let m = lines[*i].mid();
        m >= y - 0.6 * h && m <= y + 1.3 * h
    };
    // the first entry start there, best one starting at x (not a line at a hanging indent); else
    // the first line at or below the destination
    let at_x = |i: usize| x.is_some_and(|x| (lines[i].left - x).abs() < h);
    let key = |i: &usize| (!(entry_start(&lines[*i].text) || bare_label(&lines[*i].text)), !at_x(*i));
    let best = |a: &usize, b: &usize| key(a).cmp(&key(b)).then(lines[*a].top.total_cmp(&lines[*b].top)).then(lines[*a].left.total_cmp(&lines[*b].left));
    let start = col
        .iter()
        .copied()
        .filter(near)
        .min_by(best)
        .or_else(|| col.iter().copied().filter(|&i| lines[i].mid() >= y - 0.6 * h && lines[i].top <= y + 3.0 * h).min_by(best))?;
    let text = gather(lines, start);
    looks_like_reference(&text).then_some(text)
}

// ---- citations in the text ----

/// `offset` in UTF-16 code units (as NSString and JavaScript count) → char index.
fn char_index(chars: &[char], offset: usize) -> usize {
    let mut u = 0;
    for (i, c) in chars.iter().enumerate() {
        u += c.len_utf16();
        if u > offset {
            return i;
        }
    }
    chars.len().saturating_sub(1)
}

/// The brackets around position `i` (open, close), if they are close enough.
fn enclosing(chars: &[char], i: usize, open: char, close: char, max: usize) -> Option<(usize, usize)> {
    let mut a = None;
    for j in (i.saturating_sub(max)..=i.min(chars.len().saturating_sub(1))).rev() {
        if chars[j] == close && j != i {
            return None;
        }
        if chars[j] == open {
            a = Some(j);
            break;
        }
    }
    let a = a?;
    let b = (i..chars.len().min(i + max)).find(|&j| chars[j] == close || (chars[j] == open && j != a))?;
    (chars[b] == close).then_some((a, b))
}

/// "3, 5–7, 12" → [3, 5, 6, 7, 12], the one at `at` (an index into `s`) first.
fn numbers(s: &str, at: Option<usize>) -> Option<Vec<u32>> {
    if !re!(r"^\s*\d{1,3}[a-z]?(\s*[,;–—\-]\s*\d{1,3}[a-z]?)*\s*$").is_match(s) {
        return None;
    }
    let mut out: Vec<u32> = vec![];
    let mut first = None;
    let chars: Vec<char> = s.chars().collect();
    for m in re!(r"(\d{1,3})[a-z]?(?:\s*[–—\-]\s*(\d{1,3})[a-z]?)?").captures_iter(s) {
        let a: u32 = m[1].parse().ok()?;
        let b: u32 = m.get(2).and_then(|x| x.as_str().parse().ok()).unwrap_or(a);
        let whole = m.get(0).unwrap();
        let (cs, ce) = (s[..whole.start()].chars().count(), s[..whole.end()].chars().count());
        if b < a || b - a > 60 {
            out.push(a);
            continue;
        }
        for n in a..=b {
            if let Some(at) = at {
                // the number clicked in a range: the start or end of it, whichever is under the click
                if at >= cs && at < ce && first.is_none() {
                    let in_b = m.get(2).is_some_and(|x| at >= s[..x.start()].chars().count());
                    first = Some(if in_b { b } else { a });
                }
            }
            out.push(n);
        }
    }
    let _ = chars;
    if out.is_empty() || out.iter().all(|&n| n == 0) {
        return None;
    }
    if let Some(f) = first {
        out.retain(|&n| n != f);
        out.insert(0, f);
    }
    out.dedup();
    Some(out)
}

const NAME: &str = r"(?:(?:van|von|de|der|den|di|da|du|del|la|le|dos|das|ten|ter)\s+)*\p{Lu}[\p{L}'’\-]+";

/// "Smith et al., 2020" / "Smith and Jones 2019" / "Smith 2019, 2020" in one part of a
/// parenthetical citation: the author and the year nearest `at`.
fn author_year(seg: &str, at: usize) -> Option<Citation> {
    static R: OnceLock<Regex> = OnceLock::new();
    let r = R.get_or_init(|| {
        Regex::new(&format!(r"({NAME})(?:\s+et\s+al\.?|\s+(?:and|&)\s+{NAME}|(?:,\s*{NAME})*,?\s*(?:and|&)\s+{NAME})?,?\s+((?:1[89]|20)\d{{2}}[a-z]?)")).unwrap()
    });
    let c = r.captures(seg)?;
    let author = c[1].to_string();
    // a later year of the same author ("Smith 2019, 2020"): the one clicked
    let mut year = c[2].to_string();
    for y in re!(r"\b(?:1[89]|20)\d{2}[a-z]?\b").find_iter(&seg[c.get(2).unwrap().start()..]) {
        let start = seg[..c.get(2).unwrap().start() + y.start()].chars().count();
        if at + 1 >= start {
            year = y.as_str().to_string();
        }
    }
    Some(Citation::AuthorYear { author, year })
}

/// The citation at `offset` (UTF-16) in a line of text, if there is one: "[12]", "[3, 5–7]",
/// "(12)", a superscript run glued to a word ("cells12,13"), "(Smith et al., 2020; Jones 2019)",
/// "Smith and Jones (2019)".
pub fn citation_at(text: &str, offset: usize) -> Option<Citation> {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return None;
    }
    let i = char_index(&chars, offset);
    let slice = |a: usize, b: usize| chars[a..b].iter().collect::<String>();
    // [12], [3, 5–7]
    if let Some((a, b)) = enclosing(&chars, i, '[', ']', 80) {
        if let Some(n) = numbers(&slice(a + 1, b), i.checked_sub(a + 1)) {
            return Some(Citation::Numbers(n));
        }
    }
    // narrative: Smith et al. (2020), Smith and Jones (2019)
    static NARR: OnceLock<Regex> = OnceLock::new();
    let narr = NARR.get_or_init(|| Regex::new(&format!(r"({NAME})(?:\s+et\s+al\.?|\s+(?:and|&)\s+{NAME})?\s*\(((?:1[89]|20)\d{{2}}[a-z]?)[^)]{{0,20}}\)")).unwrap());
    for c in narr.captures_iter(text) {
        let m = c.get(0).unwrap();
        let (s, e) = (text[..m.start()].chars().count(), text[..m.end()].chars().count());
        if i >= s && i < e {
            return Some(Citation::AuthorYear { author: c[1].to_string(), year: c[2].to_string() });
        }
    }
    // (12), (Smith et al., 2020; Jones 2019)
    if let Some((a, b)) = enclosing(&chars, i, '(', ')', 250) {
        let inner = slice(a + 1, b);
        if let Some(n) = numbers(&inner, i.checked_sub(a + 1)) {
            return Some(Citation::Numbers(n));
        }
        // the part of "A 2020; B 2019" that was clicked
        let at = i.saturating_sub(a + 1);
        let mut pos = 0;
        for seg in inner.split(';') {
            let n = seg.chars().count();
            if at <= pos + n {
                let local = at.saturating_sub(pos);
                if let Some(c) = author_year(seg, local) {
                    return Some(c);
                }
                break;
            }
            pos += n + 1;
        }
    }
    // superscript numbers glued to the word or punctuation before them: "cells12", "disease.3,4"
    let is_part = |c: char| c.is_ascii_digit() || ",–—-".contains(c);
    if chars[i].is_ascii_digit() || (is_part(chars[i]) && i > 0 && chars[i - 1].is_ascii_digit()) {
        let mut a = i;
        while a > 0 && is_part(chars[a - 1]) {
            a -= 1;
        }
        let mut b = i + 1;
        while b < chars.len() && is_part(chars[b]) {
            b += 1;
        }
        while a < b && !chars[a].is_ascii_digit() {
            a += 1;
        }
        while b > a && !chars[b - 1].is_ascii_digit() {
            b -= 1;
        }
        let before = a.checked_sub(1).map(|j| chars[j]);
        let after = chars.get(b).copied();
        let glued = before.is_some_and(|c| c.is_alphabetic() || ".,;:)]’”\"".contains(c));
        let ends = after.is_none_or(|c| !c.is_alphanumeric() && c != '.' || c == '.' && chars.get(b + 1).is_none_or(|n| n.is_whitespace()));
        // "2D", "H2O", "COVID-19", "IL-6" are not citations
        let word_before: String = chars[..a].iter().rev().take_while(|c| c.is_alphanumeric()).collect();
        let formula = before.is_some_and(|c| c.is_uppercase()) && word_before.chars().count() <= 3;
        if glued && ends && !formula && a < b {
            return numbers(&slice(a, b), Some(i.saturating_sub(a))).map(Citation::Numbers);
        }
    }
    None
}

/// Whether there is a citation at `offset` (UTF-16) in `text`.
pub fn is_citation_at(text: &str, offset: usize) -> bool {
    citation_at(text, offset).is_some()
}

fn fold(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'ö' | 'õ' | 'ø' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            'ñ' => 'n',
            '’' => '\'',
            c => c,
        })
        .collect::<String>()
        .to_lowercase()
}

/// The entry a citation in the text refers to, from the PDF's lines (all pages, in reading
/// order): after the last reference-list heading (or anywhere, without one), the entry with the
/// cited number, or the first whose first author and year match.
pub fn reference_for_citation(lines: &[TextLine], text: &str, offset: usize) -> Option<String> {
    let cite = citation_at(text, offset)?;
    let lines = &merge_rows(lines);
    let from = lines.iter().rposition(|l| is_heading(&l.text)).map(|i| i + 1).unwrap_or(0);
    match cite {
        Citation::Numbers(ns) => {
            for n in ns.iter().take(1) {
                let hit = (from..lines.len()).find(|&i| label(&lines[i].text) == Some(*n) || bare_label(&lines[i].text) && digits(&lines[i].text) == Some(*n));
                // without a heading: the last such line that reads like a reference
                let hit = hit.filter(|_| from > 0).or_else(|| {
                    (0..lines.len()).rev().find(|&i| label(&lines[i].text) == Some(*n) && has_year(&gather(lines, i)))
                });
                if let Some(i) = hit {
                    return Some(gather(lines, i));
                }
            }
            None
        }
        Citation::AuthorYear { author, year } => {
            let who = fold(&author);
            let plain_year: String = year.chars().filter(|c| c.is_ascii_digit()).collect();
            let mut fallback = None;
            for i in from..lines.len() {
                let t = fold(re!(r"^\s*(\[\d+\]|\d+\.)\s*").replace(&lines[i].text, "").as_ref());
                let starts = t.starts_with(&who) || (author_start(&lines[i].text) && t.chars().take(40).collect::<String>().contains(&who));
                if !starts {
                    continue;
                }
                let e = gather(lines, i);
                if e.contains(&year) {
                    return Some(e);
                }
                if fallback.is_none() && e.contains(&plain_year) {
                    fallback = Some(e);
                }
            }
            fallback
        }
    }
}

fn digits(t: &str) -> Option<u32> {
    t.chars().filter(|c| c.is_ascii_digit()).collect::<String>().parse().ok()
}

// ---- the DOI ----

const STOP: &[&str] = &["the", "and", "for", "with", "from", "into", "that", "this", "are", "its", "their", "via", "between", "among", "after", "during"];

fn words(s: &str) -> Vec<String> {
    re!(r"[\p{L}\p{N}]+").find_iter(&fold(s)).map(|m| m.as_str().to_string()).collect()
}

/// Whether a title Crossref found is the one in the reference: nearly all its words are there.
pub fn title_in(reference: &str, title: &str) -> bool {
    let have: std::collections::HashSet<String> = words(&reference.replace("-\n", "")).into_iter().collect();
    let want: Vec<String> = words(title).into_iter().filter(|w| w.chars().count() >= 3 && !STOP.contains(&w.as_str())).collect();
    if want.is_empty() {
        return false;
    }
    // the reference's letters run together too: a PDF's text can split words ("foundati on",
    // "pa thology", "Nat ure"), and a title word is then still in it
    let joined: String = words(&reference.replace("-\n", "")).concat();
    let found = want.iter().filter(|w| have.contains(*w) || (w.chars().count() >= 4 && joined.contains(w.as_str()))).count();
    // short titles must be all there; long ones may differ in a word or two (hyphens, symbols)
    if want.len() <= 3 {
        found == want.len()
    } else {
        found as f64 >= 0.8 * want.len() as f64
    }
}

fn years(t: &str) -> Vec<i32> {
    re!(r"\b((?:1[89]|20)\d{2})[a-z]?\b").captures_iter(t).filter_map(|c| c[1].parse().ok()).collect()
}

/// The reference's DOI or arXiv number: printed in it, else Crossref's best match for it when
/// that is plainly the same work (its title is in the reference, the years agree). Ok(None) when
/// nothing was found; Err only when Crossref could not be asked.
pub fn resolve_reference(text: &str) -> Result<Option<ReferenceIds>> {
    let t = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if let Some(d) = metadata::doi_in(&t) {
        return Ok(Some(ReferenceIds { doi: d, ..Default::default() }));
    }
    if let Some(c) = re!(r"(?i)arxiv(?:\.org/(?:abs|pdf)/|:\s*|\s+)(\d{4}\.\d{4,5})").captures(&t) {
        return Ok(Some(ReferenceIds { arxiv: c[1].to_string(), ..Default::default() }));
    }
    let q = re!(r"^\s*(\[\d+\]|\(\d+\)|\d+\.)\s*").replace(&t, "");
    let q: String = q.chars().take(400).collect();
    if words(&q).len() < 4 {
        return Ok(None);
    }
    let url = format!("https://api.crossref.org/works?rows=3&select=DOI,title,issued&query.bibliographic={}", metadata::enc(&q));
    let j = metadata::get_json(&url)?;
    let ref_years = years(&q);
    for it in j["message"]["items"].as_array().into_iter().flatten() {
        let title = it["title"].as_array().and_then(|a| a.first()).and_then(|v| v.as_str()).unwrap_or("");
        let title = metadata::strip_tags(title);
        let doi = it["DOI"].as_str().unwrap_or("");
        if doi.is_empty() || !title_in(&q, &title) {
            continue;
        }
        let year = it["issued"]["date-parts"][0][0].as_i64().map(|y| y as i32);
        if let Some(y) = year {
            if !ref_years.is_empty() && !ref_years.iter().any(|r| (r - y).abs() <= 1) {
                continue;
            }
        }
        return Ok(Some(ReferenceIds { doi: doi.to_string(), arxiv: String::new(), title }));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(page: u32, text: &str, left: f64, top: f64) -> TextLine {
        TextLine { page, text: text.into(), left, top, right: left + 230.0, bottom: top + 9.0 }
    }

    /// A two-column numbered reference list: entries 1–4 in the left column, 4 broken into the
    /// right column, then 5.
    fn numbered() -> Vec<TextLine> {
        vec![
            line(7, "References", 50.0, 80.0),
            line(7, "1. Hanahan D, Weinberg RA. Hallmarks of cancer: the next", 50.0, 100.0),
            line(7, "generation. Cell. 2011;144:646–674.", 62.0, 110.0),
            line(7, "2. Smith J, Jones K. A study of immuno-", 50.0, 121.0),
            line(7, "logy in mice. Nature. 2019;570:1–5. https://doi.org/10.", 62.0, 131.0),
            line(7, "1038/s41586-019-1234-5", 62.0, 141.0),
            line(7, "3. Doe A. Short. Science. 2020;1:2.", 50.0, 152.0),
            line(7, "4. Long R, Wide S, Tall T, Narrow N, Deep D,", 50.0, 700.0),
            line(7, "Shallow S. Cut across columns and", 62.0, 710.0),
            line(7, "pages. Cell Rep. 2021;5:", 320.0, 100.0),
            line(7, "100–110.", 320.0, 110.0),
            line(7, "5. Next N. The next one. 2022.", 308.0, 121.0),
        ]
    }

    #[test]
    fn entry_at_a_numbered_destination() {
        let l = numbered();
        // destination at the top of entry 2, at the column's left
        assert_eq!(
            reference_at(&l, 7, Some(50.0), 120.0).unwrap(),
            "2. Smith J, Jones K. A study of immunology in mice. Nature. 2019;570:1–5. https://doi.org/10.1038/s41586-019-1234-5"
        );
        // at the baseline of its first line (some publishers), or without an x
        assert!(reference_at(&l, 7, Some(50.0), 129.0).unwrap().starts_with("2. Smith"));
        assert!(reference_at(&l, 7, None, 99.0).unwrap().starts_with("1. Hanahan"));
        assert_eq!(reference_at(&l, 7, Some(50.0), 151.0).unwrap(), "3. Doe A. Short. Science. 2020;1:2.");
    }

    #[test]
    fn entry_cut_at_the_foot_of_a_column_goes_on_in_the_next() {
        let l = numbered();
        assert_eq!(
            reference_at(&l, 7, Some(50.0), 699.0).unwrap(),
            "4. Long R, Wide S, Tall T, Narrow N, Deep D, Shallow S. Cut across columns and pages. Cell Rep. 2021;5: 100–110."
        );
    }

    #[test]
    fn author_year_entries_by_hanging_indent_and_flush() {
        let hang = vec![
            line(3, "Adams, J. A., & Brown, K. (2018). A first title that", 72.0, 100.0),
            line(3, "wraps. Journal, 1, 1–10.", 84.0, 110.0),
            line(3, "Brown, K. (2019). Second title. Journal, 2, 3–4.", 72.0, 121.0),
            line(3, "Clark, L. (2020). Third.", 72.0, 132.0),
        ];
        assert_eq!(reference_at(&hang, 3, Some(72.0), 100.0).unwrap(), "Adams, J. A., & Brown, K. (2018). A first title that wraps. Journal, 1, 1–10.");
        assert_eq!(reference_at(&hang, 3, Some(72.0), 121.0).unwrap(), "Brown, K. (2019). Second title. Journal, 2, 3–4.");
        let flush = vec![
            line(3, "Adams JA, Brown K. A first title that wraps over", 72.0, 100.0),
            line(3, "two lines. Journal 2018;1:1–10.", 72.0, 110.0),
            line(3, "Brown K. Second title. Journal 2019;2:3–4.", 72.0, 121.0),
        ];
        assert_eq!(reference_at(&flush, 3, Some(72.0), 100.0).unwrap(), "Adams JA, Brown K. A first title that wraps over two lines. Journal 2018;1:1–10.");
    }

    #[test]
    fn a_label_in_its_own_column() {
        let l = vec![
            TextLine { page: 0, text: "[7]".into(), left: 50.0, top: 100.0, right: 62.0, bottom: 109.0 },
            TextLine { page: 0, text: "A. Author, B. Author, Title of it, in: Proc. 2017,".into(), left: 70.0, top: 100.0, right: 300.0, bottom: 109.0 },
            TextLine { page: 0, text: "pp. 1–9.".into(), left: 70.0, top: 110.0, right: 120.0, bottom: 119.0 },
            TextLine { page: 0, text: "[8]".into(), left: 50.0, top: 121.0, right: 62.0, bottom: 130.0 },
            TextLine { page: 0, text: "C. Other, Next. 2018.".into(), left: 70.0, top: 121.0, right: 200.0, bottom: 130.0 },
        ];
        assert_eq!(reference_at(&l, 0, Some(50.0), 100.0).unwrap(), "[7] A. Author, B. Author, Title of it, in: Proc. 2017, pp. 1–9.");
    }

    #[test]
    fn citations_in_the_text() {
        use Citation::*;
        let t = "as shown before [3, 5–7] and in cells12,13 (Smith et al., 2020; Jones and Lee 2019a).";
        let at = |s: &str| t[..t.find(s).unwrap()].encode_utf16().count();
        assert_eq!(citation_at(t, at("3,")), Some(Numbers(vec![3, 5, 6, 7])));
        assert_eq!(citation_at(t, at("7]")), Some(Numbers(vec![7, 3, 5, 6])));
        assert_eq!(citation_at(t, at("13")), Some(Numbers(vec![13, 12])));
        assert_eq!(citation_at(t, at("Smith")), Some(AuthorYear { author: "Smith".into(), year: "2020".into() }));
        assert_eq!(citation_at(t, at("Lee")), Some(AuthorYear { author: "Jones".into(), year: "2019a".into() }));
        assert_eq!(citation_at(t, at("shown")), None);
        let n = "as Smith and Jones (2019) found, and Doe et al. (2021b) too";
        assert_eq!(citation_at(n, n.find("2019").unwrap()), Some(AuthorYear { author: "Smith".into(), year: "2019".into() }));
        assert_eq!(citation_at(n, n.find("Doe").unwrap()), Some(AuthorYear { author: "Doe".into(), year: "2021b".into() }));
        // not citations: numbers in the text, formulas, years alone
        for (s, w) in [("in 12 patients", "12"), ("the H2O level", "2"), ("a 3D model", "3"), ("COVID-19 cases", "19"), ("(n = 12)", "12"), ("in (2020)", "2020")] {
            assert_eq!(citation_at(s, s.find(w).unwrap()), None, "{s}");
        }
        // UTF-16 offsets: "é" and "𝛼" before the citation
        let u = "é 𝛼 [4]";
        assert_eq!(citation_at(u, 6), Some(Numbers(vec![4])));
    }

    #[test]
    fn reference_for_a_citation_without_a_link() {
        let mut l = vec![line(0, "Introduction text that cites [2] and (Brown, 2019).", 50.0, 100.0)];
        l.extend(numbered());
        let t = "Introduction text that cites [2] and (Brown, 2019).";
        assert!(reference_for_citation(&l, t, t.find("2]").unwrap()).unwrap().starts_with("2. Smith J, Jones K."));
        let ay = vec![
            line(0, "Text (Brown, 2019) here.", 72.0, 100.0),
            line(5, "References", 72.0, 80.0),
            line(5, "Brown, K. (2017). Older. Journal, 1, 1.", 72.0, 100.0),
            line(5, "Brown, K. (2019). Second title. Journal, 2, 3–4.", 72.0, 111.0),
        ];
        assert_eq!(reference_for_citation(&ay, "Text (Brown, 2019) here.", 7).unwrap(), "Brown, K. (2019). Second title. Journal, 2, 3–4.");
    }

    #[test]
    fn titles_found_by_crossref_must_be_in_the_reference() {
        let r = "Hanahan D, Weinberg RA. Hallmarks of cancer: the next generation. Cell. 2011;144:646–674.";
        assert!(title_in(r, "Hallmarks of Cancer: The Next Generation"));
        assert!(title_in("Smith J. Hallmarks of cancer. Cell 2000", "Hallmarks of Cancer"));
        assert!(!title_in(r, "The hallmarks of aging"));
    }

    #[test]
    fn a_title_is_found_in_a_reference_with_split_words() {
        // PDFKit's text of a reference list (CellBridge preprint): words split by letter spacing
        let r = "Xu, H., et al. A whole-slide foundati on model for digital pa thology f rom real -world data. Nat ure 6 30, 181-188 (2024).";
        assert!(title_in(r, "A whole-slide foundation model for digital pathology from real-world data"));
        assert!(!title_in(r, "A pathology foundation model for cancer diagnosis and prognosis prediction"));
    }

    #[test]
    fn dois_printed_in_the_reference_need_no_lookup() {
        let r = resolve_reference("Smith J. A study. Nature 2019. doi:10.1038/s41586-019-1234-5.").unwrap().unwrap();
        assert_eq!(r.doi, "10.1038/s41586-019-1234-5");
        let a = resolve_reference("Doe A. Preprint. arXiv:2101.01234, 2021").unwrap().unwrap();
        assert_eq!(a.arxiv, "2101.01234");
    }
}
