import Foundation

/// Article pages on Silverchair's platform: JAMA Network, Oxford Academic (academic.oup.com),
/// ASH (Blood, Blood Advances), Rockefeller University Press (JEM, JCB), AACR, MIT Press, Karger,
/// Portland Press, The Company of Biologists, … The full text sits in one widget
/// (`[data-widgetname=ArticleFulltext]`, `.widget-ArticleFulltext`; JAMA: `.article-full-text`):
///
/// - headings are flat siblings (`h2.section-title`, `h3.section-title`; OUP: `h2.abstract-title`
///   before `section.abstract`), paragraphs `p.chapter-para` (JAMA `p.para`);
/// - figures: `div.fig.fig-section[data-id=F1]` > `.graphic-wrap` (`img[data-src]` lazy, the
///   original behind `a.fig-view-orig`) + `.fig-caption` (`.fig-label`, paragraphs); JAMA:
///   `div.figure-table-image` / `.figure-section` with `.figure-table-caption`;
/// - tables: `div.table-wrap` > `.table-wrap-title` (label, `.caption`) + `table` + `.table-wrap-foot`;
/// - in-text links are `a[href="javascript:;"][reveal-id=B1]` (references, figures, tables);
///   they become `#B1` links and the targets get those ids;
/// - references: `.ref-list` > `.js-splitview-ref-item[content-id=B1]` > `.ref-content`
///   (`.label`, `.mixed-citation`), then `.citation-links` (Google Scholar, Crossref, PubMed);
///   JAMA: `.references` > `.reference[id]` > `.reference-content`;
/// - supplementary data: `.dataSuppLink` / `.supplementary-material` links.
///
/// Furniture removed: "Open in new tab / Download slide" bars, the citation link rows, the metadata
/// panel, license and copyright boxes, access messages. Then the generic reader renders it. A page
/// with only the abstract (no access) is refused.
///
/// Not tested against live pages (Silverchair sites refuse automated fetching), only against
/// pages built from the platform's documented markup; the generic reader backs it up.
///
/// Rust port: core/src/convert/silverchair.rs as an adapter for webpage.rs.
enum Silverchair {
    static func looksLike(_ html: String) -> Bool {
        html.contains("data-widgetname=\"ArticleFulltext\"") || html.contains("silverchair-cdn.com") && html.contains("chapter-para")
    }

    static func mainOf(_ doc: XNode) -> XNode? {
        doc.first { $0.attr("data-widgetname") == "ArticleFulltext" }
            ?? doc.first { $0.hasClass("widget-ArticleFulltext") }
            ?? doc.first { $0.hasClass("article-full-text") }
    }

    static func book(html: String, url: URL?, images: PageImages, log: @escaping (String) -> Void) async throws -> Book {
        let host = (url?.host ?? "").replacingOccurrences(of: "www.", with: "")
        let label = "Full text from \(host.isEmpty ? "the publisher" : host)"
        var found = false
        let adapter = GenericPage.Adapter(prepare: { doc in
            guard let main = mainOf(doc) else { return }
            found = true
            prepare(main)
        }, main: { body in
            mainOf(body)
        })
        let book = try await GenericPage.book(html: html, url: url, images: images, label: label, adapter: adapter, log: log)
        if !found { log("No Silverchair full-text widget; read generically") }
        return book
    }

    static let furniture: Set<String> = ["open in new tab", "download slide", "view large", "view large image", "download powerpoint",
                                         "close modal", "view original", "download figure", "download table", "open table in new tab",
                                         "view in article", "google scholar", "crossref", "pubmed", "search admin", "search ads",
                                         "web of science", "worldcat", "pubmed central", "close"]

    static func prepare(_ main: XNode) {
        // furniture
        for x in main.all(where: { x in
            x.hasClass("graphic-bottom") || x.hasClass("citation-links") || x.hasClass("article-metadata-panel") || x.hasClass("copyright")
                || x.hasClass("license") || x.hasClass("article-access-message") || x.hasClass("access-message") || x.hasClass("table-modal-links")
                || x.hasClass("ref-links") || x.hasClass("reference-links") || x.hasClass("kwd-group") || x.hasClass("pub-history-wrap")
                || x.hasClass("related-articles") || x.hasClass("article-tools") || x.hasClass("figure-table-links") || x.hasClass("cme-alert")
        }) where x.parent != nil { x.remove() }
        for x in main.all(where: { x in (x.name == "a" || x.name == "button" || x.name == "span" || x.name == "div")
            && furniture.contains(x.plain.lowercased()) }) where x.parent != nil { x.remove() }

        // javascript links → #id links
        for a in main.all(where: { $0.name == "a" }) {
            guard let target = a.attr("reveal-id") ?? a.attr("data-open") ?? a.attr("data-modal-source-id") else { continue }
            let h = a.attr("href") ?? ""
            if h.isEmpty || h.hasPrefix("javascript") || h == "#" { a.attrs["href"] = "#" + target }
        }

        // figures
        let figs = main.all(where: { x in
            (x.hasClass("fig-section") || (x.hasClass("fig") && x.attr("data-id") != nil) || x.hasClass("figure-section") || x.hasClass("figure-table-image"))
                && x.firstDescendant("img") != nil && x.firstDescendant("table") == nil
        })
        for f in figs where f.parent != nil && !f.ancestors().contains(where: { a in figs.contains { $0 === a } }) {
            figure(f)
        }
        // tables: one copy each (some pages repeat a table in a viewer)
        var seen = Set<String>()
        for t in main.all(where: { $0.hasClass("table-wrap") || $0.hasClass("figure-table") && $0.firstDescendant("table") != nil }) where t.parent != nil {
            let id = t.attr("data-id") ?? t.first(where: { $0.attr("data-id") != nil })?.attr("data-id") ?? t.attr("id") ?? ""
            if !id.isEmpty, seen.contains(id) { t.remove(); continue }
            if !id.isEmpty { seen.insert(id); if t.attr("id") == nil { t.attrs["id"] = id } }
            // the id may sit on the title: move it to the wrap (links point at it)
            for x in t.all(where: { $0.attr("id") == id && $0 !== t }) { x.attrs["id"] = nil }
            t.attrs["id"] = id.isEmpty ? nil : id
        }
        // the modal wrapper around a table: only a wrapper (its name would make it look like a popup)
        for m in main.all(where: { $0.hasClass("table-modal") }) where m.parent != nil {
            if m.firstDescendant("table") == nil { m.remove() } else { m.replace(with: m.children) }
        }
        retitle(main)

        references(main)
        // supplementary files: keep their list of links
        for x in main.all(where: { $0.hasClass("dataSuppLink") || $0.hasClass("supplementary-material") || $0.hasClass("supplementary-materials") }) {
            x.attrs[GenericPage.keepAttr] = "1"
            for l in x.all(where: { $0.name == "ul" || $0.name == "ol" }) { l.attrs[GenericPage.keepAttr] = "1" }
        }
    }

    /// Headings set in capitals ("MATERIALS AND METHODS") in title case.
    static func retitle(_ main: XNode) {
        for h in main.all(where: GenericPage.isHeading) {
            let t = h.plain, tc = GenericPage.titleCase(t)
            if tc != t { h.children.forEach { $0.parent = nil }; h.children = []; h.append([.textNode(tc)]) }
        }
    }

    /// A figure as `<figure id><img/><figcaption>label. caption</figcaption></figure>`.
    static func figure(_ f: XNode) {
        let id = f.attr("data-id") ?? f.attr("id") ?? ""
        let capEl = f.first { $0.hasClass("fig-caption") || $0.hasClass("figure-table-caption") || $0.name == "figcaption" }
        var imgs: [XNode] = []
        for img in f.all(where: { $0.name == "img" }) where !(capEl.map { c in img.ancestors().contains { $0 === c } } ?? false) {
            // the original (behind "fig-view-orig") as the first address to look up
            if let orig = img.ancestors().first(where: { $0.name == "a" })?.attr("href") ?? f.first(where: { $0.hasClass("fig-view-orig") })?.attr("href"),
               !orig.hasPrefix("javascript") {
                img.attrs["data-large-src"] = orig
            }
            if let s = img.attr("src"), s.hasPrefix("data:") { img.attrs["src"] = img.attr("data-src") ?? "" }
            imgs.append(img)
        }
        var cap: [XNode] = []
        if let capEl {
            for c in capEl.children {
                if c.isText { cap.append(c); continue }
                if c.hasClass("fig-label") || c.hasClass("label") || c.hasClass("title-label") || c.hasClass("table-label") {
                    cap.append(XNode.element("b", [:], [.textNode(c.plain.trimmingCharacters(in: .whitespacesAndNewlines) + " ")]))
                } else {
                    cap.append(XNode.element("div", [:], [c]))
                }
            }
        }
        var kids: [XNode] = imgs
        if !cap.isEmpty { kids.append(XNode.element("figcaption", [:], cap)) }
        f.replace(with: [XNode.element("figure", id.isEmpty ? [:] : ["id": id], kids)])
    }

    /// The reference list as `<h2>References</h2><ol><li id="B1">…</li></ol>` (a heading added
    /// when the page's is not a top-level heading of its own).
    static func references(_ main: XNode) {
        let lists = main.all(where: { $0.hasClass("ref-list") || $0.hasClass("references") }).filter { l in
            !l.ancestors().contains { $0.hasClass("ref-list") || $0.hasClass("references") }
        }
        for list in lists {
            var items: [XNode] = []
            let entries = list.all(where: { x in
                x.hasClass("js-splitview-ref-item") || x.hasClass("ref") || x.hasClass("reference") || (x.hasClass("ref-content") && !x.ancestors().contains { $0.hasClass("js-splitview-ref-item") })
            }).filter { e in !e.ancestors().contains { $0.hasClass("js-splitview-ref-item") || $0.hasClass("reference") || $0.hasClass("ref") } }
            for e in entries {
                let ids = [e.attr("content-id"), e.attr("data-legacy-id"), e.attr("id"),
                           e.first(where: { $0.hasClass("ref-content") })?.attr("id"), e.first(where: { $0.hasClass("ref-content") })?.attr("data-id")]
                    .compactMap { $0 }.filter { !$0.isEmpty }
                for x in e.all(where: { $0.hasClass("label") || $0.hasClass("reference-number") || $0.hasClass("citation-links") }) { x.remove() }
                let content = e.first { $0.hasClass("mixed-citation") || $0.hasClass("element-citation") || $0.hasClass("citation") || $0.hasClass("reference-content") }
                    ?? e.first { $0.hasClass("ref-content") } ?? e
                var kids: [XNode] = []
                for extra in ids.dropFirst() { kids.append(XNode.element("span", ["id": extra])) }
                kids.append(contentsOf: content.children)
                items.append(XNode.element("li", ids.first.map { ["id": $0] } ?? [:], kids))
            }
            guard !items.isEmpty else { continue }
            let ol = XNode.element("ol", [GenericPage.keepAttr: "1"], items)
            // a heading before it ("REFERENCES"), else one of our own
            var heading = false
            if let p = list.parent, let i = p.children.firstIndex(where: { $0 === list }) {
                heading = p.children[..<i].reversed().first(where: { !$0.isText }).map { GenericPage.isHeading($0) && GenericPage.Renderer.isReferences($0.plain) } ?? false
            }
            heading = heading || list.first(where: { GenericPage.isHeading($0) && GenericPage.Renderer.isReferences($0.plain) }) != nil
            let level = main.all(where: GenericPage.isHeading).compactMap { Int($0.name.dropFirst()) }.min() ?? 2
            list.replace(with: heading && list.first(where: GenericPage.isHeading) == nil ? [ol]
                         : [XNode.element("h\(level)", [:], [.textNode("References")]), ol])
        }
    }
}
