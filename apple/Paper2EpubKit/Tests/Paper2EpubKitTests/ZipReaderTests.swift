import XCTest
@testable import Paper2EpubKit

/// Unpacking ZIP archives (a Zotero export zipped and sent to an iPad).
final class ZipReaderTests: XCTestCase {
    private var dir: URL!

    override func setUpWithError() throws {
        dir = FileManager.default.temporaryDirectory.appendingPathComponent("zipreader-\(UUID().uuidString)")
    }

    override func tearDownWithError() throws {
        try? FileManager.default.removeItem(at: dir)
    }

    func testStoredAndDeflatedEntriesInFolders() throws {
        let bib = "@article{a, title={A paper}, file={files/1/a.pdf}}\n"
        let pdf = Data((0..<5000).map { UInt8($0 % 7) })   // compressible: stored deflated
        var w = ZipWriter()
        w.add("Exported Items/Exported Items.bib", Data(bib.utf8))
        w.add("Exported Items/files/1/a.pdf", pdf)
        w.add("Exported Items/files/2/tiny.txt", Data("x".utf8), compress: false)
        let files = try ZipReader.extract(w.finish(), to: dir)
        XCTAssertEqual(files.count, 3)
        let root = dir.appendingPathComponent("Exported Items")
        XCTAssertEqual(try String(contentsOf: root.appendingPathComponent("Exported Items.bib"), encoding: .utf8), bib)
        XCTAssertEqual(try Data(contentsOf: root.appendingPathComponent("files/1/a.pdf")), pdf)
        XCTAssertEqual(try Data(contentsOf: root.appendingPathComponent("files/2/tiny.txt")), Data("x".utf8))
    }

    func testUnsafeAndMacOSEntriesAreLeftOut() throws {
        var w = ZipWriter()
        w.add("../evil.txt", Data("no".utf8))
        w.add("a/../../evil2.txt", Data("no".utf8))
        w.add("/abs.txt", Data("no".utf8))
        w.add("__MACOSX/Export/._a.bib", Data("fork".utf8))
        w.add("Export/._a.bib", Data("fork".utf8))
        w.add("Export/", Data())
        w.add("Export/a.bib", Data("@article{a,title={T}}".utf8))
        let files = try ZipReader.extract(w.finish(), to: dir)
        XCTAssertEqual(files.map(\.lastPathComponent), ["a.bib"])
        XCTAssertFalse(FileManager.default.fileExists(atPath: dir.deletingLastPathComponent().appendingPathComponent("evil.txt").path))
    }

    #if os(macOS)
    /// An archive made the way Finder's Compress makes one (data descriptors, __MACOSX).
    func testArchiveMadeByDitto() throws {
        let src = dir.appendingPathComponent("src/Thesis")
        try FileManager.default.createDirectory(at: src.appendingPathComponent("files/12"), withIntermediateDirectories: true)
        let pdf = Data((0..<300_000).map { UInt8(truncatingIfNeeded: $0 &* 2654435761 >> 13) })
        try pdf.write(to: src.appendingPathComponent("files/12/paper.pdf"))
        try Data("@article{a,title={T},file={files/12/paper.pdf}}".utf8).write(to: src.appendingPathComponent("Thesis.bib"))
        let zip = dir.appendingPathComponent("Thesis.zip")
        let p = Process()
        p.executableURL = URL(fileURLWithPath: "/usr/bin/ditto")
        p.arguments = ["-c", "-k", "--sequesterRsrc", "--keepParent", src.path, zip.path]
        try p.run(); p.waitUntilExit()
        XCTAssertEqual(p.terminationStatus, 0)
        let out = dir.appendingPathComponent("out")
        try ZipReader.extract(zip, to: out)
        XCTAssertEqual(try Data(contentsOf: out.appendingPathComponent("Thesis/files/12/paper.pdf")), pdf)
        XCTAssertTrue(FileManager.default.fileExists(atPath: out.appendingPathComponent("Thesis/Thesis.bib").path))
    }
    #endif

    func testNotAZip() {
        XCTAssertThrowsError(try ZipReader.extract(Data("@article{a,title={T}}".utf8), to: dir))
    }
}
