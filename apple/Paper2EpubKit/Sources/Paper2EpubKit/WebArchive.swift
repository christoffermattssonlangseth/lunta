import Foundation

/// Safari's saved pages (.webarchive): a property list with the page (`WebMainResource`) and
/// what it loaded (`WebSubresources`: images, scripts, styles), each with its URL and bytes.
/// Read so a page saved in Safari (Mac, or iPhone / iPad via Share → Save to Files / Lunta) can
/// become a reading view with its figures, the same way the browser extension's pages do.
public enum WebArchive {
    public struct Page: Sendable {
        public var html: String
        public var url: URL?
        public var images: [PageImage]
    }

    static func isWebArchive(_ d: Data) -> Bool {
        d.starts(with: Array("bplist00".utf8)) && d.range(of: Data("WebMainResource".utf8)) != nil
    }

    public static func read(_ data: Data) throws -> Page {
        guard let plist = try? PropertyListSerialization.propertyList(from: data, format: nil) as? [String: Any],
              let main = plist["WebMainResource"] as? [String: Any],
              let body = main["WebResourceData"] as? Data else { throw fail("Not a Safari web archive.") }
        let enc = (main["WebResourceTextEncodingName"] as? String)?.lowercased() ?? "utf-8"
        let html = (enc.contains("utf") ? String(data: body, encoding: .utf8) : nil)
            ?? String(data: body, encoding: .isoLatin1) ?? String(decoding: body, as: UTF8.self)
        let url = (main["WebResourceURL"] as? String).flatMap(URL.init(string:))
        var images: [PageImage] = []
        for r in (plist["WebSubresources"] as? [[String: Any]]) ?? [] {
            guard let u = r["WebResourceURL"] as? String, let d = r["WebResourceData"] as? Data else { continue }
            let mime = (r["WebResourceMIMEType"] as? String) ?? ""
            if mime.hasPrefix("image/") || Images.isImage(d) { images.append(PageImage(urls: [u], data: d)) }
        }
        return Page(html: html, url: url, images: images)
    }
}

extension Paper2Epub {
    /// Converts a Safari web archive of an article page (see `convert(page:)`).
    public static func convert(webArchive data: Data, into dir: URL, log: @escaping Log = { _ in }) async throws -> ConversionResult {
        let page = try WebArchive.read(data)
        log("Web archive: \(page.url?.absoluteString ?? "no URL"), \(page.images.count) image(s)")
        return try await convert(page: page.html, url: page.url, images: page.images, into: dir, log: log)
    }
}
