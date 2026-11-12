//! Offline tests: JATS → XHTML on a small fixture (checked against the Swift converter's output),
//! EPUB packaging validity, and a few helpers. Network tests are `#[ignore]`d.

use super::*;
use std::collections::BTreeSet;
use std::io::Read;

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src/convert/testdata/sample-jats.xml");
/// `<body>` of every chapter file the Swift Paper2EpubKit wrote for the fixture, one per line.
const SWIFT_CHAPTERS: &str = include_str!("testdata/sample-jats.swift-chapters.html");
const SWIFT_NAV: &str = include_str!("testdata/sample-jats.swift-nav.html");

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("p2e-test-{name}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn unzip(path: &Path) -> zip::ZipArchive<std::fs::File> {
    zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap()
}

fn read(z: &mut zip::ZipArchive<std::fs::File>, name: &str) -> String {
    let mut s = String::new();
    z.by_name(name).unwrap().read_to_string(&mut s).unwrap();
    s
}

fn body(xhtml: &str) -> &str {
    let a = xhtml.find("\n<body").unwrap();
    let a = a + xhtml[a..].find('>').unwrap() + 1;
    let b = xhtml.rfind("</body>").unwrap();
    &xhtml[a..b]
}

fn fixture_book() -> Book {
    let data = std::fs::read(FIXTURE).unwrap();
    let doc = xml::parse(&data).unwrap();
    let images = std::collections::HashMap::new();
    let mut conv = jats::JatsConverter::new(doc.root(), &images);
    let meta = conv.meta();
    let chapters = conv.chapters();
    Book { meta, chapters, images: vec![], source_label: "JATS file".into(), notes: vec![] }
}

#[test]
fn jats_metadata() {
    let m = fixture_book().meta;
    assert_eq!(m.title, "A small sample article & its figures");
    assert_eq!(m.authors, vec!["Ada Lovelace", "Alan Turing", "The Fixture Consortium"]);
    assert_eq!(m.doi, "10.1234/fixture.2024.001");
    assert_eq!(m.journal, "Journal of Test Fixtures");
    assert_eq!(m.date, "2024-02-03");
    assert_eq!(m.license, "https://creativecommons.org/licenses/by/4.0/");
    assert_eq!(m.keywords, vec!["testing", "EPUB"]);
    assert!(m.abstract_text.starts_with("We test the converter with “quotes”"));
    assert_eq!(m.affiliations_html, "Department of Engines, Analytical University, London");
}

#[test]
fn jats_chapters_and_headings() {
    let b = fixture_book();
    let titles: Vec<&str> = b.chapters.iter().map(|c| c.title.as_str()).collect();
    assert_eq!(titles, vec!["Abstract", "Author summary", "Introduction", "Results", "Acknowledgements", "References"]);
    let intro = &b.chapters[2];
    assert_eq!(
        intro.headings,
        vec![
            Heading { level: 1, id: "s1".into(), title: "Introduction".into() },
            Heading { level: 2, id: "s1-1".into(), title: "Background".into() },
        ]
    );
    // MathML is re-serialised without prefixes and ids; TeX-only formulas are kept as code
    assert!(intro.html.contains(r#"<math xmlns="http://www.w3.org/1998/Math/MathML" display="block"><mrow><mi>E</mi>"#));
    assert!(intro.html.contains(r#"<code class="tex">\alpha + \beta</code>"#));
    let results = &b.chapters[3].html;
    assert!(results.contains(r#"<figure id="F1"><span class="missing">[Figure image not available]</span><figcaption><strong>Figure 1</strong> <strong>An engine.</strong>"#));
    assert!(results.contains(r#"<td colspan="2">1 &lt; 2</td>"#));
    assert!(b.chapters[5].html.contains(r#"<em>Phil Trans</em> <strong>1</strong>:10–20."#));
}

/// The whole EPUB for the fixture: chapter XHTML and nav must equal what Swift's Paper2EpubKit
/// produced for the same file (see testdata/*.swift-*.html).
#[test]
fn jats_file_matches_swift_output() {
    let dir = tmp("fixture");
    let r = convert_file(Path::new(FIXTURE), &dir).unwrap();
    assert_eq!(r.source, "JATS file");
    assert_eq!(r.words, 98);
    assert_eq!(
        r.notes,
        vec![
            "1 figure image(s) could not be downloaded; captions are kept.",
            "1 formula(s) only exist as TeX and are shown as source code."
        ]
    );
    assert_eq!(r.file.file_name().unwrap().to_str().unwrap(), "a-small-sample-article-its-figures.epub");
    let mut z = unzip(&r.file);
    let expected: Vec<&str> = SWIFT_CHAPTERS.lines().collect();
    for (i, exp) in expected.iter().enumerate() {
        let x = read(&mut z, &format!("EPUB/text/ch{:02}.xhtml", i + 1));
        assert_eq!(body(&x), *exp, "chapter {}", i + 1);
    }
    assert!(z.by_name(&format!("EPUB/text/ch{:02}.xhtml", expected.len() + 1)).is_err());
    assert_eq!(body(&read(&mut z, "EPUB/nav.xhtml")), SWIFT_NAV.trim_end());
    assert!(r.fulltext.contains("Thanks to Charles."));
    std::fs::remove_dir_all(dir).ok();
}

/// Structural EPUB checks: mimetype first and stored, container.xml → OPF, manifest == files,
/// spine → manifest, every XHTML well-formed.
#[test]
fn epub_packaging_is_valid() {
    let mut book = fixture_book();
    // a referenced image, so the manifest has one
    book.chapters[3].html = book.chapters[3].html.replace(
        "<span class=\"missing\">[Figure image not available]</span>",
        "<img src=\"../images/fig1.png\" alt=\"Figure 1\"/>",
    );
    let png = {
        let mut v = Vec::new();
        image::DynamicImage::ImageRgb8(image::RgbImage::new(4, 4))
            .write_to(&mut std::io::Cursor::new(&mut v), image::ImageFormat::Png)
            .unwrap();
        v
    };
    book.images = vec![BookImage { file_name: "fig1.png".into(), data: png, media_type: "image/png".into() }];
    for cover in [None, Some(vec![0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3])] {
        let dir = tmp("pkg");
        let path = dir.join("book.epub");
        epub::write(&book, cover.as_deref(), &path).unwrap();
        let mut z = unzip(&path);

        // mimetype: first entry, stored, exact content, no extra field
        {
            let m = z.by_index(0).unwrap();
            assert_eq!(m.name().unwrap(), "mimetype");
            assert_eq!(m.compression(), zip::CompressionMethod::Stored);
        }
        let raw = std::fs::read(&path).unwrap();
        assert_eq!(&raw[30..38], b"mimetype");
        assert_eq!(&raw[38..58], b"application/epub+zip");
        assert_eq!(read(&mut z, "mimetype"), "application/epub+zip");

        let container = read(&mut z, "META-INF/container.xml");
        assert!(container.contains(r#"full-path="EPUB/content.opf""#));
        let opf = read(&mut z, "EPUB/content.opf");
        let opf_doc = xml::parse(opf.as_bytes()).expect("OPF is well-formed");
        let manifest = opf_doc.root().child("manifest").unwrap();
        let items: Vec<(String, String)> = manifest
            .children_named("item")
            .map(|i| (i.attr("id").unwrap().to_string(), i.attr("href").unwrap().to_string()))
            .collect();
        let hrefs: BTreeSet<String> = items.iter().map(|(_, h)| format!("EPUB/{h}")).collect();
        let files: BTreeSet<String> = (0..z.len())
            .map(|i| z.by_index(i).unwrap().name().unwrap().to_string())
            .filter(|n| n != "mimetype" && !n.starts_with("META-INF/") && n != "EPUB/content.opf")
            .collect();
        assert_eq!(hrefs, files, "manifest must list exactly the files in the container");
        let ids: BTreeSet<&str> = items.iter().map(|(i, _)| i.as_str()).collect();
        assert_eq!(ids.len(), items.len(), "manifest ids are unique");
        for r in opf_doc.root().child("spine").unwrap().children_named("itemref") {
            assert!(ids.contains(r.attr("idref").unwrap()));
        }
        assert!(opf.contains(r#"properties="nav""#));
        assert!(opf.contains(r#"<dc:identifier id="pub-id">doi:10.1234/fixture.2024.001</dc:identifier>"#));
        assert!(opf.contains(r#"href="text/ch03.xhtml" media-type="application/xhtml+xml" properties="mathml""#));
        assert!(opf.contains(r#"<item id="img0" href="images/fig1.png" media-type="image/png"/>"#));
        assert_eq!(cover.is_some(), opf.contains(r#"properties="cover-image""#));
        let modified =
            Regex::new(r#"<meta property="dcterms:modified">\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z</meta>"#).unwrap();
        assert!(modified.is_match(&opf));

        for n in files.iter().filter(|n| n.ends_with(".xhtml") || n.ends_with(".ncx")) {
            let x = read(&mut z, n);
            xml::parse(x.as_bytes()).unwrap_or_else(|e| panic!("{n} is not well-formed: {e}"));
        }
        // cross-file links are rewritten
        assert!(read(&mut z, "EPUB/text/ch03.xhtml").contains(r#"<a href="ch06.xhtml#B1">1</a>"#));
        std::fs::remove_dir_all(dir).ok();
    }
}

#[test]
fn fix_links_rewrites_and_unwraps() {
    let files = vec![
        epub::XFile {
            name: "a.xhtml".into(),
            title: "A".into(),
            html: r##"<p><a href="#x">x</a> <a href="#nope">gone</a> <a href="#y">y</a></p>"##.into(),
        },
        epub::XFile {
            name: "b.xhtml".into(),
            title: "B".into(),
            html: r#"<p id="x">X</p><p><span id="y">Y</span></p>"#.into(),
        },
    ];
    let out = epub::fix_links(files);
    assert_eq!(out[0].html, r#"<p><a href="b.xhtml#x">x</a> gone <a href="b.xhtml#y">y</a></p>"#);
}

#[test]
fn helpers() {
    assert_eq!(slug("A  Small: sample — article!", 70), "a-small-sample-article");
    assert_eq!(slug("10.1234/ABC.def", 60), "10-1234-abc-def");
    assert_eq!(capitalized("author summary"), "Author Summary");
    assert_eq!(epub::valid_date("2024-02-03T00:00"), Some("2024-02-03".into()));
    assert_eq!(epub::valid_date("2024-2"), None);
    assert_eq!(esc("a<b & \"c\"\u{1}"), "a&lt;b &amp; &quot;c&quot;");
    assert_eq!(net::sniff(b"%PDF-1.7"), "pdf");
    assert_eq!(net::sniff(b"<?xml version='1.0'?><!DOCTYPE article><article>"), "jats");
    assert!(epub::utc_now().ends_with('Z'));
}

#[test]
#[cfg(not(feature = "pdf"))]
fn pdf_input_is_reported_as_unsupported() {
    let dir = tmp("pdf");
    let pdf = dir.join("paper.pdf");
    std::fs::write(&pdf, b"%PDF-1.4\n%nothing here\n").unwrap();
    let opts = Options { offline: true, ..Default::default() };
    match convert_file_with(&pdf, &dir, &opts) {
        Err(ConvertError::PdfNotSupported(m)) => assert!(m.contains("PDF file")),
        other => panic!("expected PdfNotSupported, got {:?}", other.map(|r| r.file)),
    }
    std::fs::remove_dir_all(dir).ok();
}

/// With PDF support, a broken PDF fails with a message (it can't be opened, or PDFium is
/// missing) instead of `PdfNotSupported`.
#[test]
#[cfg(feature = "pdf")]
fn broken_pdf_is_an_error_not_unsupported() {
    let dir = tmp("pdf");
    let pdf = dir.join("paper.pdf");
    std::fs::write(&pdf, b"%PDF-1.4\n%nothing here\n").unwrap();
    let opts = Options { offline: true, ..Default::default() };
    match convert_file_with(&pdf, &dir, &opts) {
        Err(ConvertError::Message(m)) => assert!(m == "Could not open the PDF." || m.contains("PDFium"), "{m}"),
        other => panic!("expected an error message, got {:?}", other.map(|r| r.file)),
    }
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn arxiv_html_to_chapters() {
    let html = r##"<!DOCTYPE html><html><head><title>x</title></head><body><article class="ltx_document">
<h1 class="ltx_title ltx_title_document">A Paper</h1>
<div class="ltx_authors"><span>Someone</span></div>
<div class="ltx_abstract"><h6 class="ltx_title ltx_title_abstract">Abstract</h6><p class="ltx_p">We do <span class="ltx_font_italic">things</span> &amp; more.</p></div>
<section id="S1" class="ltx_section"><h2 class="ltx_title ltx_title_section"><span class="ltx_tag ltx_tag_section">1 </span>Intro</h2>
<div id="S1.p1" class="ltx_para"><p class="ltx_p">See <math id="m1" class="ltx_Math" alttext="x" display="inline"><semantics><mi>x</mi><annotation encoding="application/x-tex">x</annotation></semantics></math> & <a href="#S2">Sec 2</a><br>.</p></div>
<figure id="S1.F1" class="ltx_figure"><img src="x1.png" alt="Figure 1" width="100"><figcaption>Fig</figcaption></figure>
<section id="S1.SS1" class="ltx_subsection"><h3 class="ltx_title">1.1 Sub</h3></section>
</section></article></body></html>"##;
    let doc = arxiv::parse(html).unwrap();
    let mut conv = arxiv::ArxivConverter::new(&doc, url::Url::parse("https://arxiv.org/html/0000.00000/").unwrap());
    let urls: Vec<String> = conv.image_urls().iter().map(|u| u.to_string()).collect();
    assert_eq!(urls, vec!["https://arxiv.org/html/0000.00000/x1.png"]);
    let chs = conv.chapters(&PaperMeta::default());
    assert_eq!(chs.len(), 2);
    assert_eq!(
        chs[0].html,
        r#"<section><h1 id="abstract">Abstract</h1><p>We do <em>things</em> &amp; more.</p></section>"#
    );
    assert_eq!(chs[1].title, "1 Intro");
    let hs: Vec<(u32, &str)> = chs[1].headings.iter().map(|h| (h.level, h.id.as_str())).collect();
    assert_eq!(hs, vec![(1, "S1"), (2, "S1.SS1")]);
    assert!(chs[1].html.contains(r##"<math xmlns="http://www.w3.org/1998/Math/MathML" display="inline" alttext="x"><semantics><mi>x</mi></semantics></math> &amp; <a href="#S2">Sec 2</a><br/>"##));
    assert!(chs[1].html.contains(r#"<figure id="S1.F1"><span class="missing">[Figure image not available]</span>"#));
    assert_eq!(conv.missing_figures, 1);
}

// MARK: - network (cargo test -- --ignored)

#[test]
#[ignore]
fn network_pmc_by_pmcid() {
    let dir = tmp("pmc");
    let r = convert("PMC7029158", &dir).unwrap();
    assert_eq!(r.source, "PMC open-access XML");
    assert_eq!(r.images, 3);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
#[ignore]
fn network_arxiv_html() {
    let dir = tmp("arxiv");
    let r = convert("1706.03762", &dir).unwrap();
    assert_eq!(r.source, "arXiv HTML");
    assert_eq!(r.meta.title, "Attention Is All You Need");
    std::fs::remove_dir_all(dir).ok();
}

#[test]
#[ignore]
fn network_elife_patterns() {
    // eLife's own XML + IIIF image patterns (normally only used when Europe PMC has no copy)
    let o = Options::default();
    let Source::Jats { data, assets, .. } = resolver::elife_source("05005", &o).unwrap() else { panic!() };
    let doc = xml::parse(&data).unwrap();
    let hrefs = jats::graphic_hrefs(doc.root());
    let imgs = images::fetch_assets(&hrefs[..3], &assets, &o);
    assert!(!imgs.is_empty());
}
