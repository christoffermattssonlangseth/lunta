import Foundation

/// Cell Press (cell.com) and The Lancet (thelancet.com) article pages: Elsevier's own journal
/// sites, separate from ScienceDirect. They run on Atypon's Literatum; depending on the template a
/// page has Atypon's core article markup (→ the Atypon reader), ScienceDirect-style markup (→ the
/// ScienceDirect reader) or the older Literatum full-text markup (→ the generic reader, with Cell
/// Press furniture removed and the parts named the way the journals name them: Highlights, eTOC
/// blurb / In brief, Graphical abstract, Summary, STAR★Methods, Research in context).
///
/// Not tested against live pages (both sites refuse automated fetching); the routing means a page
/// is read by whichever reader its markup fits, and an abstract-only page (no access) is refused.
///
/// Rust port: core/src/convert/cellpress.rs (dispatch only) when desktop / Android take pages.
enum CellPress {
    static func book(html: String, url: URL?, images: PageImages, log: @escaping (String) -> Void) async throws -> Book {
        if Atypon.looksLike(html) {
            log("Cell Press / Lancet page with Atypon core markup")
            return try await Atypon.book(html: html, url: url, images: images, log: log)
        }
        if ScienceDirect.looksLike(html) {
            log("Cell Press / Lancet page with ScienceDirect markup")
            return try await ScienceDirect.book(html: html, dir: nil, images: images, log: log)
        }
        let host = (url?.host ?? "").replacingOccurrences(of: "www.", with: "")
        let adapter = GenericPage.Adapter(prepare: { doc in
            // furniture particular to these sites: the article tools, "Linked articles", the
            // "View full text / Download PDF" rows and the access box
            for x in doc.all(where: { x in
                x.hasClass("article-tools") || x.hasClass("linked-articles") || x.hasClass("article-access-options") || x.hasClass("loa")
                    || x.hasClass("article__tools") || x.hasClass("eLetters") || x.hasClass("related-content") || x.hasClass("article-header__info")
                    || x.attr("id") == "comments" || x.hasClass("figure__links") || x.hasClass("ref__links") || x.hasClass("references__links")
            }) where x.parent != nil { x.remove() }
            // in-text citations to the side panel ("#bib1" with data-db-target-for / workspace triggers): to the list
            for a in doc.all(where: { $0.name == "a" }) {
                if let t = a.attr("data-db-target-for") ?? a.attr("data-xocs-content-id"), (a.attr("href") ?? "").isEmpty || a.attr("href") == "#" {
                    a.attrs["href"] = "#" + t
                }
            }
            Silverchair.retitle(doc)
            // reference lists: keep (some are lists of mostly linked text)
            for l in doc.all(where: { x in (x.name == "ol" || x.name == "ul") && (x.hasClass("ref-list") || x.hasClass("references__list") || x.hasClass("references")) }) {
                l.attrs[GenericPage.keepAttr] = "1"
            }
        })
        return try await GenericPage.book(html: html, url: url, images: images, label: "Full text from \(host.isEmpty ? "the publisher" : host)",
                                          adapter: adapter, log: log)
    }
}
