//! Springer Nature article pages: nature.com, link.springer.com, BMC and SpringerOpen (the
//! "c-article" templates). A port of `SpringerNature.swift`: the page is rewritten for the
//! generic reader (`webpage`):
//!
//! - `div.c-article-body` holds `section[data-title]` > `div.c-article-section` > `h2` +
//!   `div.c-article-section__content`; subsections are `h3.c-article__sub-heading`;
//! - figures: `div.c-article-section__figure` > `figure` > `figcaption > b#Fig1`, the image and
//!   `.c-article-section__figure-description` (the legend);
//! - tables are not in the article page: `div.c-article-table` only has the caption and a link to
//!   the table's own page. The browser extension sends those pages along with the images; without
//!   them they are fetched (unless offline), else the caption and a link stand in;
//! - equations: TeX in `span.mathjax-tex`, kept as TeX source (`code.tex`);
//! - references: `ol.c-article-references` > `li` > `p#ref-CR1` (+ service links and the DOI).
//!
//! Dropped: recommendations, affiliations and corresponding authors (the author contributions
//! stay), peer review, publisher's note, rights and permissions, "About this article", comments,
//! the access box. Supplementary files become a short list of links. A page whose body is only
//! the abstract (no access) is refused.

use super::webpage::{self, el, is_heading, re, text_node, words, Adapter, PageImages, KEEP_ATTR};
use super::xml::{Doc, Node, NodeId};
use super::{fail, html, net, Book, Options, Result};
use std::cell::Cell;
use std::collections::HashMap;

pub fn looks_like(html: &str) -> bool {
    html.contains("c-article-body") && html.contains("c-article-section")
}

/// Section titles that are page furniture, not article text.
const DROP_TITLES: &[&str] = &[
    "inline recommendations", "rights and permissions", "about this article", "peer review", "additional information",
    "comments", "access options", "change history", "similar content being viewed by others",
];
const SUPP_TITLES: &[&str] =
    &["supplementary information", "source data", "electronic supplementary material", "supplementary material"];
/// Back-matter sections (not article body) — for telling a full-text page from an abstract page.
const BACK_TITLES: &[&str] = &[
    "abstract", "references", "acknowledgements", "acknowledgments", "author information", "ethics declarations", "funding",
    "data availability", "code availability", "notes",
];

fn label(url: Option<&url::Url>) -> String {
    let host = url.and_then(|u| u.host_str()).map(|h| h.replace("www.", "")).unwrap_or_else(|| "Springer Nature".into());
    format!("Full text from {host}")
}

pub fn book(page: &str, url: Option<&url::Url>, images: &PageImages, opts: &Options) -> Result<Book> {
    if !looks_like(page) {
        opts.log("No Springer Nature article markup; reading the page generically");
        return webpage::generic(page, url, images, &label(url), Adapter::none(), opts);
    }
    // tables: their own pages (sent by the browser, else fetched)
    let links = table_links(page);
    let mut table_pages: HashMap<String, String> = HashMap::new();
    for link in &links {
        let abs = match url {
            Some(u) => u.join(link).ok(),
            None => url::Url::parse(link).ok(),
        };
        let Some(abs) = abs else { continue };
        if let Some(p) = images.page(&[link, abs.as_str()]) {
            table_pages.insert(link.clone(), p);
            continue;
        }
        if !opts.offline {
            if let Ok(r) = net::fetch(abs.as_str(), Some("text/html"), 20.0) {
                if r.status == 200 {
                    if let Ok(s) = String::from_utf8(r.data) {
                        if s.contains("<table") {
                            table_pages.insert(link.clone(), s);
                        }
                    }
                }
            }
        }
    }
    if !links.is_empty() {
        opts.log(&format!("Tables: {} of {} table page(s) available", table_pages.len(), links.len()));
    }
    let body_words = Cell::new(0usize);
    let adapter = Adapter {
        prepare: Box::new(|doc: &mut Doc| body_words.set(prepare(doc, &table_pages))),
        main: Box::new(|body: Node| body.first(|n| n.has_class("c-article-body")).map(|n| n.id)),
    };
    let r = webpage::generic(page, url, images, &label(url), adapter, opts);
    if body_words.get() < 250 {
        return Err(fail(webpage::ABSTRACT_ONLY));
    }
    r
}

fn table_links(html: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for m in re!(r#"<a\b[^>]*data-test="table-link"[^>]*>"#).find_iter(html) {
        if let Some(c) = re!(r#"href="([^"]+)""#).captures(m.as_str()) {
            let h = html::decode(&c[1]);
            if !out.contains(&h) {
                out.push(h);
            }
        }
    }
    out.truncate(40);
    out
}

fn ids(v: Vec<Node>) -> Vec<NodeId> {
    v.into_iter().map(|n| n.id).collect()
}

pub(crate) fn all_in(doc: &Doc, root: NodeId, pred: &dyn Fn(Node) -> bool) -> Vec<NodeId> {
    ids(doc.node(root).all(pred))
}

pub(crate) fn alive(doc: &Doc, id: NodeId) -> bool {
    doc.node(id).parent().is_some()
}

/// Rewrites the page for the generic reader; returns the words of article body text (sections
/// other than the abstract and back matter).
fn prepare(doc: &mut Doc, table_pages: &HashMap<String, String>) -> usize {
    let Some(body) = doc.root().first(|n| n.has_class("c-article-body")).map(|n| n.id) else { return 0 };
    let all = |doc: &Doc, pred: &dyn Fn(Node) -> bool| -> Vec<NodeId> { all_in(doc, body, pred) };
    let mut body_words = 0;
    // the access box of a page without access
    for x in all(doc, &|x| x.has_class("c-article-access-provider") || x.has_class("c-nature-box") || x.attr("id") == Some("access-options"))
    {
        doc.remove(x);
    }
    for sec in all(doc, &|x| x.name() == "section" && x.attr("data-title").is_some()) {
        if !alive(doc, sec) {
            continue;
        }
        let s = doc.node(sec);
        let title = s.attr("data-title").unwrap_or("").to_lowercase().trim_matches(|c: char| c == ' ' || c == '\t').to_string();
        if s.has_class("c-article-recommendations") || DROP_TITLES.contains(&title.as_str()) {
            doc.remove(sec);
            continue;
        }
        // a figure that is its own section (data-title="Fig. 1") is not a section
        if title.starts_with("fig") && s.first(|x| x.has_class("c-article-section__title")).is_none() {
            continue;
        }
        if SUPP_TITLES.contains(&title.as_str()) {
            supplementary(doc, sec);
            continue;
        }
        if title == "author information" {
            author_information(doc, sec);
            continue;
        }
        if !BACK_TITLES.contains(&title.as_str()) {
            body_words += s.all(|x| x.name() == "p").iter().map(|p| words(&p.plain())).sum::<usize>();
        }
    }
    // one list of supplementary files ("Supplementary information", "Source data")
    let supps = all(doc, &|x| {
        x.name() == "section" && SUPP_TITLES.contains(&x.attr("data-title").unwrap_or("").to_lowercase().as_str())
    });
    if let Some(&first) = supps.first() {
        if let Some(ul) = doc.node(first).first(|x| x.name() == "ul").map(|n| n.id) {
            for &other in &supps[1..] {
                let lis = ids(doc.node(other).all(|x| x.name() == "li" && x.parent().map(|p| p.name()) == Some("ul")));
                doc.append(ul, &lis);
                doc.remove(other);
            }
        }
    }
    // figures
    for f in all(doc, &|x| x.has_class("c-article-section__figure") || x.attr("data-test") == Some("figure")) {
        if alive(doc, f) {
            figure(doc, f);
        }
    }
    // tables (their own pages)
    for t in all(doc, &|x| x.has_class("c-article-table")) {
        if alive(doc, t) {
            table(doc, t, table_pages);
        }
    }
    // equations: TeX source
    for d in all(doc, &|x| x.has_class("c-article-equation")) {
        if !alive(doc, d) {
            continue;
        }
        let n = doc.node(d);
        let tex = n.all(|x| x.has_class("mathjax-tex")).iter().map(|x| tex_source(&x.all_text())).collect::<Vec<_>>().join(" ");
        let num = n.first(|x| x.has_class("c-article-equation__number")).map(|x| x.plain()).unwrap_or_default();
        if tex.is_empty() {
            continue;
        }
        let t = text_node(doc, &tex);
        let mut kids = vec![el(doc, "code", &[("class", "tex-display")], &[t])];
        if !num.is_empty() {
            kids.push(text_node(doc, &format!("  {num}")));
        }
        let p = el(doc, "p", &[("class", "equation")], &kids);
        doc.replace(d, &[p]);
    }
    for s in all(doc, &|x| x.has_class("mathjax-tex")) {
        if !alive(doc, s) {
            continue;
        }
        let tex = tex_source(&doc.node(s).all_text());
        let t = text_node(doc, &tex);
        let c = el(doc, "code", &[("class", "tex")], &[t]);
        doc.replace(s, &[c]);
    }
    // references: the text and the article's DOI; no rows of service links
    for li in all(doc, &|x| x.has_class("c-article-references__item")) {
        let n = doc.node(li);
        let Some(p) = n.first(|x| x.has_class("c-article-references__text")).map(|x| x.id) else { continue };
        let doi = n.all(|x| x.name() == "a").iter().find_map(|a| a.attr("data-doi")).map(String::from);
        let mut kids = vec![p];
        if let Some(doi) = doi.filter(|d| !d.is_empty()) {
            kids.push(text_node(doc, " "));
            let t = text_node(doc, &format!("doi:{doi}"));
            kids.push(el(doc, "a", &[("href", &format!("https://doi.org/{doi}"))], &[t]));
        }
        doc.clear_children(li);
        doc.append(li, &kids);
    }
    for x in all(doc, &|x| {
        x.has_class("c-article-references__download")
            || x.has_class("c-article-section__figure-link")
            || x.attr("data-test") == Some("article-link")
            || x.has_class("c-article-buy-box")
    }) {
        if alive(doc, x) {
            doc.remove(x);
        }
    }
    body_words
}

fn tex_source(t: &str) -> String {
    let mut s = t.trim().to_string();
    for (a, b) in [("\\(", "\\)"), ("\\[", "\\]"), ("$$", "$$"), ("$", "$")] {
        if s.starts_with(a) && s.ends_with(b) && s.len() >= a.len() + b.len() {
            s = s[a.len()..s.len() - b.len()].to_string();
            break;
        }
    }
    s.trim().to_string()
}

/// `div.c-article-section__figure` → `<figure id="Fig1"><img/><figcaption>Fig. 1: title. Legend</figcaption></figure>`.
fn figure(doc: &mut Doc, f: NodeId) {
    let n = doc.node(f);
    let cap_b = n
        .first(|x| x.attr("data-test") == Some("figure-caption-text") || x.has_class("c-article-section__figure-caption"))
        .map(|x| x.id);
    let id = cap_b.and_then(|c| doc.node(c).attr("id")).or(n.attr("id")).unwrap_or("").to_string();
    let imgs = ids(n.all(|x| x.name() == "img"));
    let desc = n.first(|x| x.has_class("c-article-section__figure-description")).map(|x| x.id);
    let mut cap: Vec<NodeId> = Vec::new();
    if let Some(c) = cap_b {
        let kids = doc.child_ids(c);
        cap.push(el(doc, "b", &[], &kids));
    }
    if let Some(d) = desc {
        for c in doc.child_ids(d) {
            let cn = doc.node(c);
            if cn.is_text() {
                cap.push(c);
            } else {
                let kids = if cn.name() == "p" { doc.child_ids(c) } else { vec![c] };
                cap.push(el(doc, "div", &[], &kids));
            }
        }
    }
    let mut kids = imgs;
    if !cap.is_empty() {
        kids.push(el(doc, "figcaption", &[], &cap));
    }
    let attrs: Vec<(&str, &str)> = if id.is_empty() { vec![] } else { vec![("id", id.as_str())] };
    let fig = el(doc, "figure", &attrs, &kids);
    doc.replace(f, &[fig]);
}

/// `div.c-article-table` → the table from its own page (caption, table, footnotes); without the
/// page, the caption and a link to it.
fn table(doc: &mut Doc, t: NodeId, pages: &HashMap<String, String>) {
    let n = doc.node(t);
    let cap_b = n.first(|x| x.attr("data-test") == Some("table-caption")).map(|x| x.id);
    let id = cap_b.and_then(|c| doc.node(c).attr("id")).or(n.attr("id")).unwrap_or("").to_string();
    let link = n.first(|x| x.attr("data-test") == Some("table-link")).and_then(|x| x.attr("href")).unwrap_or("").to_string();
    let cap_kids = cap_b.map(|c| doc.child_ids(c)).unwrap_or_default();
    if let Some(page) = pages.get(&link) {
        let d = html::parse(page);
        // (the table page's own markup only: a page without access has no table container)
        let container = d.root().first(|x| x.has_class("c-article-table-container"));
        if let Some(tbl) = container.and_then(|c| c.first(|x| x.name() == "table")) {
            let caption = el(doc, "div", &[("class", "caption")], &cap_kids);
            let mut kids = vec![caption, doc.import(tbl)];
            if let Some(foot) = d.root().first(|x| x.has_class("c-article-table-footer")) {
                let items = foot.all(|x| x.name() == "li");
                let mut parts = Vec::new();
                if items.is_empty() {
                    for c in foot.children() {
                        parts.push(doc.import(c));
                    }
                } else {
                    for li in items {
                        let ks: Vec<NodeId> = li.children().map(|c| doc.import(c)).collect();
                        parts.push(el(doc, "span", &[], &ks));
                        parts.push(text_node(doc, " "));
                    }
                }
                kids.push(el(doc, "div", &[("class", "table-foot")], &parts));
            }
            let wrap = el(doc, "div", &[("class", "table-wrap"), ("id", &id)], &kids);
            doc.replace(t, &[wrap]);
            return;
        }
    }
    // no table: its caption and a link to it
    let mut kids = vec![el(doc, "strong", &[], &cap_kids)];
    if !link.is_empty() {
        kids.push(text_node(doc, " "));
        let tx = text_node(doc, "(table on the publisher's site)");
        kids.push(el(doc, "a", &[("href", &link)], &[tx]));
    }
    let attrs: Vec<(&str, &str)> = if id.is_empty() { vec![] } else { vec![("id", id.as_str())] };
    let p = el(doc, "p", &attrs, &kids);
    doc.replace(t, &[p]);
}

/// Supplementary files: a list of their names, linked.
fn supplementary(doc: &mut Doc, sec: NodeId) {
    let Some(content) = doc.node(sec).first(|x| x.has_class("c-article-section__content")).map(|x| x.id) else { return };
    let mut items = Vec::new();
    for item in ids(
        doc.node(content)
            .all(|x| x.attr("data-test") == Some("supp-item") || x.has_class("c-article-supplementary__item")),
    ) {
        let it = doc.node(item);
        let Some(a) = it.first(|x| x.name() == "a") else { continue };
        let plain = a.plain();
        let name = re!(r"\s*\((download [^)]*)\)\s*$").replace(&plain, "").into_owned();
        let kind = re!(r"download ([A-Za-z0-9]+)").find(&plain).map(|m| m.as_str()[9..].to_string()).unwrap_or_default();
        let href = a.attr("href").unwrap_or("").to_string();
        let item_id = it.attr("id").map(String::from);
        let d = it.first(|x| x.has_class("c-article-supplementary__description")).map(|x| x.id);
        let tx = text_node(doc, &name);
        let mut li = vec![el(doc, "a", &[("href", &href)], &[tx])];
        if !kind.is_empty() {
            li.push(text_node(doc, &format!(" ({kind})")));
        }
        if let Some(d) = d {
            li.push(text_node(doc, " — "));
            li.extend(doc.child_ids(d));
        }
        let attrs: Vec<(&str, &str)> = item_id.as_deref().map(|i| vec![("id", i)]).unwrap_or_default();
        items.push(el(doc, "li", &attrs, &li));
    }
    doc.clear_children(content);
    if !items.is_empty() {
        let ul = el(doc, "ul", &[(KEEP_ATTR, "1")], &items);
        doc.append(content, &[ul]);
    } else {
        doc.remove(sec);
    }
}

/// Author information: only the authors' contributions (affiliations and corresponding authors
/// are in the book's details already).
fn author_information(doc: &mut Doc, sec: NodeId) {
    let s = doc.node(sec);
    let (Some(content), Some(title)) = (
        s.first(|x| x.has_class("c-article-section__content")).map(|x| x.id),
        s.first(|x| x.has_class("c-article-section__title")).map(|x| x.id),
    ) else {
        doc.remove(sec);
        return;
    };
    let mut keep = Vec::new();
    let mut on = false;
    for c in doc.node(content).children() {
        if is_heading(c) {
            on = c.attr("id") == Some("contributions") || c.plain().to_lowercase().starts_with("contribution");
            continue;
        }
        if on {
            keep.push(c.id);
        }
    }
    if !keep.iter().any(|&k| !doc.node(k).is_text()) {
        doc.remove(sec);
        return;
    }
    doc.clear_children(title);
    let t = text_node(doc, "Author contributions");
    doc.append(title, &[t]);
    doc.clear_children(content);
    doc.append(content, &keep);
}
