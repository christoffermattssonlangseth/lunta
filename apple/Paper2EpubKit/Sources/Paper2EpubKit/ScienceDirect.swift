import Foundation

/// ScienceDirect (Elsevier: Cell Press, The Lancet, …) article pages as the browser shows them to
/// a reader who has access — saved from the browser ("Webpage, Complete") or sent by Lunta's
/// browser extension. The page has the full text with proper structure, figures with their
/// captions, real equations and the reference list, which makes a far better reading view than
/// the PDF of a paper that isn't open access.
///
/// Equations: the page may already have been typeset by MathJax (a saved page always is), which
/// replaces the MathML with its own markup (`mjx-*` elements). That markup mirrors MathML closely
/// (mjx-msup → msup, …, each character in an `mjx-c` with its code point), so it is turned back
/// into MathML here.
enum ScienceDirect {
    static func looksLike(_ html: String) -> Bool {
        html.contains("id=\"body\"") && (html.contains("sciencedirect.com") || html.contains("els-cdn.com"))
    }

    /// `dir`: the folder the page was saved in (its `…_files` folder has the images), or nil.
    static func book(html: String, dir: URL?, log: @escaping (String) -> Void) async throws -> Book {
        guard let body = element(html, id: "body") else { throw fail("This ScienceDirect page has no full text (are you signed in?).") }
        var meta = metadata(html)
        log("Converting the ScienceDirect page")
        let conv = Converter()

        var chapters: [Chapter] = []
        // Highlights, Summary, Graphical abstract
        if let abs = element(html, id: "abstracts"), let root = try? parse(abs) {
            var inner = ""
            var heads = [Heading(level: 1, id: "abstract", title: "Summary")]
            for a in root.descendants("div") where a.hasClass("abstract") && !(a.parent.map { $0.hasClass("abstract") } ?? false) {
                let title = a.children.first { $0.name == "h2" }?.plain.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
                let content = a.children.filter { $0.name != "h2" }.map { conv.render($0) }.joined()
                if title == "Summary" || title == "Abstract" || title.isEmpty {
                    inner += content
                    if meta.abstract.isEmpty { meta.abstract = a.children.filter { $0.name != "h2" }.map(\.plain).joined(separator: " ") }
                } else {
                    let id = a.attr("id") ?? "abs-\(heads.count)"
                    heads.append(Heading(level: 2, id: id, title: title))
                    inner += "<h2 id=\"\(esc(id))\">\(esc(title))</h2>\(content)"
                }
            }
            chapters.append(Chapter(title: "Summary", html: "<section><h1 id=\"abstract\">Summary</h1>\(inner)</section>", headings: heads))
        }

        // one chapter per top-level section of the body
        let root = try parse(body)
        func topSections(_ n: XNode) -> [XNode] {
            n.elements.flatMap { $0.name == "section" ? [$0] : topSections($0) }
        }
        for s in topSections(root) {
            conv.headings = []
            let html = conv.render(s, sectionDepth: 0)
            let title = conv.headings.first?.title ?? "Section"
            chapters.append(Chapter(title: title, html: html, headings: conv.headings))
        }

        // references
        if let start = html.range(of: #"<section[^>]*class="bibliography"#, options: .regularExpression),
           let bib = element(html, from: start.lowerBound, tag: "section"), let broot = try? parse(bib) {
            var items = ""
            for li in broot.descendants("li") {
                guard let ref = li.descendants("span").first(where: { $0.hasClass("reference") }) else { continue }
                let id = li.descendants("a").compactMap { $0.attr("id") }.first { $0.hasPrefix("ref-id-bib") }
                    .map { String($0.dropFirst("ref-id-".count)) }
                items += "<li\(id.map { " id=\"\(esc($0))\"" } ?? "")>\(conv.reference(ref))</li>"
            }
            if !items.isEmpty {
                chapters.append(Chapter(title: "References", html: "<section><h1 id=\"references\">References</h1><ol class=\"refs\">\(items)</ol></section>",
                                        headings: [Heading(level: 1, id: "references", title: "References")]))
            }
        }
        guard chapters.count > 2 else { throw fail("This ScienceDirect page has no full text (are you signed in?).") }

        // figure images: the high-resolution version from Elsevier's image server, else the saved copy
        var images: [BookImage] = []
        var missing = 0
        if !conv.figures.isEmpty { log("Fetching \(conv.figures.count) figure image(s)") }
        for f in conv.figures {
            var data: Data?
            if let hi = f.hiRes, let r = try? await Net.fetch(hi, timeout: 40), (200..<300).contains(r.status) { data = r.data }
            if data == nil, let local = f.local, let dir { data = localFile(local, in: dir) }
            if data == nil, let src = f.remote, let r = try? await Net.fetch(src, timeout: 30), (200..<300).contains(r.status) { data = r.data }
            guard let d = data, let n = Images.normalise(d) else { missing += 1; continue }
            images.append(BookImage(fileName: "\(f.name).\(n.ext)", data: n.data, mediaType: n.mediaType))
        }
        // point each <img> at the file it ended up as (the extension depends on the image)
        let byName = Dictionary(images.map { ($0.fileName.components(separatedBy: ".").first ?? $0.fileName, $0.fileName) }, uniquingKeysWith: { a, _ in a })
        for i in chapters.indices {
            var h = chapters[i].html
            for f in conv.figures {
                if let file = byName[f.name] {
                    h = h.replacingOccurrences(of: "src=\"../images/\(f.name).img\"", with: "src=\"../images/\(file)\"")
                } else {
                    h = h.replacingOccurrences(of: "<img src=\"../images/\(f.name).img\" alt=\"[^\"]*\"/>", with: "<p class=\"missing\"><em>[Figure image not available]</em></p>", options: .regularExpression)
                }
            }
            chapters[i].html = h
        }
        linkAcrossChapters(&chapters)

        var notes: [String] = []
        if missing > 0 { notes.append("\(missing) figure image(s) could not be downloaded; captions are kept.") }
        if conv.mathCount > 0 { notes.append("\(conv.mathCount) equation(s) as MathML.") }
        return Book(meta: meta, chapters: chapters, images: images, sourceLabel: "ScienceDirect full text", notes: notes)
    }

    // MARK: - Page pieces

    static func metadata(_ html: String) -> PaperMeta {
        var m = PaperMeta()
        func meta(_ name: String) -> [String] {
            let re = try! NSRegularExpression(pattern: #"<meta\b[^>]*>"#)
            let ns = html as NSString
            return re.matches(in: html, range: NSRange(location: 0, length: ns.length)).compactMap { r -> String? in
                let tag = ns.substring(with: r.range)
                guard tag.contains("name=\"\(name)\""), let c = tag.range(of: #"content="([^"]*)""#, options: .regularExpression) else { return nil }
                return unescape(String(tag[c].dropFirst("content=\"".count).dropLast()))
            }
        }
        m.title = meta("citation_title").first ?? ""
        m.doi = meta("citation_doi").first ?? ""
        m.journal = meta("citation_journal_title").first ?? ""
        m.date = (meta("citation_publication_date").first ?? meta("citation_online_date").first ?? "").replacingOccurrences(of: "/", with: "-")
        m.url = m.doi.isEmpty ? "" : "https://doi.org/\(m.doi)"
        // authors: "given-name" + "surname" spans in the author list
        let re = try! NSRegularExpression(pattern: #"<span class="given-name">([^<]*)</span>\s*<span class="text surname">([^<]*)</span>"#)
        let ns = html as NSString
        var seen = Set<String>()
        for r in re.matches(in: html, range: NSRange(location: 0, length: ns.length)) {
            let name = unescape(ns.substring(with: r.range(at: 1)) + " " + ns.substring(with: r.range(at: 2)))
            if seen.insert(name).inserted { m.authors.append(name) }
        }
        return m
    }

    /// The element with this id, opening tag to its matching closing tag.
    static func element(_ html: String, id: String) -> String? {
        guard let r = html.range(of: "id=\"\(id)\"") , let open = html.range(of: "<", options: .backwards, range: html.startIndex..<r.lowerBound) else { return nil }
        let tag = html[html.index(after: open.lowerBound)...].prefix { $0.isLetter || $0.isNumber || $0 == "-" }
        return element(html, from: open.lowerBound, tag: String(tag))
    }

    static func element(_ html: String, from start: String.Index, tag: String) -> String? {
        let re = try! NSRegularExpression(pattern: "<(/?)\(tag)\\b[^>]*>")
        let ns = html as NSString
        let loc = html.utf16.distance(from: html.utf16.startIndex, to: start)
        var depth = 0
        var end: Int?
        re.enumerateMatches(in: html, range: NSRange(location: loc, length: ns.length - loc)) { m, _, stop in
            guard let m else { return }
            if ns.substring(with: m.range(at: 1)) == "/" { depth -= 1 } else if !ns.substring(with: m.range).hasSuffix("/>") { depth += 1 }
            if depth == 0 { end = m.range.location + m.range.length; stop.pointee = true }
        }
        guard let end else { return nil }
        return ns.substring(with: NSRange(location: loc, length: end - loc))
    }

    /// Browser HTML → the converter's DOM: scripts, styles, icons and MathJax's speech data
    /// removed, void elements closed, stray ampersands escaped.
    static func parse(_ fragment: String) throws -> XNode {
        var s = fragment
        let fixes: [(String, String)] = [
            (#"<!--[\s\S]*?-->"#, ""),
            (#"<script\b[\s\S]*?</script>"#, ""),
            (#"<style\b[\s\S]*?</style>"#, ""),
            (#"<svg\b[\s\S]*?</svg>"#, ""),
            (#"<button\b[\s\S]*?</button>"#, ""),
            (#"<mjx-speech\b[\s\S]*?</mjx-speech>"#, ""),
            (#"\s(?:data-semantic-[\w-]+|data-aa-[\w-]+|aria-[\w-]+|style|tabindex|ctxtmenu_counter)="[^"]*""#, ""),
            (#"<(img|br|hr|meta|link|input|col|source|wbr|area|embed|param|track)\b((?:"[^"]*"|'[^']*'|[^'">])*?)\s*/?>"#, "<$1$2/>"),
            (#"&(?!#?[A-Za-z0-9]+;)"#, "&amp;"),
        ]
        for (p, r) in fixes { s = s.replacingOccurrences(of: p, with: r, options: .regularExpression) }
        return try XML.parse(Data("<div>\(s)</div>".utf8))
    }

    static func localFile(_ src: String, in dir: URL) -> Data? {
        let name = (src.removingPercentEncoding ?? src).components(separatedBy: "/").last ?? src
        let fm = FileManager.default
        if let d = try? Data(contentsOf: dir.appendingPathComponent(src.removingPercentEncoding ?? src)) { return d }
        // the saved page's "…_files" folder (or any folder next to it)
        for sub in (try? fm.contentsOfDirectory(at: dir, includingPropertiesForKeys: nil)) ?? [] {
            if let d = try? Data(contentsOf: sub.appendingPathComponent(name)) { return d }
        }
        return nil
    }

    static func unescape(_ s: String) -> String {
        var t = s.replacingOccurrences(of: "&quot;", with: "\"").replacingOccurrences(of: "&#39;", with: "'")
            .replacingOccurrences(of: "&lt;", with: "<").replacingOccurrences(of: "&gt;", with: ">")
            .replacingOccurrences(of: "&nbsp;", with: "\u{00A0}")
        while let r = t.range(of: #"&#(x?)([0-9A-Fa-f]+);"#, options: .regularExpression) {
            let body = t[r].dropFirst(2).dropLast()
            let v = body.hasPrefix("x") ? UInt32(body.dropFirst(), radix: 16) : UInt32(body)
            t.replaceSubrange(r, with: v.flatMap(Unicode.Scalar.init).map { String(Character($0)) } ?? "")
        }
        return t.replacingOccurrences(of: "&amp;", with: "&")
    }

    /// `href="#x"` → `chNN.xhtml#x` when `x` is in another chapter (each chapter is its own file).
    static func linkAcrossChapters(_ chapters: inout [Chapter]) {
        var where_: [String: Int] = [:]
        let idRE = try! NSRegularExpression(pattern: #"\sid="([^"]+)""#)
        for (i, c) in chapters.enumerated() {
            let ns = c.html as NSString
            for m in idRE.matches(in: c.html, range: NSRange(location: 0, length: ns.length)) { where_[ns.substring(with: m.range(at: 1))] = i }
        }
        let hrefRE = try! NSRegularExpression(pattern: ##"href="#([^"]+)""##)
        for i in chapters.indices {
            let ns = chapters[i].html as NSString
            var out = ""
            var last = 0
            for m in hrefRE.matches(in: chapters[i].html, range: NSRange(location: 0, length: ns.length)) {
                let id = ns.substring(with: m.range(at: 1))
                guard let j = where_[id], j != i else { continue }
                out += ns.substring(with: NSRange(location: last, length: m.range.location - last))
                out += "href=\"" + String(format: "ch%02d.xhtml", j + 1) + "#\(id)\""
                last = m.range.location + m.range.length
            }
            out += ns.substring(from: last)
            chapters[i].html = out
        }
    }

    // MARK: - Rendering

    struct Figure {
        var name: String        // file name without extension
        var hiRes: URL?
        var local: String?      // src as saved
        var remote: URL?
    }

    final class Converter {
        var headings: [Heading] = []
        var figures: [Figure] = []
        var mathCount = 0

        private static let blocks: Set<String> = ["div", "section", "figure", "table", "ul", "ol", "p", "h2", "h3", "h4", "h5", "dl"]

        func render(_ n: XNode, sectionDepth: Int = 0) -> String {
            if n.isText { return esc(n.text) }
            let cls = n.attr("class") ?? ""
            switch n.name {
            case "section":
                let id = n.attr("id")
                return "<section\(id.map { " id=\"\(esc($0))\"" } ?? "")>" + n.children.map { render($0, sectionDepth: sectionDepth + 1) }.joined() + "</section>"
            case "h2", "h3", "h4", "h5", "h6":
                let title = n.plain.trimmingCharacters(in: .whitespacesAndNewlines)
                guard !title.isEmpty else { return "" }
                let level = max(1, min(4, sectionDepth))
                let id = n.parent?.name == "section" ? (n.parent?.attr("id") ?? n.attr("id") ?? "h\(headings.count)") : (n.attr("id") ?? "h\(headings.count)")
                if level <= 2 { headings.append(Heading(level: level, id: "h-" + id, title: title)) }
                return "<h\(level) id=\"h-\(esc(id))\">\(esc(title))</h\(level)>"
            case "figure":
                return figure(n)
            case "table":
                return "<table>" + n.children.map(tablePart).joined() + "</table>"
            case "mjx-container":
                return math(n)
            case "img", "svg", "button", "input", "noscript", "dl", "mjx-speech":
                return ""
            case "br":
                return "<br/>"
            case "sup", "sub", "i", "em", "b", "strong", "u", "small", "code":
                return "<\(n.name)>" + inline(n) + "</\(n.name)>"
            case "a":
                if cls.contains("download-link") { return "" }
                let href = n.attr("href") ?? ""
                if let hash = href.range(of: "#"), href.contains("sciencedirect.com/science/article") || href.hasPrefix("#") {
                    let frag = String(href[hash.upperBound...])
                    return frag.isEmpty ? inline(n) : "<a href=\"#\(esc(frag))\">" + inline(n) + "</a>"
                }
                if href.hasPrefix("http") { return "<a href=\"\(esc(href))\">" + inline(n) + "</a>" }
                return inline(n)
            case "ul", "ol":
                if cls.contains("u-margin-s-bottom") && n.parent?.parent?.name == "figure" { return "" }
                let items = n.children.filter { $0.name == "li" }.map { "<li>" + listItem($0) + "</li>" }.joined()
                return items.isEmpty ? "" : "<ul>\(items)</ul>"
            case "p":
                let inner = n.children.map { render($0, sectionDepth: sectionDepth) }.joined()
                return inner.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? "" : "<p>\(inner)</p>"
            case "div":
                if ["ReferenceLinks", "download", "keywords", "footnote", "related", "u-display-none"].contains(where: cls.contains) { return "" }
                if cls.contains("tables") { return tableWrap(n) }
                // anything block-level anywhere inside (a table wrapped in spans, …): no paragraph around it
                func deepBlock(_ x: XNode) -> Bool {
                    x.elements.contains { Self.blocks.contains($0.name) || $0.hasClass("captions") || ($0.name != "mjx-container" && deepBlock($0)) }
                }
                let hasBlock = deepBlock(n)
                if hasBlock { return n.children.map { render($0, sectionDepth: sectionDepth) }.joined() }
                let inner = inline(n)
                return inner.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? "" : "<p>\(inner)</p>"
            case "span":
                if cls.contains("display") {
                    // a displayed equation inside a paragraph
                    return n.children.map { render($0, sectionDepth: sectionDepth) }.joined()
                }
                if cls.contains("captions") { return "" }      // handled by figure / tableWrap
                return n.children.map { render($0, sectionDepth: sectionDepth) }.joined()
            default:
                return n.children.map { render($0, sectionDepth: sectionDepth) }.joined()
            }
        }

        private func inline(_ n: XNode) -> String { n.children.map { render($0) }.joined() }

        private func listItem(_ li: XNode) -> String {
            li.children.filter { !$0.hasClass("list-label") }.map { c -> String in
                // "list-content" may hold paragraph divs: keep them inline in the item
                if c.hasClass("list-content") {
                    return c.children.map { x in x.name == "div" && !x.elements.contains { Self.blocks.contains($0.name) } ? inline(x) : render(x) }.joined()
                }
                return render(c)
            }.joined()
        }

        private func figure(_ n: XNode) -> String {
            let id = n.attr("id") ?? "fig\(figures.count + 1)"
            guard let img = n.descendants("img").first, let src = img.attr("src") else { return "" }
            let hi = n.descendants("a").compactMap { $0.attr("href") }.first { $0.contains("_lrg.") }.flatMap(URL.init(string:))
            let name = "fig-" + id.replacingOccurrences(of: #"[^A-Za-z0-9_-]"#, with: "-", options: .regularExpression)
            figures.append(Figure(name: name, hiRes: hi, local: src.hasPrefix("http") ? nil : src, remote: src.hasPrefix("http") ? URL(string: src) : nil))
            var label = ""
            var caption = ""
            if let caps = n.descendants("span").first(where: { $0.hasClass("captions") }) {
                var first = true
                for part in caps.descendants("p") + caps.descendants("div") where part.parent?.parent?.hasClass("captions") == true || part.parent?.hasClass("captions") == true {
                    if first, part.name == "p", let l = part.descendants("span").first(where: { $0.hasClass("label") }) {
                        label = l.plain.trimmingCharacters(in: .whitespaces)
                        let rest = part.children.filter { $0 !== l }.map { render($0) }.joined()
                            .replacingOccurrences(of: #"^\s*[.:|]\s*"#, with: "", options: .regularExpression)
                        caption += "<strong>\(esc(label)).</strong> <strong>\(rest)</strong>"
                    } else {
                        caption += "<span class=\"cap-p\"> \(inline(part))</span>"
                    }
                    first = false
                }
            }
            let alt = label.isEmpty ? "Figure" : label
            return "<figure id=\"\(esc(id))\"><img src=\"../images/\(name).img\" alt=\"\(esc(alt))\"/>"
                + (caption.isEmpty ? "" : "<figcaption>\(caption)</figcaption>") + "</figure>"
        }

        private func tableWrap(_ n: XNode) -> String {
            var out = ""
            if let caps = n.descendants("span").first(where: { $0.hasClass("captions") }) {
                let t = inline(caps).trimmingCharacters(in: .whitespacesAndNewlines)
                if !t.isEmpty { out += "<p class=\"table-caption\"><strong>\(t)</strong></p>" }
            }
            for t in n.descendants("table") { out += render(t) }
            let id = n.attr("id")
            return "<div class=\"table-wrap\"\(id.map { " id=\"\(esc($0))\"" } ?? "")>\(out)</div>"
        }

        private func tablePart(_ n: XNode) -> String {
            switch n.name {
            case "thead", "tbody", "tfoot", "tr":
                return "<\(n.name)>" + n.children.map(tablePart).joined() + "</\(n.name)>"
            case "th", "td":
                var attrs = ""
                for a in ["colspan", "rowspan"] { if let v = n.attr(a) { attrs += " \(a)=\"\(esc(v))\"" } }
                return "<\(n.name)\(attrs)>" + n.children.map { c in c.name == "div" ? inline(c) : render(c) }.joined() + "</\(n.name)>"
            default:
                return n.isText ? "" : n.children.map(tablePart).joined()
            }
        }

        /// One reference: authors, title, then the journal line with its DOI link.
        func reference(_ ref: XNode) -> String {
            var parts: [String] = []
            for c in ref.descendants("div") {
                if c.hasClass("authors") { parts.append(esc(c.plain.trimmingCharacters(in: .whitespaces))) }
                else if c.hasClass("title") { parts.append(esc(c.plain.trimmingCharacters(in: .whitespaces))) }
                else if c.hasClass("host") { parts.append(inline(c).trimmingCharacters(in: .whitespaces)) }
                else if c.hasClass("other-ref") { parts.append(inline(c).trimmingCharacters(in: .whitespaces)) }
            }
            if parts.isEmpty { return esc(ref.plain) }
            return parts.filter { !$0.isEmpty }.joined(separator: ". ").replacingOccurrences(of: "..", with: ".")
        }

        // MARK: MathJax CHTML → MathML

        private static let mathNS = "http://www.w3.org/1998/Math/MathML"

        func math(_ c: XNode) -> String {
            guard let m = c.firstDescendant("mjx-math") else { return esc(c.plain) }
            mathCount += 1
            let block = c.attr("display") == "true"
            return "<math xmlns=\"\(Self.mathNS)\"\(block ? " display=\"block\"" : "")>" + m.elements.map(mml).joined() + "</math>"
        }

        /// The characters of a token element: each `mjx-c` carries its code point in its class
        /// (`mjx-c1D462` = 𝑢); a stretched operator carries it on `mjx-stretchy-v/h`.
        private func chars(_ n: XNode) -> String {
            func code(_ x: XNode) -> String? {
                for t in (x.attr("class") ?? "").split(separator: " ") where t.hasPrefix("mjx-c") {
                    if let v = UInt32(t.dropFirst(5), radix: 16), let u = Unicode.Scalar(v) { return String(Character(u)) }
                }
                return nil
            }
            var out = ""
            func rec(_ x: XNode) {
                for k in x.children where !k.isText {
                    if k.name == "mjx-stretchy-v" || k.name == "mjx-stretchy-h" { out += code(k) ?? ""; continue }
                    if k.name == "mjx-c" { out += code(k) ?? k.plain; continue }
                    rec(k)
                }
            }
            rec(n)
            return out.isEmpty ? n.plain : out
        }

        private func row(_ n: XNode?) -> String {
            guard let n else { return "<mrow/>" }
            let inner = n.elements.map(mml).joined()
            return "<mrow>\(inner)</mrow>"
        }

        private func mml(_ n: XNode) -> String {
            let tag = n.name.hasPrefix("mjx-") ? String(n.name.dropFirst(4)) : n.name
            switch n.name {
            case "mjx-mi", "mjx-mn", "mjx-mo", "mjx-mtext", "mjx-ms":
                return "<\(tag)>\(esc(chars(n)))</\(tag)>"
            case "mjx-mspace":
                return "<mspace width=\"0.28em\"/>"
            case "mjx-msup", "mjx-msub", "mjx-msubsup":
                let base = n.elements.first { $0.name != "mjx-script" }
                let scripts = (n.child("mjx-script")?.elements ?? []).filter { $0.name.hasPrefix("mjx-m") }
                let b = base.map(mml) ?? "<mrow/>"
                if tag == "msubsup", scripts.count >= 2 { return "<msubsup>\(b)\(mml(scripts[1]))\(mml(scripts[0]))</msubsup>" }
                return "<\(tag)>\(b)\(scripts.first.map(mml) ?? "<mrow/>")</\(tag)>"
            case "mjx-mfrac":
                return "<mfrac>\(row(n.firstDescendant("mjx-num")))\(row(n.firstDescendant("mjx-den")))</mfrac>"
            case "mjx-msqrt", "mjx-mroot":
                return "<msqrt>\(row(n.firstDescendant("mjx-box") ?? n))</msqrt>"
            case "mjx-munder", "mjx-mover", "mjx-munderover":
                let base = row(n.firstDescendant("mjx-base") ?? n.elements.first)
                let under = n.firstDescendant("mjx-under"), over = n.firstDescendant("mjx-over")
                if let under, let over { return "<munderover>\(base)\(row(under))\(row(over))</munderover>" }
                if let under { return "<munder>\(base)\(row(under))</munder>" }
                if let over { return "<mover>\(base)\(row(over))</mover>" }
                return base
            case "mjx-mrow", "mjx-mstyle", "mjx-mpadded", "mjx-mphantom", "mjx-menclose", "mjx-mtable", "mjx-mtr", "mjx-mtd":
                let t = ["mjx-mstyle", "mjx-mpadded", "mjx-menclose"].contains(n.name) ? "mrow" : tag
                return "<\(t)>" + n.elements.map(mml).joined() + "</\(t)>"
            default:
                // layout wrappers (mjx-box, mjx-row, …): their math children
                return n.elements.map(mml).joined()
            }
        }
    }
}
