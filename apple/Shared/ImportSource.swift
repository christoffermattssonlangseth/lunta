import Foundation
import UniformTypeIdentifiers
import LuntaCore
import Paper2EpubKit

extension UTType {
    /// BibTeX (.bib): declared by the iPhone and iPad app (UTImportedTypeDeclarations), else by extension.
    static let bibtex = UTType("org.tug.bib") ?? UTType(filenameExtension: "bib", conformingTo: .plainText) ?? .plainText
    /// RIS (.ris, application/x-research-info-systems).
    static let ris = UTType("com.cmlangseth.margins.ris") ?? UTType(filenameExtension: "ris", conformingTo: .plainText) ?? .plainText
}

/// What was picked to import a library from: an export file (.bib / .ris), a folder with one in it
/// (Zotero's "Export Files": the .bib and files/12/paper.pdf), or a ZIP of either.
struct ImportSource: Sendable {
    /// The .bib or .ris file.
    let export: URL
    /// The folder or unpacked ZIP that was picked (nil for a single file): the export's PDF paths are
    /// looked for in it too, and its other PDFs are matched to the papers by DOI or title.
    let root: URL?
    /// An unpacked ZIP, removed when the import is done.
    let temporary: URL?

    /// The folder name to suggest: an exported collection keeps its name ("Thesis.bib" → "Thesis").
    var suggestedFolder: String { export.deletingPathExtension().lastPathComponent }

    /// What the file picker offers.
    static let pickable: [UTType] = [.bibtex, .ris, .plainText, .folder, .zip]

    enum Failure: LocalizedError {
        case noExport(String)
        var errorDescription: String? {
            switch self {
            case .noExport(let name):
                return "“\(name)” has no BibTeX (.bib) or RIS (.ris) export in it. In Zotero, use File › Export Library… with the format BibTeX and Export Files ticked, then choose the folder it makes."
            }
        }
    }

    /// Opens what was picked. Call off the main thread, with access to `url` (its security scope):
    /// a ZIP is unpacked into a temporary folder, files in iCloud Drive are downloaded.
    static func open(_ url: URL) throws -> ImportSource {
        let isDir = (try? url.resourceValues(forKeys: [.isDirectoryKey]))?.isDirectory == true
        if isDir {
            guard let export = findExport(in: url) else { throw Failure.noExport(url.lastPathComponent) }
            return ImportSource(export: export, root: url, temporary: nil)
        }
        if url.pathExtension.lowercased() == "zip" {
            let dir = FileManager.default.temporaryDirectory.appendingPathComponent("import-\(UUID().uuidString)")
            do {
                try coordinated(url) { try ZipReader.extract($0, to: dir) }
                guard let export = findExport(in: dir) else { throw Failure.noExport(url.lastPathComponent) }
                return ImportSource(export: export, root: dir, temporary: dir)
            } catch {
                try? FileManager.default.removeItem(at: dir)
                throw error
            }
        }
        return ImportSource(export: url, root: nil, temporary: nil)
    }

    /// The export in a folder: a .bib, .ris (or an EndNote .txt that is RIS), the one nearest the top
    /// (Zotero puts it next to its "files" folder), BibTeX before RIS, then the largest.
    static func findExport(in folder: URL) -> URL? {
        let keys: [URLResourceKey] = [.isRegularFileKey, .fileSizeKey]
        guard let walk = FileManager.default.enumerator(at: folder, includingPropertiesForKeys: keys, options: [.skipsHiddenFiles, .skipsPackageDescendants]) else { return nil }
        let depth0 = folder.standardizedFileURL.pathComponents.count
        var found: [(url: URL, depth: Int, kind: Int, size: Int)] = []
        for case let u as URL in walk {
            let ext = u.pathExtension.lowercased()
            guard ["bib", "bibtex", "ris", "txt"].contains(ext),
                  let v = try? u.resourceValues(forKeys: Set(keys)), v.isRegularFile == true else { continue }
            let kind: Int
            switch ext {
            case "bib", "bibtex": kind = 0
            case "ris": kind = 1
            default:
                // a .txt only if it starts like an export ("TY  - " or "@article{")
                guard let h = try? FileHandle(forReadingFrom: u), let head = try? h.read(upToCount: 4096) else { continue }
                try? h.close()
                let s = String(decoding: head, as: UTF8.self)
                guard s.contains("TY  - ") || s.range(of: #"@\w+\s*\{"#, options: .regularExpression) != nil else { continue }
                kind = 2
            }
            found.append((u, u.standardizedFileURL.pathComponents.count - depth0, kind, v.fileSize ?? 0))
        }
        return found.min { ($0.depth, $0.kind, -$0.size) < ($1.depth, $1.kind, -$1.size) }?.url
    }

    /// Where a PDF path from an export is on this device: the path itself, the path relative to one of
    /// `roots` (Zotero: "files/12/paper.pdf"), or its last folders under a root (a path from another
    /// computer, "/Users/me/Zotero/storage/AB12CD/paper.pdf" → "storage/AB12CD/paper.pdf").
    static func locate(_ path: String, in roots: [URL]) -> URL? {
        let fm = FileManager.default
        if fm.fileExists(atPath: path) { return URL(fileURLWithPath: path) }
        let parts = path.split(separator: "/").map(String.init).filter { !$0.isEmpty && $0 != "~" && $0 != ".." }
        guard !parts.isEmpty else { return nil }
        for k in stride(from: parts.count, through: 1, by: -1) {
            let tail = parts.suffix(k)
            for root in roots {
                let u = tail.reduce(root) { $0.appendingPathComponent($1) }
                if fm.fileExists(atPath: u.path) { return u }
            }
        }
        return nil
    }

    /// Reads an export and finds its PDFs on this device (downloading them from iCloud Drive if needed);
    /// the entries' PDF paths become the files found. `found`: their resolved paths; `missing`: entries
    /// whose PDFs weren't found. Blocks: off the main thread.
    static func read(_ file: URL, roots: [URL]) throws -> (entries: [ImportEntry], found: Set<String>, missing: Int) {
        let text = try readText(file)
        var found = Set<String>()
        var missing = 0
        let entries = parseImport(text: text).map { e -> ImportEntry in
            var e = e
            var any = false
            e.files = e.files.map { path in
                guard let u = locate(path, in: roots) else { return path }
                download(u)
                found.insert(u.resolvingSymlinksInPath().path)
                any = true
                return u.path
            }
            if !e.files.isEmpty && !any { missing += 1 }
            return e
        }
        return (entries, found, missing)
    }

    /// The export's text: UTF-8, else Latin-1 (older EndNote and RIS files).
    static func readText(_ url: URL) throws -> String {
        try coordinated(url) { u in
            let data = try Data(contentsOf: u)
            return String(data: data, encoding: .utf8) ?? String(data: data, encoding: .isoLatin1) ?? String(decoding: data, as: UTF8.self)
        }
    }

    /// Makes sure a file is on the device (a file in iCloud Drive or another provider may only be a
    /// placeholder): a coordinated read waits for it to download. Blocks: not on the main thread.
    static func download(_ url: URL) {
        try? coordinated(url) { _ in }
    }

    static func coordinated<T>(_ url: URL, _ body: (URL) throws -> T) throws -> T {
        var coordError: NSError?
        var result: Result<T, Error>?
        NSFileCoordinator().coordinate(readingItemAt: url, options: .withoutChanges, error: &coordError) { u in
            result = Result { try body(u) }
        }
        if let result { return try result.get() }
        throw coordError ?? CocoaError(.fileReadUnknown)
    }
}

/// The counts at the end of an import, for the status line and the summary.
struct ImportSummary: Sendable {
    var added = 0, merged = 0, skipped = 0, pdfs = 0
    /// More PDFs from the picked folder, matched to papers by DOI or title.
    var matched = 0
    /// Papers whose export names a PDF that wasn't found.
    var missingPDFs = 0
    var folder: String?

    var text: String {
        var s = "Added \(added) \(added == 1 ? "paper" : "papers")"
        if merged > 0 { s += ", merged \(merged) with papers already in Lunta" }
        s += ", \(pdfs + matched) \(pdfs + matched == 1 ? "PDF" : "PDFs")."
        if let folder, !folder.isEmpty { s += " In the folder “\(folder)”." }
        if skipped > 0 { s += " \(skipped) without a title or DOI left out." }
        if missingPDFs > 0 { s += " \(missingPDFs) PDFs named in the export weren't found." }
        return s
    }
}
