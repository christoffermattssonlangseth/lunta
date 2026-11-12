import XCTest
import LuntaCore

/// Calls the Rust core through the generated Swift bindings.
final class CoreTests: XCTestCase {
    func testAddSearchAnnotateExport() throws {
        let tmp = FileManager.default.temporaryDirectory.appendingPathComponent("lunta-swift-\(UUID())")
        let lib = try Library.open(root: tmp.appendingPathComponent("lib").path,
                                   cacheDir: tmp.appendingPathComponent("cache").path, deviceId: "test-mac")
        var meta = PaperMeta(title: "Mapping cells through time and space with moscot", authors: ["Dominik Klein", "Fabian J. Theis"],
                             date: "2025", journal: "Nature", volume: "638", issue: "", pages: "1065-1075", publisher: "",
                             doi: "10.1038/s41586-024-08453-2", pmid: "", pmcid: "", arxiv: "", url: "",
                             abstractText: "Optimal transport for single-cell genomics.", keywords: [], license: "")
        let p = try lib.addPaper(meta: meta, pdf: nil, epub: Data("PK".utf8), fulltext: "Gromov-Wasserstein couplings across time points")
        XCTAssertTrue(p.hasEpub)
        XCTAssertEqual(try lib.search(query: "wasserstein", limit: 10).count, 1)

        let a = try lib.saveAnnotation(a: Annotation(id: "", paperId: p.id, kind: .highlight, color: "yellow",
                                                     location: "epub:ch02.xhtml", position: "{}", quote: "couplings",
                                                     prefix: "", suffix: "", note: "check", created: 0, modified: 0))
        XCTAssertFalse(a.id.isEmpty)
        XCTAssertEqual(lib.annotations(paper: p.id).count, 1)
        XCTAssertTrue(lib.exportBibtex(ids: [p.id]).contains("@article{Klein2025mapping,"))
        XCTAssertEqual(lib.citation(id: p.id, style: .vancouver),
                       "Klein D, Theis FJ. Mapping cells through time and space with moscot. Nature. 2025;638:1065-1075. doi:10.1038/s41586-024-08453-2")
        meta.title = "x"
        XCTAssertEqual(extractDoi(text: "see https://doi.org/10.1038/s41586-024-08453-2"), "10.1038/s41586-024-08453-2")
    }
}
