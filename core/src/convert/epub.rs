//! Packages a [`Book`] as EPUB 3 (with an EPUB 2 NCX for older readers). A port of
//! `EPUB.swift` + `Zip.swift`.

use super::xml::esc;
use super::{Book, Heading, PaperMeta, Result};
use regex::Regex;
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::sync::OnceLock;
use zip::write::SimpleFileOptions;
use zip::CompressionMethod;

pub struct XFile {
    pub name: String,
    pub title: String,
    pub html: String,
}

/// Builds the EPUB in memory.
pub fn build(book: &Book, cover: Option<&[u8]>) -> Result<Vec<u8>> {
    let mut files = vec![XFile { name: "title.xhtml".into(), title: "Title page".into(), html: title_page(&book.meta) }];
    for (i, ch) in book.chapters.iter().enumerate() {
        files.push(XFile { name: format!("ch{:02}.xhtml", i + 1), title: ch.title.clone(), html: ch.html.clone() });
    }
    let files = fix_links(files);

    let mut zw = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let mut add = |name: &str, data: &[u8], compress: bool| -> Result<()> {
        zw.start_file(name, if compress && data.len() > 64 { deflated } else { stored })
            .map_err(|e| super::fail(format!("zip: {e}")))?;
        zw.write_all(data)?;
        Ok(())
    };
    add("mimetype", b"application/epub+zip", false)?;
    add("META-INF/container.xml", CONTAINER_XML.as_bytes(), true)?;
    add("EPUB/css/style.css", CSS.as_bytes(), true)?;
    if let Some(c) = cover {
        add("EPUB/images/p2e-cover.jpg", c, false)?;
        add("EPUB/text/cover.xhtml", xhtml("Cover", COVER_BODY, Some("cover"), "../css/style.css").as_bytes(), true)?;
    }
    for f in &files {
        add(&format!("EPUB/text/{}", f.name), xhtml(&f.title, &f.html, None, "../css/style.css").as_bytes(), true)?;
    }
    // two images with one name (the zip would refuse the second, and the whole book with it): keep the first
    let mut seen = std::collections::HashSet::new();
    for img in &book.images {
        if seen.insert(img.file_name.clone()) {
            add(&format!("EPUB/images/{}", img.file_name), &img.data, false)?;
        }
    }
    add("EPUB/nav.xhtml", nav(book, &files[1..]).as_bytes(), true)?;
    let id = identifier(&book.meta);
    add("EPUB/toc.ncx", ncx(book, &files, &id).as_bytes(), true)?;
    add("EPUB/content.opf", opf(book, &files, cover.is_some(), &id).as_bytes(), true)?;
    drop(add);
    let cur = zw.finish().map_err(|e| super::fail(format!("zip: {e}")))?;
    Ok(cur.into_inner())
}

/// Writes the EPUB to `path` (atomically: a temporary file is renamed into place).
pub fn write(book: &Book, cover: Option<&[u8]>, path: &Path) -> Result<()> {
    let data = build(book, cover)?;
    let tmp = path.with_extension("epub.tmp");
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

// MARK: - Documents

pub const CONTAINER_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="EPUB/content.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>"#;

const COVER_BODY: &str = "<div class=\"cover-image\"><img src=\"../images/p2e-cover.jpg\" alt=\"Cover\"/></div>";

pub fn xhtml(title: &str, body: &str, body_class: Option<&str>, css_path: &str) -> String {
    let cls = body_class.map(|c| format!(" class=\"{c}\"")).unwrap_or_default();
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\" xml:lang=\"en\" lang=\"en\">\n<head><meta charset=\"UTF-8\"/><title>{}</title><link rel=\"stylesheet\" type=\"text/css\" href=\"{css_path}\"/></head>\n<body{cls}>{body}</body>\n</html>",
        esc(title)
    )
}

pub fn title_page(m: &PaperMeta) -> String {
    let mut s = String::from("<section class=\"titlepage\" epub:type=\"titlepage\">");
    s += &format!("<h1 class=\"title\">{}</h1>", esc(if m.title.is_empty() { "Untitled paper" } else { &m.title }));
    if !m.authors.is_empty() {
        s += &format!("<p class=\"authors\">{}</p>", esc(&m.authors.join(", ")));
    }
    let mut publ = Vec::new();
    if !m.journal.is_empty() {
        publ.push(format!("<em>{}</em>", esc(&m.journal)));
    }
    if !m.date.is_empty() {
        publ.push(esc(&m.date));
    }
    if !publ.is_empty() {
        s += &format!("<p class=\"journal\">{}</p>", publ.join(" · "));
    }
    if !m.doi.is_empty() {
        s += &format!("<p class=\"doi\"><a href=\"https://doi.org/{0}\">doi:{0}</a></p>", esc(&m.doi));
    }
    if !m.affiliations_html.is_empty() {
        s += &format!("<div class=\"frontmatter\"><p class=\"affiliations\">{}</p></div>", m.affiliations_html);
    }
    if !m.license.is_empty() {
        let lic = if m.license.starts_with("http") {
            format!("<a href=\"{0}\">{0}</a>", esc(&m.license))
        } else {
            esc(&m.license)
        };
        s += &format!("<p class=\"license\">License: {lic}</p>");
    }
    s + "</section>"
}

/// Rewrites `#id` links that point into another file, and unwraps links whose target doesn't exist.
pub fn fix_links(files: Vec<XFile>) -> Vec<XFile> {
    static ID: OnceLock<Regex> = OnceLock::new();
    let id_re = ID.get_or_init(|| Regex::new(r#"\sid="([^"]+)""#).unwrap());
    static LINK: OnceLock<Regex> = OnceLock::new();
    let link_re = LINK.get_or_init(|| Regex::new(r##"<a href="#([^"]+)">([\s\S]*?)</a>"##).unwrap());
    let mut owner: HashMap<String, String> = HashMap::new();
    for f in &files {
        for c in id_re.captures_iter(&f.html) {
            owner.entry(c[1].to_string()).or_insert_with(|| f.name.clone());
        }
    }
    files
        .into_iter()
        .map(|f| {
            let html = link_re
                .replace_all(&f.html, |c: &regex::Captures| {
                    let id = &c[1];
                    let inner = &c[2];
                    match owner.get(id) {
                        Some(file) if *file == f.name => format!("<a href=\"#{id}\">{inner}</a>"),
                        Some(file) => format!("<a href=\"{file}#{id}\">{inner}</a>"),
                        None => inner.to_string(),
                    }
                })
                .into_owned();
            XFile { html, ..f }
        })
        .collect()
}

pub fn nav(book: &Book, files: &[XFile]) -> String {
    let mut s = String::from("<nav epub:type=\"toc\" id=\"toc\"><h1>Contents</h1><ol>");
    for (ch, f) in book.chapters.iter().zip(files) {
        let fallback = [Heading { level: 1, id: String::new(), title: ch.title.clone() }];
        let hs: &[Heading] = if ch.headings.is_empty() { &fallback } else { &ch.headings };
        let mut open_sub = false;
        let mut open_top = false;
        for h in hs {
            let href = if h.id.is_empty() { format!("text/{}", f.name) } else { format!("text/{}#{}", f.name, h.id) };
            let title = esc(if h.title.is_empty() { &ch.title } else { &h.title });
            if h.level <= 1 || !open_top {
                if open_sub {
                    s += "</ol>";
                    open_sub = false;
                }
                if open_top {
                    s += "</li>";
                }
                s += &format!("<li><a href=\"{href}\">{title}</a>");
                open_top = true;
            } else {
                if !open_sub {
                    s += "<ol>";
                    open_sub = true;
                }
                s += &format!("<li><a href=\"{href}\">{title}</a></li>");
            }
        }
        if open_sub {
            s += "</ol>";
        }
        if open_top {
            s += "</li>";
        }
    }
    s += "</ol></nav>";
    s += "<nav epub:type=\"landmarks\" id=\"landmarks\" hidden=\"hidden\"><ol>";
    s += "<li><a epub:type=\"toc\" href=\"#toc\">Contents</a></li>";
    if let Some(first) = files.first() {
        s += &format!("<li><a epub:type=\"bodymatter\" href=\"text/{}\">Start</a></li>", first.name);
    }
    s += "</ol></nav>";
    xhtml("Contents", &s, None, "css/style.css")
}

pub fn ncx(book: &Book, files: &[XFile], id: &str) -> String {
    let mut points = String::new();
    for (i, f) in files.iter().enumerate() {
        points += &format!(
            "<navPoint id=\"np{0}\" playOrder=\"{0}\"><navLabel><text>{1}</text></navLabel><content src=\"text/{2}\"/></navPoint>",
            i + 1,
            esc(&f.title),
            f.name
        );
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ncx xmlns=\"http://www.daisy.org/z3986/2005/ncx/\" version=\"2005-1\">\n<head><meta name=\"dtb:uid\" content=\"{}\"/><meta name=\"dtb:depth\" content=\"1\"/>\n<meta name=\"dtb:totalPageCount\" content=\"0\"/><meta name=\"dtb:maxPageNumber\" content=\"0\"/></head>\n<docTitle><text>{}</text></docTitle>\n<navMap>{points}</navMap>\n</ncx>",
        esc(id),
        esc(&book.meta.title)
    )
}

/// `doi:<doi>`, or a random `urn:uuid:` when there is no DOI. Computed once per book so the OPF
/// and the NCX agree (the Swift code draws two different UUIDs).
pub fn identifier(m: &PaperMeta) -> String {
    if m.doi.is_empty() {
        format!("urn:uuid:{}", uuid::Uuid::new_v4())
    } else {
        format!("doi:{}", m.doi)
    }
}

/// Current UTC time as `YYYY-MM-DDThh:mm:ssZ`.
pub fn utc_now() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // civil-from-days (Howard Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem / 60 % 60, rem % 60)
}

pub fn opf(book: &Book, files: &[XFile], has_cover: bool, id: &str) -> String {
    let m = &book.meta;
    let mut md = format!("<dc:identifier id=\"pub-id\">{}</dc:identifier>", esc(id));
    md += &format!("<dc:title>{}</dc:title>", esc(if m.title.is_empty() { "Untitled paper" } else { &m.title }));
    for (i, a) in m.authors.iter().take(40).enumerate() {
        md += &format!("<dc:creator id=\"creator{}\">{}</dc:creator>", i + 1, esc(a));
    }
    md += "<dc:language>en</dc:language>";
    if let Some(d) = valid_date(&m.date) {
        md += &format!("<dc:date>{d}</dc:date>");
    }
    if !m.journal.is_empty() {
        md += &format!("<dc:publisher>{}</dc:publisher>", esc(&m.journal));
    }
    if !m.abstract_text.is_empty() {
        let a: String = m.abstract_text.chars().take(1500).collect();
        md += &format!("<dc:description>{}</dc:description>", esc(&a));
    }
    if !m.license.is_empty() {
        md += &format!("<dc:rights>{}</dc:rights>", esc(&m.license));
    }
    for k in &m.keywords {
        md += &format!("<dc:subject>{}</dc:subject>", esc(k));
    }
    md += &format!("<meta property=\"dcterms:modified\">{}</meta>", utc_now());
    if has_cover {
        md += "<meta name=\"cover\" content=\"cover-image\"/>";
    }

    let mut man = String::from("<item id=\"nav\" href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"/>");
    man += "<item id=\"ncx\" href=\"toc.ncx\" media-type=\"application/x-dtbncx+xml\"/>";
    man += "<item id=\"css\" href=\"css/style.css\" media-type=\"text/css\"/>";
    let mut spine = String::new();
    if has_cover {
        man += "<item id=\"cover-image\" href=\"images/p2e-cover.jpg\" media-type=\"image/jpeg\" properties=\"cover-image\"/>";
        man += "<item id=\"cover\" href=\"text/cover.xhtml\" media-type=\"application/xhtml+xml\"/>";
        spine += "<itemref idref=\"cover\"/>";
    }
    for (i, f) in files.iter().enumerate() {
        let props = if f.html.contains("<math") { " properties=\"mathml\"" } else { "" };
        man += &format!("<item id=\"x{i}\" href=\"text/{}\" media-type=\"application/xhtml+xml\"{props}/>", f.name);
        spine += &format!("<itemref idref=\"x{i}\"/>");
    }
    let mut seen = std::collections::HashSet::new();
    for (i, img) in book.images.iter().enumerate() {
        if seen.insert(img.file_name.clone()) {
            man += &format!("<item id=\"img{i}\" href=\"images/{}\" media-type=\"{}\"/>", esc(&img.file_name), img.media_type);
        }
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" unique-identifier=\"pub-id\" xml:lang=\"en\">\n<metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\">{md}</metadata>\n<manifest>{man}</manifest>\n<spine toc=\"ncx\">{spine}</spine>\n</package>"
    )
}

/// dc:date must be W3CDTF: YYYY, YYYY-MM or YYYY-MM-DD.
pub fn valid_date(s: &str) -> Option<String> {
    let t: String = s.chars().take(10).collect();
    static D: OnceLock<Regex> = OnceLock::new();
    D.get_or_init(|| Regex::new(r"^\d{4}(-\d{2}(-\d{2})?)?$").unwrap()).is_match(&t).then_some(t)
}

pub const CSS: &str = "body { font-family: serif; line-height: 1.45; margin: 0 0.4em; }
body.cover { margin: 0; padding: 0; text-align: center; }
.cover-image img { display: block; margin: 0 auto; width: auto; height: auto; max-width: 100%; max-height: 100vh; }
h1, h2, h3, h4, h5, h6 { font-family: sans-serif; line-height: 1.25; page-break-after: avoid; }
h1 { font-size: 1.35em; margin-top: 1.6em; }
h2 { font-size: 1.15em; margin-top: 1.3em; }
h3 { font-size: 1.0em; margin-top: 1.1em; }
h4, h5, h6 { font-size: 0.95em; margin-top: 1em; }
p { margin: 0 0 0.7em 0; text-align: left; }
.titlepage { margin-top: 2em; }
.titlepage h1.title { font-family: serif; font-size: 1.6em; margin-bottom: 0.8em; }
.titlepage .authors { font-size: 1em; }
.titlepage .journal, .titlepage .doi, .titlepage .license { font-size: 0.9em; color: #444; }
figure { margin: 1.2em 0; text-align: center; page-break-inside: avoid; }
figure img { max-width: 100%; height: auto; }
figcaption, p.caption { font-size: 0.85em; text-align: left; line-height: 1.35; margin-top: 0.4em; }
img { max-width: 100%; height: auto; }
img.math { vertical-align: middle; }
.equation { text-align: center; margin: 0.8em 0; }
.equation img { max-width: 100%; height: auto; }
.eqlabel { float: right; }
math[display=\"block\"] { display: block; margin: 0.6em auto; max-width: 100%; overflow-x: auto; }
code.tex { font-size: 0.85em; }
table { border-collapse: collapse; font-size: 0.75em; margin: 0.6em 0; display: block; overflow-x: auto; }
th, td { border-bottom: 1px solid #bbb; padding: 0.2em 0.4em; vertical-align: top; text-align: left; }
thead th { border-bottom: 2px solid #555; }
.table-wrap { margin: 1.2em 0; }
.tablefoot { font-size: 0.75em; }
.frontmatter { font-size: 0.82em; color: #444; }
.box { border: 1px solid #ccc; padding: 0.5em 0.8em; margin: 1em 0; background: #f7f7f7; }
.refs, .notes { font-size: 0.85em; }
ol.refs li { margin-bottom: 0.45em; }
.missing { color: #888; font-style: italic; }
.smallcaps { font-variant: small-caps; }
ul.simple { list-style-type: none; }
a { color: inherit; }
pre, code { font-size: 0.85em; white-space: pre-wrap; }
nav#toc ol { list-style-type: none; padding-left: 1em; }";
