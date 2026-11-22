//! Footnotes and margin notes of a PDF (a port of `PDFNotes.swift`).
//!
//! A footnote is a block at the foot of a page, below the last line of body type in its column and
//! set apart from it, in smaller type, that starts with a marker ("²", "*", "†", "a") which the
//! page's running text also has: a superscript (or a symbol) right after a word or a punctuation
//! mark. The note leaves the running text for the end of its chapter (`<aside epub:type="footnote">`)
//! and the marker in the text becomes a link to it (`<a epub:type="noteref">`) that keeps its
//! character. A block at the next page's foot without a marker continues the last note of the page
//! before.
//!
//! A superscript number in a paper that cites by number is a citation unless such a block exists on
//! its page; with several candidates for the same number the note is still moved but nothing is
//! linked. The reference list's pages and the index are left alone, and so are the first pages'
//! footnotes the front matter already took (correspondence, affiliations, "equal contribution").
//!
//! A margin note (books only) is a block of small type outside the text column ("Notebook 1.1",
//! "Problems 2.1–2.2", "Appendix C.5.2"): taken out before the page's paragraphs are made (it would
//! split them), it follows the paragraph level with it as `<aside class="sidenote">`.

use super::layout::{caption_re, count, is_prose, re, trim, Analyzer, Kind, PLine, Para, Span, R};
use std::collections::{HashMap, HashSet};

const SYMBOLS: [char; 7] = ['*', '†', '‡', '§', '¶', '‖', '#'];

/// A footnote's marker at the start of its first line: a superscript "2", "*", "†", "a", or a plain
/// "2"/"†" before its text ("2 This …", "2This …", "†These …"). Not "2. Smith …" or "[2] …".
pub fn note_marker(l: &PLine) -> Option<String> {
    let k = l.spans.iter().position(|s| !trim(&s.text).is_empty())?;
    let s = &l.spans[k];
    let t = trim(&s.text);
    if s.sup {
        if re!(r"^(\d{1,3}|[*†‡§¶‖#]{1,3}|[a-z])$").is_match(t) && l.spans[k + 1..].iter().any(|x| x.text.chars().any(char::is_alphabetic)) {
            return Some(t.to_string());
        }
        return None;
    }
    let lt = trim(&l.text);
    if let Some(c) = re!(r"^([*†‡§¶‖]{1,3})\s*\p{L}").captures(lt) {
        return Some(c[1].to_string());
    }
    // (the regex crate has no look-ahead: the number, then a capital after a space or a word)
    if let Some(c) = re!(r"^(\d{1,3})(?:\s+\p{Lu}|\p{Lu}\p{Ll})").captures(lt) {
        return Some(c[1].to_string());
    }
    None
}

/// The line without its leading marker (spans and text).
pub fn strip_marker(l: &mut PLine, m: &str) {
    if let Some(k) = l.spans.iter().position(|s| !trim(&s.text).is_empty()) {
        if l.spans[k].sup {
            l.spans.drain(0..=k);
        } else {
            let t = l.spans[k].text.trim_start_matches(' ');
            let t = t.strip_prefix(m).unwrap_or(t).to_string();
            l.spans[k].text = t;
            l.spans.drain(0..k);
        }
        if let Some(f) = l.spans.first_mut() {
            f.text = f.text.trim_start_matches(' ').to_string();
        }
    }
    let t = l.text.trim_start_matches(' ');
    let t = t.strip_prefix(m).unwrap_or(t);
    l.text = t.trim_start_matches(' ').to_string();
}

/// A place in the running text where a note's marker is: paragraph, line, span, and (for a symbol set
/// in the text's own type) the character at `at` within the span.
#[derive(Debug, Clone, Copy)]
struct NoteRef {
    para: usize,
    line: usize,
    span: usize,
    at: Option<usize>,
    score: i32,
}

/// How much a superscript looks like a note's marker: after a word of three or more letters or a
/// punctuation mark rather than a variable or a digit ("x²", "(a+b)²" in a formula), not run into
/// the next word.
fn ref_score(before: &str, after: &str, italic: bool, line: &PLine) -> i32 {
    let mut score = 0;
    if let Some(pc) = before.chars().last() {
        if ".,;:!?)”’\"]".contains(pc) {
            score += 2;
        } else if pc.is_alphabetic() {
            let run = before.chars().rev().take_while(|c| c.is_alphabetic()).count();
            score += if run >= 3 { 1 } else { -2 };
        } else if pc.is_numeric() {
            score -= 2;
        } else if pc == ' ' {
            score -= 1;
        }
    }
    if after.chars().next().is_some_and(|c| c.is_alphanumeric()) {
        score -= 2;
    }
    if italic {
        score -= 1;
    }
    if line.math > 0.5 {
        score -= 1;
    }
    score
}

/// Where `marker` stands in these paragraphs' text as a note's marker.
fn note_refs(marker: &str, paras: &[Para], which: &[usize]) -> Vec<NoteRef> {
    let symbol = marker.chars().next().is_some_and(|c| SYMBOLS.contains(&c));
    let mut out = Vec::new();
    for &pi in which {
        for (li, l) in paras[pi].lines.iter().enumerate() {
            for (k, s) in l.spans.iter().enumerate() {
                let before: String = l.spans[..k].iter().map(|x| x.text.as_str()).collect();
                let after: String = l.spans[k + 1..].iter().map(|x| x.text.as_str()).collect();
                if s.sup && s.note_ref.is_none() && trim(&s.text) == marker && !s.text.starts_with(' ') {
                    out.push(NoteRef { para: pi, line: li, span: k, at: None, score: ref_score(&before, &after, s.italic, l) });
                } else if symbol && !s.sup && s.note_ref.is_none() {
                    // "word†" in the text's own type
                    let chars: Vec<char> = s.text.chars().collect();
                    for (i, c) in chars.iter().enumerate() {
                        if c.to_string() != marker {
                            continue;
                        }
                        let prev: String = if i > 0 { chars[..i].iter().collect() } else { before.clone() };
                        let ok = prev.chars().last().is_some_and(|pc| pc.is_alphanumeric() || ".,;:)”’]".contains(pc));
                        if !ok {
                            continue;
                        }
                        let rest: String = chars[i + 1..].iter().collect::<String>() + &after;
                        out.push(NoteRef { para: pi, line: li, span: k, at: Some(i), score: ref_score(&prev, &rest, false, l) });
                    }
                }
            }
        }
    }
    out
}

/// The id of a note: "fn-p12-2" (page 12, marker 2), "fn-p3-s1" for the first symbol.
pub fn note_id(page: usize, marker: &str, taken: &mut HashSet<String>) -> String {
    let key = if re!(r"^[0-9a-z]+$").is_match(marker) {
        marker.to_string()
    } else {
        let i = marker.chars().next().and_then(|c| SYMBOLS.iter().position(|&s| s == c)).unwrap_or(0);
        let n = marker.chars().count();
        format!("s{}{}", i + 1, if n > 1 { format!("x{n}") } else { String::new() })
    };
    let mut id = format!("fn-p{}-{key}", page + 1);
    let mut n = 2;
    while taken.contains(&id) {
        id = format!("fn-p{}-{key}-{n}", page + 1);
        n += 1;
    }
    taken.insert(id.clone());
    id
}

/// Links the marker in the text to note `id` (a plain symbol becomes a span of its own).
fn link(paras: &mut [Para], r: NoteRef, id: &str) {
    let l = &mut paras[r.para].lines[r.line];
    if let Some(at) = r.at {
        let s = l.spans[r.span].clone();
        let chars: Vec<char> = s.text.chars().collect();
        let a = Span { text: chars[..at].iter().collect(), ..s.clone() };
        let m = Span { text: chars[at].to_string(), note_ref: Some(id.to_string()), ..s.clone() };
        let b = Span { text: chars[at + 1..].iter().collect(), ..s };
        let parts: Vec<Span> = [a, m, b].into_iter().filter(|x| !x.text.is_empty()).collect();
        l.spans.splice(r.span..=r.span, parts);
    } else {
        l.spans[r.span].note_ref = Some(id.to_string());
    }
}

fn overlap_x(a: &R, b: &R) -> bool {
    a.x1.min(b.x1) - a.x0.max(b.x0) > 2.0
}

/// A superscript at the end of a line that the PDF gives as a line of its own (a footnote's or a
/// citation's "2" after "outlier."): put back at the end of its line, as a superscript, rather than
/// dropped with the page numbers.
pub fn attach_markers(lines: &mut Vec<PLine>, body: f64) {
    let mut gone: HashSet<usize> = HashSet::new();
    for i in 0..lines.len() {
        let l = &lines[i];
        let t = trim(&l.text).to_string();
        if count(&t) > 3 || !re!(r"^(\d{1,3}|[*†‡§¶‖]{1,3}|[a-z])$").is_match(&t) || l.size <= 0.0 || l.size >= 0.85 * body {
            continue;
        }
        let lb = l.bbox;
        let host = (0..lines.len()).find(|&j| {
            let h = &lines[j];
            j != i
                && !gone.contains(&j)
                && count(&h.text) > 3
                && h.size >= 0.85 * body
                && lb.x0 >= h.bbox.x1 - 1.0
                && lb.x0 - h.bbox.x1 < 0.6 * h.size
                && lb.y0 >= h.bbox.y0 - 0.5 * h.size
                && lb.y1 <= h.bbox.y1 + 1.0
                && (lb.y0 + lb.y1) / 2.0 < (h.bbox.y0 + h.bbox.y1) / 2.0
        });
        let Some(j) = host else { continue };
        let h = &mut lines[j];
        h.text += &t;
        h.spans.push(Span { text: t, sup: true, ..Default::default() });
        h.bbox = h.bbox.union(&lb);
        gone.insert(i);
    }
    if !gone.is_empty() {
        let mut k = 0;
        lines.retain(|_| {
            k += 1;
            !gone.contains(&(k - 1))
        });
    }
}

impl Analyzer<'_> {
    pub fn find_footnotes(&self, input: Vec<Para>) -> Vec<Para> {
        let body = self.body_size;
        let mut paras = input;
        // the reference list on: its entries are not notes
        let refs_at = paras
            .iter()
            .position(|p| (p.kind == Kind::Heading || (p.kind == Kind::Text && p.lines.len() <= 2)) && super::refs::is_heading(&p.text()))
            .unwrap_or(paras.len());
        // a paper that cites by superscript numbers
        let numeric_sups = paras
            .iter()
            .filter(|p| matches!(p.kind, Kind::Text | Kind::Abstract))
            .flat_map(|p| &p.lines)
            .flat_map(|l| &l.spans)
            .filter(|s| s.sup && re!(r"^\d{1,3}([,–\-]\d{1,3})*$").is_match(trim(&s.text)))
            .count();
        let cites_by_number = !self.book.kind.is_book_like() && numeric_sups >= 8;
        let mut by_page: Vec<(usize, Vec<usize>)> = Vec::new();
        for (i, p) in paras.iter().enumerate().take(refs_at) {
            match by_page.iter_mut().find(|x| x.0 == p.page) {
                Some(x) => x.1.push(i),
                None => by_page.push((p.page, vec![i])),
            }
        }
        by_page.sort_by_key(|x| x.0);
        let small = |p: &Para| p.kind == Kind::Text && p.boxed.is_none() && !p.lines.is_empty() && p.size() > 0.0 && p.size() <= body - 0.6;
        let blocks = |p: &Para| match p.kind {
            Kind::Text => !small(p),
            Kind::Heading | Kind::Abstract | Kind::Equation | Kind::Figure | Kind::Table | Kind::Caption | Kind::Title => true,
            _ => false,
        };
        // a paragraph's place → what goes there instead
        let mut replace: HashMap<usize, Vec<Para>> = HashMap::new();
        let mut ids: HashSet<String> = HashSet::new();
        // the last note: in `replace` (its paragraph's place, its index there) and its page
        let mut last_note: Option<(usize, usize)> = None;
        let mut last_note_page: i64 = -10;
        let (mut found, mut linked, mut continued) = (0, 0, 0);
        for (pno, idx) in &by_page {
            let pno = *pno;
            if self.book.on_index_page(pno) {
                continue;
            }
            let h = self.pages[pno].height;
            // the foot of the page: small type with nothing of the body below it in its column, set
            // apart from the text above it (or stacked under another note)
            let mut order = idx.clone();
            order.sort_by(|&a, &b| paras[a].bbox().y0.total_cmp(&paras[b].bbox().y0));
            let mut foot: Vec<usize> = Vec::new();
            for &i in &order {
                if !small(&paras[i]) || paras[i].bbox().y0 <= 0.4 * h {
                    continue;
                }
                let r = paras[i].bbox();
                let col: Vec<usize> = idx.iter().copied().filter(|&j| j != i && blocks(&paras[j]) && overlap_x(&paras[j].bbox(), &r)).collect();
                if col.iter().any(|&j| paras[j].bbox().y1 > r.y0 + 1.0) {
                    continue;
                }
                let above_body = col.iter().map(|&j| paras[j].bbox().y1).fold(None, |m: Option<f64>, y| Some(m.map_or(y, |m| m.max(y))));
                let above_foot = foot
                    .iter()
                    .filter(|&&j| overlap_x(&paras[j].bbox(), &r))
                    .map(|&j| paras[j].bbox().y1)
                    .fold(None, |m: Option<f64>, y| Some(m.map_or(y, |m| m.max(y))));
                if let Some(f) = above_foot {
                    if f >= above_body.unwrap_or(-1.0) {
                        foot.push(i);
                        continue;
                    }
                }
                if let Some(a) = above_body {
                    if r.y0 - a >= 0.4 * body {
                        foot.push(i);
                    }
                }
            }
            if foot.is_empty() || foot.iter().map(|&i| paras[i].bbox().y1).fold(f64::MIN, f64::max) <= 0.75 * h {
                continue;
            }
            let text: Vec<usize> = idx
                .iter()
                .copied()
                .filter(|i| !foot.contains(i) && matches!(paras[*i].kind, Kind::Text | Kind::Abstract | Kind::Heading | Kind::Caption))
                .collect();
            let has_refs = |paras: &[Para], m: &str| !note_refs(m, paras, &text).is_empty();
            // the foot's blocks, cut where a line starts with a marker the text has
            let mut foot_sorted = foot.clone();
            foot_sorted.sort();
            let mut chunks: Vec<(usize, Vec<PLine>)> = Vec::new();
            for &i in &foot_sorted {
                let mut cur: Vec<PLine> = Vec::new();
                for (k, l) in paras[i].lines.iter().enumerate() {
                    if k > 0 && !cur.is_empty() && note_marker(l).is_some_and(|m| has_refs(&paras, &m)) {
                        chunks.push((i, std::mem::take(&mut cur)));
                    }
                    cur.push(l.clone());
                }
                if !cur.is_empty() {
                    chunks.push((i, cur));
                }
            }
            let markers: Vec<Option<String>> = chunks.iter().map(|c| note_marker(&c.1[0])).collect();
            let any_matched = markers.iter().any(|m| m.as_ref().is_some_and(|m| has_refs(&paras, m)));
            if !(any_matched || (last_note_page == pno as i64 - 1 && last_note.is_some())) {
                continue;
            }
            let mut out: HashMap<usize, Vec<Para>> = HashMap::new();
            let mut notes: Vec<(usize, usize)> = Vec::new(); // (place, index in out[place])
            let mut unmarked: Vec<(usize, Para, bool)> = Vec::new();
            for ((at, lines), m) in chunks.into_iter().zip(markers) {
                let mut p = Para::new(lines, pno, Kind::Text);
                let Some(m) = m.filter(|_| any_matched) else {
                    let marked = note_marker(&p.lines[0]).is_some();
                    unmarked.push((at, p, marked));
                    continue;
                };
                let rs = note_refs(&m, &paras, &text);
                // (a block that reads like a reference, when the text doesn't have its marker or the paper
                // cites by number: the reference list explains the number better; left alone)
                if (rs.is_empty() || cites_by_number) && !is_prose(&p.text()) && super::refs::score(&p.text()) >= 3 {
                    unmarked.push((at, p, true));
                    continue;
                }
                p.kind = Kind::Footnote;
                p.note_label = Some(m.clone());
                let id = note_id(pno, &m, &mut ids);
                p.note_id = Some(id.clone());
                strip_marker(&mut p.lines[0], &m);
                // the marker in the text: the likeliest place; in a paper citing by number, only if no
                // other superscript with that number could be it
                let top = rs.iter().map(|r| r.score).max().unwrap_or(-1);
                let ambiguous = cites_by_number && m.starts_with(|c: char| c.is_ascii_digit()) && rs.iter().filter(|r| r.score >= 0).count() > 1;
                if top >= 0 && !ambiguous {
                    if let Some(best) = rs.iter().find(|r| r.score == top) {
                        link(&mut paras, *best, &id);
                        linked += 1;
                    }
                }
                let v = out.entry(at).or_default();
                v.push(p);
                notes.push((at, v.len() - 1));
                found += 1;
            }
            // blocks without a marker: the note before goes on (beside it or under it), or the last note
            // of the page before does (the first block of this page's foot, before any note of its own)
            for (at, u, marked) in unmarked {
                let r = u.bbox();
                let us = u.size();
                let near = notes.iter().rev().copied().find(|&(a, k)| {
                    let b = out[&a][k].bbox();
                    (b.y0 <= r.y0 + us && overlap_x(&b, &r)) || b.y1.min(r.y1) - b.y0.max(r.y0) > 0.5 * us
                });
                if let Some((a, k)) = near {
                    let n = &mut out.get_mut(&a).unwrap()[k];
                    if r.y0 - n.bbox().y1 < 1.5 * us {
                        n.lines.extend(u.lines);
                        n.lines.sort_by(|a, b| {
                            if (a.bbox.y0 - b.bbox.y0).abs() < 0.6 * a.size {
                                a.bbox.x0.total_cmp(&b.bbox.x0)
                            } else {
                                a.bbox.y0.total_cmp(&b.bbox.y0)
                            }
                        });
                        continue;
                    }
                }
                let before_notes = !notes.iter().any(|&(a, k)| out[&a][k].bbox().y0 < r.y0);
                if let (false, Some((a, k)), true, true) = (marked, last_note, last_note_page == pno as i64 - 1, before_notes) {
                    let last = &mut replace.get_mut(&a).unwrap()[k];
                    let lt = last.text();
                    let ends = re!(r#"[.!?)”"]$"#).is_match(trim(&lt));
                    if (last.size() - us).abs() < 0.5 && (!ends || u.text().chars().next().is_some_and(|c| c.is_lowercase())) {
                        last.lines.extend(u.lines);
                        continued += 1;
                        last_note_page = pno as i64;
                        continue;
                    }
                }
                out.entry(at).or_default().push(u);
            }
            // in their places, top to bottom; the page's last note is the one a block on the next page may continue
            let mut last_here: Option<(usize, usize, f64)> = None;
            for &i in &foot {
                let mut v = out.remove(&i).unwrap_or_default();
                v.sort_by(|a, b| a.lines[0].bbox.y0.total_cmp(&b.lines[0].bbox.y0));
                for (k, p) in v.iter().enumerate() {
                    if p.kind == Kind::Footnote {
                        let y = p.bbox().y1;
                        if last_here.is_none_or(|x| y >= x.2) {
                            last_here = Some((i, k, y));
                        }
                    }
                }
                replace.insert(i, v);
            }
            if let Some((i, k, _)) = last_here {
                last_note = Some((i, k));
                last_note_page = pno as i64;
            }
        }
        if found == 0 {
            return paras;
        }
        (self.log)(&format!(
            "Footnotes: {found} found, {linked} linked to their marker{}",
            if continued > 0 { format!(", {continued} continued on the next page") } else { String::new() }
        ));
        let mut result = Vec::with_capacity(paras.len());
        for (i, p) in paras.into_iter().enumerate() {
            match replace.remove(&i) {
                Some(r) => result.extend(r),
                None => result.push(p),
            }
        }
        result
    }

    /// A book page's margin notes: blocks of small type outside the text column (the column taken
    /// from the page's wide lines). Returns the page's other lines and the notes.
    pub fn margin_notes(&self, pno: usize, lines: Vec<PLine>) -> (Vec<PLine>, Vec<Para>) {
        let body = self.body_size;
        let w = self.pages[pno].width;
        // (the column's lines: wide ones, in body type or the smaller type of a chapter's notes)
        let col: Vec<&PLine> = lines.iter().filter(|l| l.size > body - 1.6 && l.bbox.w() > 0.4 * w).collect();
        if col.len() < 4 {
            return (lines, Vec::new());
        }
        let mut x0s: Vec<f64> = col.iter().map(|l| l.bbox.x0).collect();
        let mut x1s: Vec<f64> = col.iter().map(|l| l.bbox.x1).collect();
        x0s.sort_by(f64::total_cmp);
        x1s.sort_by(f64::total_cmp);
        let (col_x0, col_x1) = (x0s[x0s.len() / 5], x1s[x1s.len() * 4 / 5]);
        // a margin line the PDF ran into a mark of the formula beside it ("Argmin function ˆ"): the
        // note without the mark (the formula's crop keeps it)
        let mut lines = lines;
        for l in lines.iter_mut() {
            if l.size <= 0.0 || l.size > body - 1.0 || l.bbox.x0 >= col_x0 - 10.0 || l.bbox.x1 <= col_x0 + 1.0 {
                continue;
            }
            let Some(m) = re!(r"\s+[^\p{Lu}\p{Ll}\p{Lo}\p{N}\s]{1,3}$").find(&l.text) else { continue };
            if !l.text[..m.start()].chars().any(char::is_alphabetic) {
                continue;
            }
            let tail = trim(m.as_str()).to_string();
            l.text.truncate(m.start());
            if let Some(k) = l.spans.iter().rposition(|s| s.text.contains(&tail)) {
                let at = l.spans[k].text.rfind(&tail).unwrap();
                l.spans[k].text.replace_range(at..at + tail.len(), "");
                let t = l.spans[k].text.trim_end_matches(' ').to_string();
                l.spans[k].text = t;
                if l.spans[k].text.is_empty() {
                    l.spans.remove(k);
                }
            }
            l.bbox.x1 = l.bbox.x1.min(col_x0 - 2.0);
        }
        let outside = |l: &PLine| -> i32 {
            if l.size <= 0.0 || l.size > body - 1.0 || l.bbox.w() >= 0.3 * w {
                0
            } else if l.bbox.x0 >= col_x1 - 1.0 {
                1
            } else if l.bbox.x1 <= col_x0 + 1.0 {
                -1
            } else {
                0
            }
        };
        let mut order: Vec<usize> = (0..lines.len()).collect();
        order.sort_by(|&a, &b| lines[a].bbox.y0.total_cmp(&lines[b].bbox.y0));
        let mut blocks: Vec<(i32, Vec<usize>)> = Vec::new();
        for i in order {
            let l = &lines[i];
            let side = outside(l);
            if side == 0 {
                continue;
            }
            let k = blocks.iter().rposition(|b| b.0 == side);
            if let Some(k) = k {
                let last = &lines[*blocks[k].1.last().unwrap()];
                if l.bbox.y0 - last.bbox.y1 < l.size && l.bbox.x1.min(last.bbox.x1) > l.bbox.x0.max(last.bbox.x0) - 4.0 {
                    blocks[k].1.push(i);
                    continue;
                }
            }
            blocks.push((side, vec![i]));
        }
        let mut taken: HashSet<usize> = HashSet::new();
        let mut kept_blocks: Vec<Vec<usize>> = Vec::new();
        for (_, b) in blocks {
            let t: String = b.iter().map(|&i| lines[i].text.as_str()).collect::<Vec<_>>().join(" ");
            let t = trim(&t);
            // (a caption set in the margin is a caption; a page number or a lone letter is nothing)
            if caption_re().is_match(t) || b.len() > 10 || count(t) > 500 || count(t) < 3 || re!(r"^[\d\s.,–-]+$").is_match(t) {
                continue;
            }
            taken.extend(b.iter().copied());
            kept_blocks.push(b);
        }
        if kept_blocks.is_empty() {
            return (lines, Vec::new());
        }
        let mut slots: Vec<Option<PLine>> = lines.into_iter().map(Some).collect();
        let notes = kept_blocks
            .into_iter()
            .map(|b| Para::new(b.iter().map(|&i| slots[i].take().unwrap()).collect(), pno, Kind::Sidenote))
            .collect();
        let rest = slots.into_iter().flatten().collect();
        (rest, notes)
    }
}

/// Puts each margin note after the paragraph level with it (its first line within the paragraph's
/// height, else the nearest one above it on the page, else the page's first).
pub fn place_sidenotes(paras: Vec<Para>, notes: Vec<Para>) -> Vec<Para> {
    if notes.is_empty() {
        return paras;
    }
    let mut after: HashMap<usize, Vec<Para>> = HashMap::new();
    let mut first: Vec<(usize, Vec<Para>)> = Vec::new();
    for n in notes {
        let y = n.bbox().y0;
        let on_page = |p: &Para| {
            p.page == n.page
                && matches!(p.kind, Kind::Text | Kind::Heading | Kind::Abstract | Kind::Equation | Kind::Caption | Kind::Figure | Kind::Table)
        };
        let level = paras.iter().position(|p| {
            let b = p.bbox();
            on_page(p) && !matches!(p.kind, Kind::Figure | Kind::Table) && b.y0 - 2.0 <= y && y <= b.y1 + 2.0
        });
        let above = paras
            .iter()
            .enumerate()
            .filter(|(_, p)| on_page(p) && p.bbox().y1 <= y + 2.0)
            .fold(None, |m: Option<(usize, f64)>, (i, p)| {
                let y1 = p.bbox().y1;
                if m.is_none_or(|m| y1 > m.1) {
                    Some((i, y1))
                } else {
                    m
                }
            })
            .map(|x| x.0);
        match level.or(above) {
            Some(t) => after.entry(t).or_default().push(n),
            None => match first.iter_mut().find(|x| x.0 == n.page) {
                Some(x) => x.1.push(n),
                None => first.push((n.page, vec![n])),
            },
        }
    }
    let mut out = Vec::new();
    let mut seen: HashSet<usize> = HashSet::new();
    for (i, p) in paras.into_iter().enumerate() {
        if seen.insert(p.page) {
            if let Some(k) = first.iter().position(|x| x.0 == p.page) {
                out.extend(first.remove(k).1);
            }
        }
        out.push(p);
        if let Some(ns) = after.remove(&i) {
            out.extend(ns);
        }
    }
    for (_, ns) in first {
        out.extend(ns);
    }
    out
}
