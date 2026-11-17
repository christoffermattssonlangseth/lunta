import Foundation

/// Springer Nature article pages: nature.com, link.springer.com, BMC (biomedcentral.com, now
/// mostly redirected to link.springer.com) and SpringerOpen. One markup (the "c-article" templates):
///
/// - details in `citation_*` meta tags;
/// - `div.c-article-body` holds `section[data-title]` > `div.c-article-section` > `h2` +
///   `div.c-article-section__content` (Abstract, Introduction / Main, Results, …, Methods, Data
///   availability, References, Acknowledgements, Author information, Ethics declarations, …);
///   subsections are `h3.c-article__sub-heading`;
/// - figures: `div.c-article-section__figure` > `figure` > `figcaption > b#Fig1` (label and title),
///   the image (`media.springernature.com/lw685/…_Fig1_HTML.png`) and
///   `.c-article-section__figure-description` (the legend);
/// - tables are not in the article page: `div.c-article-table` only has the caption and a link to
///   the table's own page (`/articles/…/tables/1`, with `table.data` and `.c-article-table-footer`).
///   The browser extension sends those pages along with the images; without them (an older
///   extension, a saved file) they are fetched (open-access articles), else the caption and a link
///   stand in;
/// - equations: TeX in `span.mathjax-tex` (before MathJax runs), display ones in
///   `.c-article-equation`; kept as TeX source (`code.tex`), as the JATS reader does;
/// - references: `ol.c-article-references` > `li` > `p#ref-CR1` (+ a row of service links and the
///   article's DOI); in-text citations link `#ref-CR1`.
///
/// Dropped: recommendations ("Similar content being viewed by others"), affiliations and
/// corresponding authors (the author contributions stay), peer review, publisher's note, rights and
/// permissions, "About this article", comments, the access box. Supplementary files become a short
/// list of links. A page whose body is only the abstract (no access) is refused.
///
/// Rust port: core/src/convert/springer.rs on top of webpage.rs (the generic reader with adapters).
enum SpringerNature {
    static func looksLike(_ html: String) -> Bool { html.contains("c-article-body") && html.contains("c-article-section") }

    /// Section titles that are page furniture, not article text.
    static let dropTitles: Set<String> = ["inline recommendations", "rights and permissions", "about this article", "peer review",
                                          "additional information", "comments", "access options", "change history", "similar content being viewed by others"]
    static let suppTitles: Set<String> = ["supplementary information", "source data", "electronic supplementary material", "supplementary material"]
    /// Back-matter sections (not article body) — for telling a full-text page from an abstract page.
    static let backTitles: Set<String> = ["abstract", "references", "acknowledgements", "acknowledgments", "author information",
                                          "ethics declarations", "funding", "data availability", "code availability", "notes"]

    static func book(html: String, url: URL?, images: PageImages, log: @escaping (String) -> Void) async throws -> Book {
        guard looksLike(html) else {
            log("No Springer Nature article markup; reading the page generically")
            return try await GenericPage.book(html: html, url: url, images: images, label: label(url), log: log)
        }
        // tables: their own pages (sent by the browser, else fetched)
        var tablePages: [String: String] = [:]
        for link in tableLinks(html) {
            guard let abs = URL(string: link, relativeTo: url)?.absoluteURL else { continue }
            if let page = images.page(for: [link, abs.absoluteString]) { tablePages[link] = page; continue }
            if !Paper2Epub.offline, let r = try? await Net.fetch(abs, accept: "text/html", timeout: 20), r.status == 200,
               let s = String(data: r.data, encoding: .utf8), s.contains("<table") {
                tablePages[link] = s
            }
        }
        let links = tableLinks(html)
        if !links.isEmpty { log("Tables: \(tablePages.count) of \(links.count) table page(s) available") }

        var bodyWords = 0
        let adapter = GenericPage.Adapter(prepare: { doc in
            bodyWords = prepare(doc, tablePages: tablePages)
        }, main: { body in
            body.first { $0.hasClass("c-article-body") }
        })
        do {
            let book = try await GenericPage.book(html: html, url: url, images: images, label: label(url), adapter: adapter, log: log)
            if bodyWords < 250 { throw fail(GenericPage.abstractOnly) }
            return book
        } catch {
            if bodyWords < 250 { throw fail(GenericPage.abstractOnly) }
            throw error
        }
    }

    static func label(_ url: URL?) -> String {
        let host = url?.host?.replacingOccurrences(of: "www.", with: "") ?? "Springer Nature"
        return "Full text from \(host)"
    }

    static func tableLinks(_ html: String) -> [String] {
        let re = try! NSRegularExpression(pattern: #"<a\b[^>]*data-test="table-link"[^>]*>"#)
        let ns = html as NSString
        var out: [String] = []
        for m in re.matches(in: html, range: NSRange(location: 0, length: ns.length)) {
            let tag = ns.substring(with: m.range)
            if let r = tag.range(of: #"href="([^"]+)""#, options: .regularExpression) {
                let h = HTMLDOM.decode(String(tag[r].dropFirst(6).dropLast()))
                if !out.contains(h) { out.append(h) }
            }
        }
        return Array(out.prefix(40))
    }

    /// Rewrites the page for the generic reader; returns the words of article body text (sections
    /// other than the abstract and back matter).
    static func prepare(_ doc: XNode, tablePages: [String: String]) -> Int {
        guard let body = doc.first(where: { $0.hasClass("c-article-body") }) else { return 0 }
        var bodyWords = 0
        // the access box of a page without access, and anything after it
        for x in body.all(where: { $0.hasClass("c-article-access-provider") || $0.hasClass("c-nature-box") || $0.attr("id") == "access-options" }) { x.remove() }

        for sec in body.all(where: { $0.name == "section" && $0.attr("data-title") != nil }) where sec.parent != nil {
            let title = (sec.attr("data-title") ?? "").lowercased().trimmingCharacters(in: .whitespaces)
            if sec.hasClass("c-article-recommendations") || dropTitles.contains(title) { sec.remove(); continue }
            // a figure that is its own section (data-title="Fig. 1") is not a section
            if title.hasPrefix("fig") && sec.first(where: { $0.hasClass("c-article-section__title") }) == nil { continue }
            if suppTitles.contains(title) { supplementary(sec); continue }
            if title == "author information" { authorInformation(sec); continue }
            if !backTitles.contains(title) {
                bodyWords += sec.all(where: { $0.name == "p" }).reduce(0) { $0 + GenericPage.words($1.plain) }
            }
        }
        // pieces without section headings (Comments, Primers, News & Views): their text sits in the
        // body outside any titled section; it counts too (else the page looked abstract-only)
        for p in body.all(where: { $0.name == "p" }) where p.parent != nil {
            var a = p.parent, titled = false
            while let n = a, n !== body {
                if n.name == "section", n.attr("data-title") != nil { titled = true; break }
                if n.name == "figure" || n.hasClass("c-article-section__figure") { titled = true; break }
                a = n.parent
            }
            if !titled { bodyWords += GenericPage.words(p.plain) }
        }
        // one list of supplementary files ("Supplementary information", "Source data")
        let supps = body.all(where: { $0.name == "section" && suppTitles.contains(($0.attr("data-title") ?? "").lowercased()) })
        if let first = supps.first, let ul = first.first(where: { $0.name == "ul" }) {
            for other in supps.dropFirst() {
                ul.append(other.all(where: { $0.name == "li" && $0.parent?.name == "ul" }))
                other.remove()
            }
        }
        // figures
        for f in body.all(where: { $0.hasClass("c-article-section__figure") || $0.attr("data-test") == "figure" }) where f.parent != nil {
            figure(f)
        }
        // tables (their own pages)
        for t in body.all(where: { $0.hasClass("c-article-table") }) where t.parent != nil {
            table(t, pages: tablePages)
        }
        // equations: TeX source
        for d in body.all(where: { $0.hasClass("c-article-equation") }) where d.parent != nil {
            let tex = d.all(where: { $0.hasClass("mathjax-tex") }).map { texSource($0.allText) }.joined(separator: " ")
            let num = d.first(where: { $0.hasClass("c-article-equation__number") })?.plain ?? ""
            guard !tex.isEmpty else { continue }
            var kids = [XNode.element("code", ["class": "tex-display"], [.textNode(tex)])]
            if !num.isEmpty { kids.append(.textNode("  " + num)) }
            d.replace(with: [XNode.element("p", ["class": "equation"], kids)])
        }
        for s in body.all(where: { $0.hasClass("mathjax-tex") }) where s.parent != nil {
            s.replace(with: [XNode.element("code", ["class": "tex"], [.textNode(texSource(s.allText))])])
        }
        // references: the text and the article's DOI; no rows of service links
        for li in body.all(where: { $0.hasClass("c-article-references__item") }) {
            guard let p = li.first(where: { $0.hasClass("c-article-references__text") }) else { continue }
            let doi = li.all(where: { $0.name == "a" }).compactMap { $0.attr("data-doi") }.first
            var kids: [XNode] = [p]
            if let doi, !doi.isEmpty {
                kids.append(.textNode(" "))
                kids.append(XNode.element("a", ["href": "https://doi.org/\(doi)"], [.textNode("doi:\(doi)")]))
            }
            li.children.forEach { $0.parent = nil }
            li.children = []
            li.append(kids)
        }
        for x in body.all(where: { $0.hasClass("c-article-references__download") || $0.hasClass("c-article-section__figure-link")
                || $0.attr("data-test") == "article-link" || $0.hasClass("c-article-buy-box") }) where x.parent != nil { x.remove() }
        return bodyWords
    }

    static func texSource(_ t: String) -> String {
        var s = t.trimmingCharacters(in: .whitespacesAndNewlines)
        for (a, b) in [("\\(", "\\)"), ("\\[", "\\]"), ("$$", "$$"), ("$", "$")] where s.hasPrefix(a) && s.hasSuffix(b) && s.count >= a.count + b.count {
            s = String(s.dropFirst(a.count).dropLast(b.count)); break
        }
        return s.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    /// `div.c-article-section__figure` → `<figure id="Fig1"><img/><figcaption>Fig. 1: title. Legend</figcaption></figure>`.
    static func figure(_ f: XNode) {
        let capB = f.first { $0.attr("data-test") == "figure-caption-text" || $0.hasClass("c-article-section__figure-caption") }
        let id = capB?.attr("id") ?? f.attr("id") ?? ""
        let imgs = f.all(where: { $0.name == "img" })
        let desc = f.first { $0.hasClass("c-article-section__figure-description") }
        var cap: [XNode] = []
        if let capB { cap.append(XNode.element("b", [:], capB.children)) }
        if let desc {
            for c in desc.children {
                if c.isText { cap.append(c) } else { cap.append(XNode.element("div", [:], c.name == "p" ? c.children : [c])) }
            }
        }
        var kids: [XNode] = imgs
        if !cap.isEmpty { kids.append(XNode.element("figcaption", [:], cap)) }
        f.replace(with: [XNode.element("figure", id.isEmpty ? [:] : ["id": id], kids)])
    }

    /// `div.c-article-table` → the table from its own page (caption, table, footnotes); without
    /// the page, the caption and a link to it.
    static func table(_ t: XNode, pages: [String: String]) {
        let capB = t.first { $0.attr("data-test") == "table-caption" }
        let id = capB?.attr("id") ?? t.attr("id") ?? ""
        let link = t.first { $0.attr("data-test") == "table-link" }?.attr("href") ?? ""
        if let page = pages[link] {
            let d = HTMLDOM.parse(page)
            // (the table page's own markup only: a page without access has no table container)
            if let container = d.first(where: { $0.hasClass("c-article-table-container") }), let table = container.first(where: { $0.name == "table" }) {
                // (made only now: making it moves the caption's children, which the link below needs otherwise)
                let caption = XNode.element("div", ["class": "caption"], capB.map { $0.children } ?? [])
                var kids = [caption, table]
                if let foot = d.first(where: { $0.hasClass("c-article-table-footer") }) {
                    let items = foot.all(where: { $0.name == "li" })
                    let parts: [XNode] = items.isEmpty ? foot.children : items.flatMap { [XNode.element("span", [:], $0.children), .textNode(" ")] }
                    kids.append(XNode.element("div", ["class": "table-foot"], parts))
                }
                t.replace(with: [XNode.element("div", ["class": "table-wrap", "id": id], kids)])
                return
            }
        }
        // no table: its caption and a link to it
        var kids: [XNode] = [XNode.element("strong", [:], capB?.children ?? [])]
        if !link.isEmpty {
            kids.append(.textNode(" "))
            kids.append(XNode.element("a", ["href": link], [.textNode("(table on the publisher's site)")]))
        }
        t.replace(with: [XNode.element("p", id.isEmpty ? [:] : ["id": id], kids)])
    }

    /// Supplementary files: a list of their names, linked.
    static func supplementary(_ sec: XNode) {
        guard let content = sec.first(where: { $0.hasClass("c-article-section__content") }) else { return }
        var items: [XNode] = []
        for item in content.all(where: { $0.attr("data-test") == "supp-item" || $0.hasClass("c-article-supplementary__item") }) {
            guard let a = item.first(where: { $0.name == "a" }) else { continue }
            let name = a.plain.replacingOccurrences(of: #"\s*\((download [^)]*)\)\s*$"#, with: "", options: .regularExpression)
            let kind = a.plain.range(of: #"download ([A-Za-z0-9]+)"#, options: .regularExpression).map { String(a.plain[$0].dropFirst(9)) } ?? ""
            var li: [XNode] = [XNode.element("a", ["href": a.attr("href") ?? ""], [.textNode(name)])]
            if !kind.isEmpty { li.append(.textNode(" (\(kind))")) }
            if let d = item.first(where: { $0.hasClass("c-article-supplementary__description") }) { li.append(.textNode(" — ")); li.append(contentsOf: d.children) }
            items.append(XNode.element("li", item.attr("id").map { ["id": $0] } ?? [:], li))
        }
        content.children.forEach { $0.parent = nil }
        content.children = []
        if !items.isEmpty { content.append([XNode.element("ul", [GenericPage.keepAttr: "1"], items)]) }
        else { sec.remove() }
    }

    /// Author information: only the authors' contributions (affiliations and corresponding authors
    /// are in the book's details already).
    static func authorInformation(_ sec: XNode) {
        guard let content = sec.first(where: { $0.hasClass("c-article-section__content") }),
              let title = sec.first(where: { $0.hasClass("c-article-section__title") }) else { sec.remove(); return }
        var keep: [XNode] = []
        var on = false
        for c in content.children {
            if GenericPage.isHeading(c) { on = c.attr("id") == "contributions" || c.plain.lowercased().hasPrefix("contribution"); continue }
            if on { keep.append(c) }
        }
        guard keep.contains(where: { !$0.isText }) else { sec.remove(); return }
        title.children.forEach { $0.parent = nil }
        title.children = []
        title.append([.textNode("Author contributions")])
        content.children.forEach { $0.parent = nil }
        content.children = []
        content.append(keep)
    }
}
