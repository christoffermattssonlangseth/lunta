import Foundation
import CoreGraphics
import CoreText

/// Generated cover (same design as the Python tool): journal band, title, authors, DOI.
enum Cover {
    static let width = 1200, height = 1800

    static func make(_ meta: PaperMeta) -> Data? {
        guard let cs = CGColorSpace(name: CGColorSpace.sRGB),
              let ctx = CGContext(data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: 0,
                                  space: cs, bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue) else { return nil }
        let H = CGFloat(height), W = CGFloat(width)
        ctx.setFillColor(rgb(0xf6f3ec)); ctx.fill(CGRect(x: 0, y: 0, width: W, height: H))
        ctx.setFillColor(rgb(0x1f4e5f)); ctx.fill(CGRect(x: 0, y: H - 360, width: W, height: 360))

        let journal = (meta.journal.isEmpty ? "Scientific paper" : meta.journal).uppercased()
        draw(String(journal.prefix(48)), font: "HelveticaNeue-Medium", size: 38, color: rgb(0xe8eef0),
             in: ctx, top: 150, maxHeight: 50)
        let year = String(meta.date.prefix(4))
        if !year.isEmpty {
            draw(year, font: "HelveticaNeue", size: 34, color: rgb(0xa9c3cc), in: ctx, top: 215, maxHeight: 50)
        }

        // Title: shrink until it fits in ~820 px
        let title = meta.title.isEmpty ? "Untitled paper" : meta.title
        var size: CGFloat = 82
        while size > 40, textHeight(title, font: "Georgia-Bold", size: size) > 820 { size -= 6 }
        let th = draw(title, font: "Georgia-Bold", size: size, color: rgb(0x1d1d1f), in: ctx, top: 460, maxHeight: 900)

        var y = 460 + th + 40
        ctx.setStrokeColor(rgb(0xc2703d)); ctx.setLineWidth(6)
        ctx.move(to: CGPoint(x: 90, y: H - y)); ctx.addLine(to: CGPoint(x: 330, y: H - y)); ctx.strokePath()
        y += 50
        if !meta.authors.isEmpty {
            let a = meta.authors.prefix(8).joined(separator: ", ") + (meta.authors.count > 8 ? " et al." : "")
            draw(a, font: "HelveticaNeue", size: 36, color: rgb(0x3a3a3c), in: ctx, top: y, maxHeight: max(60, H - y - 180))
        }
        if !meta.doi.isEmpty {
            draw("doi:" + meta.doi, font: "HelveticaNeue", size: 28, color: rgb(0x6e6e73), in: ctx, top: H - 140, maxHeight: 50)
        }
        guard let img = ctx.makeImage() else { return nil }
        return Images.jpeg(img, quality: 0.88)
    }

    private static func rgb(_ hex: Int) -> CGColor {
        CGColor(red: CGFloat((hex >> 16) & 0xff) / 255, green: CGFloat((hex >> 8) & 0xff) / 255,
                blue: CGFloat(hex & 0xff) / 255, alpha: 1)
    }

    private static func attributed(_ s: String, font: String, size: CGFloat, color: CGColor? = nil) -> CFAttributedString {
        let f = CTFontCreateWithName(font as CFString, size, nil)
        var spacing: CGFloat = size * 0.22
        // the setting points at `spacing`, so it is made and used inside withUnsafeBytes
        let para = withUnsafeBytes(of: &spacing) { buf in
            let settings = [CTParagraphStyleSetting(spec: .lineSpacingAdjustment, valueSize: buf.count, value: buf.baseAddress!)]
            return CTParagraphStyleCreate(settings, settings.count)
        }
        var attrs: [CFString: Any] = [kCTFontAttributeName: f, kCTParagraphStyleAttributeName: para]
        if let color { attrs[kCTForegroundColorAttributeName] = color }
        return CFAttributedStringCreate(nil, s as CFString, attrs as CFDictionary)
    }

    private static func textHeight(_ s: String, font: String, size: CGFloat) -> CGFloat {
        let fs = CTFramesetterCreateWithAttributedString(attributed(s, font: font, size: size))
        return CTFramesetterSuggestFrameSizeWithConstraints(fs, CFRange(location: 0, length: 0), nil,
                                                            CGSize(width: 1020, height: CGFloat.greatestFiniteMagnitude), nil).height
    }

    /// Draws wrapped text whose top edge is `top` px from the top of the image; returns the height used.
    @discardableResult
    private static func draw(_ s: String, font: String, size: CGFloat, color: CGColor, in ctx: CGContext,
                             top: CGFloat, maxHeight: CGFloat) -> CGFloat {
        let fs = CTFramesetterCreateWithAttributedString(attributed(s, font: font, size: size, color: color))
        let fit = CTFramesetterSuggestFrameSizeWithConstraints(fs, CFRange(location: 0, length: 0), nil,
                                                               CGSize(width: 1020, height: maxHeight), nil)
        let h = ceil(min(fit.height, maxHeight))
        let rect = CGRect(x: 90, y: CGFloat(height) - top - h, width: 1020, height: h)
        let frame = CTFramesetterCreateFrame(fs, CFRange(location: 0, length: 0), CGPath(rect: rect, transform: nil), nil)
        CTFrameDraw(frame, ctx)
        return h
    }
}
