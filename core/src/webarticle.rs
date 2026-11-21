//! The main text of a web page or blog post, for its reading view: a readability-style reader.
//!
//! The converter's readers (`convert::webpage`) are made for journals' article pages and refuse
//! a page without a substantial, sectioned body. A blog post is often short, has few headings or
//! none, and carries code, quotes and pictures; this reader takes it as it is:
//!
//! - the main text is the page's `<article>` / `<main>` / `itemprop="articleBody"` / the usual
//!   post containers (`post-content`, `entry-content`, `markdown-body`, Substack's
//!   `available-content`, …) holding most of the page's text, else the element holding 80 % of it;
//! - navigation, headers and footers, sidebars, comments (Disqus), share and subscribe widgets,
//!   related posts, cookie banners, scripts and forms are left out;
//! - kept: headings (the top level starts a chapter of the book), paragraphs, lists, code
//!   blocks (`<pre><code>`, monospaced, with their language), blockquotes, tables, figures and
//!   images, math (MathML, TeX source), links (absolute), footnotes and the anchors they use.
//!
//! Images are not fetched here: each becomes `<img src="lunta-image:N" alt="…"/>`, N its index in
//! [`WebArticle::images`]; the converter (Rust, `convert::webpage::web_book`) or the Apple apps
//! (Paper2EpubKit) fetch them with the same rules as figures and put the files in with
//! [`with_images`]. This module is in every build (the Apple apps call it through UniFFI).

use crate::convert::html;
use crate::convert::xml::{esc, Doc, Node, NodeId};
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

macro_rules! re {
    ($p:expr) => {{
        static RE: OnceLock<Regex> = OnceLock::new();
        RE.get_or_init(|| Regex::new($p).unwrap())
    }};
}

/// Where an image goes in the article's HTML before it is fetched.
pub const IMAGE_SCHEME: &str = "lunta-image:";
/// Where a chapter starts in the rendered text: the mark, its index, the end mark.
const CHAPTER_MARK: char = '\u{1}';
const CHAPTER_MARK_END: char = '\u{2}';
/// Fewer words than this: not enough for a reading view (a page that is all script, a gallery).
pub const MIN_WORDS: u32 = 60;

#[derive(uniffi::Record, Clone, Debug, Default, PartialEq)]
pub struct WebHeading {
    /// 1: a chapter, 2: a section in it
    pub level: u32,
    pub id: String,
    pub title: String,
}

#[derive(uniffi::Record, Clone, Debug, Default, PartialEq)]
pub struct WebChapter {
    pub title: String,
    /// XHTML: `<section><h1 id="…">Title</h1>…</section>`
    pub html: String,
    pub headings: Vec<WebHeading>,
}

#[derive(uniffi::Record, Clone, Debug, Default, PartialEq)]
pub struct WebArticle {
    pub chapters: Vec<WebChapter>,
    /// The images' addresses (absolute); `lunta-image:N` in the HTML is the N-th.
    pub images: Vec<String>,
    pub words: u32,
    /// Why there is no reading view ("" when there is one): too little text.
    pub problem: String,
}

/// The article's HTML with its images in: `files[N]` the file name in the book's images folder
/// for `lunta-image:N` (`../images/<name>`), None for one that couldn't be fetched (left out).
pub fn with_images(html: &str, files: &[Option<String>]) -> String {
    let out = re!(r#"<img src="lunta-image:(\d+)" alt="([^"]*)"/>"#).replace_all(html, |c: &regex::Captures| {
        match c[1].parse::<usize>().ok().and_then(|i| files.get(i).cloned().flatten()) {
            Some(f) => format!("<img src=\"../images/{}\" alt=\"{}\"/>", esc(&f), &c[2]),
            None => String::new(),
        }
    });
    let out = re!(r"<figure>\s*(<figcaption>\s*</figcaption>)?\s*</figure>").replace_all(&out, "");
    re!(r"<p>\s*</p>").replace_all(&out, "").into_owned()
}

/// The main text of a page (see the module documentation). `title`: the page's title, shown as
/// the first chapter's heading (a heading in the page that repeats it is left out).
pub fn extract(html_text: &str, url: &str, title: &str) -> WebArticle {
    extract_doc(&html::parse(html_text), url, title)
}

pub fn extract_doc(doc: &Doc, url: &str, title: &str) -> WebArticle {
    let root = doc.root();
    let base = root
        .first(|n| n.name() == "base")
        .and_then(|b| b.attr("href"))
        .map(|h| join_url(url, h))
        .filter(|b| b.starts_with("http"))
        .unwrap_or_else(|| url.to_string());
    let body = root.first(|n| n.name() == "body").unwrap_or(root);
    let mut r = Reader { base, title: title.trim().to_string(), ..Default::default() };
    let Some(main) = r.main_element(body) else {
        return WebArticle { problem: too_little(0), ..Default::default() };
    };
    for a in main.all(|n| n.name() == "a") {
        if let Some(i) = a.attr("href").and_then(|h| h.find('#').map(|i| h[i + 1..].to_string())) {
            r.targets.insert(clean_id(&i));
        }
    }
    let levels: Vec<usize> = main
        .all(|n| heading_level(n).is_some() && !r.junk(n))
        .into_iter()
        .filter(|h| !r.is_title(*h) && !h.plain().is_empty())
        .filter_map(heading_level)
        .collect();
    r.top = levels.iter().copied().min().unwrap_or(2);
    let mut deeper: Vec<usize> = levels.iter().copied().filter(|l| *l > r.top).collect();
    deeper.sort();
    deeper.dedup();
    r.deeper = deeper;
    let t = if r.title.is_empty() { "Untitled".to_string() } else { r.title.clone() };
    r.start_chapter(&t, "title");
    let html = r.blocks(main);
    // the top-level headings left a mark where their chapter starts
    for (i, part) in html.split(CHAPTER_MARK).enumerate() {
        let (k, text) = if i == 0 { (0, part) } else { part.split_once(CHAPTER_MARK_END).map(|(n, t)| (n.parse().unwrap_or(0), t)).unwrap_or((0, part)) };
        if let Some(c) = r.chapters.get_mut(k) {
            c.html += text;
        }
    }
    let chapters = r.finish();
    let words: usize = chapters.iter().map(|c| words(&text_of(&c.html))).sum::<usize>().saturating_sub(words(&t));
    let words = words as u32;
    WebArticle {
        problem: if words < MIN_WORDS { too_little(words) } else { String::new() },
        chapters,
        images: r.images,
        words,
    }
}

fn too_little(words: u32) -> String {
    format!("This page has too little text for a reading view ({words} words found). Its details and address are saved.")
}

/// `href` against `base`: absolute, protocol-relative (`//host/…`), root-relative (`/a/b`) and
/// relative (`../img.png`) addresses.
pub fn join_url(base: &str, href: &str) -> String {
    let h = href.trim();
    if h.is_empty() {
        return base.to_string();
    }
    let lower = h.to_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("mailto:") || lower.starts_with("data:") {
        return h.to_string();
    }
    let (scheme, rest) = base.split_once("://").unwrap_or(("https", base));
    if let Some(r) = h.strip_prefix("//") {
        return format!("{scheme}://{r}");
    }
    let host_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (host, path) = rest.split_at(host_end);
    let origin = format!("{scheme}://{host}");
    if h.starts_with('#') {
        let b = base.split('#').next().unwrap_or(base);
        return format!("{b}{h}");
    }
    if h.starts_with('?') {
        let b = base.split(['?', '#']).next().unwrap_or(base);
        return format!("{b}{h}");
    }
    let path = path.split(['?', '#']).next().unwrap_or("");
    let joined = if h.starts_with('/') {
        h.to_string()
    } else {
        let dir = match path.rfind('/') {
            Some(i) => &path[..=i],
            None => "/",
        };
        format!("{dir}{h}")
    };
    // remove . and .. segments (the query and fragment kept as they are)
    let (p, tail) = match joined.find(['?', '#']) {
        Some(i) => (joined[..i].to_string(), joined[i..].to_string()),
        None => (joined.clone(), String::new()),
    };
    let mut segs: Vec<&str> = vec![];
    let parts: Vec<&str> = p.split('/').collect();
    for (i, s) in parts.iter().enumerate() {
        match *s {
            "." => {
                if i == parts.len() - 1 {
                    segs.push("");
                }
            }
            ".." => {
                if segs.len() > 1 {
                    segs.pop();
                }
                if i == parts.len() - 1 {
                    segs.push("");
                }
            }
            s => segs.push(s),
        }
    }
    let mut p = segs.join("/");
    if !p.starts_with('/') {
        p.insert(0, '/');
    }
    format!("{origin}{p}{tail}")
}

pub(crate) fn words(s: &str) -> usize {
    s.split_whitespace().filter(|w| w.chars().any(|c| c.is_alphanumeric())).count()
}

fn text_of(h: &str) -> String {
    html::decode(&re!(r"<[^>]+>").replace_all(h, " "))
}

fn heading_level(n: Node) -> Option<usize> {
    match n.name() {
        "h1" => Some(1),
        "h2" => Some(2),
        "h3" => Some(3),
        "h4" => Some(4),
        "h5" => Some(5),
        "h6" => Some(6),
        _ => None,
    }
}

fn norm(s: &str) -> String {
    s.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect()
}

/// An id that is a valid XHTML id (and the same for the links to it).
fn clean_id(raw: &str) -> String {
    let mut id: String = raw.chars().map(|c| if c.is_ascii_alphanumeric() || "-_.".contains(c) { c } else { '-' }).collect();
    if !id.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') {
        id.insert(0, 'x');
    }
    id
}

/// Elements that never hold the text.
const DROP: &[&str] = &[
    "script", "style", "noscript", "template", "iframe", "nav", "aside", "footer", "form", "button", "input", "select",
    "textarea", "svg", "canvas", "video", "audio", "object", "embed", "link", "meta", "title", "head", "dialog", "label",
    "mjx-assistive-mml", "source", "track", "map", "area",
];
/// Class and id words of what is around a post, not in it.
const JUNK: &[&str] = &[
    "comments", "comment", "disqus", "respond", "replies", "related", "recommended", "recommendations", "newsletter", "subscribe",
    "subscription", "signup", "paywall", "cookie", "cookies", "consent", "gdpr", "share", "sharing", "sharedaddy", "social",
    "sidebar", "widget", "widgets", "advert", "advertisement", "ads", "ad", "adslot", "promo", "sponsored", "breadcrumb",
    "breadcrumbs", "pagination", "pager", "navigation", "navbar", "nav", "menu", "toolbar", "popup", "modal", "overlay",
    "skip", "masthead", "topbar", "banner", "hidden", "sr", "visually", "offscreen", "print", "byline", "author", "avatar",
    "bio", "tags", "tag", "categories", "meta", "postmeta", "reactions", "likes", "clap", "follow", "prev", "next",
    "previous", "jump", "toc", "edit", "footer", "header",
];
/// Junk words that leave the element out whatever its size (comments can be long).
const JUNK_ALWAYS: &[&str] = &["comments", "comment", "disqus", "respond", "replies", "related", "recommended", "recommendations", "newsletter", "subscribe", "sharedaddy", "cookie", "consent"];
/// Where the post usually is.
const MAIN_CLASSES: &[&str] = &[
    "post-content", "entry-content", "article-content", "article-body", "article__body", "post-body", "postbody", "post__content",
    "entry-body", "markdown-body", "blog-post", "post-entry", "story-body", "available-content", "body markup", "rich-text",
    "single-content", "td-post-content", "post-full-content", "gh-content", "content-body", "prose", "post", "entry", "article",
    "content", "page-content", "main-content",
];
const BLOCKS: &[&str] = &[
    "address", "article", "blockquote", "details", "div", "dl", "dt", "dd", "figcaption", "figure", "h1", "h2", "h3", "h4",
    "h5", "h6", "header", "hr", "li", "main", "ol", "p", "pre", "section", "table", "ul", "summary", "center", "caption",
    "tbody", "thead", "tfoot", "tr", "td", "th", "menu", "#document", "body", "html", "hgroup",
];
/// Where text counts as the article's (not a menu's or a button's).
const TEXT_HOLDERS: &[&str] = &["p", "pre", "li", "blockquote", "td", "dd", "figcaption", "h2", "h3", "h4", "h5", "h6"];

#[derive(Default)]
struct Chapter {
    title: String,
    html: String,
    headings: Vec<WebHeading>,
}

#[derive(Default)]
struct Reader {
    base: String,
    title: String,
    chapters: Vec<Chapter>,
    images: Vec<String>,
    image_index: HashMap<String, usize>,
    targets: HashSet<String>,
    used_ids: HashSet<String>,
    top: usize,
    deeper: Vec<usize>,
    block_memo: HashMap<NodeId, bool>,
    title_seen: bool,
}

impl Reader {
    fn junk(&self, n: Node) -> bool {
        if n.is_text() || n.is_math() {
            return false;
        }
        let name = n.name();
        if DROP.contains(&name) {
            return true;
        }
        if n.attr("hidden").is_some() {
            return true;
        }
        if n.attr("aria-hidden") == Some("true") && n.first_descendant("math").is_none() && n.first_descendant("img").is_none() {
            return true;
        }
        if let Some(st) = n.attr("style") {
            let st = st.replace(' ', "").to_lowercase();
            if st.contains("display:none") || st.contains("visibility:hidden") {
                return true;
            }
        }
        if matches!(n.attr("role"), Some("navigation" | "complementary" | "banner" | "contentinfo" | "search" | "dialog" | "alert")) {
            return true;
        }
        // KaTeX's drawing of an equation (its MathML is kept)
        if n.has_class("katex-html") || n.classes().iter().any(|c| c.starts_with("MathJax_Preview")) {
            return true;
        }
        if name == "header" {
            // a post's header: its title and byline (the reading view has its own)
            return words(&n.plain()) < 120;
        }
        if matches!(name, "article" | "main" | "body" | "html" | "#document" | "pre" | "code" | "table" | "math" | "figure") || n.attr("role") == Some("main") {
            return false;
        }
        let tok = n.name_tokens();
        if tok.is_empty() {
            return false;
        }
        if JUNK_ALWAYS.iter().any(|j| tok.contains(*j)) && !tok.contains("content") && !tok.contains("body") && n.first(|x| x.name() == "article").is_none() {
            return true;
        }
        if JUNK.iter().any(|j| tok.contains(*j)) && !tok.contains("content") && !tok.contains("body") && !tok.contains("footnotes") {
            let all = words(&n.plain());
            let linked: usize = n.all(|x| x.name() == "a").iter().map(|a| words(&a.plain())).sum();
            let paragraphs: usize = n.all(|x| x.name() == "p").iter().map(|p| words(&p.plain())).sum();
            if paragraphs < 120 && (all < 80 || linked * 2 > all) {
                return true;
            }
        }
        false
    }

    /// Words of article text in each element (text in paragraphs, list items, code, quotes).
    fn count(&self, n: Node, in_text: bool, map: &mut HashMap<NodeId, usize>) -> usize {
        if n.is_text() {
            return if in_text { words(n.text()) } else { 0 };
        }
        if self.junk(n) {
            map.insert(n.id, 0);
            return 0;
        }
        let holds = in_text || TEXT_HOLDERS.contains(&n.name());
        // a list that is all links (a menu, a list of posts) isn't the article
        if !in_text && (n.name() == "ul" || n.name() == "ol") {
            let all = words(&n.plain());
            let linked: usize = n.all(|x| x.name() == "a").iter().map(|a| words(&a.plain())).sum();
            if all > 0 && linked * 10 >= all * 8 {
                map.insert(n.id, 0);
                return 0;
            }
        }
        let w = n.children().map(|c| self.count(c, holds, map)).sum();
        map.insert(n.id, w);
        w
    }

    fn main_element<'a>(&self, body: Node<'a>) -> Option<Node<'a>> {
        let mut map = HashMap::new();
        let total = self.count(body, false, &mut map);
        if total == 0 {
            return None;
        }
        let get = |n: &Node| map.get(&n.id).copied().unwrap_or(0);
        let candidate = |n: Node| {
            matches!(n.name(), "article" | "main")
                || n.attr("role") == Some("main")
                || n.attr("itemprop") == Some("articleBody")
                || n.attr("class").is_some_and(|c| {
                    let c = c.to_lowercase();
                    MAIN_CLASSES.iter().any(|m| c == *m || c.split_whitespace().any(|x| x == *m) || (m.contains(' ') && c.contains(m)))
                })
        };
        let cands: Vec<Node> = body.all(candidate).into_iter().filter(|c| get(c) * 2 >= total).collect();
        if let Some(most) = cands.iter().map(get).max() {
            let good: Vec<Node> = cands.iter().copied().filter(|c| get(c) * 100 >= most * 85).collect();
            if let Some(best) = good.into_iter().max_by_key(|c| c.ancestors().len()) {
                return Some(best);
            }
        }
        let mut cur = body;
        loop {
            let next = cur.elements().max_by_key(|c| get(c));
            match next {
                Some(n) if get(&n) * 10 >= total * 8 => cur = n,
                _ => break,
            }
        }
        Some(cur)
    }

    fn is_title(&self, h: Node) -> bool {
        if self.title.is_empty() {
            return h.name() == "h1";
        }
        let a = norm(&h.plain());
        let t = norm(&self.title);
        !a.is_empty() && (a == t || (a.chars().count() > 24 && t.starts_with(&a)) || (t.chars().count() > 24 && a.starts_with(&t)))
    }

    fn is_block(&mut self, n: Node) -> bool {
        if n.is_text() || n.is_math() {
            return false;
        }
        if BLOCKS.contains(&n.name()) {
            return true;
        }
        if matches!(n.name(), "img" | "br" | "a" | "code" | "span" | "em" | "strong") && n.first(|x| BLOCKS.contains(&x.name())).is_none() {
            return false;
        }
        if let Some(&b) = self.block_memo.get(&n.id) {
            return b;
        }
        let b = n.elements().any(|e| self.is_block(e));
        self.block_memo.insert(n.id, b);
        b
    }

    fn uid(&mut self, raw: &str) -> String {
        let id = clean_id(raw);
        let mut out = id.clone();
        let mut k = 2;
        while self.used_ids.contains(&out) {
            out = format!("{id}-{k}");
            k += 1;
        }
        self.used_ids.insert(out.clone());
        out
    }

    /// ` id="…"` for an element links in the page point at.
    fn kept_id(&mut self, n: Node) -> String {
        match n.attr("id").map(clean_id) {
            Some(id) if self.targets.contains(&id) && !self.used_ids.contains(&id) => {
                self.used_ids.insert(id.clone());
                format!(" id=\"{}\"", esc(&id))
            }
            _ => String::new(),
        }
    }

    fn start_chapter(&mut self, title: &str, id: &str) {
        let id = self.uid(id);
        self.chapters.push(Chapter {
            title: title.to_string(),
            html: format!("<section><h1 id=\"{}\">{}</h1>", esc(&id), esc(title)),
            headings: vec![WebHeading { level: 1, id, title: title.to_string() }],
        });
    }

    fn finish(&mut self) -> Vec<WebChapter> {
        let all = std::mem::take(&mut self.chapters);
        let n = all.len();
        all.into_iter()
            .enumerate()
            // an empty first chapter (the post starts with its first section)
            .filter(|(i, c)| !(*i == 0 && n > 1 && words(&text_of(&c.html)) <= words(&c.title) && !c.html.contains("<img")))
            .map(|(_, c)| WebChapter { title: c.title, html: c.html + "</section>", headings: c.headings })
            .collect()
    }

    /// A container's children: blocks as they are, runs of inline content as paragraphs.
    fn blocks(&mut self, n: Node) -> String {
        let mut out = String::new();
        let mut run = String::new();
        for c in n.children() {
            if self.junk(c) {
                continue;
            }
            if self.is_block(c) {
                flush(&mut out, &mut run);
                let b = self.block(c);
                out += &b;
            } else {
                run += &self.inline(c);
            }
        }
        flush(&mut out, &mut run);
        out
    }

    /// A list item's or cell's content: inline content as it is, or blocks when it has any.
    fn content(&mut self, n: Node) -> String {
        let has_blocks = n.children().any(|c| !self.junk(c) && self.is_block(c));
        if has_blocks { self.blocks(n) } else { self.inline_kids(n).trim().to_string() }
    }

    fn block(&mut self, n: Node) -> String {
        if let Some(level) = heading_level(n) {
            return self.heading(n, level);
        }
        match n.name() {
            "p" => {
                let id = self.kept_id(n);
                let t = self.inline_kids(n);
                let t = t.trim();
                if t.is_empty() { String::new() } else { format!("<p{id}>{t}</p>") }
            }
            "pre" => self.code_block(n),
            "blockquote" => {
                let inner = self.blocks(n);
                if inner.trim().is_empty() { String::new() } else { format!("<blockquote>{inner}</blockquote>") }
            }
            "ul" | "ol" | "menu" => self.list(n),
            "dl" => {
                let mut out = String::new();
                for c in n.elements() {
                    if self.junk(c) {
                        continue;
                    }
                    match c.name() {
                        "dt" => out += &format!("<dt>{}</dt>", self.inline_kids(c).trim()),
                        "dd" => out += &format!("<dd>{}</dd>", self.content(c)),
                        _ => out += &self.blocks(c),
                    }
                }
                if out.is_empty() { out } else { format!("<dl>{out}</dl>") }
            }
            "table" => self.table(n),
            "figure" => self.figure(n),
            "hr" => "<hr/>".into(),
            "details" => self.blocks(n),
            "summary" => {
                let t = self.inline_kids(n);
                if t.trim().is_empty() { String::new() } else { format!("<p><strong>{}</strong></p>", t.trim()) }
            }
            "li" => format!("<ul><li>{}</li></ul>", self.content(n)),
            "dt" => format!("<p><strong>{}</strong></p>", self.inline_kids(n).trim()),
            "dd" | "td" | "th" | "caption" | "figcaption" => self.blocks(n),
            _ => {
                // code highlighters wrap <pre> in divs (Jekyll's "highlighter-rouge"): just go in
                let id = self.kept_id(n);
                let inner = self.blocks(n);
                if id.is_empty() { inner } else { format!("<span{id}></span>{inner}") }
            }
        }
    }

    fn heading(&mut self, h: Node, level: usize) -> String {
        let text = h.plain();
        if text.is_empty() {
            return String::new();
        }
        if !self.title_seen && self.is_title(h) {
            self.title_seen = true;
            return String::new();
        }
        let inner = self.inline_kids(h);
        let inner = re!(r#"<a href="[^"]*#[^"]*">\s*(¶|#|§|🔗)?\s*</a>"#).replace_all(&inner, "").trim().to_string();
        let raw = h.attr("id").map(str::to_string).unwrap_or_else(|| slug(&text));
        // a new chapter (not from inside a list, quote or table, whose markup it would cut)
        let nested = h.has_ancestor(|a| matches!(a.name(), "li" | "blockquote" | "table" | "figure" | "dd" | "details"));
        if level <= self.top && !nested {
            let id = self.uid(&raw);
            let title = text_of(&inner).split_whitespace().collect::<Vec<_>>().join(" ");
            let title = if title.is_empty() { text.clone() } else { title };
            self.chapters.push(Chapter {
                title: title.clone(),
                html: format!("<section><h1 id=\"{}\">{}</h1>", esc(&id), inner),
                headings: vec![WebHeading { level: 1, id, title }],
            });
            return format!("{CHAPTER_MARK}{}{CHAPTER_MARK_END}", self.chapters.len() - 1);
        }
        let rank = if level <= self.top { 0 } else { self.deeper.iter().position(|l| *l == level).unwrap_or(0) };
        let out_level = (2 + rank).min(6);
        let id = self.uid(&raw);
        if out_level == 2 {
            if let Some(c) = self.chapters.last_mut() {
                c.headings.push(WebHeading { level: 2, id: id.clone(), title: text.clone() });
            }
        }
        format!("<h{out_level} id=\"{}\">{inner}</h{out_level}>", esc(&id))
    }

    fn code_block(&mut self, pre: Node) -> String {
        let mut lang = String::new();
        let mut look = vec![pre];
        look.extend(pre.first_descendant("code"));
        look.extend(pre.ancestors().into_iter().take(3));
        for x in look {
            for c in x.classes() {
                let c = c.to_lowercase();
                if let Some(l) = c.strip_prefix("language-").or_else(|| c.strip_prefix("lang-")).or_else(|| c.strip_prefix("highlight-source-")) {
                    if !l.is_empty() && l != "plaintext" && l != "text" {
                        lang = l.to_string();
                        break;
                    }
                }
            }
            if lang.is_empty() {
                if let Some(l) = x.attr("data-lang").or(x.attr("data-language")) {
                    lang = l.to_lowercase();
                }
            }
            if !lang.is_empty() {
                break;
            }
        }
        // the text as it is (line numbers in their own elements left out)
        fn text(n: Node, out: &mut String) {
            for c in n.children() {
                if c.is_text() {
                    out.push_str(c.text());
                } else if c.name() == "br" {
                    out.push('\n');
                } else if !(c.name_tokens().contains("lineno") || c.name_tokens().contains("gutter") || c.has_class("ln") || c.attr("data-line-number").is_some() && c.plain().chars().all(|x| x.is_ascii_digit())) {
                    text(c, out);
                }
            }
        }
        let mut t = String::new();
        text(pre, &mut t);
        let t = t.trim_end_matches(['\n', '\r', ' ']).trim_start_matches(['\n', '\r']);
        if t.trim().is_empty() {
            return String::new();
        }
        let class = if lang.is_empty() { String::new() } else { format!(" class=\"language-{}\"", esc(&lang)) };
        format!("<pre><code{class}>{}</code></pre>", esc(t))
    }

    fn list(&mut self, n: Node) -> String {
        let tag = if n.name() == "ol" { "ol" } else { "ul" };
        let start = if tag == "ol" { n.attr("start").filter(|s| s.chars().all(|c| c.is_ascii_digit())).map(|s| format!(" start=\"{s}\"")).unwrap_or_default() } else { String::new() };
        let mut items = String::new();
        for c in n.elements() {
            if self.junk(c) {
                continue;
            }
            if c.name() == "li" {
                let id = self.kept_id(c);
                // task lists' checkboxes are inputs (left out): their state as a mark
                let check = match c.first(|x| x.name() == "input" && x.attr("type") == Some("checkbox")) {
                    Some(x) if x.attr("checked").is_some() => "☑ ",
                    Some(_) => "☐ ",
                    None => "",
                };
                let inner = self.content(c);
                if !inner.trim().is_empty() {
                    items += &format!("<li{id}>{check}{inner}</li>");
                }
            } else if c.name() == "ul" || c.name() == "ol" {
                let inner = self.list(c);
                if !inner.is_empty() {
                    items += &format!("<li>{inner}</li>");
                }
            } else {
                let inner = self.content(c);
                if !inner.trim().is_empty() {
                    items += &format!("<li>{inner}</li>");
                }
            }
        }
        if items.is_empty() { String::new() } else { format!("<{tag}{start}>{items}</{tag}>") }
    }

    fn table(&mut self, t: Node) -> String {
        // code with line numbers set as a table (Rouge, Pygments): the code alone
        if t.first_descendant("pre").is_some() && t.all(|x| x.name() == "td").len() <= 3 {
            let pres: Vec<Node> = t.all(|x| x.name() == "pre" && !x.has_ancestor(|a| a.name_tokens().contains("gutter") || a.name_tokens().contains("lineno") || a.name_tokens().contains("linenos")));
            return pres.into_iter().map(|p| self.code_block(p)).collect();
        }
        let mut rows = String::new();
        let mut caption = String::new();
        fn row_nodes<'a>(n: Node<'a>, out: &mut Vec<Node<'a>>) {
            for c in n.elements() {
                match c.name() {
                    "tr" => out.push(c),
                    "thead" | "tbody" | "tfoot" => row_nodes(c, out),
                    _ => {}
                }
            }
        }
        if let Some(c) = t.child("caption") {
            caption = format!("<caption>{}</caption>", self.inline_kids(c).trim());
        }
        let mut list = vec![];
        row_nodes(t, &mut list);
        for tr in list {
            let mut cells = String::new();
            for c in tr.elements().filter(|c| c.name() == "td" || c.name() == "th") {
                let mut attrs = String::new();
                for a in ["colspan", "rowspan"] {
                    if let Some(v) = c.attr(a).filter(|v| v.chars().all(|x| x.is_ascii_digit()) && !v.is_empty()) {
                        attrs += &format!(" {a}=\"{v}\"");
                    }
                }
                cells += &format!("<{0}{attrs}>{1}</{0}>", c.name(), self.content(c));
            }
            if !cells.is_empty() {
                rows += &format!("<tr>{cells}</tr>");
            }
        }
        if rows.is_empty() { String::new() } else { format!("<table>{caption}{rows}</table>") }
    }

    fn figure(&mut self, f: Node) -> String {
        if f.first_descendant("pre").is_some() || f.first_descendant("table").is_some() || f.first_descendant("blockquote").is_some() {
            let caption = f.first(|x| x.name() == "figcaption").map(|c| self.inline_kids(c)).unwrap_or_default();
            let mut inner = String::new();
            for c in f.children() {
                if c.name() == "figcaption" || self.junk(c) {
                    continue;
                }
                inner += &if self.is_block(c) { self.block(c) } else { self.inline(c) };
            }
            let cap = if caption.trim().is_empty() { String::new() } else { format!("<p class=\"caption\">{}</p>", caption.trim()) };
            return inner + &cap;
        }
        let imgs: String = f.all(|x| x.name() == "img" && !x.has_ancestor(|a| a.name() == "figcaption")).into_iter().map(|i| self.image(i)).collect();
        let caption = f.first(|x| x.name() == "figcaption").map(|c| self.inline_kids(c)).unwrap_or_default();
        let math: String = f.all(|x| x.name() == "math" && !x.has_ancestor(|a| a.name() == "math")).into_iter().map(|m| mathml(m)).collect();
        if imgs.is_empty() && math.is_empty() {
            let t = caption.trim();
            return if t.is_empty() { String::new() } else { format!("<p>{t}</p>") };
        }
        let cap = if caption.trim().is_empty() { String::new() } else { format!("<figcaption>{}</figcaption>", caption.trim()) };
        format!("<figure>{imgs}{math}{cap}</figure>")
    }

    /// The best address of an image: its `srcset`'s largest, else the lazy loaders' `data-src`,
    /// else `src` (not a data: placeholder).
    fn image_url(&self, img: Node) -> Option<String> {
        let mut cands: Vec<String> = vec![];
        let srcset = |s: &str| -> Option<String> {
            let mut best: Option<(f64, String)> = None;
            for part in s.split(',') {
                let mut it = part.split_whitespace();
                let Some(u) = it.next() else { continue };
                let w = it.next().map(|d| {
                    let v: f64 = d.trim_end_matches(['w', 'x']).parse().unwrap_or(0.0);
                    if d.ends_with('x') { v * 1000.0 } else { v }
                }).unwrap_or(1.0);
                if best.as_ref().map_or(true, |(bw, _)| w > *bw) {
                    best = Some((w, u.to_string()));
                }
            }
            best.map(|b| b.1)
        };
        for a in ["data-srcset", "srcset"] {
            if let Some(u) = img.attr(a).and_then(srcset) {
                cands.push(u);
            }
        }
        if let Some(pic) = img.parent().filter(|p| p.name() == "picture") {
            for s in pic.elements().filter(|s| s.name() == "source") {
                if let Some(u) = s.attr("srcset").and_then(srcset) {
                    cands.push(u);
                }
            }
        }
        for a in ["data-src", "data-lazy-src", "data-original", "data-full-src", "src"] {
            if let Some(u) = img.attr(a) {
                cands.push(u.to_string());
            }
        }
        cands
            .into_iter()
            .map(|u| u.trim().to_string())
            .filter(|u| !u.is_empty() && !u.starts_with("data:") && !u.starts_with("blob:"))
            .map(|u| join_url(&self.base, &u))
            .find(|u| u.starts_with("http"))
    }

    fn image(&mut self, img: Node) -> String {
        // icons, spacers and tracking pixels
        let dim = |a: &str| img.attr(a).and_then(|v| v.trim_end_matches("px").parse::<u32>().ok());
        if dim("width").is_some_and(|w| w < 24) || dim("height").is_some_and(|h| h < 24) {
            return String::new();
        }
        if img.name_tokens().iter().any(|t| t == "avatar" || t == "emoji" || t == "icon" || t == "logo") {
            return String::new();
        }
        let Some(url) = self.image_url(img) else { return String::new() };
        let n = match self.image_index.get(&url) {
            Some(&n) => n,
            None => {
                self.images.push(url.clone());
                self.image_index.insert(url, self.images.len() - 1);
                self.images.len() - 1
            }
        };
        let alt = img.attr("alt").map(|a| a.split_whitespace().collect::<Vec<_>>().join(" ")).unwrap_or_default();
        format!("<img src=\"{IMAGE_SCHEME}{n}\" alt=\"{}\"/>", esc(&alt))
    }

    fn inline_kids(&mut self, n: Node) -> String {
        let mut s = String::new();
        for c in n.children() {
            s += &self.inline(c);
        }
        s
    }

    fn inline(&mut self, n: Node) -> String {
        if n.is_text() {
            return esc(&collapse(n.text()));
        }
        if n.is_math() || n.name() == "math" {
            return mathml(n);
        }
        if self.junk(n) {
            // TeX in a script (MathJax 2) is the equation's source
            if n.name() == "script" && n.attr("type").is_some_and(|t| t.starts_with("math/tex")) {
                let t = n.all_text();
                return if n.attr("type").is_some_and(|t| t.contains("display")) {
                    format!("<code class=\"tex-display\">{}</code>", esc(t.trim()))
                } else {
                    format!("<code class=\"tex\">{}</code>", esc(t.trim()))
                };
            }
            return String::new();
        }
        let kids = |me: &mut Self| me.inline_kids(n);
        match n.name() {
            "br" => "<br/>".into(),
            "img" => self.image(n),
            "a" => {
                let inner = kids(self);
                let href = n.attr("href").unwrap_or("").trim().to_string();
                if inner.trim().is_empty() {
                    return inner;
                }
                if let Some(frag) = href.strip_prefix('#') {
                    return format!("<a href=\"#{}\">{inner}</a>", esc(&clean_id(frag)));
                }
                let abs = join_url(&self.base, &href);
                if abs.starts_with("http") || abs.starts_with("mailto:") {
                    // a link to a place in this same page
                    let here = self.base.split('#').next().unwrap_or("");
                    if let Some((page, frag)) = abs.split_once('#') {
                        if page == here && !frag.is_empty() {
                            return format!("<a href=\"#{}\">{inner}</a>", esc(&clean_id(frag)));
                        }
                    }
                    format!("<a href=\"{}\">{inner}</a>", esc(&abs))
                } else {
                    inner
                }
            }
            "em" | "i" | "cite" | "dfn" | "var" => wrap("em", kids(self)),
            "strong" | "b" => wrap("strong", kids(self)),
            "code" | "kbd" | "samp" | "tt" => {
                let t = collapse(&n.all_text());
                if t.trim().is_empty() { String::new() } else { format!("<code>{}</code>", esc(&t)) }
            }
            "sub" | "sup" => {
                let id = self.kept_id(n);
                let inner = kids(self);
                if inner.is_empty() { inner } else { format!("<{0}{id}>{inner}</{0}>", n.name()) }
            }
            "s" | "del" | "strike" => wrap("del", kids(self)),
            "q" => format!("“{}”", kids(self)),
            "span" if n.has_class("katex") || n.has_class("katex-display") => {
                match n.first(|x| x.name() == "math") {
                    Some(m) => mathml(m),
                    None => kids(self),
                }
            }
            "span" | "font" | "abbr" | "u" | "ins" | "mark" | "small" | "big" | "time" | "bdi" | "bdo" | "data" | "nobr" | "wbr" => {
                let id = self.kept_id(n);
                let inner = kids(self);
                if id.is_empty() { inner } else { format!("<span{id}></span>{inner}") }
            }
            "picture" => n.elements().find(|c| c.name() == "img").map(|i| self.image(i)).unwrap_or_default(),
            _ => kids(self),
        }
    }
}

/// "1) Neural net training" → "1-neural-net-training"
fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.to_lowercase().chars() {
        if c.is_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out: String = out.trim_end_matches('-').chars().take(60).collect();
    if out.is_empty() { "section".into() } else { out }
}

fn wrap(tag: &str, inner: String) -> String {
    if inner.trim().is_empty() {
        inner
    } else {
        format!("<{tag}>{inner}</{tag}>")
    }
}

fn collapse(t: &str) -> String {
    let mut out = String::with_capacity(t.len());
    let mut space = false;
    for c in t.chars() {
        if c.is_whitespace() && c != '\u{a0}' {
            if !space {
                out.push(' ');
            }
            space = true;
        } else {
            out.push(c);
            space = false;
        }
    }
    out
}

fn flush(out: &mut String, run: &mut String) {
    let t = run.trim();
    // a run that is only links' anchors or stray punctuation is not a paragraph
    if !t.is_empty() && (t.contains("<img") || t.contains("<math") || words(&text_of(t)) > 0) {
        *out += &format!("<p>{t}</p>");
    }
    run.clear();
}

const MATHML_NS: &str = "http://www.w3.org/1998/Math/MathML";

/// MathML written back out (with its namespace).
fn mathml(m: Node) -> String {
    fn node(n: Node, out: &mut String, top: bool) {
        if n.is_text() {
            out.push_str(&esc(n.text()));
            return;
        }
        if n.name() == "annotation-xml" {
            return;
        }
        out.push('<');
        out.push_str(n.name());
        if top {
            out.push_str(&format!(" xmlns=\"{MATHML_NS}\""));
        }
        for (k, v) in n.attrs() {
            if k == "xmlns" || k.starts_with("xmlns:") || k == "class" || k == "style" || k.starts_with("data-") {
                continue;
            }
            out.push_str(&format!(" {}=\"{}\"", k, esc(v)));
        }
        if !n.has_children() {
            out.push_str("/>");
            return;
        }
        out.push('>');
        for c in n.children() {
            node(c, out, false);
        }
        out.push_str(&format!("</{}>", n.name()));
    }
    let mut s = String::new();
    node(m, &mut s, true);
    s
}

#[cfg(test)]
mod tests;
