import Foundation
import SwiftUI
#if os(macOS)
import IOKit
#else
import UIKit
#endif
import LuntaCore
import Paper2EpubKit

extension Paper: Identifiable {}
extension PaperCollection: Identifiable {}

enum SidebarItem: Hashable {
    case all, starred, mustRead, unread
    case discover
    case alerts
    case collection(String)
    case tag(String)
    case smart(String)
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
    @Published var smartFolders: [SmartFolder] = []
    var importingShared = false
    @Published var source: SidebarItem = .all { didSet { reloadList() } }
    @Published var query = "" { didSet { scheduleSearch() } }
    private var searchTask: Task<Void, Never>?
    @Published var sort: SortKey = .added { didSet { reloadList() } }
    @Published var status: String?
    @Published var progress: Double?
    @Published var errorMessage: String?
    @Published var counts = (all: 0, starred: 0, mustRead: 0, unread: 0)
    // Discover: searching beyond the library (OpenAlex)
    @Published var discoverQuery = ""
    @Published var discoverResults: [DiscoverHit] = []
    @Published var discovering = false
    @Published var discoverError: String?
    private var refreshTimer: Timer?
    private var converting = false

    let libraryURL: URL
    let inCloud: Bool

    nonisolated static var cacheURL: URL {
        if let t = testLibrary { return t.appendingPathComponent("Cache", isDirectory: true) }
        return FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("Margins", isDirectory: true)
    }

    /// Test hook (Debug builds only): `-testLibrary <folder>` opens a scratch library in
    /// <folder>/Library with its own cache, device id and backups, and leaves the real library,
    /// iCloud and the background services (Drive copy, feeds, citation add-ins) alone.
    #if DEBUG
    nonisolated static let testLibrary: URL? = {
        let a = ProcessInfo.processInfo.arguments
        guard let i = a.firstIndex(of: "-testLibrary"), i + 1 < a.count else { return nil }
        return URL(fileURLWithPath: a[i + 1], isDirectory: true)
    }()
    #else
    nonisolated static let testLibrary: URL? = nil
    #endif

    /// This device's id ("mac-1a2b3c4d"), which names its change log and reading-position file.
    /// Settings travel in backups (a new iPhone restored from the old one, Migration Assistant), so
    /// the id is tied to this machine: on another one a new id is made, or two devices in use would
    /// write the same files.
    nonisolated static var deviceID: String {
        if testLibrary != nil { return "mac-testlib" }
        deviceIDLock.lock(); defer { deviceIDLock.unlock() }
        let d = UserDefaults.standard, host = hardwareID
        if let id = d.string(forKey: "deviceID") {
            let owner = d.string(forKey: "deviceIDHost")
            if owner == nil, let host { d.set(host, forKey: "deviceIDHost") }   // made before this check
            if owner == nil || owner == host || host == nil { return id }
        }
        #if os(macOS)
        let prefix = "mac"
        #else
        let prefix = "ios"
        #endif
        let id = "\(prefix)-\(UUID().uuidString.prefix(8).lowercased())"
        d.set(id, forKey: "deviceID")
        if let host { d.set(host, forKey: "deviceIDHost") }
        return id
    }
    nonisolated private static let deviceIDLock = NSLock()

    /// Identifies this machine (not carried over by backups or migration).
    nonisolated private static var hardwareID: String? {
        #if os(macOS)
        let service = IOServiceGetMatchingService(kIOMainPortDefault, IOServiceMatching("IOPlatformExpertDevice"))
        defer { IOObjectRelease(service) }
        return IORegistryEntryCreateCFProperty(service, "IOPlatformUUID" as CFString, kCFAllocatorDefault, 0)?.takeRetainedValue() as? String
        #else
        return UIDevice.current.identifierForVendor?.uuidString
        #endif
    }

    /// Opens the Rust library (reads all change logs, updates the search index). Slow on first
    /// launch, so call it off the main thread.
    nonisolated static func openLibrary(root: URL) throws -> Library {
        try Library.open(root: root.path, cacheDir: cacheURL.path, deviceId: deviceID)
    }

    convenience init(root: URL, inCloud: Bool) throws {
        self.init(lib: try Self.openLibrary(root: root), root: root, inCloud: inCloud)
    }

    init(lib: Library, root: URL, inCloud: Bool) {
        libraryURL = root
        self.inCloud = inCloud
        self.lib = lib
        downloadSmallFiles()
        indexFullTextInBackground()
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
        loadSmartFolders()
        let every = lib.papers(filter: PaperFilter(collection: nil, tag: nil, starredOnly: false, unreadOnly: false), sort: .added)
        counts = (every.count, every.filter(\.starred).count, every.filter(\.mustRead).count, every.filter { !$0.read }.count)
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
        if source == .discover || source == .alerts { return }
        let filter = currentFilter()
        let q = query.trimmingCharacters(in: .whitespaces)
        let lib = self.lib
        let sort = self.sort
        if case .smart(let sid) = source, let sf = smartFolder(sid) {
            let all = sf.papers(lib, sort: sort)
            if q.isEmpty { papers = all; snippets = [:]; return }
            let allowed = Set(all.map(\.id))
            let hits = ((try? lib.search(query: q, limit: 300)) ?? []).filter { allowed.contains($0.paper.id) }
            papers = hits.map(\.paper)
            snippets = Dictionary(hits.map { ($0.paper.id, $0.snippet) }, uniquingKeysWith: { a, _ in a })
            return
        }
        if q.isEmpty {
            papers = lib.papers(filter: filter, sort: sort)
            snippets = [:]
            return
        }
        let filtered = !(filter.collection == nil && filter.tag == nil && !filter.starredOnly && !filter.unreadOnly && !filter.mustReadOnly)
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
        case .mustRead: filter.mustReadOnly = true
        case .unread: filter.unreadOnly = true
        case .discover, .alerts: break
        case .collection(let id): filter.collection = id
        case .tag(let t): filter.tag = t
        case .smart: break
        }
        return filter
    }

    var sourceTitle: String {
        switch source {
        case .all: return "All Papers"
        case .starred: return "Starred"
        case .mustRead: return "Must Read"
        case .unread: return "Unread"
        case .discover: return "Discover"
        case .alerts: return "New for You"
        case .collection(let id): return collections.first { $0.id == id }?.name ?? "Collection"
        case .tag(let t): return "#\(t)"
        case .smart(let id): return smartFolder(id)?.name ?? "Smart Folder"
        }
    }

    /// Picks up changes written by agents (lunta-mcp) or synced from other devices while the app is open:
    /// checks the change logs' sizes every few seconds and reloads only when one grew.
    func watchForOutsideChanges() {
        let ops = libraryURL.appendingPathComponent("ops")
        var last: [String: Int] = [:]
        Timer.scheduledTimer(withTimeInterval: 3, repeats: true) { [weak self] _ in
            var now: [String: Int] = [:]
            for f in (try? FileManager.default.contentsOfDirectory(at: ops, includingPropertiesForKeys: [.fileSizeKey])) ?? [] {
                now[f.lastPathComponent] = (try? f.resourceValues(forKeys: [.fileSizeKey]).fileSize) ?? 0
            }
            guard now != last else { return }
            let first = last.isEmpty
            last = now
            if !first { Task { @MainActor in self?.refreshFromDisk() } }
        }
    }

    private func refreshFromDisk() {
        if (try? lib.refresh()) == true { reload() }
    }

    func refresh() {
        downloadSmallFiles()
        if (try? lib.refresh()) == true { reload() }
    }

    /// iCloud: fetch change logs and full texts eagerly (small); PDFs and reading views on demand.
    /// Off the main thread: it lists every paper's folder, and listing an iCloud folder can hang
    /// while iCloud is busy (which froze the whole app, quitting included).
    private func downloadSmallFiles() {
        guard inCloud, !scanningPlaceholders else { return }
        scanningPlaceholders = true
        let lib = self.lib
        Task.detached(priority: .utility) { [weak self] in
            for p in lib.pendingDownloads() where p.contains("/ops/") || p.hasSuffix(".fulltext.txt.icloud") {
                CloudLibrary.startDownload(CloudLibrary.realURL(forPlaceholder: p))
            }
            await MainActor.run { self?.scanningPlaceholders = false }
        }
    }
    private var scanningPlaceholders = false

    /// Makes sure a paper's file is on this device (downloading it from iCloud if needed).
    func ensureLocal(_ id: String, _ kind: FileKind) async -> URL? {
        guard lib.fileStatus(id: id, kind: kind) != .missing else { return nil }
        let url = URL(fileURLWithPath: lib.expectedFilePath(id: id, kind: kind))
        if !inCloud { return url }
        guard await CloudLibrary.download(url) else { return nil }
        // reading views and full texts can be remade on another device (e.g. figures added): if
        // iCloud has a newer version than the copy here, wait briefly for it
        if kind == .epub || kind == .fulltext { await CloudLibrary.freshen(url) }
        return url
    }

    /// True if the file still has to come down from iCloud (for "Downloading…" messages).
    func needsDownload(_ id: String, _ kind: FileKind) -> Bool {
        guard inCloud, lib.fileStatus(id: id, kind: kind) != .missing else { return false }
        return !CloudLibrary.isDownloaded(URL(fileURLWithPath: lib.expectedFilePath(id: id, kind: kind)))
    }

    /// Adds full texts to the search index a few papers at a time (files may first have to
    /// download from iCloud), without blocking the app.
    func indexFullTextInBackground() {
        let lib = self.lib
        let cloud = inCloud
        Task.detached(priority: .utility) {
            if cloud {
                // ask iCloud for all full texts up front; they are small
                for p in lib.papers(filter: PaperFilter(collection: nil, tag: nil, starredOnly: false, unreadOnly: false), sort: .added) {
                    let u = URL(fileURLWithPath: lib.expectedFilePath(id: p.id, kind: .fulltext))
                    if FileManager.default.fileExists(atPath: u.path) && !CloudLibrary.isDownloaded(u) { CloudLibrary.startDownload(u) }
                }
            }
            while let n = try? lib.indexFulltext(max: 10), n > 0 {}
        }
    }

    func paper(_ id: String) -> Paper? { lib.paper(id: id) }

    func fileURL(_ id: String, _ kind: FileKind) -> URL? {
        lib.filePath(id: id, kind: kind).map { URL(fileURLWithPath: $0) }
    }

    // MARK: - Editing

    /// Saves edited details.
    func updateMeta(_ id: String, _ meta: PaperMeta) {
        do { try lib.updateMeta(id: id, meta: meta) } catch { errorMessage = error.readable }
        reload()
    }

    func toggleStar(_ p: Paper) { try? lib.setStarred(id: p.id, starred: !p.starred); reload() }
    func setStarred(_ ids: [String], _ on: Bool) {
        guard !ids.isEmpty else { return }
        for id in ids { try? lib.setStarred(id: id, starred: on) }
        reload()
    }
    func setRead(_ p: Paper, _ read: Bool) { try? lib.setRead(id: p.id, read: read); reload() }
    /// On (marking them unread) or off the Must Read list.
    func setMustRead(_ ids: [String], _ on: Bool) {
        guard !ids.isEmpty else { return }
        for id in ids { try? lib.setMustRead(id: id, mustRead: on) }
        reload()
    }
    func toggleMustRead(_ p: Paper) { setMustRead([p.id], !p.mustRead) }
    /// Opening a paper marks it read, but keeps it on the Must Read list.
    func markOpened(_ p: Paper) { if !p.read { try? lib.markOpened(id: p.id); reload() } }
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
    /// Several papers' references in one style, a blank line apart.
    func citations(_ ids: [String], _ style: CitationStyle) -> String {
        ids.map { citation($0, style) }.filter { !$0.isEmpty }.joined(separator: "\n\n")
    }
    func bibtex(_ ids: [String]) -> String { lib.exportBibtex(ids: ids) }
    func ris(_ ids: [String]) -> String { lib.exportRis(ids: ids) }

    // MARK: - Adding papers

    /// Adds a paper from a DOI / link / PMCID: converts it for reading when an open version exists,
    /// otherwise adds it with metadata only.
    /// Adds a paper by DOI, link or PMCID and returns its id. `known` fills in what the lookup
    /// missed (e.g. arXiv DOIs, which Crossref doesn't know, from a search result's metadata).
    @discardableResult
    func add(_ input: String, known: PaperMeta? = nil) async -> String? {
        let input = input.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !input.isEmpty else { return nil }
        status = "Adding \(input)…"
        defer { status = nil }
        let lib = self.lib
        let meta = await Task.detached { try? resolveMetadata(input: input) }.value
        let tmp = FileManager.default.temporaryDirectory.appendingPathComponent("lunta-add-\(UUID())")
        let conv = try? await Paper2Epub.convert(input, into: tmp)
        do {
            var m = meta ?? Self.meta(from: conv)
            if m.title.isEmpty, let c = conv { m = Self.meta(from: c) }
            if let k = known {
                if m.title.isEmpty { m.title = k.title }
                if m.authors.isEmpty { m.authors = k.authors }
                if m.date.isEmpty { m.date = k.date }
                if m.journal.isEmpty { m.journal = k.journal }
                if m.doi.isEmpty { m.doi = k.doi }
                if m.pmid.isEmpty { m.pmid = k.pmid }
                if m.pmcid.isEmpty { m.pmcid = k.pmcid }
                if m.arxiv.isEmpty { m.arxiv = k.arxiv }
                if m.url.isEmpty { m.url = k.url }
                if m.abstractText.isEmpty { m.abstractText = k.abstractText }
            }
            guard !m.title.isEmpty || !m.doi.isEmpty else {
                errorMessage = "Couldn't find “\(input)”. Try a DOI or a link to the article."
                try? FileManager.default.removeItem(at: tmp)
                return nil
            }
            let epub = conv.flatMap { try? Data(contentsOf: $0.file) }
            let added = try await Task.detached { try lib.addPaper(meta: m, pdf: nil, epub: epub, fulltext: conv?.fulltext) }.value
            reload()
            if conv == nil { status = "Added “\(added.meta.title)” (no open full text found)" }
            try? FileManager.default.removeItem(at: tmp)
            return added.id
        } catch {
            errorMessage = error.readable
        }
        try? FileManager.default.removeItem(at: tmp)
        return nil
    }

    /// Adds PDFs (dragged in or picked): matches them to existing papers by DOI, converts them for reading.
    /// What happened to each PDF added by dropping or File › Add PDF (shown as a summary card).
    struct AddOutcome: Identifiable, Equatable {
        enum Kind { case added, attached, failed }
        let id = UUID()
        let file: String
        let kind: Kind
        let paperID: String?
        let title: String
    }
    @Published var addOutcomes: [AddOutcome] = []

    @discardableResult
    func addPDFs(_ urls: [URL], privately: Bool = false) async -> [AddOutcome] {
        var out: [AddOutcome] = []
        let started = Int64(Date().timeIntervalSince1970 * 1000) - 1000
        for (i, url) in urls.enumerated() {
            let name = url.lastPathComponent
            pdfStep = urls.count > 1 ? "PDF \(i + 1) of \(urls.count) · \(name)" : name
            status = "\(pdfStep): reading…"
            guard let data = try? Data(contentsOf: url), data.starts(with: Array("%PDF".utf8)) else {
                out.append(AddOutcome(file: name, kind: .failed, paperID: nil, title: "Not a readable PDF"))
                continue
            }
            if let p = await addPDF(data, name: name, privately: privately) {
                out.append(AddOutcome(file: name, kind: p.added >= started ? .added : .attached, paperID: p.id, title: p.meta.title))
            } else {
                out.append(AddOutcome(file: name, kind: .failed, paperID: nil, title: "Couldn't add this PDF"))
            }
        }
        pdfStep = ""
        status = nil
        reload()
        addOutcomes = out
        return out
    }

    /// Current file label for step-by-step status while adding PDFs.
    private var pdfStep = ""
    private func step(_ s: String) { if !pdfStep.isEmpty { status = "\(pdfStep): \(s)" } }

    @discardableResult
    /// `privately`: no network lookups at all; the details come only from the PDF itself.
    func addPDF(_ data: Data, name: String, privately: Bool = false) async -> Paper? {
        let lib = self.lib
        let doi = Paper2Epub.doi(inPDF: data)
        if let doi, let existing = lib.findByDoi(doi: doi) {
            try? await Task.detached { try lib.attachFile(id: existing.id, kind: .pdf, data: data) }.value
            if !existing.hasEpub { await makeReadingView(existing.id) }
            return lib.paper(id: existing.id)
        }
        let tmp = FileManager.default.temporaryDirectory.appendingPathComponent("lunta-pdf-\(UUID())")
        try? FileManager.default.createDirectory(at: tmp, withIntermediateDirectories: true)
        let file = tmp.appendingPathComponent(name.hasSuffix(".pdf") ? name : name + ".pdf")
        try? data.write(to: file)
        step("making the reading view…")
        let conv = try? await Paper2Epub.$offline.withValue(privately) { try await Paper2Epub.convert(file: file, into: tmp) }
        step("looking up the paper's details…")
        var meta = PaperMeta.empty
        if let doi, !privately { meta = (await Task.detached { try? resolveMetadata(input: doi) }.value) ?? meta }
        if meta.title.isEmpty, let conv { meta = Self.meta(from: conv) }
        // no authors from the PDF's layout: let the on-device model read them off the first page
        if meta.authors.isEmpty || meta.title.isEmpty {
            step("reading the title and authors…")
            if let x = await PDFDetails.extract(fromPDF: data) {
                if !x.title.isEmpty, meta.title.isEmpty || meta.title.count < 15 || !meta.title.lowercased().hasPrefix(String(x.title.lowercased().prefix(12))) {
                    meta.title = x.title
                }
                if meta.authors.isEmpty { meta.authors = x.authors }
                if meta.journal.isEmpty { meta.journal = x.journal }
                if meta.date.isEmpty { meta.date = x.year }
                if meta.doi.isEmpty, !x.doi.isEmpty {
                    meta.doi = x.doi
                    if !privately, let full = await Task.detached(operation: { try? resolveMetadata(input: x.doi) }).value, !full.title.isEmpty {
                        meta = full
                    }
                }
            }
        }
        // no DOI in the PDF: look the title up on OpenAlex and adopt the matching record
        if !privately, (doi == nil || meta.doi.isEmpty), meta.title.count > 15, let found = await lookUpByTitle(meta.title) {
            var m = found
            if !found.doi.isEmpty, let full = await Task.detached(operation: { try? resolveMetadata(input: found.doi) }).value { m = full }
            if m.abstractText.isEmpty { m.abstractText = found.abstractText.isEmpty ? meta.abstractText : found.abstractText }
            meta = m
            // the PDF may be a paper that is already in the library
            if !meta.doi.isEmpty, let existing = lib.findByDoi(doi: meta.doi) {
                try? await Task.detached { try lib.attachFile(id: existing.id, kind: .pdf, data: data) }.value
                if !existing.hasEpub, let conv, let epub = try? Data(contentsOf: conv.file) {
                    let text = Data(conv.fulltext.utf8)
                    try? await Task.detached {
                        try lib.attachFile(id: existing.id, kind: .epub, data: epub)
                        try lib.attachFile(id: existing.id, kind: .fulltext, data: text)
                    }.value
                }
                try? FileManager.default.removeItem(at: tmp)
                return lib.paper(id: existing.id)
            }
        }
        if meta.title.isEmpty { meta.title = (name as NSString).deletingPathExtension }
        step("saving…")
        let m = meta
        let epub = conv.flatMap { try? Data(contentsOf: $0.file) }
        let p = try? await Task.detached { try lib.addPaper(meta: m, pdf: data, epub: epub, fulltext: conv?.fulltext) }.value
        try? FileManager.default.removeItem(at: tmp)
        return p
    }

    /// Finds the published record for a title on OpenAlex (only accepts a close title match).
    func lookUpByTitle(_ title: String) async -> PaperMeta? {
        let lib = self.lib
        let hits = await Task.detached { (try? lib.discover(query: title, limit: 5)) ?? [] }.value
        func norm(_ s: String) -> String { s.lowercased().filter { $0.isLetter || $0.isNumber } }
        let want = norm(title)
        return hits.first { h in
            let t = norm(h.meta.title)
            return !t.isEmpty && (t == want || (min(t.count, want.count) > 25 && (t.hasPrefix(want) || want.hasPrefix(t))))
        }?.meta
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
    /// `folder`: put the imported papers in this folder (made if needed). Exports carry no folders, so
    /// a collection is moved by exporting it on its own and importing it into a folder of the same name.
    func importExport(file: URL, pdfFolder: URL?, folder: String? = nil) async {
        status = "Importing \(file.lastPathComponent)…"
        let lib = self.lib
        do {
            let text = try String(contentsOf: file, encoding: .utf8)
            // PDF paths relative to the export (Zotero's "Export Files": files/12/paper.pdf) are found next to it
            let base = file.deletingLastPathComponent()
            let entries = parseImport(text: text).map { e -> ImportEntry in
                var e = e
                e.files = e.files.map { path in
                    if FileManager.default.fileExists(atPath: path) { return path }
                    let rel = base.appendingPathComponent(path.trimmingCharacters(in: CharacterSet(charactersIn: "/")))
                    return FileManager.default.fileExists(atPath: rel.path) ? rel.path : path
                }
                return e
            }
            let report = try await Task.detached { try lib.importEntries(entries: entries) }.value
            if let name = folder?.trimmingCharacters(in: .whitespacesAndNewlines), !name.isEmpty, !report.paperIds.isEmpty {
                let ids = report.paperIds
                try await Task.detached {
                    let col = try lib.collections().first(where: { $0.name.caseInsensitiveCompare(name) == .orderedSame })
                        ?? lib.createCollection(name: name, parent: nil)
                    for id in ids { try lib.setMembership(collection: col.id, paper: id, present: true) }
                }.value
            }
            reload()
            status = "Imported \(report.added) new papers, merged \(report.merged)" + (folder.map { $0.isEmpty ? "." : " into “\($0)”." } ?? ".")
            if let pdfFolder { await importPDFFolder(pdfFolder) }
            await makeReadingViews()
        } catch {
            errorMessage = "Import failed: \(error.readable)"
            status = nil
        }
    }

    /// Matches a folder of PDFs (e.g. ReadCube Papers' “Export PDFs”) to papers in the library:
    /// by the DOI printed in the PDF, else by finding a paper's title on the first pages.
    /// PDFs that match nothing are reported, not added (avoids duplicates).
    func importPDFFolder(_ folder: URL) async {
        let fm = FileManager.default
        let pdfs = (fm.enumerator(at: folder, includingPropertiesForKeys: nil)?.compactMap { $0 as? URL } ?? [])
            .filter { $0.pathExtension.lowercased() == "pdf" }
        let lib = self.lib
        let norm: @Sendable (String) -> String = { s in s.lowercased().filter { $0.isLetter || $0.isNumber } }
        let all = lib.papers(filter: PaperFilter(collection: nil, tag: nil, starredOnly: false, unreadOnly: false), sort: .added)
        let titles: [(String, String)] = all.map { ($0.id, norm($0.meta.title)) }.filter { $0.1.count >= 25 }
        var byDOI = 0, byTitle = 0, already = 0
        var unmatched: [String] = []
        for (i, url) in pdfs.enumerated() {
            status = "Matching PDFs: \(i + 1) of \(pdfs.count)"
            progress = Double(i) / Double(max(1, pdfs.count))
            guard let data = try? Data(contentsOf: url) else { continue }
            let match: (id: String, how: Int)? = await Task.detached {
                if let doi = Paper2Epub.doi(inPDF: data), let p = lib.findByDoi(doi: doi) { return (p.id, 0) }
                let text = norm(Paper2Epub.firstPagesText(ofPDF: data))
                // longest title found on the first pages wins (avoids short generic titles)
                if let hit = titles.filter({ text.contains($0.1) }).max(by: { $0.1.count < $1.1.count }) { return (hit.0, 1) }
                return nil
            }.value
            guard let match else { unmatched.append(url.lastPathComponent); continue }
            if lib.paper(id: match.id)?.hasPdf == true { already += 1; continue }
            try? await Task.detached { try lib.attachFile(id: match.id, kind: .pdf, data: data) }.value
            if match.how == 0 { byDOI += 1 } else { byTitle += 1 }
            if (byDOI + byTitle) % 25 == 0 { reload() }
        }
        progress = nil
        reload()
        NSLog("Lunta: PDF import: %d by DOI, %d by title, %d already had a PDF, %d unmatched", byDOI, byTitle, already, unmatched.count)
        if !unmatched.isEmpty { NSLog("Lunta: unmatched PDFs: %@", unmatched.joined(separator: ", ")) }
        status = "PDFs: \(byDOI + byTitle) attached (\(byDOI) by DOI, \(byTitle) by title), \(unmatched.count) not matched to a paper."
    }

    // MARK: - Natural narration

    /// Loads the rendered neural narration for a paper into `narrator`, if there is one.
    func loadNeuralNarration(_ id: String, into narrator: Narrator, pageItems: [NarrationItem]) async -> Bool {
        guard paper(id)?.hasAudio == true,
              let idx = await ensureLocal(id, .audioIndex), let audio = await ensureLocal(id, .audio),
              let data = try? Data(contentsOf: idx),
              let timings = try? JSONDecoder().decode(NarrationTimings.self, from: data) else { return false }
        return narrator.loadAudio(audio, timings: timings, pageItems: pageItems, title: paper(id)?.meta.title ?? "")
    }

    // MARK: - Discover

    /// Searches beyond the library (OpenAlex).
    func discover(_ query: String) async {
        let q = query.trimmingCharacters(in: .whitespacesAndNewlines)
        discoverQuery = q
        guard !q.isEmpty else { discoverResults = []; return }
        discovering = true
        discoverError = nil
        let lib = self.lib
        do {
            discoverResults = try await Task.detached { try lib.discover(query: q, limit: 40) }.value
        } catch {
            discoverError = "Search failed: \(error.readable)"
        }
        discovering = false
    }

    /// Opens Discover with the current library search.
    func searchBeyondLibrary() {
        let q = query
        source = .discover
        Task { await discover(q) }
    }

    /// Adds a discovered paper (with a reading view when an open version exists). Returns its id.
    @discardableResult
    func addDiscovered(_ hit: DiscoverHit) async -> String? {
        if let id = hit.inLibrary { return id }
        var id: String?
        if !hit.meta.doi.isEmpty {
            id = lib.findByDoi(doi: hit.meta.doi)?.id
            if id == nil { id = await add(hit.meta.doi, known: hit.meta) }
        }
        if id == nil {
            let lib = self.lib
            let meta = hit.meta
            id = try? await Task.detached { try lib.addPaper(meta: meta, pdf: nil, epub: nil, fulltext: nil) }.value.id
            reload()
        }
        if let id, let i = discoverResults.firstIndex(where: { $0.meta.doi == hit.meta.doi && $0.meta.title == hit.meta.title }) {
            discoverResults[i].inLibrary = id
        }
        // the same paper in New for You shows as added too (and counts as a paper you wanted)
        if let id { WatchCenter.shared.added(key: Self.hitKey(hit), id: id) }
        return id
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
    /// DOIs of preprint servers (same list as the core's `is_preprint_doi`).
    nonisolated static func isPreprintDOI(_ doi: String) -> Bool {
        let d = doi.lowercased()
        return ["10.1101/", "10.64898/", "10.48550/", "10.21203/", "10.2139/ssrn", "10.31219/", "10.31234/", "10.26434/", "10.20944/", "10.22541/"].contains { d.hasPrefix($0) }
    }

    @discardableResult
    func makeReadingView(_ id: String) async -> Bool {
        guard let p = lib.paper(id: id) else { return false }
        let tmp = FileManager.default.temporaryDirectory.appendingPathComponent("lunta-conv-\(UUID())")
        defer { try? FileManager.default.removeItem(at: tmp) }
        var result: ConversionResult?
        // arXiv: the HTML arXiv renders from the LaTeX (real equations, figures) beats the PDF
        // (only for papers that are arXiv preprints: a paper published in a journal is read in its published form)
        let journalDOI = !p.meta.doi.isEmpty && !Self.isPreprintDOI(p.meta.doi)
        let arxiv = journalDOI ? ""
            : !p.meta.arxiv.isEmpty ? p.meta.arxiv
            : (p.meta.doi.lowercased().hasPrefix("10.48550/arxiv.") ? String(p.meta.doi.dropFirst("10.48550/arxiv.".count)) : "")
        if !arxiv.isEmpty {
            result = try? await Paper2Epub.convert("arxiv:\(arxiv)", into: tmp)
        }
        if result != nil {
        } else if let pdf = await ensureLocal(id, .pdf) {
            result = try? await Paper2Epub.convert(file: pdf, into: tmp)
        } else {
            for input in [p.meta.pmcid, p.meta.doi] where !input.isEmpty {
                if let r = try? await Paper2Epub.convert(input, into: tmp) { result = r; break }
            }
            // no open XML: look for a legal open-access PDF (published, accepted or preprint) and use that
            if result == nil, !p.meta.doi.isEmpty || !p.meta.arxiv.isEmpty {
                let lib = self.lib
                if (await Task.detached { (try? lib.findPdf(id: id)) ?? nil }.value) != nil, let pdf = fileURL(id, .pdf) {
                    result = try? await Paper2Epub.convert(file: pdf, into: tmp)
                }
            }
        }
        NSLog("Lunta: reading view %@ for %@", result == nil ? "FAILED" : "ok", p.meta.title.prefix(60).description)
        guard let r = result, let epub = try? Data(contentsOf: r.file) else {
            UserDefaults.standard.set(Date().timeIntervalSince1970, forKey: "noFullText.\(id)")
            return false
        }
        let lib = self.lib
        let text = Data(r.fulltext.utf8)
        try? await Task.detached {
            try lib.attachFile(id: id, kind: .epub, data: epub)
            try lib.attachFile(id: id, kind: .fulltext, data: text)
        }.value
        return true
    }

    /// Reading views made from an XML version whose figure images couldn't be downloaded (PMC
    /// author manuscripts have none) show "[Figure image not available]": remade once from the
    /// paper's PDF, which supplies the figures. Kept only if the PDF did supply some.
    /// One line per paper in ~/Library/Logs/Lunta/figures.log (what the remake did and why).
    nonisolated static func figLog(_ line: String) {
        let dir = FileManager.default.urls(for: .libraryDirectory, in: .userDomainMask)[0].appendingPathComponent("Logs/Lunta", isDirectory: true)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let url = dir.appendingPathComponent("figures.log")
        let data = Data("\(Date().formatted(.iso8601)) \(line)\n".utf8)
        if let h = try? FileHandle(forWritingTo: url) { h.seekToEndOfFile(); h.write(data); try? h.close() } else { try? data.write(to: url) }
    }

    /// Asks iCloud for newer versions of reading views that are already on this device (another
    /// device may have remade them), so they're current before they're opened.
    func freshenReadingViews() {
        guard inCloud else { return }
        let urls = papers.filter(\.hasEpub).map { URL(fileURLWithPath: lib.expectedFilePath(id: $0.id, kind: .epub)) }
        Task.detached(priority: .utility) {
            var n = 0
            for u in urls where CloudLibrary.isStale(u) { CloudLibrary.startDownload(u); n += 1 }
            NSLog("Lunta: newer reading views to fetch: %d", n)
        }
    }

    func remakeMissingFigures() async {
        let marker = "[Figure image not available]"
        let all = lib.papers(filter: PaperFilter(collection: nil, tag: nil, starredOnly: false, unreadOnly: false), sort: .added)
        let lib = self.lib
        let todo: [Paper] = await Task.detached(priority: .utility) {
            all.filter { p in
                guard p.hasEpub, p.hasPdf, !UserDefaults.standard.bool(forKey: "figuresFromPDF.\(p.id)"),
                      let path = lib.filePath(id: p.id, kind: .fulltext),
                      let text = try? String(contentsOfFile: path, encoding: .utf8) else { return false }
                return text.contains(marker)
            }
        }.value
        NSLog("Lunta: reading views missing figures: %d", todo.count)
        guard !todo.isEmpty else { return }
        var fixed = 0
        for (i, p) in todo.enumerated() {
            status = "Adding missing figures from PDFs: \(i + 1) of \(todo.count)"
            // marked done only when the PDF was read (figures or none); a failure is tried again next launch
            let done = { UserDefaults.standard.set(true, forKey: "figuresFromPDF.\(p.id)") }
            guard let pdf = await ensureLocal(p.id, .pdf) else { Self.figLog("\(p.id): PDF not available"); continue }
            let tmp = FileManager.default.temporaryDirectory.appendingPathComponent("lunta-figs-\(UUID())")
            defer { try? FileManager.default.removeItem(at: tmp) }
            let r: ConversionResult
            do { r = try await Paper2Epub.convert(file: pdf, into: tmp) } catch {
                Self.figLog("\(p.id): conversion failed: \(error.localizedDescription)"); continue
            }
            guard r.notes.contains(where: { $0.contains("taken from the PDF") }),
                  let epub = try? Data(contentsOf: r.file) else {
                Self.figLog("\(p.id): no figures from the PDF (\(r.source); \(r.notes.joined(separator: " | ")))"); done(); continue
            }
            Self.figLog("\(p.id): \(r.notes.joined(separator: " | "))")
            let text = Data(r.fulltext.utf8)
            let id = p.id
            let saved: Bool = await Task.detached {
                do {
                    try lib.attachFile(id: id, kind: .epub, data: epub)
                    try lib.attachFile(id: id, kind: .fulltext, data: text)
                    return true
                } catch { return false }
            }.value
            if saved { fixed += 1; done() } else { Self.figLog("\(p.id): couldn't save the new reading view") }
        }
        reload()
        status = fixed > 0 ? "Added missing figures to \(fixed) reading view\(fixed == 1 ? "" : "s")." : nil
    }

    /// Builds reading views for every paper that lacks one (PMC papers first), a few at a time.
    func makeReadingViews() async {
        guard !converting else { return }
        converting = true
        defer { converting = false; progress = nil }
        let all = lib.papers(filter: PaperFilter(collection: nil, tag: nil, starredOnly: false, unreadOnly: false), sort: .added)
        let weekAgo = Date().timeIntervalSince1970 - 7 * 86400
        let todo = all.filter { !$0.hasEpub && ($0.hasPdf || !$0.meta.pmcid.isEmpty || !$0.meta.doi.isEmpty || !$0.meta.arxiv.isEmpty) }
            .filter { $0.hasPdf || UserDefaults.standard.double(forKey: "noFullText.\($0.id)") < weekAgo }
            // papers with a PDF first (no network needed), then PubMed Central papers, then the rest
            .sorted { ($0.hasPdf ? 0 : 1, !$0.meta.pmcid.isEmpty ? 0 : 1) < ($1.hasPdf ? 0 : 1, !$1.meta.pmcid.isEmpty ? 0 : 1) }
        NSLog("Lunta: reading views to make: %d (of %d papers)", todo.count, all.count)
        guard !todo.isEmpty else { return }
        var done = 0, ok = 0
        await withTaskGroup(of: Bool.self) { group in
            var it = todo.makeIterator()
            for _ in 0..<6 { if let p = it.next() { group.addTask { await self.makeReadingView(p.id) } } }
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
        status = "Reading views: \(ok) more ready. The rest have no open version yet: save their PDF with the browser extension or drop it on the window."
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

extension Error {
    /// The error as a person should read it: the core's message itself, not its Swift type.
    var readable: String {
        if let c = self as? CoreError {
            switch c {
            case .Io(let m), .Db(let m), .Network(let m), .NotFound(let m), .Invalid(let m): return m
            }
        }
        return localizedDescription
    }
}
