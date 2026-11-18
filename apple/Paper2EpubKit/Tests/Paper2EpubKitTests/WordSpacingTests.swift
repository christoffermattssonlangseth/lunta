import CoreGraphics
import CoreText
import PDFKit
import XCTest
@testable import Paper2EpubKit

/// Word breaks in text read from PDFs: spaces PDFKit reports where the glyphs leave no room for one
/// ("foundati on model for pa thology"), text a page does not show, and the layouts whose spaces must
/// stay as they are.
final class WordSpacingTests: XCTestCase {

    // MARK: spaces without room

    func testJointsInTheContentStream() {
        // "foundati" -15 "on": the text goes on 0.18 pt further with no space drawn (where PDFKit, reading
        // a Type 3 font, may put a space); "Cho " + "et al": a space drawn, even if a skewed font's glyph
        // boxes cover it
        let g = graphics(raw: """
            BT /F1 12 Tf 72 700 Td [(A whole-slide foundati) -15 (on model) 40 (s)] TJ ET
            BT /F1 12 Tf 72 680 Td (Cho ) Tj 1 0 0.32 1 100 680 Tm (et al) Tj ET
            """)
        let joints = g.joints.map { (Int($0.x.rounded()), Int($0.y.rounded())) }
        XCTAssertEqual(joints.count, 2, "\(joints)")
        XCTAssertTrue(joints.allSatisfy { $0.1 == 92 }, "\(joints)")   // both on the first line
    }

    func testSpacesWithoutRoomInURLs() {
        // space glyphs word spacing takes all the room from, to let a URL break
        let t = text(raw: "BT /F1 12 Tf 72 700 Td (See https://) Tj -3.336 Tw ( ) Tj 0 Tw (creat) Tj -3.336 Tw ( ) Tj 0 Tw (ivecommons.org.) Tj ET")
        XCTAssertTrue(t.contains("See https://creativecommons.org."), t)
    }

    func testTightlyJustifiedSpacesStay() {
        // space glyphs word spacing takes back to 0.4 pt (0.036 em) are still spaces
        let t = text(raw: "BT /F1 12 Tf 72 700 Td -2.9 Tw (6e\\), which is similar to the observations in healthy mice.) Tj ET")
        XCTAssertTrue(t.contains("6e), which is similar to the observations in healthy mice."), t)
    }

    func testType3FontWidthsInFontUnits() {
        // a Type 3 font whose widths are in units of 1/67 em ("/FontMatrix [.01493 0 0 .01493 0 0]"),
        // placed in short pieces 0.18 pt apart: read as thousandths of an em, each piece seemed to end
        // where it starts, and the next to start a word ("Nat ure 6 30")
        let t = text(data: Self.type3PDF(pieces: ["Nat", "ure 6", "30, f", "rom a", " real", "-world", " foundati", "on."]))
        XCTAssertTrue(t.contains("Nature 630, from a real-world foundation."), t)
    }

    func testOCRLayerSqueezedByHorizontalScaling() {
        // invisible Courier text of a scanned page, each word placed and squeezed to fit ("48.53 Tz"):
        // word gaps of 1.4 pt
        let t = text(raw: """
            BT 3 Tr 72 700 Td /F3 9.9 Tf 59.14 Tz (*Department) Tj /F3 10.2 Tf 48.53 Tz 41.16 0 Td (of) Tj
            /F3 9.2 Tf 61.55 Tz 7.32 0 Td (Anatomy,) Tj /F3 9.5 Tf 53.26 Tz 30.72 0 Td (University) Tj ET
            BT 3 Tr 72 680 Td /F3 12.2 Tf 50 Tz (of) Tj /F3 12 Tf 73.33 Tz 8.4 0 Td (dUTP) Tj ET
            """)
        XCTAssertTrue(t.contains("*Department of Anatomy, University"), t)
        XCTAssertTrue(t.contains("of dUTP"), t)   // the gap after squeezed text, before text squeezed less
    }

    func testRule() {
        func box(_ x0: CGFloat, _ x1: CGFloat) -> R { R(x0: x0, y0: 100, x1: x1, y1: 112) }
        func spurious(_ space: R, _ p: R, _ n: R, _ next: String = "o", joint: Bool = true) -> Bool {
            PDFAnalyzer.isSpuriousSpace(" ", space, added: false, prev: ("i", p, 12), next: (next, n, 12), joint: joint)
        }
        XCTAssertTrue(spurious(box(50, 50.18), box(47, 50), box(50.18, 56)))           // "foundati on"
        XCTAssertTrue(spurious(box(50, 50.18), box(47, 50), box(50.18, 54), "-"))      // "real -world"
        XCTAssertTrue(spurious(box(50, 50), box(47, 50.3), box(50, 56)))               // kerned into each other
        XCTAssertFalse(spurious(box(50, 50.6), box(47, 50), box(50.6, 56), joint: false)) // a space drawn ("et al" in a skewed font)
        XCTAssertFalse(spurious(box(50, 53), box(47, 50), box(53, 59)))                // a normal space
        XCTAssertFalse(spurious(box(50, 50), box(47, 50), box(53, 59)))                // a gap the glyphs leave
        XCTAssertFalse(spurious(box(50, 50.18), box(47, 50), box(50.18, 56)
            .offsetBy(y: 14)))                                                          // next line
        XCTAssertFalse(PDFAnalyzer.isSpuriousSpace(" ", box(50, 50), added: true,       // put back for a line break
                                                   prev: ("s", box(47, 50), 12), next: ("t", box(50, 56), 12), joint: true))
    }

    func testTextOutsideAFormsBoxIsNotShown() {
        // a figure included as a whole manuscript page: only the figure's box shows, but the page's own
        // text lies under the article's lines, a point lower and set differently
        var body = "", hidden = ""
        for i in 0..<8 {
            let y = 700 - i * 13
            body += "BT /F2 10 Tf 72 \(y) Td (Line \(i) of the article text, set in its own way.) Tj ET\n"
            hidden += "BT /F2 10 Tf 75 \(y - 1) Td (Manuscript line \(i) with other words here.) Tj ET\n"
        }
        hidden += "BT /F2 10 Tf 250 450 Td (Figure label A) Tj ET\n"
        let form = "<< /Type /XObject /Subtype /Form /BBox [200 400 400 500] /Resources << /Font << /F2 6 0 R >> >> "
            + "/Length \(hidden.utf8.count) >>\nstream\n\(hidden)\nendstream"
        let content = body + "q /Fm1 Do Q"
        let t = text(data: Self.pdf(content, extra: [form], xobject: "/Fm1 8 0 R"))
        for i in 0..<8 { XCTAssertTrue(t.contains("Line \(i) of the article text, set in its own way."), t) }
        XCTAssertFalse(t.contains("Manuscript"), t)
        XCTAssertTrue(t.contains("Figure label A"), t)
    }

    // MARK: layouts whose spaces stay

    func testJustifiedWideSpaces() {
        let t = text(raw: "BT /F1 12 Tf 72 700 Td 6 Tw (Justified text with wide word spaces) Tj ET")
        XCTAssertTrue(t.contains("Justified text with wide word spaces"), t)
    }

    func testWordGapsWithoutSpaceGlyphs() {
        let t = text(raw: "BT /F1 12 Tf 72 700 Td [(Wide) -900 (gaps) -400 (between) -260 (the) -200 (words)] TJ ET")
        XCTAssertTrue(t.contains("Wide gaps between the words"), t)
    }

    func testKerningKeepsWords() {
        let t = text(raw: "BT /F1 12 Tf 72 700 Td [(W) 80 (av) -15 (es and) -10 ( T) 70 (ables)] TJ ET")
        XCTAssertTrue(t.contains("Waves and Tables"), t)
    }

    func testLetterSpacedHeading() {
        let t = text(raw: "BT /F1 12 Tf 72 700 Td 3 Tc (REFERENCES) Tj ET BT /F1 10 Tf 72 680 Td 0 Tc (1. A reference.) Tj ET")
        XCTAssertTrue(t.contains("REFERENCES"), t)
    }

    func testSmallCaps() {
        let t = text(raw: "BT /F1 12 Tf 72 700 Td (A) Tj /F1 9.6 Tf (BSTRACT AND) Tj /F1 12 Tf ( M) Tj /F1 9.6 Tf (ETHODS) Tj ET")
        XCTAssertTrue(t.contains("ABSTRACT AND METHODS"), t)
    }

    func testSuperscriptCitation() {
        let t = text(raw: """
            BT /F1 12 Tf 72 700 Td (Seen in tumour cells) Tj /F1 7 Tf 0 4 Td (12,13) Tj /F1 12 Tf 0 -4 Td (, and in the stroma.) Tj ET
            """)
        XCTAssertTrue(t.contains("cells12,13, and in the stroma."), t)
    }

    func testTightTableColumns() {
        // numbers in columns 4 pt (0.44 em) apart, each its own text object
        var c = ""
        for (row, y) in [700, 688, 676].enumerated() {
            var x: CGFloat = 72
            for col in 0..<4 {
                let s = String(format: "0.%02d", 90 - row * 3 - col)
                c += "BT /F2 9 Tf \(x) \(y) Td (\(s)) Tj ET\n"
                x += 4 * 9 * 0.556 + 4   // four Helvetica digits/point + 4 pt
            }
        }
        let t = text(raw: c)
        XCTAssertTrue(t.contains("0.90 0.89 0.88 0.87"), t)
    }

    func testTwoColumns() {
        var c = ""
        for i in 0..<12 {
            let y = 700 - i * 14
            c += "BT /F1 10 Tf 72 \(y) Td (left column line \(i) ends here) Tj ET\n"
            c += "BT /F1 10 Tf 320 \(y) Td (right column line \(i) goes on) Tj ET\n"
        }
        let t = text(raw: c)
        XCTAssertFalse(t.contains("hereright"), t)
        XCTAssertTrue(t.contains("left column line 3 ends here"), t)
        XCTAssertTrue(t.contains("right column line 3 goes on"), t)
    }

    func testLigaturesAndMath() {
        let t = text(drawn: [("We find an efficient workflow for fluorescence.", "Times-Roman", 12),
                             ("Then f(x) = y + 1 for all x.", "Times-Roman", 12)])
        XCTAssertTrue(t.contains("We find an efficient workflow for fluorescence."), t)
        XCTAssertTrue(t.contains("f(x) = y + 1 for all x."), t)
    }

    // MARK: helpers

    /// Text of a one-page PDF whose content stream is `content` (/F1 Helvetica without widths,
    /// /F2 Helvetica with them, /F3 Courier), as the analyzer reads its lines.
    private func text(raw content: String) -> String { text(data: Self.pdf(content)) }

    private func text(data: Data) -> String { lines(PDFDocument(data: data)!) }

    private func graphics(raw content: String) -> PageGraphics {
        let page = PDFDocument(data: Self.pdf(content))!.page(at: 0)!
        return PDFContentScanner(box: page.bounds(for: .mediaBox)).scan(page.pageRef!)
    }

    /// Text of a PDF with each line drawn by CoreText (embedded font, ligatures).
    private func text(drawn rows: [(String, String, CGFloat)]) -> String {
        let data = NSMutableData()
        var box = CGRect(x: 0, y: 0, width: 612, height: 792)
        let ctx = CGContext(consumer: CGDataConsumer(data: data as CFMutableData)!, mediaBox: &box, nil)!
        ctx.beginPDFPage(nil)
        var y: CGFloat = 700
        for (s, font, size) in rows {
            let f = CTFontCreateWithName(font as CFString, size, nil)
            let line = CTLineCreateWithAttributedString(
                NSAttributedString(string: s, attributes: [NSAttributedString.Key(kCTFontAttributeName as String): f]))
            ctx.textPosition = CGPoint(x: 72, y: y)
            CTLineDraw(line, ctx)
            y -= 1.4 * size
        }
        ctx.endPDFPage()
        ctx.closePDF()
        return lines(PDFDocument(data: data as Data)!)
    }

    private func lines(_ doc: PDFDocument) -> String {
        let a = PDFAnalyzer(doc: doc, log: { _ in })
        a.extract()
        return a.pagesLines.joined().map { $0.text.replacingOccurrences(of: PDFAnalyzer.glue, with: "") }.joined(separator: "\n")
    }

    /// `extra` objects are numbered after the fonts; `xobject` names them in the page's resources.
    static func pdf(_ content: String, fonts: [String]? = nil, extra: [String] = [], xobject: String? = nil) -> Data {
        let widths = "[" + (32...126).map { c -> String in
            let ch = Character(UnicodeScalar(c)!)
            return ch.isNumber ? "556" : ch == "." || ch == " " ? "278" : "556"
        }.joined(separator: " ") + "]"
        let fonts = fonts ?? [
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>",
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding /FirstChar 32 /LastChar 126 /Widths \(widths) >>",
            "<< /Type /Font /Subtype /Type1 /BaseFont /Courier /Encoding /WinAnsiEncoding >>",
        ]
        let fontRefs = fonts.indices.map { "/F\($0 + 1) \($0 + 5) 0 R" }.joined(separator: " ")
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>",
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << \(xobject.map { "/XObject << \($0) >> " } ?? "")/Font << \(fontRefs) >> >> >>",
            "<< /Length \(content.utf8.count) >>\nstream\n\(content)\nendstream",
        ] + fonts + extra
        var out = "%PDF-1.4\n"
        var offsets: [Int] = []
        for (i, o) in objects.enumerated() {
            offsets.append(out.utf8.count)
            out += "\(i + 1) 0 obj\n\(o)\nendobj\n"
        }
        let xref = out.utf8.count
        out += "xref\n0 \(objects.count + 1)\n0000000000 65535 f \n"
        for o in offsets { out += String(format: "%010d 00000 n \n", o) }
        out += "trailer\n<< /Size \(objects.count + 1) /Root 1 0 R >>\nstartxref\n\(xref)\n%%EOF\n"
        return Data(out.utf8)
    }
}

extension WordSpacingTests {
    /// A PDF in a Type 3 font of block glyphs with 67 units to the em, whose `pieces` are each placed
    /// 15/1000 em after the last, as in manuscripts printed to PDF.
    static func type3PDF(pieces: [String]) -> Data {
        let chars = Array(Set(pieces.joined())).sorted()
        let code = Dictionary(uniqueKeysWithValues: chars.enumerated().map { ($1, $0 + 1) })
        let em: CGFloat = 67
        func width(_ c: Character) -> Int { Int((c == " " ? 0.25 : "il.,-".contains(c) ? 0.3 : 0.55) * em) }
        // objects: 5 font, 6 CharProcs, 7 Encoding, 8 ToUnicode, 9… glyph procedures
        let procs = chars.map { c -> String in
            let w = width(c)
            let p = c == " " ? "\(w) 0 d0" : "\(w) 0 0 0 \(w) 47 d1 3 0 \(w - 6) 47 re f"
            return "<< /Length \(p.utf8.count) >>\nstream\n\(p)\nendstream"
        }
        let charProcs = "<< " + chars.indices.map { "/g\($0 + 1) \($0 + 9) 0 R" }.joined(separator: " ") + " >>"
        let encoding = "<< /Type /Encoding /Differences [1 " + chars.indices.map { "/g\($0 + 1)" }.joined(separator: " ") + "] >>"
        let bf = chars.map { String(format: "<%02X> <%04X>", code[$0]!, $0.unicodeScalars.first!.value) }.joined(separator: "\n")
        let cmap = """
            /CIDInit /ProcSet findresource begin
            12 dict begin
            begincmap
            /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def
            /CMapName /Adobe-Identity-UCS def
            /CMapType 2 def
            1 begincodespacerange
            <00> <FF>
            endcodespacerange
            \(chars.count) beginbfchar
            \(bf)
            endbfchar
            endcmap
            CMapName currentdict /CMap defineresource pop
            end
            end
            """
        let widths = chars.map { String(width($0)) }.joined(separator: " ")
        let font = "<< /Type /Font /Subtype /Type3 /FontBBox [0 0 \(Int(em)) \(Int(em))] /FontMatrix [0.01493 0 0 0.01493 0 0] "
            + "/CharProcs 6 0 R /Encoding 7 0 R /FirstChar 1 /LastChar \(chars.count) /Widths [\(widths)] /ToUnicode 8 0 R "
            + "/Resources << >> /FontDescriptor << /Type /FontDescriptor /FontName /T3 /Flags 32 /FontBBox [0 0 67 67] "
            + "/ItalicAngle 0 /Ascent 50 /Descent -13 /CapHeight 47 /StemV 8 >> >>"
        // each piece placed where the last ended, plus 15/1000 em
        let shown = pieces.map { p in
            let advance = CGFloat(p.map(width).reduce(0, +)) * 0.01493 + 0.015
            return "(" + p.map { String(format: "\\%03o", code[$0]!) }.joined() + ") Tj " + String(format: "%.5f 0 Td", advance)
        }
        let content = "BT /F1 1 Tf 12.0622 0 0 12.0622 72 700 Tm " + shown.joined(separator: " ") + " ET"
        return pdf(content, fonts: [font], extra: [charProcs, encoding,
                                                    "<< /Length \(cmap.utf8.count) >>\nstream\n\(cmap)\nendstream"] + procs)
    }
}

private extension R {
    func offsetBy(y: CGFloat) -> R { R(x0: x0, y0: y0 + y, x1: x1, y1: y1 + y) }
}
