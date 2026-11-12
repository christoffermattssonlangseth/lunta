//! JATS XML → XHTML chapters, one per top-level section. A port of `JATS.swift`
//! (itself a port of `JatsConverter` in converter/paper2epub.py).

use super::xml::{esc, Node};
use super::{capitalized, trim_ws, BookImage, Chapter, Heading, PaperMeta};
use std::collections::{HashMap, HashSet};

const BLOCK_TAGS: &[&str] = &[
    "fig", "fig-group", "table-wrap", "table-wrap-group", "disp-formula", "disp-formula-group", "list",
    "boxed-text", "disp-quote", "preformat", "code", "def-list", "supplementary-material", "statement",
    "graphic", "media", "array", "speech", "verse-group", "p", "sec", "chem-struct-wrap",
];
pub const MATH_NS: &str = "http://www.w3.org/1998/Math/MathML";

/// Every figure/graphic reference that needs downloading.
pub fn graphic_hrefs(root: Node) -> Vec<String> {
    let mut out = Vec::new();
    for part in root.children().filter(|c| c.name() != "sub-article" && c.name() != "response") {
        let mut gs = part.descendants("graphic");
        gs.extend(part.descendants("inline-graphic"));
        for g in gs {
            if g.attr("content-type") == Some("thumb") {
                continue;
            }
            if let Some(h) = g.attr("href") {
                out.push(h.to_string());
            }
        }
    }
    out
}

fn id_attr(el: Node) -> String {
    el.attr("id").map(|v| format!(" id=\"{}\"", esc(v))).unwrap_or_default()
}

fn label_of(el: Node) -> String {
    el.child_text("label").unwrap_or_default().trim().to_string()
}

pub struct JatsConverter<'a> {
    root: Node<'a>,
    images: &'a HashMap<String, BookImage>,
    pub missing_figures: usize,
    pub tex_formulas: usize,
    headings: Vec<Heading>,
    auto_id: usize,
    /// Figures/tables kept in <floats-group> (NIH manuscripts): placed after their first citation.
    floats: HashMap<String, Node<'a>>,
    float_order: Vec<String>,
    placed: HashSet<String>,
}

impl<'a> JatsConverter<'a> {
    pub fn new(root: Node<'a>, images: &'a HashMap<String, BookImage>) -> Self {
        let mut floats = HashMap::new();
        let mut float_order = Vec::new();
        for fg in root.children().filter(|c| c.name() == "floats-group") {
            for f in fg.elements() {
                if ["fig", "table-wrap", "fig-group", "table-wrap-group", "boxed-text"].contains(&f.name()) {
                    if let Some(id) = f.attr("id") {
                        floats.insert(id.to_string(), f);
                        float_order.push(id.to_string());
                    }
                }
            }
        }
        JatsConverter {
            root,
            images,
            missing_figures: 0,
            tex_formulas: 0,
            headings: vec![],
            auto_id: 0,
            floats,
            float_order,
            placed: HashSet::new(),
        }
    }

    /// Floating figures cited by `el` that have not been placed yet.
    fn cited_floats(&mut self, el: Node<'a>) -> String {
        if self.floats.is_empty() {
            return String::new();
        }
        let mut out = String::new();
        for x in el.descendants("xref") {
            for rid in x.attr("rid").unwrap_or("").split(' ') {
                if let Some(&f) = self.floats.get(rid) {
                    if self.placed.insert(rid.to_string()) {
                        out += &self.block(f, 3);
                    }
                }
            }
        }
        out
    }

    fn article_meta(&self) -> Option<Node<'a>> {
        self.root
            .child("front")
            .and_then(|f| f.child("article-meta"))
            .or_else(|| self.root.first_descendant("article-meta"))
    }

    // MARK: - Metadata

    pub fn meta(&mut self) -> PaperMeta {
        let mut m = PaperMeta::default();
        let Some(am) = self.article_meta() else { return m };
        m.title = am.child("title-group").and_then(|t| t.child("article-title")).map(|t| t.plain()).unwrap_or_default();
        for cg in am.descendants("contrib-group") {
            for c in cg.children_named("contrib") {
                if c.attr("contrib-type").unwrap_or("author") != "author" {
                    continue;
                }
                if let Some(n) = c.child("name") {
                    let given = n.child_text("given-names").unwrap_or_default().trim().to_string();
                    let sur = n.child_text("surname").unwrap_or_default().trim().to_string();
                    let full = trim_ws(&format!("{given} {sur}")).to_string();
                    if !full.is_empty() {
                        m.authors.push(full);
                    }
                } else if let Some(col) = c.child("collab") {
                    m.authors.push(col.plain());
                }
            }
        }
        for aid in am.children_named("article-id") {
            if aid.attr("pub-id-type") == Some("doi") {
                m.doi = aid.all_text().trim().to_string();
            }
        }
        m.journal = self
            .root
            .first_descendant("journal-meta")
            .and_then(|j| j.first_descendant("journal-title"))
            .map(|j| j.plain())
            .unwrap_or_default();
        let dates: Vec<Node> = am.children_named("pub-date").collect();
        let pick = dates
            .iter()
            .find(|d| {
                ["epub", "ppub", "pub"].contains(&d.attr("pub-type").unwrap_or(""))
                    || ["pub", "publication"].contains(&d.attr("date-type").unwrap_or(""))
            })
            .or(dates.first());
        if let Some(d) = pick {
            let part = |n: &str| d.child_text(n).unwrap_or_default().trim().to_string();
            let (y, mo, da) = (part("year"), part("month"), part("day"));
            let mut s = y.clone();
            if !s.is_empty() && !mo.is_empty() {
                s += &format!("-{}", pad2(&mo));
                if !da.is_empty() {
                    s += &format!("-{}", pad2(&da));
                }
            }
            m.date = s;
        }
        m.keywords = am
            .descendants("kwd-group")
            .iter()
            .flat_map(|g| g.children_named("kwd").collect::<Vec<_>>())
            .map(|k| k.plain())
            .take(15)
            .collect();
        if let Some(lic) = am.child("permissions").and_then(|p| p.child("license")) {
            // prefer the machine-readable licence URL (<ali:license_ref>) over the full licence prose
            let r = lic.first_descendant("license_ref").map(|r| r.plain()).filter(|r| !r.is_empty());
            m.license = r
                .or_else(|| lic.attr("href").map(String::from))
                .unwrap_or_else(|| lic.plain().chars().take(300).collect());
        }
        if let Some(ab) = am.children_named("abstract").find(|a| a.attr("abstract-type").is_none()) {
            m.abstract_text = ab.plain();
        }
        let affs = am.descendants("aff");
        if !affs.is_empty() {
            m.affiliations_html = affs.iter().map(|a| self.inline_skip(*a, &["label"])).collect::<Vec<_>>().join("<br/>");
        }
        m
    }

    // MARK: - Chapters

    pub fn chapters(&mut self) -> Vec<Chapter> {
        let mut out: Vec<Chapter> = Vec::new();
        if let Some(am) = self.article_meta() {
            let mut abs_n = 0;
            for ab in am.children_named("abstract") {
                let typ = ab.attr("abstract-type").unwrap_or("");
                if ["toc", "graphical", "teaser", "web-summary", "short"].contains(&typ) {
                    continue;
                }
                let mut title = ab.child("title").map(|t| t.plain()).unwrap_or_default();
                if title.is_empty() {
                    title = if typ.is_empty() {
                        "Abstract".into()
                    } else if typ == "executive-summary" {
                        "Summary".into()
                    } else {
                        capitalized(&typ.replace('-', " "))
                    };
                }
                abs_n += 1;
                let id = if abs_n == 1 { "abstract".to_string() } else { format!("abstract-{abs_n}") };
                let ch = self.chapter(&title, &id, |s| s.blocks(ab, 2, &["title"]));
                out.push(ch);
            }
        }
        if let Some(body) = self.root.child("body") {
            let mut pending = String::new();
            for ch in body.children() {
                if !ch.is_text() && ch.name() == "sec" {
                    if !pending.is_empty() {
                        self.append_or_create(&mut out, &pending, "Main text");
                        pending.clear();
                    }
                    let c = self.sec_chapter(ch, "Section");
                    out.push(c);
                } else if ch.is_text() {
                    let t = ch.text().trim();
                    if !t.is_empty() {
                        pending += &format!("<p>{}</p>", esc(t));
                    }
                } else {
                    pending += &self.block(ch, 2);
                }
            }
            if !pending.is_empty() {
                self.append_or_create(&mut out, &pending, "Main text");
            }
        }
        let unplaced: Vec<String> = self.float_order.iter().filter(|f| !self.placed.contains(*f)).cloned().collect();
        if !unplaced.is_empty() {
            let id = self.next_id();
            let ch = self.chapter("Figures and tables", &id, |s| {
                unplaced
                    .iter()
                    .map(|f| {
                        s.placed.insert(f.clone());
                        let n = s.floats[f];
                        s.block(n, 2)
                    })
                    .collect()
            });
            out.push(ch);
        }
        if let Some(back) = self.root.child("back") {
            let b = self.back_chapters(back);
            out.extend(b);
        }
        out
    }

    /// Body content that is not inside a <sec>: becomes its own "Main text" chapter.
    fn append_or_create(&mut self, out: &mut Vec<Chapter>, html: &str, title: &str) {
        let id = self.next_id();
        out.push(Chapter {
            title: title.into(),
            html: format!("<section><h1 id=\"{id}\">{}</h1>{html}</section>", esc(title)),
            headings: vec![Heading { level: 1, id, title: title.into() }],
        });
    }

    fn chapter(&mut self, title: &str, id: &str, content: impl FnOnce(&mut Self) -> String) -> Chapter {
        self.headings = vec![Heading { level: 1, id: id.into(), title: title.into() }];
        let inner = content(self);
        let html = format!("<section><h1 id=\"{}\">{}</h1>{inner}</section>", esc(id), esc(title));
        Chapter { title: title.into(), html, headings: std::mem::take(&mut self.headings) }
    }

    fn sec_chapter(&mut self, el: Node<'a>, fallback: &str) -> Chapter {
        self.headings.clear();
        let mut html = self.sec(el, 1);
        if self.headings.is_empty() {
            // untitled top-level section: give it a heading so the TOC can reach it
            let id = self.next_id();
            html = format!("<h1 id=\"{id}\">{}</h1>{html}", esc(fallback));
            self.headings = vec![Heading { level: 1, id, title: fallback.into() }];
        }
        let headings = std::mem::take(&mut self.headings);
        Chapter { title: headings[0].title.clone(), html, headings }
    }

    fn back_chapters(&mut self, back: Node<'a>) -> Vec<Chapter> {
        let mut out = Vec::new();
        for ch in back.elements() {
            match ch.name() {
                "ack" => {
                    let t = ch.child("title").map(|t| t.plain()).unwrap_or_else(|| "Acknowledgements".into());
                    let id = ch.attr("id").map(String::from).unwrap_or_else(|| self.next_id());
                    let c = self.chapter(&t, &id, |s| s.blocks(ch, 2, &["title"]));
                    out.push(c);
                }
                "sec" | "notes" | "glossary" => {
                    let c = self.sec_chapter(ch, "Notes");
                    out.push(c);
                }
                "app-group" => {
                    for app in ch.elements().filter(|a| a.name() == "app" || a.name() == "sec") {
                        let c = self.sec_chapter(app, "Appendix");
                        out.push(c);
                    }
                }
                "fn-group" => {
                    let t = ch.child("title").map(|t| t.plain()).unwrap_or_else(|| "Notes".into());
                    let id = self.next_id();
                    let c = self.chapter(&t, &id, |s| {
                        let items: String = ch
                            .children_named("fn")
                            .map(|f| format!("<li{}>{}</li>", id_attr(f), s.blocks(f, 3, &["label"])))
                            .collect();
                        format!("<ol class=\"notes\">{items}</ol>")
                    });
                    out.push(c);
                }
                _ => {}
            }
        }
        // reference lists that are not already rendered inside a back-matter section
        for rl in back.descendants("ref-list") {
            if has_section_ancestor(rl, back) {
                continue;
            }
            if rl.parent().map(|p| p.name() == "ref-list").unwrap_or(false) {
                continue;
            }
            let t = rl.child("title").map(|t| t.plain()).unwrap_or_else(|| "References".into());
            let id = rl.attr("id").unwrap_or("references").to_string();
            let c = self.chapter(&t, &id, |s| s.ref_items(rl));
            out.push(c);
        }
        out
    }

    fn next_id(&mut self) -> String {
        self.auto_id += 1;
        format!("p2e-s{}", self.auto_id)
    }

    // MARK: - Blocks

    fn sec(&mut self, el: Node<'a>, level: usize) -> String {
        let title_el = el.child("title");
        let label = label_of(el);
        let mut title_html = title_el.map(|t| self.inline(t)).unwrap_or_default();
        let mut plain_title = title_el.map(|t| t.plain()).unwrap_or_default();
        if !label.is_empty() {
            title_html = format!("{} {title_html}", esc(&label));
            plain_title = format!("{label} {plain_title}");
        }
        let h = level.min(6);
        let (inner, mark) = self.blocks_mark(el, level + 1, &["title", "label"]);
        let trimmed = title_html.trim();
        if trimmed.is_empty() {
            return format!("<section{}>{inner}</section>", id_attr(el));
        }
        let id = el.attr("id").map(String::from).unwrap_or_else(|| self.next_id());
        if level <= 2 {
            // headings are recorded in document order: this section before its children
            self.headings.insert(
                mark,
                Heading { level: level as u32, id: id.clone(), title: trim_ws(&plain_title).to_string() },
            );
        }
        format!("<section><h{h} id=\"{}\">{trimmed}</h{h}>{inner}</section>", esc(&id))
    }

    fn blocks(&mut self, el: Node<'a>, level: usize, skip: &[&str]) -> String {
        self.blocks_mark(el, level, skip).0
    }

    /// The mark is the index in `headings` before the children were rendered, so a parent
    /// section can insert its own heading in front of its subsections' headings.
    fn blocks_mark(&mut self, el: Node<'a>, level: usize, skip: &[&str]) -> (String, usize) {
        let mark = self.headings.len();
        let mut out = String::new();
        for ch in el.children() {
            if ch.is_text() {
                let t = ch.text().trim();
                if !t.is_empty() {
                    out += &format!("<p>{}</p>", esc(t));
                }
            } else if !skip.contains(&ch.name()) {
                out += &self.block(ch, level);
            }
        }
        (out, mark)
    }

    fn block(&mut self, ch: Node<'a>, level: usize) -> String {
        if ch.is_math() && ch.name() == "math" {
            return format!("<p>{}</p>", self.mathml(ch, true));
        }
        match ch.name() {
            "sec" => self.sec(ch, level),
            "p" => self.para(ch),
            "fig" => self.fig(ch),
            "fig-group" | "table-wrap-group" | "disp-formula-group" | "boxed-text" | "statement" | "chem-struct-wrap"
            | "glossary" => {
                let title = ch.path("caption/title").or_else(|| ch.child("title"));
                let label = label_of(ch);
                let boxy = ["boxed-text", "statement"].contains(&ch.name());
                let mut head = String::new();
                if boxy && (title.is_some() || !label.is_empty()) {
                    let t = title.map(|t| self.inline(t)).unwrap_or_default();
                    head = format!("<p><strong>{} {t}</strong></p>", esc(&label));
                }
                let cls = if boxy { "box" } else { ch.name() };
                let inner = self.blocks(ch, level, &["label", "caption", "title"]);
                format!("<div class=\"{cls}\">{head}{inner}</div>")
            }
            "table-wrap" => self.table_wrap(ch),
            "disp-formula" => self.formula(ch, true),
            "list" => self.list(ch),
            "disp-quote" | "verse-group" => format!("<blockquote>{}</blockquote>", self.blocks(ch, level, &[])),
            "preformat" | "code" => format!("<pre><code>{}</code></pre>", esc(&ch.all_text())),
            "def-list" => {
                let mut items = String::new();
                for di in ch.children_named("def-item") {
                    let term = di.child("term").map(|t| self.inline(t)).unwrap_or_default();
                    let def = di.child("def").map(|d| self.blocks(d, level, &[])).unwrap_or_default();
                    items += &format!("<dt>{term}</dt><dd>{def}</dd>");
                }
                if items.is_empty() {
                    String::new()
                } else {
                    format!("<dl>{items}</dl>")
                }
            }
            "supplementary-material" => self.supp(ch),
            "graphic" => {
                if ch.attr("content-type") == Some("thumb") {
                    return String::new();
                }
                match self.image(ch.attr("href")) {
                    Some(src) => format!("<figure><img src=\"{}\" alt=\"\"/></figure>", esc(&src)),
                    None => String::new(),
                }
            }
            "alternatives" => self.blocks(ch, level, &[]),
            "media" | "title" | "label" | "object-id" | "permissions" | "attrib" => String::new(),
            "ref-list" => {
                let t = ch.child("title").map(|t| t.plain()).unwrap_or_else(|| "References".into());
                let h = level.min(6);
                format!("<section class=\"references\"><h{h}>{}</h{h}>{}</section>", esc(&t), self.ref_items(ch))
            }
            "fn-group" | "fn" => format!("<div class=\"notes\">{}</div>", self.blocks(ch, level, &["label"])),
            _ => {
                let inner = self.inline(ch);
                if inner.trim().is_empty() {
                    String::new()
                } else {
                    format!("<p>{inner}</p>")
                }
            }
        }
    }

    fn para(&mut self, el: Node<'a>) -> String {
        let mut out = String::new();
        let mut buf = String::new();
        fn flush(out: &mut String, buf: &mut String) {
            let s = buf.trim();
            if !s.is_empty() {
                *out += &format!("<p>{s}</p>");
            }
            buf.clear();
        }
        for ch in el.children() {
            if ch.is_text() {
                buf += &esc(ch.text());
            } else if BLOCK_TAGS.contains(&ch.name()) {
                flush(&mut out, &mut buf);
                out += &self.block(ch, 3);
            } else {
                buf += &self.node(ch);
            }
        }
        flush(&mut out, &mut buf);
        out + &self.cited_floats(el)
    }

    fn inline(&mut self, el: Node<'a>) -> String {
        self.inline_skip(el, &[])
    }

    fn inline_skip(&mut self, el: Node<'a>, skip: &[&str]) -> String {
        let mut out = String::new();
        for ch in el.children() {
            if ch.is_text() {
                out += &esc(ch.text());
            } else if !skip.contains(&ch.name()) {
                out += &self.node(ch);
            }
        }
        out
    }

    fn node(&mut self, ch: Node<'a>) -> String {
        if ch.is_math() && ch.name() == "math" {
            return self.mathml(ch, false);
        }
        let simple = match ch.name() {
            "italic" => Some("em"),
            "bold" => Some("strong"),
            "sup" => Some("sup"),
            "sub" => Some("sub"),
            "underline" => Some("u"),
            "monospace" => Some("code"),
            "strike" => Some("s"),
            "overline" => Some("span"),
            _ => None,
        };
        if let Some(t) = simple {
            return format!("<{t}>{}</{t}>", self.inline(ch));
        }
        match ch.name() {
            "sc" => format!("<span class=\"smallcaps\">{}</span>", self.inline(ch)),
            "ext-link" | "uri" | "self-uri" => {
                let mut href = ch.attr("href").map(String::from).unwrap_or_else(|| ch.all_text().trim().to_string());
                if ch.attr("ext-link-type") == Some("doi") && !href.starts_with("http") {
                    href = format!("https://doi.org/{href}");
                }
                if !(href.starts_with("http") || href.starts_with("mailto:") || href.starts_with("ftp")) {
                    return self.inline(ch);
                }
                format!("<a href=\"{}\">{}</a>", esc(&href), self.inline(ch))
            }
            "xref" => {
                let rid = ch.attr("rid").unwrap_or("").split(' ').find(|s| !s.is_empty()).unwrap_or("").to_string();
                let txt = self.inline(ch);
                let shown = if txt.is_empty() { esc(&rid) } else { txt };
                if rid.is_empty() {
                    shown
                } else {
                    format!("<a href=\"#{}\">{shown}</a>", esc(&rid))
                }
            }
            "inline-formula" => self.formula(ch, false),
            "disp-formula" => self.formula(ch, true),
            "tex-math" => self.texmath(ch),
            "inline-graphic" => match self.image(ch.attr("href")) {
                Some(src) => format!("<img class=\"math\" src=\"{}\" alt=\"\"/>", esc(&src)),
                None => String::new(),
            },
            "break" => "<br/>".into(),
            "fn" => format!(" <span class=\"fn\">[{}]</span>", esc(&ch.plain())),
            "label" => format!("<strong>{}</strong> ", self.inline(ch)),
            "email" => {
                let e = ch.all_text().trim().to_string();
                format!("<a href=\"mailto:{}\">{}</a>", esc(&e), esc(&e))
            }
            "object-id" | "permissions" | "media" | "institution-id" => String::new(),
            "alternatives" => self.alternatives(ch, false),
            _ => self.inline(ch),
        }
    }

    // MARK: - Math

    fn alternatives(&mut self, el: Node<'a>, display: bool) -> String {
        if let Some(m) = el.elements().find(|e| e.is_math() && e.name() == "math") {
            return self.mathml(m, display);
        }
        if let Some(t) = el.child("tex-math") {
            return self.texmath(t);
        }
        if let Some(g) = el.child("graphic").or_else(|| el.child("inline-graphic")) {
            if let Some(src) = self.image(g.attr("href")) {
                return format!("<img class=\"math\" src=\"{}\" alt=\"\"/>", esc(&src));
            }
        }
        self.inline(el)
    }

    fn formula(&mut self, el: Node<'a>, display: bool) -> String {
        let mut inner = String::new();
        for ch in el.elements() {
            if ch.name() == "alternatives" {
                inner = self.alternatives(ch, display);
            } else if ch.is_math() && ch.name() == "math" {
                inner = self.mathml(ch, display);
            } else if ch.name() == "tex-math" {
                inner = self.texmath(ch);
            } else if ch.name() == "graphic" {
                if let Some(src) = self.image(ch.attr("href")) {
                    inner = format!("<img class=\"math\" src=\"{}\" alt=\"\"/>", esc(&src));
                }
            }
        }
        if inner.is_empty() {
            inner = self.inline_skip(el, &["label"]);
        }
        if !display {
            return inner;
        }
        let label = label_of(el);
        let lab = if label.is_empty() { String::new() } else { format!(" <span class=\"eqlabel\">{}</span>", esc(&label)) };
        format!("<div class=\"equation\"{}><p>{inner}{lab}</p></div>", id_attr(el))
    }

    fn mathml(&self, m: Node<'a>, display: bool) -> String {
        let mut s = format!("<math xmlns=\"{MATH_NS}\"");
        if display {
            s += " display=\"block\"";
        }
        for (k, v) in m.attrs() {
            if ["id", "display", "xmlns"].contains(&k.as_str()) {
                continue;
            }
            s += &format!(" {k}=\"{}\"", esc(v));
        }
        s += ">";
        for c in m.children() {
            serialize(c, &mut s);
        }
        s + "</math>"
    }

    /// There is no TeX → MathML converter; keep the source readable.
    fn texmath(&mut self, el: Node<'a>) -> String {
        let mut t = el.all_text();
        if let Some(i) = t.find("\\begin{document}") {
            t = t[i + "\\begin{document}".len()..].to_string();
        }
        if let Some(i) = t.find("\\end{document}") {
            t.truncate(i);
        }
        let t = t.trim().trim_matches('$');
        self.tex_formulas += 1;
        format!("<code class=\"tex\">{}</code>", esc(t.trim()))
    }

    // MARK: - Figures and tables

    fn image(&self, href: Option<&str>) -> Option<String> {
        let img = self.images.get(href?)?;
        Some(format!("../images/{}", img.file_name))
    }

    fn caption(&mut self, el: Node<'a>) -> String {
        let mut parts = Vec::new();
        let label = label_of(el);
        if !label.is_empty() {
            parts.push(format!("<strong>{}</strong>", esc(&label)));
        }
        if let Some(cap) = el.child("caption") {
            if let Some(t) = cap.child("title") {
                parts.push(format!("<strong>{}</strong>", self.inline(t)));
            }
            for p in cap.children_named("p") {
                let txt = self.inline(p);
                if txt.starts_with("<strong>DOI:</strong>") {
                    continue;
                }
                parts.push(txt);
            }
        }
        parts.join(" ")
    }

    /// Image tags for the graphics under `el`, taking one per `<alternatives>` group.
    fn graphic_imgs(&self, el: Node<'a>, alt: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut seen_alt = HashSet::new();
        let mut seen_src = HashSet::new();
        for g in el.descendants("graphic") {
            if g.attr("content-type") == Some("thumb") {
                continue;
            }
            let alt_parent = g.parent().filter(|p| p.name() == "alternatives");
            if let Some(p) = alt_parent {
                if seen_alt.contains(&p.id) {
                    continue;
                }
            }
            let Some(src) = self.image(g.attr("href")) else { continue };
            if seen_src.contains(&src) {
                continue;
            }
            if let Some(p) = alt_parent {
                seen_alt.insert(p.id);
            }
            out.push(format!("<img src=\"{}\" alt=\"{}\"/>", esc(&src), esc(alt)));
            seen_src.insert(src);
        }
        out
    }

    fn fig(&mut self, el: Node<'a>) -> String {
        let label = el.child_text("label").unwrap_or_else(|| "Figure".into()).trim().to_string();
        let mut imgs = self.graphic_imgs(el, &label);
        if imgs.is_empty() {
            self.missing_figures += 1;
            imgs = vec!["<span class=\"missing\">[Figure image not available]</span>".into()];
        }
        format!("<figure{}>{}<figcaption>{}</figcaption></figure>", id_attr(el), imgs.join(""), self.caption(el))
    }

    fn table_wrap(&mut self, el: Node<'a>) -> String {
        let tables = el.descendants("table");
        let mut body: Vec<String> = tables.iter().map(|t| self.table(*t)).collect();
        if tables.is_empty() {
            body = self.graphic_imgs(el, "Table");
        }
        let foot = match el.child("table-wrap-foot") {
            Some(f) => format!("<div class=\"tablefoot\">{}</div>", self.blocks(f, 4, &[])),
            None => String::new(),
        };
        let cap = self.caption(el);
        format!("<div class=\"table-wrap\"{}><p class=\"caption\">{cap}</p>{}{foot}</div>", id_attr(el), body.join(""))
    }

    fn table(&mut self, t: Node<'a>) -> String {
        let mut s = String::from("<table>");
        if let Some(th) = t.child("thead") {
            s += "<thead>";
            for tr in th.descendants("tr") {
                s += &self.row(tr);
            }
            s += "</thead>";
        }
        let bodies: Vec<Node> = t.children_named("tbody").collect();
        if !bodies.is_empty() {
            for b in bodies {
                s += "<tbody>";
                for tr in b.descendants("tr") {
                    s += &self.row(tr);
                }
                s += "</tbody>";
            }
        } else {
            s += "<tbody>";
            for tr in t.children_named("tr").collect::<Vec<_>>() {
                s += &self.row(tr);
            }
            s += "</tbody>";
        }
        s + "</table>"
    }

    fn row(&mut self, tr: Node<'a>) -> String {
        let mut s = String::from("<tr>");
        for c in tr.elements().filter(|c| c.name() == "td" || c.name() == "th") {
            let mut attrs = String::new();
            for a in ["rowspan", "colspan"] {
                if let Some(v) = c.attr(a) {
                    attrs += &format!(" {a}=\"{}\"", esc(v));
                }
            }
            s += &format!("<{n}{attrs}>{}</{n}>", self.inline(c), n = c.name());
        }
        s + "</tr>"
    }

    fn list(&mut self, el: Node<'a>) -> String {
        let typ = el.attr("list-type").unwrap_or("bullet");
        let ordered = ["order", "alpha-lower", "alpha-upper", "roman-lower", "roman-upper"].contains(&typ);
        let tag = if ordered { "ol" } else { "ul" };
        let style = match typ {
            "alpha-lower" => " type=\"a\"",
            "alpha-upper" => " type=\"A\"",
            "roman-lower" => " type=\"i\"",
            "roman-upper" => " type=\"I\"",
            "simple" => " class=\"simple\"",
            _ => "",
        };
        let items: String =
            el.children_named("list-item").map(|li| format!("<li>{}</li>", self.blocks(li, 4, &["label"]))).collect();
        if items.is_empty() {
            String::new()
        } else {
            format!("<{tag}{style}>{items}</{tag}>")
        }
    }

    fn supp(&mut self, el: Node<'a>) -> String {
        let label =
            el.child_text("label").unwrap_or_else(|| "Supplementary material".into()).trim().to_string();
        let txt = el.child("caption").and_then(|c| c.child("title")).map(|t| self.inline(t)).unwrap_or_default();
        let href = el
            .attr("href")
            .or_else(|| el.first_descendant("media").and_then(|m| m.attr("href")))
            .unwrap_or("");
        let href_part = if href.is_empty() { String::new() } else { format!(" <em>({})</em>", esc(href)) };
        format!("<p class=\"supp\"{}><strong>{}</strong> {txt}{href_part}</p>", id_attr(el), esc(&label))
    }

    // MARK: - References

    fn ref_items(&mut self, rl: Node<'a>) -> String {
        let mut items = String::new();
        for r in rl.children_named("ref") {
            let Some(cit) =
                ["element-citation", "mixed-citation", "citation", "nlm-citation"].iter().find_map(|n| r.child(n))
            else {
                continue;
            };
            items += &format!("<li{}>{}</li>", id_attr(r), self.citation(cit));
        }
        format!("<ol class=\"refs\">{items}</ol>")
    }

    fn citation(&mut self, c: Node<'a>) -> String {
        let pub_doi = c
            .descendants("pub-id")
            .into_iter()
            .find(|p| p.attr("pub-id-type") == Some("doi"))
            .map(|p| p.all_text().trim().to_string());
        let doi_link = |d: &str| format!("<a href=\"https://doi.org/{}\">doi:{}</a>", esc(d), esc(d));

        let own: String = c.children().filter(|x| x.is_text()).map(|x| x.text()).collect();
        if c.name() == "mixed-citation" && own.trim().chars().count() > 10 {
            let mut s = self.inline_skip(c, &["pub-id"]);
            if let Some(d) = pub_doi.as_deref().filter(|d| !d.is_empty()) {
                s += &format!(" {}", doi_link(d));
            }
            return s;
        }
        let mut names: Vec<String> = Vec::new();
        let groups: Vec<Node> = c.children_named("person-group").collect();
        let pgs = if groups.is_empty() { vec![c] } else { groups };
        for pg in pgs {
            for n in pg.elements().filter(|n| n.name() == "name" || n.name() == "string-name") {
                let sur = n.child_text("surname").unwrap_or_default();
                let giv = n.child_text("given-names").unwrap_or_default();
                let full = format!("{sur} {giv}").trim().to_string();
                names.push(if full.is_empty() { n.plain() } else { full });
            }
            for col in pg.children_named("collab") {
                names.push(col.plain());
            }
        }
        if names.len() > 10 {
            names.truncate(10);
            names.push("et al.".into());
        }
        let year = c.child_text("year").unwrap_or_default();
        let title_el = c.child("article-title").or_else(|| c.child("chapter-title"));
        let title = title_el.map(|t| self.inline(t)).unwrap_or_default();
        let src = c.child("source").map(|t| self.inline(t)).unwrap_or_default();
        let vol = c.child_text("volume").unwrap_or_default();
        let fp = c.child_text("fpage").unwrap_or_default();
        let lp = c.child_text("lpage").unwrap_or_default();
        let eloc = c.child_text("elocation-id").unwrap_or_default();
        let mut parts: Vec<String> = Vec::new();
        if !names.is_empty() {
            parts.push(format!("{}.", esc(&names.join(", "))));
        }
        if !year.is_empty() {
            parts.push(format!("{}.", esc(&year)));
        }
        if !title.is_empty() {
            parts.push(if title.ends_with('.') { title } else { format!("{title}.") });
        }
        if !src.is_empty() {
            let mut s = format!("<em>{src}</em>");
            if !vol.is_empty() {
                s += &format!(" <strong>{}</strong>", esc(&vol));
            }
            if !fp.is_empty() {
                s += &format!(":{}", esc(&fp));
                if !lp.is_empty() {
                    s += &format!("–{}", esc(&lp));
                }
            } else if !eloc.is_empty() {
                s += &format!(":{}", esc(&eloc));
            }
            parts.push(format!("{s}."));
        }
        if let Some(d) = pub_doi.as_deref().filter(|d| !d.is_empty()) {
            parts.push(doi_link(d));
        } else if let Some(p) = c.descendants("pub-id").into_iter().find(|p| p.attr("pub-id-type") == Some("pmid")) {
            let p = p.all_text().trim().to_string();
            parts.push(format!("<a href=\"https://pubmed.ncbi.nlm.nih.gov/{}\">PMID {}</a>", esc(&p), esc(&p)));
        }
        if parts.is_empty() {
            self.inline(c)
        } else {
            parts.join(" ")
        }
    }
}

fn pad2(s: &str) -> String {
    if s.chars().count() == 1 {
        format!("0{s}")
    } else {
        s.to_string()
    }
}

fn has_section_ancestor(n: Node, stop: Node) -> bool {
    let mut p = n.parent();
    while let Some(x) = p {
        if x == stop {
            break;
        }
        if ["sec", "app", "ack", "notes", "glossary"].contains(&x.name()) {
            return true;
        }
        p = x.parent();
    }
    false
}

fn serialize(n: Node, s: &mut String) {
    if n.is_text() {
        s.push_str(&esc(n.text()));
        return;
    }
    s.push('<');
    s.push_str(n.name());
    for (k, v) in n.attrs() {
        if k == "id" {
            continue;
        }
        // EPUB's MathML schema rejects displaystyle on most elements other than these
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
