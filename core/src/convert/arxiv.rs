//! arXiv papers: the HTML version arXiv renders from the LaTeX source (LaTeXML; ar5iv for older
//! papers) has real MathML, figures and tables, so it makes a much better reading view than the
//! PDF. A port of `ArxivHTML.swift`.

use super::net;
use super::resolver::{cand, Candidate, Source};
use super::xml::{self, esc, Doc, Node};
use super::{fail, trim_ws, BookImage, Chapter, Heading, PaperMeta, Result};
use regex::Regex;
use std::collections::HashMap;
use std::sync::OnceLock;
use url::Url;

/// Candidates for an arXiv id, best first: arXiv HTML, ar5iv HTML, then the PDF.
pub fn candidates(id: &str) -> Vec<Candidate> {
    let (a, b, c) = (id.to_string(), id.to_string(), id.to_string());
    vec![
        cand(format!("arXiv HTML {id}"), move |o| {
            source(&a, &format!("https://arxiv.org/html/{a}"), "https://arxiv.org/html/", "arXiv HTML", o)
        }),
        cand(format!("ar5iv {id}"), move |o| {
            source(
                &b,
                &format!("https://ar5iv.labs.arxiv.org/html/{b}"),
                &format!("https://ar5iv.labs.arxiv.org/html/{b}/"),
                "ar5iv",
                o,
            )
        }),
        cand(format!("arXiv PDF {id}"), move |o| {
            o.log("Downloading the arXiv PDF");
            let data = net::get(&format!("https://arxiv.org/pdf/{c}"), Some("application/pdf"), 60.0)?;
            let meta = metadata(&c).unwrap_or_default();
            Ok(Source::Pdf { data, meta, label: "arXiv PDF".into() })
        }),
    ]
}

pub fn source(id: &str, page: &str, base: &str, label: &str, o: &super::Options) -> Result<Source> {
    o.log(&format!("Fetching the {label} version"));
    let r = net::fetch(page, Some("text/html"), 60.0)?;
    let html = match std::str::from_utf8(&r.data) {
        Ok(h) if (200..300).contains(&r.status) => h.to_string(),
        _ => return Err(fail(format!("no {label} version (HTTP {})", r.status))),
    };
    if !html.contains("ltx_document") {
        return Err(fail(format!("no {label} version of this paper")));
    }
    let doc = parse(&html)?;
    let mut meta = metadata(id).unwrap_or_default();
    if meta.title.is_empty() {
        if let Some(h) = doc.root().descendants("h1").into_iter().find(|h| h.has_class("ltx_title_document")) {
            meta.title = h.plain();
        }
    }
    let base = Url::parse(base).map_err(|e| fail(e.to_string()))?;
    Ok(Source::Html { doc, base, meta, label: label.into() })
}

/// LaTeXML's HTML5 → a DOM: the <article> only, void elements closed, scripts and stray
/// ampersands removed, then the XML parser.
pub fn parse(html: &str) -> Result<Doc> {
    let mut s = html.to_string();
    if let (Some(a), Some(b)) = (s.find("<article"), s.rfind("</article>")) {
        if a <= b {
            s = s[a..b + "</article>".len()].to_string();
        }
    }
    static FIXES: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    let fixes = FIXES.get_or_init(|| {
        vec![
            (Regex::new(r"<!--[\s\S]*?-->").unwrap(), ""),
            (Regex::new(r"<script\b[\s\S]*?</script>").unwrap(), ""),
            (Regex::new(r"<style\b[\s\S]*?</style>").unwrap(), ""),
            (Regex::new(r"<button\b[\s\S]*?</button>").unwrap(), ""),
            (
                Regex::new(
                    r#"<(img|br|hr|meta|link|input|col|source|wbr|area|embed|param|track)\b((?:"[^"]*"|'[^']*'|[^'">])*?)\s*/?>"#,
                )
                .unwrap(),
                "<$1$2/>",
            ),
        ]
    });
    for (re, rep) in fixes {
        s = re.replace_all(&s, *rep).into_owned();
    }
    s = escape_stray_amps(&s);
    xml::parse(s.as_bytes())
}

/// `&(?!#?[A-Za-z0-9]+;)` → `&amp;` (the regex crate has no look-ahead).
fn escape_stray_amps(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut last = 0;
    for (i, _) in s.match_indices('&') {
        let mut j = i + 1;
        if j < b.len() && b[j] == b'#' {
            j += 1;
        }
        let start = j;
        while j < b.len() && b[j].is_ascii_alphanumeric() {
            j += 1;
        }
        let is_ref = j > start && j < b.len() && b[j] == b';';
        if !is_ref {
            out.push_str(&s[last..i]);
            out.push_str("&amp;");
            last = i + 1;
        }
    }
    out.push_str(&s[last..]);
    out
}

/// Title, authors, date, abstract and DOI from the arXiv API (Atom).
pub fn metadata(id: &str) -> Result<PaperMeta> {
    let url = format!("https://export.arxiv.org/api/query?id_list={id}");
    let doc = xml::parse(&net::get(&url, None, 20.0)?)?;
    let Some(e) = doc.root().child("entry") else { return Ok(PaperMeta::default()) };
    let bare = Regex::new(r"v\d+$").unwrap().replace(id, "").into_owned();
    Ok(PaperMeta {
        title: e.child("title").map(|t| t.plain()).unwrap_or_default(),
        authors: e.children_named("author").filter_map(|a| a.child("name").map(|n| n.plain())).collect(),
        date: e.child("published").map(|t| t.plain()).unwrap_or_default().chars().take(10).collect(),
        abstract_text: e.child("summary").map(|t| t.plain()).unwrap_or_default(),
        doi: e.child("doi").map(|t| t.plain()).unwrap_or_else(|| format!("10.48550/arXiv.{bare}")),
        journal: e.child("journal_ref").map(|t| t.plain()).unwrap_or_else(|| "arXiv".into()),
        url: format!("https://arxiv.org/abs/{bare}"),
        license: "arXiv".into(),
        ..Default::default()
    })
}

const PASSTHROUGH: &[&str] = &[
    "p", "div", "span", "em", "i", "b", "strong", "sup", "sub", "ul", "ol", "li", "dl", "dt", "dd", "table",
    "thead", "tbody", "tfoot", "tr", "td", "th", "caption", "figcaption", "blockquote", "pre", "code", "cite",
    "small", "section", "figure", "u", "s", "tt", "br", "hr",
];
pub const MATH_NS: &str = "http://www.w3.org/1998/Math/MathML";

/// Turns a LaTeXML article into chapters (one per top-level section).
pub struct ArxivConverter<'a> {
    root: Node<'a>,
    base: Url,
    /// by absolute source URL
    images: HashMap<String, BookImage>,
    headings: Vec<Heading>,
    pub missing_figures: usize,
    auto_id: usize,
}

impl<'a> ArxivConverter<'a> {
    pub fn new(doc: &'a Doc, base: Url) -> Self {
        ArxivConverter { root: doc.root(), base, images: HashMap::new(), headings: vec![], missing_figures: 0, auto_id: 0 }
    }

    fn resolve(&self, src: &str) -> Option<Url> {
        self.base.join(src).ok()
    }

    /// Every figure image referenced (absolute URLs), in order.
    pub fn image_urls(&self) -> Vec<Url> {
        let mut out: Vec<Url> = Vec::new();
        fn rec(c: &ArxivConverter, n: Node, out: &mut Vec<Url>) {
            for ch in n.elements() {
                let src = match ch.name() {
                    "img" => ch.attr("src"),
                    "object" => ch.attr("data"),
                    _ => None,
                };
                if let Some(u) = src.and_then(|s| c.resolve(s)) {
                    if !out.contains(&u) {
                        out.push(u);
                    }
                }
                rec(c, ch, out);
            }
        }
        rec(self, self.root, &mut out);
        out
    }

    pub fn set_images(&mut self, list: &[(Url, BookImage)]) {
        for (u, img) in list {
            self.images.insert(u.to_string(), img.clone());
        }
    }

    pub fn chapters(&mut self, meta: &PaperMeta) -> Vec<Chapter> {
        let mut out: Vec<Chapter> = Vec::new();
        let mut pending = String::new();
        let article =
            if self.root.name() == "article" { self.root } else { self.root.first_descendant("article").unwrap_or(self.root) };
        let abstract_div = article.descendants("div").into_iter().find(|d| d.has_class("ltx_abstract"));
        if let Some(ab) = abstract_div {
            self.headings = vec![Heading { level: 1, id: "abstract".into(), title: "Abstract".into() }];
            let inner: String = ab
                .children()
                .filter(|c| !(c.name().starts_with('h') && c.has_class("ltx_title")))
                .map(|c| self.render(c, 1))
                .collect();
            out.push(Chapter {
                title: "Abstract".into(),
                html: format!("<section><h1 id=\"abstract\">Abstract</h1>{inner}</section>"),
                headings: std::mem::take(&mut self.headings),
            });
        } else if !meta.abstract_text.is_empty() {
            out.push(Chapter {
                title: "Abstract".into(),
                html: format!(
                    "<section><h1 id=\"abstract\">Abstract</h1><p>{}</p></section>",
                    esc(&meta.abstract_text)
                ),
                headings: vec![Heading { level: 1, id: "abstract".into(), title: "Abstract".into() }],
            });
        }
        static TAG: OnceLock<Regex> = OnceLock::new();
        let tag = TAG.get_or_init(|| Regex::new("<[^>]+>").unwrap());
        for c in article.children() {
            if c.is_text() {
                continue;
            }
            if c.name() == "section" {
                let text = tag.replace_all(&pending, "").trim().to_string();
                if text.chars().count() > 500 {
                    out.push(Chapter {
                        title: "Main text".into(),
                        html: format!("<section><h1 id=\"main\">Main text</h1>{pending}</section>"),
                        headings: vec![Heading { level: 1, id: "main".into(), title: "Main text".into() }],
                    });
                } else if !text.is_empty() {
                    if let Some(last) = out.last_mut() {
                        // a short note before the first section (e.g. a licence line): tuck it under the abstract
                        last.html = last
                            .html
                            .replace("</section>", &format!("<p class=\"note\"><small>{text}</small></p></section>"));
                    }
                }
                pending.clear();
                self.headings.clear();
                let html = self.render(c, 0);
                let title = self.headings.first().map(|h| h.title.clone()).unwrap_or_else(|| "Section".into());
                if self.headings.is_empty() {
                    let id = c.attr("id").map(String::from).unwrap_or_else(|| self.next_id());
                    self.headings = vec![Heading { level: 1, id, title: title.clone() }];
                }
                out.push(Chapter { title, html, headings: std::mem::take(&mut self.headings) });
            } else if skip_block(c) || (c.name() == "div" && c.has_class("ltx_abstract")) || c.name().starts_with('h') {
                continue;
            } else {
                pending += &self.render(c, 1);
            }
        }
        out
    }

    fn next_id(&mut self) -> String {
        self.auto_id += 1;
        format!("ax{}", self.auto_id)
    }

    /// XHTML for a node. `depth` is the section depth (0 = chapter level).
    pub fn render(&mut self, n: Node<'a>, depth: usize) -> String {
        if n.is_text() {
            return esc(n.text());
        }
        if skip_block(n) {
            return String::new();
        }
        if n.name() == "math" {
            return mathml(n);
        }
        if n.name() == "svg" {
            let mut s = String::new();
            svg(n, true, &mut s);
            return s;
        }
        // footnotes: the mark becomes a superscript, the text follows in small type
        if n.has_class("ltx_note") {
            let Some(content) = n.descendants("span").into_iter().find(|s| s.has_class("ltx_note_content")) else {
                return String::new();
            };
            let text: String = content
                .children()
                .filter(|c| !c.has_class("ltx_note_mark") && !c.has_class("ltx_tag"))
                .map(|c| self.render(c, depth))
                .collect();
            return format!(" <small class=\"fn\">({})</small>", trim_ws(&text));
        }
        match n.name() {
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let lvl = depth.clamp(1, 4);
                let id = n
                    .parent()
                    .and_then(|p| p.attr("id"))
                    .or_else(|| n.attr("id"))
                    .map(String::from)
                    .unwrap_or_else(|| self.next_id());
                let title = n.plain();
                if lvl <= 2 {
                    self.headings.push(Heading { level: lvl as u32, id: id.clone(), title });
                }
                format!("<h{lvl} id=\"{}\">{}</h{lvl}>", esc(&id), self.inline(n, depth))
            }
            "section" => {
                let inner: String = n.children().map(|c| self.render(c, depth + 1)).collect();
                format!("<section>{inner}</section>")
            }
            "img" | "object" => {
                let src = if n.name() == "img" { n.attr("src") } else { n.attr("data") };
                let img = src.and_then(|s| self.resolve(s)).and_then(|u| self.images.get(u.as_str()));
                match img {
                    Some(img) => format!(
                        "<img src=\"../images/{}\" alt=\"{}\"/>",
                        img.file_name,
                        esc(n.attr("alt").unwrap_or("Figure"))
                    ),
                    None => {
                        self.missing_figures += 1;
                        "<span class=\"missing\">[Figure image not available]</span>".into()
                    }
                }
            }
            "a" => {
                let inner = self.inline(n, depth);
                match n.attr("href") {
                    Some(h) if h.starts_with('#') || h.starts_with("http") => {
                        format!("<a href=\"{}\">{inner}</a>", esc(h))
                    }
                    _ => inner,
                }
            }
            "br" => "<br/>".into(),
            "hr" => "<hr/>".into(),
            name => {
                let inner = self.inline(n, depth);
                if !PASSTHROUGH.contains(&name) {
                    return inner;
                }
                let mut attrs = String::new();
                if let Some(id) = n.attr("id") {
                    if ["figure", "table", "section", "li", "div", "span", "p"].contains(&name) {
                        attrs += &format!(" id=\"{}\"", esc(id));
                    }
                }
                for k in ["colspan", "rowspan"] {
                    if let Some(v) = n.attr(k) {
                        attrs += &format!(" {k}=\"{}\"", esc(v));
                    }
                }
                if n.has_class("ltx_tag") {
                    attrs += " class=\"label\"";
                }
                if name == "div" && (n.has_class("ltx_equation") || n.has_class("ltx_equationgroup")) {
                    attrs += " class=\"equation\"";
                }
                if name == "ul" && n.has_class("ltx_biblist") {
                    return format!("<ol class=\"refs\"{attrs}>{inner}</ol>");
                }
                if name == "span" && attrs.is_empty() && !n.has_class("ltx_font_italic") && !n.has_class("ltx_font_bold") {
                    return inner;
                }
                if name == "span" && n.has_class("ltx_font_italic") {
                    return format!("<em>{inner}</em>");
                }
                if name == "span" && n.has_class("ltx_font_bold") {
                    return format!("<strong>{inner}</strong>");
                }
                format!("<{name}{attrs}>{inner}</{name}>")
            }
        }
    }

    fn inline(&mut self, n: Node<'a>, depth: usize) -> String {
        n.children().map(|c| self.render(c, depth)).collect()
    }
}

fn skip_block(n: Node) -> bool {
    [
        "ltx_authors", "ltx_dates", "ltx_page_footer", "ltx_page_header", "ltx_pagination", "ltx_keywords_hidden",
        "ltx_title_document", "ltx_role_affiliationtext", "ltx_TOC",
    ]
    .iter()
    .any(|c| n.has_class(c))
        || ["nav", "footer", "header", "form", "input", "script", "style", "button"].contains(&n.name())
}

fn mathml(m: Node) -> String {
    let mut s = format!("<math xmlns=\"{MATH_NS}\"");
    if let Some(d) = m.attr("display") {
        s += &format!(" display=\"{}\"", esc(d));
    }
    if let Some(a) = m.attr("alttext") {
        s += &format!(" alttext=\"{}\"", esc(a));
    }
    s += ">";
    for c in m.children() {
        serialize(c, &mut s);
    }
    s + "</math>"
}

fn serialize(n: Node, s: &mut String) {
    if n.is_text() {
        s.push_str(&esc(n.text()));
        return;
    }
    // annotations repeat the TeX source; the reader doesn't need them
    if n.name() == "annotation" || n.name() == "annotation-xml" {
        return;
    }
    s.push('<');
    s.push_str(n.name());
    for (k, v) in n.attrs() {
        if ["id", "xref", "class", "intent", "arg"].contains(&k.as_str()) {
            continue;
        }
        if k == "displaystyle" && !["mstyle", "mtable", "math"].contains(&n.name()) {
            continue;
        }
        s.push_str(&format!(" {k}=\"{}\"", esc(v)));
    }
    if !n.has_children() {
        s.push_str("/>");
        return;
    }
    s.push('>');
    for c in n.children() {
        serialize(c, s);
    }
    s.push_str(&format!("</{}>", n.name()));
}

fn svg(x: Node, top: bool, s: &mut String) {
    if x.is_text() {
        s.push_str(&esc(x.text()));
        return;
    }
    s.push('<');
    s.push_str(x.name());
    if top {
        s.push_str(" xmlns=\"http://www.w3.org/2000/svg\"");
    }
    for (k, v) in x.attrs() {
        if k == "xmlns" {
            continue;
        }
        s.push_str(&format!(" {k}=\"{}\"", esc(v)));
    }
    if !x.has_children() {
        s.push_str("/>");
        return;
    }
    s.push('>');
    for c in x.children() {
        svg(c, false, s);
    }
    s.push_str(&format!("</{}>", x.name()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stray_ampersands() {
        assert_eq!(escape_stray_amps("a & b &amp; &#38; &x"), "a &amp; b &amp; &#38; &amp;x");
    }
}
