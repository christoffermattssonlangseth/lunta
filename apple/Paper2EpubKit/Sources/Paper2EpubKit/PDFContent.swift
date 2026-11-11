import Foundation
import CoreGraphics

/// Rectangle in page coordinates with the origin at the top-left (like PyMuPDF), in points.
struct R: Equatable {
    var x0: CGFloat, y0: CGFloat, x1: CGFloat, y1: CGFloat
    var w: CGFloat { x1 - x0 }
    var h: CGFloat { y1 - y0 }
    var area: CGFloat { max(0, w) * max(0, h) }
    var cx: CGFloat { (x0 + x1) / 2 }

    func union(_ o: R) -> R { R(x0: min(x0, o.x0), y0: min(y0, o.y0), x1: max(x1, o.x1), y1: max(y1, o.y1)) }
    func intersects(_ o: R, pad: CGFloat = 0) -> Bool {
        !(x1 + pad < o.x0 || o.x1 + pad < x0 || y1 + pad < o.y0 || o.y1 + pad < y0)
    }
    func contains(_ i: R, tol: CGFloat = 2) -> Bool {
        i.x0 >= x0 - tol && i.y0 >= y0 - tol && i.x1 <= x1 + tol && i.y1 <= y1 + tol
    }
    func intersection(_ o: R) -> R { R(x0: max(x0, o.x0), y0: max(y0, o.y0), x1: min(x1, o.x1), y1: min(y1, o.y1)) }
    static func union(_ rs: [R]) -> R { rs.dropFirst().reduce(rs[0]) { $0.union($1) } }
}

/// A run of text drawn with one font, as found in the page's content stream.
struct TextRun {
    var x0: CGFloat, x1: CGFloat     // horizontal extent (top-left page coords)
    var baseline: CGFloat            // y of the baseline (top-left page coords)
    var size: CGFloat
    var bold: Bool, italic: Bool, math: Bool
}

struct PageGraphics {
    var images: [R] = []
    var drawings: [R] = []
    var runs: [TextRun] = []
    /// Word gaps the content stream makes by moving instead of drawing a space: (x, baseline).
    var gaps: [CGPoint] = []
}

/// Walks a page's content stream with CGPDFScanner to find image placements, vector drawings and
/// the font (bold/italic/math) of each text run. PDFKit's text API does not expose real font names.
final class PDFContentScanner {
    private struct GState {
        var ctm = CGAffineTransform.identity
    }
    private struct TextState {
        var tm = CGAffineTransform.identity, tlm = CGAffineTransform.identity
        var tc: CGFloat = 0, tw: CGFloat = 0, th: CGFloat = 1, tl: CGFloat = 0, size: CGFloat = 0
        var font: FontInfo?
    }
    final class FontInfo {
        var bold = false, italic = false, math = false
        var twoByte = false
        var firstChar = 0
        var widths: [CGFloat] = []
        var cidWidths: [Int: CGFloat] = [:]
        var defaultWidth: CGFloat = 500
        var missingWidth: CGFloat = 500
        func width(_ code: Int) -> CGFloat {
            if twoByte { return cidWidths[code] ?? defaultWidth }
            let i = code - firstChar
            return (i >= 0 && i < widths.count && widths[i] > 0) ? widths[i] : missingWidth
        }
    }

    static let boldRE = try! NSRegularExpression(pattern: "(?i:bold|black|heavy|semibold|demi|-bd\\b)|-(H|B|Bd|Hv|Hvy|Blk|SB|Sb|BdIt|BI|HI|Bol)$|[a-z](Bd|Blk|Hv)$", options: [])
    static let italicRE = try! NSRegularExpression(pattern: "(?i:ital|oblique|slant)|-(I|It|MI|LI|BI|HI|BdIt|SBIt|Ita)$|[a-z]It$", options: [])
    static let mathRE = try! NSRegularExpression(
        pattern: "CMMI|CMSY|CMEX|MSAM|MSBM|Math|Symbol|STIX|rsfs|EUFM|CMBSY|LMMath|esint|MTMI|MTSY|txsy|txmi|pxmi|Euclid",
        options: .caseInsensitive)

    private var gstack: [GState] = [GState()]
    private var ts = TextState()
    private var path: R?
    private var resources: [CGPDFDictionaryRef] = []
    private var fontCache: [String: FontInfo] = [:]   // keyed by resource name; reset when resources change
    private var streams: [CGPDFContentStreamRef] = []
    private var depth = 0
    private var out = PageGraphics()
    private let pageHeight: CGFloat
    private let origin: CGPoint

    /// `box` is the page's media box; output rects are relative to its top-left corner.
    init(box: CGRect) {
        pageHeight = box.height
        origin = box.origin
    }

    func scan(_ page: CGPDFPage) -> PageGraphics {
        guard let dict = page.dictionary else { return out }
        var res: CGPDFDictionaryRef?
        if CGPDFDictionaryGetDictionary(dict, "Resources", &res), let res { resources = [res] }
        let stream = CGPDFContentStreamCreateWithPage(page)
        streams.append(stream)
        run(stream)
        streams.removeLast()
        CGPDFContentStreamRelease(stream)
        return out
    }

    private func run(_ stream: CGPDFContentStreamRef) {
        let table = Self.operatorTable
        let scanner = CGPDFScannerCreate(stream, table, Unmanaged.passUnretained(self).toOpaque())
        CGPDFScannerScan(scanner)
        CGPDFScannerRelease(scanner)
    }

    // MARK: geometry helpers

    private var ctm: CGAffineTransform {
        get { gstack[gstack.count - 1].ctm }
        set { gstack[gstack.count - 1].ctm = newValue }
    }

    /// PDF user space → top-left page coordinates.
    private func toPage(_ p: CGPoint) -> CGPoint {
        let q = p.applying(ctm)
        return CGPoint(x: q.x - origin.x, y: pageHeight - (q.y - origin.y))
    }

    private func addPathPoint(_ x: CGFloat, _ y: CGFloat) {
        let p = toPage(CGPoint(x: x, y: y))
        let r = R(x0: p.x, y0: p.y, x1: p.x, y1: p.y)
        path = path.map { $0.union(r) } ?? r
    }

    private func paint() {
        if let p = path { out.drawings.append(p) }
        path = nil
    }

    // MARK: fonts

    private func font(named name: String) -> FontInfo? {
        for res in resources.reversed() {
            var fonts: CGPDFDictionaryRef?
            guard CGPDFDictionaryGetDictionary(res, "Font", &fonts), let fonts else { continue }
            var f: CGPDFDictionaryRef?
            guard CGPDFDictionaryGetDictionary(fonts, name, &f), let f else { continue }
            let key = "\(resources.count)|\(name)"
            if let c = fontCache[key] { return c }
            let info = Self.fontInfo(f)
            fontCache[key] = info
            return info
        }
        return nil
    }

    private static func name(_ d: CGPDFDictionaryRef, _ key: String) -> String? {
        var n: UnsafePointer<CChar>?
        guard CGPDFDictionaryGetName(d, key, &n), let n else { return nil }
        return String(cString: n)
    }

    private static func number(_ obj: CGPDFObjectRef) -> CGFloat? {
        var r: CGPDFReal = 0
        if CGPDFObjectGetValue(obj, .real, &r) { return r }
        var i: CGPDFInteger = 0
        if CGPDFObjectGetValue(obj, .integer, &i) { return CGFloat(i) }
        return nil
    }

    private static func fontInfo(_ f: CGPDFDictionaryRef) -> FontInfo {
        let info = FontInfo()
        let base = name(f, "BaseFont") ?? ""
        let clean = base.contains("+") ? String(base.split(separator: "+", maxSplits: 1).last ?? "") : base
        let ns = clean as NSString
        let full = NSRange(location: 0, length: ns.length)
        info.bold = boldRE.firstMatch(in: clean, range: full) != nil
        info.italic = italicRE.firstMatch(in: clean, range: full) != nil
        info.math = mathRE.firstMatch(in: clean, range: full) != nil
        var desc: CGPDFDictionaryRef?
        var descFont = f
        if name(f, "Subtype") == "Type0" {
            info.twoByte = true
            var arr: CGPDFArrayRef?
            var d0: CGPDFDictionaryRef?
            if CGPDFDictionaryGetArray(f, "DescendantFonts", &arr), let arr, CGPDFArrayGetDictionary(arr, 0, &d0), let d0 {
                descFont = d0
                var dw: CGPDFInteger = 0
                if CGPDFDictionaryGetInteger(d0, "DW", &dw) { info.defaultWidth = CGFloat(dw) }
                var w: CGPDFArrayRef?
                if CGPDFDictionaryGetArray(d0, "W", &w), let w { parseW(w, into: info) }
            }
        } else {
            var fc: CGPDFInteger = 0
            if CGPDFDictionaryGetInteger(f, "FirstChar", &fc) { info.firstChar = fc }
            var w: CGPDFArrayRef?
            if CGPDFDictionaryGetArray(f, "Widths", &w), let w {
                for i in 0..<CGPDFArrayGetCount(w) {
                    var o: CGPDFObjectRef?
                    if CGPDFArrayGetObject(w, i, &o), let o { info.widths.append(number(o) ?? 0) } else { info.widths.append(0) }
                }
            }
        }
        if CGPDFDictionaryGetDictionary(descFont, "FontDescriptor", &desc), let desc {
            var o: CGPDFObjectRef?
            if CGPDFDictionaryGetObject(desc, "MissingWidth", &o), let o, let v = number(o), v > 0 { info.missingWidth = v }
            if CGPDFDictionaryGetObject(desc, "FontWeight", &o), let o, let v = number(o), v >= 600 { info.bold = true }
            if CGPDFDictionaryGetObject(desc, "ItalicAngle", &o), let o, let v = number(o), abs(v) > 1 { info.italic = true }
            var flags: CGPDFInteger = 0
            if CGPDFDictionaryGetInteger(desc, "Flags", &flags), flags & (1 << 18) != 0 { info.bold = true }
        }
        return info
    }

    /// CID font /W array: [c [w1 w2 …]] or [cFirst cLast w].
    private static func parseW(_ w: CGPDFArrayRef, into info: FontInfo) {
        let n = CGPDFArrayGetCount(w)
        var i = 0
        while i < n {
            var o: CGPDFObjectRef?
            guard CGPDFArrayGetObject(w, i, &o), let o, let c = number(o) else { break }
            var arr: CGPDFArrayRef?
            if CGPDFArrayGetArray(w, i + 1, &arr), let arr {
                for k in 0..<CGPDFArrayGetCount(arr) {
                    var wo: CGPDFObjectRef?
                    if CGPDFArrayGetObject(arr, k, &wo), let wo, let v = number(wo) { info.cidWidths[Int(c) + k] = v }
                }
                i += 2
            } else {
                var o2: CGPDFObjectRef?, o3: CGPDFObjectRef?
                guard CGPDFArrayGetObject(w, i + 1, &o2), let o2, let last = number(o2),
                      CGPDFArrayGetObject(w, i + 2, &o3), let o3, let v = number(o3) else { break }
                if last - c < 70000 { for code in Int(c)...Int(last) { info.cidWidths[code] = v } }
                i += 3
            }
        }
    }

    // MARK: text showing

    private func show(_ s: CGPDFStringRef) {
        guard let font = ts.font, let bytes = CGPDFStringGetBytePtr(s) else { return }
        let len = CGPDFStringGetLength(s)
        let trm = ts.tm.concatenating(ctm)
        let start = CGPoint.zero.applying(trm)
        var adv: CGFloat = 0
        var i = 0
        while i < len {
            let code: Int
            if font.twoByte, i + 1 < len { code = Int(bytes[i]) << 8 | Int(bytes[i + 1]); i += 2 }
            else { code = Int(bytes[i]); i += 1 }
            var tx = font.width(code) / 1000 * ts.size + ts.tc
            if !font.twoByte && code == 32 { tx += ts.tw }
            adv += tx * ts.th
        }
        let endP = CGPoint(x: adv, y: 0).applying(trm)
        ts.tm = CGAffineTransform(translationX: adv, y: 0).concatenating(ts.tm)
        let effSize = abs(ts.size) * sqrt(abs(trm.a * trm.d - trm.b * trm.c))
        guard abs(trm.b) < 0.1 * abs(trm.a) + 0.001 else { return }   // rotated text: ignore
        let a = CGPoint(x: start.x - origin.x, y: pageHeight - (start.y - origin.y))
        let b = CGPoint(x: endP.x - origin.x, y: pageHeight - (endP.y - origin.y))
        out.runs.append(TextRun(x0: min(a.x, b.x), x1: max(a.x, b.x), baseline: a.y, size: effSize,
                                bold: font.bold, italic: font.italic, math: font.math))
        // a jump to the right on the same baseline since the last text = a word space nobody drew
        if let e = lastEnd, abs(e.y - a.y) < 0.3 * max(effSize, 2), a.x - e.x > 0.15 * effSize, a.x - e.x < 3 * effSize {
            out.gaps.append(CGPoint(x: (a.x + e.x) / 2, y: a.y))
        }
        lastEnd = CGPoint(x: max(a.x, b.x), y: a.y)
    }

    private var lastEnd: CGPoint?

    private func adjust(_ n: CGFloat) {   // TJ number: move left by n/1000 text-space units
        let tx = -n / 1000 * ts.size * ts.th
        ts.tm = CGAffineTransform(translationX: tx, y: 0).concatenating(ts.tm)
    }

    private func nextLine(_ tx: CGFloat, _ ty: CGFloat) {
        ts.tlm = CGAffineTransform(translationX: tx, y: ty).concatenating(ts.tlm)
        ts.tm = ts.tlm
    }

    // MARK: XObjects

    private func doXObject(_ name: String) {
        for res in resources.reversed() {
            var xobjs: CGPDFDictionaryRef?
            guard CGPDFDictionaryGetDictionary(res, "XObject", &xobjs), let xobjs else { continue }
            var stream: CGPDFStreamRef?
            guard CGPDFDictionaryGetStream(xobjs, name, &stream), let stream,
                  let sd = CGPDFStreamGetDictionary(stream) else { continue }
            let sub = Self.name(sd, "Subtype")
            if sub == "Image" {
                let pts = [CGPoint(x: 0, y: 0), CGPoint(x: 1, y: 0), CGPoint(x: 0, y: 1), CGPoint(x: 1, y: 1)].map(toPage)
                let r = R(x0: pts.map(\.x).min()!, y0: pts.map(\.y).min()!, x1: pts.map(\.x).max()!, y1: pts.map(\.y).max()!)
                out.images.append(r)
            } else if sub == "Form", depth < 6 {
                var m = CGAffineTransform.identity
                var arr: CGPDFArrayRef?
                if CGPDFDictionaryGetArray(sd, "Matrix", &arr), let arr, CGPDFArrayGetCount(arr) == 6 {
                    var v: [CGFloat] = []
                    for k in 0..<6 {
                        var o: CGPDFObjectRef?
                        if CGPDFArrayGetObject(arr, k, &o), let o { v.append(Self.number(o) ?? 0) }
                    }
                    if v.count == 6 { m = CGAffineTransform(a: v[0], b: v[1], c: v[2], d: v[3], tx: v[4], ty: v[5]) }
                }
                var formRes: CGPDFDictionaryRef?
                let hasRes = CGPDFDictionaryGetDictionary(sd, "Resources", &formRes) && formRes != nil
                gstack.append(GState(ctm: m.concatenating(ctm)))
                if hasRes { resources.append(formRes!); fontCache = [:] }
                let savedTS = ts
                depth += 1
                if let parent = streams.last, let r = formRes ?? resources.last {
                    let cs = CGPDFContentStreamCreateWithStream(stream, r, parent)
                    streams.append(cs)
                    run(cs)
                    streams.removeLast()
                    CGPDFContentStreamRelease(cs)
                }
                depth -= 1
                ts = savedTS
                if hasRes { resources.removeLast(); fontCache = [:] }
                gstack.removeLast()
            }
            return
        }
    }

    // MARK: operator table

    private static func me(_ info: UnsafeMutableRawPointer?) -> PDFContentScanner {
        Unmanaged<PDFContentScanner>.fromOpaque(info!).takeUnretainedValue()
    }

    private static func pop(_ s: CGPDFScannerRef, _ n: Int) -> [CGFloat]? {
        var v = [CGFloat](repeating: 0, count: n)
        for i in (0..<n).reversed() {
            var r: CGPDFReal = 0
            guard CGPDFScannerPopNumber(s, &r) else { return nil }
            v[i] = r
        }
        return v
    }

    static let operatorTable: CGPDFOperatorTableRef = {
        let t = CGPDFOperatorTableCreate()!
        func add(_ op: String, _ cb: @escaping CGPDFOperatorCallback) { CGPDFOperatorTableSetCallback(t, op, cb) }
        add("q") { _, i in let m = me(i); m.gstack.append(m.gstack.last!) }
        add("Q") { _, i in let m = me(i); if m.gstack.count > 1 { m.gstack.removeLast() } }
        add("cm") { s, i in
            let m = me(i)
            if let v = pop(s, 6) { m.ctm = CGAffineTransform(a: v[0], b: v[1], c: v[2], d: v[3], tx: v[4], ty: v[5]).concatenating(m.ctm) }
        }
        // paths
        add("m") { s, i in if let v = pop(s, 2) { me(i).addPathPoint(v[0], v[1]) } }
        add("l") { s, i in if let v = pop(s, 2) { me(i).addPathPoint(v[0], v[1]) } }
        add("c") { s, i in if let v = pop(s, 6) { let m = me(i); m.addPathPoint(v[0], v[1]); m.addPathPoint(v[2], v[3]); m.addPathPoint(v[4], v[5]) } }
        add("v") { s, i in if let v = pop(s, 4) { let m = me(i); m.addPathPoint(v[0], v[1]); m.addPathPoint(v[2], v[3]) } }
        add("y") { s, i in if let v = pop(s, 4) { let m = me(i); m.addPathPoint(v[0], v[1]); m.addPathPoint(v[2], v[3]) } }
        add("re") { s, i in
            if let v = pop(s, 4) { let m = me(i); m.addPathPoint(v[0], v[1]); m.addPathPoint(v[0] + v[2], v[1] + v[3]) }
        }
        for op in ["f", "F", "f*", "S", "s", "B", "B*", "b", "b*"] { add(op) { _, i in me(i).paint() } }
        add("n") { _, i in me(i).path = nil }
        // text
        add("BT") { _, i in let m = me(i); m.ts.tm = .identity; m.ts.tlm = .identity }
        add("Tf") { s, i in
            let m = me(i)
            var r: CGPDFReal = 0
            var n: UnsafePointer<CChar>?
            guard CGPDFScannerPopNumber(s, &r), CGPDFScannerPopName(s, &n), let n else { return }
            m.ts.size = r
            m.ts.font = m.font(named: String(cString: n))
        }
        add("Tc") { s, i in if let v = pop(s, 1) { me(i).ts.tc = v[0] } }
        add("Tw") { s, i in if let v = pop(s, 1) { me(i).ts.tw = v[0] } }
        add("Tz") { s, i in if let v = pop(s, 1) { me(i).ts.th = v[0] / 100 } }
        add("TL") { s, i in if let v = pop(s, 1) { me(i).ts.tl = v[0] } }
        add("Td") { s, i in if let v = pop(s, 2) { me(i).nextLine(v[0], v[1]) } }
        add("TD") { s, i in if let v = pop(s, 2) { let m = me(i); m.ts.tl = -v[1]; m.nextLine(v[0], v[1]) } }
        add("Tm") { s, i in
            if let v = pop(s, 6) {
                let m = me(i)
                m.ts.tlm = CGAffineTransform(a: v[0], b: v[1], c: v[2], d: v[3], tx: v[4], ty: v[5])
                m.ts.tm = m.ts.tlm
            }
        }
        add("T*") { _, i in let m = me(i); m.nextLine(0, -m.ts.tl) }
        add("Tj") { s, i in var str: CGPDFStringRef?; if CGPDFScannerPopString(s, &str), let str { me(i).show(str) } }
        add("'") { s, i in
            let m = me(i); m.nextLine(0, -m.ts.tl)
            var str: CGPDFStringRef?; if CGPDFScannerPopString(s, &str), let str { m.show(str) }
        }
        add("\"") { s, i in
            let m = me(i)
            var str: CGPDFStringRef?
            guard CGPDFScannerPopString(s, &str), let str, let v = pop(s, 2) else { return }
            m.ts.tw = v[0]; m.ts.tc = v[1]; m.nextLine(0, -m.ts.tl); m.show(str)
        }
        add("TJ") { s, i in
            let m = me(i)
            var arr: CGPDFArrayRef?
            guard CGPDFScannerPopArray(s, &arr), let arr else { return }
            for k in 0..<CGPDFArrayGetCount(arr) {
                var o: CGPDFObjectRef?
                guard CGPDFArrayGetObject(arr, k, &o), let o else { continue }
                var str: CGPDFStringRef?
                if CGPDFObjectGetValue(o, .string, &str), let str { m.show(str) }
                else if let n = number(o) { m.adjust(n) }
            }
        }
        add("Do") { s, i in
            var n: UnsafePointer<CChar>?
            if CGPDFScannerPopName(s, &n), let n { me(i).doXObject(String(cString: n)) }
        }
        return t
    }()
}
