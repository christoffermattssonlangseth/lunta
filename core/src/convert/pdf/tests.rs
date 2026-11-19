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

/// Wiley's body font (STIXTwoText) was taken for a math font, so short paragraphs, footnotes and
/// footers became "equations" cropped as images, and pieces of them in two columns were joined into
/// one picture. Running text stays text whatever its fonts say; a real formula is still one; the
/// first page's journal footer, the running feet with "2of11" page numbers and the masthead logo
/// go; an equal-contribution footnote is front matter; a real figure is found.
#[test]
fn prose_in_a_math_font_is_text_and_wiley_furniture_goes() {
    let math = |mut l: PLine| {
        l.math = 1.0;
        l
    };
    let mut pages = Vec::new();
    for p in 0..3 {
        let mut lines = Vec::new();
        if p == 0 {
            lines.push(bold(line("A Tiny Test Paper About Hairpins", p, 54.0, 80.0, 18.0)));
            lines.push(bold(line("1 Introduction", p, 54.0, 280.0, 10.0)));
        }
        // two columns of short paragraphs, the right-hand ones on the middle page in the "math" font
        let y2 = if p == 1 { 400.0 } else { 360.0 };
        for (x, y) in [(54.0, 300.0), (54.0, y2), (314.0, 300.0), (314.0, 360.0)] {
            let m = p == 1 && x > 300.0;
            lines.extend(para(LOREM, p, x, y, 3, 45).into_iter().map(|l| if m { math(l) } else { l }));
        }
        if p == 1 {
            lines.push(math(line("x = a + b", p, 80.0, 352.0, 10.0)));
        }
        let foot = if p % 2 == 1 {
            format!("{}of3 Journal of Tests,2026", p + 1)
        } else {
            format!("Journal of Tests,2026 {}of3", p + 1)
        };
        lines.push(math(line(&foot, p, 54.0, 754.0, 7.0)));
        pages.push(Page { width: 612.0, height: 792.0, lines, graphics: PageGraphics::default() });
    }
    let p0 = &mut pages[0];
    p0.lines.push(math(line("Ada Lovelace and Alan Turing contributed equally to this work.", 0, 54.0, 691.0, 6.0)));
    p0.lines.push(math(line("Journal of Tests, 2026; 32:e70999 https://doi.org/10.1234/t.70999", 0, 54.0, 756.0, 7.0)));
    p0.graphics.drawings.push(R::new(416.0, 42.0, 558.0, 72.0)); // masthead logo
    pages[2].graphics.drawings.push(R::new(150.0, 450.0, 450.0, 640.0));
    pages[2].lines.push(line("FIGURE 1 A grey box drawn as a vector rectangle.", 2, 150.0, 650.0, 8.0));

    let mut r = White;
    let log = |_: &str| {};
    let an = Analyzer::new(pages.clone(), &mut r, &log);
    assert!(an.is_noise(pages[0].lines.iter().find(|l| l.text.starts_with("Journal of Tests, 2026;")).unwrap()));
    for (p, page) in pages.iter().enumerate() {
        assert!(an.is_noise(page.lines.iter().find(|l| l.text.contains("of3")).unwrap()), "page {p}");
    }
    let (paras, _) = analyse(pages);
    let eqs: Vec<String> = paras.iter().filter(|p| p.kind == Kind::Equation).map(|p| p.text()).collect();
    assert_eq!(eqs, vec!["x = a + b".to_string()]);
    assert!(paras.iter().filter(|p| p.kind == Kind::Text && p.text().contains("quick brown fox")).count() >= 8);
    let fn_kind = paras.iter().find(|p| p.text().contains("contributed equally")).map(|p| p.kind);
    assert_eq!(fn_kind, Some(Kind::Front));
    let figs: Vec<usize> = paras.iter().filter(|p| p.kind == Kind::Figure).map(|p| p.page).collect();
    assert_eq!(figs, vec![2]);
    assert!(paras.iter().any(|p| p.kind == Kind::Caption && p.text().starts_with("FIGURE 1")));
    assert!(!paras.iter().any(|p| p.text().contains("Journal of Tests")));
}

/// When most of the running text is in a "math" font, it's the body font and marks no formulas.
#[test]
fn math_font_setting_the_body_text_is_no_math_font() {
    let mut lines: Vec<PLine> = para(LOREM, 0, 54.0, 100.0, 20, 45);
    lines.push(line("x = a + b", 0, 80.0, 400.0, 10.0));
    let page = Page { width: 612.0, height: 792.0, lines, graphics: PageGraphics::default() };
    // all of it in the font: it's the body font
    let mut all = vec![page.clone()];
    all[0].lines.iter_mut().for_each(|l| l.math = 1.0);
    clear_math_if_body_font(&mut all, 10.0);
    assert!(all[0].lines.iter().all(|l| l.math == 0.0));
    // a prose line and the formula (a manuscript's symbols in STIX-Regular): the formula keeps it
    let mut few = vec![page];
    few[0].lines[0].math = 1.0;
    few[0].lines.last_mut().unwrap().math = 1.0;
    clear_math_if_body_font(&mut few, 10.0);
    assert_eq!(few[0].lines.last().unwrap().math, 1.0);
}

#[test]
fn prose_is_not_a_formula() {
    assert!(is_prose("the quick brown fox jumps over the lazy dog while seven wizards judge"));
    assert!(is_prose("Chensirong Yue and Changyun Tu contributed equally to this work."));
    assert!(!is_prose("x = a + b"));
    assert!(!is_prose("f(x) = sin x + cos x for all x ∈ R"));
    assert!(!is_prose("L = − ∑ i y i log p i + λ ∥ w ∥ 2 , where λ > 0"));
}

/// GPT-2 paper: a table in the right column between two rules, its caption below, and the running
/// head's rule over both columns just above it. The table is its own column, not the page's top:
/// the running head and the left column stay text.
#[test]
fn column_table_does_not_take_the_running_heads_rule() {
    let mut lines = vec![line("Language Models are Unsupervised Multitask Learners", 0, 192.0, 46.0, 9.0)];
    for k in 0..18 {
        lines.push(line("to infer and perform many different tasks on examples with this type of format and", 0, 55.0, 68.0 + 12.0 * k as f64, 10.0));
    }
    for k in 0..8 {
        lines.push(line("French sentence and its translation", 0, 315.0, 72.0 + 25.0 * k as f64, 9.0));
    }
    lines.push(line("Table 1. Examples of naturally occurring demonstrations of English", 0, 307.0, 303.0, 9.0));
    for k in 0..10 {
        lines.push(line("Common Crawl. Trinh and Le found the best results using a small", 0, 307.0, 340.0 + 12.0 * k as f64, 10.0));
    }
    let mut g = PageGraphics::default();
    g.drawings.push(R::new(55.0, 58.0, 541.0, 58.4)); // the running head's rule
    g.drawings.push(R::new(309.0, 67.0, 540.0, 67.4));
    g.drawings.push(R::new(315.0, 286.0, 534.0, 286.4));
    let page = Page { width: 612.0, height: 792.0, lines, graphics: g };
    let (paras, _) = analyse(vec![page.clone(), page]);
    let table = paras.iter().find(|p| p.kind == Kind::Table).expect("a table");
    let r = table.bbox();
    assert!(r.x0 > 300.0 && r.y0 > 60.0 && r.y1 < 300.0, "{r:?}");
    assert!(paras.iter().any(|p| p.kind == Kind::Text && p.text().starts_with("to infer and perform")));
}

/// Nature's first page: the dates in a box of rules, "Published online" on a shaded strip with the
/// Crossmark badge beside "Check for updates", and the footnote box between two rules at the foot
/// ("A full list of affiliations…", "✉ e-mail: …"), and above the title the masthead's banner.
/// None of it is a figure or a table; the dates and the footnote are front matter, not body text,
/// and the body around them stays.
#[test]
fn first_page_boxes_are_front_matter_not_figures() {
    let mut p1 = vec![
        bold(line("Microglia mediate motor memory decay", 0, 40.0, 110.0, 26.0)),
        bold(line("Ada Lovelace & Alan Turing", 0, 217.0, 240.0, 9.5)),
        line("Received: 15 December 2024", 0, 40.0, 240.0, 8.5),
        line("Accepted: 6 October 2026", 0, 40.0, 260.0, 8.5),
        line("Published online: 17 November 2026", 0, 40.0, 281.0, 8.5),
        line("Check for updates", 0, 52.0, 300.0, 8.5),
    ];
    p1.extend(para(LOREM, 0, 217.0, 302.0, 20, 60).into_iter().map(|mut l| {
        l.size = 10.0;
        l
    }));
    p1.extend(para(LOREM, 0, 40.0, 574.0, 12, 45).into_iter().map(|mut l| {
        l.size = 8.5;
        l
    }));
    p1.push(line("A full list of affiliations appears at the end of the paper.", 0, 40.0, 736.0, 7.5));
    p1.push(line("✉ e-mail: ada@example.org; alan@example.org", 0, 236.0, 736.0, 7.5));
    let mut g = PageGraphics::default();
    g.images.push(R::new(40.0, 76.0, 560.0, 104.0)); // the masthead's banner above the title
    g.images.push(R::new(39.7, 279.9, 171.7, 290.7)); // the strip behind "Published online"
    g.images.push(R::new(39.7, 301.2, 50.6, 310.8)); // the badge
    g.drawings.push(R::new(41.0, 302.0, 49.0, 310.0));
    for y in [256.5, 276.2, 295.7, 315.7] {
        g.drawings.push(R::new(39.7, y, 206.0, y + 0.5));
    }
    g.drawings.push(R::new(39.7, 734.8, 561.3, 735.3)); // the footnote box's rules
    g.drawings.push(R::new(39.7, 754.5, 562.1, 755.0));
    let mut p2 = vec![bold(line("Results", 1, 40.0, 60.0, 11.0))];
    p2.extend(para(LOREM, 1, 40.0, 80.0, 30, 45).into_iter().map(|mut l| {
        l.size = 8.5;
        l
    }));
    let pages = vec![
        Page { width: 595.0, height: 791.0, lines: p1, graphics: g },
        Page { width: 595.0, height: 791.0, lines: p2, graphics: PageGraphics::default() },
    ];
    let (paras, _) = analyse(pages);
    assert!(!paras.iter().any(|p| matches!(p.kind, Kind::Figure | Kind::Table)), "{:?}", paras.iter().map(|p| p.kind).collect::<Vec<_>>());
    for t in ["Published online", "A full list of affiliations", "✉ e-mail"] {
        let k = paras.iter().find(|p| p.text().contains(t)).map(|p| p.kind);
        assert_eq!(k, Some(Kind::Front), "{t}");
    }
    assert!(!paras.iter().any(|p| p.text().contains("Check for updates")));
    let body: usize = paras.iter().filter(|p| p.kind == Kind::Text).map(|p| p.lines.len()).sum();
    assert_eq!(body, 12 + 30);
}

/// An author manuscript's first page: a coloured bar down the left edge ("Europe PMC Funders Author
/// Manuscripts", with its logo) joined by the rules of the header box, and a frame around the
/// abstract. Neither is a figure, and the abstract and body stay text.
#[test]
fn manuscript_side_bar_and_text_frame_are_no_figures() {
    let mut lines = vec![bold(line("G-tract RNA removes Polycomb Repressive Complex 2", 0, 60.0, 80.0, 16.0))];
    lines.extend(para(LOREM, 0, 90.0, 120.0, 12, 80));
    lines.extend(para(LOREM, 0, 90.0, 300.0, 20, 80));
    let mut g = PageGraphics::default();
    g.drawings.push(R::new(18.0, 18.0, 36.0, 720.0)); // the bar
    g.images.push(R::new(20.0, 114.0, 32.0, 126.0)); // its logo
    for r in [R::new(35.5, 18.0, 534.5, 18.2), R::new(35.5, 63.3, 534.5, 63.5), R::new(534.0, 17.5, 534.2, 63.8)] {
        g.drawings.push(r); // the header box
    }
    for r in [R::new(80.0, 112.0, 560.0, 112.2), R::new(80.0, 268.0, 560.0, 268.2), R::new(80.0, 112.0, 80.2, 268.0), R::new(559.8, 112.0, 560.0, 268.0)] {
        g.drawings.push(r); // the abstract's frame
    }
    g.drawings.push(R::new(80.0, 112.0, 84.0, 116.0));
    let page = Page { width: 612.0, height: 792.0, lines, graphics: g };
    let mut other = page.clone();
    other.graphics = PageGraphics::default();
    let (paras, _) = analyse(vec![page, other]);
    assert!(!paras.iter().any(|p| p.page == 0 && p.kind == Kind::Figure), "{:?}", paras.iter().filter(|p| p.kind == Kind::Figure).map(|p| p.bbox()).collect::<Vec<_>>());
    let text_lines: usize = paras.iter().filter(|p| p.page == 0 && matches!(p.kind, Kind::Text | Kind::Abstract)).map(|p| p.lines.len()).sum();
    assert_eq!(text_lines, 32);
}

#[test]
fn first_page_footnotes_of_several_publishers() {
    let small = |t: &str| front_footnote(t, 7.0, 9.0, false);
    for t in [
        "A full list of affiliations appears at the end of the paper. ✉e-mail: a@b.org",
        "* Corresponding author. E-mail address: jane.doe@uni.edu (J. Doe).",
        "*Correspondence: jane.doe@uni.edu",
        "https://doi.org/10.1016/j.cell.2025.01.001 Received 2 May 2024; Received in revised form 1 June 2024; Accepted 3 July 2024",
        "© 2025 The Author(s). Published by Elsevier Inc. This is an open access article under the CC BY license.",
        "To whom correspondence should be addressed. Email: jdoe@pnas.org.",
        "Author contributions: J.D. and A.B. designed research; J.D. performed research.",
        "The authors declare no competing interest.",
        "This article is a PNAS Direct Submission.",
        "Edited by Jane Smith, University of Somewhere; received May 1, 2024; accepted June 2, 2024",
        "†These authors contributed equally to this work.",
        "ORCID: 0000-0002-1825-0097",
        "Open Access This article is licensed under a Creative Commons Attribution 4.0 International License",
        "Correspondence to: J. Doe, Department of Biology, University of X, jdoe@x.edu",
        "Jane Doe, Department of Biology, jdoe@x.edu",
    ] {
        assert!(small(t), "{t}");
    }
    // in body type only at the foot of the page, and only if it starts like a footnote
    assert!(front_footnote("*Correspondence: jane.doe@uni.edu", 9.0, 9.0, true));
    assert!(!front_footnote("*Correspondence: jane.doe@uni.edu", 9.0, 9.0, false));
    for t in [
        "Requests for materials should be addressed to jdoe@x.edu, who will share the code.",
        "These authors found that the effect was published in 2019 and accepted widely.",
        "This article is organised as follows. Section 2 describes the data.",
    ] {
        assert!(!front_footnote(t, 9.0, 9.0, true), "{t}");
    }
    // the body's availability statements are never front matter
    assert!(!small("Data availability: the data are available from jdoe@x.edu on request."));
    assert!(!small("Code availability: https://github.com/x/y; questions to jdoe@x.edu"));
    // nor a small legend that just says something
    assert!(!small("Scale bar, 50 μm. Data are presented as the mean ± s.e.m."));
}

/// A figure of panels in rows (Nature's "a b c / d e"), the letters and axis labels between the
/// rows, more than 22 pt apart: one figure, letters inside it, no heading "a b". Two figures with
/// their captions and body text between them stay two.
#[test]
fn figure_panels_with_letters_are_one_figure() {
    let mut lines = para(LOREM, 0, 40.0, 50.0, 4, 90);
    // two rows of three panels, 45 pt apart, their letters (bold, larger than the body) above them
    let mut g = PageGraphics::default();
    for (row, y) in [(0, 120.0), (1, 275.0)] {
        for (col, x) in [40.0, 220.0, 400.0].into_iter().enumerate() {
            // (a chart: its frame and a bar)
            g.drawings.push(R::new(x + 10.0, y, x + 160.0, y + 110.0));
            g.drawings.push(R::new(x + 40.0, y + 50.0, x + 60.0, y + 110.0));
            let letter = ["a", "b", "c", "d", "e", "f"][row * 3 + col];
            lines.push(bold(line(letter, 0, x, y - 16.0, 12.0)));
            lines.push(line("Time (days)", 0, x + 60.0, y + 113.0, 6.5));
        }
    }
    lines.push(bold(line("Fig. 1 | Panels in two rows.", 0, 40.0, 410.0, 7.0)));
    lines.extend(para(LOREM, 0, 40.0, 430.0, 3, 90));
    // a second figure under its own caption and some text
    g.drawings.push(R::new(60.0, 480.0, 300.0, 600.0));
    g.drawings.push(R::new(320.0, 480.0, 540.0, 600.0));
    lines.push(line("Fig. 2 | Another figure.", 0, 40.0, 610.0, 7.0));
    lines.extend(para(LOREM, 0, 40.0, 630.0, 3, 90));
    let page = Page { width: 595.0, height: 791.0, lines, graphics: g };
    let mut other = page.clone();
    other.graphics = PageGraphics::default();
    let (paras, _) = analyse(vec![page, other]);
    let figs: Vec<R> = paras.iter().filter(|p| p.kind == Kind::Figure).map(|p| p.bbox()).collect();
    assert_eq!(figs.len(), 2, "{figs:?}");
    let f1 = figs[0];
    assert!(f1.x0 <= 50.0 && f1.y0 <= 104.0 && f1.x1 >= 560.0 && f1.y1 >= 385.0 && f1.y1 < 410.0, "{f1:?}");
    let f2 = figs[1];
    assert!(f2.y0 >= 470.0 && f2.x0 <= 60.0 && f2.x1 >= 540.0, "{f2:?}");
    assert!(!paras.iter().any(|p| p.page == 0 && p.kind == Kind::Heading));
    assert!(!paras.iter().any(|p| p.page == 0 && p.kind == Kind::Text && (p.text().len() < 4 || p.text().contains("Time"))));
    assert!(paras.iter().any(|p| p.kind == Kind::Caption && p.text().starts_with("Fig. 2")));
}

/// Two figures side by side, each over its own caption, 30 pt apart, are two figures; a framed
/// figure's corners do not pull its legend into the picture.
#[test]
fn figures_side_by_side_stay_two() {
    let mut lines = para(LOREM, 0, 40.0, 50.0, 4, 90);
    let mut g = PageGraphics::default();
    g.drawings.push(R::new(50.0, 120.0, 280.0, 300.0));
    g.drawings.push(R::new(310.0, 120.0, 545.0, 300.0));
    lines.push(line("Figure 6. Precision.", 0, 50.0, 310.0, 9.0));
    lines.push(line("Figure 7. Recall.", 0, 310.0, 310.0, 9.0));
    lines.extend(para(LOREM, 0, 40.0, 340.0, 3, 90));
    // a frame with rounded corners around a picture and its legend
    for (x, y) in [(50.0, 420.0), (541.0, 420.0), (50.0, 716.0), (541.0, 716.0)] {
        g.drawings.push(R::new(x, y, x + 4.0, y + 4.0));
    }
    for r in [R::new(54.0, 420.0, 541.0, 420.2), R::new(54.0, 720.0, 541.0, 720.2), R::new(50.0, 424.0, 50.2, 716.0), R::new(544.8, 424.0, 545.0, 716.0)] {
        g.drawings.push(r);
    }
    g.images.push(R::new(120.0, 430.0, 470.0, 640.0));
    lines.push(line("Figure 8 A framed picture and its legend, which runs on in the frame", 0, 60.0, 660.0, 8.0));
    for k in 1..4 {
        lines.push(line("for more lines of legend text that are not part of the picture above.", 0, 60.0, 660.0 + 12.0 * k as f64, 8.0));
    }
    let page = Page { width: 595.0, height: 791.0, lines, graphics: g };
    let mut other = page.clone();
    other.graphics = PageGraphics::default();
    let (paras, _) = analyse(vec![page, other]);
    let figs: Vec<R> = paras.iter().filter(|p| p.page == 0 && p.kind == Kind::Figure).map(|p| p.bbox()).collect();
    assert_eq!(figs.len(), 3, "{figs:?}");
    assert!(figs.iter().any(|f| f.x1 < 300.0) && figs.iter().any(|f| f.x0 > 290.0), "{figs:?}");
    assert!(figs.iter().all(|f| f.y1 < 655.0), "{figs:?}");
    assert!(paras.iter().any(|p| p.kind == Kind::Caption && p.text().starts_with("Figure 8")));
}

// MARK: - Reference lists

#[test]
fn reference_headings_entries_and_scores() {
    use super::refs::*;
    for h in ["References", "REFERENCES", "6 References", "6. References", "❚ REFERENCES", "■ REFERENCES", "R E F E R E N C E S",
        "Literature Cited", "References and Notes", "Bibliography", "Literatur", "Références", "Referencias", "Reference list"] {
        assert!(is_heading(h), "{h}");
    }
    for h in ["Reference genes", "Literature review", "References 1. Smith J. A paper. 2001", "Results"] {
        assert!(!is_heading(h), "{h}");
    }
    assert_eq!(heading_text("❚ REFERENCES"), "REFERENCES");
    assert_eq!(label("12. Smith JA, Jones B."), Some(12));
    assert_eq!(label("10.Hino K, Hara Y."), Some(10));
    assert_eq!(label("[3] Y. Guo, Y. Fan"), Some(3));
    assert_eq!(label("1 Hanahan D, Weinberg RA."), Some(1));
    assert_eq!(label("2 A. Wang, Y. Xue"), Some(2));
    assert_eq!(label("2010;257:359–66."), None);
    assert_eq!(label("10.1016/j.cell.2011.02.013"), None);
    for a in ["Smith JA, Jones B (2001)", "Smith, J. A., & Jones, B.", "Cryan, J. F. & Dinan, T. G.", "van der Berg, A. (2003)",
        "J. A. Smith, B. Jones,", "Nelson PT, Jicha GA"] {
        assert!(author_start(a), "{a}");
    }
    for a in ["The cells were washed", "In 2010, Smith and colleagues", "Figure 2 shows"] {
        assert!(!author_start(a), "{a}");
    }
    assert!(score("Nelson PT, Jicha GA, Kryscio RJ. Low sensitivity in clinical diagnoses of dementia. J Neurol. 2010;257:359–66.") >= 4);
    assert!(score("[20] S. Mundlos, Clin. Genet. 2009, 76, 123.") >= 3);
    assert!(score("Swanson, L. W. & Lichtman, J. W. From Cajal to connectome and beyond. Annu. Rev. Neurosci. 39, 197–216 (2016).") >= 3);
    // running text with citations is not a reference
    let body = "We found that these cells were more active than in earlier work (Smith et al., 2010; Jones 2012), which suggests that \
                the pathway is also required in vivo, as has been shown for the related kinases in yeast and in flies.";
    assert!(score(body) < 2, "{}", score(body));
}

/// Body pages, then a list of references that would otherwise read as body text.
fn refs_pages(heading: Option<PLine>, entries: &[&str], numbered_body_list: bool) -> Vec<Page> {
    let mut p1 = vec![
        bold(line("A Tiny Test Paper About Boxes", 0, 150.0, 80.0, 18.0)),
        line("Ada Lovelace and Alan Turing", 0, 200.0, 110.0, 11.0),
        bold(line("Abstract", 0, 72.0, 150.0, 11.0)),
    ];
    p1.extend(para(LOREM, 0, 72.0, 170.0, 4, 90));
    p1.push(bold(line("1 Introduction", 0, 72.0, 240.0, 12.0)));
    p1.extend(para(LOREM, 0, 72.0, 260.0, 30, 90));
    let mut p2 = vec![bold(line("2 Methods", 1, 72.0, 72.0, 12.0))];
    p2.extend(para(LOREM, 1, 72.0, 92.0, 20, 90));
    if numbered_body_list {
        // steps of a protocol: numbered, but running text
        for (k, s) in ["1. The samples were thawed on ice and then were washed twice in the buffer that we had made.",
            "2. These were then spun down, which was done at 4 degrees for ten minutes as we had planned.",
            "3. The pellets were resuspended and were kept on ice until they were used in the assay."].iter().enumerate()
        {
            p2.push(line(s, 1, 72.0, 350.0 + 14.0 * k as f64, 10.0));
        }
    }
    p2.extend(para(LOREM, 1, 72.0, 420.0, 10, 90));
    let mut p3 = vec![bold(line("3 Discussion", 2, 72.0, 72.0, 12.0))];
    p3.extend(para(LOREM, 2, 72.0, 92.0, 8, 90));
    let mut y = 220.0;
    if let Some(h) = heading {
        let mut h = h;
        h.page = 2;
        h.bbox = R::new(72.0, y, 72.0 + h.bbox.w(), y + h.bbox.h());
        p3.push(h);
        y += 9.0;
    }
    for e in entries {
        // a hanging indent: the first line at 72, the rest at 84
        // (wrapped lines run to the right edge, the last one ends short)
        let mut chunks: Vec<String> = vec![String::new()];
        for w in e.split(' ') {
            if chunks.last().unwrap().chars().count() > 55 {
                chunks.push(String::new());
            }
            let c = chunks.last_mut().unwrap();
            if !c.is_empty() {
                c.push(' ');
            }
            *c += w;
        }
        let n = chunks.len();
        for (k, c) in chunks.iter().enumerate() {
            let x = if k == 0 { 72.0 } else { 84.0 };
            let mut l = line(c, 2, x, y, 7.5);
            if k + 1 < n {
                l.bbox.x1 = 320.0;
            }
            p3.push(l);
            y += 9.0;
        }
        y += 1.0;
    }
    vec![
        Page { width: 612.0, height: 792.0, lines: p1, graphics: PageGraphics::default() },
        Page { width: 612.0, height: 792.0, lines: p2, graphics: PageGraphics::default() },
        Page { width: 612.0, height: 792.0, lines: p3, graphics: PageGraphics::default() },
    ]
}

const NUMBERED_REFS: [&str; 7] = [
    "1. Nelson PT, Jicha GA, Kryscio RJ, Abner EL. Low sensitivity in clinical diagnoses of dementia. J Neurol. 2010;257:359–66.",
    "2. Vann Jones SA, O'Brien JT. The prevalence and incidence of dementia with Lewy bodies. Psychol Med. 2014;44:673–83.",
    "3. McKeith IG, Dickson DW, Lowe J, et al. Diagnosis and management of dementia with Lewy bodies. Neurology. 2005;65:1863–72.",
    "4. Ballard C, Howard R. Neuroleptic drugs in dementia: benefits and harm. Nat Rev Neurosci. 2006;7:492–500.",
    "5. Boeve BF, Silber MH, Ferman TJ. REM sleep behavior disorder in Parkinson's disease. J Geriatr Psychiatry Neurol. 2004;17:146–57.",
    "6. Aarsland D, Perry R, Larsen JP, et al. Neuroleptic sensitivity in Parkinson's disease. J Clin Psychiatry. 2005;66:633–7.",
    "7. Kurlan R, Cummings J, Raman R, Thal L. Quetiapine for agitation or psychosis in patients with dementia. Neurology. 2007;68:1356–63.",
];

fn book_of(pages: Vec<Page>) -> Book {
    let (paras, body) = analyse(pages);
    let opts = Options { offline: true, ..Default::default() };
    assemble(paras, PaperMeta::default(), &PaperMeta::default(), body, &opts).unwrap()
}

fn list_items(html: &str) -> Vec<String> {
    let Some(ol) = html.split("<ol class=\"refs\"").nth(1) else { return vec![] };
    let ol = ol.split("</ol>").next().unwrap();
    ol.split("<li>").skip(1).map(|li| li.split("</li>").next().unwrap().to_string()).collect()
}

/// BMC: "References" in the reference type (no bold font name), on the first line of the list's
/// first paragraph: a References chapter with one list item per entry, none of it body text.
#[test]
fn references_heading_on_the_first_line_of_the_list() {
    let book = book_of(refs_pages(Some(line("References", 0, 72.0, 0.0, 7.5)), &NUMBERED_REFS, false));
    let last = book.chapters.last().unwrap();
    assert_eq!(last.title, "References");
    let items = list_items(&last.html);
    assert_eq!(items.len(), 7, "{}", last.html);
    assert!(items[0].starts_with("1. Nelson PT") && items[0].ends_with("2010;257:359–66."), "{}", items[0]);
    assert!(items[6].starts_with("7. Kurlan R"));
    assert!(!book.chapters.iter().take(book.chapters.len() - 1).any(|c| c.html.contains("Nelson PT")));
}

/// A small bold "References" (smaller than the body, so no heading for the layout analysis), and an
/// ornamented "❚ REFERENCES".
#[test]
fn small_and_ornamented_reference_headings() {
    for h in [bold(line("References", 0, 72.0, 0.0, 7.5)), bold(line("❚ REFERENCES", 0, 72.0, 0.0, 12.0))] {
        let book = book_of(refs_pages(Some(h), &NUMBERED_REFS, false));
        let last = book.chapters.last().unwrap();
        assert!(last.title == "References" || last.title == "REFERENCES", "{}", last.title);
        assert_eq!(list_items(&last.html).len(), 7, "{}", last.html);
    }
}

/// No heading at all (Wiley): the entries themselves make a reference list; entries that share a
/// paragraph are cut at the next number.
#[test]
fn reference_list_without_a_heading() {
    let refs: Vec<String> = NUMBERED_REFS.iter().map(|e| {
        let (n, rest) = e.split_once(". ").unwrap();
        format!("[{n}] {rest}")
    }).collect();
    let refs: Vec<&str> = refs.iter().map(|s| s.as_str()).collect();
    let book = book_of(refs_pages(None, &refs, false));
    let last = book.chapters.last().unwrap();
    assert_eq!(last.title, "References");
    let items = list_items(&last.html);
    assert_eq!(items.len(), 7, "{}", last.html);
    assert!(items[2].starts_with("[3] McKeith IG"));
    let discussion = book.chapters.iter().find(|c| c.title == "3 Discussion").unwrap();
    assert!(!discussion.html.contains("Nelson PT") && discussion.html.contains("<p>"));
}

/// Author–year entries with a hanging indent, two of them run together in one paragraph.
#[test]
fn author_year_entries_are_cut_at_the_hanging_indent() {
    let refs = [
        "Abebe DA, van Bentum S, Suzuki M (2021) Plant death caused by inefficient induction of antiviral R-gene resistance. Plant Physiol 186:1–15.",
        "Akhter MS, Nakahara KS, Masuta C (2021) Resistance induction based on the understanding of molecular interactions. Plant Sci 305:110830.",
        "Alcaide C, Rabadán MP, Gómez P (2020) Implications of mixed viral infections on plant disease ecology. Viruses 12:1–17.",
    ];
    let mut pages = refs_pages(Some(bold(line("References", 0, 72.0, 0.0, 7.5))), &refs, false);
    // the second and third entries in one paragraph (no gap between them)
    let p3 = &mut pages[2].lines;
    let k = p3.iter().position(|l| l.text.starts_with("Alcaide")).unwrap();
    for l in p3[k..].iter_mut() {
        l.bbox.y0 -= 1.0;
        l.bbox.y1 -= 1.0;
    }
    let book = book_of(pages);
    let items = list_items(&book.chapters.last().unwrap().html);
    assert_eq!(items.len(), 3, "{}", book.chapters.last().unwrap().html);
    assert!(items[2].starts_with("Alcaide C"));
}

/// Conservative: a numbered list of protocol steps in the methods is running text, and without
/// a reference list nothing is cut out of the body.
#[test]
fn numbered_steps_in_the_body_are_no_references() {
    let book = book_of(refs_pages(None, &[], true));
    assert!(book.chapters.iter().all(|c| !c.html.contains("class=\"refs\"")));
    assert!(book.chapters.iter().all(|c| c.title != "References"));
    let methods = book.chapters.iter().find(|c| c.title == "2 Methods").unwrap();
    assert!(methods.html.contains("1. The samples were thawed"));
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
    pdf_with_fonts(contents, title, &["Helvetica-Bold", "Times-Roman"])
}

/// A PDF of the given page content streams (612 × 792) with fonts F1, F2, … of these base font
/// names (not embedded; one that isn't a base-14 font gets widths of half an em and a font
/// descriptor, so that PDFKit lays it out with a substitute) and `title` in the document info.
fn pdf_with_fonts(contents: &[String], title: &str, fonts: &[&str]) -> Vec<u8> {
    let base14 = |f: &str| f.starts_with("Helvetica") || f.starts_with("Times-") || f.starts_with("Courier");
    let n = contents.len();
    let custom: Vec<usize> = (0..fonts.len()).filter(|&i| !base14(fonts[i])).collect();
    let k = fonts.len() + custom.len(); // fonts, then the descriptors of the custom ones
    let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", 3 + k + 2 * i)).collect();
    let mut objs: Vec<String> = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        format!("<< /Type /Pages /Kids [{}] /Count {n} >>", kids.join(" ")),
    ];
    let widths = vec!["500"; 224].join(" ");
    for (i, f) in fonts.iter().enumerate() {
        match custom.iter().position(|&c| c == i) {
            None => objs.push(format!("<< /Type /Font /Subtype /Type1 /BaseFont /{f} /Encoding /WinAnsiEncoding >>")),
            Some(j) => objs.push(format!(
                "<< /Type /Font /Subtype /Type1 /BaseFont /{f} /Encoding /WinAnsiEncoding /FirstChar 32 /LastChar 255 \
                 /Widths [{widths}] /FontDescriptor {} 0 R >>",
                3 + fonts.len() + j
            )),
        }
    }
    for &i in &custom {
        let flags = if fonts[i].contains("Bold") { 32 | 262144 } else { 32 };
        objs.push(format!(
            "<< /Type /FontDescriptor /FontName /{} /Flags {flags} /FontBBox [-200 -250 1000 900] /ItalicAngle 0 \
             /Ascent 750 /Descent -250 /CapHeight 660 /StemV 80 >>",
            fonts[i]
        ));
    }
    let res: Vec<String> = (0..k).map(|i| format!("/F{} {} 0 R", i + 1, 3 + i)).collect();
    for (i, c) in contents.iter().enumerate() {
        objs.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << {} >> >> /Contents {} 0 R >>",
            res.join(" "),
            4 + k + 2 * i
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

const WILEY_ABSTRACT: &str = "We present a tiny test paper typeset like a Wiley article, with its abstract on a light grey \
    background, its body text in two columns, a footnote under the first column, a dashed rule, a copyright line \
    and a footer with the journal, the DOI and the page number on every page.";
const WILEY_BODY: &str = "Hairpin probes open only when they meet a perfectly matching target, and this simple fact \
    has been used for many years to tell apart sequences that differ by a single base. Here the text runs on in \
    the first column, past the footnote at its foot, and goes on at the top of the second column, where it ends \
    after a few more words about circles that are amplified many times over.";
const WILEY_BODY2: &str = "A second paragraph fills the rest of the second column, as it would in a real article, \
    and talks about nothing in particular for a while so that the column is about as long as the first one and \
    the page looks like a page of a journal rather than a sketch of one.";
const WILEY_LEGEND: &str = "A grey box drawn as a vector rectangle, standing in for a microscopy image.";
const WILEY_RESULTS: &str = "The probes found every target that was there and none that was not, in cells and in \
    tissue sections, which is what one hopes for from any method of this kind and rarely sees in practice.";

/// A three-page article typeset like Wiley's journals: body text in STIXTwoText (not a math
/// font), a masthead logo, a Crossmark badge, an abstract on a grey background, numbered headings
/// with the number set apart ("1   Introduction"), a footnote and a dashed rule under the first
/// column, a copyright line, the first page's journal/DOI footer and running feet with "2of3" page
/// numbers, and a figure whose label "FIGURE 1" stands apart from its legend.
pub(crate) fn wiley_pdf() -> Vec<u8> {
    fn t(font: &str, size: f64, x: f64, y: f64, s: &str) -> String {
        format!("BT /{font} {size} Tf {x} {y} Td ({s}) Tj ET\n")
    }
    let lines = |font: &str, size: f64, x: f64, y: f64, text: &str, n: usize| -> (String, f64) {
        let mut out = String::new();
        let mut y = y;
        for l in wrap(text, n) {
            out += &t(font, size, x, y, &l);
            y -= size * 1.3;
        }
        (out, y)
    };
    let mut p1 = t("F2", 7.0, 10.0, 780.0, "Check for updates");
    p1 += "0.3 g 416 720 142 30 re f 0 g\n";
    p1 += &t("F1", 10.0, 54.0, 741.0, "Journal of Tests");
    p1 += &t("F1", 18.0, 54.0, 620.0, "Hairpins and Circles in a Tiny Test Paper");
    p1 += &t("F2", 9.0, 54.0, 595.0, "Ada Lovelace | Alan Turing");
    p1 += "0.9 g 54 455 504 110 re f 0 g\n";
    p1 += &t("F1", 10.0, 64.0, 548.0, "ABSTRACT");
    p1 += &lines("F2", 9.0, 64.0, 535.0, WILEY_ABSTRACT, 100).0;
    p1 += &t("F1", 10.0, 54.0, 420.0, "1");
    p1 += &t("F1", 10.0, 79.0, 420.0, "Introduction");
    let body = wrap(WILEY_BODY, 48);
    let split = body.len() - 2;
    for (i, l) in body.iter().enumerate() {
        let (x, y) = if i < split { (54.0, 400.0 - 11.5 * i as f64) } else { (314.0, 420.0 - 11.5 * (i - split) as f64) };
        p1 += &t("F2", 9.0, x, y, l);
    }
    p1 += &lines("F2", 9.0, 314.0, 420.0 - 11.5 * (body.len() - split) as f64 - 14.0, WILEY_BODY2, 48).0;
    p1 += "0 G 0.5 w 54 110 m 558 110 l S\n";
    p1 += &t("F2", 6.0, 54.0, 100.0, "Ada Lovelace and Alan Turing contributed equally to this work.");
    p1 += "[2 2] 0 d 54 80 m 558 80 l S [] 0 d\n";
    p1 += &t("F2", 6.0, 54.0, 65.0, "\\251 2026 Test-VCH GmbH");
    p1 += &t("F2", 7.0, 54.0, 30.0, "Journal of Tests, 2026; 32:e70999");
    p1 += &t("F2", 7.0, 54.0, 21.0, "https://doi.org/10.1234/test.70999");
    p1 += &t("F2", 7.0, 530.0, 30.0, "1 of 3");
    let mut p2 = "0.5 g 60 450 492 300 re f 0 g\n".to_string();
    p2 += &t("F1", 8.0, 54.0, 435.0, "FIGURE 1");
    p2 += &t("F2", 8.0, 100.0, 435.0, WILEY_LEGEND);
    p2 += &t("F1", 10.0, 54.0, 400.0, "2");
    p2 += &t("F1", 10.0, 79.0, 400.0, "Results");
    p2 += &lines("F2", 9.0, 54.0, 380.0, WILEY_RESULTS, 48).0;
    p2 += &t("F2", 7.0, 54.0, 32.0, "2of3 Journal of Tests,2026");
    let mut p3 = t("F1", 8.0, 54.0, 740.0, "References");
    p3 += &t("F2", 8.0, 54.0, 726.0, "1. Someone. A paper about hairpins. Journal of Tests 2020.");
    p3 += &t("F2", 7.0, 54.0, 32.0, "Journal of Tests,2026 3of3");
    pdf_with_fonts(&[p1, p2, p3], "Hairpins and Circles in a Tiny Test Paper", &["STIXTwoText-Bold", "STIXTwoText"])
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

/// A Wiley-style article reads as text: nothing but the figure is an image, the paragraph runs on
/// past the footnote into the next column, the furniture is gone, the figure has its whole legend
/// and the headings keep their numbers. `P2E_TEST_PDF_DIR` keeps the PDF there (to try it with the
/// Swift converter).
#[test]
#[ignore]
fn pdfium_wiley_article_is_text() {
    let dir = tmp("wiley");
    let pdf = dir.join("wiley.pdf");
    std::fs::write(&pdf, wiley_pdf()).unwrap();
    if let Some(keep) = std::env::var_os("P2E_TEST_PDF_DIR") {
        std::fs::copy(&pdf, PathBuf::from(keep).join("wiley.pdf")).unwrap();
    }
    let opts = Options { offline: true, ..Default::default() };
    let r = super::super::convert_file_with(&pdf, &dir, &opts).unwrap();
    assert_eq!(r.images, 1);
    let norm = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    let text = norm(&r.fulltext);
    for want in [WILEY_ABSTRACT, WILEY_BODY, WILEY_BODY2, WILEY_RESULTS] {
        assert!(text.contains(&norm(want)), "{want}\n--- in ---\n{text}");
    }
    for gone in ["Check for updates", "Test-VCH", "Journal of Tests,", "of 3", "of3"] {
        assert!(!text.contains(gone), "{gone} in {text}");
    }
    let mut z = zip::ZipArchive::new(std::fs::File::open(&r.file).unwrap()).unwrap();
    let nav = read(&mut z, "EPUB/nav.xhtml");
    assert!(nav.contains("1 Introduction") && nav.contains("2 Results"), "{nav}");
    let all: String = (1..=4).filter_map(|i| z.by_name(&format!("EPUB/text/ch{i:02}.xhtml")).ok().map(|mut f| {
        let mut s = String::new();
        std::io::Read::read_to_string(&mut f, &mut s).unwrap();
        s
    })).collect();
    assert!(all.contains(&format!("<figcaption><strong>FIGURE 1</strong> {WILEY_LEGEND}</figcaption>")), "{all}");
    assert!(!all.contains("class=\"equation\""), "{all}");
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

/// A page of article text with a figure included as a whole manuscript page (MDPI): a form
/// XObject whose box shows only the figure, while the manuscript's own lines lie under the
/// article's, a point lower; and in the box, text the figure's image covers.
fn figure_form_pdf() -> Vec<u8> {
    let mut body = String::new();
    let mut hidden = String::new();
    for i in 0..8 {
        let y = 700 - i * 13;
        body += &format!("BT /F1 10 Tf 72 {y} Td (Line {i} of the article text, set in its own way.) Tj ET\n");
        hidden += &format!("BT /F1 10 Tf 75 {} Td (Manuscript line {i} with other words here.) Tj ET\n", y - 1);
    }
    hidden += "BT /F1 12 Tf 75 760 Td (Cells 2023, 12, x FOR PEER REVIEW) Tj ET\n";
    hidden += "BT /F1 10 Tf 310 470 Td (Under the picture) Tj ET\n";
    hidden += "q 150 0 0 60 300 430 cm /Im1 Do Q\n";
    hidden += "BT /F1 10 Tf 250 410 Td (Figure label A) Tj ET\n";
    let content = body + "q 1 0 0 1 0 0 cm /Fm1 Do Q\n";
    let objs: Vec<String> = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> /XObject << /Fm1 6 0 R >> >> /Contents 5 0 R >>".into(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Times-Roman /Encoding /WinAnsiEncoding >>".into(),
        format!("<< /Length {} >>\nstream\n{content}endstream", content.len()),
        format!(
            "<< /Type /XObject /Subtype /Form /BBox [200 400 400 500] /Resources << /Font << /F1 4 0 R >> /XObject << /Im1 7 0 R >> >> /Length {} >>\nstream\n{hidden}endstream",
            hidden.len()
        ),
        "<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8 /Length 1 >>\nstream\nAendstream".into(),
        "<< /Title (Figure form) >>".into(),
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
    let k = objs.len();
    out.extend(format!("trailer\n<< /Size {} /Root 1 0 R /Info {k} 0 R >>\nstartxref\n{xref}\n%%EOF\n", k + 1).bytes());
    out
}

/// Text a form draws outside its box, or under an image it draws later, is not shown and is left
/// out; the figure's own label stays, and the figure's image counts only where it shows.
#[test]
#[ignore]
fn pdfium_text_outside_a_forms_box_is_left_out() {
    let data = figure_form_pdf();
    if let Some(d) = std::env::var_os("P2E_TEST_PDF_DIR") {
        std::fs::write(PathBuf::from(d).join("figure-form.pdf"), &data).unwrap();
    }
    let (lines, _) = super::text_lines(&data).unwrap();
    let text: Vec<String> = lines.iter().map(|l| l.text.clone()).collect();
    for i in 0..8 {
        let want = format!("Line {i} of the article text, set in its own way.");
        assert!(text.contains(&want), "{want} not in {text:?}");
    }
    let all = text.join("\n");
    assert!(!all.contains("Manuscript") && !all.contains("PEER REVIEW"), "{all}");
    assert!(!all.contains("Under the picture"), "{all}");
    assert!(all.contains("Figure label A"), "{all}");
    let p = super::pdfium().unwrap();
    let doc = p.load_pdf_from_byte_slice(&data, None).unwrap();
    let page = doc.pages().get(0).unwrap();
    let mb = super::extract::media_box(&page);
    let (g, _) = super::extract::graphics(&page, &mb);
    assert_eq!(g.images.len(), 1, "{:?}", g.images);
    let r = g.images[0];
    // the image is 300–450 × 430–490 (PDF), cut by the box at 400; 302–362 from the top
    assert!((r.x0 - 300.0).abs() < 1.0 && (r.x1 - 400.0).abs() < 1.0 && (r.y0 - 302.0).abs() < 1.0, "{r:?}");
}

/// Elsevier's accent font has its dieresis at the code of "€", drawn after the line over the letter
/// it goes on ("J€akel"): it goes on that letter, and the word spaces stay.
#[test]
#[ignore]
fn pdfium_accent_drawn_after_the_line() {
    // Times-Roman 10 pt: "Sarah J" is 29.16 pt wide, so the "a" of "akel" spans 101.16–105.6
    let content = "BT /F2 10 Tf 72 700 Td (Sarah Jakel and Ruth Smith wrote it.) Tj ET\nBT /F2 10 Tf 100.9 700 Td (\\200) Tj ET\n".to_string();
    let data = pdf_of(&[content], "Accents");
    let (lines, _) = super::text_lines(&data).unwrap();
    let text: Vec<String> = lines.iter().map(|l| l.text.clone()).collect();
    assert!(text.iter().any(|t| t == "Sarah Jäkel and Ruth Smith wrote it."), "{text:?}");
}
