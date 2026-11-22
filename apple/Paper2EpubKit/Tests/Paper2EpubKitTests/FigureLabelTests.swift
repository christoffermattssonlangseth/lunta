import CoreGraphics
import XCTest
@testable import Paper2EpubKit

/// Figure, table and box labels: books number them by chapter ("Figure 1.2", "Fig. 3.4", an
/// appendix's "Figure A.1", "Figure S1.2", "Figure 3-4"), and a sentence that mentions one at the
/// start of a line ("Figure 1.2 depicts…") is no caption.
final class FigureLabelTests: XCTestCase {

    func testCaptionsWithChapterNumbers() {
        for c in ["Figure 1.2 Regression and classification problems.", "Figure 10.14. Foo", "Fig. 3.4 | Bar", "Figure A.1 Appendix",
                  "Figure S1.2: x", "Table 2.1 Results", "Figure 3-4 Title", "Figure 1.4a The model", "Box 1.1 Notation",
                  "Figure 10.2 1D convolution with kernel size three.",
                  // and the forms papers use
                  "Figure 2. Foo", "Fig. 5 | Bar", "Figure 3", "Extended Data Fig. 2 | X", "Figure 1 Overview", "Table S1: x",
                  "Figure 1—The model", "Figure 2 - Overview", "Figure 1. Overview", "Figure 2B Panels", "FIG. 1."] {
            XCTAssertTrue(PDFAnalyzer.captionRE.test(c), c)
        }
    }

    func testRunningTextIsNoCaption() {
        for c in ["Figure 1.2 depicts several regression problems", "Figure 1.4a depicts a model", "Fig. 3.4 shows", "Figure A.1 is",
                  "Figures 3.4–3.6 show", "Fig. 4–6) and", "Figure 2.1, which", "Figure 2 3 times",
                  "Figure S2G). Top genes", "Fig. 1a and b show", "Figure 2, which", "Figure 4) mapped"] {
            XCTAssertFalse(PDFAnalyzer.captionRE.test(c), c)
        }
    }

    func testFigureKeysTakeTheWholeNumber() {
        XCTAssertEqual(PDFBook.figureKey("Figure 1.2 Regression and classification"), "fig 1.2")
        XCTAssertEqual(PDFBook.figureKey("Figure 10.14."), "fig 10.14")
        XCTAssertEqual(PDFBook.figureKey("Fig. 3-4 | Title"), "fig 3.4")
        XCTAssertEqual(PDFBook.figureKey("Figure A.1 Appendix"), "fig A.1")
        XCTAssertEqual(PDFBook.figureKey("Figure S1.2"), "supplementary fig 1.2")
        XCTAssertEqual(PDFBook.figureKey("Figure 1. Overview"), "fig 1")
        XCTAssertEqual(PDFBook.figureKey("Fig. 3 | Title"), "fig 3")
        XCTAssertEqual(PDFBook.figureKey("Extended Data Fig. 2 | See next page"), "extended data fig 2")
        XCTAssertEqual(PDFBook.figureKey("Supplementary Figure S1"), "supplementary fig 1")
        XCTAssertEqual(PDFBook.figureKey("Figure S4"), "supplementary fig 4")
        XCTAssertNil(PDFBook.figureKey("Table 1"))
    }

    func testMarginCaptionIsNearerThanTheParagraphBelow() {
        // the textbook's page 16: figure 1.1 on the right, its caption level with it in the left margin,
        // the body text below the figure
        let fig = R(x0: 328, y0: 138, x1: 518, y1: 265)
        let side = R(x0: 145, y0: 142, x1: 307, y1: 211)
        let below = R(x0: 134, y0: 313, x1: 515, y1: 383)
        XCTAssertLessThan(PDFBook.captionGap(side, fig), PDFBook.captionGap(below, fig))
        XCTAssertEqual(PDFBook.captionGap(side, fig), 21)
        XCTAssertEqual(PDFBook.captionGap(R(x0: 330, y0: 270, x1: 518, y1: 300), fig), 5)
    }
}
