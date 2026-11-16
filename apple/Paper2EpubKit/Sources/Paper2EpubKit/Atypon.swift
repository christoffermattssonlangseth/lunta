import Foundation

/// Article pages on Atypon's Literatum platform in its "core" article markup: NEJM, Science
/// (science.org), PNAS, SAGE, ACS, ASM, AHA, Annual Reviews, Taylor & Francis and Wiley's newer
/// templates, … (and Cell Press / Lancet pages when they carry it). As the browser shows them to a
/// reader with access, or saved from Safari (.webarchive):
///
/// - details in `dc.*` / `citation_*` meta tags (NEJM has no `citation_doi`: the DOI comes from the
///   canonical link / URL);
/// - the abstract(s) before the body: `section#abstract` / `section[role=doc-abstract]` (Science:
///   also the editor's summary; PNAS: Significance), NEJM's `section#summary-abstract`;
/// - `section#bodymatter`: unheaded introduction paragraphs (NEJM), then `section`s with h2 and
///   nested sections with h3; paragraphs are `div[role=paragraph]`; figures and tables are
///   `figure.graphic` / `figure.table` (NEJM: inside `div.figure-wrap` with the label in a header)
///   with `figcaption` (`.label`, `.caption` or a title and paragraphs, `.notes`); display
///   equations are MathML;
/// - `section#backmatter`: notes / acknowledgments / funding sections, `#supplementary-materials`,
///   `#bibliography` (`.biblioentry` or `[role=listitem]` > `.citations` > `.citation-content`);
///   in-text citations link to the list (`#r1`, `#core-R1`) or to the copy in the side pane
///   (`#core-collateral-r1`), and are pointed at the list here.
///
/// Everything else (navigation, the side panes and tabs, ads, metrics, "cited by", author pane)
/// is never read. Pages without the core markup (Atypon's older templates) go to the generic
/// reader; a page with only the abstract (no access) is refused.
///
/// Rust port: core/src/convert/atypon.rs (with webpage.rs), when desktop / Android take pages.
enum Atypon {
    /// How one site differs (NEJM: its own abstract and back-matter sections, its label).
    struct Site {
        var isNEJM: Bool
        var label: String
        var name: String        // "NEJM.org", "science.org"
        var journal: String     // when the page names none

        static func of(_ url: URL?) -> Site {
            let host = (url?.host ?? "").lowercased()
            if host.hasSuffix("nejm.org") {
                return Site(isNEJM: true, label: "NEJM full text", name: "NEJM.org", journal: "New England Journal of Medicine")
            }
            let name = host.replacingOccurrences(of: #"^www\."#, with: "", options: .regularExpression)
            return Site(isNEJM: false, label: "Full text from \(name.isEmpty ? "the publisher" : name)", name: name.isEmpty ? "the publisher's site" : name, journal: "")
        }
    }

    /// The core markup, by a look at the page's source.
    static func looksLike(_ html: String) -> Bool { html.contains("id=\"bodymatter\"") && html.contains("core-container") }

    static func looksLike(_ doc: XNode) -> Bool {
        doc.all(where: { $0.name == "section" && $0.attr("id") == "bodymatter" }).first != nil
    }

    static func book(html: String, url: URL?, images: PageImages, log: @escaping (String) -> Void) async throws -> Book {
        let site = Site.of(url)
        let doc = HTMLDOM.parse(html)
        guard looksLike(doc) else {
            log("No Atypon core article markup; reading the page generically")
            return try await GenericPage.book(html: html, url: url, images: images, label: site.label, log: log)
        }
        log("Converting the \(site.isNEJM ? "NEJM" : "Atypon (\(site.name))") page")
        func byID(_ id: String) -> XNode? { doc.all(where: { $0.attr("id") == id }).first }

        // in-text citations point at the side pane's copy of the list: point them at ours
        var targets = Set<String>()
        for a in doc.all(where: { $0.name == "a" }) {
            guard var h = a.attr("href"), h.hasPrefix("#") else { continue }
            if h.hasPrefix("#core-collateral-") { h = "#" + h.dropFirst("#core-collateral-".count); a.attrs["href"] = h }
            targets.insert(String(h.dropFirst()))
        }
        var meta = metadata(doc, url: url, site: site)
        let base = url ?? URL(string: site.isNEJM ? "https://www.nejm.org/" : "https://\(site.name)/")
        let r = GenericPage.Renderer(images: images, base: base, title: meta.title)
        r.targets = targets
        let conv = Converter(r: r, base: base, nejm: site.isNEJM)
        let body = byID("bodymatter")!

        var chapters: [Chapter] = []
        // abstract(s)
        if site.isNEJM {
            if let abs = byID("summary-abstract") {
                var heads = [Heading(level: 1, id: r.uniqueID("abstract"), title: "Abstract")]
                var html = ""
                for c in abs.elements where c.name != "h2" { html += conv.blocks(c, depth: 2, headings: &heads) }
                chapters.append(Chapter(title: "Abstract", html: "<section><h1 id=\"\(heads[0].id)\">Abstract</h1>\(html)</section>", headings: heads))
                if meta.abstract.isEmpty { meta.abstract = abstractText(abs) }
            }
        } else {
            for abs in abstracts(doc, body: body) {
                let h = abs.elements.first(where: GenericPage.isHeading)
                var title = h?.plain.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
                if title.isEmpty { title = "Abstract" }
                title = Converter.titleCase(title)
                var heads = [Heading(level: 1, id: r.uniqueID(abs.attr("id") ?? "abstract"), title: title)]
                var html = ""
                for c in abs.children where c !== h { html += conv.blocks(c, depth: 2, headings: &heads) }
                guard !ReaderText.textOf(Substring(html)).trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { continue }
                chapters.append(Chapter(title: title, html: "<section><h1 id=\"\(esc(heads[0].id))\">\(esc(title))</h1>\(html)</section>", headings: heads))
                if meta.abstract.isEmpty, GenericPage.Renderer.isAbstract(title) { meta.abstract = abstractText(abs) }
            }
        }
        let abstractChapters = chapters.count
        // body: unheaded introduction, then one chapter per section
        let container = body.elements.first { $0.hasClass("core-container") } ?? body
        var intro = ""
        var noHeads: [Heading] = []
        for c in container.children {
            if c.name == "section", let h2 = c.child("h2") ?? (site.isNEJM ? nil : c.elements.first(where: { GenericPage.isHeading($0) })) {
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
        // no access: the page has the abstract and an empty (or teaser) body
        let bodyWords = chapters.dropFirst(abstractChapters).reduce(0) { $0 + GenericPage.words(ReaderText.textOf(Substring($1.html))) }
        if bodyWords < 250 {
            throw fail(abstractChapters > 0 || !meta.abstract.isEmpty ? GenericPage.abstractOnly
                       : "This page has no full text of the article (are you signed in?).")
        }
        // back matter: NEJM's notes; elsewhere every section (acknowledgments, funding, notes, …)
        var supp: XNode? = byID("supplementary-materials")
        var bib: XNode? = byID("bibliography")
        if site.isNEJM {
            if let notes = byID("backnotes"), let h2 = notes.child("h2") { chapters.append(conv.chapter(notes, heading: h2)) }
        } else if let back = byID("backmatter") {
            for s in backSections(back) {
                let id = s.attr("id") ?? ""
                if id == "supplementary-materials" || s.attr("role") == "doc-appendix" && s.plain.lowercased().contains("supplementary") { supp = supp ?? s; continue }
                if id == "bibliography" || s.attr("role") == "doc-bibliography" { bib = bib ?? s; continue }
                if id.hasPrefix("tab-") || id.hasPrefix("core-collateral") || id == "cited-by" { continue }
                guard let h = s.elements.first(where: GenericPage.isHeading) else { continue }
                if GenericPage.Renderer.isReferences(h.plain) { bib = bib ?? s; continue }
                chapters.append(conv.chapter(s, heading: h))
            }
        }
        // supplementary material: a short list of what exists (the files stay on the publisher's site)
        if let supp {
            let items = supp.all(where: { $0.hasClass("core-supplementary-material") || $0.attr("role") == "listitem" && !site.isNEJM }).compactMap { item -> String? in
                var label = item.all(where: { $0.hasClass("core-label") || $0.hasClass("label") }).first?.plain ?? ""
                if label.isEmpty, !site.isNEJM { label = item.all(where: { $0.name == "a" }).first?.plain ?? "" }
                guard !label.isEmpty else { return nil }
                let id = item.attr("id").flatMap { targets.contains($0) ? r.uniqueID($0) : nil }
                let href = item.all(where: { $0.name == "a" }).first?.attr("href").flatMap { URL(string: $0, relativeTo: base)?.absoluteString }
                let name = href.map { "<a href=\"\(esc($0))\">\(esc(label))</a>" } ?? esc(label)
                return "<li\(id.map { " id=\"\(esc($0))\"" } ?? "")>\(name)</li>"
            }
            if !items.isEmpty {
                let id = r.uniqueID("supplementary-material")
                chapters.append(Chapter(title: "Supplementary Material",
                                        html: "<section><h1 id=\"\(id)\">Supplementary Material</h1><p>Available with the full text of this article at \(esc(site.name)):</p><ul>\(items.joined())</ul></section>",
                                        headings: [Heading(level: 1, id: id, title: "Supplementary Material")]))
            }
        }
        // references
        var refCount = 0
        if let bib {
            var items = ""
            let entries = bib.all(where: { $0.hasClass("biblioentry") })
            if !entries.isEmpty || site.isNEJM {
                for e in entries {
                    guard let cit = e.all(where: { $0.hasClass("citations") }).first,
                          let content = cit.all(where: { $0.hasClass("citation-content") }).first else { continue }
                    let id = cit.attr("id").map { r.uniqueID($0) }
                    let extra = site.isNEJM ? "" : anchors([e.attr("id")], besides: id, r: r)
                    let text = site.isNEJM ? refTextRaw(content, cit: cit, r: r) : refText(content, cit: cit, r: r)
                    items += "<li\(id.map { " id=\"\(esc($0))\"" } ?? "")>\(extra)\(text)</li>"
                    refCount += 1
                }
            } else {
                // Science / PNAS: div[role=list] > div[role=listitem]#core-R1 > .label + .citations (or list items)
                let list = bib.all(where: { $0.attr("role") == "listitem" || ($0.name == "li" && !$0.ancestors().contains { $0.name == "li" }) })
                    .filter { e in !e.ancestors().contains { $0.attr("role") == "listitem" } }
                for e in list {
                    let cit = e.all(where: { $0.hasClass("citations") || $0.hasClass("citation") }).first
                    let content = cit?.all(where: { $0.hasClass("citation-content") }).first ?? cit ?? e
                    let ids = [e.attr("id"), cit?.attr("id"), content.attr("id")].compactMap { $0 }.filter { !$0.isEmpty }
                    let id = ids.first.map { r.uniqueID($0) }
                    let extra = anchors(Array(ids.dropFirst()), besides: id, r: r)
                    let text = refText(content, cit: cit ?? e, r: r)
                    guard !ReaderText.textOf(Substring(text)).trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { continue }
                    items += "<li\(id.map { " id=\"\(esc($0))\"" } ?? "")>\(extra)\(text)</li>"
                    refCount += 1
                }
            }
            if !items.isEmpty {
                let id = r.uniqueID("references")
                chapters.append(Chapter(title: "References", html: "<section><h1 id=\"\(id)\">References</h1><ol class=\"refs\">\(items)</ol></section>",
                                        headings: [Heading(level: 1, id: id, title: "References")]))
            }
        }
        ScienceDirect.linkAcrossChapters(&chapters)
        for i in chapters.indices where chapters[i].html.contains("[[missing-image]]") {
            chapters[i].html = chapters[i].html.replacingOccurrences(of: "[[missing-image]]", with: "<p class=\"missing\"><em>[Figure image not available]</em></p>")
        }
        log("\(chapters.count) chapters, \(conv.figures) figure(s), \(conv.tables) table(s), \(refCount) references, \(r.missingImages) image(s) missing")
        var notes: [String] = []
        if r.missingImages > 0 { notes.append("\(r.missingImages) figure image(s) were not sent with the page; captions are kept.") }
        if r.mathCount > 0 { notes.append("\(r.mathCount) equation(s) as MathML.") }
        return Book(meta: meta, chapters: chapters, images: r.images, sourceLabel: site.label, notes: notes)
    }

    /// A reference's text, with its DOI link (from the "View" / Crossref link).
    static func refText(_ content: XNode, cit: XNode, r: GenericPage.Renderer) -> String {
        // the copy without the label, the back link and the service links
        for x in content.all(where: { $0.hasClass("external-links") || $0.hasClass("label") || $0.hasClass("to-citation__wrapper") || $0.attr("role") == "doc-backlink" }) { x.remove() }
        return refTextRaw(content, cit: cit, r: r)
    }

    static func refTextRaw(_ content: XNode, cit: XNode, r: GenericPage.Renderer) -> String {
        var text = r.inline(content).trimmingCharacters(in: .whitespacesAndNewlines)
        if let doi = cit.all(where: { $0.hasClass("core-xlink-crossref") }).first?.firstDescendant("a")?.attr("href"),
           doi.hasPrefix("http") {
            text += " <a href=\"\(esc(doi))\">\(esc(doi.replacingOccurrences(of: "https://doi.org/", with: "doi:")))</a>"
        }
        return text
    }

    /// Empty anchors for a reference's other ids (in-text links use one or the other).
    static func anchors(_ ids: [String?], besides id: String?, r: GenericPage.Renderer) -> String {
        ids.compactMap { $0 }.filter { !$0.isEmpty && $0 != id && r.targets.contains($0) }
            .map { "<span id=\"\(esc(r.uniqueID($0)))\"></span>" }.joined()
    }

    static func abstractText(_ abs: XNode) -> String {
        let secs = abs.elements.filter { $0.name == "section" }
        if secs.isEmpty {
            return abs.elements.filter { !GenericPage.isHeading($0) }.map(\.plain).joined(separator: " ")
        }
        return secs.map { s in
            let h = s.child("h3")?.plain ?? ""
            let t = s.elements.filter { $0.name != "h3" }.map(\.plain).joined(separator: " ")
            return h.isEmpty ? t : "\(h): \(t)"
        }.joined(separator: "\n")
    }

    /// The abstracts before the body (Abstract, Structured Abstract, Editor's summary, Significance,
    /// Graphical abstract …), outermost only, in page order; not the copies in side panes.
    static func abstracts(_ doc: XNode, body: XNode) -> [XNode] {
        let isAbs: (XNode) -> Bool = { x in
            guard x.name == "section" else { return false }
            let id = (x.attr("id") ?? "").lowercased()
            return x.attr("role") == "doc-abstract" || x.attr("property") == "abstract"
                || id == "abstract" || id.hasPrefix("abstract-") || id.hasSuffix("-abstract") || id == "executive-summary" || id == "significance"
        }
        let bad: (XNode) -> Bool = { a in
            a === body || a.attr("id") == "backmatter" || (a.attr("id") ?? "").contains("collateral") || a.attr("role") == "tabpanel"
                || a.classes.contains { $0.contains("collateral") || $0.hasPrefix("tab__") }
        }
        return doc.all(where: isAbs).filter { x in
            !x.ancestors().contains(where: isAbs) && !x.ancestors().contains(where: bad) && GenericPage.words(x.plain) >= 15
        }
    }

    /// The sections of the back matter, in order (through wrappers such as `div.core-container`).
    static func backSections(_ back: XNode) -> [XNode] {
        var out: [XNode] = []
        func walk(_ n: XNode) {
            for c in n.elements {
                if c.name == "section" { out.append(c) } else if c.name == "div" { walk(c) }
            }
        }
        walk(back)
        return out
    }

    // MARK: Metadata

    static func metadata(_ doc: XNode, url: URL?, site: Site) -> PaperMeta {
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
        if m.journal.isEmpty { m.journal = site.journal.isEmpty ? first("og:site_name") : site.journal }
        let date = first("dc.date", "citation_publication_date", "citation_online_date", "citation_date", "dc.date.issued")
        if !date.isEmpty { m.date = date.replacingOccurrences(of: "/", with: "-") }
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
        let nejm: Bool

        init(r: GenericPage.Renderer, base: URL?, nejm: Bool) { self.r = r; self.base = base; self.nejm = nejm }

        /// "INTRODUCTION" → "Introduction" (Science sets section titles in capitals); others unchanged.
        static func titleCase(_ t: String) -> String { GenericPage.titleCase(t) }

        /// A top-level section as a chapter (its h2 the chapter title; nested h3 sections as h2).
        func chapter(_ s: XNode, heading h2: XNode) -> Chapter {
            let title = Self.titleCase(h2.plain.trimmingCharacters(in: .whitespacesAndNewlines))
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
            var label = wrap.all(where: { $0.hasClass("label") && $0.parent?.name == "header" }).first?.plain
                .trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
            let cap = f.firstDescendant("figcaption")
            var title = cap?.all(where: { $0.hasClass("caption") }).first.map { r.inline($0).trimmingCharacters(in: .whitespacesAndNewlines) } ?? ""
            var notes = cap?.all(where: { $0.attr("role") == "doc-footnote" }) ?? []
            var plainTitle = false   // a whole caption (title and legend), not a title to set in bold
            if !nejm {
                // Science / PNAS: the label in the caption, then a title and paragraphs; table notes after the table
                let capLabel = cap?.all(where: { $0.hasClass("label") }).first
                if label.isEmpty, let capLabel {
                    label = capLabel.plain.trimmingCharacters(in: .whitespacesAndNewlines).replacingOccurrences(of: #"\.$"#, with: "", options: .regularExpression)
                }
                if title.isEmpty, let cap {
                    plainTitle = true
                    title = cap.children.filter { c in c !== capLabel && c.attr("role") != "doc-footnote" && !(c.isText && c.text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty) }
                        .map { c in r.isBlock(c) ? "<span class=\"cap-p\">\(r.inline(c).trimmingCharacters(in: .whitespacesAndNewlines))</span> " : r.inline(c) }
                        .joined().trimmingCharacters(in: .whitespacesAndNewlines)
                }
                notes += f.all(where: { ($0.hasClass("table-wrap-foot") || $0.hasClass("table-foot") || $0.attr("role") == "doc-footnote")
                    && !$0.ancestors().contains { $0.name == "figcaption" } && !$0.ancestors().contains { $0.hasClass("table-wrap-foot") || $0.attr("role") == "doc-footnote" } })
            }
            let isTable = f.hasClass("table") || f.firstDescendant("table") != nil
            let head = (label.isEmpty ? "" : "<strong>\(esc(label)).</strong> ") + (title.isEmpty ? "" : plainTitle ? title : "<strong>\(title)</strong>")

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
                var cands = r.candidates(i)
                if !nejm { cands += f.all(where: { $0.name == "a" }).compactMap { $0.attr("href") }.filter { $0.contains("/asset/") || $0.range(of: #"\.(jpe?g|png|gif|webp)(\?|$)"#, options: .regularExpression) != nil } }
                if let file = r.imageFile(cands) {
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

extension GenericPage {
    /// "MATERIALS AND METHODS" → "Materials and Methods" (section titles set in capitals); others unchanged.
    static func titleCase(_ t: String) -> String {
        let letters = t.filter(\.isLetter)
        guard letters.count > 3, letters == letters.uppercased() else { return t }
        let small: Set<String> = ["a", "an", "and", "as", "at", "by", "for", "in", "of", "on", "or", "the", "to", "with", "vs", "via"]
        return t.lowercased().split(separator: " ", omittingEmptySubsequences: false).enumerated().map { i, w in
            let s = String(w)
            return i > 0 && small.contains(s) ? s : s.prefix(1).uppercased() + s.dropFirst()
        }.joined(separator: " ")
    }
}
