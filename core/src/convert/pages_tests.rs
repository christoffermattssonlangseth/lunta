//! Tests of the article-page readers (`webpage`, `atypon`, `springer`, `silverchair`,
//! `cellpress`, `webarchive`) on small synthetic pages in testdata/ (built from each platform's
//! markup; `{{FILLER}}` stands for a paragraph's worth of words).

use super::webpage::{self, PageImage, PageImages, PageReader};
use super::{page_book, Book, Options};

fn filler(n: usize) -> String {
    (0..n).map(|i| format!("word{}", i % 97)).collect::<Vec<_>>().join(" ")
}

fn fill(page: &str) -> String {
    fill_n(page, 600)
}

fn fill_n(page: &str, n: usize) -> String {
    page.replace("{{FILLER}}", &filler(n)).replace("{{ABSTRACT}}", &filler(60))
}

fn png() -> Vec<u8> {
    let mut out = Vec::new();
    image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(4, 4, image::Rgb([0, 90, 200])))
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .unwrap();
    out
}

fn offline() -> Options {
    Options { offline: true, ..Default::default() }
}

fn all_html(b: &Book) -> String {
    b.chapters.iter().map(|c| c.html.as_str()).collect()
}

fn titles(b: &Book) -> Vec<&str> {
    b.chapters.iter().map(|c| c.title.as_str()).collect()
}

const CELL: &str = include_str!("testdata/page-atypon-cell.html");
const GENERIC: &str = include_str!("testdata/page-generic.html");
const SPRINGER: &str = include_str!("testdata/page-springer.html");
const SPRINGER_TABLE: &str = include_str!("testdata/page-springer-table.html");
const SILVERCHAIR: &str = include_str!("testdata/page-silverchair.html");

#[test]
fn readers_are_picked_by_platform() {
    let u = |s: &str| url::Url::parse(s).ok();
    let pick = |url: &str, html: &str| webpage::pick(u(url).as_ref(), html);
    assert_eq!(pick("https://www.sciencedirect.com/science/article/pii/S1", ""), PageReader::ScienceDirect);
    assert_eq!(pick("https://www.nejm.org/doi/full/10.1056/NEJMoa1", ""), PageReader::Nejm);
    assert_eq!(pick("https://www.cell.com/cell/fulltext/S0092", ""), PageReader::CellPress);
    assert_eq!(pick("https://www.example.org/x", CELL), PageReader::Atypon);
    assert_eq!(pick("https://www.example.org/x", SPRINGER), PageReader::SpringerNature);
    assert_eq!(pick("https://www.example.org/x", SILVERCHAIR), PageReader::Silverchair);
    assert_eq!(pick("https://journals.sagepub.com/doi/10.1/x", "<html></html>"), PageReader::Atypon);
    assert_eq!(pick("https://jamanetwork.com/journals/x", "<html></html>"), PageReader::Silverchair);
    assert_eq!(pick("https://link.springer.com/article/x", "<html></html>"), PageReader::SpringerNature);
    assert_eq!(pick("https://journals.plos.org/x", "<html></html>"), PageReader::Generic);
    assert_eq!(super::page_reader("", include_str!("testdata/sciencedirect-sample.html")), "ScienceDirect");
}

#[test]
fn posted_images_are_found_by_address_or_file_name() {
    let base = url::Url::parse("https://www.cell.com/cell/fulltext/S1").unwrap();
    let bag = PageImages::new(
        vec![
            PageImage { urls: vec!["https://www.cell.com/cms/attachment/abc/gr1.jpg".into()], data: vec![1; 10] },
            PageImage { urls: vec!["https://cdn.example/x/gr2_lrg.jpg".into()], data: vec![2; 30] },
            PageImage { urls: vec!["https://cdn.example/x/gr2.jpg".into()], data: vec![3; 20] },
            PageImage { urls: vec!["https://cdn.example/tables/1".into()], data: b"<!DOCTYPE html><html><table>".to_vec() },
        ],
        Some(&base),
    );
    // relative to the page, http or https, with a fragment
    assert_eq!(bag.data(&["/cms/attachment/abc/gr1.jpg"]).unwrap().0, 0);
    assert_eq!(bag.data(&["http://www.cell.com/cms/attachment/abc/gr1.jpg#x"]).unwrap().0, 0);
    // by file name: thumbnail and full size are one figure, the largest wins
    assert_eq!(bag.data(&["https://elsewhere.example/gr2_thumb.jpg"]).unwrap().0, 1);
    assert!(bag.data(&["https://elsewhere.example/image.jpg"]).is_none());
    // pages only by their address
    assert!(bag.page(&["https://cdn.example/tables/1"]).unwrap().contains("<table>"));
    assert!(bag.data(&["https://other.example/1"]).is_none());
}

#[test]
fn atypon_cell_page_becomes_book() {
    let html = fill(CELL);
    let images = vec![PageImage { urls: vec!["https://www.cell.com/cms/attachment/abc/gr1.jpg".into()], data: png() }];
    let b = page_book(&html, "https://www.cell.com/cell/fulltext/S0092-8674(26)00001-1", images, &offline()).unwrap();
    assert_eq!(b.source_label, "Full text from cell.com");
    assert_eq!(b.meta.title, "A Cell Test Article");
    assert_eq!(b.meta.authors, vec!["Ada Lovelace", "Alan Turing"]);
    assert_eq!(b.meta.doi, "10.1016/j.cell.2026.01.001");
    assert_eq!(b.meta.date, "2026-01-21");
    assert!(b.meta.abstract_text.starts_with("We study how cells"), "{}", b.meta.abstract_text);
    assert_eq!(titles(&b), vec!["Summary", "Introduction", "Results", "Acknowledgments", "References"]);
    let intro = &b.chapters[1].html;
    // the citation's hidden pop-up is left out; the citation links to the reference list
    assert!(!all_html(&b).contains("Hidden pop-up"), "{intro}");
    assert!(intro.contains("Cells matter<a href=\"ch05.xhtml#bib1\">1</a>."), "{intro}");
    // the side pane's copy of the list: pointed at ours
    assert!(b.chapters[2].html.contains("<a href=\"ch05.xhtml#bib2\">2</a>"), "{}", b.chapters[2].html);
    assert!(b.chapters[2].html.contains("<h2 id=\"sec2.1\">A subsection</h2>"), "{}", b.chapters[2].html);
    assert!(b.chapters[2].html.contains("<math xmlns=\"http://www.w3.org/1998/Math/MathML\"><mi>x</mi><mo>=</mo><mn>2</mn></math>"));
    // the figure with the image the browser sent
    assert_eq!(b.images.len(), 1);
    assert!(
        intro.contains("<figure id=\"fig1\"><img src=\"../images/img1.png\" alt=\"Figure 1\"/><figcaption><strong>Figure 1.</strong> <strong>Overview of the study</strong></figcaption></figure>"),
        "{intro}"
    );
    let refs = &b.chapters[4].html;
    assert!(
        refs.contains("<li id=\"bib1\">Lovelace, A. Notes. <i>Journal</i> 1843. <a href=\"https://doi.org/10.1000/notes\">doi:10.1000/notes</a></li>"),
        "{refs}"
    );
    assert!(refs.contains("<li id=\"bib2\">Turing, A. Computing machinery. 1950.</li>"), "{refs}");
    assert_eq!(b.notes, vec!["1 equation(s) as MathML."]);
}

#[test]
fn atypon_page_without_access_is_refused() {
    let start = CELL.find("<section id=\"sec1\">").unwrap();
    let end = CELL.find("</div></section>\n<section id=\"backmatter\">").unwrap();
    let html = format!("{}{}", &CELL[..start], &CELL[end..]);
    let e = page_book(&html, "https://www.science.org/doi/10.1126/x", vec![], &offline()).unwrap_err();
    assert_eq!(e.to_string(), webpage::ABSTRACT_ONLY);
}

#[test]
fn generic_page_becomes_book() {
    let html = fill(GENERIC);
    let images = vec![PageImage { urls: vec!["https://journal.example/articles/1/images/f1.png".into()], data: png() }];
    let b = page_book(&html, "https://journal.example/articles/1", images, &offline()).unwrap();
    assert_eq!(b.source_label, "Full text from journal.example");
    assert_eq!(b.meta.authors, vec!["Grace Hopper"]);
    assert_eq!(b.meta.date, "2025-03-04");
    assert_eq!(b.meta.doi, "10.5555/generic.1");
    assert_eq!(titles(&b), vec!["Abstract", "Introduction", "Methods", "References"]);
    let h = all_html(&b);
    assert!(!h.contains("Tweet") && !h.contains("Journal home") && !h.contains("Copyright footer"), "{h}");
    assert!(b.chapters[1].html.contains("Intro text <a href=\"#ref1\">[1]</a>."), "{}", b.chapters[1].html);
    assert!(
        b.chapters[1].html.contains("<figure id=\"f1\"><img src=\"../images/img1.png\" alt=\"Figure 1\"/><figcaption>Figure 1. A plot.</figcaption></figure>"),
        "{}",
        b.chapters[1].html
    );
    assert!(b.chapters[2].html.contains("<h2 id=\"s2a\"><strong>Sampling</strong></h2>"), "{}", b.chapters[2].html);
    assert!(
        b.chapters[2].html.contains("<div class=\"table-wrap\"><p class=\"table-caption\"><strong>Table 1. Counts</strong></p><table><tbody><tr><td colspan=\"2\">a</td></tr></tbody></table></div>"),
        "{}",
        b.chapters[2].html
    );
    assert!(b.chapters[3].html.contains("<ol class=\"refs\"><li id=\"ref1\">Hopper G. The education of a computer. 1952.</li></ol>"));
    assert_eq!(b.chapters[2].headings.iter().map(|h| h.title.as_str()).collect::<Vec<_>>(), vec!["Methods", "Sampling"]);
}

#[test]
fn highwire_references_lose_their_service_links() {
    // bioRxiv/HighWire: "↵" back to the text and the OpenUrl / Abstract / CrossRef / PubMed buttons
    // sit loose in each reference
    let html = fill(GENERIC).replace(
        "<li id=\"ref1\">Hopper G. The education of a computer. 1952.</li>",
        "<li id=\"ref1\"><a href=\"#xref-ref-1-1\">↵</a>Hopper G. The education of a computer. 1952. <a href=\"https://www.biorxiv.org/x?openurl\">OpenUrl</a><a href=\"https://www.biorxiv.org/lookup/ijlink/a\">Abstract/FREE Full Text</a><a href=\"https://www.biorxiv.org/lookup/external-ref?access_num=10.1/x&amp;link_type=DOI\">CrossRef</a><a href=\"https://www.biorxiv.org/lookup/external-ref?access_num=1&amp;link_type=MED\">PubMed</a></li>",
    );
    let b = page_book(&html, "https://journal.example/articles/1", vec![], &offline()).unwrap();
    let refs = &b.chapters.last().unwrap().html;
    assert!(refs.contains("Hopper G. The education of a computer. 1952."), "{refs}");
    assert!(refs.contains("<a href=\"https://doi.org/10.1/x\">doi:10.1/x</a>"), "{refs}");
    for junk in ["↵", "OpenUrl", "FREE Full Text", "CrossRef", "PubMed"] {
        assert!(!refs.contains(junk), "{junk} in {refs}");
    }
}

#[test]
fn generic_page_with_only_the_abstract_is_refused() {
    let start = GENERIC.find("<section id=\"s1\">").unwrap();
    let end = GENERIC.find("</article>").unwrap();
    let html = fill(&format!("{}{}", &GENERIC[..start], &GENERIC[end..]));
    let e = page_book(&html, "https://journal.example/articles/1", vec![], &offline()).unwrap_err();
    assert_eq!(e.to_string(), webpage::ABSTRACT_ONLY);
    // not an article at all
    let e = page_book("<html><body><p>Hello</p></body></html>", "", vec![], &offline()).unwrap_err();
    assert!(e.to_string().starts_with("This page doesn't look like the article's full text"), "{e}");
}

#[test]
fn springer_page_becomes_book_with_its_table_page() {
    let html = fill_n(SPRINGER, 1100);
    let images = vec![
        PageImage {
            urls: vec!["https://media.springernature.com/lw685/springer-static/image/x/MediaObjects/1_Fig1_HTML.png".into()],
            data: png(),
        },
        PageImage {
            urls: vec!["https://www.nature.com/articles/s00000-026-00001-x/tables/1".into()],
            data: SPRINGER_TABLE.as_bytes().to_vec(),
        },
    ];
    let b = page_book(&html, "https://www.nature.com/articles/s00000-026-00001-x", images, &offline()).unwrap();
    assert_eq!(b.source_label, "Full text from nature.com");
    assert_eq!(titles(&b), vec!["Abstract", "Introduction", "References"]);
    let intro = &b.chapters[1].html;
    assert!(intro.contains("<code>E = mc^2</code>"), "{intro}");
    assert!(intro.contains("<sup><a href=\"#ref-CR1\">1</a></sup>"), "{intro}");
    assert!(
        intro.contains("<figure id=\"Fig1\"><img src=\"../images/img1.png\" alt=\"figure 1\"/><figcaption><b>Fig. 1: Overview.</b><span class=\"cap-p\">a, Panel a. b, Panel b.</span></figcaption></figure>"),
        "{intro}"
    );
    assert!(
        intro.contains("<div class=\"table-wrap\"><p class=\"table-caption\"><strong>Table 1 Counts</strong></p><table><thead><tr><th>Group</th><th>n</th></tr></thead><tbody><tr><td>A</td><td>12</td></tr></tbody></table><p class=\"table-foot\"><small>n, number of samples.</small></p></div>"),
        "{intro}"
    );
    let refs = &b.chapters[2].html;
    assert!(
        refs.contains("<li><span id=\"ref-CR1\"></span>Einstein, A. On the electrodynamics. 1905.  <a href=\"https://doi.org/10.1002/andp.19053221004\">doi:10.1002/andp.19053221004</a></li>"),
        "{refs}"
    );
    assert!(!all_html(&b).contains("Reprints"));
    // without the table's page (and offline): its caption and a link stand in
    let b = page_book(&html, "https://www.nature.com/articles/s00000-026-00001-x", vec![], &offline()).unwrap();
    assert!(
        b.chapters[1].html.contains("<strong>Table 1 Counts</strong> <a href=\"https://www.nature.com/articles/s00000-026-00001-x/tables/1\">(table on the publisher&#39;s site)</a>")
            || b.chapters[1].html.contains("<strong>Table 1 Counts</strong> <a href=\"https://www.nature.com/articles/s00000-026-00001-x/tables/1\">(table on the publisher's site)</a>"),
        "{}",
        b.chapters[1].html
    );
    assert_eq!(b.notes, vec!["1 figure image(s) were not sent by the browser; captions are kept."]);
}

#[test]
fn silverchair_page_becomes_book() {
    let html = fill(SILVERCHAIR);
    // the browser sent the original behind "View large"
    let images =
        vec![PageImage { urls: vec!["https://academic.oup.com/view-large/figure/1/gkae0001fig1.jpg".into()], data: png() }];
    let b = page_book(&html, "https://academic.oup.com/nar/article/1/1/1/1", images, &offline()).unwrap();
    assert_eq!(b.source_label, "Full text from academic.oup.com");
    assert_eq!(titles(&b), vec!["Abstract", "Introduction", "Results", "References"]);
    let intro = &b.chapters[1].html;
    assert!(intro.contains("(<a href=\"#B1\">1</a>), and in <a href=\"#F1\">Figure 1</a>."), "{intro}");
    assert!(
        intro.contains("<figure id=\"F1\"><img src=\"../images/img1.png\" alt=\"Figure 1\"/><figcaption><b>Figure 1. </b><span class=\"cap-p\">An overview.</span></figcaption></figure>"),
        "{intro}"
    );
    assert!(!all_html(&b).contains("Open in new tab") && !all_html(&b).contains("Metadata panel"));
    let refs = &b.chapters[3].html;
    assert!(
        refs.contains("<li id=\"B1\"><span id=\"ref-auto-B1\"></span>Watson J., Crick F. Molecular structure of nucleic acids. Nature. 1953.</li>")
            || refs.contains("<li id=\"B1\">Watson J., Crick F. Molecular structure of nucleic acids. Nature. 1953.</li>"),
        "{refs}"
    );
    assert!(!refs.contains("Google Scholar"));
}

#[test]
fn web_archive_converts_as_a_page() {
    let html = fill(CELL);
    let png = png();
    let data = super::webarchive::tests::archive(
        &html,
        "https://www.cell.com/cell/fulltext/S0092-8674(26)00001-1",
        &[("https://www.cell.com/cms/attachment/abc/gr1.jpg", "image/jpeg", &png)],
    );
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("page.webarchive");
    std::fs::write(&file, &data).unwrap();
    // as a file (the resolver recognises it) and directly
    let r = super::convert_file_with(&file, &dir.path().join("a"), &offline()).unwrap();
    assert_eq!(r.source, "Full text from cell.com");
    assert_eq!(r.images, 1);
    assert!(r.fulltext.contains("Cells matter"));
    let r2 = super::convert_webarchive_with(&data, &dir.path().join("b"), &offline()).unwrap();
    assert_eq!((r2.words, r2.images), (r.words, r.images));
}
