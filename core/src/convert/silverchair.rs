//! Article pages on Silverchair's platform: JAMA Network, Oxford Academic, ASH (Blood), Rockefeller
//! University Press, AACR, MIT Press, Karger, Portland Press, … A port of `Silverchair.swift`. The
//! full text sits in one widget (`[data-widgetname=ArticleFulltext]`, `.widget-ArticleFulltext`;
//! JAMA: `.article-full-text`), rewritten here for the generic reader (`webpage`):
//!
//! - figures: `div.fig.fig-section[data-id=F1]` (`img[data-src]` lazy, the original behind
//!   `a.fig-view-orig`) + `.fig-caption`; JAMA: `.figure-table-image` with `.figure-table-caption`;
//! - tables: `div.table-wrap` (one copy each);
//! - in-text links are `a[href="javascript:;"][reveal-id=B1]`; they become `#B1` links;
//! - references: `.ref-list` > `.js-splitview-ref-item[content-id=B1]` > `.ref-content`; JAMA:
//!   `.references` > `.reference[id]` > `.reference-content`; made one `<ol>` with a heading;
//! - supplementary data: `.dataSuppLink` / `.supplementary-material` links are kept.
//!
//! Furniture removed: "Open in new tab / Download slide" bars, citation link rows, the metadata
//! panel, license and copyright boxes, access messages.

use super::springer::{alive, all_in};
use super::webpage::{self, el, is_heading, is_references, text_node, title_case, Adapter, PageImages, KEEP_ATTR};
use super::xml::{Doc, Node, NodeId};
use super::{Book, Options, Result};
use std::collections::HashSet;

pub fn looks_like(html: &str) -> bool {
    html.contains("data-widgetname=\"ArticleFulltext\"") || (html.contains("silverchair-cdn.com") && html.contains("chapter-para"))
}

fn main_of(doc: Node) -> Option<Node> {
    doc.first(|n| n.attr("data-widgetname") == Some("ArticleFulltext"))
        .or_else(|| doc.first(|n| n.has_class("widget-ArticleFulltext")))
        .or_else(|| doc.first(|n| n.has_class("article-full-text")))
}

pub fn book(page: &str, url: Option<&url::Url>, images: &PageImages, opts: &Options) -> Result<Book> {
    let host = url.and_then(|u| u.host_str()).unwrap_or("").replace("www.", "");
    let label = format!("Full text from {}", if host.is_empty() { "the publisher" } else { &host });
    let found = std::cell::Cell::new(false);
    let adapter = Adapter {
        prepare: Box::new(|doc: &mut Doc| {
            let Some(main) = main_of(doc.root()).map(|m| m.id) else { return };
            found.set(true);
            prepare(doc, main);
        }),
        main: Box::new(|body: Node| main_of(body).map(|m| m.id)),
    };
    let book = webpage::generic(page, url, images, &label, adapter, opts)?;
    if !found.get() {
        opts.log("No Silverchair full-text widget; read generically");
    }
    Ok(book)
}

const FURNITURE: &[&str] = &[
    "open in new tab", "download slide", "view large", "view large image", "download powerpoint", "close modal",
    "view original", "download figure", "download table", "open table in new tab", "view in article", "google scholar",
    "crossref", "pubmed", "search admin", "search ads", "web of science", "worldcat", "pubmed central", "close",
];

const FURNITURE_CLASSES: &[&str] = &[
    "graphic-bottom", "citation-links", "article-metadata-panel", "copyright", "license", "article-access-message",
    "access-message", "table-modal-links", "ref-links", "reference-links", "kwd-group", "pub-history-wrap",
    "related-articles", "article-tools", "figure-table-links", "cme-alert",
];

fn prepare(doc: &mut Doc, main: NodeId) {
    // furniture
    for x in all_in(doc, main, &|x| FURNITURE_CLASSES.iter().any(|c| x.has_class(c))) {
        if alive(doc, x) {
            doc.remove(x);
        }
    }
    for x in all_in(doc, main, &|x| {
        matches!(x.name(), "a" | "button" | "span" | "div") && FURNITURE.contains(&x.plain().to_lowercase().as_str())
    }) {
        if alive(doc, x) {
            doc.remove(x);
        }
    }
    // javascript links → #id links
    for a in all_in(doc, main, &|x| x.name() == "a") {
        let n = doc.node(a);
        let Some(target) = n.attr("reveal-id").or(n.attr("data-open")).or(n.attr("data-modal-source-id")).map(String::from)
        else {
            continue;
        };
        let h = n.attr("href").unwrap_or("");
        if h.is_empty() || h.starts_with("javascript") || h == "#" {
            doc.set_attr(a, "href", Some(&format!("#{target}")));
        }
    }
    // figures
    let figs = all_in(doc, main, &|x| {
        (x.has_class("fig-section")
            || (x.has_class("fig") && x.attr("data-id").is_some())
            || x.has_class("figure-section")
            || x.has_class("figure-table-image"))
            && x.first_descendant("img").is_some()
            && x.first_descendant("table").is_none()
    });
    let fig_set: HashSet<NodeId> = figs.iter().copied().collect();
    for &f in &figs {
        if alive(doc, f) && !doc.node(f).has_ancestor(|a| fig_set.contains(&a.id)) {
            figure(doc, f);
        }
    }
    // tables: one copy each (some pages repeat a table in a viewer)
    let mut seen = HashSet::new();
    for t in all_in(doc, main, &|x| x.has_class("table-wrap") || (x.has_class("figure-table") && x.first_descendant("table").is_some()))
    {
        if !alive(doc, t) {
            continue;
        }
        let n = doc.node(t);
        let id = n
            .attr("data-id")
            .or_else(|| n.first(|x| x.attr("data-id").is_some()).and_then(|x| x.attr("data-id")))
            .or(n.attr("id"))
            .unwrap_or("")
            .to_string();
        if !id.is_empty() && seen.contains(&id) {
            doc.remove(t);
            continue;
        }
        if !id.is_empty() {
            seen.insert(id.clone());
            if doc.node(t).attr("id").is_none() {
                doc.set_attr(t, "id", Some(&id));
            }
        }
        // the id may sit on the title: move it to the wrap (links point at it)
        for x in all_in(doc, t, &|x| x.attr("id") == Some(id.as_str())) {
            doc.set_attr(x, "id", None);
        }
        doc.set_attr(t, "id", if id.is_empty() { None } else { Some(&id) });
    }
    // the modal wrapper around a table: only a wrapper (its name would make it look like a popup)
    for m in all_in(doc, main, &|x| x.has_class("table-modal")) {
        if !alive(doc, m) {
            continue;
        }
        if doc.node(m).first_descendant("table").is_none() {
            doc.remove(m);
        } else {
            let kids = doc.child_ids(m);
            doc.replace(m, &kids);
        }
    }
    retitle(doc, main);
    references(doc, main);
    // supplementary files: keep their list of links
    for x in all_in(doc, main, &|x| {
        x.has_class("dataSuppLink") || x.has_class("supplementary-material") || x.has_class("supplementary-materials")
    }) {
        doc.set_attr(x, KEEP_ATTR, Some("1"));
        for l in all_in(doc, x, &|y| y.name() == "ul" || y.name() == "ol") {
            doc.set_attr(l, KEEP_ATTR, Some("1"));
        }
    }
}

/// Headings set in capitals ("MATERIALS AND METHODS") in title case.
pub(crate) fn retitle(doc: &mut Doc, root: NodeId) {
    for h in all_in(doc, root, &|x| is_heading(x)) {
        let t = doc.node(h).plain();
        let tc = title_case(&t);
        if tc != t {
            doc.clear_children(h);
            let x = text_node(doc, &tc);
            doc.append(h, &[x]);
        }
    }
}

/// A figure as `<figure id><img/><figcaption>label. caption</figcaption></figure>`.
fn figure(doc: &mut Doc, f: NodeId) {
    let n = doc.node(f);
    let id = n.attr("data-id").or(n.attr("id")).unwrap_or("").to_string();
    let cap_el = n.first(|x| x.has_class("fig-caption") || x.has_class("figure-table-caption") || x.name() == "figcaption").map(|x| x.id);
    let orig_link = n.first(|x| x.has_class("fig-view-orig")).and_then(|x| x.attr("href")).map(String::from);
    let mut imgs = Vec::new();
    for img in all_in(doc, f, &|x| x.name() == "img") {
        if let Some(c) = cap_el {
            if doc.node(img).has_ancestor(|a| a.id == c) {
                continue;
            }
        }
        // the original (behind "fig-view-orig") as the first address to look up
        let i = doc.node(img);
        let orig = i.ancestors().into_iter().find(|a| a.name() == "a").and_then(|a| a.attr("href")).map(String::from).or(orig_link.clone());
        let src_data = i.attr("src").map(|s| s.starts_with("data:")).unwrap_or(false);
        let data_src = i.attr("data-src").unwrap_or("").to_string();
        if let Some(o) = orig.filter(|o| !o.starts_with("javascript")) {
            doc.set_attr(img, "data-large-src", Some(&o));
        }
        if src_data {
            doc.set_attr(img, "src", Some(&data_src));
        }
        imgs.push(img);
    }
    let mut cap = Vec::new();
    if let Some(c) = cap_el {
        for k in doc.child_ids(c) {
            let kn = doc.node(k);
            if kn.is_text() {
                cap.push(k);
                continue;
            }
            if kn.has_class("fig-label") || kn.has_class("label") || kn.has_class("title-label") || kn.has_class("table-label") {
                let t = format!("{} ", kn.plain().trim());
                let tx = text_node(doc, &t);
                cap.push(el(doc, "b", &[], &[tx]));
            } else {
                cap.push(el(doc, "div", &[], &[k]));
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

fn is_list(x: Node) -> bool {
    x.has_class("ref-list") || x.has_class("references")
}

/// The reference list as `<h2>References</h2><ol><li id="B1">…</li></ol>` (a heading added when
/// the page's is not a top-level heading of its own).
fn references(doc: &mut Doc, main: NodeId) {
    let lists: Vec<NodeId> = all_in(doc, main, &|x| is_list(x) && !x.has_ancestor(is_list));
    for list in lists {
        let entries: Vec<NodeId> = all_in(doc, list, &|x| {
            x.has_class("js-splitview-ref-item")
                || x.has_class("ref")
                || x.has_class("reference")
                || (x.has_class("ref-content") && !x.has_ancestor(|a| a.has_class("js-splitview-ref-item")))
        })
        .into_iter()
        .filter(|&e| {
            !doc.node(e).has_ancestor(|a| a.has_class("js-splitview-ref-item") || a.has_class("reference") || a.has_class("ref"))
        })
        .collect();
        let mut items = Vec::new();
        for e in entries {
            let n = doc.node(e);
            let rc = n.first(|x| x.has_class("ref-content"));
            let ids: Vec<String> = [n.attr("content-id"), n.attr("data-legacy-id"), n.attr("id"), rc.and_then(|r| r.attr("id")), rc.and_then(|r| r.attr("data-id"))]
                .into_iter()
                .flatten()
                .filter(|i| !i.is_empty())
                .map(String::from)
                .collect();
            for x in all_in(doc, e, &|x| x.has_class("label") || x.has_class("reference-number") || x.has_class("citation-links")) {
                doc.remove(x);
            }
            let n = doc.node(e);
            let content = n
                .first(|x| {
                    x.has_class("mixed-citation") || x.has_class("element-citation") || x.has_class("citation") || x.has_class("reference-content")
                })
                .or_else(|| n.first(|x| x.has_class("ref-content")))
                .map(|x| x.id)
                .unwrap_or(e);
            let mut kids = Vec::new();
            for extra in ids.iter().skip(1) {
                kids.push(el(doc, "span", &[("id", extra)], &[]));
            }
            kids.extend(doc.child_ids(content));
            let attrs: Vec<(&str, &str)> = ids.first().map(|i| vec![("id", i.as_str())]).unwrap_or_default();
            items.push(el(doc, "li", &attrs, &kids));
        }
        if items.is_empty() {
            continue;
        }
        let ol = el(doc, "ol", &[(KEEP_ATTR, "1")], &items);
        // a heading before it ("REFERENCES"), else one of our own
        let l = doc.node(list);
        let mut heading = false;
        if let (Some(p), Some(i)) = (l.parent(), l.index_in_parent()) {
            heading = p
                .children()
                .take(i)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .find(|c| !c.is_text())
                .map(|c| is_heading(c) && is_references(&c.plain()))
                .unwrap_or(false);
        }
        heading = heading || l.first(|x| is_heading(x) && is_references(&x.plain())).is_some();
        let level = doc
            .node(main)
            .all(is_heading)
            .iter()
            .filter_map(|h| h.name()[1..].parse::<i64>().ok())
            .min()
            .unwrap_or(2);
        let has_heading = l.first(is_heading).is_some();
        if heading && !has_heading {
            doc.replace(list, &[ol]);
        } else {
            let t = text_node(doc, "References");
            let h = el(doc, &format!("h{level}"), &[], &[t]);
            doc.replace(list, &[h, ol]);
        }
    }
}
