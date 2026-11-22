import CoreGraphics
import CoreText
import PDFKit
import XCTest
@testable import Paper2EpubKit

/// Footnotes and margin notes (PDFNotes.swift): a note at the foot of the page leaves the running text
/// for the end of its chapter and its marker links to it; a note continued on the next page is joined;
/// superscript citations stay citations; a book's margin notes follow their paragraph.
final class FootnoteTests: XCTestCase {

    /// A PDF drawn with Core Text: `pages` is a list of pages, each a list of pieces (text, size, x, y
    /// from the top, raised by).
    struct Piece { var text: String; var size: CGFloat = 10; var x: CGFloat; var y: CGFloat; var raise: CGFloat = 0 }

    static func pdf(_ pages: [[Piece]]) -> Data {
        let data = NSMutableData()
        var box = CGRect(x: 0, y: 0, width: 612, height: 792)
        let ctx = CGContext(consumer: CGDataConsumer(data: data as CFMutableData)!, mediaBox: &box, nil)!
        for page in pages {
            ctx.beginPDFPage(nil)
            for p in page {
                let font = CTFontCreateWithName("Times-Roman" as CFString, p.size, nil)
                let line = CTLineCreateWithAttributedString(NSAttributedString(string: p.text, attributes: [NSAttributedString.Key(kCTFontAttributeName as String): font]))
                ctx.textPosition = CGPoint(x: p.x, y: 792 - p.y + p.raise)
                CTLineDraw(line, ctx)
            }
            ctx.endPDFPage()
        }
        ctx.closePDF()
        return data as Data
    }

    static func width(_ s: String, _ size: CGFloat) -> CGFloat {
        let font = CTFontCreateWithName("Times-Roman" as CFString, size, nil)
        let line = CTLineCreateWithAttributedString(NSAttributedString(string: s, attributes: [NSAttributedString.Key(kCTFontAttributeName as String): font]))
        return CGFloat(CTLineGetTypographicBounds(line, nil, nil, nil))
    }

    /// Body text: `n` lines of a paragraph from `y`, the marker `sup` (if any) after the words of line `at`.
    static func body(_ tag: String, y: CGFloat, n: Int = 6, sup: String? = nil, at: Int = 2, x: CGFloat = 72) -> [Piece] {
        var out: [Piece] = []
        for k in 0..<n {
            let yy = y + 12 * CGFloat(k)
            if let sup, k == at {
                let a = "this line of \(tag) carries a marker here."
                out.append(Piece(text: a, x: x, y: yy))
                let w = width(a, 10)
                out.append(Piece(text: sup, size: 6, x: x + w + 0.5, y: yy, raise: 4))
                out.append(Piece(text: "Then it goes on to the margin and more", x: x + w + 6, y: yy))
            } else {
                let s = k == n - 1 ? "This is sentence \(k) of \(tag), and it ends here." : "This is sentence \(k) of \(tag), with words that run on and on to the margin"
                out.append(Piece(text: s, x: x, y: yy))
            }
        }
        return out
    }

    func html(_ book: Book) -> String { book.chapters.map(\.html).joined() }

    func testFootnoteMovesToTheChapterEndAndContinuesOnTheNextPage() async throws {
        var pages: [[Piece]] = []
        pages.append([Piece(text: "A Study of Things", size: 20, x: 72, y: 100), Piece(text: "1 Introduction", size: 12, x: 72, y: 160)]
                     + Self.body("the start", y: 190) + Self.body("the start again", y: 280))
        pages.append(Self.body("page two", y: 80) + Self.body("page two again", y: 170))
        // page three: a marker in the text, the note at the foot (in 8 pt), not ended
        pages.append(Self.body("page three", y: 80, sup: "2") + Self.body("page three again", y: 170)
                     + [Piece(text: "2", size: 5, x: 72, y: 700, raise: 3),
                        Piece(text: "This iterative approach is not actually necessary for the linear regression model, as", size: 8, x: 76, y: 700),
                        Piece(text: "closed-form expressions for the parameters exist, but", size: 8, x: 72, y: 710)])
        // page four: the note goes on at the foot, without a marker
        pages.append(Self.body("page four", y: 80) + Self.body("page four again", y: 170)
                     + [Piece(text: "it is easier to show the general method on a simple case.", size: 8, x: 72, y: 700)])
        pages.append(Self.body("page five", y: 80) + [Piece(text: "References", size: 12, x: 72, y: 300)])
        let book = try await PDFBook.build(Self.pdf(pages), meta: PaperMeta(), log: { _ in })
        let h = html(book)
        XCTAssertTrue(h.contains(#"class="fnref">2</a>"#), h)
        XCTAssertTrue(h.contains(##"<aside epub:type="footnote" role="doc-footnote" id="fn-p3-2" class="footnote"><p><a href="#fnref-fn-p3-2">2</a> This iterative"##), h)
        // the running text has no note in it, and the note has its continuation
        let note = h.range(of: "<div class=\"footnotes\">").map { String(h[$0.lowerBound...]) } ?? ""
        XCTAssertTrue(note.contains("it is easier to show the general method"), note)
        let text = h.replacingOccurrences(of: note, with: "")
        XCTAssertFalse(text.contains("iterative approach"))
        XCTAssertFalse(text.contains("easier to show"))
        // the paragraph that ran past the note at the page's foot still reads as one
        XCTAssertTrue(text.contains("of page three again"), text)
    }

    func testMarkerDetection() {
        let sup = PLine(text: "2This is a note", bbox: R(x0: 0, y0: 0, x1: 100, y1: 8), size: 8, bold: 0, math: 0,
                        spans: [Span(text: "2", sup: true), Span(text: "This is a note")], page: 0)
        XCTAssertEqual(PDFAnalyzer.noteMarker(sup), "2")
        func plain(_ t: String) -> String? {
            PDFAnalyzer.noteMarker(PLine(text: t, bbox: R(x0: 0, y0: 0, x1: 100, y1: 8), size: 8, bold: 0, math: 0, spans: [Span(text: t)], page: 0))
        }
        XCTAssertEqual(plain("2 This is a note"), "2")
        XCTAssertEqual(plain("2This is a note"), "2")
        XCTAssertEqual(plain("†These authors contributed equally"), "†")
        XCTAssertEqual(plain("*Corresponding author"), "*")
        XCTAssertNil(plain("2. Smith J, Doe A. A title. Nature 2019."))
        XCTAssertNil(plain("[2] Smith J. A title."))
        XCTAssertNil(plain("2019 was a year"))
        PDFAnalyzer.stripMarker(sup, "2")
        XCTAssertEqual(sup.text, "This is a note")
        XCTAssertEqual(sup.spans.map(\.text), ["This is a note"])
    }

    func testMarkerPlaceScores() {
        let l = PLine(text: "", bbox: R(x0: 0, y0: 0, x1: 1, y1: 1), size: 10, bold: 0, math: 0, spans: [], page: 0)
        // after a full stop or a word: a note's marker; after a variable or a digit, or inside a word: not
        XCTAssertGreaterThan(PDFAnalyzer.refScore("no further.", " Then", italic: false, line: l), 0)
        XCTAssertGreaterThanOrEqual(PDFAnalyzer.refScore("the method", " is", italic: false, line: l), 0)
        XCTAssertLessThan(PDFAnalyzer.refScore("x", " + y", italic: true, line: l), 0)
        XCTAssertLessThan(PDFAnalyzer.refScore("10", " m", italic: false, line: l), 0)
        XCTAssertLessThan(PDFAnalyzer.refScore("word", "ing", italic: false, line: l), 0)
    }

    func testSuperscriptCitationsStayCitations() async throws {
        // a paper citing by superscript numbers: no block at the foot, so nothing is a note
        var pages: [[Piece]] = [[Piece(text: "A Study of Things", size: 20, x: 72, y: 100), Piece(text: "1 Introduction", size: 12, x: 72, y: 160)]
                                + Self.body("the start", y: 190, sup: "1") + Self.body("the start again", y: 280, sup: "2")]
        for k in 3...6 { pages.append(Self.body("page \(k)", y: 80, sup: "\(k)") + Self.body("page \(k) again", y: 170, sup: "\(k + 4)")) }
        // a block at the foot that starts with a number but reads like a reference: left alone
        pages[2] += [Piece(text: "4", size: 5, x: 72, y: 700, raise: 3),
                     Piece(text: "Smith, J. & Doe, A. A title of a paper. Nature 12, 34–56 (2019). doi:10.1038/x", size: 8, x: 76, y: 700)]
        pages.append([Piece(text: "References", size: 12, x: 72, y: 80)]
                     + (1...10).map { Piece(text: "\($0). Smith, J. et al. A title number \($0). Nature 12, 34–56 (2019).", size: 9, x: 72, y: 100 + 12 * CGFloat($0)) })
        let book = try await PDFBook.build(Self.pdf(pages), meta: PaperMeta(), log: { _ in })
        let h = html(book)
        XCTAssertFalse(h.contains("fnref"), h)
        XCTAssertFalse(h.contains("doc-footnote"), h)
        XCTAssertTrue(h.contains("<sup>3</sup>"), h)
    }

    func testBookMarginNotesFollowTheirParagraph() async throws {
        // a book page: text column 72–400, margin notes at 420 in 8 pt, level with the second paragraph
        // (a book of 20 pages or more: shorter documents are papers whatever their type)
        var pages: [[Piece]] = []
        for k in 0..<22 {
            var page = Self.body("page \(k)", y: 100, n: 8) + Self.body("page \(k) on", y: 210, n: 8)
            page += [Piece(text: "Notebook \(k).1", size: 8, x: 470, y: 234), Piece(text: "Background", size: 8, x: 470, y: 243)]
            pages.append(page)
        }
        var meta = PaperMeta(); meta.workType = "book"
        let book = try await PDFBook.build(Self.pdf(pages), meta: meta, log: { _ in })
        XCTAssertTrue(book.kind.isBookLike)
        let h = html(book)
        XCTAssertTrue(h.contains(#"<aside class="sidenote" role="note"><p>Notebook 1.1 Background</p></aside>"#), h)
        // the paragraph it sat beside is whole, and the note comes right after it
        XCTAssertTrue(h.contains("This is sentence 7 of page 1 on, and it ends here.</p><aside class=\"sidenote\""), h)
        // (once: not also as a paragraph of the text)
        XCTAssertEqual(h.components(separatedBy: "Notebook 1.1").count, 2, h)
    }
}
