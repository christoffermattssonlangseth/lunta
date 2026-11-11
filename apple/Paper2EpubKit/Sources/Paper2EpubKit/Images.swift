import Foundation
import ImageIO
import CoreGraphics
import UniformTypeIdentifiers

enum Images {
    /// Downscales and re-encodes an image for the EPUB. PNG/GIF stay PNG (line art, formulas);
    /// everything else (TIFF, JPEG, …) becomes JPEG. Returns nil if ImageIO cannot read it.
    static func normalise(_ data: Data, maxPixels: Int = 1600) -> (data: Data, ext: String, mediaType: String)? {
        guard let src = CGImageSourceCreateWithData(data as CFData, nil),
              CGImageSourceGetCount(src) > 0 else { return nil }
        let type = (CGImageSourceGetType(src) as String?) ?? ""
        let props = CGImageSourceCopyPropertiesAtIndex(src, 0, nil) as? [CFString: Any] ?? [:]
        let w = props[kCGImagePropertyPixelWidth] as? Int ?? 0
        let h = props[kCGImagePropertyPixelHeight] as? Int ?? 0
        let isJPEG = type == UTType.jpeg.identifier
        let keepPNG = type == UTType.png.identifier || type == UTType.gif.identifier
        let model = props[kCGImagePropertyColorModel] as? String

        // Small enough RGB JPEGs pass through untouched.
        if isJPEG, max(w, h) <= maxPixels, model != (kCGImagePropertyColorModelCMYK as String), data.count < 1_500_000 {
            return (data, "jpg", "image/jpeg")
        }
        let opts: [CFString: Any] = [
            kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceThumbnailMaxPixelSize: min(maxPixels, max(w, h, 1)),
            kCGImageSourceCreateThumbnailWithTransform: true,
        ]
        guard let thumb = CGImageSourceCreateThumbnailAtIndex(src, 0, opts as CFDictionary),
              let rgb = toSRGB(thumb, opaque: !keepPNG) else { return nil }
        let outType = keepPNG ? UTType.png : UTType.jpeg
        let out = NSMutableData()
        guard let dest = CGImageDestinationCreateWithData(out, outType.identifier as CFString, 1, nil) else { return nil }
        let dprops: [CFString: Any] = keepPNG ? [:] : [kCGImageDestinationLossyCompressionQuality: 0.82]
        CGImageDestinationAddImage(dest, rgb, dprops as CFDictionary)
        guard CGImageDestinationFinalize(dest) else { return nil }
        return (out as Data, keepPNG ? "png" : "jpg", keepPNG ? "image/png" : "image/jpeg")
    }

    /// Redraws into an sRGB bitmap (handles CMYK and 16-bit TIFFs); opaque images get a white background.
    static func toSRGB(_ img: CGImage, opaque: Bool) -> CGImage? {
        guard let cs = CGColorSpace(name: CGColorSpace.sRGB) else { return nil }
        let info = opaque ? CGImageAlphaInfo.noneSkipLast.rawValue : CGImageAlphaInfo.premultipliedLast.rawValue
        guard let ctx = CGContext(data: nil, width: img.width, height: img.height, bitsPerComponent: 8,
                                  bytesPerRow: 0, space: cs, bitmapInfo: info) else { return nil }
        let r = CGRect(x: 0, y: 0, width: img.width, height: img.height)
        if opaque {
            ctx.setFillColor(CGColor(red: 1, green: 1, blue: 1, alpha: 1))
            ctx.fill(r)
        }
        ctx.draw(img, in: r)
        return ctx.makeImage()
    }

    static func jpeg(_ img: CGImage, quality: Double = 0.85) -> Data? {
        let out = NSMutableData()
        guard let dest = CGImageDestinationCreateWithData(out, UTType.jpeg.identifier as CFString, 1, nil) else { return nil }
        CGImageDestinationAddImage(dest, img, [kCGImageDestinationLossyCompressionQuality: quality] as CFDictionary)
        return CGImageDestinationFinalize(dest) ? out as Data : nil
    }

    static func isImage(_ d: Data) -> Bool {
        d.starts(with: [0xFF, 0xD8, 0xFF]) || d.starts(with: [0x89, 0x50, 0x4E, 0x47]) || d.starts(with: Array("GIF8".utf8))
            || d.starts(with: [0x49, 0x49, 0x2A, 0x00]) || d.starts(with: [0x4D, 0x4D, 0x00, 0x2A])
    }
}

/// Downloads and normalises the figure images a JATS document references.
enum AssetFetcher {
    static let imageExts: Set<String> = ["jpg", "jpeg", "png", "gif", "tif", "tiff", "bmp", "webp"]

    /// Returns href → image (with a unique file name). Missing images are simply absent.
    static func fetch(hrefs: [String], from locator: AssetLocator, log: @escaping (String) -> Void) async -> [String: BookImage] {
        let unique = Array(Set(hrefs.filter { !$0.isEmpty })).sorted()
        if unique.isEmpty { return [:] }
        if case .none = locator { return [:] }
        log("Downloading \(unique.count) figure image(s)")
        var result: [String: BookImage] = [:]
        var usedNames = Set<String>()
        await withTaskGroup(of: (String, Data?).self) { group in
            var it = unique.makeIterator()
            func addNext() -> Bool {
                guard let href = it.next() else { return false }
                group.addTask { (href, await download(href, locator)) }
                return true
            }
            for _ in 0..<6 { if !addNext() { break } }
            while let (href, data) = await group.next() {
                if let data, let norm = Images.normalise(data) {
                    var base = slug(stem(href))
                    if base.isEmpty { base = "img" }
                    var name = "\(base).\(norm.ext)"
                    var i = 2
                    while usedNames.contains(name) { name = "\(base)-\(i).\(norm.ext)"; i += 1 }
                    usedNames.insert(name)
                    result[href] = BookImage(fileName: name, data: norm.data, mediaType: norm.mediaType)
                }
                _ = addNext()
            }
        }
        return result
    }

    static func download(_ href: String, _ locator: AssetLocator) async -> Data? {
        switch locator {
        case .none:
            return nil
        case .s3(let base, let files):
            let name = (href as NSString).lastPathComponent
            let st = stem(href)
            // exact name first, then same stem with a web-friendly extension preferred
            var options = files.filter { $0 == name || $0 == href }
            let rank: [String: Int] = ["jpg": 0, "jpeg": 0, "png": 1, "gif": 2, "tif": 3, "tiff": 3]
            options += files.filter { stem($0) == st && imageExts.contains(ext($0)) }
                .sorted { (rank[ext($0)] ?? 9) < (rank[ext($1)] ?? 9) }
            for f in options {
                if let d = try? await Net.get(base.appendingPathComponent(f), timeout: 90), Images.isImage(d) { return d }
            }
            return nil
        case .patterns(let pats):
            let enc = href.addingPercentEncoding(withAllowedCharacters: .urlPathAllowed) ?? href
            let st = stem(href).addingPercentEncoding(withAllowedCharacters: .urlPathAllowed) ?? stem(href)
            for p in pats {
                let s = p.replacingOccurrences(of: "{href}", with: enc).replacingOccurrences(of: "{stem}", with: st)
                if let u = URL(string: s), let d = try? await Net.get(u, timeout: 60), Images.isImage(d) { return d }
            }
            return nil
        }
    }

    static func stem(_ p: String) -> String {
        let last = (p as NSString).lastPathComponent
        let e = ext(last)
        return imageExts.contains(e) || ["eps", "pdf", "svg", "xml"].contains(e) ? (last as NSString).deletingPathExtension : last
    }

    static func ext(_ p: String) -> String { (p as NSString).pathExtension.lowercased() }
}

func slug(_ s: String, max n: Int = 60) -> String {
    let lowered = s.lowercased()
    var out = ""
    var lastDash = false
    for ch in lowered.unicodeScalars {
        if CharacterSet.alphanumerics.contains(ch) && ch.isASCII {
            out.unicodeScalars.append(ch); lastDash = false
        } else if !lastDash && !out.isEmpty {
            out += "-"; lastDash = true
        }
    }
    while out.hasSuffix("-") { out.removeLast() }
    return String(out.prefix(n)).trimmingCharacters(in: CharacterSet(charactersIn: "-"))
}
