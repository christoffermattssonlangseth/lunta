//! Code listings in a book (a port of `PDFCode.swift`): lines set in a monospaced font (LMMono,
//! Courier, Inconsolata…), one after another in a column, become one block, shown as <pre><code>
//! with the lines' breaks and indentation, instead of paragraphs of prose.

use super::layout::*;

impl Analyzer<'_> {
    /// The page's code listings: their lines out of `lines`, each listing a paragraph of kind `Code`.
    pub fn code_blocks(&self, pno: usize, lines: Vec<PLine>) -> (Vec<PLine>, Vec<Para>) {
        let blank = |l: &PLine| trim(&l.text).is_empty();
        let mut code: Vec<usize> = (0..lines.len()).filter(|&i| lines[i].mono >= 0.8 && !blank(&lines[i])).collect();
        if code.len() < 3 {
            return (lines, vec![]);
        }
        code.sort_by(|&a, &b| (lines[a].bbox.y0, lines[a].bbox.x0).partial_cmp(&(lines[b].bbox.y0, lines[b].bbox.x0)).unwrap_or(std::cmp::Ordering::Equal));
        let other: Vec<&PLine> = lines.iter().filter(|l| l.mono < 0.8 && !blank(l)).collect();
        let width = self.pages[pno].width;
        let mut blocks: Vec<Vec<usize>> = Vec::new();
        for i in code {
            let l = &lines[i];
            // (the next line of the listing: close below the last, starting at or right of its left
            // edge, with no line of other type between: a web address in a reference list is no listing)
            let joins = blocks.last().is_some_and(|b| {
                let last = &lines[*b.last().unwrap()];
                let left = b.iter().map(|&k| lines[k].bbox.x0).fold(f64::INFINITY, f64::min);
                l.bbox.y0 - last.bbox.y1 < 2.2 * l.size.max(4.0)
                    && l.bbox.y0 >= last.bbox.y0 - 1.0
                    && (l.bbox.x0 - left).abs() < 0.5 * width
                    && !other.iter().any(|o| {
                        o.bbox.y0 > last.bbox.y0 + 1.0 && o.bbox.y0 < l.bbox.y0 - 1.0 && o.bbox.x1 > l.bbox.x0 - 20.0 && o.bbox.x0 < l.bbox.x1 + 20.0
                    })
            });
            if joins {
                blocks.last_mut().unwrap().push(i);
            } else {
                blocks.push(vec![i]);
            }
        }
        let url = |t: &str| re!(r"https?\s*:|www\.|\.(com|org|net|edu)\b|^\s*/|\w/\S*/").is_match(t);
        let mut taken = vec![false; lines.len()];
        let mut keep: Vec<Vec<usize>> = Vec::new();
        for b in blocks {
            if b.len() >= 3 && b.iter().filter(|&&i| url(&lines[i].text)).count() * 3 < b.len() {
                for &i in &b {
                    taken[i] = true;
                }
                keep.push(b);
            }
        }
        if keep.is_empty() {
            return (lines, vec![]);
        }
        let out = keep.iter().map(|b| Para::new(b.iter().map(|&i| lines[i].clone()).collect(), pno, Kind::Code)).collect();
        let rest = lines.into_iter().enumerate().filter(|(i, _)| !taken[*i]).map(|(_, l)| l).collect();
        (rest, out)
    }
}

/// A listing's text: its lines, each indented by how far right of the listing's left edge it starts
/// (in the lines' own character width), blank lines where the listing leaves a gap.
pub fn code_text(p: &Para) -> String {
    let mut ls: Vec<&PLine> = p.lines.iter().collect();
    ls.sort_by(|a, b| a.bbox.y0.total_cmp(&b.bbox.y0));
    let Some(left) = ls.iter().map(|l| l.bbox.x0).reduce(f64::min) else { return String::new() };
    let mut ws: Vec<f64> = ls.iter().filter(|l| count(&l.text) >= 8).map(|l| l.bbox.w() / count(&l.text) as f64).collect();
    ws.sort_by(|a, b| a.total_cmp(b));
    let mut out: Vec<String> = Vec::new();
    let mut prev: Option<&PLine> = None;
    for l in ls {
        let cw = ws.get(ws.len() / 2).copied().unwrap_or(0.525 * l.size.max(4.0));
        if prev.is_some_and(|p| l.bbox.y0 - p.bbox.y1 > 0.8 * l.size) {
            out.push(String::new());
        }
        let indent = ((l.bbox.x0 - left) / cw).round().max(0.0) as usize;
        out.push(" ".repeat(indent) + trim(&line_ends_cleaned(&l.text)));
        prev = Some(l);
    }
    out.join("\n")
}
