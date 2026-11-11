import Foundation
import SwiftUI
import MarginsCore
import Paper2EpubKit

extension Paper: Identifiable {}
extension PaperCollection: Identifiable {}

enum SidebarItem: Hashable {
    case all, starred, unread
    case collection(String)
    case tag(String)
}

/// App state shared by the Mac and iPhone apps: wraps the Rust `Library` and runs slow work
/// (metadata lookups, conversions, imports) off the main thread.
@MainActor
final class LibraryModel: ObservableObject {
    let lib: Library
    @Published var papers: [Paper] = []
    @Published var snippets: [String: String] = [:]
    @Published var collections: [PaperCollection] = []
    @Published var tags: [String] = []
    @Published var source: SidebarItem = .all { didSet { reloadList() } }
    @Published var query = "" { didSet { scheduleSearch() } }
    private var searchTask: Task<Void, Never>?
    @Published var sort: SortKey = .added { didSet { reloadList() } }
    @Published var status: String?
    @Published var progress: Double?
    @Published var errorMessage: String?
    @Published var counts = (all: 0, starred: 0, unread: 0)
    private var refreshTimer: Timer?
    private var converting = false

    static var libraryURL: URL {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("Margins/Library", isDirectory: true)
    }

    static var cacheURL: URL {
        FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("Margins", isDirectory: true)
    }

    static var deviceID: String {
        if let id = UserDefaults.standard.string(forKey: "deviceID") { return id }
        #if os(macOS)
        let prefix = "mac"
        #else
        let prefix = "ios"
        #endif
        let id = "\(prefix)-\(UUID().uuidString.prefix(8).lowercased())"
        UserDefaults.standard.set(id, forKey: "deviceID")
        return id
    }

    init() throws {
        lib = try Library.open(root: Self.libraryURL.path, cacheDir: Self.cacheURL.path, deviceId: Self.deviceID)
        reload()
        refreshTimer = Timer.scheduledTimer(withTimeInterval: 10, repeats: true) { [weak self] _ in
            Task { @MainActor in self?.refresh() }
        }
    }

    // MARK: - Reading the library

    /// Full refresh: list plus sidebar data (collections, tags, counts). Call after the library changes.
    func reload() {
        reloadList()
        collections = lib.collections()
        tags = lib.tags()
        let every = lib.papers(filter: PaperFilter(collection: nil, tag: nil, starredOnly: false, unreadOnly: false), sort: .added)
        counts = (every.count, every.filter(\.starred).count, every.filter { !$0.read }.count)
    }

    /// Typing in the search field: wait for a short pause, then search off the main thread.
    private func scheduleSearch() {
        searchTask?.cancel()
        searchTask = Task { [weak self] in
            try? await Task.sleep(nanoseconds: 150_000_000)
            guard !Task.isCancelled else { return }
            await self?.reloadList()
        }
    }

    /// Only the paper list (cheap): used for sidebar selection, sorting and search.
    func reloadList() {
        searchTask?.cancel()
        let filter = currentFilter()
        let q = query.trimmingCharacters(in: .whitespaces)
        let lib = self.lib
        let sort = self.sort
        if q.isEmpty {
            papers = lib.papers(filter: filter, sort: sort)
            snippets = [:]
            return
        }
        let filtered = !(filter.collection == nil && filter.tag == nil && !filter.starredOnly && !filter.unreadOnly)
        searchTask = Task { [weak self] in
            let result = await Task.detached { () -> ([Paper], [String: String]) in
                let hits = (try? lib.search(query: q, limit: 300)) ?? []
                let allowed: Set<String>? = filtered ? Set(lib.papers(filter: filter, sort: sort).map(\.id)) : nil
                let kept = allowed.map { a in hits.filter { a.contains($0.paper.id) } } ?? hits
                return (kept.map(\.paper), Dictionary(kept.map { ($0.paper.id, $0.snippet) }, uniquingKeysWith: { a, _ in a }))
            }.value
            guard !Task.isCancelled, let self, self.query.trimmingCharacters(in: .whitespaces) == q else { return }
            self.papers = result.0
            self.snippets = result.1
        }
    }

    private func currentFilter() -> PaperFilter {
        var filter = PaperFilter(collection: nil, tag: nil, starredOnly: false, unreadOnly: false)
        switch source {
        case .all: break
        case .starred: filter.starredOnly = true
        case .unread: filter.unreadOnly = true
        case .collection(let id): filter.collection = id
        case .tag(let t): filter.tag = t
        }
        return filter
    }

    var sourceTitle: String {
        switch source {
        case .all: return "All Papers"
        case .starred: return "Starred"
        case .unread: return "Unread"
        case .collection(let id): return collections.first { $0.id == id }?.name ?? "Collection"
        case .tag(let t): return "#\(t)"
        }
    }

    func refresh() {
        if (try? lib.refresh()) == true { reload() }
    }

    func paper(_ id: String) -> Paper? { lib.paper(id: id) }

    func fileURL(_ id: String, _ kind: FileKind) -> URL? {
        lib.filePath(id: id, kind: kind).map { URL(fileURLWithPath: $0) }
    }

    // MARK: - Editing

    func toggleStar(_ p: Paper) { try? lib.setStarred(id: p.id, starred: !p.starred); reload() }
    func setRead(_ p: Paper, _ read: Bool) { try? lib.setRead(id: p.id, read: read); reload() }
    func delete(_ ids: [String]) { for id in ids { try? lib.deletePaper(id: id) }; reload() }
    func setTag(_ p: Paper, _ tag: String, _ on: Bool) { try? lib.setTag(id: p.id, tag: tag, present: on); reload() }

    func newCollection(_ name: String) {
        guard !name.trimmingCharacters(in: .whitespaces).isEmpty else { return }
        _ = try? lib.createCollection(name: name, parent: nil)
        reload()
    }

    func setMembership(_ ids: [String], collection: String, _ on: Bool) {
        for id in ids { try? lib.setMembership(collection: collection, paper: id, present: on) }
        reload()
    }

    func citation(_ id: String, _ style: CitationStyle) -> String { lib.citation(id: id, style: style) ?? "" }
    func bibtex(_ ids: [String]) -> String { lib.exportBibtex(ids: ids) }
    func ris(_ ids: [String]) -> String { lib.exportRis(ids: ids) }

    // MARK: - Adding papers

    /// Adds a paper from a DOI / link / PMCID: converts it for reading when an open version exists,
    /// otherwise adds it with metadata only.
    func add(_ input: String) async {
        let input = input.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !input.isEmpty else { return }
        status = "Adding \(input)…"
        defer { status = nil }
        let lib = self.lib
        let meta = await Task.detached { try? resolveMetadata(input: input) }.value
        let tmp = FileManager.default.temporaryDirectory.appendingPathComponent("margins-add-\(UUID())")
        let conv = try? await Paper2Epub.convert(input, into: tmp)
        do {
            var m = meta ?? Self.meta(from: conv)
            if m.title.isEmpty, let c = conv { m = Self.meta(from: c) }
            guard !m.title.isEmpty || !m.doi.isEmpty else {
                errorMessage = "Couldn't find “\(input)”. Try a DOI or a link to the article."
                return
            }
            let epub = conv.flatMap { try? Data(contentsOf: $0.file) }
            let added = try await Task.detached { try lib.addPaper(meta: m, pdf: nil, epub: epub, fulltext: conv?.fulltext) }.value
            reload()
            if conv == nil { status = "Added “\(added.meta.title)” (no open full text found)" }
        } catch {
            errorMessage = error.localizedDescription
        }
        try? FileManager.default.removeItem(at: tmp)
    }

    /// Adds PDFs (dragged in or picked): matches them to existing papers by DOI, converts them for reading.
    func addPDFs(_ urls: [URL]) async {
        for (i, url) in urls.enumerated() {
            status = "Adding PDF \(i + 1) of \(urls.count)…"
            guard let data = try? Data(contentsOf: url) else { continue }
            await addPDF(data, name: url.lastPathComponent)
        }
        status = nil
        reload()
    }

    @discardableResult
    func addPDF(_ data: Data, name: String) async -> Paper? {
        let lib = self.lib
        let doi = Paper2Epub.doi(inPDF: data)
        if let doi, let existing = lib.findByDoi(doi: doi) {
            try? await Task.detached { try lib.attachFile(id: existing.id, kind: .pdf, data: data) }.value
            if !existing.hasEpub { await makeReadingView(existing.id) }
            return lib.paper(id: existing.id)
        }
        let tmp = FileManager.default.temporaryDirectory.appendingPathComponent("margins-pdf-\(UUID())")
        try? FileManager.default.createDirectory(at: tmp, withIntermediateDirectories: true)
        let file = tmp.appendingPathComponent(name.hasSuffix(".pdf") ? name : name + ".pdf")
        try? data.write(to: file)
        let conv = try? await Paper2Epub.convert(file: file, into: tmp)
        var meta = PaperMeta.empty
        if let doi { meta = (await Task.detached { try? resolveMetadata(input: doi) }.value) ?? meta }
        if meta.title.isEmpty, let conv { meta = Self.meta(from: conv) }
        if meta.title.isEmpty { meta.title = (name as NSString).deletingPathExtension }
        let m = meta
        let epub = conv.flatMap { try? Data(contentsOf: $0.file) }
        let p = try? await Task.detached { try lib.addPaper(meta: m, pdf: data, epub: epub, fulltext: conv?.fulltext) }.value
        try? FileManager.default.removeItem(at: tmp)
        return p
    }

    static func meta(from c: ConversionResult?) -> PaperMeta {
        guard let c else { return .empty }
        var m = PaperMeta.empty
        m.title = c.meta.title
        m.authors = c.meta.authors
        m.date = c.meta.date
        m.journal = c.meta.journal
        m.doi = c.meta.doi
        m.abstractText = c.meta.abstract
        m.license = c.meta.license
        m.keywords = c.meta.keywords
        return m
    }

    // MARK: - Import

    /// Imports a BibTeX / RIS export (e.g. ReadCube Papers' “Export to… → Bibliography file”),
    /// then optionally matches a folder of exported PDFs and builds reading views in the background.
    func importExport(file: URL, pdfFolder: URL?) async {
        status = "Importing \(file.lastPathComponent)…"
        let lib = self.lib
        do {
            let text = try String(contentsOf: file, encoding: .utf8)
            let report = try await Task.detached { try lib.importEntries(entries: parseImport(text: text)) }.value
            reload()
            status = "Imported \(report.added) new papers, merged \(report.merged)."
            if let pdfFolder { await importPDFFolder(pdfFolder) }
            await makeReadingViews()
        } catch {
            errorMessage = "Import failed: \(error.localizedDescription)"
            status = nil
        }
    }

    func importPDFFolder(_ folder: URL) async {
        let fm = FileManager.default
        let pdfs = (fm.enumerator(at: folder, includingPropertiesForKeys: nil)?.compactMap { $0 as? URL } ?? [])
            .filter { $0.pathExtension.lowercased() == "pdf" }
        var matched = 0, added = 0
        let lib = self.lib
        for (i, url) in pdfs.enumerated() {
            status = "Matching PDFs: \(i + 1) of \(pdfs.count)"
            progress = Double(i) / Double(max(1, pdfs.count))
            guard let data = try? Data(contentsOf: url) else { continue }
            if let doi = Paper2Epub.doi(inPDF: data), let p = lib.findByDoi(doi: doi) {
                if !p.hasPdf { try? await Task.detached { try lib.attachFile(id: p.id, kind: .pdf, data: data) }.value }
                matched += 1
            } else if let p = matchByTitle(url) {
                if !p.hasPdf { try? await Task.detached { try lib.attachFile(id: p.id, kind: .pdf, data: data) }.value }
                matched += 1
            } else {
                await addPDF(data, name: url.lastPathComponent)
                added += 1
            }
        }
        progress = nil
        reload()
        status = "PDFs: \(matched) matched to papers, \(added) added as new papers."
    }

    /// Exported PDFs are often named after the title; match on the normalised file name.
    private func matchByTitle(_ url: URL) -> Paper? {
        func norm(_ s: String) -> String { s.lowercased().filter { $0.isLetter || $0.isNumber } }
        let n = norm(url.deletingPathExtension().lastPathComponent)
        guard n.count >= 20 else { return nil }
        return lib.papers(filter: PaperFilter(collection: nil, tag: nil, starredOnly: false, unreadOnly: false), sort: .added)
            .first { let t = norm($0.meta.title); return !t.isEmpty && (t.hasPrefix(n) || n.hasPrefix(t) || n.contains(t)) }
    }

    // MARK: - Finding PDFs

    /// Looks for a legal open-access PDF (PMC, Europe PMC, OpenAlex, arXiv, preprint servers),
    /// attaches it and, if needed, builds the reading view from it. Returns the source, or nil.
    @discardableResult
    func findPDF(_ id: String) async -> String? {
        let lib = self.lib
        let source = await Task.detached { (try? lib.findPdf(id: id)) ?? nil }.value
        if source != nil, lib.paper(id: id)?.hasEpub == false { await makeReadingView(id) }
        reload()
        return source
    }

    /// Finds PDFs for every paper that has none, a few at a time.
    func findMissingPDFs() async {
        let todo = lib.papers(filter: PaperFilter(collection: nil, tag: nil, starredOnly: false, unreadOnly: false), sort: .added)
            .filter { !$0.hasPdf && (!$0.meta.doi.isEmpty || !$0.meta.pmcid.isEmpty || !$0.meta.arxiv.isEmpty) }
        guard !todo.isEmpty else { status = "Every paper already has a PDF."; return }
        var done = 0, found = 0
        await withTaskGroup(of: Bool.self) { group in
            var it = todo.makeIterator()
            for _ in 0..<4 { if let p = it.next() { group.addTask { await self.findPDF(p.id) != nil } } }
            for await ok in group {
                done += 1
                if ok { found += 1 }
                status = "Finding PDFs: \(done) of \(todo.count) (\(found) found)"
                progress = Double(done) / Double(todo.count)
                if let p = it.next() { group.addTask { await self.findPDF(p.id) != nil } }
            }
        }
        progress = nil
        status = "Found open-access PDFs for \(found) of \(todo.count) papers."
    }

    // MARK: - Reading views

    /// Makes the reflowable reading view for one paper: from its PDF if we have one (the converter
    /// still prefers an open-access XML version found via the PDF's DOI), else via PMCID / DOI.
    @discardableResult
    func makeReadingView(_ id: String) async -> Bool {
        guard let p = lib.paper(id: id) else { return false }
        let tmp = FileManager.default.temporaryDirectory.appendingPathComponent("margins-conv-\(UUID())")
        defer { try? FileManager.default.removeItem(at: tmp) }
        var result: ConversionResult?
        if let pdf = fileURL(id, .pdf) {
            result = try? await Paper2Epub.convert(file: pdf, into: tmp)
        } else {
            for input in [p.meta.pmcid, p.meta.doi] where !input.isEmpty {
                if let r = try? await Paper2Epub.convert(input, into: tmp) { result = r; break }
            }
        }
        guard let r = result, let epub = try? Data(contentsOf: r.file) else { return false }
        let lib = self.lib
        let text = Data(r.fulltext.utf8)
        try? await Task.detached {
            try lib.attachFile(id: id, kind: .epub, data: epub)
            try lib.attachFile(id: id, kind: .fulltext, data: text)
        }.value
        return true
    }

    /// Builds reading views for every paper that lacks one (PMC papers first), a few at a time.
    func makeReadingViews() async {
        guard !converting else { return }
        converting = true
        defer { converting = false; progress = nil }
        let all = lib.papers(filter: PaperFilter(collection: nil, tag: nil, starredOnly: false, unreadOnly: false), sort: .added)
        let todo = all.filter { !$0.hasEpub && ($0.hasPdf || !$0.meta.pmcid.isEmpty || !$0.meta.doi.isEmpty) }
            .sorted { (!$0.meta.pmcid.isEmpty ? 0 : 1, $0.hasPdf ? 0 : 1) < (!$1.meta.pmcid.isEmpty ? 0 : 1, $1.hasPdf ? 0 : 1) }
        guard !todo.isEmpty else { return }
        var done = 0, ok = 0
        await withTaskGroup(of: Bool.self) { group in
            var it = todo.makeIterator()
            for _ in 0..<4 { if let p = it.next() { group.addTask { await self.makeReadingView(p.id) } } }
            for await success in group {
                done += 1
                if success { ok += 1 }
                status = "Making reading views: \(done) of \(todo.count) (\(ok) ready)"
                progress = Double(done) / Double(todo.count)
                if done % 10 == 0 { reload() }
                if let p = it.next() { group.addTask { await self.makeReadingView(p.id) } }
            }
        }
        reload()
        status = "Reading views ready for \(ok) of \(todo.count) papers. The rest have no open full text yet: add their PDFs."
    }
}

extension PaperMeta {
    static var empty: PaperMeta {
        PaperMeta(title: "", authors: [], date: "", journal: "", volume: "", issue: "", pages: "", publisher: "",
                  doi: "", pmid: "", pmcid: "", arxiv: "", url: "", abstractText: "", keywords: [], license: "")
    }

    var authorLine: String {
        switch authors.count {
        case 0: return ""
        case 1: return authors[0]
        case 2: return "\(authors[0]) & \(authors[1])"
        default: return "\(authors[0]) et al."
        }
    }

    var year: String { String(date.prefix(4)) }
}
