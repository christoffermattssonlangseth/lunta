import Foundation
import PDFKit

/// Entry point: resolve the input, build the book, write the EPUB.
public enum Paper2Epub {
    public typealias Log = (String) -> Void

    /// Converts a link / DOI / PMCID / PubMed ID.
    public static func convert(_ input: String, into dir: URL, log: @escaping Log = { _ in }) async throws -> ConversionResult {
        let cands = try Resolver.candidates(for: input, log: log)
        return try await run(cands, into: dir, log: log)
    }

    /// Converts a file (PDF or JATS XML) handed to the app.
    public static func convert(file: URL, into dir: URL, log: @escaping Log = { _ in }) async throws -> ConversionResult {
        let cands = try Resolver.candidates(forFile: file, log: log)
        return try await run(cands, into: dir, log: log)
    }

    static func run(_ cands: [Candidate], into dir: URL, log: @escaping Log) async throws -> ConversionResult {
        var errors: [String] = []
        for c in cands {
            try Task.checkCancellation()
            let src: Source
            do { src = try await c.load() } catch {
                errors.append("\(c.label): \(error.localizedDescription)")
                log("  ✗ \(c.label): \(error.localizedDescription)")
                continue
            }
            let book = try await build(src, log: log)
            return try write(book, into: dir, log: log)
        }
        if errors.count == 1 { throw fail(errors[0].components(separatedBy: ": ").dropFirst().joined(separator: ": ")) }
        throw fail("No usable source found.\n" + errors.joined(separator: "\n"))
    }

    static func build(_ src: Source, log: @escaping Log) async throws -> Book {
        switch src {
        case .pdf(let data, let meta, _):
            // An open-access XML version beats any PDF analysis: check for one via the PDF's DOI first.
            if let doc = PDFDocument(data: data), let doi = PDFAnalyzer.quickDOI(doc) {
                log("PDF has DOI \(doi); checking for an XML version")
                for c in Resolver.doiCandidates(doi, log: log) {
                    if let src = try? await c.load(), case .jats = src { return try await build(src, log: log) }
                }
                log("No XML version; converting the PDF itself")
            }
            return try await PDFBook.build(data, meta: meta, log: log)
        case .jats(let data, let assets, let extraMeta, let label):
            log("Converting \(label)")
            let root = try XML.parse(data)
            let hrefs = JATSConverter.graphicHrefs(root)
            let images = await AssetFetcher.fetch(hrefs: hrefs, from: assets, log: log)
            let conv = JATSConverter(root: root, images: images)
            var meta = conv.meta()
            meta.merge(extraMeta)
            var chapters = conv.chapters()
            let hasBody = chapters.contains { !$0.headings.contains { h in h.id.hasPrefix("abstract") } }
            if !hasBody {
                throw fail("The XML only contains the abstract, not the full text.")
            }
            if !chapters.contains(where: { $0.headings.first?.id.hasPrefix("abstract") == true }), !meta.abstract.isEmpty {
                chapters.insert(Chapter(title: "Abstract",
                                        html: "<section><h1 id=\"abstract\">Abstract</h1><p>\(esc(meta.abstract))</p></section>",
                                        headings: [Heading(level: 1, id: "abstract", title: "Abstract")]), at: 0)
            }
            var notes: [String] = []
            if conv.missingFigures > 0 {
                notes.append("\(conv.missingFigures) figure image(s) could not be downloaded; captions are kept.")
            }
            if conv.texFormulas > 0 {
                notes.append("\(conv.texFormulas) formula(s) only exist as TeX and are shown as source code.")
            }
            // keep only images actually referenced
            let allHTML = chapters.map(\.html).joined()
            let imgs = images.values.filter { allHTML.contains("../images/\($0.fileName)\"") }
                .sorted { $0.fileName < $1.fileName }
            return Book(meta: meta, chapters: chapters, images: imgs, sourceLabel: label, notes: notes)
        }
    }

    static func write(_ book: Book, into dir: URL, log: @escaping Log) throws -> ConversionResult {
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        var name = slug(book.meta.title, max: 70)
        if name.isEmpty { name = slug(book.meta.doi) }
        if name.isEmpty { name = "paper" }
        let url = dir.appendingPathComponent("\(name).epub")
        log("Writing EPUB")
        try EPUBWriter.write(book, cover: Cover.make(book.meta), to: url)
        let words = book.chapters.reduce(0) { n, ch in
            n + ch.html.replacingOccurrences(of: "<[^>]+>", with: " ", options: .regularExpression)
                .split(whereSeparator: { $0.isWhitespace }).count
        }
        let text = book.chapters.map { ch in
            ch.html.replacingOccurrences(of: "</(p|h[1-6]|li|figcaption|div)>", with: "\n", options: .regularExpression)
                .replacingOccurrences(of: "<[^>]+>", with: " ", options: .regularExpression)
                .replacingOccurrences(of: "&lt;", with: "<").replacingOccurrences(of: "&gt;", with: ">")
                .replacingOccurrences(of: "&quot;", with: "\"").replacingOccurrences(of: "&amp;", with: "&")
                .replacingOccurrences(of: "[ \t]+", with: " ", options: .regularExpression)
        }.joined(separator: "\n")
        return ConversionResult(file: url, title: book.meta.title, source: book.sourceLabel, words: words,
                                images: book.images.count, notes: book.notes, meta: book.meta, fulltext: text)
    }
}

extension Paper2Epub {
    /// The DOI printed in a PDF (first pages, last page, document info), if any.
    public static func doi(inPDF data: Data) -> String? {
        guard let doc = PDFDocument(data: data) else { return nil }
        return PDFAnalyzer.quickDOI(doc)
    }
}

extension Paper2Epub {
    /// Plain text of the first pages of a PDF (for matching a PDF to a known paper by its title).
    public static func firstPagesText(ofPDF data: Data, pages: Int = 2) -> String {
        guard let doc = PDFDocument(data: data) else { return "" }
        return (0..<min(pages, doc.pageCount)).compactMap { doc.page(at: $0)?.string }.joined(separator: " ")
    }
}
