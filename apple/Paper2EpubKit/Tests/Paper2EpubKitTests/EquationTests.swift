import CoreGraphics
import XCTest
@testable import Paper2EpubKit

/// Display equations found line by line (PDFEquations.swift): a formula with its number at the column's
/// right edge is cropped as one image, without the number, which stays text with an id; the prose
/// around it stays prose; a number in the running text is not an equation's.
final class EquationTests: XCTestCase {
    typealias Piece = FootnoteTests.Piece

    func html(_ book: Book) -> String { book.chapters.map(\.html).joined() }

    /// A page of prose with a display formula after the first paragraph, its number at the right edge.
    static func page(_ k: Int, formula: [String], tag: String?, y: CGFloat = 160) -> [Piece] {
        var out = [Piece(text: "A Study of Things", size: 20, x: 72, y: 80), Piece(text: "1 Introduction", size: 12, x: 72, y: 130)]
        out += FootnoteTests.body("page \(k)", y: y, n: 4)
        let row = y + 12 * 4 + 14
        for (i, f) in formula.enumerated() { out.append(Piece(text: f, x: 220, y: row + 14 * CGFloat(i))) }
        if let tag {
            let w = FootnoteTests.width(tag, 10)
            // (the column's right edge: the body lines end at 72 + their width)
            let right = 72 + FootnoteTests.width("This is sentence 0 of page \(k), with words that run on and on to the margin", 10)
            out.append(Piece(text: tag, x: right - w, y: row + 14 * CGFloat(formula.count - 1)))
        }
        let after = row + 14 * CGFloat(formula.count) + 12
        out += FootnoteTests.body("page \(k) after", y: after, n: 4)
        return out
    }

    func testNumberedDisplayIsOneImageWithItsNumberAsText() async throws {
        let pages = [Self.page(0, formula: ["y = f[x, w] + b", "z = g[y] − 1"], tag: "(2.5)"),
                     FootnoteTests.body("the end", y: 100)]
        let book = try await PDFBook.build(FootnoteTests.pdf(pages), meta: PaperMeta(), log: { _ in })
        let h = html(book)
        XCTAssertTrue(h.contains(#"<div class="equation" id="eq-2-5"><span class="eqlabel eqno">(2.5)</span><img class="equation""#), h)
        // its text as the image's alternative, both rows; not in the paragraphs
        XCTAssertTrue(h.contains(#"alt="y = f[x, w] + b z = g[y] − 1""#), h)
        XCTAssertEqual(h.components(separatedBy: "(2.5)").count, 2, h)
        XCTAssertFalse(h.contains("<p>y = f"), h)
        // the prose before and after it stays prose, as two paragraphs
        XCTAssertTrue(h.contains("This is sentence 3 of page 0, and it ends here.</p>"), h)
        XCTAssertTrue(h.contains("<p>This is sentence 0 of page 0 after"), h)
        XCTAssertEqual(book.images.filter { $0.fileName.contains("-eq") }.count, 1)
    }

    func testANumberInTheTextIsNoEquation() async throws {
        // "(3)" at the end of a line of running text, after an ordinary word space
        var p = [Piece(text: "A Study of Things", size: 20, x: 72, y: 80), Piece(text: "1 Introduction", size: 12, x: 72, y: 130)]
        p += FootnoteTests.body("page 0", y: 160, n: 3)
        p.append(Piece(text: "and the result follows from what was said in (3)", x: 72, y: 196))
        p += FootnoteTests.body("page 0 on", y: 220, n: 3)
        let book = try await PDFBook.build(FootnoteTests.pdf([p, FootnoteTests.body("the end", y: 100)]), meta: PaperMeta(), log: { _ in })
        let h = html(book)
        XCTAssertFalse(h.contains("class=\"equation\""), h)
        XCTAssertTrue(h.contains("what was said in (3)"), h)
    }

    func testEquationTagPattern() {
        for t in ["(2.5)", "(12)", "(C.8)", "(4.6a)", "(S3)", "(10.14)"] { XCTAssertTrue(PDFAnalyzer.isEquationTag(t), t) }
        for t in ["(2019)", "(a)", "(see 2.5)", "2.5", "(i)", "(1.2.3.4)"] { XCTAssertFalse(PDFAnalyzer.isEquationTag(t), t) }
    }

    func testASuperscriptPieceGoesBackIntoItsLine() {
        // "the value x* of the" with the star given as a line of its own, in smaller type, raised
        func line(_ t: String, _ x0: CGFloat, _ x1: CGFloat, _ y0: CGFloat, _ size: CGFloat) -> PLine {
            let l = PLine(text: t, bbox: R(x0: x0, y0: y0, x1: x1, y1: y0 + size), size: size, bold: 0, math: 0, spans: [Span(text: t)], page: 0)
            let w = (x1 - x0) / CGFloat(t.count)
            l.charBoxes = (0..<t.count).map { R(x0: x0 + w * CGFloat($0), y0: y0, x1: x0 + w * CGFloat($0 + 1), y1: y0 + size) }
            return l
        }
        var lines = [line("the value x of the", 100, 190, 100, 10), line("∗", 155, 158, 97, 6), line("next line here", 100, 170, 112, 10)]
        PDFAnalyzer.attachFragments(&lines)
        XCTAssertEqual(lines.count, 2)
        XCTAssertEqual(lines[0].text, "the value x∗ of the")
        XCTAssertTrue(lines[0].spans.contains { $0.text == "∗" && $0.sup }, "\(lines[0].spans)")
        // a subscript below the line's middle is lowered
        var l2 = [line("sum over j of a", 100, 175, 100, 10), line("ij", 166, 172, 106, 6)]
        PDFAnalyzer.attachFragments(&l2)
        XCTAssertTrue(l2[0].spans.contains { $0.text == "ij" && $0.sub }, "\(l2[0].spans)")
    }

    func testAPrimeIsNoScript() async throws {
        // "the derivative θ′ of it", the prime set small and raised after θ
        var p = [Piece(text: "A Study of Things", size: 20, x: 72, y: 80), Piece(text: "1 Introduction", size: 12, x: 72, y: 130)]
        p += FootnoteTests.body("page 0", y: 160, n: 3)
        let a = "and the derivative θ"
        p.append(Piece(text: a, x: 72, y: 196))
        let w = FootnoteTests.width(a, 10)
        p.append(Piece(text: "′", size: 7, x: 72 + w + 0.3, y: 196, raise: 3))
        p.append(Piece(text: "of it is what we wanted to show here.", x: 72 + w + 4, y: 196))
        p += FootnoteTests.body("page 0 on", y: 220, n: 3)
        let book = try await PDFBook.build(FootnoteTests.pdf([p, FootnoteTests.body("the end", y: 100)]), meta: PaperMeta(), log: { _ in })
        let h = html(book)
        XCTAssertTrue(h.contains("θ′"), h)
        XCTAssertFalse(h.contains("<sup>′</sup>") || h.contains("<sub>′</sub>"), h)
    }

    func testARunInHeadStartsItsParagraph() async throws {
        // "Forward pass: We treat the computation…" with the head in bold on the paragraph's first line
        var p = [Piece(text: "A Study of Things", size: 20, x: 72, y: 80), Piece(text: "1 Introduction", size: 12, x: 72, y: 130)]
        p += FootnoteTests.body("page 0", y: 160, n: 3)
        let head = "Forward pass:"
        p.append(Piece(text: head, x: 72, y: 210, font: "Times-Bold"))
        let w = FootnoteTests.width(head, 10) + 4
        p.append(Piece(text: "We treat the computation of the loss as a series of calcu-", x: 72 + w, y: 210))
        p.append(Piece(text: "lations that run on and on to the margin of the page, and end.", x: 72, y: 222))
        p += FootnoteTests.body("page 0 on", y: 250, n: 3)
        let book = try await PDFBook.build(FootnoteTests.pdf([p, FootnoteTests.body("the end", y: 100)]), meta: PaperMeta(), log: { _ in })
        let h = html(book)
        // (one paragraph, the head bold at its start; whether PDFKit's synthetic test PDF keeps the space
        // after the colon is not the point)
        XCTAssertNotNil(h.range(of: #"<p><strong>Forward pass:</strong> ?We treat the computation of the loss as a series of calculations that run on"#,
                                options: .regularExpression), h)
        XCTAssertFalse(h.contains("<h3"), h)
    }

    func testAParagraphGoesOnPastTheFiguresOfTheNextPages() async throws {
        // a sentence cut at a page's end, two pages of figure legends (four of them) set in between
        var p1 = [Piece(text: "A Study of Things", size: 20, x: 72, y: 80), Piece(text: "1 Introduction", size: 12, x: 72, y: 130)]
        p1 += FootnoteTests.body("page 0", y: 160, n: 3)
        p1.append(Piece(text: "and so we see that the direction of the deviation (whether the line is", x: 72, y: 700))
        let legends = { (k: Int) in [Piece(text: "Figure \(k).1 A legend for a figure that is not drawn here.", size: 9, x: 72, y: 300),
                                     Piece(text: "Figure \(k).2 Another legend for a figure not drawn.", size: 9, x: 72, y: 600)] }
        let p4 = [Piece(text: "above or below the data) is unimportant, and the rest of it goes on here.", x: 72, y: 100)]
            + FootnoteTests.body("page 3", y: 130, n: 3)
        let book = try await PDFBook.build(FootnoteTests.pdf([p1, legends(1), legends(2), p4]), meta: PaperMeta(), log: { _ in })
        let h = html(book)
        XCTAssertTrue(h.contains("(whether the line is above or below the data) is unimportant"), h)
    }

    func testABooksCodeListingIsPreformatted() async throws {
        // a book's page with a listing in Courier, indented, under its legend
        var pages: [[Piece]] = []
        for k in 0..<22 {
            var page = FootnoteTests.body("page \(k)", y: 100, n: 6)
            if k == 5 {
                let code = [("for epoch in range(100):", 0), ("for i, data in enumerate(loader):", 1), ("pred = model(x_batch)", 2),
                            ("loss = criterion(pred, y_batch)", 2), ("loss.backward()", 2), ("scheduler.step()", 1)]
                for (j, (t, indent)) in code.enumerated() {
                    page.append(Piece(text: t, size: 8, x: 90 + 4.8 * 4 * CGFloat(indent), y: 220 + 9.5 * CGFloat(j), font: "Courier"))
                }
                page.append(Piece(text: "Figure 5.1 Sample code for training a network.", size: 9, x: 90, y: 290))
                page += FootnoteTests.body("page \(k) on", y: 320, n: 4)
            }
            pages.append(page)
        }
        var meta = PaperMeta(); meta.workType = "book"
        let book = try await PDFBook.build(FootnoteTests.pdf(pages), meta: meta, log: { _ in })
        let h = html(book)
        XCTAssertTrue(h.contains("<figure class=\"code\"><pre><code>for epoch in range(100):\n    for i, data in enumerate(loader):\n        pred = model(x_batch)"), h)
        XCTAssertTrue(h.contains("\n    scheduler.step()</code></pre><figcaption>Figure 5.1 Sample code"), h)
    }
}
