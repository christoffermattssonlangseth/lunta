//! **Figure marks**: a region of a figure's image marked in the figure viewer (a rectangle or an
//! ellipse, "panel c", a cluster in a UMAP), with a colour and an optional note.
//!
//! A mark is an `Annotation` of kind `FigureMark`, so it syncs, undoes, counts and lists as
//! highlights do: `location` "figure", `position` its `FigureRegion` as JSON (the figure's label,
//! its image's file name and content hash, the region from 0 to 1 of the image), `quote`
//! "Figure 3 (region)" (what lists, outlines and summaries show for it), `suffix` the start of the
//! figure's caption (what a summary may say about the mark besides its note), `prefix` empty.
//! The reading view never looks for its quote in the text (the apps leave marks out there).
//!
//! Versions before 1.3 can't read the kind, so they skip a mark's op when they replay the logs
//! (`Store::replay` skips every line it can't read), exactly as they skip ops added after them.
//!
//! A reading view made anew (Remake, a better source) may name its images differently and number
//! its figures differently: `place_figure_marks` finds each mark's figure again by its label and
//! image name, else by the image's content, else by the label alone.

use crate::model::*;
use crate::Library;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A mark's `location`.
pub const LOCATION: &str = "figure";

/// Whether the annotation is a figure mark.
pub fn is_mark(a: &Annotation) -> bool {
    a.kind == AnnotationKind::FigureMark || a.location == LOCATION
}

/// The mark's region, if it is a mark.
pub fn region_of(a: &Annotation) -> Option<FigureRegion> {
    if !is_mark(a) {
        return None;
    }
    serde_json::from_str(&a.position).ok()
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// "Figure 3 (region)": what a mark shows as its quote.
pub fn quote_for(label: &str) -> String {
    let l = one_line(label);
    format!("{} (region)", if l.is_empty() { "Figure" } else { &l })
}

/// The start of a caption, on one line: at most `CONTEXT` characters, cut at a word.
pub fn caption_context(caption: &str) -> String {
    let c = one_line(caption);
    if c.chars().count() <= CONTEXT {
        return c;
    }
    let cut: String = c.chars().take(CONTEXT).collect();
    let cut = match cut.rfind(' ') {
        Some(i) if i > CONTEXT / 2 => cut[..i].to_string(),
        _ => cut,
    };
    format!("{}…", cut.trim_end_matches(|ch: char| ch == ',' || ch == ';' || ch == ':' || ch.is_whitespace()))
}

const CONTEXT: usize = 240;

/// The region inside the image (0–1), with a known shape.
fn tidy(mut r: FigureRegion) -> FigureRegion {
    let f = |v: f64| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 };
    // a rectangle drawn from right to left or bottom to top
    if r.w < 0.0 {
        r.x += r.w;
        r.w = -r.w;
    }
    if r.h < 0.0 {
        r.y += r.h;
        r.h = -r.h;
    }
    r.x = f(r.x);
    r.y = f(r.y);
    r.w = f(r.w).min(1.0 - r.x);
    r.h = f(r.h).min(1.0 - r.y);
    r.shape = if r.shape.trim().eq_ignore_ascii_case("ellipse") { "ellipse".into() } else { "rect".into() };
    r
}

/// A new figure mark (unsaved: give it to `Library::save_annotation`). `caption`: the figure's
/// caption as the viewer shows it (its start is kept with the mark).
#[uniffi::export]
pub fn new_figure_mark(paper_id: String, region: FigureRegion, color: String, note: String, caption: String) -> Annotation {
    let region = tidy(region);
    let color = color.trim().to_lowercase();
    Annotation {
        id: String::new(),
        paper_id,
        kind: AnnotationKind::FigureMark,
        color: if crate::highlight::COLORS.contains(&color.as_str()) { color } else { "yellow".into() },
        location: LOCATION.into(),
        quote: quote_for(&region.figure),
        position: serde_json::to_string(&region).unwrap_or_default(),
        prefix: String::new(),
        suffix: caption_context(&caption),
        note: note.trim().to_string(),
        created: 0,
        modified: 0,
    }
}

/// A figure mark's region (nil for a highlight or note).
#[uniffi::export]
pub fn figure_region(annotation: Annotation) -> Option<FigureRegion> {
    region_of(&annotation)
}

/// The mark with this region (moved, resized, another shape); unsaved.
#[uniffi::export]
pub fn set_figure_region(annotation: Annotation, region: FigureRegion) -> Annotation {
    let mut a = annotation;
    let region = tidy(region);
    a.position = serde_json::to_string(&region).unwrap_or_default();
    a.kind = AnnotationKind::FigureMark;
    a.location = LOCATION.into();
    a
}

/// A figure image's content hash (FNV-1a, 64 bits, hex): the same file gives the same hash on
/// every device. "" when the file can't be read. Takes a path or a file:// URL.
#[uniffi::export]
pub fn image_hash(path: String) -> String {
    local_path(&path).and_then(|p| std::fs::read(p).ok()).map(|b| hash_bytes(&b)).unwrap_or_default()
}

pub fn hash_bytes(b: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for x in b {
        h ^= *x as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// One of the reading view's figures as the viewer has it: its label and its images (paths or
/// file:// URLs, in the figure's order).
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct FigureImages {
    pub label: String,
    pub images: Vec<String>,
}

/// Where a mark is in this reading view: figure `figure` (its place in the list given), image
/// `image` of it.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct FigureMarkPlace {
    pub annotation_id: String,
    pub figure: u32,
    pub image: u32,
}

/// A label to compare: "Figure 3", "Fig. 3" and "FIGURE 3." are one figure.
pub fn label_key(label: &str) -> String {
    let k: String = label.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect();
    k.replace("figures", "fig").replace("figure", "fig").replace("figs", "fig")
}

/// The file name in a path or URL ("fig3.jpg"), without a query and percent-decoded.
pub fn file_name(path: &str) -> String {
    let p = path.split(['?', '#']).next().unwrap_or("");
    let last = p.rsplit(['/', '\\']).next().unwrap_or("");
    percent_decode(last)
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() && b[i + 1].is_ascii_hexdigit() && b[i + 2].is_ascii_hexdigit() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A path on this device for a path or file:// URL (None for other URLs).
fn local_path(s: &str) -> Option<PathBuf> {
    if let Some(rest) = s.strip_prefix("file://") {
        let rest = rest.strip_prefix("localhost").unwrap_or(rest);
        let rest = rest.split(['?', '#']).next().unwrap_or("");
        // file:///C:/… on Windows
        let rest = if rest.len() > 2 && rest.as_bytes()[2] == b':' { &rest[1..] } else { rest };
        return Some(PathBuf::from(percent_decode(rest)));
    }
    if s.contains("://") {
        return None;
    }
    Some(PathBuf::from(s))
}

/// Where each mark is among these figures (marks whose figure isn't there are left out):
/// 1. the figure with the mark's label that has an image of the mark's file name;
/// 2. else the image with the mark's content hash (the label or the name changed);
/// 3. else the figure with the mark's label (the nearest to its old place when several have it),
///    the image at the mark's place in it;
/// 4. else, for a mark on a figure without a label, the image of its file name.
#[uniffi::export]
pub fn place_figure_marks(marks: Vec<Annotation>, figures: Vec<FigureImages>) -> Vec<FigureMarkPlace> {
    let keys: Vec<String> = figures.iter().map(|f| label_key(&f.label)).collect();
    let names: Vec<Vec<String>> = figures.iter().map(|f| f.images.iter().map(|i| file_name(i)).collect()).collect();
    let mut hashes: HashMap<(usize, usize), String> = HashMap::new();
    let mut out = vec![];
    for a in &marks {
        let Some(r) = region_of(a) else { continue };
        let key = label_key(&r.figure);
        // "Figure" alone (a figure the page gives no number) names no one figure
        let labelled = !key.is_empty() && key != "fig";
        let same_label: Vec<usize> = if labelled { (0..figures.len()).filter(|&i| keys[i] == key).collect() } else { vec![] };
        let mut found: Option<(usize, usize)> = None;
        // 1. label and file name
        if !r.image.is_empty() {
            found = same_label.iter().find_map(|&i| names[i].iter().position(|n| *n == r.image).map(|j| (i, j)));
        }
        // 2. the same image, renamed or under another label
        if found.is_none() && !r.hash.is_empty() {
            'outer: for (i, f) in figures.iter().enumerate() {
                for (j, img) in f.images.iter().enumerate() {
                    let h = hashes.entry((i, j)).or_insert_with(|| image_hash(img.clone()));
                    if *h == r.hash {
                        found = Some((i, j));
                        break 'outer;
                    }
                }
            }
        }
        // 3. the label alone
        if found.is_none() && !same_label.is_empty() {
            let i = *same_label.iter().min_by_key(|&&i| (i as i64 - r.index as i64).abs()).unwrap();
            let n = figures[i].images.len().max(1);
            found = Some((i, (r.image_index as usize).min(n - 1)));
        }
        // 4. an unlabelled figure's image by its name
        if found.is_none() && !labelled && !r.image.is_empty() {
            found = (0..figures.len()).find_map(|i| names[i].iter().position(|n| *n == r.image).map(|j| (i, j)));
        }
        if let Some((i, j)) = found {
            out.push(FigureMarkPlace { annotation_id: a.id.clone(), figure: i as u32, image: j as u32 });
        }
    }
    out
}

const IMAGE_EXT: [&str; 9] = ["png", "jpg", "jpeg", "gif", "webp", "svg", "tif", "tiff", "bmp"];

fn images_in(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            images_in(&p, out);
        } else if p.extension().and_then(|x| x.to_str()).is_some_and(|x| IMAGE_EXT.contains(&x.to_lowercase().as_str())) {
            out.push(p);
        }
    }
}

#[uniffi::export]
impl Library {
    /// The image file a figure mark is on, in the paper's reading view on this device (by its
    /// file name, else its content): for a thumbnail of the region in lists and exports. None
    /// when the reading view isn't here or no longer has that image.
    pub fn figure_mark_image(&self, paper: String, id: String) -> Option<String> {
        let a = self.annotations(paper.clone()).into_iter().find(|a| a.id == id)?;
        let r = region_of(&a)?;
        let book = self.open_epub(paper).ok()?;
        let mut all = vec![];
        images_in(Path::new(&book.dir), &mut all);
        all.sort();
        let named = |p: &PathBuf| p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if !r.image.is_empty() {
            if let Some(p) = all.iter().find(|p| named(p) == r.image) {
                return Some(p.to_string_lossy().into());
            }
        }
        if !r.hash.is_empty() {
            if let Some(p) = all.iter().find(|p| std::fs::read(p).map(|b| hash_bytes(&b) == r.hash).unwrap_or(false)) {
                return Some(p.to_string_lossy().into());
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn region(label: &str, image: &str, hash: &str) -> FigureRegion {
        FigureRegion { figure: label.into(), image: image.into(), hash: hash.into(), x: 0.1, y: 0.2, w: 0.3, h: 0.4, shape: "rect".into(), ..Default::default() }
    }

    fn mark(id: &str, r: FigureRegion) -> Annotation {
        let mut a = new_figure_mark("p".into(), r, "green".into(), " panel c ".into(), "Figure 3. UMAP of the cells.".into());
        a.id = id.into();
        a
    }

    fn figs(list: &[(&str, &[&str])]) -> Vec<FigureImages> {
        list.iter().map(|(l, i)| FigureImages { label: l.to_string(), images: i.iter().map(|s| s.to_string()).collect() }).collect()
    }

    #[test]
    fn a_mark_is_an_annotation_with_its_region() {
        let a = mark("m", FigureRegion { figure: "Figure 3".into(), x: 0.9, y: -0.2, w: 0.4, h: 0.5, shape: "Ellipse".into(), ..Default::default() });
        assert_eq!((a.kind, a.location.as_str(), a.quote.as_str(), a.color.as_str(), a.note.as_str()),
                   (AnnotationKind::FigureMark, "figure", "Figure 3 (region)", "green", "panel c"));
        assert_eq!(a.suffix, "Figure 3. UMAP of the cells.");
        let r = figure_region(a.clone()).unwrap();
        // kept inside the image
        assert_eq!((r.x, r.y, r.shape.as_str()), (0.9, 0.0, "ellipse"));
        assert!((r.w - 0.1).abs() < 1e-9 && (r.h - 0.5).abs() < 1e-9);
        // drawn from bottom right to top left
        let b = set_figure_region(a.clone(), FigureRegion { x: 0.6, y: 0.7, w: -0.2, h: -0.3, ..r.clone() });
        let rb = figure_region(b.clone()).unwrap();
        assert!((rb.x - 0.4).abs() < 1e-9 && (rb.y - 0.4).abs() < 1e-9 && (rb.w - 0.2).abs() < 1e-9 && (rb.h - 0.3).abs() < 1e-9);
        assert_eq!(b.quote, a.quote);
        // a highlight has no region
        let mut h = a.clone();
        h.kind = AnnotationKind::Highlight;
        h.location = "epub".into();
        assert!(figure_region(h).is_none());
        assert_eq!(quote_for(""), "Figure (region)");
        let long = "word ".repeat(100);
        let c = caption_context(&long);
        assert!(c.ends_with('…') && c.chars().count() <= CONTEXT + 1);
        // an unknown colour is yellow
        assert_eq!(new_figure_mark("p".into(), FigureRegion::default(), "purple".into(), "".into(), "".into()).color, "yellow");
    }

    #[test]
    fn labels_and_names() {
        assert_eq!(label_key("Figure 3"), label_key("Fig. 3"));
        assert_eq!(label_key("FIGURE 3."), "fig3");
        assert_ne!(label_key("Figure 3"), label_key("Figure 4"));
        assert_eq!(label_key("Extended Data Fig. 2"), "extendeddatafig2");
        assert_eq!(file_name("file:///Users/x/My%20Book/EPUB/images/fig%203.jpg?x=1"), "fig 3.jpg");
        assert_eq!(file_name("C:\\books\\fig3.png"), "fig3.png");
        assert_eq!(local_path("file:///tmp/a%20b.png").unwrap(), PathBuf::from("/tmp/a b.png"));
        assert!(local_path("https://example.org/a.png").is_none());
    }

    #[test]
    fn placed_by_label_and_name_then_hash_then_label() {
        let dir = tempfile::tempdir().unwrap();
        let img = |name: &str, bytes: &[u8]| {
            let p = dir.path().join(name);
            std::fs::write(&p, bytes).unwrap();
            p.to_string_lossy().to_string()
        };
        let a3 = img("fig3.png", b"three");
        let a4 = img("fig4.png", b"four");
        let renamed = img("p7-figure2.png", b"three");
        let other = img("p8-figure1.png", b"remade three");
        let h3 = hash_bytes(b"three");
        assert_eq!(image_hash(a3.clone()), h3);
        assert_eq!(image_hash(format!("file://{}", a3)), h3);
        assert_eq!(image_hash("/no/such/file.png".into()), "");

        let m = mark("m", FigureRegion { index: 2, image_index: 0, ..region("Figure 3", "fig3.png", &h3) });
        // the reading view it was made in
        let first = figs(&[("Figure 1", &["fig1.png"]), ("Figure 2", &["fig2.png"]), ("Figure 3", &[&a3]), ("Figure 4", &[&a4])]);
        assert_eq!(place_figure_marks(vec![m.clone()], first), vec![FigureMarkPlace { annotation_id: "m".into(), figure: 2, image: 0 }]);
        // remade: figures numbered differently and the image renamed, the same image: by its content
        let remade = figs(&[("Graphical abstract", &["ga.png"]), ("Fig. 4", &[&a4]), ("Fig. 3", &[&renamed])]);
        assert_eq!(place_figure_marks(vec![m.clone()], remade)[0].figure, 2);
        // remade with the image itself made anew (another file, other bytes): by the label
        let anew = figs(&[("Figure 1", &["x1.png"]), ("FIGURE 3", &["x3a.png", &other])]);
        assert_eq!(place_figure_marks(vec![m.clone()], anew), vec![FigureMarkPlace { annotation_id: "m".into(), figure: 1, image: 0 }]);
        // the figure is gone: not placed
        let gone = figs(&[("Figure 1", &["x1.png"]), ("Figure 4", &[&a4])]);
        assert!(place_figure_marks(vec![m.clone()], gone).is_empty());
        // the label and an image name, but the name is on another figure: the label wins
        let swapped = figs(&[("Figure 1", &["fig3.png"]), ("Figure 3", &["new.png"])]);
        assert_eq!(place_figure_marks(vec![m.clone()], swapped)[0].figure, 1);
        // the second image of a figure in two images
        let two = mark("t", FigureRegion { image_index: 1, ..region("Figure 3", "b.png", "") });
        assert_eq!(place_figure_marks(vec![two.clone()], figs(&[("Figure 3", &["a.png", "b.png"])]))[0].image, 1);
        assert_eq!(place_figure_marks(vec![two], figs(&[("Figure 3", &["c.png", "d.png"])]))[0].image, 1);
        // a figure without a label: by its image's name only
        let ga = mark("g", region("", "ga.png", ""));
        assert_eq!(place_figure_marks(vec![ga.clone()], figs(&[("Figure", &["x.png"]), ("Figure", &["ga.png"])]))[0].figure, 1);
        assert!(place_figure_marks(vec![ga], figs(&[("Figure", &["x.png"])])).is_empty());
        // two figures with the same label: the one nearest the old place
        let near = mark("n", FigureRegion { index: 3, ..region("Figure 1", "", "") });
        assert_eq!(place_figure_marks(vec![near], figs(&[("Figure 1", &["a"]), ("Figure 2", &["b"]), ("Figure 3", &["c"]), ("Figure 1", &["d"])]))[0].figure, 3);
        // highlights are not placed
        let mut h = mark("h", region("Figure 3", "fig3.png", ""));
        h.kind = AnnotationKind::Highlight;
        h.location = "epub".into();
        assert!(place_figure_marks(vec![h], figs(&[("Figure 3", &["fig3.png"])])).is_empty());
    }

    // ---- in the library: synced, kept a mark, old versions skip it, outlines and summaries ----

    fn epub_with_figures() -> Vec<u8> {
        use std::io::Write;
        use zip::write::SimpleFileOptions;
        let mut zw = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let o = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let mut add = |name: &str, data: &[u8]| {
            zw.start_file(name, o).unwrap();
            zw.write_all(data).unwrap();
        };
        add("mimetype", b"application/epub+zip");
        add("META-INF/container.xml", br#"<?xml version="1.0"?><container><rootfiles><rootfile full-path="EPUB/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#);
        add("EPUB/content.opf", br#"<package><manifest><item id="c1" href="text/ch01.xhtml" media-type="application/xhtml+xml"/><item id="f3" href="images/fig3.png" media-type="image/png"/></manifest><spine><itemref idref="c1"/></spine></package>"#);
        add("EPUB/text/ch01.xhtml", "<?xml version=\"1.0\"?>\n<html xmlns=\"http://www.w3.org/1999/xhtml\"><head><title>t</title></head><body>\n<h2>Introduction</h2>\n<p>Lesions grow.</p>\n<h2>Results</h2>\n<p>Microglia gather at the rim.</p>\n<figure><img src=\"../images/fig3.png\"/><figcaption>Figure 3. UMAP of microglia.</figcaption></figure>\n<p>Later text about oligodendrocytes.</p>\n</body></html>".as_bytes());
        add("EPUB/images/fig3.png", b"png three");
        add("EPUB/images/fig4.png", b"png four");
        drop(add);
        zw.finish().unwrap().into_inner()
    }

    fn library(dir: &Path, device: &str) -> std::sync::Arc<Library> {
        let root = dir.join("lib");
        Library::open(root.to_string_lossy().into(), dir.join(format!("cache-{device}")).to_string_lossy().into(), device.into()).unwrap()
    }

    #[test]
    fn marks_sync_stay_marks_and_old_versions_skip_them() {
        let dir = tempfile::tempdir().unwrap();
        let mac = library(dir.path(), "mac");
        let meta = PaperMeta { title: "Lesions".into(), authors: vec!["Petra Kukanja".into()], date: "2024".into(), ..Default::default() };
        let p = mac.add_paper(meta, None, Some(epub_with_figures()), None).unwrap();
        let text = crate::highlight::dom_text(&std::fs::read_to_string(mac.open_epub(p.id.clone()).unwrap().combined).unwrap());
        let fig_at = text.find("Figure 3").unwrap() as u64;
        let r = FigureRegion { figure: "Figure 3".into(), image: "fig3.png".into(), hash: hash_bytes(b"png three"), x: 0.5, y: 0.1, w: 0.2, h: 0.3, start: fig_at, ..Default::default() };
        let m = mac.save_annotation(new_figure_mark(p.id.clone(), r, "blue".into(), "".into(), "Figure 3. UMAP of microglia.".into())).unwrap();
        assert_eq!(mac.paper(p.id.clone()).unwrap().annotation_count, 1);

        // the phone sees it (the same synced folder)
        let phone = library(dir.path(), "phone");
        let got = phone.annotations(p.id.clone());
        assert_eq!(got.len(), 1);
        assert_eq!(figure_region(got[0].clone()).unwrap().x, 0.5);

        // an editor that sets the kind from the note (a highlight's editor) keeps it a mark
        let mut edited = got[0].clone();
        edited.note = "the rim cluster".into();
        edited.kind = AnnotationKind::Note;
        let saved = phone.save_annotation(edited).unwrap();
        assert_eq!((saved.kind, saved.location.as_str()), (AnnotationKind::FigureMark, LOCATION));

        // the image it's on, in the reading view on this device, by name; renamed: by content
        let img = phone.figure_mark_image(p.id.clone(), m.id.clone()).unwrap();
        assert!(img.ends_with("fig3.png"));
        let moved = set_figure_region(saved.clone(), FigureRegion { image: "old-name.png".into(), ..figure_region(saved.clone()).unwrap() });
        phone.save_annotation(moved).unwrap();
        assert!(phone.figure_mark_image(p.id.clone(), m.id.clone()).unwrap().ends_with("fig3.png"));
        let lost = set_figure_region(saved.clone(), FigureRegion { image: "old-name.png".into(), hash: "0".into(), ..figure_region(saved.clone()).unwrap() });
        phone.save_annotation(lost).unwrap();
        assert!(phone.figure_mark_image(p.id.clone(), m.id.clone()).is_none());
        phone.save_annotation(saved.clone()).unwrap();

        // an older version reads the kind of a highlight it knows, but not a mark's: its replay
        // (`serde_json::from_str::<Op>` per line, unreadable lines skipped) skips the op
        #[derive(serde::Deserialize, Debug)]
        #[allow(dead_code)]
        enum OldKind { Highlight, Underline, Note }
        let log = std::fs::read_to_string(dir.path().join("lib/ops/mac.jsonl")).unwrap();
        let line = log.lines().find(|l| l.contains("FigureMark")).unwrap();
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        assert_eq!(v["op"], "annotation_upsert");
        assert!(serde_json::from_value::<OldKind>(v["annotation"]["kind"].clone()).is_err());
        assert!(serde_json::from_value::<OldKind>(serde_json::json!("Highlight")).is_ok());
        // and this version does the same with ops and kinds newer than itself
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().append(true).open(dir.path().join("lib/ops/mac.jsonl")).unwrap();
        let newer = line.replace("FigureMark", "FigureSketch").replace(&m.id, "future-id");
        writeln!(f, "{newer}").unwrap();
        writeln!(f, r#"{{"at":{},"device":"mac","op":"figure_layer","paper":"{}"}}"#, crate::model::now_ms(), p.id).unwrap();
        drop(f);
        let later = library(dir.path(), "laptop");
        let anns = later.annotations(p.id.clone());
        assert_eq!(anns.len(), 1, "the unknown op and kind are skipped, the rest is read");
        assert_eq!(anns[0].id, m.id);

        // outlines and summaries: "Figure 3 (region)" with the note, in the figure's place in
        // the paper, and the caption as all the summary may say about it besides the note
        let h = later.add_highlight(p.id.clone(), "Later text about oligodendrocytes".into(), None, "".into(), "".into()).unwrap();
        let items = later.outline_items(vec![p.id.clone()]);
        assert_eq!(items.iter().map(|i| i.annotation_id.clone()).collect::<Vec<_>>(), vec![m.id.clone(), h.id.clone()]);
        assert_eq!((items[0].quote.as_str(), items[0].note.as_str(), items[0].section.as_str()), ("Figure 3 (region)", "the rim cluster", "Results"));
        assert_eq!(items[0].context, "Figure 3. UMAP of microglia.");
        assert!(items[1].context.is_empty());
        let o = later.outline_from_highlights(vec![p.id.clone()], String::new(), None, None);
        assert!(o.markdown.contains("- “Figure 3 (region)” — *Note:* the rim cluster"), "{}", o.markdown);
        assert!(crate::outline::outline_check(o.markdown.clone(), items.clone()).is_empty());
        let plan = later.summary_plan(vec![p.id.clone()], String::new(), crate::outline::summary::SummaryLength::Short, crate::outline::summary::SummaryStyle::Prose, 25, 6);
        assert!(plan.parts[0].prompt.contains("\"Figure 3 (region)\" (a region the researcher marked on the figure; its caption: \"Figure 3. UMAP of microglia.\") Researcher's note: the rim cluster"), "{}", plan.parts[0].prompt);
        let mark_src: Vec<&crate::outline::summary::SummarySource> = plan.sources.iter().filter(|s| s.annotation_id == m.id).collect();
        // what the caption and the note say is supported; what neither says is not
        assert!(crate::outline::summary::supported("Microglia gather in the rim cluster of the UMAP.", &mark_src));
        assert!(!crate::outline::summary::supported("Astrocytes proliferate strongly after injury.", &mark_src));
    }
}
