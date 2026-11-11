import Foundation
import PDFKit
import CoreGraphics
#if canImport(UIKit)
import UIKit
typealias PFont = UIFont
#else
import AppKit
typealias PFont = NSFont
#endif

// Heuristic PDF → book conversion (no LLM). A port of PdfAnalyzer / pdf_heuristic_markdown in
// paper2epub.py, with PDFKit for text and PDFContentScanner for fonts, images and drawings.

struct Span {
    var text: String
    var bold = false, italic = false, sup = false, sub = false
}

final class PLine {
    var text: String
    var bbox: R
    var size: CGFloat
    var bold: CGFloat      // fraction of characters in a bold font
    var math: CGFloat      // fraction in a math font
    var spans: [Span]
    let page: Int

    init(text: String, bbox: R, size: CGFloat, bold: CGFloat, math: CGFloat, spans: [Span], page: Int) {
        self.text = text; self.bbox = bbox; self.size = size; self.bold = bold; self.math = math
        self.spans = spans; self.page = page
    }
}

final class Para {
    enum Kind { case text, heading, caption, front, title, abstract, equation, figure, table }
    var lines: [PLine]
    let page: Int
    var kind: Kind
    var level = 0
    var asset: BookImage?
    var region: R?

    init(_ lines: [PLine], page: Int, kind: Kind = .text, region: R? = nil) {
        self.lines = lines; self.page = page; self.kind = kind; self.region = region
    }

    var text: String { joinLines(lines.map(\.text)) }
    var bbox: R { region ?? R.union(lines.map(\.bbox)) }
    var size: CGFloat {
        var c: [CGFloat: Int] = [:]
        for l in lines { c[l.size, default: 0] += l.text.count }
        return c.max { $0.value < $1.value }?.key ?? 0
    }
}

func joinLines(_ parts: [String]) -> String {
    var out = ""
    for raw in parts {
        let p = raw.trimmingCharacters(in: .whitespaces)
        if p.isEmpty { continue }
        if out.isEmpty { out = p; continue }
        if out.range(of: "[A-Za-z]-$", options: .regularExpression) != nil, p.first?.isLowercase == true {
            out.removeLast(); out += p
        } else if (out.hasSuffix("-") || out.hasSuffix("/") || out.hasSuffix("–")) && !out.hasSuffix(" -") {
            out += p
        } else {
            out += " " + p
        }
    }
    return out
}

private func re(_ p: String, _ opts: NSRegularExpression.Options = []) -> NSRegularExpression {
    try! NSRegularExpression(pattern: p, options: opts)
}

extension NSRegularExpression {
    func test(_ s: String) -> Bool { firstMatch(in: s, range: NSRange(location: 0, length: (s as NSString).length)) != nil }
}

extension String {
    func matches(_ p: String, _ opts: NSRegularExpression.Options = []) -> Bool { re(p, opts).test(self) }
}

final class PDFAnalyzer {
    static let knownSections = re(
        #"^(?:\d+(?:\.\d+)*\.?\s+|[IVX]+\.\s+)?(abstract|summary|introduction|background|results?|discussion|"#
        + #"results and discussion|methods?|materials and methods|methods and materials|star\s*methods|star\+methods|online methods|"#
        + #"conclusions?|references|bibliography|acknowledge?ments?|funding|author contributions?|"#
        + #"competing interests?|declaration of interests?|conflicts? of interest|data availability|code availability|"#
        + #"data and code availability|supplementary (?:information|material|figures?|tables?)|supporting information|"#
        + #"appendix|keywords|significance|highlights|limitations|related work|experiments?|"#
        + #"figure legends|figures|tables|abbreviations|ethics statement|resource availability)\s*[:.]?$"#, .caseInsensitive)
    static let captionRE = re(
        #"^(?:Supplementary\s+|Extended\s+Data\s+|Supporting\s+)?(Figure|Fig\.?|Table|Algorithm|Box)\s*S?\d+[A-Za-z]?\s*[.:|\-–—]?"#,
        .caseInsensitive)

    let doc: PDFDocument
    var pagesLines: [[PLine]] = []
    var graphics: [PageGraphics] = []
    var boxes: [CGRect] = []
    var bodySize: CGFloat = 10
    var running = Set<String>()
    private var imageCounter = 0
    let log: (String) -> Void

    init(doc: PDFDocument, log: @escaping (String) -> Void) {
        self.doc = doc
        self.log = log
    }

    // MARK: - Extraction

    func extract() {
        var sizes: [CGFloat: Int] = [:]
        for pno in 0..<doc.pageCount {
            guard let page = doc.page(at: pno) else { pagesLines.append([]); graphics.append(PageGraphics()); boxes.append(.zero); continue }
            let mb = page.bounds(for: .mediaBox)
            boxes.append(mb)
            if Self.isCoverSheet(page) {
                pagesLines.append([]); graphics.append(PageGraphics()); continue
            }
            let g = page.pageRef.map { PDFContentScanner(box: mb).scan($0) } ?? PageGraphics()
            graphics.append(g)
            let lines = extractLines(page, pno: pno, mb: mb, runs: g.runs)
            for l in lines { sizes[l.size, default: 0] += l.text.count }
            pagesLines.append(lines)
            if pno % 5 == 4 { log("Reading page \(pno + 1) of \(doc.pageCount)") }
        }
        if let best = sizes.max(by: { $0.value < $1.value }) { bodySize = best.key }
        findRunning()
    }

    private func extractLines(_ page: PDFPage, pno: Int, mb: CGRect, runs: [TextRun]) -> [PLine] {
        guard let attr = page.attributedString, let sel = page.selection(for: mb) else { return [] }
        let ns = attr.string as NSString
        // bucket text runs by baseline for quick style lookup
        var buckets: [Int: [TextRun]] = [:]
        for r in runs { buckets[Int(r.baseline / 4), default: []].append(r) }

        func toTop(_ b: CGRect) -> R {
            R(x0: b.minX - mb.minX, y0: mb.maxY - b.maxY, x1: b.maxX - mb.minX, y1: mb.maxY - b.minY)
        }
        func style(_ cb: R, size: CGFloat) -> TextRun? {
            let cx = cb.cx
            var best: TextRun?
            var bestD = CGFloat.greatestFiniteMagnitude
            let key = Int(cb.y1 / 4)
            for k in (key - 2)...(key + 2) {
                for r in buckets[k] ?? [] where cx >= r.x0 - 0.8 && cx <= r.x1 + 0.8
                    && r.baseline >= cb.y0 + 0.3 * cb.h && r.baseline <= cb.y1 + 1 {
                    let d = abs(r.baseline - (cb.y1 - 0.22 * max(size, 4)))
                    if d < bestD { bestD = d; best = r }
                }
            }
            return best
        }

        var out: [PLine] = []
        for ls in sel.selectionsByLine() {
            let nr = ls.numberOfTextRanges(on: page)
            guard nr > 0 else { continue }
            var range = ls.range(at: 0, on: page)
            for k in 1..<max(1, nr) { range = NSUnionRange(range, ls.range(at: k, on: page)) }
            guard range.location != NSNotFound, range.location + range.length <= ns.length, range.length > 0 else { continue }
            let bbox = toTop(ls.bounds(for: page))
            if bbox.h > 2.5 * bbox.w && range.length > 2 { continue }   // rotated margin text

            struct C { var ch: String; var size: CGFloat; var box: R; var run: TextRun? }
            var chars: [C] = []
            var i = range.location
            while i < range.location + range.length {
                let r = ns.rangeOfComposedCharacterSequence(at: i)
                let ch = ns.substring(with: r)
                i = r.location + r.length
                if ch == "\n" || ch == "\r" { continue }
                let f = attr.attribute(.font, at: r.location, effectiveRange: nil) as? PFont
                // characterBounds(at:) uses a different index space on some PDFs; a 1-char selection is reliable
                let box = page.selection(for: r).map { toTop($0.bounds(for: page)) } ?? R(x0: 0, y0: 0, x1: 0, y1: 0)
                let size = f?.pointSize ?? 0
                chars.append(C(ch: ch, size: size, box: box, run: nil))
            }
            guard !chars.isEmpty else { continue }
            var sizeCount: [CGFloat: Int] = [:]
            for c in chars where !c.ch.trimmingCharacters(in: .whitespaces).isEmpty {
                sizeCount[(c.size * 2).rounded() / 2, default: 0] += 1
            }
            guard let dom = sizeCount.max(by: { $0.value < $1.value })?.key, dom > 0 else { continue }
            let mains = chars.filter { abs($0.size - dom) < 0.6 && $0.box.h > 0 }
            let mainCenter = mains.isEmpty ? bbox.y0 + bbox.h / 2 : mains.map { ($0.box.y0 + $0.box.y1) / 2 }.reduce(0, +) / CGFloat(mains.count)
            let mainHeight = mains.isEmpty ? bbox.h : mains.map(\.box.h).reduce(0, +) / CGFloat(mains.count)
            var spans: [Span] = []
            var nBold = 0, nMath = 0, n = 0
            var last: TextRun?
            var prevWasSpace = true
            var text = ""
            for idx in chars.indices {
                let c = chars[idx]
                let isSpace = c.ch.trimmingCharacters(in: .whitespaces).isEmpty
                var run = last
                if !isSpace, c.box.w > 0 || c.box.h > 0 {
                    run = style(c.box, size: c.size) ?? last
                    last = run
                }
                prevWasSpace = isSpace
                text += c.ch
                let small = c.size > 0 && c.size < 0.8 * dom
                let center = (c.box.y0 + c.box.y1) / 2
                // some PDFs give every character the full line box: then fall back to "small citation-like text"
                let flatBoxes = c.box.h > 0.85 * mainHeight
                let citeLike = c.ch.range(of: #"^[0-9,–\-−*†‡§a-e]$"#, options: .regularExpression) != nil
                let sup = !isSpace && small && (flatBoxes ? citeLike : center < mainCenter - 0.12 * dom)
                let sub = !isSpace && small && !flatBoxes && center > mainCenter + 0.12 * dom
                if !isSpace {
                    n += 1
                    if run?.bold == true { nBold += 1 }
                    if run?.math == true { nMath += 1 }
                }
                let sp = Span(text: c.ch, bold: run?.bold ?? false, italic: run?.italic ?? false, sup: sup, sub: sub)
                if let lastSpan = spans.last, (isSpace && !lastSpan.sup && !lastSpan.sub) ||
                    (lastSpan.bold == sp.bold && lastSpan.italic == sp.italic && lastSpan.sup == sp.sup && lastSpan.sub == sp.sub) {
                    spans[spans.count - 1].text += c.ch
                } else {
                    spans.append(sp)
                }
            }
            guard !text.trimmingCharacters(in: .whitespaces).isEmpty else { continue }
            out.append(PLine(text: text, bbox: bbox, size: dom, bold: CGFloat(nBold) / CGFloat(max(1, n)),
                             math: CGFloat(nMath) / CGFloat(max(1, n)), spans: spans, page: pno))
        }
        return out
    }

    /// Publisher download sheets ("Use of this article is subject to the Terms of service") carry no content.
    static func isCoverSheet(_ page: PDFPage) -> Bool {
        let t = page.string ?? ""
        return t.count < 3000 && (t.contains("Use of this article is subject to the Terms of service")
            || t.contains("View the article online") && t.contains("Permissions"))
    }

    private func norm(_ s: String) -> String {
        s.lowercased().replacingOccurrences(of: #"\b[es]?\d+\b"#, with: "", options: .regularExpression)
            .replacingOccurrences(of: #"\s+"#, with: " ", options: .regularExpression).trimmingCharacters(in: .whitespaces)
    }

    private func findRunning() {
        let n = doc.pageCount
        guard n >= 3 else { return }
        var c: [String: Int] = [:]
        for (pno, lines) in pagesLines.enumerated() {
            let h = boxes[pno].height
            var seen = Set<String>()
            for l in lines where l.bbox.y0 < 0.09 * h || l.bbox.y1 > 0.91 * h { seen.insert(norm(l.text)) }
            for s in seen { c[s, default: 0] += 1 }
        }
        running = Set(c.filter { Double($0.value) >= max(2, 0.3 * Double(n)) }.keys)
    }

    func isNoise(_ l: PLine) -> Bool {
        let t = l.text.trimmingCharacters(in: .whitespaces)
        let h = boxes[l.page].height
        let inMargin = l.bbox.y0 < 0.09 * h || l.bbox.y1 > 0.91 * h
        if inMargin && (running.contains(norm(t)) || t.matches(#"^[\divxlcIVXLC\s\-–/of|]+$"#)) { return true }
        if t.matches(#"^\d{1,4}$"#) && l.bbox.w < 25 { return true }
        if t.matches(#"^(bioRxiv|medRxiv) preprint doi"#, .caseInsensitive) || t.contains("copyright holder for this preprint") { return true }
        if inMargin && t.matches(#"^(downloaded from|arxiv:\d{4}\.\d+)"#, .caseInsensitive) { return true }
        return false
    }

    // MARK: - Figure / table regions

    struct Region { var rect: R; var kind: Para.Kind; var image: BookImage? }

    private struct Cluster { var rect: R; var hasImage: Bool; var n: Int }

    func findRegions(_ pno: Int, _ lines: [PLine]) -> [Region] {
        let mb = boxes[pno]
        let parea = mb.width * mb.height
        let g = graphics[pno]
        var rects: [(img: Bool, r: R)] = []
        for r in g.images {
            if (r.w < 1 && r.h < 1) || r.area > 0.95 * parea { continue }
            rects.append((true, r))
        }
        var thin: [R] = []
        for r in g.drawings {
            if r.w < 2.5 || r.h < 2.5 { thin.append(r) } else if r.area < 0.9 * parea { rects.append((false, r)) }
        }
        rects.sort { ($0.r.y0, $0.r.x0) < ($1.r.y0, $1.r.x0) }
        var clusters: [Cluster] = []
        for (img, r) in rects {
            if let li = clusters.indices.last, clusters[li].rect.intersects(r, pad: 6) {
                clusters[li].rect = clusters[li].rect.union(r); clusters[li].hasImage = clusters[li].hasImage || img; clusters[li].n += 1
                continue
            }
            if let ci = clusters.firstIndex(where: { $0.rect.intersects(r, pad: 6) }) {
                clusters[ci].rect = clusters[ci].rect.union(r); clusters[ci].hasImage = clusters[ci].hasImage || img; clusters[ci].n += 1
            } else {
                clusters.append(Cluster(rect: r, hasImage: img, n: 1))
            }
        }
        mergeClusters(&clusters) { a, b in a.intersects(b, pad: 6) }
        // join nearby pieces of one figure unless body text sits between them
        let prose = lines.filter { $0.text.count > 50 && $0.size >= bodySize - 0.5 }.map(\.bbox)
        mergeClusters(&clusters) { a, b in
            guard a.intersects(b, pad: 22) else { return false }
            let u = a.union(b)
            return !prose.contains { u.contains($0, tol: 0) && !$0.intersects(a) && !$0.intersects(b) }
        }
        for ci in clusters.indices {
            for r in thin where clusters[ci].rect.intersects(r, pad: 2) { clusters[ci].rect = clusters[ci].rect.union(r) }
        }
        var regions: [Region] = []
        let mTop = 0.07 * mb.height, mBot = 0.93 * mb.height
        for c in clusters {
            let r = c.rect
            if r.area < 2500 || r.y1 < mTop || r.y0 > mBot { continue }
            let inside = lines.filter { r.contains($0.bbox) }.reduce(0) { $0 + $1.text.count }
            if !c.hasImage && c.n <= 3 && inside > 250 { continue }   // shaded text box
            if !c.hasImage && c.n <= 2 && r.h < 30 { continue }
            regions.append(Region(rect: r, kind: .figure))
        }

        // tables: caption + horizontal rules, or a borderless grid of short cells
        let caps = lines.filter { $0.text.trimmingCharacters(in: .whitespaces).matches(#"^(Supplementary\s+)?Table\s*S?\d+"#, .caseInsensitive) }
        let hrules = thin.filter { $0.w > 80 && $0.h < 2.5 }.sorted { $0.y0 < $1.y0 }
        for cap in caps {
            if regions.contains(where: { cap.bbox.intersects($0.rect, pad: 30) }) { continue }
            let below = hrules.filter { $0.y0 > cap.bbox.y0 - 2 }
            let above = hrules.filter { $0.y1 < cap.bbox.y0 + 2 }
            var grp: [R]?
            if let f = below.first, f.y0 - cap.bbox.y1 < 60 {
                var gg = [f]
                for r in below.dropFirst() { if r.y0 - gg.last!.y1 < 250 { gg.append(r) } else { break } }
                grp = gg
            } else if let l = above.last, cap.bbox.y0 - l.y1 < 60 {
                var gg = [l]
                for r in above.dropLast().reversed() { if gg.last!.y0 - r.y1 < 250 { gg.append(r) } else { break } }
                grp = gg
            }
            if let grp, grp.count >= 2 {
                regions.append(Region(rect: R.union(grp), kind: .table))
            } else if let tr = textTableRegion(cap, lines, mb) {
                regions.append(Region(rect: tr, kind: .table))
            }
        }
        // expand regions with nearby short labels (axis labels, panel letters)
        for gi in regions.indices {
            var r = regions[gi].rect
            for _ in 0..<2 {
                for l in lines {
                    let t = l.text.trimmingCharacters(in: .whitespaces)
                    if Self.captionRE.test(t) { continue }
                    let vo = min(r.y1, l.bbox.y1) - max(r.y0, l.bbox.y0) > 0.6 * l.bbox.h
                    if r.intersects(l.bbox, pad: 8) && t.count < 70 && !r.contains(l.bbox)
                        && (l.size < bodySize - 0.4 || vo || t.count <= 12) {
                        r = r.union(l.bbox)
                    }
                }
            }
            regions[gi].rect = r
        }
        return regions
    }

    private func mergeClusters(_ cs: inout [Cluster], _ shouldMerge: (R, R) -> Bool) {
        var merged = true
        while merged {
            merged = false
            outer: for i in cs.indices {
                for j in (i + 1)..<cs.count where shouldMerge(cs[i].rect, cs[j].rect) {
                    cs[i].rect = cs[i].rect.union(cs[j].rect)
                    cs[i].hasImage = cs[i].hasImage || cs[j].hasImage
                    cs[i].n += cs[j].n
                    cs.remove(at: j)
                    merged = true
                    break outer
                }
            }
        }
    }

    private func textTableRegion(_ cap: PLine, _ lines: [PLine], _ mb: CGRect) -> R? {
        let (x0, x1): (CGFloat, CGFloat) = cap.bbox.w < 0.6 * mb.width ? (cap.bbox.x0 - 5, cap.bbox.x1 + 5) : (0, mb.width)
        var rows: [Int: [PLine]] = [:]
        for l in lines where l !== cap && l.bbox.x0 >= x0 - 40 && l.bbox.x1 <= x1 + 40 {
            rows[Int((l.bbox.y0 / 3).rounded()), default: []].append(l)
        }
        let ys = rows.keys.sorted()
        func runFrom(_ startY: CGFloat, down: Bool) -> R? {
            let seq = down ? ys.filter { CGFloat($0) * 3 >= startY } : ys.filter { CGFloat($0) * 3 <= startY }.reversed()
            var got: [Int] = []
            var last: CGFloat?
            var nMulti = 0
            for y in seq {
                let row = rows[y]!
                let ry = down ? row.map(\.bbox.y0).min()! : row.map(\.bbox.y1).max()!
                if let last, abs(ry - last) > 40 { break }
                let multi = row.count >= 2
                if !multi && row.reduce(0, { $0 + $1.text.count }) > 90 { break }
                got.append(y)
                if multi { nMulti += 1 }
                last = down ? row.map(\.bbox.y1).max()! : row.map(\.bbox.y0).min()!
            }
            if got.count >= 3 && Double(nMulti) >= 0.5 * Double(got.count) {
                return R.union(got.flatMap { rows[$0]! }.map(\.bbox))
            }
            return nil
        }
        let up = cap.bbox.y0 > mb.height * 0.25 ? runFrom(cap.bbox.y0 - 1, down: false) : nil
        let down = runFrom(cap.bbox.y1 + 1, down: true)
        return [up, down].compactMap { $0 }.max { $0.area < $1.area }
    }

    // MARK: - Rendering crops

    func render(_ pno: Int, _ rect: R, photo: Bool, name: String) -> BookImage? {
        guard let page = doc.page(at: pno) else { return nil }
        let mb = boxes[pno]
        var r = R(x0: rect.x0 - 4, y0: rect.y0 - 4, x1: rect.x1 + 4, y1: rect.y1 + 4)
            .intersection(R(x0: 0, y0: 0, x1: mb.width, y1: mb.height))
        guard r.w > 2, r.h > 2 else { return nil }
        let scale = min(200.0 / 72.0, 1600 / max(r.w, r.h))
        let W = Int((r.w * scale).rounded()), H = Int((r.h * scale).rounded())
        guard let cs = CGColorSpace(name: CGColorSpace.sRGB),
              let ctx = CGContext(data: nil, width: W, height: H, bitsPerComponent: 8, bytesPerRow: 0, space: cs,
                                  bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue) else { return nil }
        ctx.setFillColor(CGColor(red: 1, green: 1, blue: 1, alpha: 1))
        ctx.fill(CGRect(x: 0, y: 0, width: W, height: H))
        ctx.scaleBy(x: scale, y: scale)
        ctx.translateBy(x: -r.x0, y: -(mb.height - r.y1))
        page.draw(with: .mediaBox, to: ctx)
        guard let img = ctx.makeImage() else { return nil }
        r = rect
        imageCounter += 1
        if photo, let d = Images.jpeg(img, quality: 0.85) {
            return BookImage(fileName: "\(name).jpg", data: d, mediaType: "image/jpeg")
        }
        let out = NSMutableData()
        guard let dest = CGImageDestinationCreateWithData(out, "public.png" as CFString, 1, nil) else { return nil }
        CGImageDestinationAddImage(dest, img, nil)
        guard CGImageDestinationFinalize(dest) else { return nil }
        return BookImage(fileName: "\(name).png", data: out as Data, mediaType: "image/png")
    }

    // MARK: - Assembly

    func analyze() -> [Para] {
        extract()
        var all: [Para] = []
        let pageLines = pagesLines.map { $0.filter { !isNoise($0) } }
        var pageRegions = pageLines.indices.map { findRegions($0, pageLines[$0]) }
        // logos and banners that sit in the same place on many pages are not figures
        func key(_ r: R) -> String { "\(Int(r.x0 / 6)),\(Int(r.y0 / 6)),\(Int(r.x1 / 6)),\(Int(r.y1 / 6))" }
        var counts: [String: Int] = [:]
        for rs in pageRegions { for k in Set(rs.map { key($0.rect) }) { counts[k, default: 0] += 1 } }
        let limit = max(3, Int(0.3 * Double(pageRegions.count)))
        pageRegions = pageRegions.map { rs in rs.filter { (counts[key($0.rect)] ?? 0) < limit } }
        for pno in pagesLines.indices {
            let lines = pageLines[pno]
            var regions = pageRegions[pno]
            for i in regions.indices {
                let rr = regions[i].rect
                let photo = graphics[pno].images.contains { rr.contains($0, tol: 5) && $0.area > 0.3 * rr.area }
                regions[i].image = render(pno, rr, photo: photo, name: "p\(pno + 1)-\(regions[i].kind == .table ? "table" : "figure")\(i + 1)")
            }
            let free = lines.filter { l in
                !regions.contains { g in
                    g.rect.contains(l.bbox, tol: 3) || (l.bbox.area > 0 && g.rect.intersects(l.bbox)
                        && g.rect.intersection(l.bbox).area > 0.6 * l.bbox.area)
                }
            }
            all += paragraphs(pno, free, regions)
            if pno % 5 == 4 { log("Laying out page \(pno + 1) of \(doc.pageCount)") }
        }
        return classify(all)
    }

    func paragraphs(_ pno: Int, _ input: [PLine], _ regions: [Region]) -> [Para] {
        // merge fragments on the same baseline (PDFKit sometimes splits a line)
        let sorted = input.sorted { (($0.bbox.y0).rounded(), $0.bbox.x0) < (($1.bbox.y0).rounded(), $1.bbox.x0) }
        var merged: [PLine] = []
        for l in sorted {
            if let m = merged.last, abs(m.bbox.y0 - l.bbox.y0) < 2, l.bbox.x0 >= m.bbox.x1 - 1, l.bbox.x0 - m.bbox.x1 < 1.2 * l.size {
                m.text += (m.text.hasSuffix(" ") ? "" : " ") + l.text
                m.spans += [Span(text: " ")] + l.spans
                m.bbox = m.bbox.union(l.bbox)
            } else {
                merged.append(l)
            }
        }
        var units = merged.map { Para([$0], page: pno) }
        for g in regions {
            let p = Para([], page: pno, kind: g.kind, region: g.rect)
            p.asset = g.image
            units.append(p)
        }
        units = order(pno, units)
        let txt = units.filter { $0.kind == .text }.map { $0.lines[0] }
        func rightEdge(_ l: PLine) -> CGFloat {
            let ov = txt.filter { min($0.bbox.x1, l.bbox.x1) - max($0.bbox.x0, l.bbox.x0) > 0.3 * l.bbox.w }.map(\.bbox.x1)
            return ov.max() ?? l.bbox.x1
        }
        let abbrev = re(#"(\b(e\.g|i\.e|et al|cf|Fig|Figs|Eq|Eqs|vs|approx|ca|resp|Ref|Refs|No|Dr|Prof|al)\.|\b[A-Z]\.)$"#)
        let W = boxes[pno].width
        var paras: [Para] = []
        var cur: [PLine] = []
        func flush() { if !cur.isEmpty { paras.append(Para(cur, page: pno)); cur = [] } }
        for u in units {
            if u.kind != .text { flush(); paras.append(u); continue }
            let l = u.lines[0]
            guard let a = cur.last else { cur.append(l); continue }
            let left = cur.map(\.bbox.x0).min()!
            let gap = l.bbox.y0 - a.bbox.y1
            let jumped = l.bbox.y0 < a.bbox.y0 - 2 || abs(l.bbox.x0 - left) > 0.3 * W
            let reA = rightEdge(a)
            let width = max(1, reA - left)
            let at = a.text.trimmingCharacters(in: .whitespaces)
            let endsSentence = at.hasSuffix(".") || at.hasSuffix(":") || at.hasSuffix("?") || at.hasSuffix("!")
            let shortA = (reA - a.bbox.x1) > 0.18 * width && endsSentence && !abbrev.test(at)
            let indent = l.bbox.x0 - left > 8 && cur.dropFirst().allSatisfy { abs($0.bbox.x0 - left) < 3 } && endsSentence && !abbrev.test(at)
            let hanging = cur.count >= 2 && l.bbox.x0 < a.bbox.x0 - 6 && cur[0].bbox.x0 < a.bbox.x0 - 6
            let sizeChange = abs(l.size - a.size) >= 1.0
            let styleBreak = (a.bold > 0.9) != (l.bold > 0.9) && (at.count < 120 || l.text.count < 120)
            let cap = Self.captionRE.test(l.text.trimmingCharacters(in: .whitespaces))
            if jumped || gap > 0.75 * max(l.size, a.size) || shortA || indent || hanging || sizeChange || styleBreak || cap
                || (cur.count == 1 && l.bbox.x0 > a.bbox.x0 + 8 && gap > 0.3 * l.size) {
                if jumped && !(gap > 0.75 * l.size) && !shortA && !sizeChange && !cap
                    && l.text.trimmingCharacters(in: .whitespaces).first?.isLowercase == true {
                    cur.append(l); continue   // column continuation
                }
                flush()
            }
            cur.append(l)
        }
        flush()
        return paras
    }

    func order(_ pno: Int, _ paras: [Para]) -> [Para] {
        let W = boxes[pno].width
        let textual = paras.filter { $0.kind == .text }
        let narrow = textual.filter { $0.bbox.w < 0.55 * W }
        let charsN = narrow.reduce(0) { $0 + $1.text.count }
        let charsAll = max(1, textual.reduce(0) { $0 + $1.text.count })
        let left = narrow.filter { $0.bbox.cx < W / 2 }.reduce(0) { $0 + $1.text.count }
        let right = charsN - left
        let twoCol = Double(charsN) > 0.5 * Double(charsAll) && Double(left) > 0.2 * Double(charsN) && Double(right) > 0.2 * Double(charsN)
        if !twoCol {
            return paras.sorted { (($0.bbox.y0 / 4).rounded(), $0.bbox.x0) < (($1.bbox.y0 / 4).rounded(), $1.bbox.x0) }
        }
        let wide = paras.filter { $0.bbox.w >= 0.55 * W }.sorted { $0.bbox.y0 < $1.bbox.y0 }
        let rest = paras.filter { $0.bbox.w < 0.55 * W }
        var out: [Para] = []
        var prevY: CGFloat = -1
        for wp in wide.map(Optional.some) + [nil] {
            let y = wp?.bbox.y0 ?? .greatestFiniteMagnitude
            let band = rest.filter { prevY <= $0.bbox.y0 && $0.bbox.y0 < y }
            out += band.filter { $0.bbox.cx < W / 2 }.sorted { $0.bbox.y0 < $1.bbox.y0 }
            out += band.filter { $0.bbox.cx >= W / 2 }.sorted { $0.bbox.y0 < $1.bbox.y0 }
            if let wp { out.append(wp); prevY = wp.bbox.y0 }
        }
        return out
    }

    func classify(_ input: [Para]) -> [Para] {
        let body = bodySize
        // glue wrapped large-font lines (titles, long headings)
        var paras: [Para] = []
        for p in input {
            if let q = paras.last, p.kind == .text, q.kind == .text, p.page == q.page, p.size == q.size,
               p.size >= body + 1, p.bbox.y0 - q.bbox.y1 >= 0, p.bbox.y0 - q.bbox.y1 < 0.9 * p.size,
               q.text.count + p.text.count < 400 {
                paras[paras.count - 1] = Para(q.lines + p.lines, page: q.page)
            } else {
                paras.append(p)
            }
        }
        if let big = paras.filter({ $0.page == 0 && $0.kind == .text }).max(by: { $0.size < $1.size }),
           big.size >= body + 2, big.text.count < 400 {
            big.kind = .title
        }
        var headingSizes = Set<CGFloat>()
        for p in paras where p.kind == .text {
            let t = p.text.trimmingCharacters(in: .whitespaces)
            if Self.captionRE.test(t) { p.kind = .caption; continue }
            let n = t.count
            let totalChars = max(1, p.lines.reduce(0) { $0 + $1.text.count })
            let boldish = p.lines.reduce(CGFloat(0)) { $0 + $1.bold * CGFloat($1.text.count) } / CGFloat(totalChars)
            let numbered = t.matches(#"^(\d+(?:\.\d+)*)\.?\s+[A-Z]"#)
            let endsSentence = t.hasSuffix(".") && n > 60
            if n <= 160 && p.lines.count <= 3 && !endsSentence {
                if p.size >= body + 1 || Self.knownSections.test(t) || (boldish > 0.85 && n < 110 && p.size >= body - 0.6)
                    || (numbered && boldish > 0.5) {
                    if p.size < body - 1.5 { continue }
                    p.kind = .heading
                    headingSizes.insert(p.size)
                }
            }
        }
        // runs of 3+ "headings" are usually table cells or figure labels
        var i = 0
        while i < paras.count {
            var j = i
            while j < paras.count && paras[j].kind == .heading { j += 1 }
            if j - i >= 3 {
                for p in paras[i..<j] {
                    let t = p.text.trimmingCharacters(in: .whitespaces)
                    if !(Self.knownSections.test(t) || t.matches(#"^[A-Z]?\d+(\.\d+)*\.?\s+[A-Z][a-z]"#)) { p.kind = .text }
                }
            }
            i = max(j, i + 1)
        }
        for p in paras where p.kind == .heading {
            let t = p.text.trimmingCharacters(in: .whitespaces)
            if t.matches(#"^[\(\[\d·.,\s]+$"#) || t.hasPrefix("(") || t.hasPrefix("[") { p.kind = .text }
        }
        // front matter: page-1 material before the first real section heading
        var boundary: Int?
        for (k, p) in paras.enumerated() {
            if p.page > 1 { break }
            let t = p.text.trimmingCharacters(in: .whitespaces)
            if p.kind == .heading && (Self.knownSections.test(t) || t.matches(#"^\d+\.?\s+[A-Z]"#)) { boundary = k; break }
        }
        let bnd = boundary ?? (paras.firstIndex { $0.page > 1 || ($0.page == 1 && $0.kind == .heading) } ?? paras.count)
        var abstractStarted = false
        for p in paras[0..<bnd] {
            if [.title, .figure, .table, .caption, .equation].contains(p.kind) { continue }
            let affil = p.text.components(separatedBy: .whitespaces).filter {
                $0.matches(#"(?i)universit|institut|department|centre|center|hospital|laborator|faculty"#)
            }.count
            if abstractStarted && p.kind == .text { continue }
            if p.text.trimmingCharacters(in: .whitespaces).matches(#"^abstract\b"#, .caseInsensitive) || (p.text.count >= 500 && affil < 2) {
                p.kind = .abstract
                abstractStarted = true
            } else if p.kind == .text || p.kind == .heading {
                p.kind = .front
            }
        }
        // affiliation / correspondence footnotes on the first page belong to the front matter
        for p in paras where p.page == 0 && p.kind == .text {
            let affil = p.text.matches(#"(?i)universit|institut|department|hospital|laborator|faculty"#)
            if affil && (p.text.matches(#"(?i)e-?mail:|correspond"#) || p.text.matches(#"^\d\s?[A-Z]"#)) { p.kind = .front }
        }
        let ranks = headingSizes.sorted(by: >)
        let big = ranks.filter { $0 >= body + 1 }
        let titleText = paras.first { $0.kind == .title }?.text ?? ""
        for p in paras where p.kind == .heading {
            let t = p.text.trimmingCharacters(in: .whitespaces)
            if !titleText.isEmpty && PDFBook.similar(t, titleText) { p.kind = .title; continue }
            if Self.knownSections.test(t) && !t.matches(#"^\d+\.\d"#) { p.level = 1; continue }
            if let m = Resolver.firstMatch(#"^(\d+(?:\.\d+)*)\.?\s"#, in: t) {
                p.level = min(3, m.filter { $0 == "." }.count + 1)
            } else {
                p.level = min(3, (big.firstIndex(of: p.size).map { $0 + 1 }) ?? (big.count + 1))
            }
        }
        // display equations: mostly math-font lines; merge stacked pieces
        func mathFrac(_ p: Para) -> (CGFloat, Int) {
            let n = p.lines.reduce(0) { $0 + $1.text.count }
            return (p.lines.reduce(CGFloat(0)) { $0 + $1.math * CGFloat($1.text.count) } / CGFloat(max(1, n)), n)
        }
        var out: [Para] = []
        for p in paras {
            if p.kind == .text, !p.lines.isEmpty {
                let (mf, n) = mathFrac(p)
                if mf > 0.3 && n < 400 { p.kind = .equation }
            }
            if let prev = out.last, prev.page == p.page, (p.kind == .equation || p.kind == .text), prev.kind == .equation,
               p.bbox.y0 - prev.bbox.y1 < 1.6 * body, p.kind == .equation || p.text.count < 30 {
                out[out.count - 1] = Para(prev.lines + p.lines, page: p.page, kind: .equation)
                continue
            }
            if let prev = out.last, prev.page == p.page, p.kind == .equation, prev.kind == .text, prev.text.count < 30,
               p.bbox.y0 - prev.bbox.y1 < body, !prev.text.matches("[a-z]{4}") {
                out[out.count - 1] = Para(prev.lines + p.lines, page: p.page, kind: .equation)
                continue
            }
            out.append(p)
        }
        for p in out where p.kind == .equation {
            p.asset = render(p.page, p.bbox, photo: false, name: "p\(p.page + 1)-eq\(Int(p.bbox.y0))")
        }
        return out
    }

    /// Paragraph text as XHTML with italics/bold/super- and subscripts.
    func styled(_ p: Para) -> String {
        let totalChars = max(1, p.lines.reduce(0) { $0 + $1.text.count })
        let boldChars = p.lines.flatMap(\.spans).filter(\.bold).reduce(0) { $0 + $1.text.count }
        let paraBold = Double(boldChars) > 0.85 * Double(totalChars)
        var linesHTML: [(plain: String, html: String)] = []
        for l in p.lines {
            var h = ""
            for s in l.spans {
                let core = s.text.trimmingCharacters(in: .whitespaces)
                if core.isEmpty { h += esc(s.text); continue }
                let lead = String(s.text.prefix { $0 == " " }), trail = String(s.text.reversed().prefix { $0 == " " })
                var c = esc(core)
                if s.sup && core.count < 25 { c = "<sup>\(c)</sup>" }
                else if s.sub && core.count < 25 { c = "<sub>\(c)</sub>" }
                else {
                    if s.bold && !paraBold { c = "<strong>\(c)</strong>" }
                    if s.italic { c = "<em>\(c)</em>" }
                }
                h += lead + c + trail
            }
            linesHTML.append((l.text.trimmingCharacters(in: .whitespaces), h.trimmingCharacters(in: .whitespaces)))
        }
        // join with the same de-hyphenation rules as joinLines
        var out = "", outPlain = ""
        for (plain, html) in linesHTML where !plain.isEmpty {
            if outPlain.isEmpty { out = html; outPlain = plain; continue }
            if outPlain.range(of: "[A-Za-z]-$", options: .regularExpression) != nil, plain.first?.isLowercase == true,
               let r = out.range(of: #"-((</[a-z]+>)*)$"#, options: .regularExpression) {
                let tags = String(out[r].dropFirst())
                out.replaceSubrange(r, with: tags)
                out += html; outPlain = String(outPlain.dropLast()) + plain
            } else if (outPlain.hasSuffix("-") || outPlain.hasSuffix("/") || outPlain.hasSuffix("–")) && !outPlain.hasSuffix(" -") {
                out += html; outPlain += plain
            } else {
                out += " " + html; outPlain += " " + plain
            }
        }
        return out
    }

    // MARK: - Metadata

    func metaGuess(_ paras: [Para]) -> PaperMeta {
        var m = PaperMeta()
        let attrs = doc.documentAttributes ?? [:]
        let t = ((attrs[PDFDocumentAttribute.titleAttribute] as? String) ?? "").trimmingCharacters(in: .whitespaces)
        if t.count > 12 && !t.matches(#"(?i)microsoft word|untitled|\.docx?$|\.pdf$|^paper$"#) { m.title = t }
        if m.title.isEmpty, let tp = paras.first(where: { $0.kind == .title }) { m.title = tp.text }
        let a = ((attrs[PDFDocumentAttribute.authorAttribute] as? String) ?? "").trimmingCharacters(in: .whitespaces)
        if !a.isEmpty && !a.matches(#"(?i)administrator|user|owner|^admin"#) {
            m.authors = a.components(separatedBy: CharacterSet(charactersIn: ";,")).flatMap { $0.components(separatedBy: " and ") }
                .map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }
        }
        if let d = attrs[PDFDocumentAttribute.creationDateAttribute] as? Date {
            m.date = String(Calendar(identifier: .gregorian).component(.year, from: d))
        }
        let firstText = paras.prefix(60).filter { $0.page < 2 }.map(\.text).joined(separator: " ")
            + " " + ((attrs[PDFDocumentAttribute.subjectAttribute] as? String) ?? "")
        if let d = Resolver.doi(in: firstText) ?? Self.quickDOI(doc) { m.doi = d }
        return m
    }

    /// Quick DOI sniff from the first pages, before the full analysis.
    static func quickDOI(_ doc: PDFDocument) -> String? {
        var text = ((doc.documentAttributes?[PDFDocumentAttribute.subjectAttribute] as? String) ?? "") + " "
        for i in Array(0..<min(2, doc.pageCount)) + [doc.pageCount - 1] { text += (doc.page(at: i)?.string ?? "") + " " }
        return Resolver.doi(in: text)
    }
}

// MARK: - Book assembly

enum PDFBook {
    static func build(_ data: Data, meta extra: PaperMeta, log: @escaping (String) -> Void) async throws -> Book {
        guard let doc = PDFDocument(data: data) else { throw fail("Could not open the PDF.") }
        if doc.isLocked && !doc.unlock(withPassword: "") { throw fail("The PDF is password-protected.") }
        guard doc.pageCount > 0 else { throw fail("The PDF has no pages.") }
        log("Analysing \(doc.pageCount) pages")
        let an = PDFAnalyzer(doc: doc, log: log)
        let paras = an.analyze()
        var meta = an.metaGuess(paras)
        let guessed = meta
        if !extra.title.isEmpty || !extra.authors.isEmpty { meta = extra; meta.merge(guessed) }
        if !meta.doi.isEmpty, let cr = await crossref(meta.doi) {
            log("Found metadata for doi:\(meta.doi)")
            var m = cr; m.merge(meta); meta = m
        }

        if ProcessInfo.processInfo.environment["P2E_DEBUG"] != nil {
            for p in paras {
                let b = p.bbox
                print(String(format: "p%-2d %-9@ L%d sz%4.1f bold%.2f (%3.0f,%3.0f)-(%3.0f,%3.0f) ", p.page + 1, "\(p.kind)" as NSString,
                             p.level, p.size, p.lines.first?.bold ?? 0, b.x0, b.y0, b.x1, b.y1) + String(p.text.prefix(70)))
            }
        }
        // merge paragraphs split across columns/pages
        var merged: [Para] = []
        for p in paras {
            if p.kind == .text, p.text.first?.isLowercase == true,
               let j = merged.lastIndex(where: { ![.figure, .table, .caption].contains($0.kind) }),
               merged[j].kind == .text, merged.count - j <= 4 {
                let q = merged[j]
                if !(q.text.hasSuffix(".") || q.text.hasSuffix("?") || q.text.hasSuffix("!") || q.text.hasSuffix(":")) {
                    merged[j] = Para(q.lines + p.lines, page: q.page)
                    continue
                }
            }
            merged.append(p)
        }
        // figure legends continue after the caption title in smaller type: fold them into the caption
        var folded: [Para] = []
        for p in merged {
            let t = p.text.trimmingCharacters(in: .whitespaces)
            let legendLike = t.matches(#"^\([A-Z][A-Za-z]{0,3}\)\s|^See also Figure|^Scale bars?"#)
            if p.kind == .text, p.size < an.bodySize - 0.4,
               let j = folded.lastIndex(where: { $0.kind == .caption }), folded.count - j <= 8,
               abs(folded[j].page - p.page) <= 1, abs(p.size - folded[j].size) < 0.8,
               legendLike || j == folded.count - 1 {
                folded[j] = Para(folded[j].lines + p.lines, page: folded[j].page, kind: .caption)
                continue
            }
            folded.append(p)
        }
        merged = folded
        // attach captions to the nearest figure/table on the same page
        var capOf: [Int: Int] = [:]
        for (i, p) in merged.enumerated() where p.kind == .figure || p.kind == .table {
            var best: Int?, bd = CGFloat.greatestFiniteMagnitude
            for j in max(0, i - 4)..<min(merged.count, i + 5) {
                let c = merged[j]
                guard c.kind == .caption, c.page == p.page, !capOf.values.contains(j) else { continue }
                let isTab = c.text.matches(#"^(supplementary\s+)?table"#, .caseInsensitive)
                if (p.kind == .table) != isTab { continue }
                let d = min(abs(c.bbox.y0 - p.bbox.y1), abs(p.bbox.y0 - c.bbox.y1))
                if d < bd && d < 120 { best = j; bd = d }
            }
            if let best { capOf[i] = best }
        }
        for (i, p) in merged.enumerated() where (p.kind == .figure || p.kind == .table) && capOf[i] == nil
            && (p.region.map { $0.area > 40000 } ?? false) {
            var best: Int?
            var bd = Int.max
            for j in max(0, i - 12)..<min(merged.count, i + 13) {
                let c = merged[j]
                guard c.kind == .caption, abs(c.page - p.page) == 1, !capOf.values.contains(j) else { continue }
                let isTab = c.text.matches(#"^(supplementary\s+)?table"#, .caseInsensitive)
                if (p.kind == .table) != isTab { continue }
                if abs(j - i) < bd { best = j; bd = abs(j - i) }
            }
            if let best { capOf[i] = best }
        }
        let usedCaps = Set(capOf.values)

        var chapters: [Chapter] = []
        var cur: (title: String, id: String, html: String, headings: [Heading])?
        var images: [BookImage] = []
        var front = ""
        var autoID = 0
        func nextID() -> String { autoID += 1; return "s\(autoID)" }
        func close() {
            if let c = cur { chapters.append(Chapter(title: c.title, html: "<section>\(c.html)</section>", headings: c.headings)) }
            cur = nil
        }
        func open(_ title: String) {
            close()
            let id = nextID()
            cur = (title, id, "<h1 id=\"\(id)\">\(esc(title))</h1>", [Heading(level: 1, id: id, title: title)])
        }
        func append(_ html: String) {
            if cur == nil { open("Main text") }
            cur!.html += html
        }
        func addImage(_ img: BookImage?) -> String? {
            guard let img else { return nil }
            images.append(img)
            return "../images/\(img.fileName)"
        }

        let hasAbstractHeading = merged.prefix(30).contains { $0.kind == .heading && $0.text.lowercased().hasPrefix("abstract") }
        let hasIntro = merged.contains { $0.kind == .heading && $0.text.matches("(?i)introduction|background") }
        var abstractDone = false, introDone = false
        var lastKind: Para.Kind?
        let missingTitle = meta.title.isEmpty
        for (i, p) in merged.enumerated() {
            switch p.kind {
            case .figure, .table:
                let cap = capOf[i].map { an.styled(merged[$0]) } ?? ""
                guard let src = addImage(p.asset) else { break }
                if p.kind == .table {
                    append("<div class=\"table-wrap\">\(cap.isEmpty ? "" : "<p class=\"caption\">\(cap)</p>")<img src=\"\(src)\" alt=\"Table\"/></div>")
                } else {
                    append("<figure><img src=\"\(src)\" alt=\"Figure\"/>\(cap.isEmpty ? "" : "<figcaption>\(cap)</figcaption>")</figure>")
                }
            case _ where usedCaps.contains(i):
                break
            case .title:
                if missingTitle { meta.title = p.text }
            case .heading:
                let t = p.text.trimmingCharacters(in: .whitespaces)
                if Self.knownSections(t) == "abstract" { abstractDone = true }
                if p.level <= 1 { open(t) } else {
                    let id = nextID()
                    let h = min(p.level, 3)
                    append("<h\(h) id=\"\(id)\">\(esc(t))</h\(h)>")
                    if h == 2 { cur!.headings.append(Heading(level: 2, id: id, title: t)) }
                }
            case .front:
                if !meta.title.isEmpty && similar(p.text, meta.title) { break }
                front += "<p>\(an.styled(p))</p>"
            case .abstract:
                if !abstractDone { open("Abstract"); abstractDone = true }
                let s = an.styled(p).replacingOccurrences(of: #"^(<strong>)?Abstract(</strong>)?[.:]?(</strong>)?\s*"#, with: "",
                                                          options: [.regularExpression, .caseInsensitive])
                append("<p>\(s)</p>")
            case .equation:
                if let src = addImage(p.asset) {
                    let wem = (p.bbox.w + 8) / max(an.bodySize, 6) * 0.95
                    append("<div class=\"equation\"><img class=\"equation\" src=\"\(src)\" alt=\"equation\" style=\"width: \(String(format: "%.1f", wem))em\"/></div>")
                }
            case .caption:
                append("<p class=\"caption\">\(an.styled(p))</p>")
            default:
                if abstractDone && !introDone && lastKind == .abstract && !hasIntro { open("Introduction") }
                introDone = introDone || lastKind == .heading
                append("<p>\(an.styled(p))</p>")
            }
            if p.kind != .front { lastKind = p.kind }
        }
        close()
        _ = hasAbstractHeading
        if !front.isEmpty && meta.affiliationsHTML.isEmpty { meta.affiliationsHTML = front }
        guard !chapters.isEmpty else { throw fail("Could not find any text in the PDF (is it a scanned image?).") }
        return Book(meta: meta, chapters: chapters, images: images, sourceLabel: "PDF (layout analysis)",
                    notes: ["Converted from the PDF without AI: equations and tables are kept as images."])
    }

    static func knownSections(_ t: String) -> String? {
        t.lowercased().hasPrefix("abstract") ? "abstract" : nil
    }

    static func similar(_ a: String, _ b: String) -> Bool {
        let na = a.lowercased().replacingOccurrences(of: #"\W+"#, with: "", options: .regularExpression)
        let nb = b.lowercased().replacingOccurrences(of: #"\W+"#, with: "", options: .regularExpression)
        return !na.isEmpty && !nb.isEmpty && (na == nb || (na.count > 20 && (na.contains(nb) || nb.contains(na))))
    }

    /// Title, authors, journal and date from Crossref.
    static func crossref(_ doi: String) async -> PaperMeta? {
        guard let enc = doi.addingPercentEncoding(withAllowedCharacters: .urlPathAllowed),
              let url = URL(string: "https://api.crossref.org/works/\(enc)"),
              let j = try? await Net.json(url), let m = j["message"] as? [String: Any] else { return nil }
        var meta = PaperMeta()
        meta.doi = doi
        meta.title = ((m["title"] as? [String])?.first ?? "").replacingOccurrences(of: "<[^>]+>", with: "", options: .regularExpression)
        meta.authors = ((m["author"] as? [[String: Any]]) ?? []).compactMap { a in
            let s = [a["given"] as? String, a["family"] as? String].compactMap { $0 }.joined(separator: " ")
            return s.isEmpty ? a["name"] as? String : s
        }
        meta.journal = (m["container-title"] as? [String])?.first ?? ""
        if let parts = ((m["issued"] as? [String: Any])?["date-parts"] as? [[Int]])?.first, let y = parts.first {
            meta.date = parts.count >= 3 ? String(format: "%04d-%02d-%02d", y, parts[1], parts[2]) : String(y)
        }
        if let lic = (m["license"] as? [[String: Any]])?.compactMap({ $0["URL"] as? String })
            .first(where: { $0.contains("creativecommons") }) { meta.license = lic }
        return meta
    }
}
