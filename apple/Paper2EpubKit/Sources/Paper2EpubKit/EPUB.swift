import Foundation

/// Packages a `Book` as EPUB 3 (with an EPUB 2 NCX for older readers).
enum EPUBWriter {
    static func write(_ book: Book, cover: Data?, to url: URL) throws {
        var files: [(name: String, title: String, html: String)] = []
        files.append(("title.xhtml", "Title page", titlePage(book.meta)))
        for (i, ch) in book.chapters.enumerated() {
            files.append((String(format: "ch%02d.xhtml", i + 1), ch.title, ch.html))
        }
        files = fixLinks(files)

        var zip = ZipWriter()
        zip.add("mimetype", Data("application/epub+zip".utf8), compress: false)
        zip.add("META-INF/container.xml", Data(containerXML.utf8))
        zip.add("EPUB/css/style.css", Data(Style.css.utf8))
        if let cover {
            zip.add("EPUB/images/p2e-cover.jpg", cover, compress: false)
            zip.add("EPUB/text/cover.xhtml", Data(xhtml(title: "Cover", body: coverBody, bodyClass: "cover").utf8))
        }
        for f in files {
            zip.add("EPUB/text/\(f.name)", Data(xhtml(title: f.title, body: f.html).utf8))
        }
        for img in book.images {
            zip.add("EPUB/images/\(img.fileName)", img.data, compress: false)
        }
        let chapterFiles = Array(files.dropFirst())
        zip.add("EPUB/nav.xhtml", Data(nav(book, chapterFiles).utf8))
        zip.add("EPUB/toc.ncx", Data(ncx(book, files).utf8))
        zip.add("EPUB/content.opf", Data(opf(book, files: files, hasCover: cover != nil).utf8))
        try zip.finish().write(to: url, options: .atomic)
    }

    // MARK: - Documents

    static let containerXML = """
    <?xml version="1.0" encoding="UTF-8"?>
    <container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
      <rootfiles><rootfile full-path="EPUB/content.opf" media-type="application/oebps-package+xml"/></rootfiles>
    </container>
    """

    static let coverBody = "<div class=\"cover-image\"><img src=\"../images/p2e-cover.jpg\" alt=\"Cover\"/></div>"

    static func xhtml(title: String, body: String, bodyClass: String? = nil, cssPath: String = "../css/style.css") -> String {
        let cls = bodyClass.map { " class=\"\($0)\"" } ?? ""
        return """
        <?xml version="1.0" encoding="UTF-8"?>
        <!DOCTYPE html>
        <html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops" xml:lang="en" lang="en">
        <head><meta charset="UTF-8"/><title>\(esc(title))</title><link rel="stylesheet" type="text/css" href="\(cssPath)"/></head>
        <body\(cls)>\(body)</body>
        </html>
        """
    }

    static func titlePage(_ m: PaperMeta) -> String {
        var s = "<section class=\"titlepage\" epub:type=\"titlepage\">"
        s += "<h1 class=\"title\">\(esc(m.title.isEmpty ? "Untitled paper" : m.title))</h1>"
        if !m.authors.isEmpty { s += "<p class=\"authors\">\(esc(m.authors.joined(separator: ", ")))</p>" }
        var pub: [String] = []
        if !m.journal.isEmpty { pub.append("<em>\(esc(m.journal))</em>") }
        if !m.date.isEmpty { pub.append(esc(m.date)) }
        if !pub.isEmpty { s += "<p class=\"journal\">\(pub.joined(separator: " · "))</p>" }
        if !m.doi.isEmpty { s += "<p class=\"doi\"><a href=\"https://doi.org/\(esc(m.doi))\">doi:\(esc(m.doi))</a></p>" }
        if !m.affiliationsHTML.isEmpty { s += "<div class=\"frontmatter\"><p class=\"affiliations\">\(m.affiliationsHTML)</p></div>" }
        if !m.license.isEmpty {
            let lic = m.license.hasPrefix("http") ? "<a href=\"\(esc(m.license))\">\(esc(m.license))</a>" : esc(m.license)
            s += "<p class=\"license\">License: \(lic)</p>"
        }
        return s + "</section>"
    }

    /// Rewrites `#id` links that point into another file, and unwraps links whose target doesn't exist.
    static func fixLinks(_ files: [(name: String, title: String, html: String)]) -> [(name: String, title: String, html: String)] {
        let idRE = try! NSRegularExpression(pattern: #"\sid="([^"]+)""#)
        var owner: [String: String] = [:]
        for f in files {
            let ns = f.html as NSString
            for m in idRE.matches(in: f.html, range: NSRange(location: 0, length: ns.length)) {
                let id = ns.substring(with: m.range(at: 1))
                if owner[id] == nil { owner[id] = f.name }
            }
        }
        let linkRE = try! NSRegularExpression(pattern: ##"<a href="#([^"]+)">([\s\S]*?)</a>"##)
        return files.map { f in
            let ns = f.html as NSString
            var out = ""
            var last = 0
            for m in linkRE.matches(in: f.html, range: NSRange(location: 0, length: ns.length)) {
                out += ns.substring(with: NSRange(location: last, length: m.range.location - last))
                let id = ns.substring(with: m.range(at: 1))
                let inner = ns.substring(with: m.range(at: 2))
                if let file = owner[id] {
                    let href = file == f.name ? "#\(id)" : "\(file)#\(id)"
                    out += "<a href=\"\(href)\">\(inner)</a>"
                } else {
                    out += inner
                }
                last = m.range.location + m.range.length
            }
            out += ns.substring(from: last)
            return (f.name, f.title, out)
        }
    }

    static func nav(_ book: Book, _ files: [(name: String, title: String, html: String)]) -> String {
        var s = "<nav epub:type=\"toc\" id=\"toc\"><h1>Contents</h1><ol>"
        for (ch, f) in zip(book.chapters, files) {
            let hs = ch.headings.isEmpty ? [Heading(level: 1, id: "", title: ch.title)] : ch.headings
            var openSub = false
            var openTop = false
            for h in hs {
                let href = h.id.isEmpty ? "text/\(f.name)" : "text/\(f.name)#\(h.id)"
                let title = esc(h.title.isEmpty ? ch.title : h.title)
                if h.level <= 1 || !openTop {
                    if openSub { s += "</ol>"; openSub = false }
                    if openTop { s += "</li>" }
                    s += "<li><a href=\"\(href)\">\(title)</a>"
                    openTop = true
                } else {
                    if !openSub { s += "<ol>"; openSub = true }
                    s += "<li><a href=\"\(href)\">\(title)</a></li>"
                }
            }
            if openSub { s += "</ol>" }
            if openTop { s += "</li>" }
        }
        s += "</ol></nav>"
        s += "<nav epub:type=\"landmarks\" id=\"landmarks\" hidden=\"hidden\"><ol>"
        s += "<li><a epub:type=\"toc\" href=\"#toc\">Contents</a></li>"
        if let first = files.first { s += "<li><a epub:type=\"bodymatter\" href=\"text/\(first.name)\">Start</a></li>" }
        s += "</ol></nav>"
        return xhtml(title: "Contents", body: s, cssPath: "css/style.css")
    }

    static func ncx(_ book: Book, _ files: [(name: String, title: String, html: String)]) -> String {
        var points = ""
        for (i, f) in files.enumerated() {
            points += "<navPoint id=\"np\(i + 1)\" playOrder=\"\(i + 1)\"><navLabel><text>\(esc(f.title))</text></navLabel>"
            points += "<content src=\"text/\(f.name)\"/></navPoint>"
        }
        return """
        <?xml version="1.0" encoding="UTF-8"?>
        <ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">
        <head><meta name="dtb:uid" content="\(esc(identifier(book.meta)))"/><meta name="dtb:depth" content="1"/>
        <meta name="dtb:totalPageCount" content="0"/><meta name="dtb:maxPageNumber" content="0"/></head>
        <docTitle><text>\(esc(book.meta.title))</text></docTitle>
        <navMap>\(points)</navMap>
        </ncx>
        """
    }

    static func identifier(_ m: PaperMeta) -> String {
        m.doi.isEmpty ? "urn:uuid:\(UUID().uuidString.lowercased())" : "doi:\(m.doi)"
    }

    static func opf(_ book: Book, files: [(name: String, title: String, html: String)], hasCover: Bool) -> String {
        let m = book.meta
        let modified: String = {
            let f = ISO8601DateFormatter()
            f.formatOptions = [.withInternetDateTime]
            f.timeZone = TimeZone(identifier: "UTC")
            return f.string(from: Date())
        }()
        var md = "<dc:identifier id=\"pub-id\">\(esc(identifier(m)))</dc:identifier>"
        md += "<dc:title>\(esc(m.title.isEmpty ? "Untitled paper" : m.title))</dc:title>"
        for (i, a) in m.authors.prefix(40).enumerated() { md += "<dc:creator id=\"creator\(i + 1)\">\(esc(a))</dc:creator>" }
        md += "<dc:language>en</dc:language>"
        if let d = validDate(m.date) { md += "<dc:date>\(d)</dc:date>" }
        if !m.journal.isEmpty { md += "<dc:publisher>\(esc(m.journal))</dc:publisher>" }
        if !m.abstract.isEmpty { md += "<dc:description>\(esc(String(m.abstract.prefix(1500))))</dc:description>" }
        if !m.license.isEmpty { md += "<dc:rights>\(esc(m.license))</dc:rights>" }
        for k in m.keywords { md += "<dc:subject>\(esc(k))</dc:subject>" }
        md += "<meta property=\"dcterms:modified\">\(modified)</meta>"
        if hasCover { md += "<meta name=\"cover\" content=\"cover-image\"/>" }

        var man = "<item id=\"nav\" href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"/>"
        man += "<item id=\"ncx\" href=\"toc.ncx\" media-type=\"application/x-dtbncx+xml\"/>"
        man += "<item id=\"css\" href=\"css/style.css\" media-type=\"text/css\"/>"
        var spine = ""
        if hasCover {
            man += "<item id=\"cover-image\" href=\"images/p2e-cover.jpg\" media-type=\"image/jpeg\" properties=\"cover-image\"/>"
            man += "<item id=\"cover\" href=\"text/cover.xhtml\" media-type=\"application/xhtml+xml\"/>"
            spine += "<itemref idref=\"cover\"/>"
        }
        for (i, f) in files.enumerated() {
            let props = f.html.contains("<math") ? " properties=\"mathml\"" : ""
            man += "<item id=\"x\(i)\" href=\"text/\(f.name)\" media-type=\"application/xhtml+xml\"\(props)/>"
            spine += "<itemref idref=\"x\(i)\"/>"
        }
        for (i, img) in book.images.enumerated() {
            man += "<item id=\"img\(i)\" href=\"images/\(esc(img.fileName))\" media-type=\"\(img.mediaType)\"/>"
        }
        return """
        <?xml version="1.0" encoding="UTF-8"?>
        <package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="pub-id" xml:lang="en">
        <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">\(md)</metadata>
        <manifest>\(man)</manifest>
        <spine toc="ncx">\(spine)</spine>
        </package>
        """
    }

    /// dc:date must be W3CDTF: YYYY, YYYY-MM or YYYY-MM-DD.
    static func validDate(_ s: String) -> String? {
        let t = String(s.prefix(10))
        return t.range(of: #"^\d{4}(-\d{2}(-\d{2})?)?$"#, options: .regularExpression) != nil ? t : nil
    }
}

enum Style {
    static let css = """
    body { font-family: serif; line-height: 1.45; margin: 0 0.4em; }
    body.cover { margin: 0; padding: 0; text-align: center; }
    .cover-image img { display: block; margin: 0 auto; width: auto; height: auto; max-width: 100%; max-height: 100vh; }
    h1, h2, h3, h4, h5, h6 { font-family: sans-serif; line-height: 1.25; page-break-after: avoid; }
    h1 { font-size: 1.35em; margin-top: 1.6em; }
    h2 { font-size: 1.15em; margin-top: 1.3em; }
    h3 { font-size: 1.0em; margin-top: 1.1em; }
    h4, h5, h6 { font-size: 0.95em; margin-top: 1em; }
    p { margin: 0 0 0.7em 0; text-align: left; }
    .titlepage { margin-top: 2em; }
    .titlepage h1.title { font-family: serif; font-size: 1.6em; margin-bottom: 0.8em; }
    .titlepage .authors { font-size: 1em; }
    .titlepage .journal, .titlepage .doi, .titlepage .license { font-size: 0.9em; color: #444; }
    figure { margin: 1.2em 0; text-align: center; page-break-inside: avoid; }
    figure img { max-width: 100%; height: auto; }
    figcaption, p.caption { font-size: 0.85em; text-align: left; line-height: 1.35; margin-top: 0.4em; }
    img { max-width: 100%; height: auto; }
    img.math { vertical-align: middle; }
    .equation { text-align: center; margin: 0.8em 0; }
    .equation img { max-width: 100%; height: auto; }
    .eqlabel { float: right; }
    math[display="block"] { display: block; margin: 0.6em auto; max-width: 100%; overflow-x: auto; }
    code.tex { font-size: 0.85em; }
    table { border-collapse: collapse; font-size: 0.75em; margin: 0.6em 0; display: block; overflow-x: auto; }
    th, td { border-bottom: 1px solid #bbb; padding: 0.2em 0.4em; vertical-align: top; text-align: left; }
    thead th { border-bottom: 2px solid #555; }
    .table-wrap { margin: 1.2em 0; }
    .tablefoot { font-size: 0.75em; }
    .frontmatter { font-size: 0.82em; color: #444; }
    .box { border: 1px solid #ccc; padding: 0.5em 0.8em; margin: 1em 0; background: #f7f7f7; }
    .refs, .notes { font-size: 0.85em; }
    ol.refs li { margin-bottom: 0.45em; }
    .missing { color: #888; font-style: italic; }
    .smallcaps { font-variant: small-caps; }
    ul.simple { list-style-type: none; }
    a { color: inherit; }
    pre, code { font-size: 0.85em; white-space: pre-wrap; }
    nav#toc ol { list-style-type: none; padding-left: 1em; }
    """
}
