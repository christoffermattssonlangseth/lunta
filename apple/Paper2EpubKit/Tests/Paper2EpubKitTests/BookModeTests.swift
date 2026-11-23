import CoreGraphics
import CoreText
import PDFKit
import XCTest
@testable import Paper2EpubKit

/// Books as PDFs: the paper-or-book decision, chapters from the outline, running heads that change
/// with the chapter, the printed table of contents and the index; and image names that may not
/// repeat in the EPUB.
final class BookModeTests: XCTestCase {

    // MARK: the classifier

    func testShortDocumentsArePapers() {
        var s = KindSignals()
        s.pages = 12; s.abstract = true; s.doiLine = true
        XCTAssertEqual(DocumentClassifier.classify(s).0, .paper)
        // a long review with bookmarks and a contents box is still a paper
        s = KindSignals()
        s.pages = 38; s.outlineEntries = 30; s.outlineTop = 9; s.outlineDepth = 2; s.contentsPage = true; s.numberedHeads = 0.9
        XCTAssertEqual(DocumentClassifier.classify(s).0, .paper)
    }

    func testLongPaperWithoutBookSignals() {
        // a 60-page supplement or report: pages alone don't make a book
        var s = KindSignals()
        s.pages = 60; s.outlineEntries = 12; s.outlineTop = 6; s.outlineDepth = 2; s.abstract = true; s.doiLine = true
        XCTAssertEqual(DocumentClassifier.classify(s).0, .paper)
    }

    func testTextbookSignals() {
        // Prince, Understanding Deep Learning (541 pages)
        var s = KindSignals()
        s.pages = 541; s.outlineEntries = 181; s.outlineTop = 28; s.outlineDepth = 2
        s.tocPages = 5; s.chapterPages = 21; s.indexPage = true; s.preface = true; s.numberedHeads = 0.79
        let (kind, why) = DocumentClassifier.classify(s)
        XCTAssertEqual(kind, .book)
        XCTAssertTrue(why.contains { $0.contains("541 pages") }, "\(why)")
        // the same with nothing but its size and an ISBN
        s = KindSignals()
        s.pages = 320; s.isbn = true
        XCTAssertEqual(DocumentClassifier.classify(s).0, .book)
    }

    func testWorkTypeAndThesis() {
        var s = KindSignals()
        s.pages = 30; s.workType = "book"
        XCTAssertEqual(DocumentClassifier.classify(s).0, .book)
        s = KindSignals()
        s.pages = 180; s.outlineEntries = 40; s.outlineTop = 7; s.outlineDepth = 2; s.tocPages = 2; s.thesisWords = true
        XCTAssertEqual(DocumentClassifier.classify(s).0, .thesis)
        s.workType = "dissertation"
        XCTAssertEqual(DocumentClassifier.classify(s).0, .thesis)
        // a journal article's type keeps a long article a paper
        s = KindSignals()
        s.pages = 90; s.outlineEntries = 20; s.outlineTop = 6; s.outlineDepth = 2; s.workType = "journal-article"
        XCTAssertEqual(DocumentClassifier.classify(s).0, .paper)
    }

    func testRunningHeadNumbers() {
        XCTAssertEqual(PDFAnalyzer.headNumber("28 3 Shallow neural networks"), 28)
        XCTAssertEqual(PDFAnalyzer.headNumber("3.1 Neural network example 29"), 29)
        XCTAssertNil(PDFAnalyzer.headNumber("3.1 Neural network example"))
        XCTAssertNil(PDFAnalyzer.headNumber("29"))
    }

    // MARK: a whole book

    func testSyntheticBookWithOutline() async throws {
        let book = try await PDFBook.build(Self.bookPDF(outline: true), meta: PaperMeta(), log: { _ in })
        XCTAssertEqual(book.kind, .book, "\(book.kindReasons)")
        XCTAssertTrue(book.notes.contains { $0.contains("outline") }, "\(book.notes)")
        let titles = book.chapters.map(\.title)
        XCTAssertEqual(Array(titles.filter { $0.first?.isNumber == true }),
                       ["1 Foundations of Things", "2 Methods of Study", "3 Results in Detail", "4 Further Directions", "5 Closing Thoughts"],
                       "\(titles)")
        XCTAssertEqual(titles.last, "Index")
        XCTAssertTrue(titles.contains("Preface"), "\(titles)")   // its bookmark points to the contents page
        guard let ch1 = book.chapters.first(where: { $0.title.hasPrefix("1 ") }) else { return XCTFail("\(titles)") }
        XCTAssertEqual(ch1.headings.filter { $0.level == 2 }.map(\.title), ["1.1 First Part of Chapter One", "1.2 Second Part of Chapter One"])
        let all = book.chapters.map(\.html).joined()
        // no running heads, no table of contents, no "Chapter 1" label
        XCTAssertNil(all.range(of: #"\d+ \d Foundations of Things"#, options: .regularExpression), "running head left in")
        XCTAssertNil(all.range(of: #"Part of Chapter One \d+"#, options: .regularExpression), "running head left in")
        XCTAssertFalse(all.contains(". . . ."), "table of contents left in")
        XCTAssertFalse(all.contains(">Chapter 1<"))
        XCTAssertTrue(all.contains("This is sentence 3 of page 20"), "body text lost")
        let index = book.chapters.last!.html
        XCTAssertTrue(index.contains("<p class=\"index\">apples, 3, 17</p>"), index)
        // paper steps don't run: no abstract chapter, no affiliations from the title page
        XCTAssertFalse(titles.contains("Abstract"))
        XCTAssertEqual(book.meta.authors, ["Jane Q. Writer"])
    }

    func testRunningHeadWithItsPageNumberApart() async throws {
        // the head's words and its page number in pieces on one baseline: one margin line, left out
        let book = try await PDFBook.build(Self.bookPDF(outline: true, numberApart: true), meta: PaperMeta(), log: { _ in })
        let all = book.chapters.map(\.html).joined()
        XCTAssertNil(all.range(of: #"<(h3|p)[^>]*>\d Foundations of Things<"#, options: .regularExpression), "running head left in")
        XCTAssertNil(all.range(of: #"<p>\d{1,2}</p>"#, options: .regularExpression), "page number left in")
        XCTAssertTrue(all.contains("This is sentence 3 of page 20"), "body text lost")
    }

    func testSyntheticBookWithoutOutline() async throws {
        let book = try await PDFBook.build(Self.bookPDF(outline: false), meta: PaperMeta(), log: { _ in })
        XCTAssertEqual(book.kind, .book, "\(book.kindReasons)")
        let titles = book.chapters.map(\.title)
        for t in ["1 Foundations of Things", "2 Methods of Study", "5 Closing Thoughts", "Index"] {
            XCTAssertTrue(titles.contains(t), "\(t) not in \(titles)")
        }
        // one chapter per chapter, not one per page
        XCTAssertLessThan(titles.count, 12, "\(titles)")
    }

    // MARK: image names

    func testImageNamesDoNotRepeat() throws {
        let an = PDFAnalyzer(doc: PDFDocument(data: WordSpacingTests.pdf("BT /F1 12 Tf 72 700 Td (x) Tj ET"))!, log: { _ in })
        XCTAssertEqual(an.uniqueName("p296-eq192"), "p296-eq192")
        XCTAssertEqual(an.uniqueName("p296-eq192"), "p296-eq192-2")
        XCTAssertEqual(an.uniqueName("p296-eq192"), "p296-eq192-3")
        XCTAssertEqual(an.uniqueName("p296-eq193"), "p296-eq193")
    }

    func testEPUBHoldsEachImageOnce() throws {
        let img = BookImage(fileName: "p1-eq100.png", data: Data([1, 2, 3]), mediaType: "image/png")
        let book = Book(meta: PaperMeta(), chapters: [Chapter(title: "A", html: "<section><h1 id=\"a\">A</h1><p>x</p></section>", headings: [])],
                        images: [img, img], sourceLabel: "test", notes: [])
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("epub-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: dir) }
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let url = dir.appendingPathComponent("a.epub")
        try EPUBWriter.write(book, cover: nil, to: url)
        let files = try ZipReader.extract(Data(contentsOf: url), to: dir.appendingPathComponent("x"))
        XCTAssertEqual(files.filter { $0.lastPathComponent == "p1-eq100.png" }.count, 1)
        let opf = try String(contentsOf: dir.appendingPathComponent("x/EPUB/content.opf"), encoding: .utf8)
        XCTAssertEqual(opf.components(separatedBy: "images/p1-eq100.png").count - 1, 1)
    }

    func testCMapSuffixIsNotHeavy() {
        // XeLaTeX's "LMRoman10-Regular-Identity-H": the "-H" is the CMap's (horizontal), not "Heavy"
        let fonts = ["<< /Type /Font /Subtype /Type1 /BaseFont /ABCDEF+LMRoman10-Regular-Identity-H /Encoding /WinAnsiEncoding >>",
                     "<< /Type /Font /Subtype /Type1 /BaseFont /ABCDEF+LMSans10-Bold-Identity-H /Encoding /WinAnsiEncoding >>",
                     "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-H /Encoding /WinAnsiEncoding >>"]
        let page = PDFDocument(data: WordSpacingTests.pdf("BT /F1 12 Tf 72 700 Td (plain) Tj ET BT /F2 12 Tf 72 650 Td (bold) Tj ET BT /F3 12 Tf 72 600 Td (heavy) Tj ET",
                                                         fonts: fonts))!.page(at: 0)!
        let runs = PDFContentScanner(box: page.bounds(for: .mediaBox)).scan(page.pageRef!).runs.sorted { $0.baseline < $1.baseline }
        XCTAssertEqual(runs.map(\.bold), [false, true, true], "\(runs.map { ($0.baseline, $0.bold) })")
    }

    // MARK: helpers

    /// A 60-page book: title page, a contents page in dot leaders (whose bookmark "Preface" points
    /// one page early, as hyperref does with \chapter*), a preface, five chapters of 11 pages with
    /// "Chapter N" pages and running heads with page numbers that change with the chapter ("12 1
    /// Foundations of Things" on the left, "1.1 First Part of Chapter One 13" on the right), and a
    /// two-page index.
    static func bookPDF(outline: Bool, numberApart: Bool = false) -> Data {
        let data = NSMutableData()
        var box = CGRect(x: 0, y: 0, width: 612, height: 792)
        let ctx = CGContext(consumer: CGDataConsumer(data: data as CFMutableData)!, mediaBox: &box, nil)!
        let body = CTFontCreateWithName("Times-Roman" as CFString, 10, nil)
        let bold = CTFontCreateWithName("Times-Bold" as CFString, 12, nil)
        let big = CTFontCreateWithName("Times-Bold" as CFString, 20, nil)
        func draw(_ s: String, _ f: CTFont, _ x: CGFloat, _ y: CGFloat) {
            let line = CTLineCreateWithAttributedString(NSAttributedString(string: s, attributes: [NSAttributedString.Key(kCTFontAttributeName as String): f]))
            ctx.textPosition = CGPoint(x: x, y: 792 - y)
            CTLineDraw(line, ctx)
        }
        let chapters = ["Foundations of Things", "Methods of Study", "Results in Detail", "Further Directions", "Closing Thoughts"]
        let ordinals = ["One", "Two", "Three", "Four", "Five"]
        var marks: [(String, Int, Int, CGFloat)] = []   // title, level, page index, y from top
        var pno = 0
        func page(_ content: () -> Void) { ctx.beginPDFPage(nil); content(); ctx.endPDFPage(); pno += 1 }
        func paragraph(_ y: inout CGFloat, _ tag: String, lines: Int = 6) {
            for k in 0..<lines {
                let s = k == lines - 1 ? "This is sentence \(k) of \(tag), and it ends here." : "This is sentence \(k) of \(tag), with words that run on and on to the margin"
                draw(s, body, 72, y); y += 12
            }
            y += 8
        }
        page { draw("A Book About Things", big, 150, 200); draw("Jane Q. Writer", bold, 200, 250) }
        // contents
        page {
            draw("Contents", big, 72, 100)
            var y: CGFloat = 140
            draw("Preface . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . iii", body, 72, y); y += 14
            for (i, c) in chapters.enumerated() {
                draw("\(i + 1) \(c) \(4 + 11 * i)", bold, 72, y); y += 14
                for s in 1...2 {
                    draw("\(i + 1).\(s) Part \(s) . . . . . . . . . . . . . . . . . . . . . . . . . . . . \(5 + 11 * i + 3 * s)", body, 90, y); y += 14
                }
            }
            draw("Index . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . 60", body, 72, y)
        }
        marks.append(("Preface", 1, pno - 1, 60))   // points to the contents page
        page {
            draw("Preface", big, 72, 120)
            var y: CGFloat = 170
            paragraph(&y, "the preface")
        }
        for (i, c) in chapters.enumerated() {
            for k in 0..<11 {
                let printed = pno - 2   // printed page = index - 2
                page {
                    var y: CGFloat = 140
                    if k == 0 {
                        marks.append((c, 1, pno, 90))
                        draw("Chapter \(i + 1)", bold, 72, 110)
                        draw(c, big, 72, 140)
                        y = 190
                    } else if printed % 2 == 0 {
                        // (or the page number apart from the head, as a line of its own: "406     20 Why does…")
                        if numberApart { draw("\(printed)", body, 72, 60); draw("\(i + 1) \(c)", body, 150, 60) } else {
                            draw("\(printed) \(i + 1) \(c)", body, 72, 60)
                        }
                    } else {
                        draw("\(i + 1).\(k < 6 ? 1 : 2) \(k < 6 ? "First" : "Second") Part of Chapter \(ordinals[i]) \(printed)", body, 300, 60)
                    }
                    if k == 1 || k == 6 {
                        let s = k == 1 ? 1 : 2
                        marks.append(("\(s == 1 ? "First" : "Second") Part of Chapter \(ordinals[i])", 2, pno, y - 4))
                        draw("\(i + 1).\(s) \(s == 1 ? "First" : "Second") Part of Chapter \(ordinals[i])", bold, 72, y + 10)
                        y += 34
                    }
                    paragraph(&y, "page \(pno)")
                    paragraph(&y, "page \(pno) again")
                }
            }
        }
        marks.append(("Index", 1, pno, 90))
        let words = ["apples", "bananas", "cherries", "dates", "elderberries", "figs", "grapes", "honeydew"]
        for half in 0..<2 {
            page {
                var y: CGFloat = 100
                if half == 0 { draw("Index", big, 72, y); y += 40 } else { draw("\(pno - 2) Index", body, 72, 60) }
                for (k, w) in words.enumerated() {
                    draw("\(half == 0 ? w : w.uppercased()), \(3 + k), \(17 + k)", body, 72, y); y += 12
                    draw("\(w) pie, \(30 + k)", body, 300, y - 12)
                }
            }
        }
        if outline {
            // CGPDFContext's outline: "Destination" is the page number from 1, "DestinationRect" the point
            var tops: [[String: Any]] = []
            for (t, level, p, y) in marks {
                let item: [String: Any] = ["Title": t, "Destination": p + 1,
                                           "DestinationRect": CGRect(x: 72, y: 792 - y, width: 0, height: 0).dictionaryRepresentation]
                if level == 1 { tops.append(item) } else {
                    var last = tops.removeLast()
                    last["Children"] = ((last["Children"] as? [[String: Any]]) ?? []) + [item]
                    tops.append(last)
                }
            }
            CGPDFContextSetOutline(ctx, ["Children": tops] as CFDictionary)
        }
        ctx.closePDF()
        return data as Data
    }
}
