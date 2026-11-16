//! ScienceDirect (Elsevier: Cell Press, The Lancet, …) article pages as the browser shows them to
//! a reader who has access — saved from the browser ("Webpage, Complete") or sent as HTML by an
//! app or browser extension. A port of `ScienceDirect.swift`.
//!
//! The page has the full text with proper structure, figures with their captions, real
//! equations and the reference list, which makes a far better reading view than the PDF of a
//! paper that isn't open access.
//!
//! Equations: the page may already have been typeset by MathJax (a saved page always is), which
//! replaces the MathML with its own markup (`mjx-*` elements). That markup mirrors MathML closely
//! (mjx-msup → msup, …, each character in an `mjx-c` with its code point), so it is turned back
//! into MathML here.

use super::xml::{self, Node};
use super::webpage::PageImages;
use super::{esc, fail, images, net, Book, BookImage, Chapter, Heading, Options, PaperMeta, Result};
use regex::Regex;
use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

macro_rules! re {
    ($p:expr) => {{
        static R: OnceLock<Regex> = OnceLock::new();
        #[allow(clippy::regex_creation_in_loops)] // compiled once
        let r = R.get_or_init(|| Regex::new($p).unwrap());
        r
    }};
}

/// A ScienceDirect article page (with its full-text body)?
pub fn looks_like(html: &str) -> bool {
    html.contains("id=\"body\"") && (html.contains("sciencedirect.com") || html.contains("els-cdn.com"))
}

/// Converts the page into a book. `dir`: the folder the page was saved in (its `…_files`
/// folder has the images), if any; `base`: the page's address, for image links relative to it.
/// Figures come in high resolution from Elsevier's image server (unless `opts.offline`), else
/// from the saved copy, else from the image link on the page.
pub fn book(html: &str, dir: Option<&Path>, base: Option<&url::Url>, opts: &Options) -> Result<Book> {
    book_with(html, dir, base, None, opts)
}

/// [`book`] with figure images the browser extension fetched with the reader's access (`posted`,
/// used first).
pub fn book_with(
    html: &str,
    dir: Option<&Path>,
    base: Option<&url::Url>,
    posted: Option<&PageImages>,
    opts: &Options,
) -> Result<Book> {
    let no_text = || fail("This ScienceDirect page has no full text (are you signed in?).");
    let body = element_by_id(html, "body").ok_or_else(no_text)?;
    let mut meta = metadata(html);
    opts.log("Converting the ScienceDirect page");
    let mut conv = Conv { base: base.cloned(), ..Default::default() };

    let mut chapters: Vec<Chapter> = Vec::new();
    // Highlights, Summary, Graphical abstract
    if let Some(abs) = element_by_id(html, "abstracts") {
        if let Ok(doc) = parse(&abs) {
            let root = doc.root();
            let mut inner = String::new();
            let mut heads = vec![Heading { level: 1, id: "abstract".into(), title: "Summary".into() }];
            for a in root.descendants("div") {
                if !a.has_class("abstract") || a.parent().map(|p| p.has_class("abstract")).unwrap_or(false) {
                    continue;
                }
                let title = a.children().find(|c| c.name() == "h2").map(|h| h.plain().trim().to_string()).unwrap_or_default();
                let content: String = a.children().filter(|c| c.name() != "h2").map(|c| conv.render(c, 0)).collect();
                if title == "Summary" || title == "Abstract" || title.is_empty() {
                    inner += &content;
                    if meta.abstract_text.is_empty() {
                        meta.abstract_text =
                            a.children().filter(|c| c.name() != "h2").map(|c| c.plain()).collect::<Vec<_>>().join(" ");
                    }
                } else {
                    let id = a.attr("id").map(String::from).unwrap_or_else(|| format!("abs-{}", heads.len()));
                    inner += &format!("<h2 id=\"{}\">{}</h2>{content}", esc(&id), esc(&title));
                    heads.push(Heading { level: 2, id, title });
                }
            }
            chapters.push(Chapter {
                title: "Summary".into(),
                html: format!("<section><h1 id=\"abstract\">Summary</h1>{inner}</section>"),
                headings: heads,
            });
        }
    }

    // one chapter per top-level section of the body
    let doc = parse(&body)?;
    fn top_sections<'a>(n: Node<'a>, out: &mut Vec<Node<'a>>) {
        for e in n.elements() {
            if e.name() == "section" {
                out.push(e);
            } else {
                top_sections(e, out);
            }
        }
    }
    let mut sections = Vec::new();
    top_sections(doc.root(), &mut sections);
    for s in sections {
        conv.headings.clear();
        let h = conv.render(s, 0);
        let title = conv.headings.first().map(|h| h.title.clone()).unwrap_or_else(|| "Section".into());
        chapters.push(Chapter { title, html: h, headings: std::mem::take(&mut conv.headings) });
    }

    // references
    if let Some(m) = re!(r#"<section[^>]*class="bibliography"#).find(html) {
        if let Some(bib) = element_from(html, m.start(), "section") {
            if let Ok(bdoc) = parse(&bib) {
                let mut items = String::new();
                for li in bdoc.root().descendants("li") {
                    let Some(r) = li.descendants("span").into_iter().find(|s| s.has_class("reference")) else { continue };
                    let id = li
                        .descendants("a")
                        .into_iter()
                        .filter_map(|a| a.attr("id"))
                        .find(|i| i.starts_with("ref-id-bib"))
                        .map(|i| i["ref-id-".len()..].to_string());
                    let id_attr = id.map(|i| format!(" id=\"{}\"", esc(&i))).unwrap_or_default();
                    items += &format!("<li{id_attr}>{}</li>", conv.reference(r));
                }
                if !items.is_empty() {
                    chapters.push(Chapter {
                        title: "References".into(),
                        html: format!(
                            "<section><h1 id=\"references\">References</h1><ol class=\"refs\">{items}</ol></section>"
                        ),
                        headings: vec![Heading { level: 1, id: "references".into(), title: "References".into() }],
                    });
                }
            }
        }
    }
    if chapters.len() <= 2 {
        return Err(no_text());
    }

    // figure images: the high-resolution version from Elsevier's image server, else the saved copy
    if !conv.figures.is_empty() {
        opts.log(&format!("Fetching {} figure image(s)", conv.figures.len()));
    }
    let fetch_ok = |u: &str, timeout: f64| -> Option<Vec<u8>> {
        let r = net::fetch(u, None, timeout).ok()?;
        (200..300).contains(&r.status).then_some(r.data)
    };
    let from_page: Vec<Option<Vec<u8>>> = conv
        .figures
        .iter()
        .map(|f| {
            let p = posted?;
            let cands: Vec<&str> =
                [f.hi_res.as_deref(), f.remote.as_deref(), f.local.as_deref()].into_iter().flatten().collect();
            p.data(&cands).map(|(_, d)| d.to_vec())
        })
        .collect();
    let hi_res: Vec<Option<Vec<u8>>> = if opts.offline {
        from_page
    } else {
        let idx: Vec<usize> = (0..conv.figures.len()).collect();
        net::par_map(&idx, 4, |&i| {
            from_page[i].clone().or_else(|| conv.figures[i].hi_res.as_deref().and_then(|u| fetch_ok(u, 40.0)))
        })
    };
    let mut imgs: Vec<BookImage> = Vec::new();
    let mut missing = 0;
    for (f, hi) in conv.figures.iter().zip(hi_res) {
        let mut data = hi;
        if data.is_none() {
            if let (Some(local), Some(dir)) = (&f.local, dir) {
                data = local_file(local, dir);
            }
        }
        if data.is_none() && !opts.offline {
            if let Some(src) = &f.remote {
                data = fetch_ok(src, 30.0);
            }
        }
        match data.as_deref().and_then(|d| images::normalise(d, 1600)) {
            Some(n) => imgs.push(BookImage {
                file_name: format!("{}.{}", f.name, n.ext),
                data: n.data,
                media_type: n.media_type.into(),
            }),
            None => missing += 1,
        }
    }
    // point each <img> at the file it ended up as (the extension depends on the image)
    let by_name: HashMap<&str, &str> = imgs
        .iter()
        .map(|i| (i.file_name.split('.').next().unwrap_or(&i.file_name), i.file_name.as_str()))
        .collect();
    for ch in chapters.iter_mut() {
        let mut h = std::mem::take(&mut ch.html);
        for f in &conv.figures {
            let placeholder = format!("src=\"../images/{}.img\"", f.name);
            if !h.contains(&placeholder) {
                continue;
            }
            if let Some(file) = by_name.get(f.name.as_str()) {
                h = h.replace(&placeholder, &format!("src=\"../images/{file}\""));
            } else {
                let pat = Regex::new(&format!(r#"<img src="\.\./images/{}\.img" alt="[^"]*"/>"#, regex::escape(&f.name))).unwrap();
                h = pat.replace_all(&h, "<p class=\"missing\"><em>[Figure image not available]</em></p>").into_owned();
            }
        }
        ch.html = h;
    }
    link_across_chapters(&mut chapters);

    let mut notes = Vec::new();
    if missing > 0 {
        notes.push(format!("{missing} figure image(s) could not be downloaded; captions are kept."));
    }
    if conv.math_count > 0 {
        notes.push(format!("{} equation(s) as MathML.", conv.math_count));
    }
    Ok(Book { meta, chapters, images: imgs, source_label: "ScienceDirect full text".into(), notes })
}

// MARK: - Page pieces

pub fn metadata(html: &str) -> PaperMeta {
    let mut m = PaperMeta::default();
    let meta = |name: &str| -> Vec<String> {
        let key = format!("name=\"{name}\"");
        re!(r"<meta\b[^>]*>")
            .find_iter(html)
            .filter(|t| t.as_str().contains(&key))
            .filter_map(|t| re!(r#"content="([^"]*)""#).captures(t.as_str()).map(|c| unescape(&c[1])))
            .collect()
    };
    let first = |name: &str| meta(name).into_iter().next().unwrap_or_default();
    m.title = first("citation_title");
    m.doi = first("citation_doi");
    m.journal = first("citation_journal_title");
    let date = meta("citation_publication_date").into_iter().next().or_else(|| meta("citation_online_date").into_iter().next());
    m.date = date.unwrap_or_default().replace('/', "-");
    m.url = if m.doi.is_empty() { String::new() } else { format!("https://doi.org/{}", m.doi) };
    // authors: "given-name" + "surname" spans in the author list
    let mut seen = std::collections::HashSet::new();
    for c in re!(r#"<span class="given-name">([^<]*)</span>\s*<span class="text surname">([^<]*)</span>"#).captures_iter(html) {
        let name = unescape(&format!("{} {}", &c[1], &c[2]));
        if seen.insert(name.clone()) {
            m.authors.push(name);
        }
    }
    m
}

/// The element with this id, opening tag to its matching closing tag.
fn element_by_id(html: &str, id: &str) -> Option<String> {
    let at = html.find(&format!("id=\"{id}\""))?;
    let open = html[..at].rfind('<')?;
    let tag: String = html[open + 1..].chars().take_while(|c| c.is_alphanumeric() || *c == '-').collect();
    element_from(html, open, &tag)
}

/// From the opening tag at `start` to its matching closing tag.
fn element_from(html: &str, start: usize, tag: &str) -> Option<String> {
    let r = Regex::new(&format!(r"<(/?){}\b[^>]*>", regex::escape(tag))).ok()?;
    let mut depth = 0i64;
    for m in r.captures_iter(&html[start..]) {
        let whole = m.get(0).unwrap();
        if &m[1] == "/" {
            depth -= 1;
        } else if !whole.as_str().ends_with("/>") {
            depth += 1;
        }
        if depth == 0 {
            return Some(html[start..start + whole.end()].to_string());
        }
    }
    None
}

/// Browser HTML → the converter's DOM: scripts, styles, icons and MathJax's speech data
/// removed, void elements closed, stray ampersands escaped.
pub fn parse(fragment: &str) -> Result<xml::Doc> {
    let mut s = fragment.to_string();
    let fixes: [(&Regex, &str); 8] = [
        (re!(r"<!--[\s\S]*?-->"), ""),
        (re!(r"<script\b[\s\S]*?</script>"), ""),
        (re!(r"<style\b[\s\S]*?</style>"), ""),
        (re!(r"<svg\b[\s\S]*?</svg>"), ""),
        (re!(r"<button\b[\s\S]*?</button>"), ""),
        (re!(r"<mjx-speech\b[\s\S]*?</mjx-speech>"), ""),
        (re!(r#"\s(?:data-semantic-[\w-]+|data-aa-[\w-]+|aria-[\w-]+|style|tabindex|ctxtmenu_counter)="[^"]*""#), ""),
        (
            re!(r#"<(img|br|hr|meta|link|input|col|source|wbr|area|embed|param|track)\b((?:"[^"]*"|'[^']*'|[^'">])*?)\s*/?>"#),
            "<${1}${2}/>",
        ),
    ];
    for (r, with) in fixes {
        if r.is_match(&s) {
            s = r.replace_all(&s, with).into_owned();
        }
    }
    // stray ampersands (not starting an entity or character reference)
    s = re!(r"&(#?[A-Za-z0-9]+;)?")
        .replace_all(&s, |c: &regex::Captures| if c.get(1).is_some() { c[0].to_string() } else { "&amp;".into() })
        .into_owned();
    xml::parse(format!("<div>{s}</div>").as_bytes())
}

fn local_file(src: &str, dir: &Path) -> Option<Vec<u8>> {
    let decoded = net::percent_decode(src).unwrap_or_else(|| src.to_string());
    let name = decoded.rsplit('/').next().unwrap_or(&decoded).to_string();
    if let Ok(d) = std::fs::read(dir.join(&decoded)) {
        return Some(d);
    }
    // the saved page's "…_files" folder (or any folder next to it)
    let mut subs: Vec<_> = std::fs::read_dir(dir).ok()?.filter_map(|e| e.ok().map(|e| e.path())).collect();
    subs.sort();
    subs.into_iter().filter(|p| p.is_dir()).find_map(|p| std::fs::read(p.join(&name)).ok())
}

pub fn unescape(s: &str) -> String {
    let t = s
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", "\u{00A0}");
    let t = re!(r"&#(x?)([0-9A-Fa-f]+);").replace_all(&t, |c: &regex::Captures| {
        let v = if &c[1] == "x" { u32::from_str_radix(&c[2], 16).ok() } else { c[2].parse::<u32>().ok() };
        v.and_then(char::from_u32).map(String::from).unwrap_or_default()
    });
    t.replace("&amp;", "&")
}

/// `href="#x"` → `chNN.xhtml#x` when `x` is in another chapter (each chapter is its own file).
pub fn link_across_chapters(chapters: &mut [Chapter]) {
    let mut at: HashMap<String, usize> = HashMap::new();
    for (i, c) in chapters.iter().enumerate() {
        for m in re!(r#"\sid="([^"]+)""#).captures_iter(&c.html) {
            at.insert(m[1].to_string(), i);
        }
    }
    for (i, c) in chapters.iter_mut().enumerate() {
        c.html = re!(r##"href="#([^"]+)""##)
            .replace_all(&c.html, |m: &regex::Captures| match at.get(&m[1]) {
                Some(&j) if j != i => format!("href=\"ch{:02}.xhtml#{}\"", j + 1, &m[1]),
                _ => m[0].to_string(),
            })
            .into_owned();
    }
}

// MARK: - Rendering

struct Figure {
    /// file name without extension
    name: String,
    hi_res: Option<String>,
    /// src as saved
    local: Option<String>,
    remote: Option<String>,
}

#[derive(Default)]
struct Conv {
    headings: Vec<Heading>,
    figures: Vec<Figure>,
    math_count: usize,
    base: Option<url::Url>,
}

const BLOCKS: [&str; 12] = ["div", "section", "figure", "table", "ul", "ol", "p", "h2", "h3", "h4", "h5", "dl"];

impl Conv {
    fn render(&mut self, n: Node, depth: usize) -> String {
        if n.is_text() {
            return esc(n.text());
        }
        let cls = n.attr("class").unwrap_or("");
        match n.name() {
            "section" => {
                let id = n.attr("id").map(|i| format!(" id=\"{}\"", esc(i))).unwrap_or_default();
                let inner: String = n.children().map(|c| self.render(c, depth + 1)).collect();
                format!("<section{id}>{inner}</section>")
            }
            "h2" | "h3" | "h4" | "h5" | "h6" => {
                let title = n.plain().trim().to_string();
                if title.is_empty() {
                    return String::new();
                }
                let level = depth.clamp(1, 4);
                let fallback = || format!("h{}", self.headings.len());
                let id = match n.parent() {
                    Some(p) if p.name() == "section" => {
                        p.attr("id").or(n.attr("id")).map(String::from).unwrap_or_else(fallback)
                    }
                    _ => n.attr("id").map(String::from).unwrap_or_else(fallback),
                };
                if level <= 2 {
                    self.headings.push(Heading { level: level as u32, id: format!("h-{id}"), title: title.clone() });
                }
                format!("<h{level} id=\"h-{}\">{}</h{level}>", esc(&id), esc(&title))
            }
            "figure" => self.figure(n),
            "table" => {
                let inner: String = n.children().map(|c| self.table_part(c)).collect();
                format!("<table>{inner}</table>")
            }
            "mjx-container" => self.math(n),
            "img" | "svg" | "button" | "input" | "noscript" | "dl" | "mjx-speech" => String::new(),
            "br" => "<br/>".into(),
            "sup" | "sub" | "i" | "em" | "b" | "strong" | "u" | "small" | "code" => {
                format!("<{0}>{1}</{0}>", n.name(), self.inline(n))
            }
            "a" => {
                if cls.contains("download-link") {
                    return String::new();
                }
                let href = n.attr("href").unwrap_or("");
                if let Some(hash) = href.find('#') {
                    if href.contains("sciencedirect.com/science/article") || href.starts_with('#') {
                        let frag = &href[hash + 1..];
                        let inner = self.inline(n);
                        return if frag.is_empty() { inner } else { format!("<a href=\"#{}\">{inner}</a>", esc(frag)) };
                    }
                }
                if href.starts_with("http") {
                    return format!("<a href=\"{}\">{}</a>", esc(href), self.inline(n));
                }
                self.inline(n)
            }
            "ul" | "ol" => {
                if cls.contains("u-margin-s-bottom") && n.parent().and_then(|p| p.parent()).map(|g| g.name() == "figure").unwrap_or(false) {
                    return String::new();
                }
                let items: String =
                    n.children().filter(|c| c.name() == "li").map(|li| format!("<li>{}</li>", self.list_item(li))).collect();
                if items.is_empty() {
                    String::new()
                } else {
                    format!("<ul>{items}</ul>")
                }
            }
            "p" => {
                let inner: String = n.children().map(|c| self.render(c, depth)).collect();
                if inner.trim().is_empty() {
                    String::new()
                } else {
                    format!("<p>{inner}</p>")
                }
            }
            "div" => {
                if ["ReferenceLinks", "download", "keywords", "footnote", "related", "u-display-none"].iter().any(|c| cls.contains(c)) {
                    return String::new();
                }
                if cls.contains("tables") {
                    return self.table_wrap(n);
                }
                // anything block-level anywhere inside (a table wrapped in spans, …): no paragraph around it
                fn deep_block(x: Node) -> bool {
                    x.elements().any(|e| {
                        BLOCKS.contains(&e.name()) || e.has_class("captions") || (e.name() != "mjx-container" && deep_block(e))
                    })
                }
                if deep_block(n) {
                    return n.children().map(|c| self.render(c, depth)).collect();
                }
                let inner = self.inline(n);
                if inner.trim().is_empty() {
                    String::new()
                } else {
                    format!("<p>{inner}</p>")
                }
            }
            "span" => {
                if cls.contains("display") {
                    // a displayed equation inside a paragraph
                    return n.children().map(|c| self.render(c, depth)).collect();
                }
                if cls.contains("captions") {
                    return String::new(); // handled by figure / table_wrap
                }
                n.children().map(|c| self.render(c, depth)).collect()
            }
            _ => n.children().map(|c| self.render(c, depth)).collect(),
        }
    }

    fn inline(&mut self, n: Node) -> String {
        n.children().map(|c| self.render(c, 0)).collect()
    }

    fn list_item(&mut self, li: Node) -> String {
        let mut out = String::new();
        for c in li.children().filter(|c| !c.has_class("list-label")) {
            if c.has_class("list-content") {
                // "list-content" may hold paragraph divs: keep them inline in the item
                for x in c.children() {
                    if x.name() == "div" && !x.elements().any(|e| BLOCKS.contains(&e.name())) {
                        out += &self.inline(x);
                    } else {
                        out += &self.render(x, 0);
                    }
                }
            } else {
                out += &self.render(c, 0);
            }
        }
        out
    }

    fn figure(&mut self, n: Node) -> String {
        let id = n.attr("id").map(String::from).unwrap_or_else(|| format!("fig{}", self.figures.len() + 1));
        let Some(src) = n.descendants("img").first().and_then(|i| i.attr("src")).map(String::from) else {
            return String::new();
        };
        let hi = n.descendants("a").into_iter().filter_map(|a| a.attr("href")).find(|h| h.contains("_lrg.")).map(String::from);
        let name = format!("fig-{}", re!(r"[^A-Za-z0-9_-]").replace_all(&id, "-"));
        let is_remote = src.starts_with("http");
        let remote = if is_remote {
            Some(src.clone())
        } else {
            self.base.as_ref().and_then(|b| b.join(&src).ok()).filter(|u| u.scheme().starts_with("http")).map(|u| u.to_string())
        };
        self.figures.push(Figure { name: name.clone(), hi_res: hi, local: (!is_remote).then(|| src.clone()), remote });
        let mut label = String::new();
        let mut caption = String::new();
        if let Some(caps) = n.descendants("span").into_iter().find(|s| s.has_class("captions")) {
            let mut first = true;
            let parts = caps.descendants("p").into_iter().chain(caps.descendants("div"));
            for part in parts {
                let in_caps = part.parent().map(|p| p.has_class("captions")).unwrap_or(false)
                    || part.parent().and_then(|p| p.parent()).map(|g| g.has_class("captions")).unwrap_or(false);
                if !in_caps {
                    continue;
                }
                let l = if first && part.name() == "p" {
                    part.descendants("span").into_iter().find(|s| s.has_class("label"))
                } else {
                    None
                };
                if let Some(l) = l {
                    label = l.plain().trim().to_string();
                    let rest: String = part.children().filter(|c| *c != l).map(|c| self.render(c, 0)).collect();
                    let rest = re!(r"^\s*[.:|]\s*").replace(&rest, "").into_owned();
                    caption += &format!("<strong>{}.</strong> <strong>{rest}</strong>", esc(&label));
                } else {
                    caption += &format!("<span class=\"cap-p\"> {}</span>", self.inline(part));
                }
                first = false;
            }
        }
        let alt = if label.is_empty() { "Figure".to_string() } else { label };
        let cap = if caption.is_empty() { String::new() } else { format!("<figcaption>{caption}</figcaption>") };
        format!("<figure id=\"{}\"><img src=\"../images/{name}.img\" alt=\"{}\"/>{cap}</figure>", esc(&id), esc(&alt))
    }

    fn table_wrap(&mut self, n: Node) -> String {
        let mut out = String::new();
        if let Some(caps) = n.descendants("span").into_iter().find(|s| s.has_class("captions")) {
            // (caption paragraphs flattened: a <p> can't sit inside the caption's <p>)
            let t = self.inline(caps).replace("</p><p>", " ").replace("<p>", "").replace("</p>", "");
            let t = t.trim().to_string();
            if !t.is_empty() {
                out += &format!("<p class=\"table-caption\"><strong>{t}</strong></p>");
            }
        }
        for t in n.descendants("table") {
            out += &self.render(t, 0);
        }
        let id = n.attr("id").map(|i| format!(" id=\"{}\"", esc(i))).unwrap_or_default();
        format!("<div class=\"table-wrap\"{id}>{out}</div>")
    }

    fn table_part(&mut self, n: Node) -> String {
        match n.name() {
            "thead" | "tbody" | "tfoot" | "tr" => {
                let inner: String = n.children().map(|c| self.table_part(c)).collect();
                format!("<{0}>{inner}</{0}>", n.name())
            }
            "th" | "td" => {
                let mut attrs = String::new();
                for a in ["colspan", "rowspan"] {
                    if let Some(v) = n.attr(a) {
                        attrs += &format!(" {a}=\"{}\"", esc(v));
                    }
                }
                let inner: String =
                    n.children().map(|c| if c.name() == "div" { self.inline(c) } else { self.render(c, 0) }).collect();
                format!("<{0}{attrs}>{inner}</{0}>", n.name())
            }
            _ => {
                if n.is_text() {
                    String::new()
                } else {
                    n.children().map(|c| self.table_part(c)).collect()
                }
            }
        }
    }

    /// One reference: authors, title, then the journal line with its DOI link.
    fn reference(&mut self, r: Node) -> String {
        let mut parts: Vec<String> = Vec::new();
        for c in r.descendants("div") {
            if c.has_class("authors") || c.has_class("title") {
                parts.push(esc(c.plain().trim()));
            } else if c.has_class("host") || c.has_class("other-ref") {
                parts.push(self.inline(c).trim().to_string());
            }
        }
        if parts.is_empty() {
            return esc(&r.plain());
        }
        parts.into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>().join(". ").replace("..", ".")
    }

    // MARK: MathJax CHTML → MathML

    fn math(&mut self, c: Node) -> String {
        if c.first_descendant("mjx-math").is_some() {
            self.math_count += 1;
        }
        mjx_math(c)
    }
}

/// A MathJax CHTML `mjx-container` as MathML (its text if it has no `mjx-math`).
pub(crate) fn mjx_math(c: Node) -> String {
    let Some(m) = c.first_descendant("mjx-math") else { return esc(&c.plain()) };
    let block = if c.attr("display") == Some("true") { " display=\"block\"" } else { "" };
    let inner: String = m.elements().map(mml).collect();
    format!("<math xmlns=\"http://www.w3.org/1998/Math/MathML\"{block}>{inner}</math>")
}

/// The characters of a token element: each `mjx-c` carries its code point in its class
/// (`mjx-c1D462` = 𝑢); a stretched operator carries it on `mjx-stretchy-v/h`.
fn chars(n: Node) -> String {
    fn code(x: Node) -> Option<char> {
        x.attr("class")?
            .split(' ')
            .filter_map(|t| t.strip_prefix("mjx-c"))
            .find_map(|h| u32::from_str_radix(h, 16).ok().and_then(char::from_u32))
    }
    fn rec(x: Node, out: &mut String) {
        for k in x.elements() {
            match k.name() {
                "mjx-stretchy-v" | "mjx-stretchy-h" => {
                    if let Some(c) = code(k) {
                        out.push(c);
                    }
                }
                "mjx-c" => match code(k) {
                    Some(c) => out.push(c),
                    None => out.push_str(&k.plain()),
                },
                _ => rec(k, out),
            }
        }
    }
    let mut out = String::new();
    rec(n, &mut out);
    if out.is_empty() {
        n.plain()
    } else {
        out
    }
}

fn row(n: Option<Node>) -> String {
    match n {
        Some(n) => format!("<mrow>{}</mrow>", n.elements().map(mml).collect::<String>()),
        None => "<mrow/>".into(),
    }
}

fn mml(n: Node) -> String {
    let name = n.name();
    let tag = name.strip_prefix("mjx-").unwrap_or(name);
    match name {
        "mjx-mi" | "mjx-mn" | "mjx-mo" | "mjx-mtext" | "mjx-ms" => format!("<{tag}>{}</{tag}>", esc(&chars(n))),
        "mjx-mspace" => "<mspace width=\"0.28em\"/>".into(),
        "mjx-msup" | "mjx-msub" | "mjx-msubsup" => {
            let base = n.elements().find(|e| e.name() != "mjx-script");
            let scripts: Vec<Node> =
                n.child("mjx-script").map(|s| s.elements().filter(|e| e.name().starts_with("mjx-m")).collect()).unwrap_or_default();
            let b = base.map(mml).unwrap_or_else(|| "<mrow/>".into());
            if tag == "msubsup" && scripts.len() >= 2 {
                return format!("<msubsup>{b}{}{}</msubsup>", mml(scripts[1]), mml(scripts[0]));
            }
            let s = scripts.first().map(|s| mml(*s)).unwrap_or_else(|| "<mrow/>".into());
            format!("<{tag}>{b}{s}</{tag}>")
        }
        "mjx-mfrac" => format!("<mfrac>{}{}</mfrac>", row(n.first_descendant("mjx-num")), row(n.first_descendant("mjx-den"))),
        "mjx-msqrt" | "mjx-mroot" => format!("<msqrt>{}</msqrt>", row(Some(n.first_descendant("mjx-box").unwrap_or(n)))),
        "mjx-munder" | "mjx-mover" | "mjx-munderover" => {
            let base = row(n.first_descendant("mjx-base").or_else(|| n.elements().next()));
            match (n.first_descendant("mjx-under"), n.first_descendant("mjx-over")) {
                (Some(u), Some(o)) => format!("<munderover>{base}{}{}</munderover>", row(Some(u)), row(Some(o))),
                (Some(u), None) => format!("<munder>{base}{}</munder>", row(Some(u))),
                (None, Some(o)) => format!("<mover>{base}{}</mover>", row(Some(o))),
                (None, None) => base,
            }
        }
        "mjx-mrow" | "mjx-mstyle" | "mjx-mpadded" | "mjx-mphantom" | "mjx-menclose" | "mjx-mtable" | "mjx-mtr" | "mjx-mtd" => {
            let t = if matches!(name, "mjx-mstyle" | "mjx-mpadded" | "mjx-menclose") { "mrow" } else { tag };
            format!("<{t}>{}</{t}>", n.elements().map(mml).collect::<String>())
        }
        // layout wrappers (mjx-box, mjx-row, …): their math children
        _ => n.elements().map(mml).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = include_str!("testdata/sciencedirect-sample.html");

    #[test]
    fn recognises_pages() {
        assert!(looks_like(PAGE));
        assert!(!looks_like("<html><body><div id=\"body\">x</div></body></html>"));
    }

    #[test]
    fn page_becomes_book() {
        let opts = Options { offline: true, ..Default::default() };
        let b = book(PAGE, None, None, &opts).unwrap();
        assert_eq!(b.meta.title, "A Small Test Article");
        assert_eq!(b.meta.doi, "10.1016/j.test.2026.01.001");
        assert_eq!(b.meta.authors, vec!["Ada Lovelace", "Alan Turing"]);
        assert_eq!(b.meta.date, "2026-01-21");
        let titles: Vec<&str> = b.chapters.iter().map(|c| c.title.as_str()).collect();
        assert_eq!(titles, vec!["Summary", "Introduction", "Results", "References"]);
        let summary = &b.chapters[0];
        assert!(summary.html.contains("<h2 id=\"abs0002\">Highlights</h2>"), "{}", summary.html);
        assert!(summary.html.contains("<p>We test things.</p>"));
        assert_eq!(b.meta.abstract_text, "We test things.");
        let intro = &b.chapters[1].html;
        // a citation pointing into the references chapter links across files
        assert!(intro.contains("<a href=\"ch04.xhtml#bib1\"><sup>1</sup></a>"), "{intro}");
        // MathJax CHTML back to MathML
        assert!(
            intro.contains("<math xmlns=\"http://www.w3.org/1998/Math/MathML\"><msup><mi>𝑥</mi><mn>2</mn></msup></math>"),
            "{intro}"
        );
        assert!(intro.contains("&amp; more"));
        let results = &b.chapters[2];
        assert_eq!(results.headings.iter().map(|h| (h.level, h.id.as_str())).collect::<Vec<_>>(), vec![(1, "h-sec2"), (2, "h-sec2.1")]);
        // offline, no saved copy: the figure is missing but its caption stays
        assert!(results.html.contains("<figure id=\"fig1\"><p class=\"missing\"><em>[Figure image not available]</em></p><figcaption><strong>Figure 1.</strong> <strong>Overview</strong><span class=\"cap-p\"> (A) Panel.</span></figcaption></figure>"), "{}", results.html);
        assert!(results.html.contains("<div class=\"table-wrap\" id=\"tbl1\"><p class=\"table-caption\"><strong>Table 1. Counts</strong></p><table><tbody><tr><td colspan=\"2\">a</td></tr></tbody></table></div>"), "{}", results.html);
        assert!(b.chapters[3].html.contains("<li id=\"bib1\">Lovelace, A.. Notes. Journal 1, 2 (1843)</li>") || b.chapters[3].html.contains("<li id=\"bib1\">Lovelace, A. Notes. Journal 1, 2 (1843)</li>"), "{}", b.chapters[3].html);
        assert_eq!(b.notes, vec!["1 figure image(s) could not be downloaded; captions are kept.", "1 equation(s) as MathML."]);
    }

    #[test]
    fn saved_copy_of_a_figure_is_used() {
        let dir = std::env::temp_dir().join(format!("lunta-sd-{}", std::process::id()));
        let files = dir.join("Page_files");
        std::fs::create_dir_all(&files).unwrap();
        let mut png = Vec::new();
        image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(4, 4, image::Rgb([200, 0, 0])))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        std::fs::write(files.join("gr1.png"), &png).unwrap();
        let opts = Options { offline: true, ..Default::default() };
        let b = book(PAGE, Some(&dir), None, &opts).unwrap();
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(b.images.len(), 1);
        assert_eq!(b.images[0].file_name, "fig-fig1.png");
        assert!(b.chapters[2].html.contains("<img src=\"../images/fig-fig1.png\" alt=\"Figure 1\"/>"));
        assert!(b.notes.iter().all(|n| !n.contains("could not")));
    }

    #[test]
    fn html_string_and_saved_file_entry_points() {
        let opts = Options { offline: true, ..Default::default() };
        let b = super::super::html_book(PAGE, "https://www.sciencedirect.com/science/article/pii/S1", &opts).unwrap();
        assert_eq!(b.source_label, "ScienceDirect full text");
        assert!(super::super::html_book("<html><body>hello</body></html>", "", &opts).is_err());
        // a saved page goes through `convert_file`: the resolver recognises it
        let dir = std::env::temp_dir().join(format!("lunta-sd-file-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("article.html");
        std::fs::write(&file, PAGE).unwrap();
        let r = super::super::convert_file_with(&file, &dir.join("out"), &opts).unwrap();
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(r.source, "ScienceDirect full text");
        assert_eq!(r.title, "A Small Test Article");
        assert!(r.fulltext.contains("We test things."));
    }

    #[test]
    fn no_full_text_is_an_error() {
        let opts = Options { offline: true, ..Default::default() };
        let page = "<html><meta name=\"citation_title\" content=\"x\"/><div id=\"body\"><p>sciencedirect.com</p></div></html>";
        assert!(book(page, None, None, &opts).is_err());
    }
}
