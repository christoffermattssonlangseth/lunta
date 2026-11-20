import PDFKit
import XCTest
@testable import Paper2EpubKit

/// Symbol fonts that name their glyphs like letters, with no ToUnicode map (see SymbolFonts): Cell's
/// Highlights bullets ("d" in AdvPSMPi6) and "© 2024 The Author(s)" ("ª" in AdvPSSym).
final class SymbolFontTests: XCTestCase {

    func testFamilies() {
        XCTAssertEqual(SymbolFonts.family("HBKIKE+AdvPSMPi6")?.names["d"], "•")
        XCTAssertEqual(SymbolFonts.family("HBKKJK+AdvPSSym")?.names["ordfeminine"], "©")
        XCTAssertEqual(SymbolFonts.family("AdvPSSym")?.names["C211"], "©")
        XCTAssertEqual(SymbolFonts.family("XcyghdAdvPi3")?.names["numbersign"], "©")   // Springer's prefix
        XCTAssertEqual(SymbolFonts.family("Wingdings-Regular")?.names["envelopeback"], "✉")
        XCTAssertEqual(SymbolFonts.family("TeX_CM_Maths_Symbols10")?.names["eth"], "(")
        XCTAssertEqual(SymbolFonts.family("AdvP4C4E74")?.names["C2"], "×")
        XCTAssertEqual(SymbolFonts.family("Universal-GreekwithMathPi")?.names["H11001"], "+")
        XCTAssertNil(SymbolFonts.family("AdvPSMP13x"))
        XCTAssertNil(SymbolFonts.family("AdvPi10"))            // not AdvPi1
        XCTAssertNil(SymbolFonts.family("Helvetica"))
        // by code: WinAnsi 0xAA is "ordfeminine", 0x64 "d"
        XCTAssertEqual(SymbolFonts.family("AdvPSSym")?.winAnsi[0xAA], "©")
        XCTAssertEqual(SymbolFonts.family("AdvPSMPi6")?.winAnsi[0x64], "•")
    }

    func testCellHighlightsBulletsAndCopyright() {
        // the bullet drawn in AdvPSMPi6 ("d"), the text in Helvetica; © in AdvPSSym at 0xAA
        let t = lines("""
            BT /F2 7 Tf 72 700 Td (d) Tj ET BT /F1 10 Tf 84 700 Td (ISS elucidates cellular dynamics) Tj ET
            BT /F2 7 Tf 72 680 Td (d) Tj ET BT /F1 10 Tf 84 680 Td (Active EAE lesions propagate) Tj ET
            BT /F1 8 Tf 72 640 Td (April 11, 2024 ) Tj ET BT /F3 8 Tf 128.5 640 Td (\\252) Tj ET BT /F1 8 Tf 135 640 Td ( 2024 The Author\\(s\\)) Tj ET
            """, fonts: [
                "<< /Type /Font /Subtype /Type1 /BaseFont /HBKIKE+AdvPSMPi6 /Encoding /WinAnsiEncoding /FirstChar 100 /LastChar 100 /Widths [833] \(desc) >>",
                "<< /Type /Font /Subtype /Type1 /BaseFont /HBKKJK+AdvPSSym /Encoding /WinAnsiEncoding /FirstChar 170 /LastChar 170 /Widths [791] \(desc) >>",
            ])
        XCTAssertTrue(t.contains("• ISS elucidates cellular dynamics"), t)
        XCTAssertTrue(t.contains("• Active EAE lesions propagate"), t)
        XCTAssertTrue(t.contains("© 2024 The Author(s)"), t)
        XCTAssertFalse(t.contains("ª"), t)
        XCTAssertFalse(t.contains("d ISS"), t)
    }

    func testIdentityToUnicodeDoesNotCount() {
        // Advent's ToUnicode map that only restates the name (<aa> <00AA>) says nothing; one that gives
        // the real character is kept
        let cmap = { (u: String) in """
            /CIDInit /ProcSet findresource begin 12 dict begin begincmap /CMapName /X def /CMapType 2 def
            1 begincodespacerange <aa> <aa> endcodespacerange 1 beginbfchar <aa> <\(u)> endbfchar
            endcmap CMapName currentdict /CMap defineresource pop end end
            """ }
        let font = "<< /Type /Font /Subtype /Type1 /BaseFont /GNDAIO+AdvPSSym /Encoding /WinAnsiEncoding /FirstChar 170 /LastChar 170 /Widths [791] \(desc) /ToUnicode 7 0 R >>"
        let content = "BT /F1 9 Tf 72 700 Td (Cell 187, ) Tj ET BT /F2 9 Tf 112 700 Td (\\252) Tj ET BT /F1 9 Tf 120 700 Td ( 2024 The Author\\(s\\)) Tj ET"
        for (u, want) in [("00AA", "©")] {
            let c = cmap(u)
            let data = WordSpacingTests.pdf(content, fonts: [
                "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>", font,
            ], extra: ["<< /Length \(c.utf8.count) >>\nstream\n\(c)\nendstream"])
            let t = text(PDFDocument(data: data)!)
            XCTAssertTrue(t.contains("\(want) 2024 The Author(s)"), "\(u): \(t)")
        }
        let g = graphics(content, fonts: [
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>",
            "<< /Type /Font /Subtype /Type1 /BaseFont /GNDAIO+AdvPSSym /Encoding /WinAnsiEncoding /FirstChar 170 /LastChar 170 /Widths [791] \(desc) /ToUnicode 7 0 R >>",
        ], extra: ["<< /Length \(cmap("2122").utf8.count) >>\nstream\n\(cmap("2122"))\nendstream"])
        XCTAssertTrue(g.ligatures.filter(\.symbol).isEmpty, "a map to another character (™) is right")
    }

    func testDifferencesNames() {
        // Advent's Symbol with Differences "/C211" (Symbol's code 211, ©) and Elsevier's Universal Greek
        // with Math Pi "/H11001" (+), which PDFKit reads as "Ó" or not at all
        let g = graphics("""
            BT /F2 9 Tf 72 700 Td <02> Tj ET
            BT /F3 9 Tf 72 680 Td <02> Tj ET
            """, fonts: [
                "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>",
                "<< /Type /Font /Subtype /Type1 /BaseFont /AAAAAA+AdvPSSym /Encoding << /Type /Encoding /Differences [2 /C211 /C176] >> /FirstChar 2 /LastChar 3 /Widths [791 400] \(desc) >>",
                "<< /Type /Font /Subtype /Type1 /BaseFont /AAAAAB+Universal-GreekwithMathPi /Encoding << /Type /Encoding /Differences [2 /H11001] >> /FirstChar 2 /LastChar 2 /Widths [600] \(desc) >>",
            ])
        XCTAssertEqual(g.ligatures.filter(\.symbol).map(\.text), ["©", "+"])
        XCTAssertTrue(g.ligatures[0].reads.contains("Ó"), "\(g.ligatures[0].reads)")
    }

    func testBulletLists() {
        let html = "<p>Highlights</p><p>• One item</p><p><sup>•</sup> Two</p><p>Authors</p><p>• Alone</p>"
        XCTAssertEqual(PDFAnalyzer.bulletLists(html),
                       "<p>Highlights</p><ul><li>One item</li><li>Two</li></ul><p>Authors</p><ul><li>Alone</li></ul>")
        XCTAssertEqual(PDFAnalyzer.bulletLists("<p>No • here</p>"), "<p>No • here</p>")
    }

    // MARK: -

    /// a font descriptor for a font that is not embedded (PDFKit reads no text without one)
    private let desc = "/FontDescriptor << /Type /FontDescriptor /FontName /X /Flags 32 /FontBBox [0 -200 1000 800] /ItalicAngle 0 /Ascent 800 /Descent -200 /CapHeight 700 /StemV 80 >>"

    private func lines(_ content: String, fonts extra: [String]) -> String {
        text(PDFDocument(data: WordSpacingTests.pdf(content, fonts: [
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>",
        ] + extra))!)
    }

    private func graphics(_ content: String, fonts: [String], extra: [String] = []) -> PageGraphics {
        let page = PDFDocument(data: WordSpacingTests.pdf(content, fonts: fonts, extra: extra))!.page(at: 0)!
        return PDFContentScanner(box: page.bounds(for: .mediaBox)).scan(page.pageRef!)
    }

    private func text(_ doc: PDFDocument) -> String {
        let a = PDFAnalyzer(doc: doc, log: { _ in })
        a.extract()
        return a.pagesLines.joined().map { $0.text.replacingOccurrences(of: PDFAnalyzer.glue, with: "") }.joined(separator: "\n")
    }
}
