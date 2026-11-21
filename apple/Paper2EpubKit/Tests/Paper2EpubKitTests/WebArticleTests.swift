import XCTest
@testable import Paper2EpubKit

/// A web page's reading view: the images the core marked, put in or left out (as the core's
/// `web_article_with_images`), and the book written without network when there are none.
final class WebArticleTests: XCTestCase {
    func testImagesInOrLeftOut() {
        let html = #"<section><h1 id="title">Post</h1><p>Text <img src="lunta-image:0" alt="a curve"/></p><figure><img src="lunta-image:1" alt=""/></figure></section>"#
        let out = Paper2Epub.withImages(html, ["web1.png", nil])
        XCTAssertEqual(out, #"<section><h1 id="title">Post</h1><p>Text <img src="../images/web1.png" alt="a curve"/></p></section>"#)
    }

    func testBookWithoutImages() async throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("p2e-web-\(UUID())")
        defer { try? FileManager.default.removeItem(at: dir) }
        let ch: Paper2Epub.WebChapter = (title: "A Recipe", html: #"<section><h1 id="title">A Recipe</h1><p>Some words here.</p><pre><code class="language-python">x = 1</code></pre></section>"#,
                                         headings: [(level: 1, id: "title", title: "A Recipe")])
        let r = try await Paper2Epub.convert(webPage: [ch], imageURLs: [], title: "A Recipe", authors: ["Andrej Karpathy"], date: "2019-04-25",
                                             site: "Andrej Karpathy blog", url: "https://karpathy.github.io/2019/04/25/recipe/", into: dir)
        XCTAssertTrue(FileManager.default.fileExists(atPath: r.file.path))
        XCTAssertTrue(r.fulltext.contains("x = 1"))
        XCTAssertEqual(r.source, "Web page from karpathy.github.io")
    }
}
