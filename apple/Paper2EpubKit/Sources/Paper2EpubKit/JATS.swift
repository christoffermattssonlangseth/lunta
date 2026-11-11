import Foundation

/// JATS XML → XHTML chapters. A port of `JatsConverter` in paper2epub.py, producing
/// one chapter per top-level section instead of a single HTML document for pandoc.
final class JATSConverter {
    static let blockTags: Set<String> = [
        "fig", "fig-group", "table-wrap", "table-wrap-group", "disp-formula", "disp-formula-group", "list",
        "boxed-text", "disp-quote", "preformat", "code", "def-list", "supplementary-material", "statement",
        "graphic", "media", "array", "speech", "verse-group", "p", "sec", "chem-struct-wrap",
    ]
    static let mathNS = "http://www.w3.org/1998/Math/MathML"

    let root: XNode
    let images: [String: BookImage]
    private(set) var missingFigures = 0
    private(set) var texFormulas = 0
    private var headings: [Heading] = []
    private var autoID = 0
    /// Figures/tables kept in <floats-group> (NIH manuscripts): placed after their first citation.
    private var floats: [String: XNode] = [:]
    private var floatOrder: [String] = []
    private var placed = Set<String>()

    init(root: XNode, images: [String: BookImage]) {
        self.root = root
        self.images = images
        for fg in root.children("floats-group") {
            for f in fg.elements where ["fig", "table-wrap", "fig-group", "table-wrap-group", "boxed-text"].contains(f.name) {
                if let id = f.attr("id") { floats[id] = f; floatOrder.append(id) }
            }
        }
    }

    /// Floating figures cited by `el` that have not been placed yet.
    private func citedFloats(_ el: XNode) -> String {
        guard !floats.isEmpty else { return "" }
        var out = ""
        for x in el.descendants("xref") {
            for rid in (x.attr("rid") ?? "").split(separator: " ").map(String.init) where floats[rid] != nil && !placed.contains(rid) {
                placed.insert(rid)
                out += block(floats[rid]!, level: 3)
            }
        }
        return out
    }

    var articleMeta: XNode? { root.child("front")?.child("article-meta") ?? root.firstDescendant("article-meta") }

    /// Every figure/graphic reference that needs downloading.
    static func graphicHrefs(_ root: XNode) -> [String] {
        let body = root.children.filter { $0.name != "sub-article" && $0.name != "response" }
        var out: [String] = []
        for part in body {
            for g in part.descendants("graphic") + part.descendants("inline-graphic")
            where g.attr("content-type") != "thumb" {
                if let h = g.attr("href") { out.append(h) }
            }
        }
        return out
    }

    // MARK: - Metadata

    func meta() -> PaperMeta {
        var m = PaperMeta()
        guard let am = articleMeta else { return m }
        m.title = am.child("title-group")?.child("article-title")?.plain ?? ""
        for cg in am.descendants("contrib-group") {
            for c in cg.children("contrib") where (c.attr("contrib-type") ?? "author") == "author" {
                if let n = c.child("name") {
                    let given = (n.childText("given-names") ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
                    let sur = (n.childText("surname") ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
                    let full = "\(given) \(sur)".trimmingCharacters(in: .whitespaces)
                    if !full.isEmpty { m.authors.append(full) }
                } else if let col = c.child("collab") {
                    m.authors.append(col.plain)
                }
            }
        }
        for aid in am.children("article-id") where aid.attr("pub-id-type") == "doi" {
            m.doi = aid.allText.trimmingCharacters(in: .whitespacesAndNewlines)
        }
        m.journal = root.firstDescendant("journal-meta")?.firstDescendant("journal-title")?.plain ?? ""
        let dates = am.children("pub-date")
        let pick = dates.first { ["epub", "ppub", "pub"].contains($0.attr("pub-type") ?? "")
            || ["pub", "publication"].contains($0.attr("date-type") ?? "") } ?? dates.first
        if let d = pick {
            let parts = [d.childText("year"), d.childText("month"), d.childText("day")]
                .map { ($0 ?? "").trimmingCharacters(in: .whitespacesAndNewlines) }
            var s = parts[0]
            if !s.isEmpty, !parts[1].isEmpty {
                s += "-" + pad2(parts[1])
                if !parts[2].isEmpty { s += "-" + pad2(parts[2]) }
            }
            m.date = s
        }
        m.keywords = Array(am.descendants("kwd-group").flatMap { $0.children("kwd") }.map(\.plain).prefix(15))
        if let lic = am.child("permissions")?.child("license") {
            // prefer the machine-readable licence URL (<ali:license_ref>) over the full licence prose
            let ref = lic.firstDescendant("license_ref")?.plain
            m.license = (ref?.isEmpty == false ? ref : nil) ?? lic.attr("href") ?? String(lic.plain.prefix(300))
        }
        if let ab = am.children("abstract").first(where: { $0.attr("abstract-type") == nil }) {
            m.abstract = ab.plain
        }
        let affs = am.descendants("aff")
        if !affs.isEmpty {
            m.affiliationsHTML = affs.map { inline($0, skip: ["label"]) }.joined(separator: "<br/>")
        }
        return m
    }

    private func pad2(_ s: String) -> String { s.count == 1 ? "0" + s : s }

    // MARK: - Chapters

    func chapters() -> [Chapter] {
        var out: [Chapter] = []
        if let am = articleMeta {
            var absN = 0
            for ab in am.children("abstract") {
                let typ = ab.attr("abstract-type") ?? ""
                if ["toc", "graphical", "teaser", "web-summary", "short"].contains(typ) { continue }
                var title = ab.child("title")?.plain ?? ""
                if title.isEmpty {
                    title = typ.isEmpty ? "Abstract" : (typ == "executive-summary" ? "Summary"
                        : typ.replacingOccurrences(of: "-", with: " ").capitalized)
                }
                absN += 1
                let id = absN == 1 ? "abstract" : "abstract-\(absN)"
                out.append(chapter(title: title, id: id) { self.blocks(ab, level: 2, skip: ["title"]) })
            }
        }
        if let body = root.child("body") {
            var pending = ""
            for ch in body.children {
                if !ch.isText, ch.name == "sec" {
                    if !pending.isEmpty {
                        appendOrCreate(&out, html: pending, fallbackTitle: out.isEmpty ? "Main text" : "Main text")
                        pending = ""
                    }
                    out.append(secChapter(ch, fallbackTitle: "Section"))
                } else if ch.isText {
                    let t = ch.text.trimmingCharacters(in: .whitespacesAndNewlines)
                    if !t.isEmpty { pending += "<p>\(esc(t))</p>" }
                } else {
                    pending += block(ch, level: 2)
                }
            }
            if !pending.isEmpty { appendOrCreate(&out, html: pending, fallbackTitle: "Main text") }
        }
        let unplaced = floatOrder.filter { !placed.contains($0) }
        if !unplaced.isEmpty {
            out.append(chapter(title: "Figures and tables", id: nextID()) {
                unplaced.map { self.placed.insert($0); return self.block(self.floats[$0]!, level: 2) }.joined()
            })
        }
        if let back = root.child("back") {
            out += backChapters(back)
        }
        return out
    }

    /// Body content that is not inside a <sec>: becomes its own "Main text" chapter.
    private func appendOrCreate(_ out: inout [Chapter], html: String, fallbackTitle: String) {
        let id = nextID()
        out.append(Chapter(title: fallbackTitle,
                           html: "<section><h1 id=\"\(id)\">\(esc(fallbackTitle))</h1>\(html)</section>",
                           headings: [Heading(level: 1, id: id, title: fallbackTitle)]))
    }

    private func chapter(title: String, id: String, content: () -> String) -> Chapter {
        headings = [Heading(level: 1, id: id, title: title)]
        let inner = content()
        let html = "<section><h1 id=\"\(esc(id))\">\(esc(title))</h1>\(inner)</section>"
        return Chapter(title: title, html: html, headings: headings)
    }

    private func secChapter(_ el: XNode, fallbackTitle: String) -> Chapter {
        headings = []
        var html = sec(el, level: 1)
        if headings.isEmpty {   // untitled top-level section: give it a heading so the TOC can reach it
            let id = nextID()
            html = "<h1 id=\"\(id)\">\(esc(fallbackTitle))</h1>" + html
            headings = [Heading(level: 1, id: id, title: fallbackTitle)]
        }
        return Chapter(title: headings.first!.title, html: html, headings: headings)
    }

    private func backChapters(_ back: XNode) -> [Chapter] {
        var out: [Chapter] = []
        for ch in back.elements {
            switch ch.name {
            case "ack":
                let t = ch.child("title")?.plain ?? "Acknowledgements"
                out.append(chapter(title: t, id: ch.attr("id") ?? nextID()) { self.blocks(ch, level: 2, skip: ["title"]) })
            case "sec", "notes", "glossary":
                out.append(secChapter(ch, fallbackTitle: "Notes"))
            case "app-group":
                for app in ch.elements where app.name == "app" || app.name == "sec" {
                    out.append(secChapter(app, fallbackTitle: "Appendix"))
                }
            case "fn-group":
                let t = ch.child("title")?.plain ?? "Notes"
                out.append(chapter(title: t, id: nextID()) {
                    let items = ch.children("fn").map { fn -> String in
                        let idAttr = fn.attr("id").map { " id=\"\(esc($0))\"" } ?? ""
                        return "<li\(idAttr)>\(self.blocks(fn, level: 3, skip: ["label"]))</li>"
                    }.joined()
                    return "<ol class=\"notes\">\(items)</ol>"
                })
            default:
                continue
            }
        }
        // reference lists that are not already rendered inside a back-matter section
        for rl in back.descendants("ref-list") where !hasSectionAncestor(rl, below: back) {
            if rl.parent?.name == "ref-list" { continue }
            let t = rl.child("title")?.plain ?? "References"
            out.append(chapter(title: t, id: rl.attr("id") ?? "references") { self.refItems(rl) })
        }
        return out
    }

    private func hasSectionAncestor(_ n: XNode, below stop: XNode) -> Bool {
        var p = n.parent
        while let x = p, x !== stop {
            if ["sec", "app", "ack", "notes", "glossary"].contains(x.name) { return true }
            p = x.parent
        }
        return false
    }

    private func nextID() -> String {
        autoID += 1
        return "p2e-s\(autoID)"
    }

    // MARK: - Blocks

    func sec(_ el: XNode, level: Int) -> String {
        let titleEl = el.child("title")
        let label = (el.childText("label") ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        var titleHTML = titleEl.map { inline($0) } ?? ""
        var plainTitle = titleEl?.plain ?? ""
        if !label.isEmpty {
            titleHTML = esc(label) + " " + titleHTML
            plainTitle = label + " " + plainTitle
        }
        let h = min(level, 6)
        let inner = blocks(el, level: level + 1, skip: ["title", "label"], deferHeading: true)
        let trimmed = titleHTML.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmed.isEmpty {
            let idAttr = el.attr("id").map { " id=\"\(esc($0))\"" } ?? ""
            return "<section\(idAttr)>\(inner.html)</section>"
        }
        let id = el.attr("id") ?? nextID()
        if level <= 2 {
            // headings are recorded in document order: this section before its children
            headings.insert(Heading(level: level, id: id, title: plainTitle.trimmingCharacters(in: .whitespaces)),
                            at: inner.headingMark)
        }
        return "<section><h\(h) id=\"\(esc(id))\">\(trimmed)</h\(h)>\(inner.html)</section>"
    }

    func blocks(_ el: XNode, level: Int, skip: Set<String> = []) -> String {
        blocks(el, level: level, skip: skip, deferHeading: false).html
    }

    /// `headingMark` is the index in `headings` before the children were rendered, so a parent
    /// section can insert its own heading in front of its subsections' headings.
    private func blocks(_ el: XNode, level: Int, skip: Set<String>, deferHeading: Bool) -> (html: String, headingMark: Int) {
        let mark = headings.count
        var out = ""
        for ch in el.children {
            if ch.isText {
                let t = ch.text.trimmingCharacters(in: .whitespacesAndNewlines)
                if !t.isEmpty { out += "<p>\(esc(t))</p>" }
            } else if !skip.contains(ch.name) {
                out += block(ch, level: level)
            }
        }
        return (out, mark)
    }

    func block(_ ch: XNode, level: Int) -> String {
        if ch.isMath && ch.name == "math" { return "<p>\(mathml(ch, display: true))</p>" }
        switch ch.name {
        case "sec":
            return sec(ch, level: level)
        case "p":
            return para(ch)
        case "fig":
            return fig(ch)
        case "fig-group", "table-wrap-group", "disp-formula-group", "boxed-text", "statement", "chem-struct-wrap", "glossary":
            let title = ch.path("caption/title") ?? ch.child("title")
            let label = (ch.childText("label") ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
            var head = ""
            if ["boxed-text", "statement"].contains(ch.name), title != nil || !label.isEmpty {
                head = "<p><strong>\(esc(label)) \(title.map { inline($0) } ?? "")</strong></p>"
            }
            let cls = ["boxed-text", "statement"].contains(ch.name) ? "box" : ch.name
            return "<div class=\"\(cls)\">\(head)\(blocks(ch, level: level, skip: ["label", "caption", "title"]))</div>"
        case "table-wrap":
            return tableWrap(ch)
        case "disp-formula":
            return formula(ch, display: true)
        case "list":
            return list(ch)
        case "disp-quote", "verse-group":
            return "<blockquote>\(blocks(ch, level: level))</blockquote>"
        case "preformat", "code":
            return "<pre><code>\(esc(ch.allText))</code></pre>"
        case "def-list":
            let items = ch.children("def-item").map { di -> String in
                let term = di.child("term").map { inline($0) } ?? ""
                let def = di.child("def").map { blocks($0, level: level) } ?? ""
                return "<dt>\(term)</dt><dd>\(def)</dd>"
            }.joined()
            return items.isEmpty ? "" : "<dl>\(items)</dl>"
        case "supplementary-material":
            return supp(ch)
        case "graphic":
            if ch.attr("content-type") == "thumb" { return "" }
            if let src = image(ch.attr("href")) { return "<figure><img src=\"\(esc(src))\" alt=\"\"/></figure>" }
            return ""
        case "alternatives":
            return blocks(ch, level: level)
        case "media", "title", "label", "object-id", "permissions", "attrib":
            return ""
        case "ref-list":
            let t = ch.child("title")?.plain ?? "References"
            return "<section class=\"references\"><h\(min(level, 6))>\(esc(t))</h\(min(level, 6))>\(refItems(ch))</section>"
        case "fn-group", "fn":
            return "<div class=\"notes\">\(blocks(ch, level: level, skip: ["label"]))</div>"
        default:
            let inner = inline(ch)
            return inner.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? "" : "<p>\(inner)</p>"
        }
    }

    func para(_ el: XNode) -> String {
        var out = ""
        var buf = ""
        func flush() {
            let s = buf.trimmingCharacters(in: .whitespacesAndNewlines)
            if !s.isEmpty { out += "<p>\(s)</p>" }
            buf = ""
        }
        for ch in el.children {
            if ch.isText {
                buf += esc(ch.text)
            } else if Self.blockTags.contains(ch.name) {
                flush()
                out += block(ch, level: 3)
            } else {
                buf += node(ch)
            }
        }
        flush()
        return out + citedFloats(el)
    }

    func inline(_ el: XNode, skip: Set<String> = []) -> String {
        var out = ""
        for ch in el.children {
            if ch.isText { out += esc(ch.text) } else if !skip.contains(ch.name) { out += node(ch) }
        }
        return out
    }

    static let simpleInline: [String: String] = [
        "italic": "em", "bold": "strong", "sup": "sup", "sub": "sub", "underline": "u",
        "monospace": "code", "strike": "s", "overline": "span",
    ]

    func node(_ ch: XNode) -> String {
        if ch.isMath && ch.name == "math" { return mathml(ch, display: false) }
        if let t = Self.simpleInline[ch.name] { return "<\(t)>\(inline(ch))</\(t)>" }
        switch ch.name {
        case "sc":
            return "<span class=\"smallcaps\">\(inline(ch))</span>"
        case "ext-link", "uri", "self-uri":
            var href = ch.attr("href") ?? ch.allText.trimmingCharacters(in: .whitespacesAndNewlines)
            if ch.attr("ext-link-type") == "doi", !href.hasPrefix("http") { href = "https://doi.org/" + href }
            guard href.hasPrefix("http") || href.hasPrefix("mailto:") || href.hasPrefix("ftp") else { return inline(ch) }
            return "<a href=\"\(esc(href))\">\(inline(ch))</a>"
        case "xref":
            let rid = (ch.attr("rid") ?? "").split(separator: " ").first.map(String.init) ?? ""
            let txt = inline(ch)
            let shown = txt.isEmpty ? esc(rid) : txt
            return rid.isEmpty ? shown : "<a href=\"#\(esc(rid))\">\(shown)</a>"
        case "inline-formula":
            return formula(ch, display: false)
        case "disp-formula":
            return formula(ch, display: true)
        case "tex-math":
            return texmath(ch, display: false)
        case "inline-graphic":
            if let src = image(ch.attr("href")) { return "<img class=\"math\" src=\"\(esc(src))\" alt=\"\"/>" }
            return ""
        case "break":
            return "<br/>"
        case "fn":
            return " <span class=\"fn\">[\(esc(ch.plain))]</span>"
        case "label":
            return "<strong>\(inline(ch))</strong> "
        case "email":
            let e = ch.allText.trimmingCharacters(in: .whitespacesAndNewlines)
            return "<a href=\"mailto:\(esc(e))\">\(esc(e))</a>"
        case "object-id", "permissions", "media", "institution-id":
            return ""
        case "alternatives":
            return alternatives(ch, display: false)
        default:
            return inline(ch)
        }
    }

    // MARK: - Math

    func alternatives(_ el: XNode, display: Bool) -> String {
        if let m = el.elements.first(where: { $0.isMath && $0.name == "math" }) { return mathml(m, display: display) }
        if let t = el.child("tex-math") { return texmath(t, display: display) }
        if let g = el.child("graphic") ?? el.child("inline-graphic"), let src = image(g.attr("href")) {
            return "<img class=\"math\" src=\"\(esc(src))\" alt=\"\"/>"
        }
        return inline(el)
    }

    func formula(_ el: XNode, display: Bool) -> String {
        var inner = ""
        for ch in el.elements {
            if ch.name == "alternatives" { inner = alternatives(ch, display: display) }
            else if ch.isMath && ch.name == "math" { inner = mathml(ch, display: display) }
            else if ch.name == "tex-math" { inner = texmath(ch, display: display) }
            else if ch.name == "graphic", let src = image(ch.attr("href")) {
                inner = "<img class=\"math\" src=\"\(esc(src))\" alt=\"\"/>"
            }
        }
        if inner.isEmpty { inner = inline(el, skip: ["label"]) }
        if !display { return inner }
        let label = (el.childText("label") ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        let idAttr = el.attr("id").map { " id=\"\(esc($0))\"" } ?? ""
        let lab = label.isEmpty ? "" : " <span class=\"eqlabel\">\(esc(label))</span>"
        return "<div class=\"equation\"\(idAttr)><p>\(inner)\(lab)</p></div>"
    }

    func mathml(_ m: XNode, display: Bool) -> String {
        var s = "<math xmlns=\"\(Self.mathNS)\""
        if display { s += " display=\"block\"" }
        for k in m.attrOrder where !["id", "display", "xmlns"].contains(k) {
            s += " \(k)=\"\(esc(m.attrs[k] ?? ""))\""
        }
        s += ">"
        for c in m.children { s += serialize(c) }
        return s + "</math>"
    }

    private func serialize(_ n: XNode) -> String {
        if n.isText { return esc(n.text) }
        var s = "<\(n.name)"
        for k in n.attrOrder where k != "id" {
            // EPUB's MathML schema rejects displaystyle on most elements other than these
            if k == "displaystyle", !["mstyle", "mtable", "math"].contains(n.name) { continue }
            s += " \(k)=\"\(esc(n.attrs[k] ?? ""))\""
        }
        if n.children.isEmpty { return s + "/>" }
        s += ">"
        for c in n.children { s += serialize(c) }
        return s + "</\(n.name)>"
    }

    /// Without pandoc there is no TeX → MathML converter yet; keep the source readable.
    func texmath(_ el: XNode, display: Bool) -> String {
        var t = el.allText
        if let r = t.range(of: "\\begin{document}") { t = String(t[r.upperBound...]) }
        if let r = t.range(of: "\\end{document}") { t = String(t[..<r.lowerBound]) }
        t = t.trimmingCharacters(in: .whitespacesAndNewlines).trimmingCharacters(in: CharacterSet(charactersIn: "$"))
        texFormulas += 1
        return "<code class=\"tex\">\(esc(t.trimmingCharacters(in: .whitespacesAndNewlines)))</code>"
    }

    // MARK: - Figures and tables

    func image(_ href: String?) -> String? {
        guard let href, let img = images[href] else { return nil }
        return "../images/\(img.fileName)"
    }

    func caption(_ el: XNode) -> String {
        var parts: [String] = []
        let label = (el.childText("label") ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        if !label.isEmpty { parts.append("<strong>\(esc(label))</strong>") }
        if let cap = el.child("caption") {
            if let t = cap.child("title") { parts.append("<strong>\(inline(t))</strong>") }
            for p in cap.children("p") {
                let txt = inline(p)
                if txt.hasPrefix("<strong>DOI:</strong>") { continue }
                parts.append(txt)
            }
        }
        return parts.joined(separator: " ")
    }

    /// Image tags for the graphics under `el`, taking one per `<alternatives>` group.
    private func graphicImgs(_ el: XNode, alt: String) -> [String] {
        var out: [String] = []
        var seenAlt = Set<ObjectIdentifier>()
        var seenSrc = Set<String>()
        for g in el.descendants("graphic") where g.attr("content-type") != "thumb" {
            if let p = g.parent, p.name == "alternatives", seenAlt.contains(ObjectIdentifier(p)) { continue }
            guard let src = image(g.attr("href")), !seenSrc.contains(src) else { continue }
            if let p = g.parent, p.name == "alternatives" { seenAlt.insert(ObjectIdentifier(p)) }
            seenSrc.insert(src)
            out.append("<img src=\"\(esc(src))\" alt=\"\(esc(alt))\"/>")
        }
        return out
    }

    func fig(_ el: XNode) -> String {
        let label = (el.childText("label") ?? "Figure").trimmingCharacters(in: .whitespacesAndNewlines)
        var imgs = graphicImgs(el, alt: label)
        if imgs.isEmpty {
            missingFigures += 1
            imgs = ["<span class=\"missing\">[Figure image not available]</span>"]
        }
        let idAttr = el.attr("id").map { " id=\"\(esc($0))\"" } ?? ""
        return "<figure\(idAttr)>\(imgs.joined())<figcaption>\(caption(el))</figcaption></figure>"
    }

    func tableWrap(_ el: XNode) -> String {
        let tables = el.descendants("table")
        var body = tables.map(table)
        if tables.isEmpty { body = graphicImgs(el, alt: "Table") }
        let foot = el.child("table-wrap-foot").map { "<div class=\"tablefoot\">\(blocks($0, level: 4))</div>" } ?? ""
        let idAttr = el.attr("id").map { " id=\"\(esc($0))\"" } ?? ""
        return "<div class=\"table-wrap\"\(idAttr)><p class=\"caption\">\(caption(el))</p>\(body.joined())\(foot)</div>"
    }

    func table(_ t: XNode) -> String {
        func cell(_ c: XNode) -> String {
            var attrs = ""
            for a in ["rowspan", "colspan"] { if let v = c.attr(a) { attrs += " \(a)=\"\(esc(v))\"" } }
            return "<\(c.name)\(attrs)>\(inline(c))</\(c.name)>"
        }
        func row(_ tr: XNode) -> String {
            "<tr>" + tr.elements.filter { $0.name == "td" || $0.name == "th" }.map(cell).joined() + "</tr>"
        }
        var s = "<table>"
        if let th = t.child("thead") { s += "<thead>" + th.descendants("tr").map(row).joined() + "</thead>" }
        let bodies = t.children("tbody")
        if !bodies.isEmpty {
            for b in bodies { s += "<tbody>" + b.descendants("tr").map(row).joined() + "</tbody>" }
        } else {
            s += "<tbody>" + t.children("tr").map(row).joined() + "</tbody>"
        }
        return s + "</table>"
    }

    func list(_ el: XNode) -> String {
        let typ = el.attr("list-type") ?? "bullet"
        let ordered = ["order", "alpha-lower", "alpha-upper", "roman-lower", "roman-upper"].contains(typ)
        let tag = ordered ? "ol" : "ul"
        var style = ["alpha-lower": " type=\"a\"", "alpha-upper": " type=\"A\"", "roman-lower": " type=\"i\"",
                     "roman-upper": " type=\"I\""][typ] ?? ""
        if typ == "simple" { style = " class=\"simple\"" }
        let items = el.children("list-item").map { "<li>\(blocks($0, level: 4, skip: ["label"]))</li>" }.joined()
        return items.isEmpty ? "" : "<\(tag)\(style)>\(items)</\(tag)>"
    }

    func supp(_ el: XNode) -> String {
        let label = (el.childText("label") ?? "Supplementary material").trimmingCharacters(in: .whitespacesAndNewlines)
        let txt = el.child("caption")?.child("title").map { inline($0) } ?? ""
        let href = el.attr("href") ?? el.firstDescendant("media")?.attr("href") ?? ""
        let idAttr = el.attr("id").map { " id=\"\(esc($0))\"" } ?? ""
        let hrefPart = href.isEmpty ? "" : " <em>(\(esc(href)))</em>"
        return "<p class=\"supp\"\(idAttr)><strong>\(esc(label))</strong> \(txt)\(hrefPart)</p>"
    }

    // MARK: - References

    func refItems(_ rl: XNode) -> String {
        var items = ""
        for ref in rl.children("ref") {
            guard let cit = ["element-citation", "mixed-citation", "citation", "nlm-citation"]
                .lazy.compactMap({ ref.child($0) }).first else { continue }
            let idAttr = ref.attr("id").map { " id=\"\(esc($0))\"" } ?? ""
            items += "<li\(idAttr)>\(citation(cit))</li>"
        }
        return "<ol class=\"refs\">\(items)</ol>"
    }

    func citation(_ c: XNode) -> String {
        let pubDOI = c.descendants("pub-id").first { $0.attr("pub-id-type") == "doi" }?.allText
            .trimmingCharacters(in: .whitespacesAndNewlines)
        func doiLink(_ d: String) -> String { "<a href=\"https://doi.org/\(esc(d))\">doi:\(esc(d))</a>" }

        let ownText = c.children.filter(\.isText).map(\.text).joined().trimmingCharacters(in: .whitespacesAndNewlines)
        if c.name == "mixed-citation", ownText.count > 10 {
            var s = inline(c, skip: ["pub-id"])
            if let d = pubDOI, !d.isEmpty { s += " " + doiLink(d) }
            return s
        }
        var names: [String] = []
        let groups = c.children("person-group")
        for pg in groups.isEmpty ? [c] : groups {
            for n in pg.elements where n.name == "name" || n.name == "string-name" {
                let sur = n.childText("surname") ?? ""
                let giv = n.childText("given-names") ?? ""
                let full = "\(sur) \(giv)".trimmingCharacters(in: .whitespacesAndNewlines)
                names.append(full.isEmpty ? n.plain : full)
            }
            for col in pg.children("collab") { names.append(col.plain) }
        }
        if names.count > 10 { names = Array(names.prefix(10)) + ["et al."] }
        let year = c.childText("year") ?? ""
        let titleEl = c.child("article-title") ?? c.child("chapter-title")
        let title = titleEl.map { inline($0) } ?? ""
        let src = c.child("source").map { inline($0) } ?? ""
        let vol = c.childText("volume") ?? "", fp = c.childText("fpage") ?? "", lp = c.childText("lpage") ?? ""
        let eloc = c.childText("elocation-id") ?? ""
        var parts: [String] = []
        if !names.isEmpty { parts.append(esc(names.joined(separator: ", ")) + ".") }
        if !year.isEmpty { parts.append(esc(year) + ".") }
        if !title.isEmpty { parts.append(title.hasSuffix(".") ? title : title + ".") }
        if !src.isEmpty {
            var s = "<em>\(src)</em>"
            if !vol.isEmpty { s += " <strong>\(esc(vol))</strong>" }
            if !fp.isEmpty { s += ":" + esc(fp) + (lp.isEmpty ? "" : "–" + esc(lp)) } else if !eloc.isEmpty { s += ":" + esc(eloc) }
            parts.append(s + ".")
        }
        if let d = pubDOI, !d.isEmpty { parts.append(doiLink(d)) }
        else if let pmid = c.descendants("pub-id").first(where: { $0.attr("pub-id-type") == "pmid" })?.allText {
            let p = pmid.trimmingCharacters(in: .whitespacesAndNewlines)
            parts.append("<a href=\"https://pubmed.ncbi.nlm.nih.gov/\(esc(p))\">PMID \(esc(p))</a>")
        }
        return parts.isEmpty ? inline(c) : parts.joined(separator: " ")
    }
}
