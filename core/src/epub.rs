//! Unpacks a paper's EPUB into the device cache and reads its spine and table of contents,
//! so the apps can show it in a web view (the in-app reading view).

use crate::model::*;
use regex::Regex;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(uniffi::Record, Clone, Debug)]
pub struct EpubChapter {
    pub title: String,
    /// Absolute path of the XHTML file.
    pub path: String,
}

#[derive(uniffi::Record, Clone, Debug)]
pub struct TocEntry {
    pub title: String,
    pub level: u32,
    /// Absolute path, optionally with a #fragment.
    pub target: String,
}

#[derive(uniffi::Record, Clone, Debug)]
pub struct EpubBook {
    /// Folder the book was unpacked into (grant the web view read access to it).
    pub dir: String,
    /// One continuous XHTML document with every section (cover excluded), for scrolling reading.
    /// Sections start at anchors `#section-<n>` (n = index in `chapters`).
    pub combined: String,
    pub chapters: Vec<EpubChapter>,
    pub toc: Vec<TocEntry>,
}

pub fn open(epub: &Path, cache: &Path) -> Result<EpubBook> {
    let stamp = fs::metadata(epub).map(|m| m.len()).unwrap_or(0);
    let dir = cache.join(format!("v{stamp}"));
    if !dir.join("META-INF/container.xml").exists() {
        let _ = fs::remove_dir_all(cache);
        fs::create_dir_all(&dir)?;
        let f = fs::File::open(epub)?;
        let mut zip = zip::ZipArchive::new(f).map_err(|e| CoreError::Invalid(format!("not an EPUB: {e}")))?;
        zip.extract(&dir).map_err(|e| CoreError::Io(e.to_string()))?;
    }
    let container = fs::read_to_string(dir.join("META-INF/container.xml"))?;
    let opf_rel = attr(&container, "rootfile", "full-path").ok_or_else(|| CoreError::Invalid("EPUB without rootfile".into()))?;
    let opf_path = dir.join(&opf_rel);
    let opf_dir = opf_path.parent().unwrap_or(&dir).to_path_buf();
    let opf = fs::read_to_string(&opf_path)?;

    // manifest id -> (href, properties)
    let item_re = Regex::new(r"<item\s[^>]*>").unwrap();
    let mut items: Vec<(String, String, String)> = vec![];
    for m in item_re.find_iter(&opf) {
        let t = m.as_str();
        if let (Some(id), Some(href)) = (attr(t, "item", "id"), attr(t, "item", "href")) {
            items.push((id, href, attr(t, "item", "properties").unwrap_or_default()));
        }
    }
    let spine_re = Regex::new(r#"<itemref\s[^>]*idref="([^"]+)""#).unwrap();
    let spine: Vec<PathBuf> = spine_re
        .captures_iter(&opf)
        .filter_map(|c| items.iter().find(|(id, _, _)| *id == c[1]).map(|(_, h, _)| opf_dir.join(unescape(h))))
        .collect();

    // table of contents from the EPUB 3 nav document
    let mut toc = vec![];
    if let Some((_, nav_href, _)) = items.iter().find(|(_, _, p)| p.split_whitespace().any(|x| x == "nav")) {
        let nav_path = opf_dir.join(unescape(nav_href));
        let nav_dir = nav_path.parent().unwrap_or(&opf_dir).to_path_buf();
        let nav = fs::read_to_string(&nav_path).unwrap_or_default();
        let toc_part = nav.split("epub:type=\"landmarks\"").next().unwrap_or(&nav);
        let tok = Regex::new(r#"(?s)(<ol\b[^>]*>|</ol>|<a\s[^>]*href="([^"]+)"[^>]*>(.*?)</a>)"#).unwrap();
        let mut depth: i32 = 0;
        for c in tok.captures_iter(toc_part) {
            let t = &c[1];
            if t.starts_with("<ol") {
                depth += 1;
            } else if t == "</ol>" {
                depth -= 1;
            } else if let (Some(h), Some(label)) = (c.get(2), c.get(3)) {
                let (file, frag) = h.as_str().split_once('#').unwrap_or((h.as_str(), ""));
                let mut target = nav_dir.join(unescape(file)).to_string_lossy().to_string();
                if !frag.is_empty() {
                    target = format!("{target}#{frag}");
                }
                toc.push(TocEntry { title: strip(label.as_str()), level: depth.max(1) as u32, target });
            }
        }
    }
    let chapters: Vec<EpubChapter> = spine
        .iter()
        .map(|p| {
            let ps = p.to_string_lossy().to_string();
            let title = toc
                .iter()
                .find(|t| t.target.split('#').next() == Some(ps.as_str()))
                .map(|t| t.title.clone())
                .unwrap_or_else(|| p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default());
            EpubChapter { title, path: ps }
        })
        .collect();
    let combined = combine(&chapters)?;
    Ok(EpubBook { dir: dir.to_string_lossy().to_string(), combined, chapters, toc })
}

fn attr(text: &str, tag: &str, name: &str) -> Option<String> {
    let re = Regex::new(&format!(r#"<{tag}\b[^>]*\s{name}="([^"]*)""#)).ok()?;
    re.captures(text).map(|c| unescape(&c[1]))
}

fn unescape(s: &str) -> String {
    s.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("%20", " ")
}

fn strip(s: &str) -> String {
    let re = Regex::new(r"<[^>]+>").unwrap();
    unescape(&re.replace_all(s, "")).split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Marks the continuous document as made by this version of `combine` (an older one, without
/// lazy images, is made again).
const VIEW_MARK: &str = "data-lunta-view=\"2\"";

/// Concatenates the section bodies into `all.xhtml` next to the sections (so relative image and
/// style paths keep working) and turns links between sections into in-page links. Images load
/// lazily and keep their place while they load (`lazy_images`).
pub(crate) fn combine(chapters: &[EpubChapter]) -> Result<String> {
    let first = chapters.first().ok_or_else(|| CoreError::Invalid("EPUB has no sections".into()))?;
    let folder = Path::new(&first.path).parent().unwrap_or(Path::new("/")).to_path_buf();
    let out = folder.join("all.xhtml");
    if starts_with_mark(&out, VIEW_MARK) {
        return Ok(out.to_string_lossy().to_string());
    }
    let body_re = Regex::new(r"(?s)<body[^>]*>(.*)</body>").unwrap();
    let file_names: Vec<String> = chapters
        .iter()
        .map(|c| Path::new(&c.path).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default())
        .collect();
    let mut head = String::new();
    let mut body = String::new();
    for (i, c) in chapters.iter().enumerate() {
        if file_names[i] == "cover.xhtml" {
            continue;
        }
        let text = fs::read_to_string(&c.path)?;
        if head.is_empty() {
            head = text.split("<body").next().unwrap_or("").to_string();
        }
        let inner = body_re.captures(&text).map(|m| m[1].to_string()).unwrap_or_default();
        body += &format!("<div class=\"section-start\" id=\"section-{i}\"></div>\n{inner}\n");
    }
    // links between sections: "ch05.xhtml#id" -> "#id", "ch05.xhtml" -> "#section-<n>"
    for (i, name) in file_names.iter().enumerate() {
        if name.is_empty() {
            continue;
        }
        body = body.replace(&format!("href=\"{name}#"), "href=\"#");
        body = body.replace(&format!("href=\"{name}\""), &format!("href=\"#section-{i}\""));
    }
    let body = lazy_images(&body, &folder);
    let doc = format!("{head}<body class=\"continuous\" {VIEW_MARK}>\n{body}</body>\n</html>\n");
    fs::write(&out, doc)?;
    Ok(out.to_string_lossy().to_string())
}

/// True when the file's first few kilobytes (its head and <body> tag) hold `mark`.
fn starts_with_mark(path: &Path, mark: &str) -> bool {
    use std::io::Read;
    let Ok(f) = fs::File::open(path) else { return false };
    let mut buf = Vec::new();
    if f.take(16 * 1024).read_to_end(&mut buf).is_err() {
        return false;
    }
    String::from_utf8_lossy(&buf).contains(mark)
}

/// Images in a reading view: `loading="lazy"` and `decoding="async"` (WebKit decodes only those
/// near the screen: a textbook's 600 equation images, decoded at once, are over half a gigabyte),
/// and their pixel size as `width`/`height` where the tag has none, so the page keeps its layout
/// while they load (the reader's CSS has `height: auto`, so the size only gives the proportions).
/// The text is unchanged (attributes add no text), so highlights and reading positions stay put.
pub(crate) fn lazy_images(html: &str, folder: &Path) -> String {
    let re = Regex::new(r"<img\b[^>]*>").unwrap();
    let attr_re = Regex::new(r#"\s(loading|decoding|width|height)\s*="#).unwrap();
    let src_re = Regex::new(r#"\ssrc\s*=\s*"([^"]*)""#).unwrap();
    re.replace_all(html, |c: &regex::Captures| {
        let tag = &c[0];
        let present: Vec<&str> = attr_re.captures_iter(tag).map(|m| m.get(1).unwrap().as_str()).collect();
        let has = |name: &str| present.contains(&name);
        let mut add = String::new();
        if !has("loading") {
            add += " loading=\"lazy\"";
        }
        if !has("decoding") {
            add += " decoding=\"async\"";
        }
        if !has("width") && !has("height") {
            let src = src_re.captures(tag).map(|m| unescape(&m[1]));
            if let Some((w, h)) = src.filter(|s| !s.contains(':')).and_then(|s| image_size(&folder.join(s))) {
                add += &format!(" width=\"{w}\" height=\"{h}\"");
            }
        }
        if add.is_empty() {
            return tag.to_string();
        }
        let cut = if tag.ends_with("/>") { tag.len() - 2 } else { tag.len() - 1 };
        format!("{}{}{}", tag[..cut].trim_end(), add, &tag[cut..])
    })
    .into_owned()
}

/// Width and height in pixels of a PNG, JPEG or GIF, from its header (None: another format or unreadable).
pub(crate) fn image_size(path: &Path) -> Option<(u32, u32)> {
    use std::io::Read;
    let mut f = fs::File::open(path).ok()?;
    let mut head = vec![0u8; 64 * 1024];
    let n = f.read(&mut head).ok()?;
    let b = &head[..n];
    if b.len() >= 24 && b.starts_with(b"\x89PNG\r\n\x1a\n") {
        let w = u32::from_be_bytes([b[16], b[17], b[18], b[19]]);
        let h = u32::from_be_bytes([b[20], b[21], b[22], b[23]]);
        return (w > 0 && h > 0).then_some((w, h));
    }
    if b.len() >= 10 && (b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a")) {
        let w = u16::from_le_bytes([b[6], b[7]]) as u32;
        let h = u16::from_le_bytes([b[8], b[9]]) as u32;
        return (w > 0 && h > 0).then_some((w, h));
    }
    if b.len() >= 4 && b[0] == 0xFF && b[1] == 0xD8 {
        // JPEG: the frame header (SOFn) holds the size; markers before it are skipped by their length
        let mut i = 2;
        while i + 9 < b.len() {
            if b[i] != 0xFF {
                i += 1;
                continue;
            }
            let m = b[i + 1];
            if m == 0xFF {
                i += 1;
                continue;
            }
            if m == 0xD8 || m == 0x01 || (0xD0..=0xD7).contains(&m) {
                i += 2;
                continue;
            }
            let len = u16::from_be_bytes([b[i + 2], b[i + 3]]) as usize;
            if (0xC0..=0xCF).contains(&m) && m != 0xC4 && m != 0xC8 && m != 0xCC {
                let h = u16::from_be_bytes([b[i + 5], b[i + 6]]) as u32;
                let w = u16::from_be_bytes([b[i + 7], b[i + 8]]) as u32;
                return (w > 0 && h > 0).then_some((w, h));
            }
            i += 2 + len;
        }
    }
    None
}

// ---- big reading views, chapter by chapter ----

/// One page of a big reading view read chapter by chapter: one or more sections in a file of
/// their own, `lunta-part-<n>.xhtml` next to the sections.
///
/// Its text is exactly a slice of the continuous document's (`combine`): the same `<body>`
/// newline, then for each section the same `section-start` anchor, newline, section and
/// newline. So an offset in the continuous view's text (where highlights and reading positions
/// are stored, on every device) is `start` plus the offset in this part's text, and a highlight
/// made in one chapter on the iPhone is found by the Mac in the whole book, and the other way.
#[derive(uniffi::Record, Clone, Debug)]
pub struct ReadingPart {
    /// Absolute path of the part's page.
    pub path: String,
    /// Its first section's title.
    pub title: String,
    /// The sections in it (indices into `EpubBook::chapters`).
    pub sections: Vec<u32>,
    /// Where its text starts in the continuous view's text (UTF-16 code units, as the page counts).
    pub start: u64,
    /// Its text's length (UTF-16 code units).
    pub length: u64,
    pub words: u64,
    pub images: u64,
    /// Its text as the page has it (every text node of its <body>): the app finds highlights
    /// and searches the whole book in these.
    pub text: String,
}

#[derive(uniffi::Record, Clone, Debug)]
pub struct ReadingParts {
    /// Shown chapter by chapter (a big reading view); false: the continuous view, and `parts` is empty.
    pub chaptered: bool,
    pub parts: Vec<ReadingPart>,
    /// The part holding the reference list ("References", "Bibliography"), if any: citations in
    /// other chapters find their references in it.
    pub bibliography: Option<u32>,
    pub words: u64,
    pub images: u64,
    /// Total size of the images the sections show (bytes).
    pub image_bytes: u64,
}

/// When a reading view is read chapter by chapter: more than a few sections, and more than a
/// phone's web view lays out comfortably as one page (a long review paper has ~15,000 words and
/// 10-40 figures; the textbook that went black on an iPhone had 204,000 words and 677 images).
pub(crate) fn chaptered(sections: usize, words: u64, images: u64, image_bytes: u64) -> bool {
    sections >= 4 && (words >= 60_000 || images >= 150 || image_bytes >= 30_000_000)
}

/// Sections are put together into one part until it has this many words (or images): the title
/// page, front matter and a short preface share a page with what follows.
const PART_WORDS: u64 = 1_000;
const PART_IMAGES: u64 = 12;

/// Version of the part files (made again when it changes).
const PART_MARK: &str = "lunta-part-v1";

/// The reading view's parts (see `ReadingPart`), writing their files when needed; for a small
/// reading view only the decision (`chaptered` false). `force`: parts even for a small one (tests).
pub fn reading_parts(book: &EpubBook, force: bool) -> Result<ReadingParts> {
    let body_re = Regex::new(r"(?s)<body[^>]*>(.*)</body>").unwrap();
    let img_re = Regex::new(r#"<img\b[^>]*\ssrc\s*=\s*"([^"]*)""#).unwrap();
    let first = book.chapters.first().ok_or_else(|| CoreError::Invalid("EPUB has no sections".into()))?;
    let folder = Path::new(&first.path).parent().unwrap_or(Path::new("/")).to_path_buf();
    let file_names: Vec<String> = book
        .chapters
        .iter()
        .map(|c| Path::new(&c.path).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default())
        .collect();
    struct Sec {
        index: usize,
        inner: String,
        words: u64,
        images: u64,
        heading: String,
    }
    let heading_re = Regex::new(r"(?s)<h[1-3]\b[^>]*>(.*?)</h[1-3]>").unwrap();
    let mut head = String::new();
    let mut secs: Vec<Sec> = vec![];
    let (mut words, mut images, mut image_bytes) = (0u64, 0u64, 0u64);
    let mut seen_images = std::collections::HashSet::new();
    for (i, c) in book.chapters.iter().enumerate() {
        if file_names[i] == "cover.xhtml" {
            continue;
        }
        let text = fs::read_to_string(&c.path)?;
        if head.is_empty() {
            head = text.split("<body").next().unwrap_or("").to_string();
        }
        let inner = body_re.captures(&text).map(|m| m[1].to_string()).unwrap_or_default();
        let w = crate::highlight::dom_text(&format!("<body>{inner}</body>")).split_whitespace().count() as u64;
        let mut n = 0u64;
        for m in img_re.captures_iter(&inner) {
            n += 1;
            let src = unescape(&m[1]);
            if seen_images.insert(src.clone()) {
                image_bytes += fs::metadata(folder.join(&src)).map(|m| m.len()).unwrap_or(0);
            }
        }
        let heading = heading_re.captures(&inner).map(|m| strip(&m[1])).unwrap_or_default();
        words += w;
        images += n;
        secs.push(Sec { index: i, inner, words: w, images: n, heading });
    }
    let is_chaptered = chaptered(secs.len(), words, images, image_bytes);
    let mut out = ReadingParts { chaptered: is_chaptered, parts: vec![], bibliography: None, words, images, image_bytes };
    if !(is_chaptered || force) || secs.is_empty() {
        return Ok(out);
    }

    // sections into parts
    let mut groups: Vec<Vec<usize>> = vec![];
    let (mut gw, mut gi) = (0u64, 0u64);
    for (k, s) in secs.iter().enumerate() {
        if groups.is_empty() || gw >= PART_WORDS || gi >= PART_IMAGES {
            groups.push(vec![]);
            gw = 0;
            gi = 0;
        }
        groups.last_mut().unwrap().push(k);
        gw += s.words;
        gi += s.images;
    }
    let part_name = |p: usize| format!("lunta-part-{p}.xhtml");
    let mut part_of = vec![usize::MAX; book.chapters.len()];
    for (p, g) in groups.iter().enumerate() {
        for &k in g {
            part_of[secs[k].index] = p;
        }
    }
    let title_of = |g: &Vec<usize>| -> String {
        let s = &secs[g[0]];
        let t = book.chapters[s.index].title.clone();
        if t.is_empty() || t == Path::new(&book.chapters[s.index].path).file_stem().map(|x| x.to_string_lossy().to_string()).unwrap_or_default() {
            if s.heading.is_empty() { t } else { s.heading.clone() }
        } else {
            t
        }
    };
    let titles: Vec<String> = groups.iter().map(title_of).collect();
    let refs_re = Regex::new(r"(?i)^\s*([0-9IVX]+(\.\d+)*\.?\s+)?(references( and notes| cited)?|bibliography|literature( cited)?|works cited|cited literature|reference list)\s*$").unwrap();

    // the parts' bodies, links between sections pointing into the right part
    let mut bodies: Vec<String> = vec![];
    for (p, g) in groups.iter().enumerate() {
        let mut body = String::new();
        for &k in g {
            let s = &secs[k];
            body += &format!("<div class=\"section-start\" id=\"section-{}\"></div>\n{}\n", s.index, s.inner);
        }
        for (i, name) in file_names.iter().enumerate() {
            if name.is_empty() || part_of[i] == usize::MAX {
                continue;
            }
            if part_of[i] == p {
                body = body.replace(&format!("href=\"{name}#"), "href=\"#");
                body = body.replace(&format!("href=\"{name}\""), &format!("href=\"#section-{i}\""));
            } else {
                let other = part_name(part_of[i]);
                body = body.replace(&format!("href=\"{name}#"), &format!("href=\"{other}#"));
                body = body.replace(&format!("href=\"{name}\""), &format!("href=\"{other}#section-{i}\""));
            }
        }
        if out.bibliography.is_none() && g.iter().any(|&k| refs_re.is_match(&secs[k].heading)) {
            out.bibliography = Some(p as u32);
        }
        bodies.push(lazy_images(&body, &folder));
    }
    // the way to the chapters before and after: empty links (no text), labelled by the reader's CSS
    let nav = |p: usize, d: i64| -> String {
        let q = p as i64 + d;
        if q < 0 || q as usize >= groups.len() {
            return String::new();
        }
        let (cls, label) = if d > 0 { ("lunta-next", "Next") } else { ("lunta-prev", "Previous") };
        format!(
            "<a class=\"lunta-part-nav {cls}\" href=\"{}\" data-t=\"{label}: {}\"></a>",
            part_name(q as usize),
            xml_escape(&titles[q as usize])
        )
    };
    let mut start = 0u64;
    for (p, g) in groups.iter().enumerate() {
        let body_tag = format!("<body class=\"continuous lunta-part\">{}\n", nav(p, -1));
        let draft = format!("<html>{body_tag}{}{}</body></html>", bodies[p], nav(p, 1));
        let text = crate::highlight::dom_text(&draft);
        let length = text.encode_utf16().count() as u64;
        let metas = format!(
            "<meta name=\"lunta-text-base\" content=\"{start}\"/><meta name=\"lunta-part\" content=\"{p}\" data-parts=\"{}\" data-version=\"{PART_MARK}\"/>",
            groups.len()
        );
        let mut h = head.clone();
        h = Regex::new(r"(?s)<title>.*?</title>").unwrap().replace(&h, format!("<title>{}</title>", xml_escape(&titles[p])).as_str()).into_owned();
        h = if let Some(i) = h.find("</head>") { format!("{}{}{}", &h[..i], metas, &h[i..]) } else { h + &metas };
        let doc = format!("{h}{body_tag}{}{}</body>\n</html>\n", bodies[p], nav(p, 1));
        let path = folder.join(part_name(p));
        if fs::read_to_string(&path).map(|old| old != doc).unwrap_or(true) {
            fs::write(&path, &doc)?;
        }
        out.parts.push(ReadingPart {
            path: path.to_string_lossy().to_string(),
            title: titles[p].clone(),
            sections: g.iter().map(|&k| secs[k].index as u32).collect(),
            start,
            length,
            words: g.iter().map(|&k| secs[k].words).sum(),
            images: g.iter().map(|&k| secs[k].images).sum(),
            text,
        });
        // the next part's body newline is this one's last newline
        start += length.saturating_sub(1);
    }
    Ok(out)
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}
