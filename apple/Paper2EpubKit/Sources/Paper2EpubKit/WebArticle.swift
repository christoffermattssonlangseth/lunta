import Foundation

/// Web pages and blog posts: the reading view of a page whose main text Lunta's core picked
/// out (`readWebPage` / `fetchWebPage` in LuntaCore, core/src/webarticle.rs: a readability-style
/// reader, the same on every platform). Here its images are fetched with the same rules as
/// figures (at most 80, normalised to 1600 px, SVG kept) and the book is written. Rust: the
/// same in core/src/convert/webpage.rs (`web_book`).
extension Paper2Epub {
    /// One chapter of the page as the core gives it: XHTML with `<img src="lunta-image:N" …/>`
    /// for the N-th of `imageURLs`.
    public typealias WebChapter = (title: String, html: String, headings: [(level: Int, id: String, title: String)])

    /// Writes a web page's reading view into `dir`. `site`: the site's name (the title page's
    /// "journal" line); `url`: the page's address.
    public static func convert(webPage chapters: [WebChapter], imageURLs: [String], title: String, authors: [String],
                               date: String, site: String, url: String, into dir: URL,
                               log: @escaping Log = { _ in }) async throws -> ConversionResult {
        guard !chapters.isEmpty else { throw fail("This page has no text for a reading view.") }
        let urls = Array(imageURLs.prefix(80))
        if !urls.isEmpty { log("Downloading \(urls.count) image(s)") }
        var files: [String?] = Array(repeating: nil, count: imageURLs.count)
        var images: [BookImage] = []
        for (i, s) in urls.enumerated() {
            try Task.checkCancellation()
            guard let u = URL(string: s), let r = try? await Net.fetch(u, accept: "image/avif,image/webp,image/png,image/svg+xml,image/*;q=0.8", timeout: 30),
                  (200..<300).contains(r.status) else { continue }
            if u.pathExtension.lowercased() == "svg" || r.contentType.contains("svg") {
                images.append(BookImage(fileName: "web\(i + 1).svg", data: r.data, mediaType: "image/svg+xml"))
            } else if let n = Images.normalise(r.data) {
                images.append(BookImage(fileName: "web\(i + 1).\(n.ext)", data: n.data, mediaType: n.mediaType))
            } else { continue }
            files[i] = images.last?.fileName
        }
        let missing = imageURLs.count - files.compactMap { $0 }.count
        let book = chapters.map { c in
            Chapter(title: c.title, html: withImages(c.html, files),
                    headings: c.headings.map { Heading(level: $0.level, id: $0.id, title: $0.title) })
        }
        var meta = PaperMeta()
        meta.title = title
        meta.authors = authors
        meta.date = date
        meta.journal = site
        meta.url = url
        let all = book.map(\.html).joined()
        let host = URL(string: url)?.host?.replacingOccurrences(of: "www.", with: "") ?? "the web"
        let b = Book(meta: meta, chapters: book, images: images.filter { all.contains("../images/\($0.fileName)\"") },
                     sourceLabel: "Web page from \(host)",
                     notes: missing > 0 ? ["\(missing) image(s) could not be downloaded."] : [])
        return try write(b, into: dir, log: log)
    }

    /// The chapter's images in (`../images/<file>`), or left out when they couldn't be fetched:
    /// as `web_article_with_images` in the core.
    static func withImages(_ html: String, _ files: [String?]) -> String {
        let re = try! NSRegularExpression(pattern: #"<img src="lunta-image:(\d+)" alt="([^"]*)"/>"#)
        var out = ""
        var last = html.startIndex
        for m in re.matches(in: html, range: NSRange(html.startIndex..., in: html)) {
            guard let r = Range(m.range, in: html), let ri = Range(m.range(at: 1), in: html), let ra = Range(m.range(at: 2), in: html) else { continue }
            out += html[last..<r.lowerBound]
            if let i = Int(html[ri]), i < files.count, let f = files[i] {
                out += "<img src=\"../images/\(f)\" alt=\"\(html[ra])\"/>"
            }
            last = r.upperBound
        }
        out += html[last...]
        out = out.replacingOccurrences(of: #"<figure>\s*(<figcaption>\s*</figcaption>)?\s*</figure>"#, with: "", options: .regularExpression)
        return out.replacingOccurrences(of: #"<p>\s*</p>"#, with: "", options: .regularExpression)
    }
}
