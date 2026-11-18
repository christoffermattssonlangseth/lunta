//! The reference list of a PDF: found from its heading, or, without one, from the entries
//! themselves, and cut into one entry per reference (a port of `PDFReferences.swift`; the two
//! must agree).
//!
//! A heading the layout analysis missed is found when it is followed by entries: "References" in
//! the reference type (smaller than the body, so not taken for a heading), on the first line of
//! the list's first paragraph ("References 1. Hanahan D. …"), set off by an ornament ("❚
//! REFERENCES"), numbered ("6 References") or in another language ("Literatur", "Références").
//! Without any reference heading in the paper, a run of at least six paragraphs that each start
//! like a reference ("1.", "[1]", "Smith JA,", "Smith, J. A.,", "J. A. Smith,") and look like one
//! (a year, volume and pages, a DOI or web address, "et al.", initials), numbered in order or
//! sorted by author, after the first third of the text and with almost no running text in body
//! type after it, is a reference list.
//!
//! Entries: in a numbered list each paragraph that starts with a number is an entry, a line that
//! starts with the next number (or a number at the hanging indent) starts the next one, and
//! anything else continues the current one (an entry broken across columns or pages); in an
//! author–year list each paragraph is an entry unless it plainly continues the one before, and a
//! paragraph holding several entries is cut where a line starts at the hanging indent or after a
//! short line that ends an entry.

use super::layout::{count, re, Kind, PLine, Para, Span};

/// A reference-list heading: "References", "REFERENCES", "6 References", "❚ REFERENCES",
/// "R E F E R E N C E S", "Literature Cited", "References and Notes", "Bibliography",
/// "Literatur", "Références", "Referencias".
pub fn is_heading(t: &str) -> bool {
    let s = heading_text(t);
    re!(concat!(
        r"(?i)^(references?(\s+and\s+notes)?|references\s+cited|cited\s+references|reference\s+list|list\s+of\s+references|",
        r"bibliography|literature\s+cited|cited\s+literature|works\s+cited|referenzen|literatur|literaturverzeichnis|",
        r"références(\s+bibliographiques)?|referencias(\s+bibliográficas)?|bibliografía|bibliografia|",
        r"referências(\s+bibliográficas)?|riferimenti(\s+bibliografici)?|literatuur|referenser|litteratur|referanser)\s*[:.]?$"
    ))
    .is_match(&s)
}

/// A heading's words without an ornament or a number in front and without letter spacing.
pub fn heading_text(t: &str) -> String {
    let s = re!(r"^[^\p{L}\p{N}]+").replace(t.trim(), "");
    let s = re!(r"^(\d+(\.\d+)*\.?|[IVX]+\.)\s+").replace(&s, "");
    let s = s.trim();
    if re!(r"^(\p{L} )+\p{L}$").is_match(s) {
        return s.replace(' ', "");
    }
    s.to_string()
}

/// The number an entry starts with: "12.", "12.Smith", "[12]", "(12)", "12)", "12 Smith" or "12 J. Smith".
pub fn label(t: &str) -> Option<u32> {
    let c = re!(r"^\s*(?:\[(\d{1,4})\]|\((\d{1,4})\)\s|(\d{1,4})[.)](?:\s|$|\p{Lu}\p{Ll})|(\d{1,3})\s+\p{Lu}[\p{L}.])").captures(t)?;
    (1..=4).find_map(|i| c.get(i)).and_then(|m| m.as_str().parse().ok())
}

/// Starts with an author: "Smith JA,", "Smith, J. A.,", "Smith J.A.", "van der Berg, A.",
/// "J. A. Smith,", "Smith, J. A. & Jones".
pub fn author_start(t: &str) -> bool {
    re!(concat!(
        r"^\s*(?:(?:van|von|de|der|den|di|da|du|del|la|le|dos|das|ten|ter|el|al)\s+)*\p{Lu}[\p{L}'’\-]+(?:[\s\-]\p{Lu}[\p{L}'’\-]+)?",
        r"(?:,\s*(?:\p{Lu}\.\s?-?){1,3}|,?\s+\p{Lu}{1,3}(?:[,.:]|\s+(?:and|&)\s)|,?\s+(?:\p{Lu}\.\s?-?){1,3}[,\s])",
        r"|^\s*(?:\p{Lu}\.\s?-?){1,3}\s*\p{Lu}[\p{L}'’\-]+(?:\s\p{Lu}[\p{L}'’\-]+)?,"
    ))
    .is_match(t)
}

/// Starts like a reference: a number or an author.
pub fn entry_start(t: &str) -> bool {
    label(t).is_some() || author_start(t)
}

/// How much a piece of text looks like a reference (0–6): a year, a year in brackets or before
/// volume and pages, volume and pages, a DOI or web address, "et al.", several initials; less for
/// running text (many words like "we", "was", "were").
pub fn score(t: &str) -> i32 {
    let mut s = 0;
    if re!(r"\b(1[89]|20)\d{2}[a-z]?\b").is_match(t) {
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
    let initials = re!(r"\b\p{Lu}\.\s?(\p{Lu}\.\s?)*").find_iter(t).count()
        + re!(r"\b\p{Lu}[\p{Ll}'’\-]+\s\p{Lu}{1,3}[,.]").find_iter(t).count();
    if initials >= 2 {
        s += 1;
    }
    if prose(t) {
        s -= 2;
    }
    s
}

/// Running text: many words of a sentence ("we", "was", "were", "these", "however") or very long.
pub fn prose(t: &str) -> bool {
    let words = re!(r"\p{L}+").find_iter(t).count();
    let common = re!(r"(?i)\b(we|our|was|were|is|are|this|these|that|which|have|has|had|been|showed|shown|found|suggest|suggests|however|thus|therefore|could|would|may|might|should|also|than|there)\b")
        .find_iter(t)
        .count();
    count(t) > 1500 || (words >= 20 && common as f64 > 0.08 * words as f64)
}

/// A reference entry for a paragraph in a list without a heading: starts like one and looks like
/// one.
fn strong(t: &str) -> bool {
    entry_start(t) && score(t) >= 3
}

/// Continues the entry before it: starts in lower case, with a digit or punctuation.
fn continues(t: &str) -> bool {
    re!(r"^\s*([\p{Ll}\d(,;:.–\-&]|and\s)").is_match(t) && label(t).is_none()
}

fn text_para(p: &Para) -> bool {
    p.kind == Kind::Text && p.boxed.is_none() && !p.lines.is_empty()
}

fn neutral(p: &Para) -> bool {
    matches!(p.kind, Kind::Figure | Kind::Table | Kind::Caption | Kind::Equation)
}

fn heading_para(lines: Vec<PLine>, page: usize) -> Para {
    let mut lines = lines;
    for l in &mut lines {
        let t = heading_text(&l.text);
        l.spans = vec![Span { text: t.clone(), bold: true, ..Default::default() }];
        l.text = t;
    }
    let mut h = Para::new(lines, page, Kind::Heading);
    h.level = 1;
    h
}

/// The analysed paragraphs with the reference lists marked: their headings are level-1 headings,
/// and each entry is one paragraph, flagged in the second vector.
pub fn mark(paras: Vec<Para>, body: f64) -> (Vec<Para>, Vec<bool>) {
    // 1. headings: known ones, and "References" in small type or on the first line of the list
    let mut out: Vec<Para> = Vec::with_capacity(paras.len() + 2);
    let mut any_heading = false;
    let n = paras.len();
    let mut paras: Vec<Option<Para>> = paras.into_iter().map(Some).collect();
    for i in 0..n {
        let mut p = paras[i].take().unwrap();
        let text = p.text();
        let next_entry = || {
            paras[i + 1..]
                .iter()
                .flatten()
                .find(|q| !neutral(q))
                .map(|q| q.kind == Kind::Text && q.boxed.is_none() && entry_start(&q.text()))
                .unwrap_or(false)
        };
        if p.kind == Kind::Heading && is_heading(&text) {
            let page = p.page;
            out.push(heading_para(std::mem::take(&mut p.lines), page));
            any_heading = true;
            continue;
        }
        if matches!(p.kind, Kind::Text | Kind::Heading) && p.boxed.is_none() && p.lines.len() <= 2 && is_heading(&text) && next_entry() {
            let page = p.page;
            out.push(heading_para(std::mem::take(&mut p.lines), page));
            any_heading = true;
            continue;
        }
        if p.kind == Kind::Text && p.boxed.is_none() && p.lines.len() >= 2 && is_heading(&p.lines[0].text) {
            let rest: String = p.lines[1].text.clone();
            if entry_start(&rest) {
                let first = p.lines.remove(0);
                out.push(heading_para(vec![first], p.page));
                p.page = p.lines[0].page;
                out.push(p);
                any_heading = true;
                continue;
            }
        }
        out.push(p);
    }
    let mut paras = out;
    // 2. a list without a heading
    if !any_heading {
        paras = find_runs(paras, body);
    }
    // 3. the entries of each list
    let mut out: Vec<Para> = Vec::with_capacity(paras.len());
    let mut flags: Vec<bool> = Vec::with_capacity(paras.len());
    let mut it = paras.into_iter().peekable();
    while let Some(p) = it.next() {
        let is_ref_heading = p.kind == Kind::Heading && p.level <= 1 && is_heading(&p.text());
        out.push(p);
        flags.push(false);
        if !is_ref_heading {
            continue;
        }
        let mut section: Vec<Para> = Vec::new();
        while let Some(q) = it.peek() {
            if q.kind == Kind::Heading && q.level <= 1 && !(entry_start(&q.text()) && score(&q.text()) >= 2) {
                break;
            }
            let mut q = it.next().unwrap();
            if q.kind == Kind::Heading && q.boxed.is_none() && entry_start(&q.text()) && score(&q.text()) >= 2 {
                q.kind = Kind::Text;
                q.level = 0;
            }
            section.push(q);
        }
        for (q, f) in entries(section) {
            out.push(q);
            flags.push(f);
        }
    }
    (out, flags)
}

/// Inserts a "References" heading before each run of paragraphs that is a reference list.
fn find_runs(paras: Vec<Para>, body: f64) -> Vec<Para> {
    let total: usize = paras.iter().filter(|p| text_para(p)).map(|p| count(&p.text())).sum();
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let mut before = 0usize;
    let mut i = 0;
    while i < paras.len() {
        let p = &paras[i];
        if !(text_para(p) && strong(&p.text())) {
            if text_para(p) {
                before += count(&p.text());
            }
            i += 1;
            continue;
        }
        // the run: entries, pieces that continue one, figures and captions in between
        let (mut j, mut n_strong, mut n_cont) = (i, 0usize, 0usize);
        let mut labels: Vec<u32> = Vec::new();
        let mut firsts: Vec<char> = Vec::new();
        let mut authors = 0;
        let mut last = i;
        while j < paras.len() {
            let q = &paras[j];
            if neutral(q) {
                j += 1;
                continue;
            }
            if !text_para(q) {
                break;
            }
            let t = q.text();
            if strong(&t) {
                n_strong += 1;
                if let Some(l) = label(&t) {
                    labels.push(l);
                } else if author_start(&t) {
                    authors += 1;
                    firsts.extend(t.trim().chars().next().map(|c| c.to_lowercase().next().unwrap_or(c)));
                }
            } else if (entry_start(&t) && score(&t) >= 2) || (!entry_start(&t) && (continues(&t) || score(&t) >= 2) && !prose(&t)) {
                n_cont += 1;
            } else {
                break;
            }
            last = j;
            j += 1;
        }
        let in_order = |v: &[u32]| v.len() >= 6 && v.windows(2).filter(|w| w[1] > w[0] && w[1] <= w[0] + 5).count() as f64 >= 0.8 * (v.len() - 1) as f64;
        let sorted = |v: &[char]| v.windows(2).filter(|w| w[1] >= w[0]).count() as f64 >= 0.7 * (v.len().max(2) - 1) as f64;
        let ok_list = if labels.len() >= n_strong / 2 { in_order(&labels) } else { authors >= 8 && sorted(&firsts) };
        // running text after the list, up to the next section (notes in small type do not count)
        let after = paras[last + 1..]
            .iter()
            .take_while(|q| !(q.kind == Kind::Heading && q.level <= 1))
            .filter(|q| text_para(q) && count(&q.text()) >= 100 && q.size() > body - 1.0)
            .count();
        if n_strong >= 6
            && n_strong as f64 >= 0.6 * (n_strong + n_cont) as f64
            && ok_list
            && paras[i].page >= 1
            && before as f64 >= 0.3 * total as f64
            && after <= 2
        {
            // (a list broken by an entry that does not look like one goes on)
            match runs.last_mut() {
                Some(r) if i <= r.1 + 2 => r.1 = last,
                _ => runs.push((i, last)),
            }
        }
        for q in &paras[i..=last] {
            if text_para(q) {
                before += count(&q.text());
            }
        }
        i = last + 1;
    }
    if runs.is_empty() {
        return paras;
    }
    let mut out = Vec::with_capacity(paras.len() + runs.len());
    for (k, p) in paras.into_iter().enumerate() {
        if runs.iter().any(|r| r.0 == k) {
            let mut l = p.lines[0].clone();
            l.text = "References".into();
            l.bbox.x1 = l.bbox.x0 + 5.0 * l.size;
            out.push(heading_para(vec![l], p.page));
        }
        out.push(p);
    }
    out
}

/// One paragraph per entry, flagged; other paragraphs of the section (notes after the list, a
/// publisher's box) stay as they are.
fn entries(section: Vec<Para>) -> Vec<(Para, bool)> {
    // a number set apart from its entry ("1." on a line of its own) goes with the entry
    let mut section = section;
    let mut k = 0;
    while k + 1 < section.len() {
        if text_para(&section[k]) && text_para(&section[k + 1]) && re!(r"^\s*(\[\d{1,4}\]|\d{1,4}[.)]?)\s*$").is_match(&section[k].text()) {
            let next = section.remove(k + 1);
            section[k].lines.extend(next.lines);
        }
        k += 1;
    }
    let texts: Vec<String> = section.iter().filter(|p| text_para(p)).map(|p| p.text()).collect();
    let numbered = !texts.is_empty() && texts.iter().filter(|t| label(t).is_some()).count() * 2 >= texts.len();
    // where the next entry starts in the section (a paragraph without a number before it continues an entry)
    let starts_later: Vec<bool> = {
        let mut v = vec![false; section.len()];
        let mut seen = false;
        for k in (0..section.len()).rev() {
            v[k] = seen;
            if text_para(&section[k]) && label(&section[k].text()).is_some() {
                seen = true;
            }
        }
        v
    };
    let mut out: Vec<(Para, bool)> = Vec::new();
    let mut cur: Option<u32> = None;
    // the index in `out` of the entry being built, while no other paragraph came after it
    let mut open: Option<usize> = None;
    for (k, p) in section.into_iter().enumerate() {
        if !text_para(&p) {
            if !neutral(&p) {
                open = None;
            }
            out.push((p, false));
            continue;
        }
        let t = p.text();
        // at the hanging indent of the entry before it, in the same column
        let indented = open.is_some_and(|o| {
            let (a, b) = (&out[o].0.lines[0], &p.lines[0]);
            a.page == b.page && b.bbox.x0 > a.bbox.x0 + 3.0 && b.bbox.x0 < a.bbox.x0 + 40.0
        });
        let starts = if numbered {
            label(&t).is_some()
        } else {
            (entry_start(&t) || score(&t) >= 2) && !(open.is_some() && (continues(&t) || indented))
        };
        if !starts {
            let goes_on = open.is_some()
                && if numbered {
                    starts_later[k] || continues(&t) || score(&t) >= 1
                } else {
                    continues(&t) || indented || !prev_ends(&out, open)
                };
            if goes_on {
                let mut units = split(p.lines, numbered, &mut cur).into_iter();
                if let (Some(o), Some(first)) = (open, units.next()) {
                    out[o].0.lines.extend(first);
                }
                for unit in units {
                    let page = unit[0].page;
                    out.push((Para::new(unit, page, Kind::Text), true));
                    open = Some(out.len() - 1);
                }
                continue;
            }
            open = None;
            out.push((p, false));
            continue;
        }
        if numbered {
            cur = label(&t);
        }
        for unit in split(p.lines, numbered, &mut cur) {
            let page = unit[0].page;
            out.push((Para::new(unit, page, Kind::Text), true));
        }
        open = Some(out.len() - 1);
    }
    out
}

/// Whether the entry being built ends like a finished reference (a full stop or a bracket).
fn prev_ends(out: &[(Para, bool)], open: Option<usize>) -> bool {
    open.map(|o| re!(r"[.)\]>]\s*$").is_match(&out[o].0.text())).unwrap_or(true)
}

/// A paragraph's lines cut into entries: at the next number (numbered list), at the hanging indent,
/// or after a short line that ends an entry when the next starts with an author.
fn split(lines: Vec<PLine>, numbered: bool, cur: &mut Option<u32>) -> Vec<Vec<PLine>> {
    // per line, the column it is in (the lines of the paragraph starting within 40 pt of it): its
    // left and right edge, and whether lines in it are indented (a hanging indent)
    let cols: Vec<(f64, f64, bool)> = lines
        .iter()
        .map(|l| {
            let same: Vec<&PLine> = lines.iter().filter(|m| (m.bbox.x0 - l.bbox.x0).abs() < 40.0).collect();
            let left = same.iter().map(|m| m.bbox.x0).fold(f64::MAX, f64::min);
            let right = same.iter().map(|m| m.bbox.x1).fold(f64::MIN, f64::max);
            (left, right, same.iter().any(|m| m.bbox.x0 > left + 3.0))
        })
        .collect();
    let mut units: Vec<Vec<PLine>> = Vec::new();
    for (k, l) in lines.into_iter().enumerate() {
        let t = l.text.trim().to_string();
        let new = k > 0
            && if numbered {
                // the next number, or a number at the hanging indent
                let (left, _, hanging) = cols[k];
                match (label(&t), *cur) {
                    (Some(n), Some(c)) => n == c + 1 || (hanging && (l.bbox.x0 - left).abs() < 1.5 && n > c && n <= c + 20),
                    _ => false,
                }
            } else {
                let (left, _, hanging) = cols[k];
                let prev = units.last().and_then(|u| u.last()).unwrap();
                let (pleft, pright, _) = cols[k - 1];
                (hanging && (l.bbox.x0 - left).abs() < 1.5 && entry_start(&t))
                    || (!hanging
                        && re!(r"[.)]$").is_match(prev.text.trim())
                        && pright - prev.bbox.x1 > 0.15 * (pright - pleft).max(1.0)
                        && author_start(&t))
            };
        if new {
            if numbered {
                *cur = label(&t);
            }
            units.push(vec![l]);
        } else if let Some(u) = units.last_mut() {
            u.push(l);
        } else {
            units.push(vec![l]);
        }
    }
    units
}
