import Foundation

/// A figure image the browser fetched with the reader's access (the app can't fetch paywalled
/// images itself). `urls`: every address the page uses for it (the high-resolution link, the
/// `src` in the page, …), so the converter can find it whichever one its reader looks at.
public struct PageImage: Sendable {
    public var urls: [String]
    public var data: Data
    public init(urls: [String], data: Data) { self.urls = urls; self.data = data }
}

/// Looks posted images up by any of their addresses, then by file name (`…-gr1.jpg` and
/// `…-gr1_lrg.jpg` are the same figure) when that name is unambiguous.
final class PageImages {
    private var byURL: [String: Int] = [:]
    private var byStem: [String: [Int]] = [:]
    private let list: [PageImage]
    let base: URL?

    init(_ list: [PageImage], base: URL?) {
        self.list = list
        self.base = base
        for (i, img) in list.enumerated() {
            for u in img.urls {
                byURL[Self.key(u, base: base)] = i
                let st = Self.stem(u)
                if !st.isEmpty, !(byStem[st]?.contains(i) ?? false) { byStem[st, default: []].append(i) }
            }
        }
    }

    var isEmpty: Bool { list.isEmpty }

    func data(for candidates: [String]) -> Data? {
        for c in candidates where !c.isEmpty { if let i = byURL[Self.key(c, base: base)] { return list[i].data } }
        // by name: several files of one figure (thumbnail, full size): the largest
        for c in candidates where !c.isEmpty {
            if let hits = byStem[Self.stem(c)], let best = hits.max(by: { list[$0].data.count < list[$1].data.count }) { return list[best].data }
        }
        return nil
    }

    static func key(_ u: String, base: URL?) -> String {
        var s = u.trimmingCharacters(in: .whitespaces)
        if s.hasPrefix("//") { s = "https:" + s }
        if let abs = URL(string: s, relativeTo: base)?.absoluteURL.absoluteString { s = abs }
        if let h = s.firstIndex(of: "#") { s = String(s[..<h]) }
        return s.replacingOccurrences(of: "http://", with: "https://")
    }

    /// File name without extension and size suffixes, lower-cased; "" for generic names.
    static func stem(_ u: String) -> String {
        var s = u.components(separatedBy: "?")[0].components(separatedBy: "#")[0]
        s = (s.removingPercentEncoding ?? s).components(separatedBy: "/").last ?? s
        if let dot = s.lastIndex(of: "."), s.distance(from: dot, to: s.endIndex) <= 5 { s = String(s[..<dot]) }
        s = s.lowercased()
        for suffix in ["_lrg", "_large", "-large", "_hires", "_highres", "-highres", "_full", "_med", "_sml", "_small", "_thumb", "-thumb"] where s.hasSuffix(suffix) {
            s = String(s.dropLast(suffix.count))
        }
        return ["default", "image", "img", "figure", "large", "full", "original", "download", "fig", ""].contains(s) ? "" : s
    }
}

extension Paper2Epub {
    /// Converts an article page the reader has open in the browser (sent by Lunta's browser
    /// extension with the HTML as the server delivered it and the figure images the browser
    /// fetched). Picks the publisher's reader by the page's host: ScienceDirect, NEJM (generic for
    /// now), otherwise the generic reader for standard scholarly markup, which refuses pages
    /// without a substantial body rather than make a poor reading view.
    public static func convert(page html: String, url: URL?, images: [PageImage], into dir: URL,
                               log: @escaping Log = { _ in }) async throws -> ConversionResult {
        let bag = PageImages(images, base: url)
        var book: Book
        switch PageReader.pick(url: url, html: html) {
        case .scienceDirect:
            book = try await ScienceDirect.book(html: html, dir: nil, images: bag, log: log)
        case .nejm:
            book = try await NEJM.book(html: html, url: url, images: bag, log: log)
        case .generic:
            book = try await GenericPage.book(html: html, url: url, images: bag, label: "Full text from \(url?.host ?? "the publisher")", log: log)
        }
        if book.meta.url.isEmpty, let url { book.meta.url = url.absoluteString }
        if book.meta.doi.isEmpty, let url, let d = Resolver.doi(in: url.absoluteString) { book.meta.doi = d }
        return try write(book, into: dir, log: log)
    }

    /// Which reader `convert(page:)` uses for a page ("ScienceDirect", "NEJM", "generic").
    public static func pageReader(url: URL?, html: String) -> String { PageReader.pick(url: url, html: html).rawValue }
}

enum PageReader: String {
    case scienceDirect = "ScienceDirect", nejm = "NEJM", generic

    static func pick(url: URL?, html: String) -> PageReader {
        let host = url?.host?.lowercased() ?? ""
        if host.hasSuffix("sciencedirect.com") || (host.isEmpty && ScienceDirect.looksLike(html)) { return .scienceDirect }
        if host.hasSuffix("nejm.org") { return .nejm }
        return .generic
    }
}

/// NEJM article pages. Placeholder: until a saved NEJM page is available to build a dedicated
/// reader from, NEJM pages go through the generic reader.
enum NEJM {
    static func book(html: String, url: URL?, images: PageImages, log: @escaping (String) -> Void) async throws -> Book {
        try await GenericPage.book(html: html, url: url, images: images, label: "NEJM full text", log: log)
    }
}

/// MathML written back out (namespace on <math>, attributes EPUB accepts).
enum MathML {
    static let ns = "http://www.w3.org/1998/Math/MathML"

    static func serialize(_ m: XNode, display: Bool) -> String {
        var s = "<math xmlns=\"\(ns)\""
        if display { s += " display=\"block\"" }
        for k in m.attrOrder where !["id", "display", "xmlns", "class", "style", "altimg", "alttext", "overflow"].contains(k) && !k.hasPrefix("data-") {
            s += " \(k)=\"\(esc(m.attrs[k] ?? ""))\""
        }
        s += ">"
        for c in m.children { s += node(c) }
        return s + "</math>"
    }

    private static func node(_ n: XNode) -> String {
        if n.isText { return esc(n.text) }
        if n.name == "annotation" || n.name == "annotation-xml" { return "" }
        var s = "<\(n.name)"
        for k in n.attrOrder where k != "id" && k != "class" && k != "style" && !k.hasPrefix("data-") {
            if k == "displaystyle", !["mstyle", "mtable", "math"].contains(n.name) { continue }
            s += " \(k)=\"\(esc(n.attrs[k] ?? ""))\""
        }
        if n.children.isEmpty { return s + "/>" }
        s += ">"
        for c in n.children { s += node(c) }
        return s + "</\(n.name)>"
    }
}

/// Article pages in general, read from standard scholarly markup: `citation_*` meta tags for the
/// details; the article's main element (article, main, [role=main], .article-body, #body, …) for
/// the text: headings, paragraphs, lists, figures (img + figcaption), tables, MathML and the
/// reference list (the section headed References). Navigation, asides, ads, share bars and scripts
/// are dropped. Conservative: a page without a substantial body (under 1,000 words, or no
/// headings) is refused.
///
/// Rust port (desktop / Android): this whole reader, `HTMLDOM` and `PageImages` would move to
/// core/src/convert (e.g. `convert/webpage.rs`), with `sciencedirect.rs::book` taking posted images.
enum GenericPage {
    static let minWords = 1000

    static func book(html: String, url: URL?, images: PageImages, label: String, log: @escaping (String) -> Void) async throws -> Book {
        log("Reading the page (\(label))")
        let pre = html
            .replacingOccurrences(of: #"<script\b[^>]*type="math/mml"[^>]*>([\s\S]*?)</script>"#, with: "$1", options: .regularExpression)
            .replacingOccurrences(of: #"<script\b[^>]*type="math/tex; *mode=display"[^>]*>([\s\S]*?)</script>"#, with: "<code class=\"tex-display\">$1</code>", options: .regularExpression)
            .replacingOccurrences(of: #"<script\b[^>]*type="math/tex"[^>]*>([\s\S]*?)</script>"#, with: "<code class=\"tex\">$1</code>", options: .regularExpression)
        let doc = HTMLDOM.parse(pre)
        var base = url
        if let b = doc.all(where: { $0.name == "base" }).first?.attr("href"), let u = URL(string: b, relativeTo: url) { base = u.absoluteURL }
        var meta = metadata(doc, url: url)
        let r = Renderer(images: images, base: base, title: meta.title)
        let body = doc.all(where: { $0.name == "body" }).first ?? doc
        // what links in the page point at (those ids are kept)
        for a in doc.all(where: { $0.name == "a" }) {
            if let h = a.attr("href"), let i = h.firstIndex(of: "#") { r.targets.insert(String(h[h.index(after: i)...])) }
        }
        clean(body)
        guard let main = mainElement(body) else { throw refusal(words: 0, headings: 0) }
        log("Main text: <\(main.name)\(main.attr("id").map { " id=\($0)" } ?? "")\(main.attr("class").map { " class=\($0.prefix(60))" } ?? "")>")

        // the abstract may sit outside the main text
        let mainHasAbstract = main.all(where: { isHeading($0) }).contains { Renderer.isAbstract($0.plain) }
            || main.all(where: { $0.nameTokens.contains("abstract") }).count > 0
        if !mainHasAbstract, let abs = body.all(where: { x in
            x.nameTokens.contains("abstract") && !["meta", "a", "span", "li"].contains(x.name) && !x.ancestors().contains { $0 === main }
                && !main.ancestors().contains { $0 === x } && words(x.plain) >= 30
        }).first {
            r.startChapter(title: "Abstract", id: "abstract")
            if let h = abs.all(where: isHeading).first { h.children = [] }   // its own heading: we have one
            r.flow(abs)
        } else if !mainHasAbstract, !meta.abstract.isEmpty {
            r.startChapter(title: "Abstract", id: "abstract")
            r.emit("<p>\(esc(meta.abstract))</p>")
        }
        r.topLevel = main.all(where: isHeading)
            .filter { !r.isTitle($0) && !$0.plain.isEmpty && !$0.ancestors().contains { $0.name == "figure" || $0.name == "table" } }
            .compactMap { Int($0.name.dropFirst()) }.min() ?? 2
        r.titleAhead = main.all(where: isHeading).contains { r.isTitle($0) }
        r.flow(main)
        var chapters = r.finish()

        let bodyChapters = chapters.filter { !$0.headings.contains { $0.id == "references" } }
        let wordCount = bodyChapters.reduce(0) { $0 + words(ReaderText.textOf(Substring($1.html))) }
        let headingCount = r.headingCount
        log("\(wordCount) words, \(headingCount) headings, \(r.images.count) figure image(s), \(r.missingImages) missing")
        if wordCount < minWords || headingCount == 0 { throw refusal(words: wordCount, headings: headingCount) }
        if meta.abstract.isEmpty, let abs = chapters.first(where: { $0.headings.first?.id == "abstract" }) {
            meta.abstract = ReaderText.textOf(Substring(abs.html)).split(whereSeparator: \.isWhitespace).joined(separator: " ")
                .replacingOccurrences(of: #"^Abstract\s*"#, with: "", options: .regularExpression)
        }
        for i in chapters.indices where chapters[i].html.contains("[[missing-image]]") {
            chapters[i].html = chapters[i].html.replacingOccurrences(of: "[[missing-image]]", with: "<p class=\"missing\"><em>[Figure image not available]</em></p>")
        }
        var notes: [String] = []
        if r.missingImages > 0 { notes.append("\(r.missingImages) figure image(s) were not sent by the browser; captions are kept.") }
        if r.mathCount > 0 { notes.append("\(r.mathCount) equation(s) as MathML.") }
        return Book(meta: meta, chapters: chapters, images: r.images, sourceLabel: label, notes: notes)
    }

    static func refusal(words: Int, headings: Int) -> ConversionError {
        fail("This page doesn't look like the article's full text (found \(words) words of article text"
             + (headings == 0 ? " and no section headings" : "") + "). If you have access, open the full-text view of the article and try again.")
    }

    static func words(_ s: String) -> Int { s.split(whereSeparator: { $0.isWhitespace }).count }

    static func isHeading(_ n: XNode) -> Bool {
        n.name.count == 2 && n.name.hasPrefix("h") && ("1"..."6").contains(String(n.name.last!))
    }

    // MARK: Metadata

    static func metadata(_ doc: XNode, url: URL?) -> PaperMeta {
        var tags: [String: [String]] = [:]
        for m in doc.all(where: { $0.name == "meta" }) {
            guard let n = (m.attr("name") ?? m.attr("property"))?.lowercased(), let c = m.attr("content")?.trimmingCharacters(in: .whitespacesAndNewlines), !c.isEmpty else { continue }
            tags[n, default: []].append(c)
        }
        func first(_ names: String...) -> String { for n in names { if let v = tags[n]?.first { return v } }; return "" }
        var m = PaperMeta()
        m.title = first("citation_title", "dc.title", "og:title")
        if m.title.isEmpty { m.title = doc.all(where: { $0.name == "title" }).first?.plain ?? "" }
        m.authors = (tags["citation_author"] ?? tags["dc.creator"] ?? []).map { a in
            let parts = a.components(separatedBy: ",").map { $0.trimmingCharacters(in: .whitespaces) }
            return parts.count == 2 && !parts[1].isEmpty ? "\(parts[1]) \(parts[0])" : a
        }
        m.journal = first("citation_journal_title", "prism.publicationname", "dc.source")
        m.date = first("citation_publication_date", "citation_date", "citation_online_date", "dc.date", "prism.publicationdate").replacingOccurrences(of: "/", with: "-")
        for d in [first("citation_doi"), first("prism.doi"), first("dc.identifier"), first("bepress_citation_doi")] where m.doi.isEmpty {
            if let x = Resolver.doi(in: d) { m.doi = x }
        }
        if m.doi.isEmpty, let u = url?.absoluteString, let x = Resolver.doi(in: u) { m.doi = x }
        m.abstract = first("citation_abstract", "dc.description").replacingOccurrences(of: "<[^>]+>", with: "", options: .regularExpression)
        m.license = first("dc.rights", "citation_license")
        m.keywords = (tags["citation_keywords"] ?? []).flatMap { $0.components(separatedBy: ";") }.map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }
        m.url = url?.absoluteString ?? ""
        return m
    }

    // MARK: Cleaning and finding the main text

    static let dropNames: Set<String> = ["script", "style", "noscript", "template", "iframe", "nav", "aside", "footer", "form", "button",
                                         "input", "select", "textarea", "svg", "canvas", "video", "audio", "object", "embed", "link",
                                         "meta", "title", "head", "dialog", "label", "mjx-assistive-mml"]
    static let junkTokens: Set<String> = ["ad", "ads", "advert", "advertisement", "adslot", "banner", "promo", "share", "sharing", "social",
                                          "toolbar", "toolbox", "cookie", "cookies", "consent", "related", "recommended", "recommendations",
                                          "newsletter", "sidebar", "breadcrumb", "breadcrumbs", "skip", "metrics", "altmetric", "signup",
                                          "login", "paywall", "modal", "popup", "popover", "tooltip", "hidden", "hide", "sr", "visually",
                                          "navigation", "nav", "menu", "toc", "pagination", "comments", "download", "downloads", "sticky",
                                          "feedback", "survey", "alert", "rightslink", "reprints", "permissions", "jumplinks", "offscreen",
                                          "print", "tools", "widget"]
    static let protected: Set<String> = ["html", "body", "main", "article", "figure", "table", "h1", "h2", "h3", "h4", "h5", "h6", "p",
                                         "math", "tr", "td", "th", "thead", "tbody", "caption", "figcaption"]

    /// Removes navigation, asides, ads, share bars, hidden elements and scripts.
    static let debug = ProcessInfo.processInfo.environment["P2E_DEBUG"] != nil
    static func clean(_ n: XNode) {
        if debug {
            for c in n.children where !c.isText && !c.isMath && dropReason(c) != nil && words(c.plain) >= 80 {
                FileHandle.standardError.write(Data("  dropped <\(c.name) id=\(c.attr("id") ?? "") class=\(c.attr("class") ?? "")> \(words(c.plain)) words: \(dropReason(c)!)\n".utf8))
            }
        }
        n.children.removeAll { c in dropReason(c) != nil }
        for c in n.children where !c.isText && !c.isMath { clean(c) }
    }

    static func dropReason(_ c: XNode) -> String? {
        let r: Bool = { (c: XNode) -> Bool in
            guard !c.isText, !c.isMath else { return false }
            // a button or label around a figure (zoomable images): kept, it's only a wrapper
            if (c.name == "button" || c.name == "label"), c.firstDescendant("img") != nil || c.firstDescendant("figure") != nil || c.firstDescendant("math") != nil { return false }
            if dropNames.contains(c.name) { return true }
            // lists of links only (in-page contents, "View article / Google Scholar" under a reference)
            if c.name == "ul" || c.name == "ol" {
                let all = words(c.plain), linked = c.all(where: { $0.name == "a" }).reduce(0) { $0 + words($1.plain) }
                if all > 0, linked * 10 >= all * 9, c.parent?.name != "li" || all < 12 { return true }
            }
            if c.name.hasPrefix("mjx-") && c.name != "mjx-container" { return false }
            if c.attrs["hidden"] != nil { return true }
            if c.attr("aria-hidden") == "true", c.name != "mjx-container", c.firstDescendant("math") == nil, c.firstDescendant("img") == nil { return true }
            if let st = c.attr("style")?.replacingOccurrences(of: " ", with: "").lowercased(), st.contains("display:none") || st.contains("visibility:hidden") { return true }
            if c.attr("role") == "navigation" || c.attr("role") == "complementary" || c.attr("role") == "banner" { return true }
            if c.classes.contains(where: { $0.hasPrefix("MathJax") || $0.hasPrefix("MJX") }) { return true }
            if !protected.contains(c.name), c.attr("role") != "main", !c.nameTokens.isDisjoint(with: junkTokens),
               c.firstDescendant("math") == nil, !c.nameTokens.contains("abstract"), !c.nameTokens.contains("body"),
               // class names are only a hint: anything holding real paragraphs or much unlinked text stays
               c.all(where: { $0.name == "p" }).reduce(0, { $0 + words($1.plain) }) < 120, {
                   let all = words(c.plain), linked = c.all(where: { $0.name == "a" }).reduce(0) { $0 + words($1.plain) }
                   return all < 80 || linked * 2 > all
               }() {
                return true
            }
            return false
        }(c)
        if !r { return nil }
        if dropNames.contains(c.name) { return "element" }
        return "tokens \(c.nameTokens.intersection(junkTokens).sorted()) / hidden / role"
    }

    static let mainSelectors: [(XNode) -> Bool] = [
        { $0.name == "article" }, { $0.name == "main" }, { $0.attr("role") == "main" }, { $0.attr("itemprop") == "articleBody" },
        { $0.attr("id") == "body" || $0.attr("id") == "article-body" || $0.attr("id") == "artText" || $0.attr("id") == "main-content" },
        { n in let c = n.classes; return c.contains { ["article-body", "article__body", "c-article-body", "articleBody", "article-text",
                                                       "fulltext-view", "hlFld-Fulltext", "JournalFullText", "html-body", "article-full-text",
                                                       "main-content", "content-body", "article__fulltext", "fulltext"].contains($0) } },
    ]

    /// The element with the article's text: of the usual containers, the one holding most of the
    /// page's paragraph text (at least half of it); otherwise the smallest element holding 80 %.
    static func mainElement(_ body: XNode) -> XNode? {
        var pw: [ObjectIdentifier: Int] = [:]
        @discardableResult func count(_ n: XNode) -> Int {
            var w = 0
            if n.name == "p" { w = words(n.plain) } else { for c in n.children where !c.isText { w += count(c) } }
            if n.name == "p" { for c in n.children where !c.isText { _ = count(c) } }
            pw[ObjectIdentifier(n)] = w
            return w
        }
        let total = count(body)
        guard total > 0 else { return nil }
        let cands = body.all { n in mainSelectors.contains { $0(n) } }.filter { (pw[ObjectIdentifier($0)] ?? 0) * 2 >= total }
        // the innermost of them that still holds nearly all of the text the best one holds
        if let most = cands.map({ pw[ObjectIdentifier($0)] ?? 0 }).max(),
           let best = cands.filter({ (pw[ObjectIdentifier($0)] ?? 0) * 100 >= most * 85 }).max(by: { $0.ancestors().count < $1.ancestors().count }) {
            return best
        }
        var cur = body
        while let next = cur.elements.max(by: { (pw[ObjectIdentifier($0)] ?? 0) < (pw[ObjectIdentifier($1)] ?? 0) }),
              (pw[ObjectIdentifier(next)] ?? 0) * 10 >= total * 8 { cur = next }
        return cur
    }

    // MARK: Rendering

    final class Renderer {
        struct Ch { var title: String; var html: String; var headings: [Heading]; var refs: [String]?; var pre: Bool }
        var chapters: [Ch] = [Ch(title: "Summary", html: "", headings: [], refs: nil, pre: true)]
        var images: [BookImage] = []
        var imageNames: [Data: String] = [:]
        var missingImages = 0
        var mathCount = 0
        var headingCount = 0
        var topLevel = 2
        var targets = Set<String>()
        private var usedIDs = Set<String>()
        let bag: PageImages
        let base: URL?
        let title: String
        let mjx = ScienceDirect.Converter()

        init(images: PageImages, base: URL?, title: String) { bag = images; self.base = base; self.title = title }

        static let blockNames: Set<String> = ["address", "article", "aside", "blockquote", "details", "div", "dl", "dt", "dd", "fieldset",
                                              "figcaption", "figure", "footer", "form", "h1", "h2", "h3", "h4", "h5", "h6", "header", "hr",
                                              "li", "main", "nav", "ol", "p", "pre", "section", "table", "ul", "summary", "center", "caption",
                                              "tbody", "thead", "tfoot", "tr", "td", "th", "menu", "#document", "body", "html"]

        static func isAbstract(_ t: String) -> Bool {
            let s = t.lowercased().trimmingCharacters(in: .whitespacesAndNewlines.union(.punctuationCharacters))
            return ["abstract", "summary", "structured abstract"].contains(s)
        }
        static func isReferences(_ t: String) -> Bool {
            let s = t.lowercased().trimmingCharacters(in: .whitespacesAndNewlines.union(.punctuationCharacters))
                .replacingOccurrences(of: #"^[0-9.\s]+"#, with: "", options: .regularExpression)
            return ["references", "reference", "references and notes", "bibliography", "literature cited", "works cited",
                    "cited literature", "reference list", "references cited", "notes and references"].contains(s)
        }

        func isTitle(_ h: XNode) -> Bool {
            func norm(_ s: String) -> String { s.lowercased().filter { $0.isLetter || $0.isNumber } }
            return h.name == "h1" && (title.isEmpty || norm(h.plain) == norm(title) || norm(h.plain).count > 30 && norm(title).hasPrefix(norm(h.plain).prefix(30)))
        }

        private var blockMemo: [ObjectIdentifier: Bool] = [:]
        func isBlock(_ n: XNode) -> Bool {
            if n.isText || n.isMath { return false }
            if Self.blockNames.contains(n.name) { return true }
            if n.name == "mjx-container" || n.name == "img" || n.name == "br" { return false }
            if let b = blockMemo[ObjectIdentifier(n)] { return b }
            let b = n.elements.contains(where: isBlock)
            blockMemo[ObjectIdentifier(n)] = b
            return b
        }

        private var pendingMarker = ""
        func emit(_ s: String) {
            guard !s.isEmpty else { return }
            if chapters[chapters.count - 1].refs != nil, !s.hasPrefix("<h") {
                // the reference list holds list items only; anchors go into the next item
                if s.hasPrefix("<span id=") { pendingMarker += s; return }
                let item = s.hasPrefix("<li") ? s : "<li>\(s)</li>"
                if !pendingMarker.isEmpty, let gt = item.firstIndex(of: ">") {
                    chapters[chapters.count - 1].refs!.append(String(item[...gt]) + pendingMarker + String(item[item.index(after: gt)...]))
                    pendingMarker = ""
                } else {
                    chapters[chapters.count - 1].refs!.append(item)
                }
                return
            }
            chapters[chapters.count - 1].html += s
        }

        func uniqueID(_ raw: String?) -> String {
            var id = (raw ?? "").replacingOccurrences(of: #"[^A-Za-z0-9_.-]"#, with: "-", options: .regularExpression)
            if id.isEmpty { id = "sec-\(usedIDs.count + 1)" }
            if !(id.first!.isLetter || id.first == "_") { id = "x" + id }
            var out = id, k = 2
            while usedIDs.contains(out) { out = "\(id)-\(k)"; k += 1 }
            usedIDs.insert(out)
            return out
        }

        /// The id an element keeps (if links in the page point at it).
        func keptID(_ n: XNode) -> String? {
            guard let id = n.attr("id"), targets.contains(id), !usedIDs.contains(id) else { return nil }
            return uniqueID(id) == id ? id : nil
        }

        func startChapter(title: String, id: String) {
            let refs = Self.isReferences(title)
            chapters.append(Ch(title: title, html: "", headings: [], refs: refs ? [] : nil, pre: false))
            let hid = refs ? uniqueID("references") : uniqueID(id)
            chapters[chapters.count - 1].html = "<section><h1 id=\"\(esc(hid))\">\(esc(title))</h1>"
            chapters[chapters.count - 1].headings = [Heading(level: 1, id: hid, title: title)]
        }

        func finish() -> [Chapter] {
            var out: [Chapter] = []
            for c in chapters {
                var html = c.html
                if let refs = c.refs {
                    html += "<ol class=\"refs\">" + refs.joined() + "</ol>"
                }
                if c.pre {
                    // before the abstract: the masthead (authors, affiliations, notes)
                    if chapters.contains(where: { !$0.pre && Self.isAbstract($0.title) }) { continue }
                    // text before the first heading: kept only if it reads like a summary or an
                    // introduction, not bylines; "Label: value" lines (citation, copyright, funding) left out
                    html = html.replacingOccurrences(of: #"<p>\s*<(strong|b)>[^<]{1,40}:\s*</(strong|b)>[\s\S]*?</p>"#, with: "", options: .regularExpression)
                    let t = ReaderText.textOf(Substring(html))
                    guard GenericPage.words(t) >= 60, html.contains("<p"),
                          html.components(separatedBy: "</p>").contains(where: { GenericPage.words(ReaderText.textOf(Substring($0))) >= 40 }) else { continue }
                    let name = chapters.contains { !$0.pre && Self.isAbstract($0.title) } ? "Introduction" : "Summary"
                    let id = uniqueID(name.lowercased())
                    out.append(Chapter(title: name, html: "<section><h1 id=\"\(id)\">\(name)</h1>\(html)</section>", headings: [Heading(level: 1, id: id, title: name)]))
                    continue
                }
                out.append(Chapter(title: c.title, html: html + "</section>", headings: c.headings))
            }
            return out
        }

        // block level: a container's children, inline runs as paragraphs
        func flow(_ n: XNode, paragraphID: String? = nil) {
            var run = ""
            var pid = paragraphID
            func flush() {
                if !run.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                    emit("<p\(pid.map { " id=\"\(esc($0))\"" } ?? "")>\(run.trimmingCharacters(in: .whitespacesAndNewlines))</p>")
                    pid = nil
                }
                run = ""
            }
            for c in n.children {
                if isBlock(c) { flush(); block(c) } else { run += inline(c) }
            }
            flush()
        }

        func block(_ n: XNode) {
            if GenericPage.isHeading(n) { heading(n); return }
            if inRefs, refItem(n) { return }
            switch n.name {
            case "p":
                flow(n, paragraphID: keptID(n))
            case "figure":
                // a table set as a figure (LaTeXML, some journals)
                if n.firstDescendant("img") == nil, n.firstDescendant("table") != nil {
                    emit(tableWrap(n, tables: n.all(where: { $0.name == "table" && !$0.ancestors().contains { $0.name == "table" } })))
                } else {
                    emit(figure(n))
                }
            case "table":
                emit(tableWrap(n, tables: [n]))
            case "ul", "ol":
                emit(list(n))
            case "pre":
                emit("<pre>\(esc(n.allText))</pre>")
            case "hr":
                return
            case "dt":
                let t = inline(n).trimmingCharacters(in: .whitespacesAndNewlines)
                if !t.isEmpty { emit("<p><strong>\(t)</strong></p>") }
            default:
                let tok = n.nameTokens
                if n.name == "div" || n.name == "section", (tok.contains("figure") || tok.contains("fig")), n.firstDescendant("img") != nil,
                   n.firstDescendant("table") == nil, GenericPage.words(n.plain) < 400 {
                    emit(figure(n)); return
                }
                if tok.contains("table") || tok.contains("tbl"), n.firstDescendant("table") != nil,
                   !n.all(where: GenericPage.isHeading).contains(where: { !$0.plain.isEmpty }) {
                    let tables = n.all(where: { $0.name == "table" && !$0.ancestors().contains { $0.name == "table" } })
                    if tables.count <= 3 { emit(tableWrap(n, tables: tables)); return }
                }
                if let id = keptID(n) { emit("<span id=\"\(esc(id))\"></span>") }
                flow(n)
            }
        }

        var inRefs: Bool { chapters[chapters.count - 1].refs != nil }

        /// In the reference list: list items and leaf paragraphs are references.
        func refItem(_ n: XNode) -> Bool {
            if n.name == "ul" || n.name == "ol" {
                for li in n.children where li.name == "li" {
                    let t = refInline(li).trimmingCharacters(in: .whitespacesAndNewlines)
                    if !t.isEmpty { emit("<li\(keptID(li).map { " id=\"\(esc($0))\"" } ?? "")>\(t)</li>") }
                }
                return true
            }
            // one reference: a paragraph-like element without lists, short enough not to be a list of them
            let w = GenericPage.words(n.plain)
            let hasList = n.firstDescendant("ol") != nil || n.firstDescendant("ul") != nil || n.firstDescendant("li") != nil
            let leaf = !n.elements.contains { ["p", "div", "section", "table", "figure"].contains($0.name) && GenericPage.words($0.plain) >= 3 }
            let refLike = !n.nameTokens.isDisjoint(with: ["ref", "reference", "citation", "bibitem", "bib", "csl"])
            if ["p", "div", "li", "span"].contains(n.name), !hasList, w >= 3, leaf || (refLike && w <= 150) {
                let t = refInline(n).trimmingCharacters(in: .whitespacesAndNewlines)
                if !t.isEmpty { emit("<li\(keptID(n).map { " id=\"\(esc($0))\"" } ?? "")>\(t)</li>") }
                return true
            }
            return false
        }

        var titleAhead = false   // the article's title heading is still to come (masthead headings before it are skipped)
        /// A reference's text without its rows of service links ("Article · PubMed · Google Scholar").
        func refInline(_ n: XNode) -> String {
            n.children.map { c -> String in
                if !c.isText, c.name != "a", isBlock(c) || c.name == "span" {
                    let all = GenericPage.words(c.plain), linked = c.all(where: { $0.name == "a" }).reduce(0) { $0 + GenericPage.words($1.plain) }
                    if all > 0, all < 15, linked * 10 >= all * 9 { return "" }
                }
                return inline(c)
            }.joined()
        }

        func heading(_ h: XNode) {
            let t = h.plain.trimmingCharacters(in: .whitespacesAndNewlines)
            if isTitle(h) { titleAhead = false; return }
            guard !t.isEmpty, !titleAhead else { return }
            let level = (Int(h.name.dropFirst()) ?? 2) - topLevel
            let sectionID = h.parent.flatMap { p in (p.name == "section" || p.name == "div") && p.elements.first === h ? p.attr("id") : nil }
            let raw = h.attr("id") ?? sectionID
            if level <= 0 || (Self.isAbstract(t) && !inRefs) {
                headingCount += 1
                startChapter(title: t, id: raw ?? "sec-\(chapters.count)")
                if let s = sectionID, s != raw, targets.contains(s), !usedIDs.contains(s) { usedIDs.insert(s); emit("<span id=\"\(esc(s))\"></span>") }
                return
            }
            if inRefs { return }
            headingCount += 1
            let id = uniqueID(raw)
            let tag = level == 1 ? "h2" : "h3"
            if level == 1 { chapters[chapters.count - 1].headings.append(Heading(level: 2, id: id, title: t)) }
            emit("<\(tag) id=\"\(esc(id))\">\(inline(h).trimmingCharacters(in: .whitespacesAndNewlines))</\(tag)>")
        }

        // MARK: inline

        func inline(_ n: XNode) -> String {
            if n.isText { return esc(n.text) }
            // an element links point at (e.g. an empty <a id> before a reference): keep the anchor
            if !n.isMath, n.name != "img", let raw = n.attr("id") ?? (n.name == "a" ? n.attr("name") : nil), targets.contains(raw), !usedIDs.contains(raw), uniqueID(raw) == raw {
                return "<span id=\"\(esc(raw))\"></span>" + inlineContent(n)
            }
            return inlineContent(n)
        }

        func inlineContent(_ n: XNode) -> String {
            if n.isText { return esc(n.text) }
            if n.isMath && n.name == "math" { mathCount += 1; return MathML.serialize(n, display: n.attr("display") == "block") }
            switch n.name {
            case "mjx-container":
                if let m = n.firstDescendant("math") { mathCount += 1; return MathML.serialize(m, display: n.attr("display") == "true") }
                let s = mjx.math(n); mathCount = mathCount + 1; return s
            case "br": return "<br/>"
            case "img": return inlineImage(n)
            case "em", "i", "strong", "b", "sup", "sub", "u", "small", "code", "s":
                let inner = n.children.map(inline).joined()
                if inner.isEmpty { return "" }
                if n.attr("class")?.contains("tex") == true { return "<code>\(inner)</code>" }
                return "<\(n.name)>\(inner)</\(n.name)>"
            case "a":
                let inner = n.children.map(inline).joined()
                guard !inner.trimmingCharacters(in: .whitespaces).isEmpty, let href = n.attr("href") else { return inner }
                if href.hasPrefix("#"), href.count > 1 { return "<a href=\"\(esc(href))\">\(inner)</a>" }
                if let u = URL(string: href, relativeTo: base)?.absoluteURL, ["http", "https"].contains(u.scheme ?? "") {
                    // a link to a place in this same page
                    if let base, let f = u.fragment, u.absoluteString.components(separatedBy: "#")[0] == base.absoluteString.components(separatedBy: "#")[0] {
                        return "<a href=\"#\(esc(f))\">\(inner)</a>"
                    }
                    return "<a href=\"\(esc(u.absoluteString))\">\(inner)</a>"
                }
                return inner
            default:
                // headings and blocks inside inline content: their text
                if GenericPage.isHeading(n) { return "<strong>\(n.children.map(inline).joined())</strong> " }
                let inner = n.children.map(inline).joined()
                return Self.blockNames.contains(n.name) && !inner.isEmpty ? inner + " " : inner
            }
        }

        func candidates(_ img: XNode) -> [String] {
            var c: [String] = []
            for k in ["src", "data-src", "data-lazy-src", "data-original", "data-hi-res-src", "data-large-src"] {
                if let v = img.attr(k), !v.isEmpty, !v.hasPrefix("data:") { c.append(v) }
            }
            for k in ["srcset", "data-srcset"] {
                if let v = img.attr(k) { c += v.components(separatedBy: ",").compactMap { $0.trimmingCharacters(in: .whitespaces).components(separatedBy: " ").first }.filter { !$0.isEmpty }.reversed() }
            }
            if let pic = img.parent, pic.name == "picture" {
                for src in pic.children where src.name == "source" {
                    if let v = src.attr("srcset") { c += v.components(separatedBy: ",").compactMap { $0.trimmingCharacters(in: .whitespaces).components(separatedBy: " ").first }.filter { !$0.isEmpty } }
                }
            }
            if let a = img.ancestors().first(where: { $0.name == "a" }), let h = a.attr("href") { c.append(h) }
            return c
        }

        /// The image's file in the book, from the posted images; nil if the browser didn't send it.
        func imageFile(_ cands: [String]) -> String? {
            guard let data = bag.data(for: cands) else { return nil }
            if let name = imageNames[data] { return name }
            let n = images.count + 1
            if let x = Images.normalise(data) {
                let name = "img\(n).\(x.ext)"
                images.append(BookImage(fileName: name, data: x.data, mediaType: x.mediaType))
                imageNames[data] = name
                return name
            }
            if data.starts(with: Data("<svg".utf8)) || data.starts(with: Data("<?xml".utf8)) {
                let name = "img\(n).svg"
                images.append(BookImage(fileName: name, data: data, mediaType: "image/svg+xml"))
                imageNames[data] = name
                return name
            }
            return nil
        }

        func inlineImage(_ img: XNode) -> String {
            guard let f = imageFile(candidates(img)) else { return "" }
            return "<img src=\"../images/\(f)\" alt=\"\(esc(img.attr("alt") ?? ""))\"/>"
        }

        // MARK: figures, tables, lists

        func figure(_ n: XNode) -> String {
            let id = keptID(n) ?? uniqueID(n.attr("id") ?? "fig-\(images.count + missingImages + 1)")
            let isCaption: (XNode) -> Bool = { x in
                x.name == "figcaption" || x.name == "caption" || !x.nameTokens.isDisjoint(with: ["caption", "figcaption", "legend", "captions"])
            }
            let caps = n.all(where: isCaption).filter { c in !c.ancestors().contains(where: isCaption) }
            var imgs = n.all(where: { $0.name == "img" }).filter { img in !img.ancestors().contains(where: isCaption) }
            // hi-res links inside the figure (e.g. "_lrg" or "download full size")
            let links = n.all(where: { $0.name == "a" }).compactMap { $0.attr("href") }
                .filter { $0.range(of: #"(_lrg|large|full|original|hi-?res)|\.(jpe?g|png|gif|webp|tiff?)(\?|$)"#, options: [.regularExpression, .caseInsensitive]) != nil }
            if imgs.count > 1 { imgs = imgs.filter { !($0.attr("width").flatMap(Int.init).map { $0 < 40 } ?? false) } }
            var body = ""
            for (k, img) in imgs.enumerated() {
                var c = candidates(img)
                if k == 0 { c = links + c }
                if let f = imageFile(c) {
                    body += "<img src=\"../images/\(f)\" alt=\"\(esc(img.attr("alt") ?? "Figure"))\"/>"
                } else {
                    missingImages += 1
                    body += "[[missing-image]]"
                }
            }
            var caption = caps.map { c in
                c.elements.contains(where: isBlock)
                    ? c.children.map { x in isBlock(x) ? "<span class=\"cap-p\">\(inline(x).trimmingCharacters(in: .whitespacesAndNewlines))</span> " : inline(x) }.joined()
                    : inline(c)
            }.joined(separator: " ").trimmingCharacters(in: .whitespacesAndNewlines)
            if caption.isEmpty {
                // no marked caption: the figure's own text (label, title, legend paragraphs)
                caption = n.children.filter { !$0.isText || !$0.text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }
                    .filter { $0.name != "img" && $0.firstDescendant("img") == nil }
                    .map { inline($0).trimmingCharacters(in: .whitespacesAndNewlines) }.filter { !$0.isEmpty }.joined(separator: " ")
            }
            if body.isEmpty && caption.isEmpty { return "" }
            return "<figure id=\"\(esc(id))\">\(body)" + (caption.isEmpty ? "" : "<figcaption>\(caption)</figcaption>") + "</figure>"
        }

        func tableWrap(_ wrap: XNode, tables: [XNode]) -> String {
            var out = ""
            let isCap: (XNode) -> Bool = { x in
                x.name == "caption" || x.name == "figcaption" || !x.nameTokens.isDisjoint(with: ["caption", "label", "title", "captions"])
            }
            var caps = tables.flatMap { $0.children.filter { $0.name == "caption" } }
            if wrap.name != "table" {
                caps = wrap.all(where: isCap).filter { c in !c.ancestors().contains(where: isCap) && !c.ancestors().contains { $0.name == "table" } } + caps
            }
            let capText = caps.map { inline($0).trimmingCharacters(in: .whitespacesAndNewlines) }.filter { !$0.isEmpty }.joined(separator: " ")
            if !capText.isEmpty { out += "<p class=\"table-caption\"><strong>\(capText)</strong></p>" }
            for t in tables { out += table(t) }
            if wrap.name != "table" {
                if let f = wrap.all(where: { !$0.nameTokens.isDisjoint(with: ["foot", "footnote", "footnotes", "fn", "notes"]) && !$0.ancestors().contains { $0.name == "table" } }).first {
                    let t = inline(f).trimmingCharacters(in: .whitespacesAndNewlines)
                    if !t.isEmpty { out += "<p class=\"table-foot\"><small>\(t)</small></p>" }
                }
            }
            let id = keptID(wrap) ?? tables.first.flatMap(keptID)
            return "<div class=\"table-wrap\"\(id.map { " id=\"\(esc($0))\"" } ?? "")>\(out)</div>"
        }

        func table(_ t: XNode) -> String {
            func part(_ n: XNode) -> String {
                switch n.name {
                case "thead", "tbody", "tfoot", "tr":
                    return "<\(n.name)>" + n.children.map(part).joined() + "</\(n.name)>"
                case "th", "td":
                    var attrs = ""
                    for a in ["colspan", "rowspan"] { if let v = n.attr(a), Int(v) != nil { attrs += " \(a)=\"\(v)\"" } }
                    return "<\(n.name)\(attrs)>" + n.children.map(inline).joined().trimmingCharacters(in: .whitespacesAndNewlines) + "</\(n.name)>"
                case "caption", "colgroup", "col":
                    return ""
                default:
                    return n.isText ? "" : n.children.map(part).joined()
                }
            }
            var rows = t.children.map(part).joined()
            // rows directly in <table>: wrap them so the XHTML is valid
            if !rows.hasPrefix("<thead") && !rows.hasPrefix("<tbody") && !rows.hasPrefix("<tfoot") { rows = "<tbody>\(rows)</tbody>" }
            return "<table>\(rows)</table>"
        }

        func list(_ n: XNode) -> String {
            var items = ""
            for li in n.children where li.name == "li" {
                var inner = ""
                var run = ""
                for c in li.children {
                    if c.name == "ul" || c.name == "ol" { inner += run; run = ""; inner += list(c) }
                    else if c.name == "figure" { inner += run; run = ""; inner += figure(c) }
                    else if c.name == "table" { inner += run; run = ""; inner += tableWrap(c, tables: [c]) }
                    else { run += inline(c) }
                }
                inner += run
                inner = inner.trimmingCharacters(in: .whitespacesAndNewlines)
                if !inner.isEmpty { items += "<li\(keptID(li).map { " id=\"\(esc($0))\"" } ?? "")>\(inner)</li>" }
            }
            guard !items.isEmpty else { return "" }
            let tag = n.name == "ol" ? "ol" : "ul"
            return "<\(tag)>\(items)</\(tag)>"
        }
    }
}
