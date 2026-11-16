//! Cell Press (cell.com) and The Lancet (thelancet.com) article pages: Elsevier's own journal
//! sites, separate from ScienceDirect. A port of `CellPress.swift`. Depending on the template a
//! page has Atypon's core article markup (→ `atypon`), ScienceDirect-style markup (→
//! `sciencedirect`) or the older Literatum full-text markup (→ the generic reader, with Cell Press
//! furniture removed).

use super::springer::{alive, all_in};
use super::webpage::{self, Adapter, PageImages, KEEP_ATTR};
use super::xml::Doc;
use super::{Book, Options, Result};

pub fn book(page: &str, url: Option<&url::Url>, images: &PageImages, opts: &Options) -> Result<Book> {
    if super::atypon::looks_like(page) {
        opts.log("Cell Press / Lancet page with Atypon core markup");
        return super::atypon::book(page, url, images, opts);
    }
    if super::sciencedirect::looks_like(page) {
        opts.log("Cell Press / Lancet page with ScienceDirect markup");
        return super::sciencedirect::book_with(page, None, url, Some(images), opts);
    }
    let host = url.and_then(|u| u.host_str()).unwrap_or("").replace("www.", "");
    let adapter = Adapter {
        prepare: Box::new(|doc: &mut Doc| {
            // furniture particular to these sites: the article tools, "Linked articles", the
            // "View full text / Download PDF" rows and the access box
            for x in all_in(doc, 0, &|x| {
                [
                    "article-tools", "linked-articles", "article-access-options", "loa", "article__tools", "eLetters",
                    "related-content", "article-header__info", "figure__links", "ref__links", "references__links",
                ]
                .iter()
                .any(|c| x.has_class(c))
                    || x.attr("id") == Some("comments")
            }) {
                if alive(doc, x) {
                    doc.remove(x);
                }
            }
            // in-text citations to the side panel ("#bib1" with data-db-target-for / workspace triggers): to the list
            for a in all_in(doc, 0, &|x| x.name() == "a") {
                let n = doc.node(a);
                if let Some(t) = n.attr("data-db-target-for").or(n.attr("data-xocs-content-id")).map(String::from) {
                    let h = n.attr("href").unwrap_or("");
                    if h.is_empty() || h == "#" {
                        doc.set_attr(a, "href", Some(&format!("#{t}")));
                    }
                }
            }
            super::silverchair::retitle(doc, 0);
            // reference lists: keep (some are lists of mostly linked text)
            for l in all_in(doc, 0, &|x| {
                (x.name() == "ol" || x.name() == "ul")
                    && (x.has_class("ref-list") || x.has_class("references__list") || x.has_class("references"))
            }) {
                doc.set_attr(l, KEEP_ATTR, Some("1"));
            }
        }),
        main: Box::new(|_| None),
    };
    let label = format!("Full text from {}", if host.is_empty() { "the publisher" } else { &host });
    webpage::generic(page, url, images, &label, adapter, opts)
}
