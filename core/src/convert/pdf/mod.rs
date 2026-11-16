//! PDF → book without AI: a port of `PDFAnalyzer.swift` / `PDFContent.swift` and the PDF parts
//! of `Converter.swift` (feature "pdf").
//!
//! PDFium (via `pdfium-render`) stands in for PDFKit: per-character text, bounds, font names
//! and sizes, image and path objects, document info and page rendering. The library is loaded
//! at run time — see [`pdfium`] for where it is looked for. The layout heuristics live in
//! [`layout`] and do not touch PDFium.

pub mod extract;
pub mod layout;

use super::{esc, fail, net, Book, BookImage, Chapter, ConvertError, Heading, Options, PaperMeta, Result};
use layout::{re, styled, Analyzer, Kind, Para};
use pdfium_render::prelude::*;
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
    let mut renderer = extract::PdfiumRenderer::new(&doc);
    let mut an = Analyzer::new(pages, &mut renderer, &log);
    let paras = an.analyze();
    let guessed = an.meta_guess(&paras, &info, || quick_doi_doc(&doc));
    let body_size = an.body_size;
    drop(an);
    assemble(paras, guessed, extra, body_size, opts)
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
    let head = re!(r"(?i)^(?:Extended\s+Data\s+|Supplementary\s+|Supporting\s+)?(?:Figure|Fig\.?)\s*S?\d+\s*(?:[|.:—–]|\s-\s)");
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

/// Turns analysed paragraphs into chapters (the rest of Swift's `PDFBook.build`).
pub fn assemble(paras: Vec<Para>, guessed: PaperMeta, extra: &PaperMeta, body_size: f64, opts: &Options) -> Result<Book> {
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
    if std::env::var_os("P2E_DEBUG").is_some() {
        debug_dump(&paras);
    }
    let ends_sentence = |t: &str| t.ends_with('.') || t.ends_with('?') || t.ends_with('!') || t.ends_with(':');
    // merge paragraphs split across columns/pages
    let mut merged: Vec<Para> = Vec::new();
    for p in paras {
        let text = p.text();
        if p.kind == Kind::Text && text.chars().next().is_some_and(|c| c.is_lowercase()) {
            if let Some(j) = merged.iter().rposition(|q| !matches!(q.kind, Kind::Figure | Kind::Table | Kind::Caption)) {
                if merged[j].kind == Kind::Text && merged.len() - j <= 4 && !ends_sentence(&merged[j].text()) {
                    let q = &mut merged[j];
                    let mut lines = std::mem::take(&mut q.lines);
                    lines.extend(p.lines);
                    *q = Para::new(lines, q.page, Kind::Text);
                    continue;
                }
            }
        }
        merged.push(p);
    }
    // figure legends continue after the caption title in smaller type: fold them into the caption
    let mut folded: Vec<Para> = Vec::new();
    for p in merged {
        let text = p.text();
        let t = super::trim_ws(&text);
        let legend_like = re!(r"^\([A-Z][A-Za-z]{0,3}\)\s|^See also Figure|^Scale bars?").is_match(t);
        if p.kind == Kind::Text && p.size() < body_size - 0.4 {
            if let Some(j) = folded.iter().rposition(|q| q.kind == Kind::Caption) {
                // a legend's "(D) …" panels continue after the page's body text when it runs onto the next page
                if (folded.len() - j <= 8 || legend_like)
                    && folded[j].page.abs_diff(p.page) <= 1
                    && (p.size() - folded[j].size()).abs() < 0.8
                    && (legend_like || j == folded.len() - 1)
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
            let d = (cb.y0 - pb.y1).abs().min((pb.y0 - cb.y1).abs());
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

    struct Cur {
        title: String,
        html: String,
        headings: Vec<Heading>,
    }
    let mut chapters: Vec<Chapter> = Vec::new();
    let mut cur: Option<Cur> = None;
    let mut images: Vec<BookImage> = Vec::new();
    let mut front = String::new();
    let mut auto_id = 0;
    let mut next_id = || {
        auto_id += 1;
        format!("s{auto_id}")
    };
    fn close(cur: &mut Option<Cur>, chapters: &mut Vec<Chapter>) {
        if let Some(c) = cur.take() {
            chapters.push(Chapter { title: c.title, html: format!("<section>{}</section>", c.html), headings: c.headings });
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
    for (i, p) in merged.iter().enumerate() {
        // appends to the current chapter, opening "Main text" if there is none
        macro_rules! append {
            ($html:expr) => {{
                let h: String = $html;
                if cur.is_none() {
                    let id = next_id();
                    open("Main text", id, &mut cur, &mut chapters);
                }
                cur.as_mut().unwrap().html += &h;
            }};
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
                    let wem = (p.bbox().w() + 8.0) / body_size.max(6.0) * 0.95;
                    append!(format!(
                        "<div class=\"equation\"><img class=\"equation\" src=\"{src}\" alt=\"equation\" style=\"width: {wem:.1}em\"/></div>"
                    ));
                }
            }
            Kind::Caption => {
                append!(format!("<p class=\"caption\">{}</p>", styled(p)));
            }
            Kind::Text => {
                if abstract_done && !intro_done && last_kind == Some(Kind::Abstract) && !has_intro {
                    let id = next_id();
                    open("Introduction", id, &mut cur, &mut chapters);
                }
                intro_done = intro_done || last_kind == Some(Kind::Heading);
                append!(format!("<p>{}</p>", styled(p)));
            }
        }
        if p.kind != Kind::Front {
            last_kind = Some(p.kind);
        }
    }
    close(&mut cur, &mut chapters);
    if !front.is_empty() && meta.affiliations_html.is_empty() {
        meta.affiliations_html = front;
    }
    if chapters.is_empty() {
        return Err(fail("Could not find any text in the PDF (is it a scanned image?)."));
    }
    Ok(Book {
        meta,
        chapters,
        images,
        source_label: "PDF (layout analysis)".into(),
        notes: vec!["Converted from the PDF without AI: equations and tables are kept as images.".into()],
    })
}

#[cfg(test)]
mod tests;
