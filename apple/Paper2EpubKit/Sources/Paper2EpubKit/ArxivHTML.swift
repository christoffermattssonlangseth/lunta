import Foundation

/// arXiv papers: the HTML version arXiv renders from the LaTeX source (LaTeXML; ar5iv for older
/// papers) has real MathML, figures and tables, so it makes a much better reading view than the PDF.
enum ArxivHTML {
    /// Candidates for an arXiv id, best first: arXiv HTML, ar5iv HTML, then the PDF.
    static func candidates(_ id: String, log: @escaping (String) -> Void) -> [Candidate] {
        [
            ("arXiv HTML \(id)", { try await source(id, page: URL(string: "https://arxiv.org/html/\(id)")!,
                                                    base: URL(string: "https://arxiv.org/html/")!, label: "arXiv HTML", log: log) }),
            ("ar5iv \(id)", { try await source(id, page: URL(string: "https://ar5iv.labs.arxiv.org/html/\(id)")!,
                                               base: URL(string: "https://ar5iv.labs.arxiv.org/html/\(id)/")!, label: "ar5iv", log: log) }),
            ("arXiv PDF \(id)", {
                log("Downloading the arXiv PDF")
                let data = try await Net.get(URL(string: "https://arxiv.org/pdf/\(id)")!, accept: "application/pdf")
                let meta = (try? await metadata(id)) ?? PaperMeta()
                return .pdf(data, meta: meta, label: "arXiv PDF")
            }),
        ]
    }

    static func source(_ id: String, page: URL, base: URL, label: String, log: @escaping (String) -> Void) async throws -> Source {
        log("Fetching the \(label) version")
        let r = try await Net.fetch(page, accept: "text/html", timeout: 60)
        guard (200..<300).contains(r.status), let html = String(data: r.data, encoding: .utf8) else {
            throw fail("no \(label) version (HTTP \(r.status))")
        }
        guard html.contains("ltx_document") else { throw fail("no \(label) version of this paper") }
        let root = try parse(html)
        var meta = (try? await metadata(id)) ?? PaperMeta()
        if meta.title.isEmpty, let h = root.descendants("h1").first(where: { $0.hasClass("ltx_title_document") }) { meta.title = h.plain }
        return .html(root, base: base, meta: meta, label: label)
    }

    /// LaTeXML's HTML5 → a DOM: the <article> only, void elements closed, scripts and stray
    /// ampersands removed, then the XML parser.
    static func parse(_ html: String) throws -> XNode {
        var s = html
        if let a = s.range(of: "<article"), let b = s.range(of: "</article>", options: .backwards) {
            s = String(s[a.lowerBound..<b.upperBound])
        }
        let fixes: [(String, String)] = [
            (#"<!--[\s\S]*?-->"#, ""),
            (#"<script\b[\s\S]*?</script>"#, ""),
            (#"<style\b[\s\S]*?</style>"#, ""),
            (#"<button\b[\s\S]*?</button>"#, ""),
            (#"<(img|br|hr|meta|link|input|col|source|wbr|area|embed|param|track)\b((?:"[^"]*"|'[^']*'|[^'">])*?)\s*/?>"#, "<$1$2/>"),
            (#"&(?!#?[A-Za-z0-9]+;)"#, "&amp;"),
        ]
        for (p, r) in fixes { s = s.replacingOccurrences(of: p, with: r, options: .regularExpression) }
        return try XML.parse(Data(s.utf8))
    }

    /// Title, authors, date, abstract and DOI from the arXiv API (Atom).
    static func metadata(_ id: String) async throws -> PaperMeta {
        let url = URL(string: "https://export.arxiv.org/api/query?id_list=\(id)")!
        let root = try XML.parse(try await Net.get(url, timeout: 20))
        guard let e = root.child("entry") else { return PaperMeta() }
        var m = PaperMeta()
        m.title = e.child("title")?.plain ?? ""
        m.authors = e.children("author").compactMap { $0.child("name")?.plain }
        m.date = String((e.child("published")?.plain ?? "").prefix(10))
        m.abstract = e.child("summary")?.plain ?? ""
        let bare = id.replacingOccurrences(of: #"v\d+$"#, with: "", options: .regularExpression)
        m.doi = e.child("doi")?.plain ?? "10.48550/arXiv.\(bare)"
        m.journal = e.child("journal_ref")?.plain ?? "arXiv"
        m.url = "https://arxiv.org/abs/\(bare)"
        m.license = "arXiv"
        return m
    }
}

extension XNode {
    func hasClass(_ c: String) -> Bool { (attr("class") ?? "").split(separator: " ").contains { $0 == c } }
}

/// Turns a LaTeXML article into chapters (one per top-level section).
final class ArxivConverter {
    let root: XNode
    let base: URL
    private(set) var images: [String: BookImage] = [:]   // by source URL
    private var imageOrder: [String] = []
    private var headings: [Heading] = []
    private(set) var missingFigures = 0
    private var autoID = 0

    init(root: XNode, base: URL) {
        self.root = root
        self.base = base
    }

    /// Every figure image referenced (absolute URLs), in order.
    func imageURLs() -> [URL] {
        var out: [URL] = []
        func rec(_ n: XNode) {
            for c in n.children where !c.isText {
                if let src = c.name == "img" ? c.attr("src") : (c.name == "object" ? c.attr("data") : nil),
                   let u = URL(string: src, relativeTo: base)?.absoluteURL, !out.contains(u) {
                    out.append(u)
                }
                rec(c)
            }
        }
        rec(root)
        return out
    }

    func setImages(_ list: [(URL, BookImage)]) {
        for (u, img) in list { images[u.absoluteString] = img }
    }

    func chapters(meta: PaperMeta) -> [Chapter] {
        var out: [Chapter] = []
        var pending = ""
        let article = root.name == "article" ? root : (root.firstDescendant("article") ?? root)
        if let ab = article.descendants("div").first(where: { $0.hasClass("ltx_abstract") }) {
            headings = [Heading(level: 1, id: "abstract", title: "Abstract")]
            let inner = ab.children.filter { !($0.name.hasPrefix("h") && $0.hasClass("ltx_title")) }.map { render($0, depth: 1) }.joined()
            out.append(Chapter(title: "Abstract", html: "<section><h1 id=\"abstract\">Abstract</h1>\(inner)</section>", headings: headings))
        } else if !meta.abstract.isEmpty {
            out.append(Chapter(title: "Abstract", html: "<section><h1 id=\"abstract\">Abstract</h1><p>\(esc(meta.abstract))</p></section>",
                               headings: [Heading(level: 1, id: "abstract", title: "Abstract")]))
        }
        for c in article.children {
            if c.isText { continue }
            if c.name == "section" {
                let text = pending.replacingOccurrences(of: "<[^>]+>", with: "", options: .regularExpression)
                    .trimmingCharacters(in: .whitespacesAndNewlines)
                if text.count > 500 {
                    out.append(Chapter(title: "Main text", html: "<section><h1 id=\"main\">Main text</h1>\(pending)</section>",
                                       headings: [Heading(level: 1, id: "main", title: "Main text")]))
                } else if !text.isEmpty, let last = out.indices.last {
                    // a short note before the first section (e.g. a licence line): tuck it under the abstract
                    out[last].html = out[last].html.replacingOccurrences(of: "</section>", with: "<p class=\"note\"><small>\(text)</small></p></section>", options: .backwards)
                }
                pending = ""
                headings = []
                let html = render(c, depth: 0)
                let title = headings.first?.title ?? "Section"
                if headings.isEmpty { headings = [Heading(level: 1, id: c.attr("id") ?? nextID(), title: title)] }
                out.append(Chapter(title: title, html: html, headings: headings))
            } else if skipBlock(c) || (c.name == "div" && c.hasClass("ltx_abstract")) || c.name.hasPrefix("h") {
                continue
            } else {
                pending += render(c, depth: 1)
            }
        }
        return out
    }

    private func nextID() -> String { autoID += 1; return "ax\(autoID)" }

    private func skipBlock(_ n: XNode) -> Bool {
        ["ltx_authors", "ltx_dates", "ltx_page_footer", "ltx_page_header", "ltx_pagination", "ltx_keywords_hidden",
         "ltx_title_document", "ltx_role_affiliationtext", "ltx_TOC"].contains { n.hasClass($0) }
            || ["nav", "footer", "header", "form", "input", "script", "style", "button"].contains(n.name)
    }

    private static let passthrough: Set<String> = ["p", "div", "span", "em", "i", "b", "strong", "sup", "sub", "ul", "ol", "li",
        "dl", "dt", "dd", "table", "thead", "tbody", "tfoot", "tr", "td", "th", "caption", "figcaption", "blockquote",
        "pre", "code", "cite", "small", "section", "figure", "u", "s", "tt", "br", "hr"]

    /// XHTML for a node. `depth` is the section depth (0 = chapter level).
    func render(_ n: XNode, depth: Int) -> String {
        if n.isText { return esc(n.text) }
        if skipBlock(n) { return "" }
        if n.name == "math" { return mathml(n) }
        if n.name == "svg" { return svg(n) }
        // footnotes: the mark becomes a superscript, the text follows in small type
        if n.hasClass("ltx_note") {
            let content = n.descendants("span").first { $0.hasClass("ltx_note_content") }
            guard let content else { return "" }
            let text = content.children.filter { !$0.hasClass("ltx_note_mark") && !$0.hasClass("ltx_tag") }.map { render($0, depth: depth) }.joined()
            return " <small class=\"fn\">(\(text.trimmingCharacters(in: .whitespaces)))</small>"
        }
        switch n.name {
        case "h1", "h2", "h3", "h4", "h5", "h6":
            let lvl = min(4, max(1, depth))
            let id = n.parent?.attr("id") ?? n.attr("id") ?? nextID()
            let title = n.plain
            if lvl <= 2 { headings.append(Heading(level: lvl, id: id, title: title)) }
            return "<h\(lvl) id=\"\(esc(id))\">\(inline(n, depth: depth))</h\(lvl)>"
        case "section":
            let inner = n.children.map { render($0, depth: depth + 1) }.joined()
            return "<section>\(inner)</section>"
        case "img", "object":
            let src = n.name == "img" ? n.attr("src") : n.attr("data")
            guard let src, let u = URL(string: src, relativeTo: base)?.absoluteURL, let img = images[u.absoluteString] else {
                missingFigures += 1
                return "<span class=\"missing\">[Figure image not available]</span>"
            }
            return "<img src=\"../images/\(img.fileName)\" alt=\"\(esc(n.attr("alt") ?? "Figure"))\"/>"
        case "a":
            let inner = n.children.map { render($0, depth: depth) }.joined()
            guard let href = n.attr("href") else { return inner }
            if href.hasPrefix("#") || href.hasPrefix("http") { return "<a href=\"\(esc(href))\">\(inner)</a>" }
            return inner
        case "br": return "<br/>"
        case "hr": return "<hr/>"
        default:
            let inner = n.children.map { render($0, depth: depth) }.joined()
            guard Self.passthrough.contains(n.name) else { return inner }
            var attrs = ""
            if let id = n.attr("id"), ["figure", "table", "section", "li", "div", "span", "p"].contains(n.name) { attrs += " id=\"\(esc(id))\"" }
            for k in ["colspan", "rowspan"] { if let v = n.attr(k) { attrs += " \(k)=\"\(esc(v))\"" } }
            if n.hasClass("ltx_tag") { attrs += " class=\"label\"" }
            if n.name == "div", n.hasClass("ltx_equation") || n.hasClass("ltx_equationgroup") { attrs += " class=\"equation\"" }
            if n.name == "ul" && n.hasClass("ltx_biblist") { return "<ol class=\"refs\"\(attrs)>\(inner)</ol>" }
            if n.name == "span", attrs.isEmpty, !n.hasClass("ltx_font_italic"), !n.hasClass("ltx_font_bold") { return inner }
            if n.name == "span", n.hasClass("ltx_font_italic") { return "<em>\(inner)</em>" }
            if n.name == "span", n.hasClass("ltx_font_bold") { return "<strong>\(inner)</strong>" }
            return "<\(n.name)\(attrs)>\(inner)</\(n.name)>"
        }
    }

    private func inline(_ n: XNode, depth: Int) -> String {
        n.children.map { render($0, depth: depth) }.joined()
    }

    static let mathNS = "http://www.w3.org/1998/Math/MathML"

    private func mathml(_ m: XNode) -> String {
        var s = "<math xmlns=\"\(Self.mathNS)\""
        if let d = m.attr("display") { s += " display=\"\(esc(d))\"" }
        if let a = m.attr("alttext") { s += " alttext=\"\(esc(a))\"" }
        s += ">"
        for c in m.children { s += serialize(c) }
        return s + "</math>"
    }

    private func serialize(_ n: XNode) -> String {
        if n.isText { return esc(n.text) }
        // annotations repeat the TeX source; the reader doesn't need them
        if n.name == "annotation" || n.name == "annotation-xml" { return "" }
        var s = "<\(n.name)"
        for k in n.attrOrder where !["id", "xref", "class", "intent", "arg"].contains(k) {
            if k == "displaystyle", !["mstyle", "mtable", "math"].contains(n.name) { continue }
            s += " \(k)=\"\(esc(n.attrs[k] ?? ""))\""
        }
        if n.children.isEmpty { return s + "/>" }
        return s + ">" + n.children.map(serialize).joined() + "</\(n.name)>"
    }

    private func svg(_ n: XNode) -> String {
        func ser(_ x: XNode, top: Bool) -> String {
            if x.isText { return esc(x.text) }
            var s = "<\(x.name)"
            if top { s += " xmlns=\"http://www.w3.org/2000/svg\"" }
            for k in x.attrOrder where k != "xmlns" { s += " \(k)=\"\(esc(x.attrs[k] ?? ""))\"" }
            if x.children.isEmpty { return s + "/>" }
            return s + ">" + x.children.map { ser($0, top: false) }.joined() + "</\(x.name)>"
        }
        return ser(n, top: true)
    }
}
