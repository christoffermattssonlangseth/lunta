//! Reading a PDF with PDFium: text lines with per-character fonts, sizes and positions, image
//! placements and vector drawings, document info, and rendering crops. Replaces PDFKit plus
//! `PDFContentScanner` (PDFContent.swift): PDFium already exposes real font names and flags per
//! character and inserts the word spaces a content stream only implies by moving, so neither
//! the content-stream walk nor its gap detection is needed.

use super::layout::{DocInfo, PLine, Page, PageGraphics, Renderer, Span, R};
use pdfium_render::prelude::*;
use std::collections::HashMap;

/// A page's media box in PDF user space.
#[derive(Debug, Clone, Copy)]
pub struct MediaBox {
    left: f64,
    top: f64,
    width: f64,
    height: f64,
}

pub fn media_box(page: &PdfPage) -> MediaBox {
    let r = page
        .boundaries()
        .media()
        .map(|b| b.bounds)
        .unwrap_or_else(|_| PdfRect::new_from_values(0.0, 0.0, page.height().value, page.width().value));
    MediaBox {
        left: r.left().value as f64,
        top: r.top().value as f64,
        width: r.width().value as f64,
        height: r.height().value as f64,
    }
}

impl MediaBox {
    /// PDF user-space rectangle → top-left page coordinates.
    fn to_top(self, left: f64, bottom: f64, right: f64, top: f64) -> R {
        R::new(left - self.left, self.top - top, right - self.left, self.top - bottom)
    }
}

/// Publisher download sheets ("Use of this article is subject to the Terms of service") carry no content.
pub fn is_cover_sheet(text: &str) -> bool {
    text.chars().count() < 3000
        && (text.contains("Use of this article is subject to the Terms of service")
            || text.contains("View the article online") && text.contains("Permissions"))
}

#[derive(Clone, Copy, Default)]
struct Style {
    bold: bool,
    italic: bool,
    math: bool,
}

fn style_for(name: &str, force_bold: bool, italic_flag: bool) -> Style {
    let clean = name.split_once('+').map(|x| x.1).unwrap_or(name);
    let bold = force_bold
        || layout_re::bold().is_match(clean);
    let italic = italic_flag || layout_re::italic().is_match(clean);
    let math = layout_re::math().is_match(clean);
    Style { bold, italic, math }
}

mod layout_re {
    use super::super::layout::re;
    use regex::Regex;
    pub fn bold() -> &'static Regex {
        re!(r"(?i:bold|black|heavy|semibold|demi|-bd\b)|-(H|B|Bd|Hv|Hvy|Blk|SB|Sb|BdIt|BI|HI|Bol)$|[a-z](Bd|Blk|Hv)$")
    }
    pub fn italic() -> &'static Regex {
        re!(r"(?i:ital|oblique|slant)|-(I|It|MI|LI|BI|HI|BdIt|SBIt|Ita)$|[a-z]It$")
    }
    pub fn math() -> &'static Regex {
        re!(r"(?i)CMMI|CMSY|CMEX|MSAM|MSBM|Math|Symbol|STIX|rsfs|EUFM|CMBSY|LMMath|esint|MTMI|MTSY|txsy|txmi|pxmi|Euclid")
    }
}

/// One character as PDFium reports it, in top-left page coordinates.
#[derive(Clone)]
struct C {
    ch: char,
    size: f64,
    bx: R,
    style: Option<Style>,
    generated: bool,
}

/// Spacing accents TeX draws as glyphs of their own → the combining mark.
fn combining(c: char) -> Option<char> {
    Some(match c {
        '`' => '\u{300}',
        '´' => '\u{301}',
        'ˆ' | '^' => '\u{302}',
        '˜' => '\u{303}',
        '¯' => '\u{304}',
        '˘' => '\u{306}',
        '˙' => '\u{307}',
        '¨' => '\u{308}',
        '˚' => '\u{30a}',
        '˝' => '\u{30b}',
        'ˇ' => '\u{30c}',
        '¸' => '\u{327}',
        '˛' => '\u{328}',
        _ => return None,
    })
}

/// Puts accents back on the letter they sit over (as a combining mark), dropping the word
/// spaces PDFium inserts around them.
fn attach_accents(cs: &mut Vec<C>) {
    let mut i = 0;
    while i < cs.len() {
        let Some(mark) = combining(cs[i].ch).filter(|_| cs[i].bx.w() > 0.0) else {
            i += 1;
            continue;
        };
        let a = cs[i].bx;
        let over = |c: &C| c.ch.is_alphabetic() && c.bx.w() > 0.0 && a.cx() > c.bx.x0 - 0.5 && a.cx() < c.bx.x1 + 0.5;
        // the letter under it, usually adjacent but sometimes a few characters away
        let base = (i.saturating_sub(6)..cs.len().min(i + 7))
            .filter(|&k| k != i && over(&cs[k]))
            .min_by_key(|&k| k.abs_diff(i));
        let Some(b) = base else {
            i += 1;
            continue;
        };
        let mut acc = cs.remove(i);
        acc.ch = mark;
        let b = if b > i { b - 1 } else { b };
        // generated spaces between the accent's old place and its letter go
        let (lo, hi) = if b < i { (b + 1, i) } else { (i, b) };
        let mut removed_before_b = 0;
        for k in (lo..hi).rev() {
            if cs[k].generated && is_space(cs[k].ch) {
                cs.remove(k);
                if k < b {
                    removed_before_b += 1;
                }
            }
        }
        let b = b - removed_before_b;
        acc.bx = cs[b].bx;
        cs.insert(b + 1, acc);
        if b + 2 < cs.len() && cs[b + 2].generated && is_space(cs[b + 2].ch) && b + 3 < cs.len() && !is_space(cs[b + 3].ch) {
            // "Ruckl ¨ e" → "Rückle": a generated space right after the joined letter
            let after_gap = cs[b + 3].bx.x0 - cs[b].bx.x1;
            if after_gap < 0.15 * cs[b].size {
                cs.remove(b + 2);
            }
        }
        i = b + 2;
    }
    // generated word spaces left next to a re-accented letter where the letters touch
    let is_mark = |c: &C| ('\u{300}'..='\u{36f}').contains(&c.ch);
    let mut k = 1;
    while k + 1 < cs.len() {
        if cs[k].generated && is_space(cs[k].ch) {
            let next = k + 1;
            let (p, n) = (&cs[k - 1], &cs[next]);
            let near_mark = is_mark(p) || cs.get(next + 1).map(is_mark).unwrap_or(false);
            if near_mark && p.bx.w() > 0.0 && n.bx.w() > 0.0 && n.bx.x0 - p.bx.x1 < 0.15 * p.size.max(n.size) {
                cs.remove(k);
                continue;
            }
        }
        k += 1;
    }
}

fn is_space(c: char) -> bool {
    c.is_whitespace() && !matches!(c, '\n' | '\r')
}

/// Text lines of a page. PDFium gives characters in content order with generated line breaks;
/// a line is also cut where the text jumps back left or across a column gutter.
pub fn extract_lines(text: &PdfPageText, mb: &MediaBox, pno: usize) -> Vec<PLine> {
    let mut styles: HashMap<(String, bool, bool), Style> = HashMap::new();
    let mut raw_lines: Vec<Vec<C>> = vec![vec![]];
    let mut pending_break = false;
    let dbg_chars = std::env::var("P2E_DEBUG_CHARS").ok().and_then(|v| v.parse::<usize>().ok()) == Some(pno + 1);
    for c in text.chars().iter() {
        if dbg_chars {
            let b = c.loose_bounds().ok();
            eprintln!(
                "CHAR {:?} gen={:?} ang={:?} sz={:.1} font={} box={:?}",
                c.unicode_char(),
                c.is_generated().ok(),
                c.angle_degrees().ok(),
                c.scaled_font_size().value,
                c.font_name(),
                b.map(|r| (r.left().value as i32, r.top().value as i32, r.right().value as i32, r.bottom().value as i32))
            );
        }
        let Some(ch) = c.unicode_char() else { continue };
        if ch == '\r' || ch == '\n' {
            // decided at the next character: PDFium also breaks around inline super-/subscripts
            pending_break = !raw_lines.last().unwrap().is_empty();
            continue;
        }
        if (ch as u32) < 0x20 && ch != '\t' || ch == '\u{fffe}' || ch == '\u{feff}' {
            // PDFium marks a line-end hyphen as U+0002 (and emits no line break after it); in
            // symbol fonts U+0002 is just an unmapped glyph
            if ch == '\u{2}' && c.is_hyphen().unwrap_or(false) {
                push_char(&mut raw_lines, c_hyphen(&c, mb));
            }
            continue;
        }
        let generated = c.is_generated().unwrap_or(false);
        let bx = c
            .loose_bounds()
            .map(|r| mb.to_top(r.left().value as f64, r.bottom().value as f64, r.right().value as f64, r.top().value as f64))
            .unwrap_or_default();
        // rotated text (margin stamps, vertical labels) is left out; a skewed but level baseline
        // is a synthetic italic (PDFium's char "angle" counts the skew too)
        let mut skewed = false;
        if !generated {
            if let Ok(m) = c.matrix() {
                let (a, b, cc, d) = (m.a() as f64, m.b() as f64, m.c() as f64, m.d() as f64);
                if b.abs() > 0.1 * a.abs() + 0.001 || a <= 0.0 {
                    continue;
                }
                skewed = cc.abs() > 0.1 * d.abs();
            }
        }
        let size = c.scaled_font_size().value as f64;
        let style = if generated || is_space(ch) {
            None
        } else {
            let name = c.font_name();
            let key = (name, c.font_is_bold_reenforced(), c.font_is_italic());
            let mut st = *styles.entry(key.clone()).or_insert_with(|| style_for(&key.0, key.1, key.2));
            st.italic |= skewed;
            Some(st)
        };
        let cc = C { ch, size, bx, style, generated };
        // cut where the text moves to another baseline or jumps across a gutter
        if !is_space(ch) && bx.w() > 0.0 {
            let mut cut = pending_break;
            let mut space = false;
            if let Some(p) = raw_lines.last().unwrap().iter().rev().find(|p| p.bx.w() > 0.0 && !is_space(p.ch)) {
                let s = p.size.max(size).max(4.0);
                let same_line = bx.y1.min(p.bx.y1) - bx.y0.max(p.bx.y0) > 0.3 * bx.h().min(p.bx.h());
                let gap = bx.x0 - p.bx.x1;
                // a change of size right next to the previous character is a super-/subscript
                let script = (size - p.size).abs() > 0.15 * s
                    && gap > -0.5 * s
                    && gap < 1.5 * s
                    && bx.y1.min(p.bx.y1) + 0.6 * s > bx.y0.max(p.bx.y0);
                let adjacent = (same_line || script) && gap > -0.5 * s && gap < 1.0 * s;
                if (!same_line && !script) || gap > 2.0 * s || gap < -2.0 * s {
                    cut = true;
                } else if pending_break && adjacent {
                    cut = false;
                    space = gap > 0.2 * size.min(p.size) && !raw_lines.last().unwrap().last().map(|c| is_space(c.ch)).unwrap_or(true);
                }
            }
            if cut {
                raw_lines.push(vec![]);
            } else if space {
                let prev = raw_lines.last().unwrap().last().unwrap().bx;
                push_char(&mut raw_lines, C { ch: ' ', size, bx: R::new(prev.x1, bx.y0, bx.x0, bx.y1), style: None, generated: true });
            }
            pending_break = false;
        }
        push_char(&mut raw_lines, cc);
    }
    fold_fragments(&mut raw_lines);
    let mut raw_lines: Vec<Vec<C>> = raw_lines.into_iter().flat_map(split_numbers).collect();
    raw_lines.iter_mut().for_each(attach_accents);
    if std::env::var("P2E_DEBUG_LINES").ok().and_then(|v| v.parse::<usize>().ok()) == Some(pno + 1) {
        for l in &raw_lines {
            let t: String = l.iter().map(|c| c.ch).collect();
            let sz: Vec<String> = l.iter().filter(|c| !is_space(c.ch)).take(6).map(|c| format!("{:.1}@{:.0},{:.0}", c.size, c.bx.x0, c.bx.y1)).collect();
            eprintln!("LINE {t:?} {}", sz.join(" "));
        }
    }
    raw_lines.into_iter().filter_map(|cs| make_line(cs, pno)).collect()
}

/// Line numbers (preprints, manuscripts) sit on the text's baseline a little to the left or
/// right: cut a short number separated by a clear gap off either end of a line.
fn split_numbers(cs: Vec<C>) -> Vec<Vec<C>> {
    let vis: Vec<usize> = (0..cs.len()).filter(|&i| !is_space(cs[i].ch) && cs[i].bx.w() > 0.0).collect();
    if vis.len() < 3 {
        return vec![cs];
    }
    let is_num = |idx: &[usize]| idx.len() <= 4 && idx.iter().all(|&i| cs[i].ch.is_ascii_digit());
    // a gap after vis[k]; `num` is the side holding the number: it must be set smaller than the
    // text, or stand well apart (so "1 Introduction" stays whole)
    let cut_at = |k: usize, num_left: bool| {
        let (a, b) = (&cs[vis[k]], &cs[vis[k + 1]]);
        let (num, text) = if num_left { (a, b) } else { (b, a) };
        let gap = b.bx.x0 - a.bx.x1;
        let s = text.size.max(4.0);
        gap > 0.8 * num.size.min(s) && (num.size < 0.85 * s || gap > 1.5 * s)
    };
    let gap_after = |k: usize| {
        let (a, b) = (&cs[vis[k]], &cs[vis[k + 1]]);
        b.bx.x0 - a.bx.x1 > 0.8 * a.size.min(b.size).max(3.0)
    };
    let mut cuts: Vec<usize> = Vec::new(); // split before these char indices
    if let Some(k) = (0..vis.len() - 1).find(|&k| gap_after(k)) {
        if is_num(&vis[..=k]) && cut_at(k, true) {
            cuts.push(vis[k + 1]);
        }
    }
    if let Some(k) = (0..vis.len() - 1).rev().find(|&k| gap_after(k)) {
        if is_num(&vis[k + 1..]) && cut_at(k, false) && cuts.first().map(|&c| vis[k + 1] > c).unwrap_or(true) {
            cuts.push(vis[k + 1]);
        }
    }
    if cuts.is_empty() {
        return vec![cs];
    }
    let mut out = Vec::new();
    let mut cur = Vec::new();
    for (i, c) in cs.into_iter().enumerate() {
        if cuts.contains(&i) && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        cur.push(c);
    }
    out.push(cur);
    out
}

/// Bounding box and average size of a line's visible characters.
fn extent(cs: &[C]) -> Option<(R, f64, usize)> {
    let vis: Vec<&C> = cs.iter().filter(|c| !is_space(c.ch) && c.bx.w() > 0.0).collect();
    if vis.is_empty() {
        return None;
    }
    let r = R::union_all(vis.iter().map(|c| &c.bx));
    Some((r, vis.iter().map(|c| c.size).sum::<f64>() / vis.len() as f64, vis.len()))
}

/// PDFium starts a new line for sub- and superscripts drawn after the rest of a line (common in
/// TeX display math); PDFKit keeps them in place. Put short runs of smaller text back into the
/// line whose box they sit in, at their horizontal position.
fn fold_fragments(lines: &mut Vec<Vec<C>>) {
    let ext: Vec<Option<(R, f64, usize)>> = lines.iter().map(|l| extent(l)).collect();
    let mut target: Vec<Option<usize>> = vec![None; lines.len()];
    for (i, e) in ext.iter().enumerate() {
        let Some((fr, fs, fnn)) = e else { continue };
        if *fnn > 12 {
            continue;
        }
        let accents = lines[i].iter().filter(|c| !is_space(c.ch)).all(|c| combining(c.ch).is_some());
        let mut best: Option<(usize, f64)> = None;
        for (j, h) in ext.iter().enumerate() {
            let Some((hr, hs, hn)) = h else { continue };
            if j == i || *hn <= *fnn || (*fs >= 0.9 * hs && !accents) || target[j].is_some() {
                continue;
            }
            let pad = 0.5 * hs;
            let inside_x = fr.x0 >= hr.x0 - pad && fr.x1 <= hr.x1 + pad;
            let cy = (fr.y0 + fr.y1) / 2.0;
            let inside_y = cy > hr.y0 - 0.25 * hr.h() && cy < hr.y1 + 0.25 * hr.h();
            if inside_x && inside_y {
                let d = (cy - (hr.y0 + hr.y1) / 2.0).abs();
                if best.map(|b| d < b.1).unwrap_or(true) {
                    best = Some((j, d));
                }
            }
        }
        target[i] = best.map(|b| b.0);
    }
    if target.iter().all(|t| t.is_none()) {
        return;
    }
    for i in 0..lines.len() {
        let Some(j) = target[i] else { continue };
        let frag = std::mem::take(&mut lines[i]);
        for c in frag.into_iter().filter(|c| !is_space(c.ch)) {
            let host = &mut lines[j];
            let pos = host
                .iter()
                .rposition(|h| h.bx.w() > 0.0 && !is_space(h.ch) && h.bx.x0 <= c.bx.x0)
                .map(|p| p + 1)
                .unwrap_or(0);
            host.insert(pos, c);
        }
    }
    lines.retain(|l| !l.is_empty());
}

fn c_hyphen(c: &PdfPageTextChar, mb: &MediaBox) -> C {
    let bx = c
        .loose_bounds()
        .map(|r| mb.to_top(r.left().value as f64, r.bottom().value as f64, r.right().value as f64, r.top().value as f64))
        .unwrap_or_default();
    C { ch: '-', size: c.scaled_font_size().value as f64, bx, style: None, generated: false }
}

fn push_char(lines: &mut [Vec<C>], c: C) {
    lines.last_mut().unwrap().push(c);
}

/// Builds a line: dominant size, super-/subscripts, bold/italic spans, math fraction.
fn make_line(chars: Vec<C>, pno: usize) -> Option<PLine> {
    // trim generated/space characters at both ends
    let start = chars.iter().position(|c| !is_space(c.ch))?;
    let end = chars.iter().rposition(|c| !is_space(c.ch))?;
    let chars = &chars[start..=end];
    let boxes: Vec<&R> = chars.iter().filter(|c| !is_space(c.ch) && (c.bx.w() > 0.0 || c.bx.h() > 0.0)).map(|c| &c.bx).collect();
    if boxes.is_empty() {
        return None;
    }
    let bbox = R::union_all(boxes);
    if bbox.h() > 2.5 * bbox.w() && chars.len() > 2 {
        return None; // rotated margin text
    }
    let mut size_count: Vec<(f64, usize)> = Vec::new();
    for c in chars.iter().filter(|c| !is_space(c.ch)) {
        let k = (c.size * 2.0).round() / 2.0;
        match size_count.iter_mut().find(|x| x.0 == k) {
            Some(x) => x.1 += 1,
            None => size_count.push((k, 1)),
        }
    }
    let dom = super::layout::first_max(&size_count, |x| x.1)?.0;
    if dom <= 0.0 {
        return None;
    }
    let mains: Vec<&C> = chars.iter().filter(|c| (c.size - dom).abs() < 0.6 && c.bx.h() > 0.0 && !is_space(c.ch)).collect();
    let main_center = if mains.is_empty() {
        bbox.y0 + bbox.h() / 2.0
    } else {
        mains.iter().map(|c| (c.bx.y0 + c.bx.y1) / 2.0).sum::<f64>() / mains.len() as f64
    };
    let main_height =
        if mains.is_empty() { bbox.h() } else { mains.iter().map(|c| c.bx.h()).sum::<f64>() / mains.len() as f64 };
    let mut spans: Vec<Span> = Vec::new();
    let (mut n_bold, mut n_math, mut n) = (0usize, 0usize, 0usize);
    let mut last: Option<Style> = None;
    let mut text = String::new();
    for c in chars {
        let space = is_space(c.ch);
        let ch = if space { ' ' } else { c.ch };
        let run = if space { last } else { c.style.or(last) };
        if !space {
            last = run;
        }
        text.push(ch);
        let small = c.size > 0.0 && c.size < 0.8 * dom;
        let center = (c.bx.y0 + c.bx.y1) / 2.0;
        // some PDFs give every character the full line box: then fall back to "small citation-like text"
        let flat_boxes = c.bx.h() > 0.85 * main_height;
        let cite_like = matches!(ch, '0'..='9' | ',' | '–' | '-' | '−' | '*' | '†' | '‡' | '§' | 'a'..='e');
        let sup = !space && small && if flat_boxes { cite_like } else { center < main_center - 0.12 * dom };
        let sub = !space && small && !flat_boxes && center > main_center + 0.12 * dom;
        let st = run.unwrap_or_default();
        if !space {
            n += 1;
            n_bold += st.bold as usize;
            n_math += st.math as usize;
        }
        let sp = Span { text: ch.to_string(), bold: st.bold, italic: st.italic, sup, sub };
        match spans.last_mut() {
            Some(l)
                if (space && !l.sup && !l.sub)
                    || (l.bold == sp.bold && l.italic == sp.italic && l.sup == sp.sup && l.sub == sp.sub) =>
            {
                l.text.push(ch)
            }
            _ => spans.push(sp),
        }
    }
    let nfc = icu_normalizer::ComposingNormalizerBorrowed::new_nfc();
    let text = nfc.normalize(&text).into_owned();
    for sp in spans.iter_mut() {
        if sp.text.chars().any(|c| ('\u{300}'..='\u{36f}').contains(&c)) {
            sp.text = nfc.normalize(&sp.text).into_owned();
        }
    }
    let nn = n.max(1) as f64;
    Some(PLine { text, bbox, size: dom, bold: n_bold as f64 / nn, math: n_math as f64 / nn, spans, page: pno })
}

/// Images and painted paths on a page, descending into form XObjects.
pub fn graphics(page: &PdfPage, mb: &MediaBox) -> PageGraphics {
    let mut g = PageGraphics::default();
    let identity = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    for o in page.objects().iter() {
        walk(&o, identity, mb, &mut g, 0);
    }
    g
}

type Affine = [f64; 6];

fn apply(m: &Affine, x: f64, y: f64) -> (f64, f64) {
    (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
}

/// `inner` followed by `outer`.
fn concat(inner: &Affine, outer: &Affine) -> Affine {
    [
        inner[0] * outer[0] + inner[1] * outer[2],
        inner[0] * outer[1] + inner[1] * outer[3],
        inner[2] * outer[0] + inner[3] * outer[2],
        inner[2] * outer[1] + inner[3] * outer[3],
        inner[4] * outer[0] + inner[5] * outer[2] + outer[4],
        inner[4] * outer[1] + inner[5] * outer[3] + outer[5],
    ]
}

fn walk(o: &PdfPageObject, ctm: Affine, mb: &MediaBox, g: &mut PageGraphics, depth: usize) {
    let rect = |o: &PdfPageObject| -> Option<R> {
        let q = o.bounds().ok()?;
        let pts = [(q.x1(), q.y1()), (q.x2(), q.y2()), (q.x3(), q.y3()), (q.x4(), q.y4())]
            .map(|(x, y)| apply(&ctm, x.value as f64, y.value as f64));
        let (l, r) = pts.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |a, p| (a.0.min(p.0), a.1.max(p.0)));
        let (b, t) = pts.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |a, p| (a.0.min(p.1), a.1.max(p.1)));
        Some(mb.to_top(l, b, r, t))
    };
    match o {
        PdfPageObject::Image(_) => {
            if let Some(r) = rect(o) {
                g.images.push(r);
            }
        }
        PdfPageObject::Path(p) => {
            // a path that is neither filled nor stroked only clips
            let filled = p.fill_mode().map(|m| m != PdfPathFillMode::None).unwrap_or(true);
            let stroked = p.is_stroked().unwrap_or(true);
            if filled || stroked {
                if let Some(r) = rect(o) {
                    g.drawings.push(r);
                }
            }
        }
        PdfPageObject::XObjectForm(f) if depth < 6 => {
            let inner = f
                .matrix()
                .map(|m| [m.a() as f64, m.b() as f64, m.c() as f64, m.d() as f64, m.e() as f64, m.f() as f64])
                .unwrap_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
            let m = concat(&inner, &ctm);
            for child in f.iter() {
                walk(&child, m, mb, g, depth + 1);
            }
        }
        _ => {}
    }
}

/// Reads every page into [`Page`]s (cover sheets come out empty).
pub fn pages(doc: &PdfDocument, log: &dyn Fn(&str)) -> Vec<Page> {
    let n = doc.pages().len() as usize;
    let mut out = Vec::with_capacity(n);
    for (pno, page) in doc.pages().iter().enumerate() {
        let mb = media_box(&page);
        let mut p = Page { width: mb.width, height: mb.height, ..Default::default() };
        if let Ok(text) = page.text() {
            if !is_cover_sheet(&text.all()) {
                p.graphics = graphics(&page, &mb);
                p.lines = extract_lines(&text, &mb, pno);
            }
        }
        out.push(p);
        if pno % 5 == 4 {
            log(&format!("Reading page {} of {}", pno + 1, n));
        }
    }
    out
}

/// Title, author, subject and creation year from the document information dictionary.
pub fn doc_info(doc: &PdfDocument) -> DocInfo {
    let md = doc.metadata();
    let get = |t| md.get(t).map(|v| v.value().to_string()).unwrap_or_default();
    let created = get(PdfDocumentMetadataTagType::CreationDate);
    let year = created.trim_start_matches("D:").get(0..4).and_then(|y| y.parse::<i32>().ok());
    DocInfo {
        title: get(PdfDocumentMetadataTagType::Title),
        author: get(PdfDocumentMetadataTagType::Author),
        subject: get(PdfDocumentMetadataTagType::Subject),
        year,
    }
}

/// Plain text of one page.
pub fn page_text(doc: &PdfDocument, i: usize) -> String {
    doc.pages().get(i as i32).ok().and_then(|p| p.text().ok().map(|t| t.all())).unwrap_or_default()
}

/// Renders crops of pages; caches the last page bitmap since a page often has several crops.
pub struct PdfiumRenderer<'a, 'b> {
    pub doc: &'a PdfDocument<'b>,
    cache: Option<(usize, u32, image::RgbImage, f64, f64)>,
}

impl<'a, 'b> PdfiumRenderer<'a, 'b> {
    pub fn new(doc: &'a PdfDocument<'b>) -> Self {
        PdfiumRenderer { doc, cache: None }
    }
}

impl Renderer for PdfiumRenderer<'_, '_> {
    fn render(&mut self, pno: usize, r: R, scale: f64) -> Option<image::RgbImage> {
        let key = (scale * 1000.0).round() as u32;
        if !matches!(&self.cache, Some((p, k, ..)) if *p == pno && *k == key) {
            let page = self.doc.pages().get(pno as i32).ok()?;
            let mb = media_box(&page);
            // PDFium renders the crop box; find where it sits in the media box
            let crop = page.boundaries().crop().map(|b| b.bounds).ok();
            let (cx, cy, cw, ch) = match crop {
                Some(c) => (
                    c.left().value as f64 - mb.left,
                    mb.top - c.top().value as f64,
                    c.width().value as f64,
                    c.height().value as f64,
                ),
                None => (0.0, 0.0, mb.width, mb.height),
            };
            let w = (cw * scale).round().max(1.0) as i32;
            let h = (ch * scale).round().max(1.0) as i32;
            let cfg = PdfRenderConfig::new().set_target_size(w, h).render_form_data(true);
            let bmp = page.render_with_config(&cfg).ok()?;
            let img = bmp.as_image().ok()?.to_rgb8();
            self.cache = Some((pno, key, img, cx, cy));
        }
        let (_, _, img, cx, cy) = self.cache.as_ref()?;
        let x0 = (((r.x0 - cx) * scale).round().max(0.0) as u32).min(img.width());
        let y0 = (((r.y0 - cy) * scale).round().max(0.0) as u32).min(img.height());
        let w = ((r.w() * scale).round() as u32).min(img.width() - x0);
        let h = ((r.h() * scale).round() as u32).min(img.height() - y0);
        if w == 0 || h == 0 {
            return None;
        }
        Some(image::imageops::crop_imm(img, x0, y0, w, h).to_image())
    }
}
