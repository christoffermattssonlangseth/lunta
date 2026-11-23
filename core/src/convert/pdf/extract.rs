//! Reading a PDF with PDFium: text lines with per-character fonts, sizes and positions, image
//! placements and vector drawings, document info, and rendering crops. Replaces PDFKit plus
//! `PDFContentScanner` (PDFContent.swift): PDFium already exposes real font names and flags per
//! character and inserts the word spaces a content stream only implies by moving, so neither
//! the content-stream walk nor its gap detection is needed.

use super::layout::{re, DocInfo, PLine, Page, PageGraphics, Renderer, Span, R};
use pdfium_render::prelude::*;
use std::collections::{HashMap, HashSet};

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
    /// The box's left and top in PDF user space.
    pub fn origin(&self) -> (f64, f64) {
        (self.left, self.top)
    }

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
    /// Computer Modern roman (CMR10, CMBX10): math where the text is set in another font
    cm_roman: bool,
    /// a monospaced font (LMMono, Courier, Inconsolata): code
    mono: bool,
}

fn style_for(name: &str, force_bold: bool, italic_flag: bool) -> Style {
    let clean = name.split_once('+').map(|x| x.1).unwrap_or(name);
    let bold = force_bold
        || layout_re::bold().is_match(clean);
    let italic = italic_flag || layout_re::italic().is_match(clean);
    let math = layout_re::math().is_match(clean) && !layout_re::stix_text().is_match(clean);
    let cm_roman = re!(r"^(CMR|CMBX|CMSL|CMB)\d").is_match(clean);
    let mono = re!(r"(?i)mono|courier|^CMTT|^SFTT|consol|menlo|inconsolata|typewriter|SourceCodePro|FiraCode|^LMTT").is_match(clean);
    Style { bold, italic, math, cm_roman, mono }
}

pub(crate) mod layout_re {
    use super::super::layout::re;
    use regex::Regex;
    pub fn bold() -> &'static Regex {
        re!(r"(?i:bold|black|heavy|semibold|demi|-bd\b)|-(H|B|Bd|Hv|Hvy|Blk|SB|Sb|BdIt|BI|HI|Bol)$|[a-z](Bd|Blk|Hv)$")
    }
    pub fn italic() -> &'static Regex {
        re!(r"(?i:ital|oblique|slant)|-(I|It|MI|LI|BI|HI|BdIt|SBIt|Ita)$|[a-z]It$")
    }
    /// Math fonts. STIX counts, except STIXTwoText: the body text of Wiley's journals (their math is
    /// STIXTwoMath).
    pub fn math() -> &'static Regex {
        re!(r"(?i)CMMI|CMSY|CMEX|MSAM|MSBM|Math|Symbol|STIX|rsfs|EUFM|CMBSY|LMMath|esint|MTMI|MTSY|txsy|txmi|pxmi|Euclid")
    }
    pub fn stix_text() -> &'static Regex {
        re!(r"(?i)STIXTwo-?Text")
    }
    /// A figure/table label on its own: "FIGURE 1", "Table S2"
    pub fn label() -> &'static Regex {
        re!(concat!(r"(?i)^(Figure|Fig\.|Scheme|Table|Box)\s*", super::super::layout::label_number!(), r"[A-Za-z]?$"))
    }
    /// A section number on its own: "1", "2.4"
    pub fn number() -> &'static Regex {
        re!(r"^\d+(\.\d+)*\.?$")
    }
}

/// One character as PDFium reports it, in top-left page coordinates.
#[derive(Clone)]
struct C {
    ch: char,
    size: f64,
    bx: R,
    /// the baseline the glyph is set on (its origin), top-left page coordinates; NaN if unknown
    base: f64,
    style: Option<Style>,
    generated: bool,
}

/// Spacing accents TeX draws as glyphs of their own → the combining mark. Elsevier's accent font
/// (AdvP4C4E59) has its dieresis at the code of "€" ("J€akel"): it counts only where it sits over a
/// letter, as accents do (see `attach_accents`).
fn combining(c: char) -> Option<char> {
    Some(match c {
        '€' => '\u{308}',
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
        // generated spaces between the accent's old place and its letter go, unless the letters
        // either side stand a word space apart
        let (lo, hi) = if b < i { (b + 1, i) } else { (i, b) };
        let mut removed_before_b = 0;
        for k in (lo..hi).rev() {
            let apart = k > 0
                && k + 1 < cs.len()
                && cs[k - 1].bx.w() > 0.0
                && cs[k + 1].bx.w() > 0.0
                && cs[k + 1].bx.x0 - cs[k - 1].bx.x1 > 0.15 * cs[k - 1].size.max(cs[k + 1].size);
            if cs[k].generated && is_space(cs[k].ch) && !apart {
                cs.remove(k);
                if k < b {
                    removed_before_b += 1;
                }
            }
        }
        let b = b - removed_before_b;
        acc.bx = cs[b].bx;
        // TeX puts accents on a dotless i or j
        match cs[b].ch {
            'ı' => cs[b].ch = 'i',
            'ȷ' => cs[b].ch = 'j',
            _ => {}
        }
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
pub fn extract_lines(text: &PdfPageText, mb: &MediaBox, pno: usize, hidden: &Hidden) -> Vec<PLine> {
    let mut styles: HashMap<(String, bool, bool), Style> = HashMap::new();
    let mut symbol_fonts: HashMap<String, Option<&'static [(&'static str, &'static str)]>> = HashMap::new();
    let mut raw_lines: Vec<Vec<C>> = vec![vec![]];
    let mut pending_break = false;
    // inside a run of text the page does not show (and the spaces and breaks PDFium puts after it)
    let mut in_hidden = false;
    // accents drawn away from where the text has got to (Elsevier draws them after the line, or
    // after the word): set aside as lines of their own, for `fold_fragments` to put back over their
    // letters; the spaces PDFium makes around them are decided again from where the letters stand
    let mut stray: Vec<Vec<C>> = Vec::new();
    let mut after_stray = false;
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
        let Some(mut ch) = c.unicode_char() else { continue };
        // a symbol font's glyph named like a letter ("d" for Cell's bullets): what it shows
        if !is_space(ch) && (ch as u32) >= 0x20 {
            let name = c.font_name();
            let table = match symbol_fonts.get(&name) {
                Some(t) => *t,
                None => {
                    let t = super::symbols::family(&name);
                    symbol_fonts.insert(name, t);
                    t
                }
            };
            if let Some(m) = table.and_then(|t| super::symbols::map(t, ch)).and_then(|m| m.chars().next()) {
                ch = m;
            }
        }
        if after_stray {
            if c.is_generated().unwrap_or(false) && is_space(ch) {
                continue;
            }
            after_stray = false;
        }
        if !hidden.is_empty() {
            if c.is_generated().unwrap_or(false) {
                if in_hidden {
                    continue;
                }
            } else {
                in_hidden = c.text_object().ok().and_then(|o| o.bounds().ok()).map(|q| hidden.contains(&q)).unwrap_or(false);
                if in_hidden {
                    // what follows is decided like the start of a new line
                    pending_break |= !raw_lines.last().unwrap().is_empty();
                    continue;
                }
            }
        }
        if ch == '\r' || ch == '\n' {
            // decided at the next character: PDFium also breaks around inline super-/subscripts
            pending_break = !raw_lines.last().unwrap().is_empty();
            continue;
        }
        if (ch as u32) < 0x20 && ch != '\t' || ch == '\u{fffe}' || ch == '\u{feff}' {
            // PDFium marks a line-end hyphen as U+0002 (and emits no line break after it); in
            // symbol fonts U+0002 is just an unmapped glyph
            if ch == '\u{2}' && c.is_hyphen().unwrap_or(false) {
                let mut h = c_hyphen(&c, mb);
                // a discretionary hyphen drawn on its own (InDesign): the word goes on in the next
                // line; mark it so the lines are joined without the hyphen (see `layout::GLUE`)
                if is_soft_hyphen(&c, &h) {
                    h.ch = super::layout::GLUE;
                }
                push_char(&mut raw_lines, h);
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
        let base = c.origin_y().map(|y| mb.top - y.value as f64).unwrap_or(f64::NAN);
        let cc = C { ch, size, bx, base, style, generated };
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
            if cut && combining(ch).is_some() && !generated && raw_lines.last().unwrap().iter().any(|c| !is_space(c.ch)) {
                let line = raw_lines.last_mut().unwrap();
                if line.last().map(|c| c.generated && is_space(c.ch)).unwrap_or(false) {
                    line.pop();
                }
                stray.push(vec![cc]);
                after_stray = true;
                pending_break = true;
                continue;
            }
            if cut {
                raw_lines.push(vec![]);
            } else if space {
                let prev = raw_lines.last().unwrap().last().unwrap().bx;
                push_char(&mut raw_lines, C { ch: ' ', size, bx: R::new(prev.x1, bx.y0, bx.x0, bx.y1), base: f64::NAN, style: None, generated: true });
            }
            pending_break = false;
        }
        push_char(&mut raw_lines, cc);
    }
    raw_lines.extend(stray);
    fold_fragments(&mut raw_lines);
    let mut raw_lines: Vec<Vec<C>> = raw_lines.into_iter().flat_map(split_numbers).collect();
    join_labels(&mut raw_lines);
    raw_lines.iter_mut().for_each(attach_accents);
    if std::env::var("P2E_DEBUG_LINES").ok().and_then(|v| v.parse::<usize>().ok()) == Some(pno + 1) {
        for l in &raw_lines {
            let t: String = l.iter().map(|c| c.ch).collect();
            let sz: Vec<String> = l.iter().filter(|c| !is_space(c.ch)).take(6).map(|c| format!("{:.1}@{:.0},{:.0}", c.size, c.bx.x0, c.bx.y1)).collect();
            eprintln!("LINE {t:?} {}", sz.join(" "));
        }
    }
    let mut out = Vec::new();
    for cs in raw_lines {
        // a display equation's number set apart at the line's end: a line of its own, for the
        // equations to be found by (see `equations.rs`)
        let (head, tag) = split_equation_tag(cs);
        out.extend(make_line(head, pno));
        if let Some(mut t) = tag.and_then(|t| make_line(t, pno)) {
            t.tag_split = true;
            out.push(t);
        }
    }
    out
}

/// A line whose text ends in an equation number ("(7.11)") set apart from the rest by a wide gap
/// (0.8 em or more): the line without it, and the number.
fn split_equation_tag(cs: Vec<C>) -> (Vec<C>, Option<Vec<C>>) {
    let Some(end) = cs.iter().rposition(|c| !is_space(c.ch)) else { return (cs, None) };
    if cs[end].ch != ')' {
        return (cs, None);
    }
    let Some(open) = cs[..end].iter().rposition(|c| c.ch == '(') else { return (cs, None) };
    if end - open > 12 {
        return (cs, None);
    }
    let tag: String = cs[open..=end].iter().map(|c| c.ch).collect();
    if !super::equations::is_equation_tag(&tag) {
        return (cs, None);
    }
    let Some(p) = cs[..open].iter().rposition(|c| !is_space(c.ch)) else { return (cs, None) };
    let (a, b) = (cs[p].bx, cs[open].bx);
    let size = cs[open..=end].iter().map(|c| c.size).fold(cs[p].size, f64::max).max(4.0);
    if !(a.w() > 0.0 && b.w() > 0.0 && b.x0 - a.x1 >= 0.8 * size) {
        return (cs, None);
    }
    let mut cs = cs;
    let tag = cs.split_off(open);
    (cs, Some(tag))
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

/// A label set apart from its line by a wide space ("FIGURE 1   (a) Hairpin…", "2.4   High
/// Specificity…", Wiley's "1 | Introduction"): PDFium gives it a line of its own; join it to the
/// text beside it on the same baseline. A bare number must be bold, like the heading after it (so
/// a manuscript's line numbers stay apart).
fn join_labels(lines: &mut Vec<Vec<C>>) {
    let mut i = 0;
    while i + 1 < lines.len() {
        let (a, b) = (&lines[i], &lines[i + 1]);
        let vis = |cs: &[C]| cs.iter().filter(|c| !is_space(c.ch) && c.bx.w() > 0.0).count();
        let text: String = a.iter().map(|c| c.ch).collect::<String>().trim().to_string();
        let ok = (|| {
            let (ea, _, _) = extent(a)?;
            let (eb, sb, _) = extent(b)?;
            let first = b.iter().find(|c| !is_space(c.ch) && c.bx.w() > 0.0)?;
            let s = sb.max(4.0);
            let gap = eb.x0 - ea.x1;
            let same_line = ea.y1.min(eb.y1) - ea.y0.max(eb.y0) > 0.5 * ea.h().min(eb.h());
            let caption = layout_re::label().is_match(&text);
            let bold = |c: &C| c.style.map(|st| st.bold).unwrap_or(false);
            let number = layout_re::number().is_match(&text)
                && a.iter().filter(|c| !is_space(c.ch) && c.bx.w() > 0.0).all(bold)
                && bold(first);
            Some(vis(a) > 0 && same_line && gap > 0.0 && gap < 3.0 * s && (caption || (number && gap < 2.5 * s)))
        })()
        .unwrap_or(false);
        if ok {
            let b = lines.remove(i + 1);
            let a = &mut lines[i];
            while a.last().map(|c| is_space(c.ch)).unwrap_or(false) {
                a.pop();
            }
            let (prev, next) = (a.last().unwrap().bx, b.iter().find(|c| c.bx.w() > 0.0).map(|c| c.bx).unwrap_or(a.last().unwrap().bx));
            let size = a.last().unwrap().size;
            a.push(C { ch: ' ', size, bx: R::new(prev.x1, next.y0, next.x0, next.y1), base: f64::NAN, style: None, generated: true });
            a.extend(b);
        } else {
            i += 1;
        }
    }
}

/// Bounding box and average size of a line's visible characters (a drop cap, set in far larger
/// type than the rest of the line, left out).
fn extent(cs: &[C]) -> Option<(R, f64, usize)> {
    let vis: Vec<&C> = cs.iter().filter(|c| !is_space(c.ch) && c.bx.w() > 0.0).collect();
    if vis.is_empty() {
        return None;
    }
    let mut sizes: Vec<f64> = vis.iter().map(|c| c.size).collect();
    sizes.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = sizes[sizes.len() / 2];
    let vis: Vec<&C> = vis.into_iter().filter(|c| c.size <= 2.5 * median).collect();
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
    C { ch: '-', size: c.scaled_font_size().value as f64, bx, base: f64::NAN, style: None, generated: false }
}

/// A line-end hyphen PDFium found that is a hyphenation (soft) hyphen rather than part of the
/// word: drawn as a text object of its own, with a glyph box far shorter than the line. Layout
/// programs that do this (InDesign) draw real hyphens within the word's text run.
fn is_soft_hyphen(c: &PdfPageTextChar, h: &C) -> bool {
    h.bx.h() > 0.0
        && h.bx.h() < 0.4 * h.size
        && c.text_object().map(|o| o.text().trim().chars().count() <= 1).unwrap_or(false)
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
    let dom = super::layout::first_max(&size_count, |x| (x.1, x.0))?.0;
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
    // the line's baseline: where its characters in the main size are set (their origins); a smaller
    // character is raised or lowered by its own baseline against it (a box's middle says little where
    // the boxes are the whole line's, and "citation-like" said every small digit was raised: β₃ as β³)
    let mut bases: Vec<f64> = mains.iter().map(|c| c.base).filter(|b| b.is_finite()).collect();
    bases.sort_by(|a, b| a.total_cmp(b));
    let main_base = bases.get(bases.len() / 2).copied();
    let mut spans: Vec<Span> = Vec::new();
    let (mut n_bold, mut n_math, mut n_cmr, mut n_mono, mut n) = (0usize, 0usize, 0usize, 0usize, 0usize);
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
        let (mut sup, mut sub) = match main_base {
            Some(mb) if !space && small && c.base.is_finite() => (c.base < mb - 0.15 * dom, c.base > mb + 0.08 * dom),
            _ => (
                !space && small && if flat_boxes { cite_like } else { center < main_center - 0.12 * dom },
                !space && small && !flat_boxes && center > main_center + 0.12 * dom,
            ),
        };
        // a prime is set raised already: not a script of its own (θ′, f′ᵢ)
        if matches!(ch, '′' | '″' | '‴') {
            sup = false;
            sub = false;
        }
        let st = run.unwrap_or_default();
        if !space {
            n += 1;
            n_bold += st.bold as usize;
            n_math += st.math as usize;
            n_cmr += st.cm_roman as usize;
            n_mono += st.mono as usize;
        }
        let sp = Span { text: ch.to_string(), bold: st.bold, italic: st.italic, sup, sub, note_ref: None };
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
    Some(PLine {
        text,
        bbox,
        size: dom,
        bold: n_bold as f64 / nn,
        math: n_math as f64 / nn,
        spans,
        page: pno,
        cmr: n_cmr as f64 / nn,
        mono: n_mono as f64 / nn,
        tag_split: false,
    })
}

/// Text objects a page holds but does not show, by their bounds as PDFium reports them (in the
/// space of the form they are in): text in a form XObject outside the form's box or clip, or under
/// an image the form draws after it. MDPI includes each figure as a whole page of the manuscript
/// ("cells-2381855-g001.pdf", the figure and the manuscript's text around it) cut to the figure by
/// the form's box; PDFium reads the cut-away text too ("Cells 2023, 12, x FOR PEER REVIEW", a
/// second copy of the caption).
#[derive(Debug, Default)]
pub struct Hidden {
    hidden: HashSet<[u32; 4]>,
    /// objects drawn on the page itself, in case one has the same bounds as a hidden one
    shown: HashSet<[u32; 4]>,
}

fn quad_key(q: &PdfQuadPoints) -> [u32; 4] {
    [q.left().value, q.bottom().value, q.right().value, q.top().value].map(f32::to_bits)
}

impl Hidden {
    pub fn is_empty(&self) -> bool {
        self.hidden.is_empty()
    }

    fn contains(&self, q: &PdfQuadPoints) -> bool {
        let k = quad_key(q);
        self.hidden.contains(&k) && !self.shown.contains(&k)
    }
}

/// Images and painted paths on a page, descending into form XObjects, and the text the page does
/// not show (see [`Hidden`]).
pub fn graphics(page: &PdfPage, mb: &MediaBox) -> (PageGraphics, Hidden) {
    let mut w = Walk::default();
    let identity = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    for o in page.objects().iter() {
        walk(&o, identity, None, mb, &mut w, 0);
    }
    // text in a form that an image the form draws later covers is not shown either (the
    // manuscript page's text under the figure it was included for); not under an image of the
    // whole page, which may be a scan drawn over its own text layer
    let page_area = mb.width * mb.height;
    let covers = w.form_images.iter().filter(|(img, _)| img.area() < 0.8 * page_area).collect::<Vec<_>>();
    for (key, r, at) in &w.form_texts {
        let (cx, cy) = (r.cx(), (r.y0 + r.y1) / 2.0);
        if covers.iter().any(|(img, k)| k > at && img.contains(&R::new(cx, cy, cx, cy), 0.0)) {
            w.hidden.hidden.insert(*key);
        }
    }
    (w.g, w.hidden)
}

#[derive(Default)]
struct Walk {
    g: PageGraphics,
    hidden: Hidden,
    /// text objects shown in forms: key, page rectangle, number of objects drawn before
    form_texts: Vec<([u32; 4], R, usize)>,
    /// images drawn in forms (the part that shows), with the number of objects drawn before
    form_images: Vec<(R, usize)>,
    drawn: usize,
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

/// Bounding box of points in the object's space, in top-left page coordinates.
fn rect_of(pts: impl IntoIterator<Item = (f64, f64)>, ctm: &Affine, mb: &MediaBox) -> Option<R> {
    let (mut l, mut b, mut r, mut t) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for (x, y) in pts {
        let (x, y) = apply(ctm, x, y);
        l = l.min(x);
        r = r.max(x);
        b = b.min(y);
        t = t.max(y);
    }
    (l.is_finite() && b.is_finite() && r.is_finite() && t.is_finite()).then(|| mb.to_top(l, b, r, t))
}

/// Bounding box of a path's points (control points included), in top-left page coordinates.
fn path_rect(p: &PdfPagePathObject, ctm: &Affine, mb: &MediaBox) -> Option<R> {
    let m = p.matrix().ok()?;
    let pm = [m.a() as f64, m.b() as f64, m.c() as f64, m.d() as f64, m.e() as f64, m.f() as f64];
    let full = concat(&pm, ctm);
    let segs = p.segments();
    let pts = (0..segs.len()).filter_map(|i| segs.get(i).ok()).map(|sg| (sg.x().value as f64, sg.y().value as f64));
    rect_of(pts, &full, mb)
}

/// The box an object's clip path keeps (the intersection of its paths' boxes), in top-left page
/// coordinates. In a form, PDFium clips each object to the form's bounding box.
fn clip_rect(o: &PdfPageObject, ctm: &Affine, mb: &MediaBox) -> Option<R> {
    let cp = o.get_clip_path()?;
    let mut out: Option<R> = None;
    for path in cp.iter() {
        let pts: Vec<(f64, f64)> = path.iter().map(|sg| (sg.x().value as f64, sg.y().value as f64)).collect();
        let Some(r) = rect_of(pts, ctm, mb) else { continue };
        out = Some(out.map(|o| o.intersection(&r)).unwrap_or(r));
    }
    out
}

/// `clip` is what the enclosing forms let show (top-left page coordinates); None on the page itself.
fn walk(o: &PdfPageObject, ctm: Affine, clip: Option<R>, mb: &MediaBox, w: &mut Walk, depth: usize) {
    let rect = |o: &PdfPageObject| -> Option<R> {
        let q = o.bounds().ok()?;
        rect_of([(q.x1(), q.y1()), (q.x2(), q.y2()), (q.x3(), q.y3()), (q.x4(), q.y4())].map(|(x, y)| (x.value as f64, y.value as f64)), &ctm, mb)
    };
    // in a form, only what lies in the form's box (and the object's own clip) shows
    let clip = if depth > 0 {
        match (clip, clip_rect(o, &ctm, mb)) {
            (Some(a), Some(b)) => Some(a.intersection(&b)),
            (a, b) => a.or(b),
        }
    } else {
        None
    };
    let visible = |r: R| -> Option<R> {
        let r = clip.map(|c| r.intersection(&c)).unwrap_or(r);
        (r.w() >= 0.0 && r.h() >= 0.0).then_some(r)
    };
    w.drawn += 1;
    match o {
        PdfPageObject::Text(_) => {
            let (Ok(q), Some(r)) = (o.bounds(), rect(o)) else { return };
            let key = quad_key(&q);
            let cy = (r.y0 + r.y1) / 2.0;
            if depth == 0 {
                w.hidden.shown.insert(key);
            } else if clip.is_some_and(|c| cy < c.y0 || cy > c.y1 || r.x1 < c.x0 || r.x0 > c.x1) {
                // its middle above or below the box, or all of it to one side: not shown
                w.hidden.hidden.insert(key);
            } else {
                // (PDFium leaves out the clip of an object that lies wholly inside it)
                w.form_texts.push((key, r, w.drawn));
            }
        }
        PdfPageObject::Image(_) => {
            if let Some(r) = rect(o).and_then(visible) {
                if r.w() > 0.0 && r.h() > 0.0 {
                    if depth > 0 {
                        w.form_images.push((r, w.drawn));
                    }
                    w.g.images.push(r);
                }
            }
        }
        PdfPageObject::Path(p) => {
            // a path that is neither filled nor stroked only clips
            let filled = p.fill_mode().map(|m| m != PdfPathFillMode::None).unwrap_or(true);
            let stroked = p.is_stroked().unwrap_or(true);
            if filled || stroked {
                // PDFium's object bounds grow a stroked path by its line width scaled through the
                // path's matrix, which can turn a thin rule into something taller than the page:
                // where they overshoot the path's own points by far, the points are used
                let r = match (rect(o), path_rect(p, &ctm, mb)) {
                    (Some(b), Some(pts)) => {
                        let over = (pts.x0 - b.x0).max(b.x1 - pts.x1).max(pts.y0 - b.y0).max(b.y1 - pts.y1);
                        Some(if over > 20.0 { pts } else { b })
                    }
                    (b, pts) => b.or(pts),
                };
                if let Some(r) = r.and_then(visible) {
                    w.g.drawings.push(r);
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
                walk(&child, m, clip, mb, w, depth + 1);
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
                let (g, hidden) = graphics(&page, &mb);
                p.graphics = g;
                if std::env::var("P2E_DEBUG_GFX").ok().and_then(|v| v.parse::<usize>().ok()) == Some(pno + 1) {
                    for r in &p.graphics.images { eprintln!("IMG {:?}", r); }
                    for r in &p.graphics.drawings { if r.area() > 2000.0 { eprintln!("DRAW {:?}", r); } }
                }
                p.lines = extract_lines(&text, &mb, pno, &hidden);
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

#[cfg(test)]
mod script_tests {
    use super::*;

    fn c(ch: char, size: f64, x: f64, bx_h: f64, base: f64) -> C {
        // every character the full line's box (as some PDFs give them): only the baseline tells
        C { ch, size, bx: R::new(x, 100.0, x + 0.5 * size, 100.0 + bx_h), base, style: None, generated: false }
    }

    #[test]
    fn scripts_go_by_the_glyphs_baselines() {
        // β₃ + x² and θ′: the 3 set 2 pt lower, the 2 3.5 pt higher, the prime raised but no script
        let chars = vec![
            c('β', 10.0, 0.0, 14.0, 110.0),
            c('3', 7.0, 5.0, 14.0, 112.0),
            c(' ', 10.0, 9.0, 14.0, f64::NAN),
            c('+', 10.0, 12.0, 14.0, 110.0),
            c(' ', 10.0, 17.0, 14.0, f64::NAN),
            c('x', 10.0, 20.0, 14.0, 110.0),
            c('2', 7.0, 25.0, 14.0, 106.5),
            c(' ', 10.0, 29.0, 14.0, f64::NAN),
            c('θ', 10.0, 32.0, 14.0, 110.0),
            c('′', 7.0, 37.0, 14.0, 106.0),
        ];
        let l = make_line(chars, 0).unwrap();
        assert!(l.spans.iter().any(|s| s.text == "3" && s.sub && !s.sup), "{:?}", l.spans);
        assert!(l.spans.iter().any(|s| s.text == "2" && s.sup && !s.sub), "{:?}", l.spans);
        assert!(l.spans.iter().all(|s| !(s.text.contains('′') && (s.sup || s.sub))), "{:?}", l.spans);
    }
}
