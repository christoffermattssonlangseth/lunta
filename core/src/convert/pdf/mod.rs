//! PDF → book without AI: a port of `PDFAnalyzer.swift` / `PDFContent.swift` and the PDF parts
//! of `Converter.swift` (feature "pdf").
//!
//! PDFium (via `pdfium-render`) stands in for PDFKit: per-character text, bounds, font names
//! and sizes, image and path objects, document info and page rendering. The library is loaded
//! at run time — see [`pdfium`] for where it is looked for. The layout heuristics live in
//! [`layout`] and do not touch PDFium.

pub mod book;
pub mod equations;
pub mod extract;
pub mod layout;
pub mod notes;
pub mod refs;
pub mod symbols;

use super::{esc, fail, net, Book, BookImage, Chapter, ConvertError, Heading, Options, PaperMeta, Result};
use layout::{re, styled, Analyzer, Kind, Para};
use pdfium_render::prelude::*;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::OnceLock;

/// Where PDFium is looked for, in order: `$PDFIUM_PATH` (the library file or its folder), next
/// to the executable (and in `../lib`, `../Frameworks`, `../Resources` from it), then the
/// system library search path.
pub fn pdfium() -> Result<&'static Pdfium> {
    static P: OnceLock<std::result::Result<Pdfium, String>> = OnceLock::new();
    P.get_or_init(load_pdfium).as_ref().map_err(|e| ConvertError::Message(e.clone()))
}

fn candidate_paths() -> Vec<PathBuf> {
    let name = Pdfium::pdfium_platform_library_name();
    let mut out = Vec::new();
    if let Some(p) = std::env::var_os("PDFIUM_PATH").filter(|p| !p.is_empty()) {
        let p = PathBuf::from(p);
        if p.is_dir() {
            out.push(p.join(&name));
            out.push(p.join("lib").join(&name));
        } else {
            out.push(p);
        }
    }
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.to_path_buf())) {
        out.push(dir.join(&name));
        for sub in ["../lib", "../Frameworks", "../Resources", "lib"] {
            out.push(dir.join(sub).join(&name));
        }
    }
    out
}

fn load_pdfium() -> std::result::Result<Pdfium, String> {
    let mut tried = Vec::new();
    for p in candidate_paths() {
        if !p.exists() {
            continue;
        }
        match Pdfium::bind_to_library(&p) {
            Ok(b) => return Ok(Pdfium::new(b)),
            Err(e) => tried.push(format!("{}: {e:?}", p.display())),
        }
    }
    match Pdfium::bind_to_system_library() {
        Ok(b) => Ok(Pdfium::new(b)),
        Err(_) => {
            tried.push("the system library path".into());
            let name = Pdfium::pdfium_platform_library_name();
            let looked: Vec<String> = candidate_paths().iter().map(|p| p.display().to_string()).collect();
            Err(format!(
                "PDF conversion needs the PDFium library ({}), which was not found. Set PDFIUM_PATH to it (or to \
                 its folder), or put it next to the program; prebuilt libraries: \
                 https://github.com/bblanchon/pdfium-binaries. Looked in: {}; {}",
                name.to_string_lossy(),
                if looked.is_empty() { "-".to_string() } else { looked.join(", ") },
                tried.join("; ")
            ))
        }
    }
}

fn open<'a>(p: &'a Pdfium, data: &'a [u8]) -> Result<PdfDocument<'a>> {
    match p.load_pdf_from_byte_slice(data, None) {
        Ok(d) => Ok(d),
        Err(PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::PasswordError)) => {
            Err(fail("The PDF is password-protected."))
        }
        Err(_) => Err(fail("Could not open the PDF.")),
    }
}

/// Quick DOI sniff from the first pages, the last page and the document info.
fn quick_doi_doc(doc: &PdfDocument) -> Option<String> {
    let n = doc.pages().len() as usize;
    if n == 0 {
        return None;
    }
    let mut text = extract::doc_info(doc).subject + " ";
    let mut idx: Vec<usize> = (0..n.min(2)).collect();
    idx.push(n - 1);
    for i in idx {
        text += &extract::page_text(doc, i);
        text.push(' ');
    }
    super::resolver::doi_in(&text)
}

/// The DOI printed in a PDF (first pages, last page, document info), if any.
pub fn doi_in_pdf(data: &[u8]) -> Option<String> {
    let p = pdfium().ok()?;
    let doc = p.load_pdf_from_byte_slice(data, None).ok()?;
    quick_doi_doc(&doc)
}

/// Every page's lines with where they are (top-left page coordinates), for finding the reference
/// a citation points to (`refcite`). Each page's media box is in `boxes`: (left, top) in PDF user
/// space, to turn a destination into these coordinates.
pub fn text_lines(data: &[u8]) -> Result<(Vec<crate::refcite::TextLine>, Vec<(f64, f64)>)> {
    let p = pdfium()?;
    let doc = open(p, data)?;
    let mut out = Vec::new();
    let mut boxes = Vec::new();
    for (pno, page) in doc.pages().iter().enumerate() {
        let mb = extract::media_box(&page);
        boxes.push(mb.origin());
        let Ok(text) = page.text() else { continue };
        let (_, hidden) = extract::graphics(&page, &mb);
        for l in extract::extract_lines(&text, &mb, pno, &hidden) {
            out.push(crate::refcite::TextLine {
                page: pno as u32,
                text: l.text,
                left: l.bbox.x0,
                top: l.bbox.y0,
                right: l.bbox.x1,
                bottom: l.bbox.y1,
            });
        }
    }
    Ok((out, boxes))
}

/// Plain text of the first pages of a PDF (for matching a PDF to a known paper by its title).
pub fn first_pages_text(data: &[u8], pages: usize) -> String {
    let Ok(p) = pdfium() else { return String::new() };
    let Ok(doc) = p.load_pdf_from_byte_slice(data, None) else { return String::new() };
    let n = (doc.pages().len() as usize).min(pages);
    (0..n).map(|i| extract::page_text(&doc, i)).collect::<Vec<_>>().join(" ")
}

/// Title, authors, journal and date from Crossref.
pub fn crossref(doi: &str) -> Option<PaperMeta> {
    let j = net::json(&format!("https://api.crossref.org/works/{}", net::encode_path(doi))).ok()?;
    let m = j.get("message")?.as_object()?;
    let strs = |k: &str| -> Vec<String> {
        m.get(k).and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default()
    };
    let mut meta = PaperMeta { doi: doi.to_string(), ..Default::default() };
    meta.title = re!(r"<[^>]+>").replace_all(strs("title").first().map(|s| s.as_str()).unwrap_or(""), "").into_owned();
    meta.authors = m
        .get("author")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| {
                    let s = [x.get("given"), x.get("family")]
                        .iter()
                        .filter_map(|v| v.and_then(|v| v.as_str()))
                        .collect::<Vec<_>>()
                        .join(" ");
                    if s.is_empty() {
                        x.get("name").and_then(|v| v.as_str()).map(String::from)
                    } else {
                        Some(s)
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    meta.journal = strs("container-title").into_iter().next().unwrap_or_default();
    if let Some(parts) = m
        .get("issued")
        .and_then(|v| v.get("date-parts"))
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .and_then(|v| v.as_array())
    {
        let p: Vec<i64> = parts.iter().filter_map(|x| x.as_i64()).collect();
        if let Some(y) = p.first() {
            meta.date = if p.len() >= 3 { format!("{:04}-{:02}-{:02}", y, p[1], p[2]) } else { y.to_string() };
        }
    }
    if let Some(lic) = m.get("license").and_then(|v| v.as_array()).and_then(|a| {
        a.iter().filter_map(|x| x.get("URL").and_then(|u| u.as_str())).find(|u| u.contains("creativecommons"))
    }) {
        meta.license = lic.to_string();
    }
    Some(meta)
}

/// The DOI of a PDF, for the XML-first lookup in `convert::build`.
pub(crate) fn quick_doi(data: &[u8]) -> Result<Option<String>> {
    let p = pdfium()?;
    let doc = open(p, data)?;
    Ok(quick_doi_doc(&doc))
}

/// Analyses the PDF and assembles the book (Swift's `PDFBook.build`).
pub fn build(data: &[u8], extra: &PaperMeta, opts: &Options) -> Result<Book> {
    let p = pdfium()?;
    let doc = open(p, data)?;
    let n = doc.pages().len() as usize;
    if n == 0 {
        return Err(fail("The PDF has no pages."));
    }
    opts.log(&format!("Analysing {n} pages"));
    let log = |m: &str| opts.log(m);
    let pages = extract::pages(&doc, &log);
    let info = extract::doc_info(&doc);
    let heights: Vec<f64> = pages.iter().map(|p| p.height).collect();
    let outline = outline(&doc, &heights);
    let mut renderer = extract::PdfiumRenderer::new(&doc);
    let mut an = Analyzer::new(pages, &mut renderer, &log);
    an.book.outline = outline;
    an.work_type = opts.work_type.clone().unwrap_or_default();
    let paras = an.analyze();
    let guessed = an.meta_guess(&paras, &info, || quick_doi_doc(&doc));
    let body_size = an.body_size;
    let book = std::mem::take(&mut an.book);
    drop(an);
    assemble_book(paras, guessed, extra, body_size, opts, &book)
}

/// The PDF's outline (bookmarks) as a flat list in reading order, levels from 1, `y` from the top
/// of the page (0 when the destination gives none). A lone top entry with entries below it (the
/// book's title) is skipped; a part's chapters stay chapters ("PART I …" at the same level). Up to
/// three levels.
fn outline(doc: &PdfDocument, heights: &[f64]) -> Vec<book::OutlineEntry> {
    use pdfium_render::prelude::{PdfBookmark, PdfDestinationViewSettings};
    fn walk(b: Option<PdfBookmark>, level: usize, heights: &[f64], out: &mut Vec<book::OutlineEntry>, budget: &mut usize) {
        let mut cur = b;
        while let Some(bm) = cur {
            if *budget == 0 || level > 3 {
                return;
            }
            *budget -= 1;
            let title = bm.title().unwrap_or_default().split_whitespace().collect::<Vec<_>>().join(" ");
            if let Some(d) = bm.destination() {
                if let Ok(pi) = d.page_index() {
                    let page = pi as usize;
                    if page < heights.len() && !title.is_empty() {
                        let h = heights[page];
                        let y = match d.view_settings() {
                            Ok(PdfDestinationViewSettings::SpecificCoordinatesAndZoom(_, Some(y), _)) => {
                                let y = y.value as f64;
                                if y.is_finite() && y >= -1.0 && y <= h + 1.0 { (h - y).max(0.0) } else { 0.0 }
                            }
                            _ => 0.0,
                        };
                        out.push(book::OutlineEntry { title: title.clone(), level, page, y });
                    }
                }
            }
            let part = level == 1 && bm.first_child().is_some() && book::is_part_title(&title);
            walk(bm.first_child(), if part { level } else { level + 1 }, heights, out, budget);
            cur = bm.next_sibling();
        }
    }
    let bookmarks = doc.bookmarks();
    let mut root = bookmarks.root();
    // a lone top entry with several below it: the book's title
    while let Some(r) = &root {
        if r.next_sibling().is_none() && r.children_len() > 1 {
            root = r.first_child();
        } else {
            break;
        }
    }
    let mut out = Vec::new();
    let mut budget = 5000;
    walk(root, 1, heights, &mut out, &mut budget);
    let mut ix: Vec<usize> = (0..out.len()).collect();
    ix.sort_by_key(|&i| (out[i].page, i));
    ix.into_iter().map(|i| out[i].clone()).collect()
}

/// How far a caption is from a figure: the vertical gap for one above or below it; for one beside it (a
/// book's margin caption, level with its figure) the horizontal gap, so that a paragraph below the
/// figure isn't taken for its caption.
fn caption_gap(c: &layout::R, f: &layout::R) -> f64 {
    let vertical = (c.y0 - f.y1).abs().min((f.y0 - c.y1).abs());
    let overlap = c.y1.min(f.y1) - c.y0.max(f.y0);
    if overlap > 0.5 * (c.y1 - c.y0) && (c.x0 >= f.x1 - 2.0 || c.x1 <= f.x0 + 2.0) {
        return vertical.min((c.x0 - f.x1).max(f.x0 - c.x1).max(0.0));
    }
    vertical
}

/// The figures in the PDF by their label ([`super::jats::figure_key`]: "fig 3", "extended data
/// fig 2", "supplementary fig 1"): to fill in figures another source (an XML version without its
/// images) doesn't have. Empty when the PDF can't be read.
///
/// Matched by page, not through caption detection: a figure is the page with a line that starts
/// its caption ("Fig. 3 | …", "Extended Data Fig. 2 | See next page for caption"), and the image
/// is all figure areas on that page together (figures are often found in pieces). With several
/// captions on a page, each area goes to the nearest one.
pub fn figures_by_label(data: &[u8], log: &dyn Fn(&str)) -> std::collections::HashMap<String, super::BookImage> {
    use std::collections::HashMap;
    let mut out = HashMap::new();
    let Ok(p) = pdfium() else { return out };
    let Ok(doc) = open(p, data) else { return out };
    if doc.pages().is_empty() {
        return out;
    }
    let pages = extract::pages(&doc, log);
    let mut renderer = extract::PdfiumRenderer::new(&doc);
    let mut an = Analyzer::new(pages, &mut renderer, log);
    let paras = an.analyze();
    let mut areas: HashMap<usize, Vec<layout::R>> = HashMap::new();
    for p in paras.iter().filter(|p| p.kind == Kind::Figure) {
        areas.entry(p.page).or_default().push(p.bbox());
    }
    // caption starts: the label, then a separator (not "Fig. 5a)" in the text)
    // (a dot or en dash into a digit is the number's: "Fig. 1.2 |", not "Fig. 1" + ".2")
    let head = re!(concat!(
        r"(?i)^(?:Extended\s+Data\s+|Supplementary\s+|Supporting\s+)?(?:Figure|Fig\.?)\s*",
        layout::label_number!(),
        r"\s*(?:[|:—]|[.–](?:$|\D)|\s-\s)"
    ));
    let mut heads: Vec<(usize, Vec<(String, f64)>)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (pno, page) in an.pages.iter().enumerate() {
        if !areas.contains_key(&pno) {
            continue;
        }
        let mut hs = Vec::new();
        for l in &page.lines {
            let t = layout::line_ends_cleaned(super::trim_ws(&l.text));
            if !head.is_match(&t) {
                continue;
            }
            let Some(key) = super::jats::figure_key(&t) else { continue };
            if seen.insert(key.clone()) {
                hs.push((key, (l.bbox.y0 + l.bbox.y1) / 2.0));
            }
        }
        if !hs.is_empty() {
            heads.push((pno, hs));
        }
    }
    for (pno, hs) in heads {
        let mut rects: Vec<(String, layout::R)> = Vec::new();
        for a in &areas[&pno] {
            let cy = (a.y0 + a.y1) / 2.0;
            let Some(near) = hs.iter().min_by(|x, y| (x.1 - cy).abs().total_cmp(&(y.1 - cy).abs())) else { continue };
            match rects.iter_mut().find(|r| r.0 == near.0) {
                Some(r) => r.1 = r.1.union(a),
                None => rects.push((near.0.clone(), *a)),
            }
        }
        for (key, r) in rects {
            let photo = an.pages[pno].graphics.images.iter().any(|g| r.contains(g, 5.0) && g.area() > 0.3 * r.area());
            let name = format!("p{}-{}", pno + 1, super::slug(&key, 60));
            if let Some(img) = an.render(pno, r, photo, &name) {
                out.insert(key, img);
            }
        }
    }
    out
}

/// Debug listing of the analysed paragraphs (set `P2E_DEBUG`), in the Swift converter's format.
fn debug_dump(paras: &[Para]) {
    for p in paras {
        let b = p.bbox();
        let text: String = p.text().chars().take(70).collect();
        println!(
            "p{:<2} {:<9} L{} sz{:4.1} bold{:.2} ({:3.0},{:3.0})-({:3.0},{:3.0}) {}",
            p.page + 1,
            format!("{:?}", p.kind).to_lowercase(),
            p.level,
            p.size(),
            p.lines.first().map(|l| l.bold).unwrap_or(0.0),
            b.x0,
            b.y0,
            b.x1,
            b.y1,
            text
        );
    }
}

/// The notes at the end of the chapters their markers are in, each as an `<aside>` whose label leads
/// back to the marker; a note whose marker wasn't found goes to the chapter it was found in.
/// (`notes`: id, label, the note's HTML, the chapter it was found in.)
pub fn place_footnotes(notes: &[(String, String, String, usize)], chapters: &mut [Chapter]) {
    if notes.is_empty() || chapters.is_empty() {
        return;
    }
    let mut by_chapter: Vec<(usize, String)> = Vec::new();
    for (id, label, html, home) in notes {
        let k = chapters.iter().position(|c| c.html.contains(&format!("href=\"#{id}\""))).unwrap_or((*home).min(chapters.len() - 1));
        let label = if chapters[k].html.contains(&format!("id=\"fnref-{id}\"")) {
            format!("<a href=\"#fnref-{id}\">{}</a>", esc(label))
        } else {
            format!("<span class=\"fn-label\">{}</span>", esc(label))
        };
        let aside = format!("<aside epub:type=\"footnote\" role=\"doc-footnote\" id=\"{id}\" class=\"footnote\"><p>{label} {html}</p></aside>");
        match by_chapter.iter_mut().find(|x| x.0 == k) {
            Some(x) => x.1 += &aside,
            None => by_chapter.push((k, aside)),
        }
    }
    for (k, html) in by_chapter {
        let h = &mut chapters[k].html;
        let block = format!("<div class=\"footnotes\">{html}</div>");
        if h.ends_with("</section>") {
            h.truncate(h.len() - "</section>".len());
            *h += &block;
            *h += "</section>";
        } else {
            *h += &block;
        }
    }
}

/// A PDF's reference list: its entries keep the numbers they are printed with.
const REFS_OL: &str = "<ol class=\"refs\" style=\"list-style-type: none; padding-left: 0\">";

/// Turns analysed paragraphs into chapters (the rest of Swift's `PDFBook.build`).
pub fn assemble(paras: Vec<Para>, guessed: PaperMeta, extra: &PaperMeta, body_size: f64, opts: &Options) -> Result<Book> {
    assemble_book(paras, guessed, extra, body_size, opts, &book::BookInfo::default())
}

/// [`assemble`] for a paper or a book (`info` from the analyzer: kind, outline, index pages).
pub fn assemble_book(
    paras: Vec<Para>,
    guessed: PaperMeta,
    extra: &PaperMeta,
    body_size: f64,
    opts: &Options,
    info: &book::BookInfo,
) -> Result<Book> {
    let is_book = info.kind.is_book_like();
    let mut meta = guessed.clone();
    if !extra.title.is_empty() || !extra.authors.is_empty() {
        meta = extra.clone();
        meta.merge(&guessed);
    }
    // Swift asks Crossref even when offline; here offline means no network at all.
    if !meta.doi.is_empty() && !opts.offline {
        if let Some(mut cr) = crossref(&meta.doi) {
            opts.log(&format!("Found metadata for doi:{}", meta.doi));
            cr.merge(&meta);
            meta = cr;
        }
    }
    if is_book && meta.authors.is_empty() {
        meta.authors = book::title_page_authors(&paras);
    }
    if std::env::var_os("P2E_DEBUG").is_some() {
        debug_dump(&paras);
    }
    let ends_sentence = |t: &str| t.ends_with('.') || t.ends_with('?') || t.ends_with('!') || t.ends_with(':');
    // merge paragraphs split across columns/pages
    let mut merged: Vec<Para> = Vec::new();
    for p in paras {
        let text = p.text();
        if p.kind == Kind::Text && text.chars().next().is_some_and(|c| c.is_lowercase()) {
            // (past figures, legends, front matter, notes and boxes set between the columns)
            let skip = |q: &Para| {
                matches!(q.kind, Kind::Figure | Kind::Table | Kind::Caption | Kind::Front | Kind::Footnote | Kind::Sidenote) || q.boxed != p.boxed
            };
            if let Some(j) = merged.iter().rposition(|q| !skip(q)) {
                let between = merged[j + 1..]
                    .iter()
                    .filter(|q| q.boxed == p.boxed && !matches!(q.kind, Kind::Footnote | Kind::Sidenote))
                    .count();
                if merged[j].kind == Kind::Text && between <= 3 && !ends_sentence(&merged[j].text()) {
                    merged[j].lines.extend(p.lines);
                    continue;
                }
            }
        }
        merged.push(p);
    }
    // figure legends continue after the caption title in smaller type: fold them into the caption
    // (in a book only what follows right below it: its exercises and notes are set in that type too)
    let adjacent = |p: &Para, c: &Para| -> bool {
        let (Some(last), Some(first)) = (c.lines.last(), p.lines.first()) else { return false };
        p.page == c.page
            && first.bbox.y0 >= last.bbox.y0
            && first.bbox.y0 - last.bbox.y1 < 1.2 * p.size().max(1.0)
            && first.bbox.x0 < last.bbox.x1
            && first.bbox.x1 > last.bbox.x0
    };
    let mut folded: Vec<Para> = Vec::new();
    for p in merged {
        let text = p.text();
        let t = super::trim_ws(&text);
        let legend_like = re!(r"^\([A-Z][A-Za-z]{0,3}\)\s|^See also Figure|^Scale bars?").is_match(t);
        if p.kind == Kind::Text && p.boxed.is_none() && p.size() < body_size - 0.4 {
            if let Some(j) = folded.iter().rposition(|q| q.kind == Kind::Caption) {
                // a legend's "(D) …" panels continue after the page's body text when it runs onto the next page
                if (folded.len() - j <= 8 || legend_like)
                    && folded[j].page.abs_diff(p.page) <= 1
                    && (p.size() - folded[j].size()).abs() < 0.8
                    && (legend_like || j == folded.len() - 1)
                    && (!is_book || adjacent(&p, &folded[j]))
                {
                    let q = &mut folded[j];
                    let mut lines = std::mem::take(&mut q.lines);
                    lines.extend(p.lines);
                    *q = Para::new(lines, q.page, Kind::Caption);
                    continue;
                }
            }
        }
        folded.push(p);
    }
    let merged = folded;
    // the reference list: one entry per reference
    let (merged, ref_entry) = refs::mark(merged, body_size);
    // attach captions to the nearest figure/table on the same page
    let is_table_caption = |c: &Para| re!(r"(?i)^(supplementary\s+)?table").is_match(&c.text());
    let mut cap_of: Vec<(usize, usize)> = Vec::new();
    let used = |cap_of: &Vec<(usize, usize)>, j: usize| cap_of.iter().any(|x| x.1 == j);
    for (i, p) in merged.iter().enumerate() {
        if !matches!(p.kind, Kind::Figure | Kind::Table) {
            continue;
        }
        let mut best: Option<usize> = None;
        let mut bd = f64::MAX;
        for (j, c) in merged.iter().enumerate().take(i + 5).skip(i.saturating_sub(4)) {
            if c.kind != Kind::Caption || c.page != p.page || used(&cap_of, j) {
                continue;
            }
            if (p.kind == Kind::Table) != is_table_caption(c) {
                continue;
            }
            let (cb, pb) = (c.bbox(), p.bbox());
            let d = caption_gap(&cb, &pb);
            // (beside it only if no other figure on the page is nearer: not the next column's caption)
            if d < (cb.y0 - pb.y1).abs().min((pb.y0 - cb.y1).abs())
                && merged.iter().enumerate().any(|(k, q)| k != i && q.kind == p.kind && q.page == p.page && caption_gap(&cb, &q.bbox()) < d)
            {
                continue;
            }
            if d < bd && d < 120.0 {
                best = Some(j);
                bd = d;
            }
        }
        if let Some(b) = best {
            cap_of.push((i, b));
        }
    }
    for (i, p) in merged.iter().enumerate() {
        if !matches!(p.kind, Kind::Figure | Kind::Table)
            || cap_of.iter().any(|x| x.0 == i)
            || !p.region.map(|r| r.area() > 40000.0).unwrap_or(false)
        {
            continue;
        }
        let mut best: Option<usize> = None;
        let mut bd = usize::MAX;
        for (j, c) in merged.iter().enumerate().take(i + 13).skip(i.saturating_sub(12)) {
            if c.kind != Kind::Caption || c.page.abs_diff(p.page) != 1 || used(&cap_of, j) {
                continue;
            }
            if (p.kind == Kind::Table) != is_table_caption(c) {
                continue;
            }
            if j.abs_diff(i) < bd {
                best = Some(j);
                bd = j.abs_diff(i);
            }
        }
        if let Some(b) = best {
            cap_of.push((i, b));
        }
    }

    // a book: chapters from its outline
    let plan = if is_book && !info.outline.is_empty() { Some(book::plan(&merged, &info.outline)) } else { None };
    let outline_sections = plan.as_ref().map(|p| p.entries.iter().any(|e| e.level >= 2)).unwrap_or(false);
    let mut next_entry = 0;
    let mut index_opened = false;
    let mut chapter_label: Option<String> = None;
    struct Cur {
        title: String,
        html: String,
        headings: Vec<Heading>,
    }
    let mut chapters: Vec<Chapter> = Vec::new();
    let mut cur: Option<Cur> = None;
    let mut images: Vec<BookImage> = Vec::new();
    let mut eq_ids: HashSet<String> = HashSet::new();
    let mut front = String::new();
    let mut auto_id = 0;
    let mut next_id = || {
        auto_id += 1;
        format!("s{auto_id}")
    };
    fn close(cur: &mut Option<Cur>, chapters: &mut Vec<Chapter>) {
        if let Some(c) = cur.take() {
            chapters.push(Chapter { title: c.title, html: format!("<section>{}</section>", layout::bullet_lists(&c.html)), headings: c.headings });
        }
    }
    let open = |title: &str, id: String, cur: &mut Option<Cur>, chapters: &mut Vec<Chapter>| {
        close(cur, chapters);
        *cur = Some(Cur {
            title: title.to_string(),
            html: format!("<h1 id=\"{id}\">{}</h1>", esc(title)),
            headings: vec![Heading { level: 1, id, title: title.to_string() }],
        });
    };

    let has_intro = merged
        .iter()
        .any(|p| p.kind == Kind::Heading && re!(r"(?i)introduction|background").is_match(&p.text()));
    let mut abstract_done = false;
    let mut intro_done = false;
    let mut last_kind: Option<Kind> = None;
    let missing_title = meta.title.is_empty();
    let mut open_box: Option<usize> = None;
    let mut in_list = false;
    // footnotes: each goes to the end of the chapter its marker is in (else the one it was found in)
    let mut footnotes: Vec<(String, String, String, usize)> = Vec::new();
    for (i, p) in merged.iter().enumerate() {
        // appends to the current chapter, opening "Main text" if there is none
        macro_rules! append {
            ($html:expr) => {{
                let h: String = $html;
                if cur.is_none() {
                    let id = next_id();
                    open(if is_book { "Front matter" } else { "Main text" }, id, &mut cur, &mut chapters);
                }
                cur.as_mut().unwrap().html += &h;
            }};
        }
        if let Some(plan) = &plan {
            // the outline's entries that start here
            while next_entry < plan.entries.len() && !book::before(p, &plan.entries[next_entry]) {
                if in_list {
                    append!("</ol>".to_string());
                    in_list = false;
                }
                if open_box.is_some() {
                    append!("</div>".to_string());
                    open_box = None;
                }
                let (e, title) = (&plan.entries[next_entry], &plan.titles[next_entry]);
                let id = next_id();
                if e.level <= 1 {
                    open(title, id, &mut cur, &mut chapters);
                } else {
                    let h = e.level.min(3);
                    append!(format!("<h{h} id=\"{id}\">{}</h{h}>", esc(title)));
                    if h == 2 {
                        cur.as_mut().unwrap().headings.push(Heading { level: 2, id, title: title.clone() });
                    }
                }
                next_entry += 1;
            }
            if plan.consumed.contains(&i) {
                continue;
            }
        }
        // a book's index: one entry (line) per paragraph, in one chapter
        if is_book && info.on_index_page(p.page) && matches!(p.kind, Kind::Text | Kind::Heading | Kind::Caption | Kind::Front | Kind::Title) {
            if in_list {
                append!("</ol>".to_string());
                in_list = false;
            }
            if plan.is_none() && !index_opened {
                let id = next_id();
                open("Index", id, &mut cur, &mut chapters);
                index_opened = true;
            }
            if book::is_index_title(&p.text()) {
                continue;
            }
            for l in &p.lines {
                let t = l.text.replace(layout::GLUE, "").replace(layout::HARD_HYPHEN, "-");
                let t = t.trim();
                if !t.is_empty() {
                    append!(format!("<p class=\"index\">{}</p>", esc(t)));
                }
            }
            continue;
        }
        if p.kind == Kind::Footnote {
            if let Some(id) = &p.note_id {
                footnotes.push((id.clone(), p.note_label.clone().unwrap_or_default(), styled(p), chapters.len()));
            }
            continue;
        }
        // reference entries: one list item each (numbered as printed)
        if in_list && !ref_entry[i] {
            append!("</ol>".to_string());
            in_list = false;
        }
        if p.kind == Kind::Sidenote {
            append!(format!("<aside class=\"sidenote\" role=\"note\"><p>{}</p></aside>", styled(p)));
            continue;
        }
        if ref_entry[i] {
            if !in_list {
                append!(REFS_OL.to_string());
                in_list = true;
            }
            append!(format!("<li>{}</li>", styled(p)));
            continue;
        }
        // a box's paragraphs, kept together
        if p.boxed != open_box {
            if open_box.is_some() {
                append!("</div>".to_string());
            }
            if p.boxed.is_some() {
                append!("<div class=\"box\">".to_string());
            }
            open_box = p.boxed;
        }
        match p.kind {
            Kind::Figure | Kind::Table => {
                let cap = cap_of.iter().find(|x| x.0 == i).map(|x| styled(&merged[x.1])).unwrap_or_default();
                if let Some(img) = p.asset.clone() {
                    let src = format!("../images/{}", img.file_name);
                    images.push(img);
                    if p.kind == Kind::Table {
                        let c = if cap.is_empty() { String::new() } else { format!("<p class=\"caption\">{cap}</p>") };
                        append!(format!("<div class=\"table-wrap\">{c}<img src=\"{src}\" alt=\"Table\"/></div>"));
                    } else {
                        let c = if cap.is_empty() { String::new() } else { format!("<figcaption>{cap}</figcaption>") };
                        append!(format!("<figure><img src=\"{src}\" alt=\"Figure\"/>{c}</figure>"));
                    }
                }
            }
            _ if used(&cap_of, i) => {}
            Kind::Title => {
                if missing_title {
                    meta.title = p.text();
                }
            }
            Kind::Heading if is_book => {
                let text = p.text();
                let mut t = super::trim_ws(&text).to_string();
                if plan.is_some() {
                    // the outline gives the chapters: a heading it doesn't list is a sub-heading
                    let section = !outline_sections && re!(r"^(\d{1,3}|[A-Z])\.\d{1,3}\.?\s+\S").is_match(&t);
                    let id = next_id();
                    let h = if section { 2 } else { 3 };
                    append!(format!("<h{h} id=\"{id}\">{}</h{h}>", esc(&t)));
                    if section {
                        cur.as_mut().unwrap().headings.push(Heading { level: 2, id, title: t });
                    }
                } else if let (Some(l), true) = (book::chapter_label(&t), merged.get(i + 1).is_some_and(|q| q.kind == Kind::Heading)) {
                    // no outline: "Chapter 3" over the chapter's title makes one title
                    chapter_label = Some(l);
                } else if let Some(l) = chapter_label.take() {
                    t = format!("{l} {t}");
                    let id = next_id();
                    open(&t, id, &mut cur, &mut chapters);
                } else if p.level <= 1 {
                    let id = next_id();
                    open(&t, id, &mut cur, &mut chapters);
                } else {
                    let id = next_id();
                    let h = p.level.min(3);
                    append!(format!("<h{h} id=\"{id}\">{}</h{h}>", esc(&t)));
                    if h == 2 {
                        cur.as_mut().unwrap().headings.push(Heading { level: 2, id, title: t });
                    }
                }
            }
            Kind::Heading => {
                let text = p.text();
                let t = super::trim_ws(&text);
                if t.to_lowercase().starts_with("abstract") {
                    abstract_done = true;
                }
                if p.level <= 1 {
                    let id = next_id();
                    open(t, id, &mut cur, &mut chapters);
                } else {
                    let id = next_id();
                    let h = p.level.min(3);
                    append!(format!("<h{h} id=\"{id}\">{}</h{h}>", esc(t)));
                    if h == 2 {
                        cur.as_mut().unwrap().headings.push(Heading { level: 2, id, title: t.to_string() });
                    }
                }
            }
            Kind::Front => {
                if !(!meta.title.is_empty() && layout::similar(&p.text(), &meta.title)) {
                    front += &format!("<p>{}</p>", styled(p));
                }
            }
            Kind::Abstract => {
                if !abstract_done {
                    let id = next_id();
                    open("Abstract", id, &mut cur, &mut chapters);
                    abstract_done = true;
                }
                let s = styled(p);
                let s = re!(r"(?i)^(<strong>)?Abstract(</strong>)?[.:]?(</strong>)?\s*").replace(&s, "").into_owned();
                append!(format!("<p>{s}</p>"));
            }
            Kind::Equation => {
                if let Some(img) = p.asset.clone() {
                    let src = format!("../images/{}", img.file_name);
                    images.push(img);
                    let wem = p.ink_width.map(|w| w + 4.0).unwrap_or(p.bbox().w() + 8.0) / body_size.max(6.0) * 0.95;
                    // its text as the alternative (search, VoiceOver); its number as text, for references
                    let alt = equations::equation_text(p);
                    let alt = if alt.is_empty() { "equation".to_string() } else { alt };
                    let img = format!("<img class=\"equation\" src=\"{src}\" alt=\"{}\" style=\"width: {wem:.1}em\"/>", esc(&alt));
                    if let Some(label) = &p.eq_label {
                        let key = label.replace('.', "-");
                        let mut eid = format!("eq-{key}");
                        let mut k = 1;
                        while eq_ids.contains(&eid) {
                            k += 1;
                            eid = format!("eq-{key}-{k}");
                        }
                        eq_ids.insert(eid.clone());
                        append!(format!("<div class=\"equation\" id=\"{eid}\"><span class=\"eqlabel eqno\">({})</span>{img}</div>", esc(label)));
                    } else {
                        append!(format!("<div class=\"equation\">{img}</div>"));
                    }
                }
            }
            Kind::Caption => {
                append!(format!("<p class=\"caption\">{}</p>", styled(p)));
            }
            Kind::Footnote | Kind::Sidenote => {}
            Kind::Text => {
                if abstract_done && !intro_done && last_kind == Some(Kind::Abstract) && !has_intro && p.boxed.is_none() {
                    let id = next_id();
                    open("Introduction", id, &mut cur, &mut chapters);
                }
                intro_done = intro_done || last_kind == Some(Kind::Heading);
                append!(format!("<p>{}</p>", styled(p)));
            }
        }
        if !matches!(p.kind, Kind::Front | Kind::Footnote | Kind::Sidenote) {
            last_kind = Some(p.kind);
        }
    }
    if let Some(c) = cur.as_mut() {
        if in_list {
            c.html += "</ol>";
        }
        if open_box.is_some() {
            c.html += "</div>";
        }
    }
    close(&mut cur, &mut chapters);
    place_footnotes(&footnotes, &mut chapters);
    if is_book {
        // outline entries with nothing under them (a "Contents" entry, its pages left out)
        chapters.retain(|c| re!(r"<(p|img|li|figure|table|div)\b").is_match(&c.html));
    }
    if !front.is_empty() && meta.affiliations_html.is_empty() {
        meta.affiliations_html = layout::bullet_lists(&front);
    }
    if chapters.is_empty() {
        return Err(fail("Could not find any text in the PDF (is it a scanned image?)."));
    }
    let mut notes = vec!["Converted from the PDF without AI: equations and tables are kept as images.".to_string()];
    if is_book {
        notes.push(format!(
            "Converted as a {}{}: {}.",
            info.kind.as_str(),
            if plan.is_some() { ", chapters from the PDF's outline" } else { "" },
            info.reasons.join("; ")
        ));
    }
    Ok(Book {
        meta,
        chapters,
        images,
        source_label: "PDF (layout analysis)".into(),
        notes,
        kind: info.kind,
        kind_reasons: info.reasons.clone(),
    })
}

#[cfg(test)]
mod tests;
