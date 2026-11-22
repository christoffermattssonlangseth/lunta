//! Article pages on Atypon's Literatum platform in its "core" article markup: NEJM, Science
//! (science.org), PNAS, SAGE, ACS, ASM, AHA, Annual Reviews, Taylor & Francis and Wiley's newer
//! templates, … (and Cell Press / Lancet pages when they carry it). A port of `Atypon.swift`.
//!
//! - details in `dc.*` / `citation_*` meta tags (NEJM has no `citation_doi`: the DOI comes from
//!   the canonical link / URL);
//! - the abstract(s) before the body: `section#abstract` / `section[role=doc-abstract]` (Science:
//!   also the editor's summary; PNAS: Significance), NEJM's `section#summary-abstract`;
//! - `section#bodymatter`: unheaded introduction paragraphs (NEJM), then `section`s with h2 and
//!   nested sections with h3; paragraphs are `div[role=paragraph]`; figures and tables are
//!   `figure.graphic` / `figure.table` (NEJM: inside `div.figure-wrap` with the label in a
//!   header) with `figcaption`; display equations are MathML;
//! - `section#backmatter`: notes / acknowledgments / funding sections, `#supplementary-materials`,
//!   `#bibliography` (`.biblioentry` or `[role=listitem]` > `.citations` > `.citation-content`);
//!   in-text citations link to the list (`#r1`, `#core-R1`), to the copy in the side pane
//!   (`#core-collateral-r1`), or (cell.com) to "#" with the reference in a hidden pop-up and its
//!   id in `data-xml-rid`; they are pointed at the list here.
//!
//! Everything else (navigation, side panes and tabs, ads, metrics, "cited by", author pane) is
//! never read. Pages without the core markup go to the generic reader; a page with only the
//! abstract (no access) is refused.

use super::sciencedirect::link_across_chapters;
use super::webpage::{self, is_heading, is_references, re, text_of, title_case, words, Adapter, MetaTags, PageImages, Renderer};
use super::xml::{Node, NodeId};
use super::{esc, fail, html, resolver, Book, Chapter, Heading, Options, PaperMeta, Result};
use std::collections::HashSet;

/// How one site differs (NEJM: its own abstract and back-matter sections, its label).
struct Site {
    is_nejm: bool,
    label: String,
    /// "NEJM.org", "science.org"
    name: String,
    /// when the page names none
    journal: String,
}

impl Site {
    fn of(url: Option<&url::Url>) -> Site {
        let host = url.and_then(|u| u.host_str()).unwrap_or("").to_lowercase();
        if host.ends_with("nejm.org") {
            return Site {
                is_nejm: true,
                label: "NEJM full text".into(),
                name: "NEJM.org".into(),
                journal: "New England Journal of Medicine".into(),
            };
        }
        let name = host.strip_prefix("www.").unwrap_or(&host).to_string();
        Site {
            is_nejm: false,
            label: format!("Full text from {}", if name.is_empty() { "the publisher" } else { &name }),
            name: if name.is_empty() { "the publisher's site".into() } else { name },
            journal: String::new(),
        }
    }
}

/// The core markup, by a look at the page's source.
pub fn looks_like(html: &str) -> bool {
    html.contains("id=\"bodymatter\"") && html.contains("core-container")
}

pub fn book(page: &str, url: Option<&url::Url>, images: &PageImages, opts: &Options) -> Result<Book> {
    let site = Site::of(url);
    let mut doc = html::parse(page);
    if doc.root().first(|n| n.name() == "section" && n.attr("id") == Some("bodymatter")).is_none() {
        opts.log("No Atypon core article markup; reading the page generically");
        return webpage::generic(page, url, images, &site.label, Adapter::none(), opts);
    }
    opts.log(&format!(
        "Converting the {} page",
        if site.is_nejm { "NEJM".to_string() } else { format!("Atypon ({})", site.name) }
    ));

    // cell.com and other Atypon sites: each in-text citation carries its whole reference in a
    // hidden pop-up (.dropBlock__holder) and links to "#": drop the pop-up (it would land in the
    // text) and point the citation at the reference list (data-xml-rid="bib1" → #bib1)
    let holders: Vec<NodeId> = doc
        .root()
        .all(|n| n.has_class("reference-citations"))
        .into_iter()
        .flat_map(|s| s.all(|n| n.has_class("dropBlock__holder")))
        .map(|n| n.id)
        .collect();
    for h in holders {
        doc.remove(h);
    }
    let rids: Vec<(NodeId, String)> = doc
        .root()
        .all(|n| n.name() == "a" && !n.attr("data-xml-rid").unwrap_or("").is_empty())
        .into_iter()
        .filter(|a| matches!(a.attr("href").unwrap_or("#"), "#" | ""))
        .map(|a| (a.id, a.attr("data-xml-rid").unwrap().to_string()))
        .collect();
    for (a, rid) in rids {
        doc.set_attr(a, "href", Some(&format!("#{rid}")));
    }
    // in-text citations point at the side pane's copy of the list: point them at ours
    let mut targets = HashSet::new();
    let links: Vec<(NodeId, String)> = doc
        .root()
        .all(|n| n.name() == "a")
        .into_iter()
        .filter_map(|a| a.attr("href").filter(|h| h.starts_with('#')).map(|h| (a.id, h.to_string())))
        .collect();
    for (a, mut h) in links {
        if let Some(rest) = h.strip_prefix("#core-collateral-") {
            h = format!("#{rest}");
            doc.set_attr(a, "href", Some(&h));
        }
        targets.insert(h[1..].to_string());
    }

    let root = doc.root();
    let by_id = |id: &str| root.first(|n| n.attr("id") == Some(id)).map(|n| n.id);
    let mut meta = metadata(root, url, &site);
    let base = url.cloned().or_else(|| {
        url::Url::parse(&if site.is_nejm { "https://www.nejm.org/".to_string() } else { format!("https://{}/", site.name) }).ok()
    });
    let mut r = Renderer::new(images, base.clone(), &meta.title);
    r.targets = targets.clone();
    let mut conv = Conv { r, base: base.clone(), nejm: site.is_nejm, figures: 0, tables: 0 };
    let body = root.node_by(by_id("bodymatter").unwrap());

    let mut chapters: Vec<Chapter> = Vec::new();
    // abstract(s)
    if site.is_nejm {
        if let Some(abs) = by_id("summary-abstract").map(|i| root.node_by(i)) {
            let mut heads = vec![Heading { level: 1, id: conv.r.uid("abstract"), title: "Abstract".into() }];
            let mut h = String::new();
            for c in abs.elements().filter(|c| c.name() != "h2") {
                h += &conv.blocks(c, 2, &mut heads);
            }
            chapters.push(Chapter {
                title: "Abstract".into(),
                html: format!("<section><h1 id=\"{}\">Abstract</h1>{h}</section>", heads[0].id),
                headings: heads,
            });
            if meta.abstract_text.is_empty() {
                meta.abstract_text = abstract_text(abs);
            }
        }
    } else {
        for abs in abstracts(root, body) {
            let h = abs.elements().find(|e| is_heading(*e));
            let mut title = h.map(|h| h.plain().trim().to_string()).unwrap_or_default();
            if title.is_empty() {
                title = "Abstract".into();
            }
            title = title_case(&title);
            let hid = conv.r.unique_id(Some(abs.attr("id").unwrap_or("abstract")));
            let mut heads = vec![Heading { level: 1, id: hid, title: title.clone() }];
            let mut s = String::new();
            for c in abs.children().filter(|c| Some(c.id) != h.map(|h| h.id)) {
                s += &conv.blocks(c, 2, &mut heads);
            }
            if text_of(&s).trim().is_empty() {
                continue;
            }
            chapters.push(Chapter {
                title: title.clone(),
                html: format!("<section><h1 id=\"{}\">{}</h1>{s}</section>", esc(&heads[0].id), esc(&title)),
                headings: heads,
            });
            if meta.abstract_text.is_empty() && webpage::is_abstract(&title) {
                meta.abstract_text = abstract_text(abs);
            }
        }
    }
    let abstract_chapters = chapters.len();
    // body: unheaded introduction, then one chapter per section
    let container = body.elements().find(|e| e.has_class("core-container")).unwrap_or(body);
    let mut intro = String::new();
    let mut no_heads: Vec<Heading> = Vec::new();
    let push_intro = |conv: &mut Conv, chapters: &mut Vec<Chapter>, intro: &mut String| {
        if !intro.is_empty() {
            let id = conv.r.uid("introduction");
            chapters.push(Chapter {
                title: "Introduction".into(),
                html: format!("<section><h1 id=\"{id}\">Introduction</h1>{intro}</section>"),
                headings: vec![Heading { level: 1, id, title: "Introduction".into() }],
            });
            intro.clear();
        }
    };
    for c in container.children() {
        let h2 = if c.name() == "section" {
            c.child("h2").or_else(|| if site.is_nejm { None } else { c.elements().find(|e| is_heading(*e)) })
        } else {
            None
        };
        if let Some(h2) = h2 {
            push_intro(&mut conv, &mut chapters, &mut intro);
            chapters.push(conv.chapter(c, h2));
        } else {
            intro += &conv.blocks(c, 2, &mut no_heads);
        }
    }
    push_intro(&mut conv, &mut chapters, &mut intro);
    // no access: the page has the abstract and an empty (or teaser) body
    let body_words: usize = chapters[abstract_chapters..].iter().map(|c| words(&text_of(&c.html))).sum();
    if body_words < 250 {
        return Err(fail(if abstract_chapters > 0 || !meta.abstract_text.is_empty() {
            webpage::ABSTRACT_ONLY
        } else {
            "This page has no full text of the article (are you signed in?)."
        }));
    }
    // back matter: NEJM's notes; elsewhere every section (acknowledgments, funding, notes, …)
    let mut supp = by_id("supplementary-materials");
    let mut bib = by_id("bibliography");
    if site.is_nejm {
        if let Some(notes) = by_id("backnotes").map(|i| root.node_by(i)) {
            if let Some(h2) = notes.child("h2") {
                chapters.push(conv.chapter(notes, h2));
            }
        }
    } else if let Some(back) = by_id("backmatter").map(|i| root.node_by(i)) {
        for s in back_sections(back) {
            let id = s.attr("id").unwrap_or("");
            if id == "supplementary-materials"
                || (s.attr("role") == Some("doc-appendix") && s.plain().to_lowercase().contains("supplementary"))
            {
                supp = supp.or(Some(s.id));
                continue;
            }
            if id == "bibliography" || s.attr("role") == Some("doc-bibliography") {
                bib = bib.or(Some(s.id));
                continue;
            }
            if id.starts_with("tab-") || id.starts_with("core-collateral") || id == "cited-by" {
                continue;
            }
            let Some(h) = s.elements().find(|e| is_heading(*e)) else { continue };
            if is_references(&h.plain()) {
                bib = bib.or(Some(s.id));
                continue;
            }
            chapters.push(conv.chapter(s, h));
        }
    }
    // supplementary material: a short list of what exists (the files stay on the publisher's site)
    if let Some(supp) = supp.map(|i| root.node_by(i)) {
        let mut items = String::new();
        for item in supp.all(|x| x.has_class("core-supplementary-material") || (x.attr("role") == Some("listitem") && !site.is_nejm))
        {
            let mut label = item.first(|x| x.has_class("core-label") || x.has_class("label")).map(|x| x.plain()).unwrap_or_default();
            if label.is_empty() && !site.is_nejm {
                label = item.first(|x| x.name() == "a").map(|x| x.plain()).unwrap_or_default();
            }
            if label.is_empty() {
                continue;
            }
            let id = item.attr("id").filter(|i| targets.contains(*i)).map(|i| conv.r.uid(i));
            let href = item
                .first(|x| x.name() == "a")
                .and_then(|a| a.attr("href"))
                .and_then(|h| webpage::absolute(base.as_ref(), h))
                .map(|x| x.1);
            let name = href.map(|h| format!("<a href=\"{}\">{}</a>", esc(&h), esc(&label))).unwrap_or_else(|| esc(&label));
            items += &format!("<li{}>{name}</li>", id.map(|i| format!(" id=\"{}\"", esc(&i))).unwrap_or_default());
        }
        if !items.is_empty() {
            let id = conv.r.uid("supplementary-material");
            chapters.push(Chapter {
                title: "Supplementary Material".into(),
                html: format!(
                    "<section><h1 id=\"{id}\">Supplementary Material</h1><p>Available with the full text of this article at {}:</p><ul>{items}</ul></section>",
                    esc(&site.name)
                ),
                headings: vec![Heading { level: 1, id, title: "Supplementary Material".into() }],
            });
        }
    }

    // references: first the entries and what to leave out of each (labels, back links, service
    // links), which is taken out of the page before the entries are written
    struct Entry {
        id_attrs: Vec<String>,
        cit: NodeId,
        content: NodeId,
        raw: bool,
        entry: NodeId,
    }
    let mut entries: Vec<Entry> = Vec::new();
    let mut biblio = false;
    if let Some(bib) = bib.map(|i| root.node_by(i)) {
        let list = bib.all(|x| x.has_class("biblioentry"));
        if !list.is_empty() || site.is_nejm {
            biblio = true;
            for e in list {
                let Some(cit) = e.first(|x| x.has_class("citations")) else { continue };
                let Some(content) = cit.first(|x| x.has_class("citation-content")) else { continue };
                entries.push(Entry { id_attrs: vec![], cit: cit.id, content: content.id, raw: site.is_nejm, entry: e.id });
            }
        } else {
            // Science / PNAS: div[role=list] > div[role=listitem]#core-R1 > .label + .citations (or list items)
            let list: Vec<Node> = bib
                .all(|x| x.attr("role") == Some("listitem") || (x.name() == "li" && !x.has_ancestor(|a| a.name() == "li")))
                .into_iter()
                .filter(|e| !e.has_ancestor(|a| a.attr("role") == Some("listitem")))
                .collect();
            for e in list {
                let cit = e.first(|x| x.has_class("citations") || x.has_class("citation"));
                let content = cit.and_then(|c| c.first(|x| x.has_class("citation-content"))).or(cit).unwrap_or(e);
                let ids: Vec<String> = [e.attr("id"), cit.and_then(|c| c.attr("id")), content.attr("id")]
                    .into_iter()
                    .flatten()
                    .filter(|i| !i.is_empty())
                    .map(String::from)
                    .collect();
                entries.push(Entry { id_attrs: ids, cit: cit.unwrap_or(e).id, content: content.id, raw: false, entry: e.id });
            }
        }
    }
    let strip: Vec<NodeId> = entries
        .iter()
        .filter(|e| !e.raw)
        .flat_map(|e| {
            root.node_by(e.content).all(|x| {
                x.has_class("external-links")
                    || x.has_class("label")
                    || x.has_class("to-citation__wrapper")
                    || x.attr("role") == Some("doc-backlink")
            })
        })
        .map(|x| x.id)
        .collect();
    for x in strip {
        doc.remove(x);
    }
    let root = doc.root();
    let mut ref_count = 0;
    let mut items = String::new();
    for e in &entries {
        let cit = root.node_by(e.cit);
        let content = root.node_by(e.content);
        if biblio {
            let id = cit.attr("id").map(|i| conv.r.uid(i));
            let extra =
                if site.is_nejm { String::new() } else { anchors(&[root.node_by(e.entry).attr("id")], id.as_deref(), &mut conv.r) };
            let text = ref_text(content, cit, &mut conv.r);
            items += &format!("<li{}>{extra}{text}</li>", id.map(|i| format!(" id=\"{}\"", esc(&i))).unwrap_or_default());
            ref_count += 1;
        } else {
            let id = e.id_attrs.first().map(|i| conv.r.uid(i));
            let rest: Vec<Option<&str>> = e.id_attrs.iter().skip(1).map(|s| Some(s.as_str())).collect();
            let extra = anchors(&rest, id.as_deref(), &mut conv.r);
            let text = ref_text(content, cit, &mut conv.r);
            if text_of(&text).trim().is_empty() {
                continue;
            }
            items += &format!("<li{}>{extra}{text}</li>", id.map(|i| format!(" id=\"{}\"", esc(&i))).unwrap_or_default());
            ref_count += 1;
        }
    }
    if !items.is_empty() {
        let id = conv.r.uid("references");
        chapters.push(Chapter {
            title: "References".into(),
            html: format!("<section><h1 id=\"{id}\">References</h1><ol class=\"refs\">{items}</ol></section>"),
            headings: vec![Heading { level: 1, id, title: "References".into() }],
        });
    }
    link_across_chapters(&mut chapters);
    webpage::replace_missing(&mut chapters);
    opts.log(&format!(
        "{} chapters, {} figure(s), {} table(s), {ref_count} references, {} image(s) missing",
        chapters.len(),
        conv.figures,
        conv.tables,
        conv.r.missing_images
    ));
    let mut notes = Vec::new();
    if conv.r.missing_images > 0 {
        notes.push(format!("{} figure image(s) were not sent with the page; captions are kept.", conv.r.missing_images));
    }
    if conv.r.math_count > 0 {
        notes.push(format!("{} equation(s) as MathML.", conv.r.math_count));
    }
    Ok(Book { meta, chapters, images: std::mem::take(&mut conv.r.images), source_label: site.label, notes, ..Default::default() })
}

trait NodeBy<'a> {
    fn node_by(&self, id: NodeId) -> Node<'a>;
}

impl<'a> NodeBy<'a> for Node<'a> {
    fn node_by(&self, id: NodeId) -> Node<'a> {
        self.doc().node(id)
    }
}

/// A reference's text, with its DOI link (from the "View" / Crossref link).
fn ref_text(content: Node, cit: Node, r: &mut Renderer) -> String {
    let mut text = r.inline(content).trim().to_string();
    if let Some(doi) = cit
        .first(|x| x.has_class("core-xlink-crossref"))
        .and_then(|x| x.first_descendant("a"))
        .and_then(|a| a.attr("href"))
        .filter(|d| d.starts_with("http"))
    {
        text += &format!(" <a href=\"{}\">{}</a>", esc(doi), esc(&doi.replace("https://doi.org/", "doi:")));
    }
    text
}

/// Empty anchors for a reference's other ids (in-text links use one or the other).
fn anchors(ids: &[Option<&str>], besides: Option<&str>, r: &mut Renderer) -> String {
    let mut out = String::new();
    for i in ids.iter().flatten() {
        if !i.is_empty() && Some(*i) != besides && r.targets.contains(*i) {
            out += &format!("<span id=\"{}\"></span>", esc(&r.uid(i)));
        }
    }
    out
}

fn abstract_text(abs: Node) -> String {
    let secs: Vec<Node> = abs.elements().filter(|e| e.name() == "section").collect();
    if secs.is_empty() {
        return abs.elements().filter(|e| !is_heading(*e)).map(|e| e.plain()).collect::<Vec<_>>().join(" ");
    }
    secs.iter()
        .map(|s| {
            let h = s.child("h3").map(|h| h.plain()).unwrap_or_default();
            let t = s.elements().filter(|e| e.name() != "h3").map(|e| e.plain()).collect::<Vec<_>>().join(" ");
            if h.is_empty() {
                t
            } else {
                format!("{h}: {t}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The abstracts before the body (Abstract, Structured Abstract, Editor's summary, Significance,
/// Graphical abstract …), outermost only, in page order; not the copies in side panes.
fn abstracts<'a>(doc: Node<'a>, body: Node<'a>) -> Vec<Node<'a>> {
    fn is_abs(x: Node) -> bool {
        if x.name() != "section" {
            return false;
        }
        let id = x.attr("id").unwrap_or("").to_lowercase();
        x.attr("role") == Some("doc-abstract")
            || x.attr("property") == Some("abstract")
            || id == "abstract"
            || id.starts_with("abstract-")
            || id.ends_with("-abstract")
            || id == "executive-summary"
            || id == "significance"
    }
    let bad = |a: Node| {
        a.id == body.id
            || a.attr("id") == Some("backmatter")
            || a.attr("id").unwrap_or("").contains("collateral")
            || a.attr("role") == Some("tabpanel")
            || a.classes().iter().any(|c| c.contains("collateral") || c.starts_with("tab__"))
    };
    doc.all(is_abs)
        .into_iter()
        .filter(|x| !x.has_ancestor(is_abs) && !x.has_ancestor(bad) && words(&x.plain()) >= 15)
        .collect()
}

/// The sections of the back matter, in order (through wrappers such as `div.core-container`).
fn back_sections(back: Node) -> Vec<Node> {
    fn walk<'a>(n: Node<'a>, out: &mut Vec<Node<'a>>) {
        for c in n.elements() {
            if c.name() == "section" {
                out.push(c);
            } else if c.name() == "div" {
                walk(c, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(back, &mut out);
    out
}

fn metadata(doc: Node, url: Option<&url::Url>, site: &Site) -> PaperMeta {
    let tags = MetaTags::of(doc);
    let mut m = webpage::metadata(doc, url);
    m.title = tags.first(&["citation_title", "dc.title"]);
    if m.title.is_empty() {
        if let Some(h) = doc.first(|n| n.name() == "h1" && n.attr("property") == Some("name")) {
            m.title = h.plain();
        }
    }
    if m.authors.is_empty() {
        m.authors = tags.all("dc.creator").cloned().unwrap_or_default();
    }
    if m.journal.is_empty() {
        m.journal = if site.journal.is_empty() { tags.first(&["og:site_name"]) } else { site.journal.clone() };
    }
    let date = tags.first(&["dc.date", "citation_publication_date", "citation_online_date", "citation_date", "dc.date.issued"]);
    if !date.is_empty() {
        m.date = date.replace('/', "-");
    }
    if m.doi.is_empty() {
        let canonical =
            doc.first(|n| n.name() == "link" && n.attr("rel") == Some("canonical")).and_then(|l| l.attr("href")).unwrap_or("");
        for s in [canonical.to_string(), tags.first(&["og:url"]), url.map(|u| u.to_string()).unwrap_or_default()] {
            if m.doi.is_empty() {
                if let Some(d) = resolver::doi_in(&s) {
                    m.doi = d;
                }
            }
        }
    }
    m.license = String::new(); // dc.Rights is the copyright line
    m.url = if m.doi.is_empty() { url.map(|u| u.to_string()).unwrap_or_default() } else { format!("https://doi.org/{}", m.doi) };
    m.abstract_text = String::new(); // from the page's structured abstract (the meta description is cut short)
    m
}

// MARK: Rendering

struct Conv<'b> {
    r: Renderer<'b>,
    base: Option<url::Url>,
    nejm: bool,
    figures: usize,
    tables: usize,
}

impl Conv<'_> {
    /// A top-level section as a chapter (its h2 the chapter title; nested h3 sections as h2).
    fn chapter(&mut self, s: Node, h2: Node) -> Chapter {
        let title = title_case(h2.plain().trim());
        let id = self.r.unique_id(Some(s.attr("id").map(String::from).unwrap_or_else(|| title.to_lowercase()).as_str()));
        let mut heads = vec![Heading { level: 1, id: id.clone(), title: title.clone() }];
        let mut h = String::new();
        for c in s.children().filter(|c| c.id != h2.id) {
            h += &self.blocks(c, 2, &mut heads);
        }
        Chapter { title: title.clone(), html: format!("<section><h1 id=\"{}\">{}</h1>{h}</section>", esc(&id), esc(&title)), headings: heads }
    }

    /// Block content. `depth`: the heading level a nested section's heading gets (2, then 3).
    fn blocks(&mut self, n: Node, depth: usize, headings: &mut Vec<Heading>) -> String {
        if n.is_text() {
            return if n.text().trim().is_empty() { String::new() } else { format!("<p>{}</p>", esc(n.text())) };
        }
        match n.name() {
            "section" => {
                let mut out = String::new();
                if let Some(h) = n.elements().find(|e| is_heading(*e)) {
                    let t = h.plain().trim().to_string();
                    let level = depth.min(3);
                    let id = self.r.unique_id(n.attr("id").or(h.attr("id")));
                    if level == 2 {
                        headings.push(Heading { level: 2, id: id.clone(), title: t.clone() });
                    }
                    if !t.is_empty() {
                        out += &format!("<h{level} id=\"{}\">{}</h{level}>", esc(&id), esc(&t));
                    }
                    for c in n.children().filter(|c| c.id != h.id) {
                        out += &self.blocks(c, depth + 1, headings);
                    }
                } else if let Some(id) = n.attr("id").filter(|i| self.r.targets.contains(*i)) {
                    out += &format!("<span id=\"{}\"></span>", esc(&self.r.uid(id)));
                    for c in n.children() {
                        out += &self.blocks(c, depth, headings);
                    }
                } else {
                    for c in n.children() {
                        out += &self.blocks(c, depth, headings);
                    }
                }
                out
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let t = n.plain().trim().to_string();
                if t.is_empty() {
                    String::new()
                } else {
                    format!("<p><strong>{}</strong></p>", esc(&t))
                }
            }
            "figure" => self.figure_wrap(n, n),
            "ul" | "ol" => self.r.list(n),
            "table" => {
                self.tables += 1;
                format!("<div class=\"table-wrap\">{}</div>", self.r.table(n))
            }
            "script" | "style" | "button" | "nav" | "aside" | "noscript" | "svg" | "header" | "form" | "input" => String::new(),
            "div" => {
                if n.has_class("figure-wrap") {
                    if let Some(f) = n.first_descendant("figure") {
                        return self.figure_wrap(f, n);
                    }
                }
                if n.attr("role") == Some("paragraph") || n.attr("role") == Some("doc-footnote") {
                    // a paragraph may hold a display formula or list; otherwise it is one paragraph
                    if n.elements().any(|e| ["ul", "ol", "table", "figure"].contains(&e.name()) || e.has_class("figure-wrap")) {
                        let mut out = String::new();
                        for c in n.children() {
                            if c.is_text() || !self.r.is_block(c) {
                                out += &self.wrap_inline(c);
                            } else {
                                out += &self.blocks(c, depth, headings);
                            }
                        }
                        return out;
                    }
                    let inner = self.r.inline(n).trim().to_string();
                    if inner.is_empty() {
                        return String::new();
                    }
                    let id = n.attr("id").filter(|i| self.r.targets.contains(*i)).map(|i| self.r.uid(i));
                    return format!("<p{}>{inner}</p>", id.map(|i| format!(" id=\"{}\"", esc(&i))).unwrap_or_default());
                }
                if n.attr("data-type") == Some("digital-object-links") {
                    return String::new();
                }
                if n.attr("role") == Some("list") {
                    return n.elements().map(|e| format!("<p>{}</p>", self.r.inline(e).trim())).collect();
                }
                n.children().map(|c| self.blocks(c, depth, headings)).collect()
            }
            _ => {
                if self.r.is_block(n) {
                    n.children().map(|c| self.blocks(c, depth, headings)).collect()
                } else {
                    self.wrap_inline(n)
                }
            }
        }
    }

    fn wrap_inline(&mut self, n: Node) -> String {
        let t = self.r.inline(n).trim().to_string();
        if t.is_empty() {
            String::new()
        } else {
            format!("<p>{t}</p>")
        }
    }

    /// `figure.graphic` → image + caption; `figure.table` → the HTML table (its image if the page
    /// has no table markup) with caption and footnotes.
    fn figure_wrap(&mut self, f: Node, wrap: Node) -> String {
        let id = self.r.unique_id(f.attr("id"));
        let mut label = wrap
            .first(|x| x.has_class("label") && x.parent().map(|p| p.name()) == Some("header"))
            .map(|x| x.plain().trim().to_string())
            .unwrap_or_default();
        let cap = f.first_descendant("figcaption");
        let mut title = match cap.and_then(|c| c.first(|x| x.has_class("caption"))) {
            Some(c) => self.r.inline(c).trim().to_string(),
            None => String::new(),
        };
        let mut notes: Vec<Node> = cap.map(|c| c.all(|x| x.attr("role") == Some("doc-footnote"))).unwrap_or_default();
        let mut plain_title = false; // a whole caption (title and legend), not a title to set in bold
        if !self.nejm {
            // Science / PNAS: the label in the caption, then a title and paragraphs; table notes after the table
            let cap_label = cap.and_then(|c| c.first(|x| x.has_class("label")));
            if label.is_empty() {
                if let Some(l) = cap_label {
                    label = re!(r"\.$").replace(l.plain().trim(), "").into_owned();
                }
            }
            if title.is_empty() {
                if let Some(cap) = cap {
                    plain_title = true;
                    let mut t = String::new();
                    for c in cap.children() {
                        if Some(c.id) == cap_label.map(|l| l.id)
                            || c.attr("role") == Some("doc-footnote")
                            || (c.is_text() && c.text().trim().is_empty())
                        {
                            continue;
                        }
                        if self.r.is_block(c) {
                            t += &format!("<span class=\"cap-p\">{}</span> ", self.r.inline(c).trim());
                        } else {
                            t += &self.r.inline(c);
                        }
                    }
                    title = t.trim().to_string();
                }
            }
            notes.extend(f.all(|x| {
                (x.has_class("table-wrap-foot") || x.has_class("table-foot") || x.attr("role") == Some("doc-footnote"))
                    && !x.has_ancestor(|a| a.name() == "figcaption")
                    && !x.has_ancestor(|a| a.has_class("table-wrap-foot") || a.attr("role") == Some("doc-footnote"))
            }));
        }
        let is_table = f.has_class("table") || f.first_descendant("table").is_some();
        let head = format!(
            "{}{}",
            if label.is_empty() { String::new() } else { format!("<strong>{}.</strong> ", esc(&label)) },
            if title.is_empty() {
                String::new()
            } else if plain_title {
                title.clone()
            } else {
                format!("<strong>{title}</strong>")
            }
        );

        if let Some(table) = f.first_descendant("table").filter(|_| is_table) {
            self.tables += 1;
            let mut out = if head.is_empty() { String::new() } else { format!("<p class=\"table-caption\">{head}</p>") };
            out += &self.r.table(table);
            for fnode in notes {
                let mark = fnode.first(|x| x.has_class("label")).map(|x| x.plain()).unwrap_or_default();
                let body = fnode.elements().find(|e| !e.has_class("label"));
                let fid = body.and_then(|b| b.attr("id")).map(|i| self.r.uid(i));
                let text = match body {
                    Some(b) => self.r.inline(b),
                    None => self.r.inline(fnode),
                };
                out += &format!(
                    "<p class=\"table-foot\"{}><small>{}{}</small></p>",
                    fid.map(|i| format!(" id=\"{}\"", esc(&i))).unwrap_or_default(),
                    if mark.is_empty() { String::new() } else { esc(&mark) + " " },
                    text.trim()
                );
            }
            return format!("<div class=\"table-wrap\" id=\"{}\">{out}</div>", esc(&id));
        }
        if is_table {
            self.tables += 1;
        } else {
            self.figures += 1;
        }
        let mut img = String::new();
        if let Some(i) = f.first(|x| x.name() == "img") {
            let mut cands = self.r.candidates(i);
            if !self.nejm {
                cands.extend(
                    f.all(|x| x.name() == "a")
                        .iter()
                        .filter_map(|a| a.attr("href"))
                        .filter(|h| h.contains("/asset/") || re!(r"\.(jpe?g|png|gif|webp)(\?|$)").is_match(h))
                        .map(String::from),
                );
            }
            if let Some(file) = self.r.image_file(&cands) {
                img = format!("<img src=\"../images/{file}\" alt=\"{}\"/>", esc(if label.is_empty() { "Figure" } else { &label }));
            } else {
                self.r.missing_images += 1;
                img = "[[missing-image]]".into();
            }
        }
        let mut note_text = String::new();
        for n in notes {
            let t = self.r.inline(n).trim().to_string();
            if !t.is_empty() {
                note_text += &format!("<span class=\"cap-p\"> {t}</span>");
            }
        }
        let caption = head + &note_text;
        let _ = &self.base;
        format!(
            "<figure id=\"{}\">{img}{}</figure>",
            esc(&id),
            if caption.is_empty() { String::new() } else { format!("<figcaption>{caption}</figcaption>") }
        )
    }
}
