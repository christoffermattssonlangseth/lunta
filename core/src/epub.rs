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

/// Concatenates the section bodies into `all.xhtml` next to the sections (so relative image and
/// style paths keep working) and turns links between sections into in-page links.
fn combine(chapters: &[EpubChapter]) -> Result<String> {
    let first = chapters.first().ok_or_else(|| CoreError::Invalid("EPUB has no sections".into()))?;
    let folder = Path::new(&first.path).parent().unwrap_or(Path::new("/")).to_path_buf();
    let out = folder.join("all.xhtml");
    if out.exists() {
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
    let doc = format!("{head}<body class=\"continuous\">\n{body}</body>\n</html>\n");
    fs::write(&out, doc)?;
    Ok(out.to_string_lossy().to_string())
}
