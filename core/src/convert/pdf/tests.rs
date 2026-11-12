//! PDF analysis tests. The layout tests feed synthetic pages to the analyser and need nothing
//! else; the end-to-end tests generate a small PDF and need PDFium, so they are `#[ignore]`d:
//! `PDFIUM_PATH=/path/to/libpdfium.dylib cargo test pdf -- --ignored`.

use super::layout::*;
use super::*;

fn line(text: &str, page: usize, x0: f64, y0: f64, size: f64) -> PLine {
    let w = 0.5 * size * text.chars().count() as f64;
    PLine {
        text: text.into(),
        bbox: R::new(x0, y0, x0 + w, y0 + size * 1.2),
        size,
        bold: 0.0,
        math: 0.0,
        spans: vec![Span { text: text.into(), ..Default::default() }],
        page,
    }
}

fn bold(mut l: PLine) -> PLine {
    l.bold = 1.0;
    l.spans.iter_mut().for_each(|s| s.bold = true);
    l
}

/// Wrapped body text: `n` lines of about `width_chars` characters starting at `y`.
fn para(words: &str, page: usize, x0: f64, y: f64, n: usize, width_chars: usize) -> Vec<PLine> {
    let mut out = Vec::new();
    let all: Vec<&str> = words.split(' ').collect();
    let mut k = 0;
    for i in 0..n {
        let mut s = String::new();
        while s.chars().count() < width_chars {
            if !s.is_empty() {
                s.push(' ');
            }
            s += all[k % all.len()];
            k += 1;
        }
        if i == n - 1 {
            s.push('.');
        }
        out.push(line(&s, page, x0, y + i as f64 * 12.0, 10.0));
    }
    out
}

struct White;
impl Renderer for White {
    fn render(&mut self, _page: usize, r: R, scale: f64) -> Option<image::RgbImage> {
        let (w, h) = ((r.w() * scale).round() as u32, (r.h() * scale).round() as u32);
        Some(image::RgbImage::from_pixel(w.max(1), h.max(1), image::Rgb([255, 255, 255])))
    }
}

const LOREM: &str = "the quick brown fox jumps over the lazy dog while seven wizards quietly judge boxing matches";

fn sample_pages() -> Vec<Page> {
    let mut p1 = vec![
        bold(line("A Tiny Test Paper About Boxes", 0, 150.0, 80.0, 18.0)),
        line("Ada Lovelace and Alan Turing", 0, 200.0, 110.0, 11.0),
        bold(line("Abstract", 0, 72.0, 150.0, 11.0)),
    ];
    p1.extend(para(LOREM, 0, 72.0, 170.0, 4, 90));
    p1.push(bold(line("1 Introduction", 0, 72.0, 240.0, 12.0)));
    p1.extend(para(LOREM, 0, 72.0, 260.0, 5, 90));
    p1.push(line("1", 0, 300.0, 760.0, 9.0)); // page number
    let mut p2 = vec![bold(line("2 Methods", 1, 72.0, 72.0, 12.0))];
    p2.extend(para(LOREM, 1, 72.0, 92.0, 3, 90));
    p2.push(line("Figure 1. A grey box drawn as a vector rectangle.", 1, 150.0, 300.0, 9.0));
    p2.push(bold(line("References", 1, 72.0, 340.0, 12.0)));
    p2.push(line("1. Someone. A paper. Journal 2020.", 1, 72.0, 360.0, 10.0));
    let mut g2 = PageGraphics::default();
    g2.drawings.push(R::new(150.0, 140.0, 400.0, 290.0));
    vec![
        Page { width: 612.0, height: 792.0, lines: p1, graphics: PageGraphics::default() },
        Page { width: 612.0, height: 792.0, lines: p2, graphics: g2 },
    ]
}

fn analyse(pages: Vec<Page>) -> (Vec<Para>, f64) {
    let mut r = White;
    let log = |_: &str| {};
    let mut an = Analyzer::new(pages, &mut r, &log);
    let paras = an.analyze();
    (paras, an.body_size)
}

#[test]
fn join_lines_dehyphenates() {
    assert_eq!(join_lines(["exam-", "ple text"]), "example text");
    assert_eq!(join_lines(["A-", "B"]), "A-B");
    assert_eq!(join_lines(["and/", "or", "  ", "more"]), "and/or more");
    assert_eq!(join_lines(["well -", "known"]), "well - known");
}

#[test]
fn styled_marks_scripts_and_styles() {
    let mut l1 = line("E = mc2 and fo-", 0, 0.0, 0.0, 10.0);
    l1.spans = vec![
        Span { text: "E = mc".into(), ..Default::default() },
        Span { text: "2".into(), sup: true, ..Default::default() },
        Span { text: " and ".into(), ..Default::default() },
        Span { text: "fo-".into(), italic: true, ..Default::default() },
    ];
    let mut l2 = line("cus & <done>", 0, 0.0, 12.0, 10.0);
    l2.spans = vec![
        Span { text: "cus".into(), italic: true, ..Default::default() },
        Span { text: " & <done>".into(), bold: true, ..Default::default() },
    ];
    let p = Para::new(vec![l1, l2], 0, Kind::Text);
    assert_eq!(styled(&p), "E = mc<sup>2</sup> and <em>fo</em><em>cus</em> <strong>&amp; &lt;done&gt;</strong>");
}

#[test]
fn similar_titles() {
    assert!(similar("Attention Is All You Need", "attention is all you need."));
    assert!(!similar("Short", "Other"));
    assert!(similar("A long title that is quite long indeed", "A long title that is quite long indeed: with subtitle"));
}

#[test]
fn layout_classifies_title_headings_figures() {
    let (paras, body) = analyse(sample_pages());
    assert_eq!(body, 10.0);
    let kinds: Vec<(Kind, String)> = paras.iter().map(|p| (p.kind, p.text().chars().take(20).collect())).collect();
    let find = |k: Kind| kinds.iter().filter(|x| x.0 == k).map(|x| x.1.clone()).collect::<Vec<_>>();
    assert_eq!(find(Kind::Title), vec!["A Tiny Test Paper Ab"]);
    assert_eq!(find(Kind::Heading), vec!["Abstract", "1 Introduction", "2 Methods", "References"]);
    assert_eq!(find(Kind::Figure).len(), 1);
    assert_eq!(find(Kind::Caption), vec!["Figure 1. A grey box"]);
    // the page number is noise, the author line front matter
    assert!(!kinds.iter().any(|x| x.1 == "1"));
    assert_eq!(find(Kind::Front), vec!["Ada Lovelace and Ala"]);
    let h: Vec<usize> = paras.iter().filter(|p| p.kind == Kind::Heading).map(|p| p.level).collect();
    assert_eq!(h, vec![1, 1, 1, 1]);
}

#[test]
fn assemble_builds_chapters_with_figure_and_caption() {
    let (paras, body) = analyse(sample_pages());
    let opts = Options { offline: true, ..Default::default() };
    let book = assemble(paras, PaperMeta::default(), &PaperMeta::default(), body, &opts).unwrap();
    let titles: Vec<&str> = book.chapters.iter().map(|c| c.title.as_str()).collect();
    assert_eq!(titles, vec!["Abstract", "1 Introduction", "2 Methods", "References"]);
    assert_eq!(book.meta.title, "A Tiny Test Paper About Boxes");
    assert_eq!(book.meta.affiliations_html, "<p>Ada Lovelace and Alan Turing</p>");
    assert_eq!(book.images.len(), 1);
    assert_eq!(book.images[0].file_name, "p2-figure1.png");
    let methods = &book.chapters[2].html;
    assert!(methods.contains(
        "<figure><img src=\"../images/p2-figure1.png\" alt=\"Figure\"/><figcaption>Figure 1. A grey box drawn as a vector rectangle.</figcaption></figure>"
    ));
    assert!(!methods.contains("<p class=\"caption\">"));
    assert_eq!(book.source_label, "PDF (layout analysis)");
}

#[test]
fn two_columns_are_read_left_then_right() {
    let mut lines = Vec::new();
    for i in 0..20 {
        let y = 100.0 + 12.0 * i as f64;
        lines.push(line(&format!("left column line number {i:02} with some more words here"), 0, 50.0, y, 10.0));
        lines.push(line(&format!("right column line number {i:02} with some more words here"), 0, 320.0, y, 10.0));
    }
    for l in lines.iter_mut() {
        l.bbox.x1 = l.bbox.x0 + 240.0; // fit the column
    }
    let page = Page { width: 612.0, height: 792.0, lines, graphics: PageGraphics::default() };
    let mut r = White;
    let log = |_: &str| {};
    let an = Analyzer::new(vec![page.clone()], &mut r, &log);
    let paras = an.paragraphs(0, page.lines.clone(), vec![]);
    let all = paras.iter().map(|p| p.text()).collect::<Vec<_>>().join(" | ");
    let l19 = all.find("left column line number 19").unwrap();
    let r00 = all.find("right column line number 00").unwrap();
    assert!(l19 < r00, "{all}");
}

#[test]
fn running_heads_are_noise() {
    let mut pages = Vec::new();
    for p in 0..4 {
        let mut lines = vec![line("Journal of Tests 12 (2024) 1-10", p, 72.0, 30.0, 8.0)];
        lines.extend(para(LOREM, p, 72.0, 100.0, 5, 90));
        pages.push(Page { width: 612.0, height: 792.0, lines, graphics: PageGraphics::default() });
    }
    let mut r = White;
    let log = |_: &str| {};
    let an = Analyzer::new(pages.clone(), &mut r, &log);
    assert!(an.is_noise(&pages[2].lines[0]));
    assert!(!an.is_noise(&pages[2].lines[1]));
}

// MARK: - PDFium (generated PDF)

/// A small two-page PDF with base-14 fonts: title, authors, abstract, numbered sections, an
/// italic word, a vector "figure" with its caption, references, page numbers, and `doi` in the
/// document info.
pub(crate) fn sample_pdf(doi: &str) -> Vec<u8> {
    fn t(font: &str, size: f64, x: f64, y: f64, s: &str) -> String {
        format!("BT /{font} {size} Tf {x} {y} Td ({s}) Tj ET\n")
    }
    let body = |y0: f64, n: usize| -> String {
        let words = [
            "Boxes are everywhere in science and their careful study rewards the patient reader with",
            "insight into containers of every kind, from cardboard to the abstract sets used by",
            "mathematicians, and we describe here a few simple experiments with grey rectangles drawn",
            "on a page, measured by a ruler and recorded in a notebook for later analysis by the",
            "authors, who are grateful to everyone who helped with the boxes during the long winter",
        ];
        (0..n).map(|i| t("F2", 10.0, 72.0, y0 - 12.0 * i as f64, words[i % words.len()])).collect::<String>()
            + &t("F2", 10.0, 72.0, y0 - 12.0 * n as f64, "months of this year.")
    };
    let p1 = String::new()
        + &t("F1", 18.0, 150.0, 720.0, "A Tiny Test Paper About Boxes")
        + &t("F2", 11.0, 220.0, 696.0, "Ada Lovelace and Alan Turing")
        + &t("F1", 11.0, 72.0, 660.0, "Abstract")
        + &body(644.0, 3)
        + &t("F1", 12.0, 72.0, 580.0, "1 Introduction")
        + &body(562.0, 4)
        + "BT /F2 10 Tf 72 490 Td (We also like ) Tj /F3 10 Tf (italic boxes) Tj /F2 10 Tf ( very much indeed.) Tj ET\n"
        + &t("F2", 9.0, 300.0, 40.0, "1");
    let p2 = String::new()
        + &t("F1", 12.0, 72.0, 720.0, "2 Methods")
        + &body(702.0, 3)
        + "0.6 g 150 440 250 150 re f 0 g\n"
        + &t("F2", 9.0, 150.0, 425.0, "Figure 1. A grey box drawn as a vector rectangle.")
        + &t("F1", 12.0, 72.0, 380.0, "References")
        + &t("F2", 10.0, 72.0, 362.0, "1. Someone. A paper about boxes. Journal of Boxes 2020.")
        + &t("F2", 9.0, 300.0, 40.0, "2");
    let fonts = "<< /F1 4 0 R /F2 5 0 R /F3 6 0 R >>";
    let page = |contents: usize| {
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font {fonts} >> /Contents {contents} 0 R >>")
    };
    let objs: Vec<String> = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Kids [3 0 R 8 0 R] /Count 2 >>".into(),
        page(7),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold >>".into(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Times-Roman >>".into(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Times-Italic >>".into(),
        format!("<< /Length {} >>\nstream\n{p1}endstream", p1.len()),
        page(9),
        format!("<< /Length {} >>\nstream\n{p2}endstream", p2.len()),
        format!(
            "<< /Title (A Tiny Test Paper About Boxes) /Author (Ada Lovelace; Alan Turing) /Subject (doi:{doi}) /CreationDate (D:20240203120000Z) >>"
        ),
    ];
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).bytes());
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).bytes());
    for o in offsets {
        out.extend(format!("{o:010} 00000 n \n").bytes());
    }
    let n = objs.len();
    out.extend(format!("trailer\n<< /Size {} /Root 1 0 R /Info {n} 0 R >>\nstartxref\n{xref}\n%%EOF\n", n + 1).bytes());
    out
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("p2e-test-{name}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn read(z: &mut zip::ZipArchive<std::fs::File>, name: &str) -> String {
    let mut s = String::new();
    std::io::Read::read_to_string(&mut z.by_name(name).unwrap(), &mut s).unwrap();
    s
}

#[test]
#[ignore]
fn pdfium_end_to_end() {
    let dir = tmp("pdf");
    let data = sample_pdf("10.1234/boxes.2024.1");
    let pdf = dir.join("boxes.pdf");
    std::fs::write(&pdf, &data).unwrap();
    assert_eq!(doi_in_pdf(&data).as_deref(), Some("10.1234/boxes.2024.1"));
    assert!(first_pages_text(&data, 1).contains("A Tiny Test Paper About Boxes"));
    let opts = Options { offline: true, ..Default::default() };
    let r = super::super::convert_file_with(&pdf, &dir, &opts).unwrap();
    assert_eq!(r.source, "PDF (layout analysis)");
    assert_eq!(r.title, "A Tiny Test Paper About Boxes");
    assert_eq!(r.meta.authors, vec!["Ada Lovelace", "Alan Turing"]);
    assert_eq!(r.meta.date, "2024");
    assert_eq!(r.meta.doi, "10.1234/boxes.2024.1");
    assert_eq!(r.images, 1);
    assert!(r.file.ends_with("a-tiny-test-paper-about-boxes.epub"));
    assert!(r.fulltext.contains("We also like italic boxes very much indeed."), "{}", r.fulltext);

    let mut z = zip::ZipArchive::new(std::fs::File::open(&r.file).unwrap()).unwrap();
    let nav = read(&mut z, "EPUB/nav.xhtml");
    for h in ["Abstract", "1 Introduction", "2 Methods", "References"] {
        assert!(nav.contains(h), "{h} missing from {nav}");
    }
    let intro = read(&mut z, "EPUB/text/ch02.xhtml");
    assert!(intro.contains("<em>italic boxes</em>"), "{intro}");
    let methods = read(&mut z, "EPUB/text/ch03.xhtml");
    assert!(methods.contains("<figcaption>Figure 1. A grey box drawn as a vector rectangle.</figcaption>"), "{methods}");
    assert!(z.by_name("EPUB/images/p2-figure1.png").is_ok());
    std::fs::remove_dir_all(dir).ok();
}

#[test]
#[ignore]
fn pdfium_rejects_garbage() {
    let opts = Options { offline: true, ..Default::default() };
    match build(b"%PDF-1.4\nnot really\n", &PaperMeta::default(), &opts) {
        Err(ConvertError::Message(m)) => assert_eq!(m, "Could not open the PDF."),
        other => panic!("unexpected {:?}", other.map(|b| b.chapters.len())),
    }
}

/// Online, a PDF's DOI leads to its open XML version (needs network and PDFium).
#[test]
#[ignore]
fn network_pdf_prefers_xml_via_doi() {
    let dir = tmp("pdfxml");
    let path = dir.join("paper.pdf");
    // DOI of PMC7029158, which has open-access XML
    std::fs::write(&path, sample_pdf("10.1016/j.idm.2020.02.001")).unwrap();
    let r = super::super::convert_file_with(&path, &dir, &Options::default()).unwrap();
    assert_eq!(r.source, "PMC open-access XML");
    std::fs::remove_dir_all(dir).ok();
}
