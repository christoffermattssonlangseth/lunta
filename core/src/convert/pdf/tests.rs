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

#[test]
fn line_end_markers_urls_and_captions() {
    // a hyphenation hyphen taken out (GLUE) joins with nothing; a real one (HARD_HYPHEN) is kept
    assert_eq!(join_lines([format!("ques{GLUE}").as_str(), "tions remain"]), "questions remain");
    assert_eq!(join_lines([format!("scA{GLUE}").as_str(), "TAC-seq"]), "scATAC-seq");
    assert_eq!(join_lines([format!("sample{HARD_HYPHEN}").as_str(), "level data"]), "sample-level data");
    // URLs broken across lines go on without a space
    assert_eq!(join_lines(["see https://doi.org/10.", "1038/nature21350."]), "see https://doi.org/10.1038/nature21350.");
    assert_eq!(join_lines(["at https://doi", ".org/10.1/x"]), "at https://doi.org/10.1/x");
    assert_eq!(join_lines(["Fig. 1.", "Next"]), "Fig. 1. Next");
    assert!(!url_continues("plain text.", "next"));
    // styled HTML follows the same joins and never shows the markers
    let p = Para::new(
        vec![line(&format!("sample{HARD_HYPHEN}"), 0, 0.0, 0.0, 10.0), line(&format!("level ques{GLUE}"), 0, 0.0, 12.0, 10.0), line("tions", 0, 0.0, 24.0, 10.0)],
        0,
        Kind::Text,
    );
    assert_eq!(styled(&p), "sample-level questions");
    assert_eq!(p.text(), "sample-level questions");
    // captions: a label followed by punctuation, a capital or nothing; not running text
    for c in ["Figure 2. Foo", "Fig. 5 | Bar", "Figure 3", "Extended Data Fig. 2 | X", "Figure 1 Overview", "Table S1: x", "Figure 1—The model", "Figure 2 - Overview"] {
        assert!(caption_re().is_match(c), "{c}");
    }
    for c in ["Figure S2G). Top genes", "Fig. 1a and b show", "Figure 2, which", "Figure 4) mapped", "Extended Data Fig. 4a–c). These", "Figs. 3-5 show"] {
        assert!(!caption_re().is_match(c), "{c}");
    }
}

#[test]
fn hard_hyphens_only_where_the_pdf_tells_soft_ones_apart() {
    let mk = |texts: &[&str]| {
        let lines = texts.iter().enumerate().map(|(i, t)| line(t, 0, 50.0, 100.0 + 12.0 * i as f64, 10.0)).collect();
        vec![Page { width: 612.0, height: 792.0, lines, graphics: PageGraphics::default() }]
    };
    let g = format!("ab{GLUE}");
    let mut pages = mk(&[&g, &g, &g, "sample-", "cd"]);
    mark_hard_hyphens(&mut pages);
    assert_eq!(pages[0].lines[3].text, format!("sample{HARD_HYPHEN}"));
    assert_eq!(pages[0].lines[3].spans[0].text, format!("sample{HARD_HYPHEN}"));
    // no glued lines: the old rule (drop the hyphen before a lowercase letter) stays
    let mut pages = mk(&["sample-", "level"]);
    mark_hard_hyphens(&mut pages);
    assert_eq!(pages[0].lines[0].text, "sample-");
}

#[test]
fn publisher_footers_and_legend_notes_are_noise() {
    let mut lines = para(LOREM, 0, 72.0, 100.0, 8, 90);
    lines.push(line("Cell 190, 1–19, January 21, 2027 © 2026 Elsevier Inc. All rights are reserved.", 0, 72.0, 740.0, 7.0));
    lines.push(line("Jiang et al., 2027, Cell 190, 1–19", 0, 72.0, 752.0, 7.0));
    lines.push(line("(legend continued on next page)", 0, 400.0, 500.0, 7.5));
    lines.push(line("(Legend on next page)", 0, 400.0, 520.0, 7.0));
    let page = Page { width: 612.0, height: 792.0, lines, graphics: PageGraphics::default() };
    let mut r = White;
    let log = |_: &str| {};
    let an = Analyzer::new(vec![page.clone()], &mut r, &log);
    let n = page.lines.len();
    for l in &page.lines[n - 4..] {
        assert!(an.is_noise(l), "{}", l.text);
    }
    assert!(!an.is_noise(&page.lines[0]));
}

/// Two columns with full-width legends between them, and a left column that runs past the
/// right one: the gutter comes from lines side by side, each band is read left then right.
#[test]
fn two_columns_with_full_width_blocks_read_in_bands() {
    let mut lines = Vec::new();
    let col = |s: &str, x: f64, y: f64| {
        let mut l = line(s, 0, x, y, 10.0);
        l.bbox.x1 = x + 240.0;
        l
    };
    for i in 0..6 {
        let y = 80.0 + 12.0 * i as f64;
        lines.push(col(&format!("top left line {i} with words"), 50.0, y));
        lines.push(col(&format!("top right line {i} with words"), 320.0, y));
    }
    // a full-width legend: as much text as both columns
    for i in 0..8 {
        let mut l = line(&format!("wide legend line {i} that runs across both of the columns of the page"), 0, 50.0, 180.0 + 9.0 * i as f64, 7.5);
        l.bbox.x1 = 560.0;
        lines.push(l);
    }
    for i in 0..6 {
        let y = 280.0 + 12.0 * i as f64;
        lines.push(col(&format!("bottom left line {i} with words"), 50.0, y));
        lines.push(col(&format!("bottom right line {i} with words"), 320.0, y));
    }
    let page = Page { width: 612.0, height: 792.0, lines, graphics: PageGraphics::default() };
    let mut r = White;
    let log = |_: &str| {};
    let an = Analyzer::new(vec![page.clone()], &mut r, &log);
    let units: Vec<Para> = page.lines.iter().map(|l| Para::new(vec![l.clone()], 0, Kind::Text)).collect();
    let (ordered, cols) = an.order_with_columns(0, units);
    let texts: Vec<String> = ordered.iter().map(|p| p.text()).collect();
    let g = cols[texts.iter().position(|t| t.starts_with("top left line 0")).unwrap()].1;
    assert!(g > 290.0 && g < 320.0, "{g}");
    let pos = |s: &str| texts.iter().position(|t| t.starts_with(s)).unwrap();
    assert!(pos("top left line 5") < pos("top right line 0"));
    assert!(pos("top right line 5") < pos("wide legend line 0"));
    assert!(pos("wide legend line 7") < pos("bottom left line 0"));
    assert!(pos("bottom left line 5") < pos("bottom right line 0"));
}

/// Three columns with a narrow gutter, a framed box across the first two (with columns of its
/// own), a full-width figure and a legend in two columns under it: each region is read column by
/// column, the box as one block.
#[test]
fn three_columns_box_and_two_column_legend() {
    let mut lines = Vec::new();
    let col = |s: &str, x: f64, y: f64, w: f64, size: f64| {
        let mut l = line(s, 0, x, y, size);
        l.bbox.x1 = x + w;
        l.bbox.y1 = y + size;
        l
    };
    // the box: two columns of its own, gutter at 200 (the page's first gutter is at 212)
    for i in 0..5 {
        let y = 60.0 + 11.0 * i as f64;
        lines.push(col(&format!("box left {i} words words"), 45.0, y, 150.0, 8.0));
        lines.push(col(&format!("box right {i} words words"), 205.0, y, 170.0, 8.0));
    }
    for i in 0..20 {
        let y = 140.0 + 11.0 * i as f64;
        lines.push(col(&format!("col one {i:02} words words"), 40.0, y, 166.0, 8.5));
        lines.push(col(&format!("col two {i:02} words words"), 218.0, y, 166.0, 8.5));
    }
    for i in 0..27 {
        let y = 60.0 + 11.0 * i as f64;
        lines.push(col(&format!("col three {i:02} words words"), 395.0, y, 166.0, 8.5));
    }
    // a legend in two columns, gutter at 303
    for i in 0..5 {
        let y = 600.0 + 10.0 * i as f64;
        lines.push(col(&format!("legend left {i} words"), 40.0, y, 258.0, 7.0));
        lines.push(col(&format!("legend right {i} words"), 308.0, y, 254.0, 7.0));
    }
    let mut graphics = PageGraphics::default();
    graphics.drawings.push(R::new(40.0, 55.0, 384.0, 118.0)); // the box's frame
    let page = Page { width: 595.0, height: 792.0, lines, graphics };
    let mut r = White;
    let log = |_: &str| {};
    let an = Analyzer::new(vec![page.clone()], &mut r, &log);
    let mut units: Vec<Para> = page.lines.iter().map(|l| Para::new(vec![l.clone()], 0, Kind::Text)).collect();
    let mut fig = Para::new(vec![], 0, Kind::Figure);
    fig.region = Some(R::new(50.0, 400.0, 550.0, 590.0));
    units.push(fig);
    let (ordered, _) = an.order_with_columns(0, units);
    let texts: Vec<String> = ordered.iter().map(|p| p.text()).collect();
    let pos = |s: &str| texts.iter().position(|t| t.starts_with(s)).unwrap_or_else(|| panic!("{s} missing"));
    assert!(pos("box left 4") < pos("box right 0"), "{texts:?}");
    assert!(pos("box right 4") < pos("col one 00"), "{texts:?}");
    assert!(pos("col one 19") < pos("col two 00"), "{texts:?}");
    assert!(pos("col two 19") < pos("col three 00"), "{texts:?}");
    assert!(pos("col three 26") < pos("legend left 0"), "{texts:?}");
    assert!(pos("legend left 4") < pos("legend right 0"), "{texts:?}");
    for i in 0..4 {
        assert_eq!(pos(&format!("col one {:02}", i + 1)), pos(&format!("col one {i:02}")) + 1);
    }
    let figure = ordered.iter().position(|p| p.kind == Kind::Figure).unwrap();
    assert!(pos("col three 26") < figure && figure < pos("legend left 0"), "{texts:?}");
    assert!(ordered.iter().filter(|p| p.text().starts_with("box")).all(|p| p.boxed.is_some()));
    assert!(ordered.iter().filter(|p| p.text().starts_with("col")).all(|p| p.boxed.is_none()));

    // through the whole analysis: the legend is one caption, the box's lines stay in the box and
    // the page's text reads on
    let (paras, _) = analyse(vec![page]);
    let all: Vec<String> = paras.iter().map(|p| p.text()).collect();
    assert!(all.iter().any(|t| t.starts_with("legend left 0") && t.ends_with("legend right 4 words")), "{all:?}");
}

/// A two-page piece without section headings (the first is "References"): no guessed abstract,
/// and its footer, the same on both pages, is a running foot.
#[test]
fn piece_without_headings_has_no_guessed_abstract() {
    let mut p1 = vec![bold(line("A Tiny Test Paper About Boxes", 0, 72.0, 80.0, 18.0)), line("By Ada Lovelace", 0, 72.0, 110.0, 11.0)];
    p1.extend(para(LOREM, 0, 72.0, 140.0, 12, 90));
    p1.push(line("Journal of Boxes | Volume 3 | 2026 | 101", 0, 72.0, 760.0, 8.0));
    p1.push(line("Check for updates", 0, 450.0, 110.0, 8.0));
    p1.push(line("https://doi.org/10.1234/boxes.5", 0, 400.0, 60.0, 8.0));
    let mut p2 = para(LOREM, 1, 72.0, 72.0, 6, 90);
    p2.push(bold(line("References", 1, 72.0, 160.0, 10.0)));
    p2.push(line("1. Someone. A paper. Journal 2020.", 1, 72.0, 175.0, 10.0));
    p2.push(line("Journal of Boxes | Volume 3 | 2026 | 102", 1, 72.0, 760.0, 8.0));
    let pages = vec![
        Page { width: 612.0, height: 792.0, lines: p1, graphics: PageGraphics::default() },
        Page { width: 612.0, height: 792.0, lines: p2, graphics: PageGraphics::default() },
    ];
    let (paras, _) = analyse(pages);
    assert!(!paras.iter().any(|p| p.kind == Kind::Abstract));
    assert!(paras.iter().any(|p| p.kind == Kind::Text && p.page == 0 && count(&p.text()) > 500));
    let all = paras.iter().map(|p| p.text()).collect::<Vec<_>>().join(" | ");
    assert!(!all.contains("Journal of Boxes") && !all.contains("Check for updates"), "{all}");
    let doi = paras.iter().find(|p| p.text().starts_with("https://doi.org/")).unwrap();
    assert_eq!(doi.kind, Kind::Front);
}

#[test]
fn summary_page_highlights_are_front_matter() {
    let mut p1 = vec![bold(line("Highlights", 0, 72.0, 100.0, 11.0))];
    p1.extend(para(LOREM, 0, 72.0, 120.0, 3, 60));
    let mut p2 = vec![
        bold(line("A Tiny Test Paper About Boxes", 1, 72.0, 80.0, 18.0)),
        line("Ada Lovelace and Alan Turing", 1, 72.0, 110.0, 11.0),
        bold(line("SUMMARY", 1, 72.0, 150.0, 11.0)),
    ];
    p2.extend(para(LOREM, 1, 72.0, 170.0, 4, 90));
    let pages = vec![
        Page { width: 612.0, height: 792.0, lines: p1, graphics: PageGraphics::default() },
        Page { width: 612.0, height: 792.0, lines: p2, graphics: PageGraphics::default() },
    ];
    let (paras, _) = analyse(pages);
    let hl = paras.iter().find(|p| p.text() == "Highlights").unwrap();
    assert_eq!(hl.kind, Kind::Front);
    assert!(paras.iter().any(|p| p.kind == Kind::Heading && p.text() == "SUMMARY"));
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

/// A PDF of the given page content streams (612 × 792, base-14 fonts F1 Helvetica-Bold, F2
/// Times-Roman) with `title` in the document info.
fn pdf_of(contents: &[String], title: &str) -> Vec<u8> {
    let n = contents.len();
    let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", 5 + 2 * i)).collect();
    let mut objs: Vec<String> = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        format!("<< /Type /Pages /Kids [{}] /Count {n} >>", kids.join(" ")),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold >>".into(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Times-Roman >>".into(),
    ];
    for (i, c) in contents.iter().enumerate() {
        objs.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 3 0 R /F2 4 0 R >> >> /Contents {} 0 R >>",
            6 + 2 * i
        ));
        objs.push(format!("<< /Length {} >>\nstream\n{c}endstream", c.len()));
    }
    objs.push(format!("<< /Title ({title}) /Author (Ada Lovelace) >>"));
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
    let k = objs.len();
    out.extend(format!("trailer\n<< /Size {} /Root 1 0 R /Info {k} 0 R >>\nstartxref\n{xref}\n%%EOF\n", k + 1).bytes());
    out
}

/// `text` in lines of at most `n` characters.
fn wrap(text: &str, n: usize) -> Vec<String> {
    let mut out: Vec<String> = vec![String::new()];
    for w in text.split(' ') {
        let cur = out.last_mut().unwrap();
        if !cur.is_empty() && cur.len() + 1 + w.len() > n {
            out.push(w.to_string());
        } else {
            if !cur.is_empty() {
                cur.push(' ');
            }
            cur.push_str(w);
        }
    }
    out
}

const COLUMNS_BODY: &str = "Most models in biology are built for a single task and trained on examples that someone has \
    labelled by hand, which takes a great deal of expert effort for every new question. A foundation model is \
    trained once on a broad collection of one kind of data and then adapted to many tasks with little extra \
    training, so that the costly part of the work is shared between laboratories and problems. The model \
    learns a numerical summary of each protein, cell or image that places similar inputs close together, and \
    researchers can use these summaries as inputs to familiar analyses or adapt the model to their own data \
    with a small set of examples they have outlined themselves. Such models do not remove the need for \
    validation: in careful comparisons they do not always beat simple baselines designed for one task, and \
    their predictions should be checked against established methods and confirmed by experiments before \
    anyone relies on them in practice for decisions about patients or the direction of a research programme. \
    A reasonable approach is to compare a foundation model with the methods already in use, to test it on \
    data that resemble the intended use, and to report how it was adapted so that others can repeat the work \
    and learn from its failures as well as from its successes over the coming years of rapid development.";
const COLUMNS_LEGEND: &str = "Figure 1. A grey box drawn as a vector rectangle across the whole page. Its legend \
    is set in two columns under it, narrower than the three of the page: the left half of the legend comes first \
    and it goes on in the right half, which ends the legend after a few more words.";

const COLUMNS_BOX: &str = "A primer on foundation models for biologists, with tools to access and run them, \
    a repository of general models, a plug-in for segmenting cells in microscopy images, and a critical \
    evaluation of how well such models predict the effects of genetic perturbations against simple baselines.";

/// A two-page piece like a journal's opinion column: three columns with a narrow gutter, a
/// full-width figure with its legend in two columns, a footer on both pages and a Crossmark
/// badge, then two columns, the references and a framed box.
pub(crate) fn columns_pdf() -> Vec<u8> {
    fn t(font: &str, size: f64, x: f64, y: f64, s: &str) -> String {
        format!("BT /{font} {size} Tf {x} {y} Td ({s}) Tj ET\n")
    }
    let body = wrap(COLUMNS_BODY, 34);
    let legend = wrap(COLUMNS_LEGEND, 62);
    let mut p1 = t("F1", 18.0, 50.0, 740.0, "Columns Around a Figure") + &t("F2", 11.0, 50.0, 716.0, "By Ada Lovelace");
    p1 += &t("F2", 7.0, 500.0, 740.0, "Check for updates");
    let per_col = 10;
    for (i, l) in body.iter().take(3 * per_col).enumerate() {
        let x = [50.0, 220.0, 390.0][i / per_col];
        p1 += &t("F2", 9.0, x, 690.0 - 11.0 * (i % per_col) as f64, l);
    }
    p1 += "0.6 g 50 330 512 200 re f 0 g\n";
    let half = legend.len().div_ceil(2);
    for (i, l) in legend.iter().enumerate() {
        let x = if i < half { 50.0 } else { 312.0 };
        p1 += &t("F2", 7.0, x, 312.0 - 9.0 * (i % half) as f64, l);
    }
    p1 += &t("F2", 8.0, 50.0, 30.0, "Journal of Columns | Volume 1 | 2026 | 101");
    let mut p2 = String::new();
    let rest: Vec<&String> = body.iter().skip(3 * per_col).collect();
    for (i, l) in rest.iter().enumerate() {
        p2 += &t("F2", 9.0, 50.0, 740.0 - 11.0 * i as f64, l);
    }
    p2 += &t("F1", 10.0, 312.0, 740.0, "References");
    p2 += &t("F2", 8.0, 312.0, 726.0, "1. Someone. A paper about columns. Journal of Columns 2020.");
    // a framed box
    p2 += "0 G 0.5 w 306 560 262 120 re S\n";
    p2 += &t("F1", 9.0, 316.0, 664.0, "Box 1. Further reading");
    for (i, l) in wrap(COLUMNS_BOX, 55).iter().enumerate() {
        p2 += &t("F2", 8.0, 316.0, 650.0 - 10.0 * i as f64, l);
    }
    p2 += &t("F2", 8.0, 50.0, 30.0, "Journal of Columns | Volume 1 | 2026 | 102");
    pdf_of(&[p1, p2], "Columns Around a Figure")
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

/// Three columns, a full-width figure with a two-column legend, a footer and a badge: the text
/// reads column by column, the legend is one caption, the footer and badge are gone, no
/// abstract or introduction is made up for a piece without headings, and the box stays a box. `P2E_TEST_PDF_DIR` keeps
/// the PDF there (to try it with the Swift converter).
#[test]
#[ignore]
fn pdfium_columns_and_figure_legend() {
    let dir = tmp("cols");
    let pdf = dir.join("columns.pdf");
    std::fs::write(&pdf, columns_pdf()).unwrap();
    if let Some(keep) = std::env::var_os("P2E_TEST_PDF_DIR") {
        std::fs::copy(&pdf, PathBuf::from(keep).join("columns.pdf")).unwrap();
    }
    let opts = Options { offline: true, ..Default::default() };
    let r = super::super::convert_file_with(&pdf, &dir, &opts).unwrap();
    let norm = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    let text = norm(&r.fulltext);
    assert!(text.contains(&norm(COLUMNS_BODY)), "{text}");
    assert!(text.contains(&norm(COLUMNS_LEGEND)), "{text}");
    assert!(!text.contains("Journal of Columns |") && !text.contains("Check for updates"), "{text}");
    let mut z = zip::ZipArchive::new(std::fs::File::open(&r.file).unwrap()).unwrap();
    let ch = read(&mut z, "EPUB/text/ch01.xhtml");
    assert!(ch.contains(&format!("<figcaption>{}</figcaption>", norm(COLUMNS_LEGEND))), "{ch}");
    let nav = read(&mut z, "EPUB/nav.xhtml");
    assert!(!nav.contains("Abstract") && !nav.contains("Introduction") && nav.contains("References"), "{nav}");
    let refs = read(&mut z, "EPUB/text/ch02.xhtml");
    let boxed = refs.split("<div class=\"box\">").nth(1).and_then(|b| b.split("</div>").next()).unwrap_or_default();
    assert!(boxed.contains("Box 1. Further reading") && boxed.contains(&format!("<p>{}</p>", norm(COLUMNS_BOX))), "{refs}");
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
