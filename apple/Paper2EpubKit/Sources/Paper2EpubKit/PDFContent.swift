import Foundation
import CoreGraphics
import CoreText

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
    /// a Computer Modern roman font (CMR10, CMBX10): math in a book or paper whose text is set in
    /// another font (Latin Modern, Times), the text font where everything is set in Computer Modern
    var cmRoman = false
    /// a monospaced font (LMMono, Courier, Inconsolata): code
    var mono = false
    /// drawn outside the bounding box of the form it is in, so not shown (see GState.clip)
    var hidden = false
    /// the bounding box of the form it is in (page coordinates): the run's letters outside it are not shown
    var clip: R?
    /// where each glyph starts (x), to tell its letters from others drawn in about the same place
    var starts: [CGFloat] = []
}

struct PageGraphics {
    var images: [R] = []
    var drawings: [R] = []
    var runs: [TextRun] = []
    /// Word gaps the content stream makes by moving instead of drawing a space: (x, baseline).
    var gaps: [CGPoint] = []
    /// Places where the text goes on, with no space drawn, right where the last text ended ("foundati"
    /// -15 "on" in a TJ array), whatever PDFKit makes of them, and space glyphs that take no room
    /// (put in URLs to let them break: "https:// creat iveco mmons"): (x, baseline). A space glyph that
    /// word spacing takes most of the room from is still a space (0.05 em in tightly justified lines).
    var joints: [CGPoint] = []
    /// Ligature glyphs ("fi", "ffl") of fonts that name them in their encoding: PDFKit leaves out the
    /// ones a font puts at codes below 32 ("detoxi cation", "to lter"), even with a ToUnicode map.
    var ligatures: [Ligature] = []
}

/// A ligature glyph as drawn: its horizontal extent and baseline (top-left page coords) and letters.
struct Ligature {
    var x0: CGFloat, x1: CGFloat, baseline: CGFloat, size: CGFloat
    var text: String
    /// at a code below 32, which PDFKit leaves out (above, PDFKit has a character of its own there)
    var lowCode = false
    /// a symbol font's glyph (see SymbolFonts): its text replaces what PDFKit made of it, one of `reads`
    /// (the character of its name, of its code)
    var symbol = false
    var reads: [String] = []
    /// for a font with its own encoding: what it is by what PDFKit read (`text` is empty)
    var byRead: [String: String]?
}

/// Walks a page's content stream with CGPDFScanner to find image placements, vector drawings and
/// the font (bold/italic/math) of each text run. PDFKit's text API does not expose real font names.
final class PDFContentScanner {
    private struct GState {
        var ctm = CGAffineTransform.identity
        /// Text state parameters in force at `q` (Tc, Tw, Tz, TL, Tf are part of the graphics state and
        /// come back at `Q`; the text matrices are not).
        var savedText: TextState?
        /// The bounding boxes of the forms being drawn, in page coordinates: nothing outside shows. A
        /// figure included as a whole page of a manuscript ("cells-2381855-g001.pdf", with the
        /// manuscript's text around the figure) would otherwise add a second, shifted copy of the text.
        var clip: R?
    }
    private struct TextState {
        var tm = CGAffineTransform.identity, tlm = CGAffineTransform.identity
        var tc: CGFloat = 0, tw: CGFloat = 0, th: CGFloat = 1, tl: CGFloat = 0, size: CGFloat = 0
        var font: FontInfo?
    }
    final class FontInfo {
        var bold = false, italic = false, math = false, cmRoman = false, mono = false
        var twoByte = false
        var firstChar = 0
        var widths: [CGFloat] = []
        var cidWidths: [Int: CGFloat] = [:]
        var defaultWidth: CGFloat = 500
        var missingWidth: CGFloat = 500
        /// Type 3 fonts: their FontMatrix's scale × 1000 (glyph widths are in its units, not 1/1000 em)
        var widthScale: CGFloat = 1, sizeScale: CGFloat = 1
        /// codes the encoding's Differences give a ligature's glyph name ("/uniFB01", "/fi", "/f_f_l")
        var ligatures: [Int: String] = [:]
        /// what the glyphs of a symbol font without a ToUnicode map show (see SymbolFonts), by code
        var symbols: [Int: (text: String, reads: [String])] = [:]
        /// a symbol font with its own encoding: what its glyphs are by what PDFKit reads for them
        var symbolsByRead: (reads: [String], byRead: [String: String])?
        func width(_ code: Int) -> CGFloat {
            if twoByte { return cidWidths[code] ?? defaultWidth }
            let i = code - firstChar
            return (i >= 0 && i < widths.count && widths[i] > 0) ? widths[i] : missingWidth
        }
    }

    static let boldRE = try! NSRegularExpression(pattern: "(?i:bold|black|heavy|semibold|demi|-bd\\b)|-(H|B|Bd|Hv|Hvy|Blk|SB|Sb|BdIt|BI|HI|Bol)$|[a-z](Bd|Blk|Hv)$", options: [])
    static let cmapSuffix = #"-(Identity|Uni[A-Za-z0-9]+|[A-Za-z0-9]+-UCS2|UCS2)-[HV]$"#
    static let italicRE = try! NSRegularExpression(pattern: "(?i:ital|oblique|slant)|-(I|It|MI|LI|BI|HI|BdIt|SBIt|Ita)$|[a-z]It$", options: [])
    /// Math fonts. STIX counts, except STIXTwoText: the body text of Wiley's journals (their math is
    /// STIXTwoMath).
    static let mathRE = try! NSRegularExpression(
        pattern: "CMMI|CMSY|CMEX|MSAM|MSBM|Math|Symbol|STIX(?!Two-?Text)|rsfs|EUFM|CMBSY|LMMath|esint|MTMI|MTSY|txsy|txmi|pxmi|Euclid",
        options: .caseInsensitive)

    static let cmRomanRE = try! NSRegularExpression(pattern: "^(CMR|CMBX|CMSL|CMB)\\d", options: [])
    static let monoRE = try! NSRegularExpression(pattern: "(?i)mono|courier|^CMTT|^SFTT|consol|menlo|inconsolata|typewriter|SourceCodePro|FiraCode|^LMTT", options: [])

    private var gstack: [GState] = [GState()]
    private var ts = TextState()
    private var path: R?
    private var resources: [CGPDFDictionaryRef] = []
    private var fontCache: [String: FontInfo] = [:]   // keyed by resource name; reset when resources change
    private var streams: [CGPDFContentStreamRef] = []
    private var depth = 0
    private var out = PageGraphics()
    private let pageHeight: CGFloat
    private let pageArea: CGFloat
    private let origin: CGPoint

    /// `box` is the page's media box; output rects are relative to its top-left corner.
    init(box: CGRect) {
        pageHeight = box.height
        pageArea = box.width * box.height
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
        // text in a form that an image drawn after it in the form covers is not shown either (the
        // manuscript page's text under the figure it was included for); not under an image of the
        // whole page, which may be a scan drawn over its own text layer
        for (img, at) in formImages where img.w * img.h < 0.8 * pageArea {
            for k in out.runs.indices where k < at && out.runs[k].clip != nil && !out.runs[k].hidden {
                let r = out.runs[k]
                let c = R(x0: (r.x0 + r.x1) / 2, y0: r.baseline - 0.3 * r.size, x1: (r.x0 + r.x1) / 2, y1: r.baseline - 0.3 * r.size)
                if img.contains(c, tol: 0) { out.runs[k].hidden = true }
            }
        }
        return out
    }

    /// images drawn in forms, with the number of text runs drawn before them
    private var formImages: [(R, Int)] = []

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
        if var p = path {
            if let c = gstack.last?.clip { p = p.intersection(c) }
            if p.w >= 0, p.h >= 0 { out.drawings.append(p) }
        }
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

    private static var standardCache: [String: [CGFloat]] = [:]
    private static let standardLock = NSLock()

    /// Glyph widths (per 1000 em, codes 0–255 in WinAnsi) of a standard font named without its
    /// widths ("Helvetica", "Times-Bold", "Courier"), from the system's copy of the font.
    static func standardWidths(_ base: String) -> [CGFloat]? {
        let n = base.replacingOccurrences(of: ",", with: "-")
        let family: String, style: String
        if n.hasPrefix("Helvetica") || n.hasPrefix("Arial") { family = "Helvetica" }
        else if n.hasPrefix("Times") { family = "Times" }
        else if n.hasPrefix("Courier") { family = "Courier" }
        else { return nil }
        let bold = n.range(of: "Bold", options: .caseInsensitive) != nil
        let italic = n.range(of: "Italic|Oblique", options: [.regularExpression, .caseInsensitive]) != nil
        switch family {
        case "Times": style = bold && italic ? "-BoldItalic" : bold ? "-Bold" : italic ? "-Italic" : "-Roman"
        default: style = bold && italic ? "-BoldOblique" : bold ? "-Bold" : italic ? "-Oblique" : ""
        }
        let ps = family + style
        standardLock.lock(); defer { standardLock.unlock() }
        if let c = standardCache[ps] { return c }
        var widths = [CGFloat](repeating: 0, count: 256)
        if family == "Courier" {
            widths = [CGFloat](repeating: 600, count: 256)
        } else {
            let font = CTFontCreateWithName(ps as CFString, 1000, nil)
            guard (CTFontCopyPostScriptName(font) as String) == ps else { return nil }   // not on this system
            for code in 32..<256 {
                guard let ch = String(bytes: [UInt8(code)], encoding: .windowsCP1252) else { continue }
                var chars = Array(ch.utf16)
                var glyphs = [CGGlyph](repeating: 0, count: chars.count)
                guard chars.count == 1, CTFontGetGlyphsForCharacters(font, &chars, &glyphs, 1) else { continue }
                var adv = CGSize.zero
                CTFontGetAdvancesForGlyphs(font, .horizontal, &glyphs, &adv, 1)
                widths[code] = adv.width
            }
        }
        standardCache[ps] = widths
        return widths
    }

    private static func fontInfo(_ f: CGPDFDictionaryRef) -> FontInfo {
        let info = FontInfo()
        let base = name(f, "BaseFont") ?? ""
        var clean = base.contains("+") ? String(base.split(separator: "+", maxSplits: 1).last ?? "") : base
        // a CID font's name may end in its CMap ("LMRoman10-Regular-Identity-H", XeLaTeX and LuaLaTeX):
        // the "-H" (horizontal) is no "Heavy"
        clean = clean.replacingOccurrences(of: Self.cmapSuffix, with: "", options: .regularExpression)
        let ns = clean as NSString
        let full = NSRange(location: 0, length: ns.length)
        info.bold = boldRE.firstMatch(in: clean, range: full) != nil
        info.italic = italicRE.firstMatch(in: clean, range: full) != nil
        info.math = mathRE.firstMatch(in: clean, range: full) != nil
        info.cmRoman = cmRomanRE.firstMatch(in: clean, range: full) != nil
        info.mono = monoRE.firstMatch(in: clean, range: full) != nil
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
            var enc: CGPDFDictionaryRef?, diffs: CGPDFArrayRef?
            var glyphNames: [Int: String] = [:]
            if CGPDFDictionaryGetDictionary(f, "Encoding", &enc), let enc, CGPDFDictionaryGetArray(enc, "Differences", &diffs), let diffs {
                var code = 0
                for i in 0..<CGPDFArrayGetCount(diffs) {
                    var o: CGPDFObjectRef?
                    guard CGPDFArrayGetObject(diffs, i, &o), let o else { continue }
                    if let v = number(o) { code = Int(v); continue }
                    var n: UnsafePointer<CChar>?
                    if CGPDFObjectGetValue(o, .name, &n), let n {
                        let glyph = String(cString: n)
                        if let lig = ligatureNames[glyph] { info.ligatures[code] = lig }
                        glyphNames[code] = glyph
                        code += 1
                    }
                }
            }
            // a symbol font that names its glyphs like letters, with no ToUnicode map to say otherwise: a
            // map that gives a code only what its name or the code itself stands for (Advent's
            // "<aa> <00AA>" for the © it names "ordfeminine") says nothing more
            if name(f, "Subtype") != "Type3", let fam = SymbolFonts.family(clean) {
                var encObj: CGPDFObjectRef?
                if !CGPDFDictionaryGetObject(f, "Encoding", &encObj), !fam.byRead.isEmpty {
                    info.symbolsByRead = (fam.byRead.keys.sorted(), fam.byRead)
                }
                let base = name(f, "Encoding") ?? enc.flatMap { name($0, "BaseEncoding") }
                let byCode = base == "WinAnsiEncoding" ? fam.winAnsi : base == "MacRomanEncoding" ? fam.macRoman : fam.standard
                let codepage: String.Encoding = base == "MacRomanEncoding" ? .macOSRoman : .windowsCP1252
                let map = toUnicode(f) ?? [:]
                for code in 0..<256 {
                    let glyph = glyphNames[code]
                    guard let t = glyph.map({ fam.names[$0] }) ?? byCode[code] else { continue }
                    // what the glyph reads as without a map: its name's character, its code's
                    var plain: Set<String> = [String(UnicodeScalar(UInt8(code)))]
                    if let s = String(bytes: [UInt8(code)], encoding: codepage) { plain.insert(s) }
                    if let g = glyph, let p = fam.plain[g] { plain.insert(p) }
                    if let g = glyph, g.hasPrefix("C"), let n = Int(g.dropFirst()), n < 256, let sc = UnicodeScalar(n) { plain.insert(String(sc)) }
                    if let u = map[code], !u.unicodeScalars.allSatisfy({ $0.value < 0x20 }) {
                        guard plain.contains(u) else { continue }
                    }
                    info.symbols[code] = (t, plain.filter { !$0.unicodeScalars.allSatisfy { $0.value <= 0x20 } }.sorted())
                }
            }
            // a ToUnicode map that gives such a code something else (a space, ";") is right, not the name
            if !info.ligatures.isEmpty, let map = toUnicode(f) {
                for (code, lig) in info.ligatures {
                    if let u = map[code], u != lig, !u.unicodeScalars.allSatisfy({ (0xFB00...0xFB04).contains($0.value) }) {
                        info.ligatures[code] = nil
                    }
                }
            }
            var fm: CGPDFArrayRef?
            if name(f, "Subtype") == "Type3", CGPDFDictionaryGetArray(f, "FontMatrix", &fm), let fm {
                // glyph widths in the font's own units ("/FontMatrix [.01493 0 0 -.01493 0 0]"): read as
                // thousandths of an em, "Nature" was drawn far narrower than it is and each next
                // piece seemed to start a word
                var a: CGPDFObjectRef?, d: CGPDFObjectRef?
                if CGPDFArrayGetObject(fm, 0, &a), let a, let va = number(a), va != 0,
                   CGPDFArrayGetObject(fm, 3, &d), let d, let vd = number(d), vd != 0 {
                    info.widthScale = abs(va) * 1000
                    info.sizeScale = abs(vd) * 1000
                }
            } else if info.widths.isEmpty, let std = standardWidths(clean) {
                // one of the standard 14 fonts, which may come without widths: guessing 500 for every
                // glyph puts the end of a run of wide letters ("mm") short of where the next run
                // starts, which reads as a word gap
                info.firstChar = 0
                info.widths = std
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

    /// One-byte codes → text from a font's ToUnicode CMap (bfchar and bfrange entries).
    private static func toUnicode(_ f: CGPDFDictionaryRef) -> [Int: String]? {
        var st: CGPDFStreamRef?
        guard CGPDFDictionaryGetStream(f, "ToUnicode", &st), let st else { return nil }
        var format = CGPDFDataFormat.raw
        guard let data = CGPDFStreamCopyData(st, &format) as Data?, let text = String(data: data, encoding: .isoLatin1) else { return nil }
        func utf16(_ hex: Substring) -> String? {
            var units: [UInt16] = []
            var i = hex.startIndex
            while hex.distance(from: i, to: hex.endIndex) >= 4 {
                let j = hex.index(i, offsetBy: 4)
                guard let u = UInt16(hex[i..<j], radix: 16) else { return nil }
                units.append(u)
                i = j
            }
            return String(utf16CodeUnits: units, count: units.count)
        }
        var map: [Int: String] = [:]
        let hex = try! NSRegularExpression(pattern: "<([0-9A-Fa-f]+)>")
        for section in ["bfchar", "bfrange"] {
            for part in text.components(separatedBy: "begin" + section).dropFirst() {
                let body = part.components(separatedBy: "end" + section)[0]
                let values = hex.matches(in: body, range: NSRange(body.startIndex..., in: body)).map { Substring(body[Range($0.range(at: 1), in: body)!]) }
                let n = section == "bfchar" ? 2 : 3
                for k in stride(from: 0, to: values.count - n + 1, by: n) {
                    guard let lo = Int(values[k], radix: 16) else { continue }
                    if n == 2 {
                        if let u = utf16(values[k + 1]) { map[lo] = u }
                    } else if let hi = Int(values[k + 1], radix: 16), hi >= lo, hi - lo < 256, let first = utf16(values[k + 2]),
                              let base = first.unicodeScalars.last {
                        for c in lo...hi {
                            if let sc = Unicode.Scalar(base.value + UInt32(c - lo)) {
                                map[c] = String(first.unicodeScalars.dropLast()) + String(sc)
                            }
                        }
                    }
                }
            }
        }
        return map
    }

    static let ligatureNames: [String: String] = [
        "fi": "fi", "fl": "fl", "ff": "ff", "ffi": "ffi", "ffl": "ffl",
        "f_i": "fi", "f_l": "fl", "f_f": "ff", "f_f_i": "ffi", "f_f_l": "ffl",
        "uniFB00": "ff", "uniFB01": "fi", "uniFB02": "fl", "uniFB03": "ffi", "uniFB04": "ffl",
    ]

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
        var firstCode = -1, lastCode = -1
        var starts: [CGFloat] = []
        var zeroSpaces: [CGFloat] = []   // space glyphs word spacing takes all the room from
        var ligs: [(CGFloat, CGFloat, String, Bool, [String]?, [String: String]?)] = []
        let em = abs(ts.size * font.sizeScale)
        while i < len {
            let code: Int
            if font.twoByte, i + 1 < len { code = Int(bytes[i]) << 8 | Int(bytes[i + 1]); i += 2 }
            else { code = Int(bytes[i]); i += 1 }
            var tx = font.width(code) * font.widthScale / 1000 * ts.size + ts.tc
            if !font.twoByte && code == 32 {
                tx += ts.tw
                if abs(tx) < 0.02 * em { zeroSpaces.append(adv) }
            }
            starts.append(adv)
            if let sym = font.symbols[code] { ligs.append((adv, adv + tx * ts.th, sym.text, code < 32, sym.reads, nil)) }
            else if let lig = font.ligatures[code] { ligs.append((adv, adv + tx * ts.th, lig, code < 32, nil, nil)) }
            else if let s = font.symbolsByRead, code != 32 { ligs.append((adv, adv + tx * ts.th, "", code < 32, s.reads, s.byRead)) }
            if firstCode < 0 { firstCode = code }
            lastCode = code
            adv += tx * ts.th
        }
        let endP = CGPoint(x: adv, y: 0).applying(trm)
        ts.tm = CGAffineTransform(translationX: adv, y: 0).concatenating(ts.tm)
        let effSize = abs(ts.size) * font.sizeScale * sqrt(abs(trm.a * trm.d - trm.b * trm.c))
        guard abs(trm.b) < 0.1 * abs(trm.a) + 0.001 else { return }   // rotated text: ignore
        let a = CGPoint(x: start.x - origin.x, y: pageHeight - (start.y - origin.y))
        let b = CGPoint(x: endP.x - origin.x, y: pageHeight - (endP.y - origin.y))
        let xs = starts.map { CGPoint(x: $0, y: 0).applying(trm).x - origin.x }
        // text outside the form's box is not shown: its baseline outside, or all of it to one side
        if let c = gstack.last?.clip, a.y < c.y0 || a.y - 0.5 * effSize > c.y1 || max(a.x, b.x) < c.x0 || min(a.x, b.x) > c.x1 {
            out.runs.append(TextRun(x0: min(a.x, b.x), x1: max(a.x, b.x), baseline: a.y, size: effSize,
                                    bold: font.bold, italic: font.italic, math: font.math, cmRoman: font.cmRoman, mono: font.mono, hidden: true, starts: xs))
            return
        }
        out.runs.append(TextRun(x0: min(a.x, b.x), x1: max(a.x, b.x), baseline: a.y, size: effSize,
                                bold: font.bold, italic: font.italic, math: font.math, cmRoman: font.cmRoman, mono: font.mono, clip: gstack.last?.clip, starts: xs))
        for (l0, l1, lig, low, reads, byRead) in ligs {
            let p0 = CGPoint(x: l0, y: 0).applying(trm).x - origin.x, p1 = CGPoint(x: l1, y: 0).applying(trm).x - origin.x
            out.ligatures.append(Ligature(x0: min(p0, p1), x1: max(p0, p1), baseline: a.y, size: effSize, text: lig, lowCode: low,
                                          symbol: reads != nil, reads: reads ?? [], byRead: byRead))
        }
        if Self.debugGaps {
            FileHandle.standardError.write("SHOW x \(String(format: "%.2f-%.2f", a.x, b.x)) y \(String(format: "%.1f", a.y)) sz \(String(format: "%.2f", effSize)) n \(len) last \(lastEnd.map { String(format: "%.2f", $0.x) } ?? "-") tc \(ts.tc) tw \(ts.tw) th \(ts.th)\n".data(using: .utf8)!)
        }
        // a jump to the right on the same baseline since the last text = a word space nobody drew;
        // in text squeezed by horizontal scaling (OCR layers: "48.53 Tz") the spaces shrink with it
        if let e = lastEnd, abs(e.y - a.y) < 0.3 * max(effSize, 2), a.x - e.x > 0.15 * effSize * min(1, abs(ts.th), lastScale),
           a.x - e.x < 3 * effSize {
            out.gaps.append(CGPoint(x: (a.x + e.x) / 2, y: a.y))
        } else if let e = lastEnd, abs(e.y - a.y) < 0.3 * max(effSize, 2), abs(a.x - e.x) < 0.08 * effSize,
                  !font.twoByte, firstCode != 32, !lastEndedWithSpace {
            // the text goes on right where the last ended, with no space drawn on either side
            out.joints.append(CGPoint(x: a.x, y: a.y))
        }
        for x in zeroSpaces {
            let p = CGPoint(x: x, y: 0).applying(trm)
            out.joints.append(CGPoint(x: p.x - origin.x, y: pageHeight - (p.y - origin.y)))
        }
        lastEnd = CGPoint(x: max(a.x, b.x), y: a.y)
        lastEndedWithSpace = !font.twoByte && lastCode == 32
        lastScale = abs(ts.th)
    }

    static var debugGaps = false
    private var lastEnd: CGPoint?
    private var lastEndedWithSpace = false
    private var lastScale: CGFloat = 1

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
                var r = R(x0: pts.map(\.x).min()!, y0: pts.map(\.y).min()!, x1: pts.map(\.x).max()!, y1: pts.map(\.y).max()!)
                if let c = gstack.last?.clip {   // only what lies in the form's box shows
                    r = r.intersection(c)
                    guard r.w > 0, r.h > 0 else { return }
                    formImages.append((r, out.runs.count))
                }
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
                var clip = gstack.last?.clip
                var bb: CGPDFArrayRef?
                if CGPDFDictionaryGetArray(sd, "BBox", &bb), let bb, CGPDFArrayGetCount(bb) == 4 {
                    var v: [CGFloat] = []
                    for k in 0..<4 {
                        var o: CGPDFObjectRef?
                        if CGPDFArrayGetObject(bb, k, &o), let o { v.append(Self.number(o) ?? 0) }
                    }
                    if v.count == 4 {
                        let fm = m.concatenating(ctm)
                        let pts = [CGPoint(x: v[0], y: v[1]), CGPoint(x: v[2], y: v[1]), CGPoint(x: v[0], y: v[3]), CGPoint(x: v[2], y: v[3])]
                            .map { p -> CGPoint in let q = p.applying(fm); return CGPoint(x: q.x - origin.x, y: pageHeight - (q.y - origin.y)) }
                        let r = R(x0: pts.map(\.x).min()!, y0: pts.map(\.y).min()!, x1: pts.map(\.x).max()!, y1: pts.map(\.y).max()!)
                        clip = clip.map { $0.intersection(r) } ?? r
                    }
                }
                gstack.append(GState(ctm: m.concatenating(ctm), clip: clip))
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
        add("q") { _, i in
            let m = me(i)
            var g = m.gstack.last!
            g.savedText = m.ts
            m.gstack.append(g)
        }
        add("Q") { _, i in
            let m = me(i)
            guard m.gstack.count > 1 else { return }
            if let t = m.gstack.removeLast().savedText {
                m.ts.tc = t.tc; m.ts.tw = t.tw; m.ts.th = t.th; m.ts.tl = t.tl; m.ts.size = t.size; m.ts.font = t.font
            }
        }
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
