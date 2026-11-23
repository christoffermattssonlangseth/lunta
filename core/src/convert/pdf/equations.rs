//! Display equations found line by line, before a page's paragraphs are made (a port of
//! `PDFEquations.swift`): a line ending in an equation's number ("(7.9)", "(C.8)", "(3)") set apart
//! at the column's right edge, with the formula's lines beside it and the rows, fractions and
//! limits above and below it; and, in a book, lines of math centred in the column. Each becomes one
//! equation, cropped as an image to its ink without its number; the number stays text, after it,
//! for cross-references ("equation 7.9") to find.

use super::layout::*;
use super::super::BookImage;

/// An equation's number as set at the right of a display: "(7.9)", "(C.8)", "(12)", "(4.6a)", "(S3)".
pub fn is_equation_tag(t: &str) -> bool {
    tag_number(t).is_some()
}

/// The number inside an equation's tag: "(7.9)" → "7.9".
pub fn tag_number(t: &str) -> Option<String> {
    re!(r"^\(((?:[A-Z]?\d{1,3}(?:\.\d{1,3}){0,2}|[A-Z]\.\d{1,3})[a-z]?)\)$").captures(trim(t)).map(|c| c[1].to_string())
}

fn math_char(c: char) -> bool {
    "=<>≤≥≈≡∝∼≃+−×·∑∏∫∂√∞∇∈∉⊂⊆∪∩→←↔⇒⇔∀∃^_|‖⟨⟩⌈⌉⌊⌋±∓∘⊙⊗⊕".contains(c)
        || ('\u{E000}'..='\u{F8FF}').contains(&c)
        || ('\u{391}'..='\u{3C9}').contains(&c)
}

fn v_overlap(a: &R, b: &R) -> f64 {
    a.y1.min(b.y1) - a.y0.max(b.y0)
}

fn prose_words(t: &str) -> usize {
    re!(r"\b[a-z]{3,}\b").find_iter(t).count()
}

/// The text columns' left and right ends among `lines`: the ends shared by their lines of running text.
pub fn edges_of<'a>(lines: impl IntoIterator<Item = &'a PLine>, body: f64) -> (Vec<f64>, Vec<f64>) {
    let mut l: std::collections::HashMap<i64, usize> = Default::default();
    let mut r: std::collections::HashMap<i64, usize> = Default::default();
    let mut n = 0;
    for x in lines {
        if (x.size - body).abs() <= 1.6 && count(&x.text) > 40 && is_prose(&x.text) {
            *l.entry(x.bbox.x0.round() as i64).or_default() += 1;
            *r.entry(x.bbox.x1.round() as i64).or_default() += 1;
            n += 1;
        }
    }
    let need = 3.max(n / 12);
    // (neighbouring values counted together: justified lines end within a point)
    let peaks = |h: &std::collections::HashMap<i64, usize>| {
        let mut v: Vec<(i64, usize)> = h.iter().map(|(k, c)| (*k, *c)).collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let mut out: Vec<f64> = Vec::new();
        for (x, _) in v {
            let c: usize = (x - 1..=x + 1).map(|k| h.get(&k).copied().unwrap_or(0)).sum();
            if c >= need && !out.iter().any(|o| (o - x as f64).abs() <= 2.0) {
                out.push(x as f64);
            }
        }
        out
    };
    (peaks(&l), peaks(&r))
}

/// An equation's text, for its image's text alternative: its lines top to bottom, left to right.
pub fn equation_text(p: &Para) -> String {
    let mut ls: Vec<&PLine> = p.lines.iter().collect();
    ls.sort_by(|a, b| a.bbox.y0.total_cmp(&b.bbox.y0));
    // (rows: tops within 0.4 em of the row's first; each row left to right)
    let mut rows: Vec<Vec<&PLine>> = Vec::new();
    for l in ls {
        match rows.last_mut() {
            Some(r) if (l.bbox.y0 - r[0].bbox.y0).abs() < 0.4 * r[0].size.max(4.0) => r.push(l),
            _ => rows.push(vec![l]),
        }
    }
    let ls: Vec<&PLine> = rows
        .into_iter()
        .flat_map(|mut r| {
            r.sort_by(|a, b| a.bbox.x0.total_cmp(&b.bbox.x0));
            r
        })
        .collect();
    let t: Vec<String> = ls.iter().map(|l| trim(&line_ends_cleaned(&l.text)).to_string()).filter(|s| !s.is_empty()).collect();
    let t: String = t.join(" ").chars().filter(|c| !('\u{E000}'..='\u{F8FF}').contains(c)).collect();
    re!(r"\s+").replace_all(&t, " ").into_owned()
}

impl Analyzer<'_> {
    /// The page's column edges: its own, with those of the pages on the same side of the book (a
    /// page of formulas has few of its own).
    fn column_edges(&self, pno: usize, lines: &[PLine]) -> (Vec<f64>, Vec<f64>) {
        let (mut l, mut r) = edges_of(lines.iter(), self.body_size);
        let (dl, dr) = &self.doc_edges[pno % 2];
        for d in dl {
            if !l.iter().any(|x| (x - d).abs() <= 2.0) {
                l.push(*d);
            }
        }
        for d in dr {
            if !r.iter().any(|x| (x - d).abs() <= 2.0) {
                r.push(*d);
            }
        }
        (l, r)
    }

    /// The page's display equations: their lines out of `lines`, each equation a paragraph of kind
    /// `Equation` with its number (`eq_label`), the area to crop its ink from (`crop`) and its text.
    pub fn display_equations(&self, pno: usize, lines: Vec<PLine>, regions: &[Region]) -> (Vec<PLine>, Vec<Para>) {
        let body = self.body_size;
        let (lefts, rights) = self.column_edges(pno, &lines);
        if rights.is_empty() {
            return (lines, vec![]);
        }
        let n = lines.len();
        let mut tags: Vec<usize> = (0..n)
            .filter(|&i| {
                let l = &lines[i];
                is_equation_tag(&l.text)
                    && l.size >= body - 2.5
                    && l.size <= body + 1.5
                    && rights.iter().any(|r| (r - l.bbox.x1).abs() <= 0.6 * l.size + 1.0)
            })
            .collect();
        tags.sort_by(|&a, &b| lines[a].bbox.y0.total_cmp(&lines[b].bbox.y0));
        let is_tag: Vec<bool> = (0..n).map(|i| tags.contains(&i)).collect();
        let mut taken = vec![false; n];
        let mut out: Vec<Para> = Vec::new();
        let column_left = |x: f64| -> f64 {
            let prev_right = rights.iter().copied().filter(|r| *r < x).fold(f64::NEG_INFINITY, f64::max);
            lefts.iter().copied().filter(|l| *l > prev_right && *l <= x + 3.0 * body).fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| m.min(v)))).unwrap_or(0.0)
        };
        // a line that may be part of a formula: not running text, a caption or a bold heading; set in
        // from the column's left edge or in math type
        let math_like = |l: &PLine, col_left: f64| -> bool {
            let t = trim(&l.text);
            if t.is_empty() || is_prose(t) || caption_re().is_match(t) {
                return false;
            }
            if l.bold > 0.6 && l.math < 0.2 && count(t) > 3 {
                return false;
            }
            let words = prose_words(t);
            let em = l.size.max(body);
            let indented = l.bbox.x0 > col_left + em;
            if !indented && l.math < 0.4 {
                return false;
            }
            if words >= 3 && l.math < 0.3 {
                return false;
            }
            if t.ends_with(':') && words >= 2 {
                return false;
            }
            l.math >= 0.3 || t.chars().any(math_char) || count(t) <= 6
        };
        let evidence = |b: &[usize], lines: &[PLine]| b.iter().any(|&i| lines[i].math >= 0.25 || lines[i].text.chars().any(math_char));
        let band = |t: &PLine| {
            let cy = (t.bbox.y0 + t.bbox.y1) / 2.0;
            let h = 0.6 * t.size.max(4.0);
            R::new(t.bbox.x0, t.bbox.y0.max(cy - h), t.bbox.x1, t.bbox.y1.min(cy + h))
        };
        // the lines next to the block (within `gap`) that go on with the formula, until none does
        let grow = |block: &mut Vec<usize>, taken: &[bool], col_left: f64, col_right: f64, gap: f64, mine: &dyn Fn(&PLine) -> bool| {
            let mut bx = R::union_all(block.iter().map(|&i| &lines[i].bbox));
            let mut added = true;
            while added && block.len() < 40 {
                added = false;
                for i in 0..n {
                    if taken[i] || is_tag[i] || block.contains(&i) {
                        continue;
                    }
                    let l = &lines[i];
                    if l.bbox.x0 >= col_left - 1.5 * body
                        && l.bbox.x1 <= col_right + 1.0
                        && l.bbox.y0 < bx.y1 + gap
                        && l.bbox.y1 > bx.y0 - gap
                        && math_like(l, col_left)
                        && mine(l)
                    {
                        block.push(i);
                        bx = bx.union(&l.bbox);
                        added = true;
                    }
                }
            }
        };
        let make = |block: Vec<usize>, taken: &mut Vec<bool>, label: Option<String>, col_left: f64, col_right: f64, blank: Option<R>, out: &mut Vec<Para>| {
            for &i in &block {
                taken[i] = true;
            }
            let bx = R::union_all(block.iter().map(|&i| &lines[i].bbox));
            // the crop: up and down to the lines and figures around it, at most an em; its ink is taken
            // from there (a line with a box far taller than its type, a matrix's, says little)
            let others: Vec<R> = (0..n)
                .filter(|&i| {
                    let l = &lines[i];
                    !block.contains(&i)
                        && !is_tag[i]
                        && l.bbox.x1 > col_left
                        && l.bbox.x0 < col_right
                        && (l.bbox.h() <= 1.8 * l.size.max(4.0) || (!taken[i] && !math_like(l, col_left)))
                })
                .map(|i| lines[i].bbox)
                .chain(regions.iter().map(|g| g.rect))
                .filter(|o| o.x1 > bx.x0 - body && o.x0 < bx.x1 + body)
                .collect();
            let cy = (bx.y0 + bx.y1) / 2.0;
            let above = others
                .iter()
                .filter(|o| o.y1 <= bx.y0 + 1.0 || ((o.y0 + o.y1) / 2.0 < cy && o.y0 < bx.y0))
                .map(|o| o.y1)
                .fold(f64::NEG_INFINITY, f64::max);
            let below = others
                .iter()
                .filter(|o| o.y0 >= bx.y1 - 1.0 || ((o.y0 + o.y1) / 2.0 > cy && o.y1 > bx.y1))
                .map(|o| o.y0)
                .fold(f64::INFINITY, f64::min);
            let crop = R::new(
                (col_left - 0.5 * body).max(bx.x0 - 2.0 * body),
                (bx.y0 - 1.2 * body).max(above + 0.5),
                col_right.min(bx.x1 + 2.0 * body),
                (bx.y1 + 1.2 * body).min(below - 0.5),
            );
            let mut ls: Vec<PLine> = block.iter().map(|&i| lines[i].clone()).collect();
            ls.sort_by(|a, b| (a.bbox.y0, a.bbox.x0).partial_cmp(&(b.bbox.y0, b.bbox.x0)).unwrap_or(std::cmp::Ordering::Equal));
            let mut p = Para::new(ls, pno, Kind::Equation);
            p.region = Some(bx);
            p.eq_label = label;
            p.crop = Some(crop);
            p.blank = blank;
            out.push(p);
        };

        // numbered equations
        for &ti in &tags {
            if taken[ti] {
                continue;
            }
            let tag = &lines[ti];
            let em = tag.size.max(body);
            let col_left = column_left(tag.bbox.x0 - 3.0 * em);
            let mut col_right = tag.bbox.x0 - 0.5;
            let b = band(tag);
            let in_column = |l: &PLine, cr: f64| l.bbox.x0 >= col_left - 1.5 * em && l.bbox.x1 <= cr + 0.5;
            let beside: Vec<usize> = (0..n)
                .filter(|&i| {
                    !taken[i] && !is_tag[i] && in_column(&lines[i], col_right)
                        && v_overlap(&lines[i].bbox, &b) > 0.3 * lines[i].bbox.h().max(1.0).min(b.h().max(1.0))
                })
                .collect();
            // (running text level with it: a number in the text, not an equation's)
            if beside.iter().any(|&i| {
                let l = &lines[i];
                let c = (l.bbox.y0 + l.bbox.y1) / 2.0;
                count(&l.text) > 30 && is_prose(&l.text) && c >= b.y0 && c <= b.y1
            }) {
                continue;
            }
            let mut block: Vec<usize> = beside.into_iter().filter(|&i| math_like(&lines[i], col_left)).collect();
            // a number set below its equation's last line (it did not fit beside it)
            if block.is_empty() {
                block = (0..n)
                    .filter(|&i| {
                        let l = &lines[i];
                        !taken[i] && !is_tag[i] && l.bbox.x0 >= col_left - 1.5 * em && l.bbox.x1 <= tag.bbox.x1 + 0.5
                            && l.bbox.y0 < tag.bbox.y0 - 1.0 && l.bbox.y1 >= tag.bbox.y0 - 0.9 * em && math_like(l, col_left)
                    })
                    .collect();
                if !block.is_empty() {
                    col_right = col_right.max(block.iter().map(|&i| lines[i].bbox.x1).fold(f64::MIN, f64::max) + 0.5);
                }
            }
            if block.is_empty() {
                continue;
            }
            // (the lines nearer another number in the column are that equation's; rows not level with
            // the number may reach past it, to the column's edge)
            let others: Vec<R> = tags.iter().filter(|&&j| j != ti && (lines[j].bbox.x1 - tag.bbox.x1).abs() < 3.0 * em).map(|&j| band(&lines[j])).collect();
            let cy_tag = (b.y0 + b.y1) / 2.0;
            let full_right = rights.iter().copied().filter(|r| (r - tag.bbox.x1).abs() <= 0.6 * tag.size + 1.0).fold(col_right, f64::max);
            let tb = tag.bbox;
            let cr = col_right;
            let mine = |l: &PLine| -> bool {
                if l.bbox.x1 > cr + 0.5 && l.bbox.h() <= 1.8 * l.size.max(4.0) && v_overlap(&l.bbox, &tb) > 0.5 * tb.h() {
                    return false;
                }
                let cy = (l.bbox.y0 + l.bbox.y1) / 2.0;
                !others.iter().any(|o| ((o.y0 + o.y1) / 2.0 - cy).abs() < (cy_tag - cy).abs())
            };
            grow(&mut block, &taken, col_left, full_right, 0.6 * em, &mine);
            if !evidence(&block, &lines) {
                continue;
            }
            taken[ti] = true;
            let right = block.iter().map(|&i| lines[i].bbox.x1).fold(col_right, f64::max) + 0.5;
            let blank = R::new(tb.x0 - 1.0, tb.y0 - 1.0, tb.x1 + 2.0, tb.y1 + 1.0);
            make(block, &mut taken, tag_number(&tag.text), col_left, right, Some(blank), &mut out);
        }
        // a book's unnumbered displays: math centred in the column, set apart from the running text
        if self.book.kind.is_book_like() {
            let mut order: Vec<usize> = (0..n).collect();
            order.sort_by(|&a, &b| lines[a].bbox.y0.total_cmp(&lines[b].bbox.y0));
            for i in order {
                if taken[i] || is_tag[i] {
                    continue;
                }
                let l = &lines[i];
                let em = l.size.max(body);
                let col_left = column_left(l.bbox.x0);
                let Some(col_right) = rights.iter().copied().filter(|r| *r >= l.bbox.x1 - 1.0).fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| m.min(v)))) else { continue };
                if l.size < body - 1.1 || l.math < 0.45 || count(trim(&l.text)) < 3 || col_left <= 0.0
                    || l.bbox.x0 < col_left + 2.0 * em || !math_like(l, col_left)
                {
                    continue;
                }
                let mid = (col_left + col_right) / 2.0;
                let cx = (l.bbox.x0 + l.bbox.x1) / 2.0;
                if (cx - mid).abs() > 0.2 * (col_right - col_left) {
                    continue;
                }
                // (not a piece of a line of text beside it, nor a line of a paragraph set in from the
                // column: a caption's, a quotation's)
                if (0..n).any(|j| {
                    let o = &lines[j];
                    j != i && v_overlap(&o.bbox, &l.bbox) > 0.5 * o.bbox.h().min(l.bbox.h())
                        && (o.bbox.x0 - l.bbox.x1).max(l.bbox.x0 - o.bbox.x1) < 1.5 * em
                        && !math_like(o, col_left)
                }) || (0..n).any(|j| {
                    let o = &lines[j];
                    j != i && (o.bbox.x0 - l.bbox.x0).abs() < 2.0 && count(&o.text) > 30 && is_prose(&o.text)
                        && (o.bbox.y0 - l.bbox.y1).max(l.bbox.y0 - o.bbox.y1) < 0.5 * em
                }) {
                    continue;
                }
                let mut block = vec![i];
                grow(&mut block, &taken, col_left, col_right, 0.6 * em, &|_| true);
                let nn: usize = block.iter().map(|&k| count(&lines[k].text)).sum();
                let mf = block.iter().map(|&k| lines[k].math * count(&lines[k].text) as f64).sum::<f64>() / nn.max(1) as f64;
                if mf < 0.4 {
                    continue;
                }
                make(block, &mut taken, None, col_left, col_right, None, &mut out);
            }
        }
        // a number split off its line (see `split_equation_tag`) that is no equation's goes back on it
        let mut rest: Vec<PLine> = Vec::new();
        let mut back: Vec<PLine> = Vec::new();
        for (i, l) in lines.into_iter().enumerate() {
            if taken[i] {
                continue;
            }
            if l.tag_split { back.push(l) } else { rest.push(l) }
        }
        for mut t in back {
            t.tag_split = false;
            let host = rest
                .iter_mut()
                .filter(|h| h.bbox.x1 <= t.bbox.x0 + 1.0 && v_overlap(&h.bbox, &t.bbox) > 0.5 * t.bbox.h().min(h.bbox.h()))
                .max_by(|a, b| a.bbox.x1.partial_cmp(&b.bbox.x1).unwrap_or(std::cmp::Ordering::Equal));
            match host {
                Some(h) => {
                    h.text.push(' ');
                    h.text += &t.text;
                    h.spans.push(Span { text: " ".into(), ..Default::default() });
                    h.spans.extend(t.spans);
                    h.bbox = h.bbox.union(&t.bbox);
                }
                None => rest.push(t),
            }
        }
        (rest, out)
    }

    /// An equation's ink within `crop`, cropped close (`blank`, its number, painted out), and its
    /// width in points. Ink cut by the crop's top or bottom edge and apart from the rest by a white
    /// band (the bottom of the formula above, a matrix's brackets) is left out.
    pub fn render_ink(&mut self, pno: usize, crop: R, blank: Option<R>, name: &str) -> Option<(BookImage, f64)> {
        let page = &self.pages[pno];
        let r = crop.intersection(&R::new(0.0, 0.0, page.width, page.height));
        if !(r.w() > 2.0 && r.h() > 2.0) {
            return None;
        }
        let scale = (200.0 / 72.0f64).min(1600.0 / r.w().max(r.h()));
        let mut img = self.renderer.render(pno, r, scale)?;
        let (w, h) = (img.width() as i64, img.height() as i64);
        if let Some(b) = blank {
            let (x0, y0) = (((b.x0 - r.x0) * scale).floor() as i64, ((b.y0 - r.y0) * scale).floor() as i64);
            let (x1, y1) = (((b.x1 - r.x0) * scale).ceil() as i64, ((b.y1 - r.y0) * scale).ceil() as i64);
            for y in y0.max(0)..y1.min(h) {
                for x in x0.max(0)..x1.min(w) {
                    img.put_pixel(x as u32, y as u32, image::Rgb([255, 255, 255]));
                }
            }
        }
        let dark = |img: &image::RgbImage, x: i64, y: i64| {
            let p = img.get_pixel(x as u32, y as u32);
            (p[0] as u32 + p[1] as u32 + p[2] as u32) < 3 * 200
        };
        let ink_row: Vec<bool> = (0..h).map(|y| (0..w).any(|x| dark(&img, x, y))).collect();
        let gap = 2.max((0.15 * self.body_size * scale).round() as i64);
        let after_band = |start: i64, step: i64| -> Option<i64> {
            let mut y = start;
            while y >= 0 && y < h && ink_row[y as usize] {
                y += step;
            }
            let mut white = 0;
            while y >= 0 && y < h && !ink_row[y as usize] {
                white += 1;
                y += step;
            }
            (white >= gap && y >= 0 && y < h).then_some(y)
        };
        let inked = |a: i64, b: i64| (a.max(0)..b.min(h)).filter(|&y| ink_row[y as usize]).count();
        let (mut top, mut bottom) = (0i64, h - 1);
        if ink_row.first() == Some(&true) {
            if let Some(y) = after_band(0, 1) {
                if inked(y, h) >= 2 * inked(0, y) {
                    top = y;
                }
            }
        }
        if ink_row.last() == Some(&true) {
            if let Some(y) = after_band(h - 1, -1) {
                if y > top && inked(0, y + 1) >= 2 * inked(y + 1, h) {
                    bottom = y;
                }
            }
        }
        let (mut min_x, mut min_y, mut max_x, mut max_y) = (w, h, -1i64, -1i64);
        for y in top..=bottom {
            if !ink_row[y as usize] {
                continue;
            }
            for x in 0..w {
                if dark(&img, x, y) {
                    min_x = min_x.min(x);
                    max_x = max_x.max(x);
                    min_y = min_y.min(y);
                    max_y = max_y.max(y);
                }
            }
        }
        // (nothing, or a sliver: the crop missed the formula; the caller crops its box instead)
        if max_x < min_x || max_y < min_y || ((max_y - min_y) as f64) < 0.5 * self.body_size * scale {
            return None;
        }
        let pad = (2.0 * scale).round() as i64;
        let (x0, y0) = ((min_x - pad).max(0), (min_y - pad).max(0));
        let (x1, y1) = ((max_x + pad + 1).min(w), (max_y + pad + 1).min(h));
        let cut = image::imageops::crop_imm(&img, x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32).to_image();
        let name = self.unique_name(name);
        Some((encode(&cut, false, &name)?, (x1 - x0) as f64 / scale))
    }
}
