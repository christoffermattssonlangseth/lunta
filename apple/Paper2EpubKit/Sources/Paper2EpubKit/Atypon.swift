import Foundation

/// NEJM article pages (nejm.org/doi/full/…), as the browser shows them to a reader with access, or
/// saved from Safari (.webarchive). NEJM uses Atypon's "core" article markup:
///
/// - details in `dc.*` meta tags (no `citation_doi`: the DOI comes from the canonical link / URL);
/// - `section#summary-abstract` (Background, Methods, Results, Conclusions, then the funding /
///   trial-registration line);
/// - `section#bodymatter`: the introduction as unheaded paragraphs, then `section`s with h2
///   (Methods, Results, Discussion) and nested sections with h3; paragraphs are
///   `div[role=paragraph]`; figures and tables are `div.figure-wrap` > `figure#f1.graphic` /
///   `figure#t1.table` (a real `<table>` plus an image of it) with `figcaption` > `.caption`, `.notes`;
/// - `section#backmatter`: `#backnotes` (Notes), `#supplementary-materials`, `#bibliography`
///   (`.biblioentry` > `#r1` > `.citation-content`); in-text citations link to the copy in the
///   side pane (`#core-collateral-r1`) and are pointed at the list here.
///
/// Everything else (navigation, the side panes, ads, metrics, "cited by", author pane) is never
/// read. Pages without this markup go to the generic reader.
///
/// Rust port: core/src/convert/nejm.rs (with webpage.rs), when desktop / Android take pages.
enum NEJM {
    static func looksLike(_ doc: XNode) -> Bool {
        doc.all(where: { $0.name == "section" && $0.attr("id") == "bodymatter" }).first != nil
    }

    static func book(html: String, url: URL?, images: PageImages, log: @escaping (String) -> Void) async throws -> Book {
        let doc = HTMLDOM.parse(html)
        guard looksLike(doc) else {
            log("No NEJM article markup; reading the page generically")
            return try await GenericPage.book(html: html, url: url, images: images, label: "NEJM full text", log: log)
        }
        log("Converting the NEJM page")
        func byID(_ id: String) -> XNode? { doc.all(where: { $0.attr("id") == id }).first }

        // in-text citations point at the side pane's copy of the list: point them at ours
        var targets = Set<String>()
        for a in doc.all(where: { $0.name == "a" }) {
            guard var h = a.attr("href"), h.hasPrefix("#") else { continue }
            if h.hasPrefix("#core-collateral-r") { h = "#" + h.dropFirst("#core-collateral-".count); a.attrs["href"] = h }
            targets.insert(String(h.dropFirst()))
        }
        var meta = metadata(doc, url: url)
        let base = url ?? URL(string: "https://www.nejm.org/")
        let r = GenericPage.Renderer(images: images, base: base, title: meta.title)
        r.targets = targets
        let conv = Converter(r: r, base: base)

        var chapters: [Chapter] = []
        // abstract
        if let abs = byID("summary-abstract") {
            var heads = [Heading(level: 1, id: r.uniqueID("abstract"), title: "Abstract")]
            var html = ""
            for c in abs.elements where c.name != "h2" { html += conv.blocks(c, depth: 2, headings: &heads) }
            chapters.append(Chapter(title: "Abstract", html: "<section><h1 id=\"\(heads[0].id)\">Abstract</h1>\(html)</section>", headings: heads))
            if meta.abstract.isEmpty {
                meta.abstract = abs.elements.filter { $0.name == "section" }.map { s in
                    let h = s.child("h3")?.plain ?? ""
                    let t = s.elements.filter { $0.name != "h3" }.map(\.plain).joined(separator: " ")
                    return h.isEmpty ? t : "\(h): \(t)"
                }.joined(separator: "\n")
            }
        }
        // body: unheaded introduction, then one chapter per section
        guard let body = byID("bodymatter") else { throw fail("This NEJM page has no full text (are you signed in?).") }
        let container = body.elements.first { $0.hasClass("core-container") } ?? body
        var intro = ""
        var noHeads: [Heading] = []
        for c in container.children {
            if c.name == "section", let h2 = c.child("h2") {
                if !intro.isEmpty {
                    let id = r.uniqueID("introduction")
                    chapters.append(Chapter(title: "Introduction", html: "<section><h1 id=\"\(id)\">Introduction</h1>\(intro)</section>",
                                            headings: [Heading(level: 1, id: id, title: "Introduction")]))
                    intro = ""
                }
                chapters.append(conv.chapter(c, heading: h2))
            } else {
                intro += conv.blocks(c, depth: 2, headings: &noHeads)
            }
        }
        if !intro.isEmpty {
            let id = r.uniqueID("introduction")
            chapters.append(Chapter(title: "Introduction", html: "<section><h1 id=\"\(id)\">Introduction</h1>\(intro)</section>",
                                    headings: [Heading(level: 1, id: id, title: "Introduction")]))
        }
        // notes (dates, funding, disclosures, acknowledgments)
        if let notes = byID("backnotes"), let h2 = notes.child("h2") { chapters.append(conv.chapter(notes, heading: h2)) }
        // supplementary material: a short list of what exists (the files stay on NEJM.org)
        if let supp = byID("supplementary-materials") {
            let items = supp.all(where: { $0.hasClass("core-supplementary-material") }).compactMap { item -> String? in
                let label = item.all(where: { $0.hasClass("core-label") }).first?.plain ?? ""
                guard !label.isEmpty else { return nil }
                let id = item.attr("id").flatMap { targets.contains($0) ? r.uniqueID($0) : nil }
                let href = item.all(where: { $0.name == "a" }).first?.attr("href").flatMap { URL(string: $0, relativeTo: base)?.absoluteString }
                let name = href.map { "<a href=\"\(esc($0))\">\(esc(label))</a>" } ?? esc(label)
                return "<li\(id.map { " id=\"\(esc($0))\"" } ?? "")>\(name)</li>"
            }
            if !items.isEmpty {
                let id = r.uniqueID("supplementary-material")
                chapters.append(Chapter(title: "Supplementary Material",
                                        html: "<section><h1 id=\"\(id)\">Supplementary Material</h1><p>Available with the full text of this article at NEJM.org:</p><ul>\(items.joined())</ul></section>",
                                        headings: [Heading(level: 1, id: id, title: "Supplementary Material")]))
            }
        }
        // references
        var refCount = 0
        if let bib = byID("bibliography") {
            var items = ""
            for e in bib.all(where: { $0.hasClass("biblioentry") }) {
                guard let cit = e.all(where: { $0.hasClass("citations") }).first,
                      let content = cit.all(where: { $0.hasClass("citation-content") }).first else { continue }
                let id = cit.attr("id").map { r.uniqueID($0) }
                var text = r.inline(content).trimmingCharacters(in: .whitespacesAndNewlines)
                if let doi = cit.all(where: { $0.hasClass("core-xlink-crossref") }).first?.firstDescendant("a")?.attr("href"),
                   doi.hasPrefix("http") {
                    text += " <a href=\"\(esc(doi))\">\(esc(doi.replacingOccurrences(of: "https://doi.org/", with: "doi:")))</a>"
                }
                items += "<li\(id.map { " id=\"\(esc($0))\"" } ?? "")>\(text)</li>"
                refCount += 1
            }
            if !items.isEmpty {
                let id = r.uniqueID("references")
                chapters.append(Chapter(title: "References", html: "<section><h1 id=\"\(id)\">References</h1><ol class=\"refs\">\(items)</ol></section>",
                                        headings: [Heading(level: 1, id: id, title: "References")]))
            }
        }
        guard chapters.count > 2 else { throw fail("This NEJM page has no full text (are you signed in?).") }
        ScienceDirect.linkAcrossChapters(&chapters)
        for i in chapters.indices where chapters[i].html.contains("[[missing-image]]") {
            chapters[i].html = chapters[i].html.replacingOccurrences(of: "[[missing-image]]", with: "<p class=\"missing\"><em>[Figure image not available]</em></p>")
        }
        log("\(chapters.count) chapters, \(conv.figures) figure(s), \(conv.tables) table(s), \(refCount) references, \(r.missingImages) image(s) missing")
        var notes: [String] = []
        if r.missingImages > 0 { notes.append("\(r.missingImages) figure image(s) were not sent with the page; captions are kept.") }
        if r.mathCount > 0 { notes.append("\(r.mathCount) equation(s) as MathML.") }
        return Book(meta: meta, chapters: chapters, images: r.images, sourceLabel: "NEJM full text", notes: notes)
    }

    // MARK: Metadata

    static func metadata(_ doc: XNode, url: URL?) -> PaperMeta {
        var tags: [String: [String]] = [:]
        for m in doc.all(where: { $0.name == "meta" }) {
            guard let n = (m.attr("name") ?? m.attr("property"))?.lowercased(), let c = m.attr("content")?.trimmingCharacters(in: .whitespacesAndNewlines), !c.isEmpty else { continue }
            tags[n, default: []].append(c)
        }
        func first(_ names: String...) -> String { for n in names { if let v = tags[n]?.first { return v } }; return "" }
        var m = GenericPage.metadata(doc, url: url)
        m.title = first("citation_title", "dc.title")
        if m.title.isEmpty { m.title = doc.all(where: { $0.name == "h1" && $0.attr("property") == "name" }).first?.plain ?? m.title }
        if m.authors.isEmpty { m.authors = tags["dc.creator"] ?? [] }
        if m.journal.isEmpty { m.journal = "New England Journal of Medicine" }
        m.date = first("dc.date", "citation_publication_date").replacingOccurrences(of: "/", with: "-")
        if m.doi.isEmpty {
            let canonical = doc.all(where: { $0.name == "link" && $0.attr("rel") == "canonical" }).first?.attr("href") ?? ""
            for s in [canonical, first("og:url"), url?.absoluteString ?? ""] where m.doi.isEmpty {
                if let d = Resolver.doi(in: s) { m.doi = d }
            }
        }
        m.license = ""   // dc.Rights is the copyright line
        m.url = m.doi.isEmpty ? (url?.absoluteString ?? "") : "https://doi.org/\(m.doi)"
        m.abstract = ""  // from the page's structured abstract (the meta description is cut short)
        return m
    }

    // MARK: Rendering

    final class Converter {
        let r: GenericPage.Renderer
        let base: URL?
        var figures = 0, tables = 0

        init(r: GenericPage.Renderer, base: URL?) { self.r = r; self.base = base }

        /// A top-level section as a chapter (its h2 the chapter title; nested h3 sections as h2).
        func chapter(_ s: XNode, heading h2: XNode) -> Chapter {
            let title = h2.plain.trimmingCharacters(in: .whitespacesAndNewlines)
            let id = r.uniqueID(s.attr("id") ?? title.lowercased())
            var heads = [Heading(level: 1, id: id, title: title)]
            var html = ""
            for c in s.children where c !== h2 { html += blocks(c, depth: 2, headings: &heads) }
            return Chapter(title: title, html: "<section><h1 id=\"\(esc(id))\">\(esc(title))</h1>\(html)</section>", headings: heads)
        }

        /// Block content. `depth`: the heading level a nested section's heading gets (2, then 3).
        func blocks(_ n: XNode, depth: Int, headings: inout [Heading]) -> String {
            if n.isText { return n.text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? "" : "<p>\(esc(n.text))</p>" }
            switch n.name {
            case "section":
                var out = ""
                if let h = n.elements.first(where: { GenericPage.isHeading($0) }) {
                    let t = h.plain.trimmingCharacters(in: .whitespacesAndNewlines)
                    let level = min(depth, 3)
                    let id = r.uniqueID(n.attr("id") ?? h.attr("id"))
                    if level == 2 { headings.append(Heading(level: 2, id: id, title: t)) }
                    if !t.isEmpty { out += "<h\(level) id=\"\(esc(id))\">\(esc(t))</h\(level)>" }
                    for c in n.children where c !== h { out += blocks(c, depth: depth + 1, headings: &headings) }
                } else if let id = n.attr("id"), r.targets.contains(id) {
                    out += "<span id=\"\(esc(r.uniqueID(id)))\"></span>"
                    for c in n.children { out += blocks(c, depth: depth, headings: &headings) }
                } else {
                    for c in n.children { out += blocks(c, depth: depth, headings: &headings) }
                }
                return out
            case "h1", "h2", "h3", "h4", "h5", "h6":
                let t = n.plain.trimmingCharacters(in: .whitespacesAndNewlines)
                return t.isEmpty ? "" : "<p><strong>\(esc(t))</strong></p>"
            case "figure":
                return figureWrap(n, wrap: n)
            case "ul", "ol":
                return r.list(n)
            case "table":
                tables += 1
                return "<div class=\"table-wrap\">\(r.table(n))</div>"
            case "script", "style", "button", "nav", "aside", "noscript", "svg", "header", "form", "input":
                return ""
            case "div":
                if n.hasClass("figure-wrap"), let f = n.firstDescendant("figure") { return figureWrap(f, wrap: n) }
                if n.attr("role") == "paragraph" || n.attr("role") == "doc-footnote" {
                    // a paragraph may hold a display formula or list; otherwise it is one paragraph
                    if n.elements.contains(where: { ["ul", "ol", "table", "figure"].contains($0.name) || $0.hasClass("figure-wrap") }) {
                        return n.children.map { c in c.isText || !r.isBlock(c) ? wrapInline(c) : blocks(c, depth: depth, headings: &headings) }.joined()
                    }
                    let inner = r.inline(n).trimmingCharacters(in: .whitespacesAndNewlines)
                    if inner.isEmpty { return "" }
                    let id = n.attr("id").flatMap { r.targets.contains($0) ? r.uniqueID($0) : nil }
                    return "<p\(id.map { " id=\"\(esc($0))\"" } ?? "")>\(inner)</p>"
                }
                if n.attr("data-type") == "digital-object-links" { return "" }
                if n.attr("role") == "list" { return n.elements.map { "<p>\(r.inline($0).trimmingCharacters(in: .whitespacesAndNewlines))</p>" }.joined() }
                return n.children.map { blocks($0, depth: depth, headings: &headings) }.joined()
            default:
                if r.isBlock(n) { return n.children.map { blocks($0, depth: depth, headings: &headings) }.joined() }
                return wrapInline(n)
            }
        }

        private func wrapInline(_ n: XNode) -> String {
            let t = r.inline(n).trimmingCharacters(in: .whitespacesAndNewlines)
            return t.isEmpty ? "" : "<p>\(t)</p>"
        }

        /// `figure.graphic` → image + caption; `figure.table` → the HTML table (its image if the
        /// page has no table markup) with caption and footnotes.
        func figureWrap(_ f: XNode, wrap: XNode) -> String {
            let id = r.uniqueID(f.attr("id"))
            let label = wrap.all(where: { $0.hasClass("label") && $0.parent?.name == "header" }).first?.plain
                .trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
            let cap = f.firstDescendant("figcaption")
            let title = cap?.all(where: { $0.hasClass("caption") }).first.map { r.inline($0).trimmingCharacters(in: .whitespacesAndNewlines) } ?? ""
            let notes = cap?.all(where: { $0.attr("role") == "doc-footnote" }) ?? []
            let isTable = f.hasClass("table") || f.firstDescendant("table") != nil
            let head = (label.isEmpty ? "" : "<strong>\(esc(label)).</strong> ") + (title.isEmpty ? "" : "<strong>\(title)</strong>")

            if isTable, let table = f.firstDescendant("table") {
                tables += 1
                var out = head.isEmpty ? "" : "<p class=\"table-caption\">\(head)</p>"
                out += r.table(table)
                for fn in notes {
                    let mark = fn.all(where: { $0.hasClass("label") }).first?.plain ?? ""
                    let body = fn.elements.first { !$0.hasClass("label") }
                    let fid = body?.attr("id").map { r.uniqueID($0) }
                    let text = (body.map { r.inline($0) } ?? r.inline(fn)).trimmingCharacters(in: .whitespacesAndNewlines)
                    out += "<p class=\"table-foot\"\(fid.map { " id=\"\(esc($0))\"" } ?? "")><small>\(mark.isEmpty ? "" : esc(mark) + " ")\(text)</small></p>"
                }
                return "<div class=\"table-wrap\" id=\"\(esc(id))\">\(out)</div>"
            }
            figures += isTable ? 0 : 1
            if isTable { tables += 1 }
            var img = ""
            if let i = f.all(where: { $0.name == "img" }).first {
                if let file = r.imageFile(r.candidates(i)) {
                    img = "<img src=\"../images/\(file)\" alt=\"\(esc(label.isEmpty ? "Figure" : label))\"/>"
                } else {
                    r.missingImages += 1
                    img = "[[missing-image]]"
                }
            }
            let noteText = notes.map { r.inline($0).trimmingCharacters(in: .whitespacesAndNewlines) }.filter { !$0.isEmpty }
                .map { "<span class=\"cap-p\"> \($0)</span>" }.joined()
            let caption = head + noteText
            return "<figure id=\"\(esc(id))\">\(img)" + (caption.isEmpty ? "" : "<figcaption>\(caption)</figcaption>") + "</figure>"
        }
    }
}
