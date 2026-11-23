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
    var importSharedAgain = false
    @Published var source: SidebarItem = .all { didSet { reloadList() } }
    @Published var query = "" { didSet { scheduleSearch() } }
    private var searchTask: Task<Void, Never>?
    @Published var sort: SortKey = .added { didSet { reloadList() } }
    @Published var status: String?
    @Published var progress: Double?
    @Published var errorMessage: String?
    /// Papers just added whose PDF or reading view is still on its way: what is being done
    /// ("Finding the PDF…"), shown in the list next to the paper.
    @Published var preparing: [String: String] = [:]
    /// What Safari sent: its background step (details, PDF, reading view), in turn (SafariInboxImport).
    var safariFollowUp: Task<Void, Never>?
    /// A .bib imported with its citation keys: its report (BibImport.swift).
    @Published var bibImport: BibImportResult?
    /// Checks against the published record (Checks.swift): a checked reference list, a check of
    /// the library, and what the checks found per paper (retractions, published versions).
    @Published var refCheck: RefCheckResult?
    @Published var updatesReport: UpdatesResult?
    @Published var paperUpdates: [String: PaperUpdates] = [:]
    var updateChecksScheduled = false
    @Published var counts = (all: 0, starred: 0, mustRead: 0, unread: 0)
    // Discover: searching beyond the library (OpenAlex)
    @Published var discoverQuery = ""
    @Published var discoverResults: [DiscoverHit] = []
    /// Discovered papers being added (their hitKey): added in the background, so a new search can
    /// run meanwhile; a result being added shows as such in any later search too, and isn't added twice.
    @Published private(set) var discoverAdding: Set<String> = []
    @Published var discovering = false
    @Published var discoverError: String?
    private var refreshTimer: Timer?
    /// iCloud: hears about other devices' changes as they arrive (CloudOpsWatcher.swift)
    private var opsWatcher: CloudOpsWatcher?
    private var converting = false

    let libraryURL: URL
    let inCloud: Bool

    /// A Google Drive library synced on demand (iPhone and iPad, DriveLibrarySync): fetches a
    /// paper's PDF, reading view or narration from Drive when it is needed. nil otherwise (the Mac
    /// keeps every file; iCloud has its own placeholders).
    weak var onDemand: DriveLibrarySync?
    /// With `onDemand`: the paper files that are in Google Drive and not on this device
    /// ("<id>/paper.pdf"), for the lists' cloud marks. The core counts them as present too
    /// (`Library.setRemoteFiles`), so nothing takes them for missing.
    @Published var remoteOnly: Set<String> = []
    /// Papers open in a reader now (their files aren't removed from under it).
    var reading: Set<String> = []

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
        if inCloud { opsWatcher = CloudOpsWatcher { [weak self] in self?.refreshFromDisk() } }
    }

    // MARK: - Reading the library

    /// Full refresh: list plus sidebar data (collections, tags, counts). Call after the library changes.
    func reload() {
        reloadList()
        collections = lib.collections()
        tags = lib.tags()
        loadSmartFolders()
        paperUpdates = lib.allPaperUpdates()
        let every = lib.papers(filter: PaperFilter(collection: nil, tag: nil, starredOnly: false, unreadOnly: false), sort: .added)
        counts = (every.count, every.filter(\.starred).count, every.filter(\.mustRead).count, every.filter { !$0.read }.count)
        #if os(iOS)
        publishSafariIndex()   // what the Safari extension answers "in your library?" from
        #else
        BibFiles.shared.libraryChanged(self)   // .bib files kept updated (BibFiles.swift)
        #endif
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
            RecentSearches.shared.searched(q, found: result.0.count)
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
            // the change logs already here: ask for iCloud's newest version too (iOS is slow to
            // replace a file it has; a no-op when it's current)
            if let root = await self?.libraryURL {
                let ops = root.appendingPathComponent("ops")
                for f in (try? FileManager.default.contentsOfDirectory(at: ops, includingPropertiesForKeys: nil)) ?? [] where f.pathExtension == "jsonl" {
                    CloudLibrary.startDownload(f)
                }
            }
            await MainActor.run { self?.scanningPlaceholders = false }
        }
    }
    private var scanningPlaceholders = false

    /// What getting a paper's file gave.
    enum FileFetch {
        case ready(URL)
        /// The paper has no such file.
        case missing
        /// It's in iCloud or Google Drive and couldn't be downloaded: why, for the user.
        case failed(String)
    }

    /// Makes sure a paper's file is on this device: downloaded from iCloud, or from Google Drive
    /// for a library synced on demand. Every place that reads a paper's file goes through here
    /// (or `ensureLocal`).
    func fetchFile(_ id: String, _ kind: FileKind) async -> FileFetch {
        guard lib.fileStatus(id: id, kind: kind) != .missing else { return .missing }
        let url = URL(fileURLWithPath: lib.expectedFilePath(id: id, kind: kind))
        if onDemand != nil, kind == .pdf || kind == .epub || kind == .audio { DriveLibrarySync.used(id) }
        if let drive = onDemand, !FileManager.default.fileExists(atPath: url.path) {
            if let problem = await drive.fetch(id, [kind]) { return .failed(problem) }
            return FileManager.default.fileExists(atPath: url.path) ? .ready(url) : .failed(DriveLibrarySync.notInDrive)
        }
        if !inCloud { return .ready(url) }
        guard await CloudLibrary.download(url) else { return .failed("It couldn’t be downloaded from iCloud. Check that you’re online, then try again.") }
        // reading views and full texts can be remade on another device (e.g. figures added): if
        // iCloud has a newer version than the copy here, wait briefly for it
        if kind == .epub || kind == .fulltext { await CloudLibrary.freshen(url) }
        return .ready(url)
    }

    /// `fetchFile`, for callers that only need the file or nothing.
    func ensureLocal(_ id: String, _ kind: FileKind) async -> URL? {
        if case .ready(let u) = await fetchFile(id, kind) { return u }
        return nil
    }

    /// True if the file still has to come down from iCloud or Google Drive (for "Downloading…" messages).
    func needsDownload(_ id: String, _ kind: FileKind) -> Bool {
        if onDemand != nil { return lib.fileStatus(id: id, kind: kind) == .inCloud }
        guard inCloud, lib.fileStatus(id: id, kind: kind) != .missing else { return false }
        return !CloudLibrary.isDownloaded(URL(fileURLWithPath: lib.expectedFilePath(id: id, kind: kind)))
    }

    /// Where files that aren't here yet come from: "iCloud" or "Google Drive".
    var downloadSource: String { onDemand != nil ? "Google Drive" : "iCloud" }

    /// A library synced on demand: the file is in Google Drive and not on this device.
    func isRemoteOnly(_ id: String, _ kind: FileKind) -> Bool {
        onDemand != nil && remoteOnly.contains("\(id)/\(kind.fileName)")
    }

    /// A library synced on demand: which of the paper's PDF, reading view and narration are only in Google Drive.
    func inDrive(_ id: String) -> Set<FileKind> {
        guard onDemand != nil, !remoteOnly.isEmpty else { return [] }
        return Set([FileKind.pdf, .epub, .audio].filter { remoteOnly.contains("\(id)/\($0.fileName)") })
    }

    /// Download (a library synced on demand): the paper's files, for reading offline.
    func download(_ id: String) async {
        guard let drive = onDemand else { return }
        let title = paper(id)?.meta.title ?? ""
        status = "Downloading “\(title.count > 50 ? String(title.prefix(47)) + "…" : title)” from Google Drive…"
        let problem = await drive.fetch(id, [])
        status = nil
        if let problem { errorMessage = problem }
    }

    /// Remove Download (a library synced on demand): frees the space; the files stay in Google Drive.
    func removeDownload(_ id: String) async {
        guard let drive = onDemand else { return }
        if let message = await drive.removeDownload(id) { errorMessage = message }
    }

    /// A library synced on demand: some of the paper's files (PDF, reading view, narration) are only in Google Drive.
    func hasRemoteOnly(_ id: String) -> Bool {
        onDemand != nil && [FileKind.pdf, .epub, .audio].contains { remoteOnly.contains("\(id)/\($0.fileName)") }
    }

    /// A library synced on demand: the paper's PDF, reading view or narration is on this device (can be removed).
    func hasDownload(_ id: String) -> Bool {
        guard onDemand != nil, DriveLibrarySync.onDemand else { return false }
        return [FileKind.pdf, .epub, .audio].contains { lib.fileStatus(id: id, kind: $0) == .local }
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

    /// Adds a paper by DOI, link or PMCID and returns its id: converted for reading when an open
    /// XML version exists, else (`findPDF`) with a legal open-access PDF when there is one (its
    /// reading view made from that). `known` fills in what the lookup missed (e.g. arXiv DOIs,
    /// which Crossref doesn't know, from a search result's metadata).
    ///
    /// The paper is in the list as soon as its details are known (a second or so), marked as
    /// being prepared (`preparing`); the reading view is made meanwhile (it was started with the
    /// lookup) and the PDF looked for after it. Returns when all of that is done.
    @discardableResult
    func add(_ input: String, known: PaperMeta? = nil, findPDF: Bool = true) async -> String? {
        let input = input.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !input.isEmpty else { return nil }
        // a plain web page or blog post (a page that declares a DOI is added as that paper);
        // a page that couldn't be read goes on as a link to an article
        var webError: String?
        if known == nil, isWebLink(input: input) {
            switch await addWebPage(input) {
            case .success(let id): return id
            case .failure(let e): webError = e.readable
            }
        }
        status = "Adding \(input)…"
        defer { status = nil }
        let lib = self.lib
        let tmp = FileManager.default.temporaryDirectory.appendingPathComponent("lunta-add-\(UUID())")
        defer { try? FileManager.default.removeItem(at: tmp) }
        // the reading view needs none of the details: made while they are looked up
        let conversion = Task.detached(priority: .userInitiated) { try? await Paper2Epub.convert(input, into: tmp) }
        let meta = await Task.detached(priority: .userInitiated) { try? resolveMetadata(input: input) }.value
        // in the list at once, with the details found
        var shown: String?
        if var m = meta {
            Self.fill(&m, from: known)
            if !m.title.isEmpty {
                let early = m
                shown = try? await Task.detached { try lib.addPaper(meta: early, pdf: nil, epub: nil, fulltext: nil) }.value.id
                if let id = shown {
                    preparing[id] = "Making the reading view…"
                    reload()
                }
            }
        }
        defer { if let id = shown { preparing[id] = nil } }
        let conv = await conversion.value
        do {
            var m = meta ?? Self.meta(from: conv)
            if m.title.isEmpty, let c = conv { m = Self.meta(from: c) }
            Self.fill(&m, from: known)
            guard !m.title.isEmpty || !m.doi.isEmpty else {
                errorMessage = "Couldn't find “\(input)”. Try a DOI or a link to the article." + (webError.map { "\n\($0)" } ?? "")
                return nil
            }
            let epub = conv.flatMap { try? Data(contentsOf: $0.file) }
            let added: Paper
            if let id = shown {
                // (added above with the same details: only the reading view to put in)
                if let epub {
                    let text = conv?.fulltext
                    try await Task.detached {
                        try lib.attachFile(id: id, kind: .epub, data: epub)
                        if let text { try lib.attachFile(id: id, kind: .fulltext, data: Data(text.utf8)) }
                    }.value
                }
                guard let p = lib.paper(id: id) else { return nil }
                added = p
            } else {
                let mm = m
                added = try await Task.detached { try lib.addPaper(meta: mm, pdf: nil, epub: epub, fulltext: conv?.fulltext) }.value
            }
            reload()
            if conv != nil, shown != nil { NotificationCenter.default.post(name: .luntaReadingViewMade, object: added.id) }
            if conv == nil {
                // no open XML version: a legal open-access PDF, and the reading view from it
                if findPDF, !added.hasPdf, !added.meta.doi.isEmpty || !added.meta.arxiv.isEmpty || !added.meta.pmcid.isEmpty {
                    status = "Finding a free PDF of “\(added.meta.title)”…"
                    if await self.findPDF(added.id) != nil { return added.id }
                }
                status = "Added “\(added.meta.title)” (no open full text found)"
            }
            return added.id
        } catch {
            errorMessage = error.readable
        }
        return nil
    }

    /// Fills what the lookup missed from what was already known about the paper.
    static func fill(_ m: inout PaperMeta, from k: PaperMeta?) {
        guard let k else { return }
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

    /// Adds a web page or blog post by its address: its details read off the page by the core
    /// (title, authors, date, site; read today), its reading view from the main text the core
    /// picked out (Paper2EpubKit fetches the images and writes the book). `html`: the page as
    /// the browser has it (the Safari extension's Save); without it the core fetches the page
    /// once. A page that declares a DOI is added as that paper; one already in the library (same
    /// address) is kept. A page with too little text is added with its details only.
    func addWebPage(_ url: String, html: String? = nil) async -> Result<String, Error> {
        status = "Reading \(URL(string: url)?.host ?? url)…"
        defer { status = nil }
        let page: LuntaCore.WebPage
        do {
            if let html, !html.isEmpty {
                page = await Task.detached { readWebPage(html: html, url: url) }.value
            } else {
                page = try await Task.detached { try fetchWebPage(url: url) }.value
            }
        } catch { return .failure(error) }
        if !page.scholarlyId.isEmpty {
            if let id = await add(page.scholarlyId) { return .success(id) }
            return .failure(ConversionError.message(errorMessage ?? "Couldn't add \(page.scholarlyId)"))
        }
        if let p = lib.findByUrl(url: page.url), p.hasEpub { return .success(p.id) }
        var m = page.meta
        if m.title.isEmpty { m.title = page.url }
        var epub: Data?, text: String?
        let tmp = FileManager.default.temporaryDirectory.appendingPathComponent("lunta-web-\(UUID())")
        defer { try? FileManager.default.removeItem(at: tmp) }
        if page.article.problem.isEmpty {
            status = "Making the reading view…"
            let chapters: [Paper2Epub.WebChapter] = page.article.chapters.map { c in
                (title: c.title, html: c.html, headings: c.headings.map { (level: Int($0.level), id: $0.id, title: $0.title) })
            }
            if let r = try? await Paper2Epub.convert(webPage: chapters, imageURLs: page.article.images, title: m.title, authors: m.authors,
                                                     date: m.date, site: m.journal, url: page.url, into: tmp) {
                epub = try? Data(contentsOf: r.file)
                text = r.fulltext
            }
        }
        let lib = self.lib, meta = m, e = epub, t = text
        do {
            let added = try await Task.detached { try lib.addPaper(meta: meta, pdf: nil, epub: e, fulltext: t) }.value
            reload()
            if e == nil { status = "Added “\(added.meta.title)” as a web page (\(page.article.problem.isEmpty ? "no reading view could be made" : "too little text for a reading view"))" }
            return .success(added.id)
        } catch { return .failure(error) }
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
            if !existing.hasEpub { reload(); await preparingReadingView(existing.id) }
            return lib.paper(id: existing.id)
        }
        // a DOI in the PDF that finds the paper's details: in the list (and readable, as its PDF)
        // at once, the reading view made after (it was made first, which took a while)
        if let doi, !privately {
            step("looking up the paper's details…")
            if let m = await Task.detached(priority: .userInitiated, operation: { try? resolveMetadata(input: doi) }).value, !m.title.isEmpty,
               let p = try? await Task.detached(operation: { try lib.addPaper(meta: m, pdf: data, epub: nil, fulltext: nil) }).value {
                reload()
                step("making the reading view…")
                await preparingReadingView(p.id)
                return lib.paper(id: p.id)
            }
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
    /// `roots`: more folders to look for the export's PDFs in (a picked export folder or unpacked ZIP).
    @discardableResult
    func importExport(file: URL, pdfFolder: URL?, folder: String? = nil, roots: [URL] = [], readingViews: Bool = true) async -> ImportSummary? {
        do {
            let summary = try await runImport(file: file, pdfFolder: pdfFolder, folder: folder, roots: roots)
            if readingViews { await makeReadingViews() }
            return summary
        } catch {
            errorMessage = "Import failed: \(error.readable)"
            status = nil
            progress = nil
            return nil
        }
    }

    /// The import itself (see `importExport`); throws instead of showing the error.
    func runImport(file: URL, pdfFolder: URL?, folder: String?, roots: [URL]) async throws -> ImportSummary {
        status = "Importing \(file.lastPathComponent)…"
        defer { progress = nil }
        let lib = self.lib
        // the export read and its PDFs found off the main thread (files in iCloud Drive are downloaded first).
        // PDF paths relative to the export (Zotero's "Export Files": files/12/paper.pdf) are found next to it.
        let roots = [file.deletingLastPathComponent()] + roots
        let read = try await Task.detached { try ImportSource.read(file, roots: roots) }.value
        let (entries, found, missing) = (read.entries, read.found, read.missing)
        // in batches, so the status shows how far it has got and the library fills in as it goes
        var summary = ImportSummary(missingPDFs: missing)
        var ids: [String] = []
        let step = 20
        for start in stride(from: 0, to: entries.count, by: step) {
            status = "Importing \(file.lastPathComponent): \(start) of \(entries.count) papers"
            progress = Double(start) / Double(max(1, entries.count))
            let batch = Array(entries[start ..< min(start + step, entries.count)])
            let r = try await Task.detached { try lib.importEntries(entries: batch) }.value
            summary.added += Int(r.added)
            summary.merged += Int(r.merged)
            summary.skipped += Int(r.skipped)
            summary.pdfs += Int(r.pdfsCopied)
            ids += r.paperIds
            if (start / step) % 5 == 4 { reload() }
        }
        if let name = folder?.trimmingCharacters(in: .whitespacesAndNewlines), !name.isEmpty, !ids.isEmpty {
            try await Task.detached { [ids] in
                let col = try lib.collections().first(where: { $0.name.caseInsensitiveCompare(name) == .orderedSame })
                    ?? lib.createCollection(name: name, parent: nil)
                for id in ids { try lib.setMembership(collection: col.id, paper: id, present: true) }
            }.value
            summary.folder = name
        }
        reload()
        status = "Imported \(summary.added) new papers, merged \(summary.merged)" + (summary.folder.map { " into “\($0)”." } ?? ".")
        if let pdfFolder { summary.matched = await importPDFFolder(pdfFolder, skipping: found).attached }
        status = summary.text
        return summary
    }

    /// Matches a folder of PDFs (e.g. ReadCube Papers' “Export PDFs”) to papers in the library:
    /// by the DOI printed in the PDF, else by finding a paper's title on the first pages.
    /// PDFs that match nothing are reported, not added (avoids duplicates). `skipping`: PDFs already
    /// taken from the export (their resolved paths). Returns how many were attached, and the summary.
    @discardableResult
    func importPDFFolder(_ folder: URL, skipping: Set<String> = []) async -> (attached: Int, text: String) {
        let fm = FileManager.default
        let pdfs = (fm.enumerator(at: folder, includingPropertiesForKeys: nil, options: [.skipsHiddenFiles])?.compactMap { $0 as? URL } ?? [])
            .filter { $0.pathExtension.lowercased() == "pdf" && !skipping.contains($0.resolvingSymlinksInPath().path) }
        // nothing left once the export's own PDFs are taken (a Zotero export folder): no second status
        if pdfs.isEmpty && !skipping.isEmpty { return (0, "") }
        let lib = self.lib
        let norm: @Sendable (String) -> String = { s in s.lowercased().filter { $0.isLetter || $0.isNumber } }
        let all = lib.papers(filter: PaperFilter(collection: nil, tag: nil, starredOnly: false, unreadOnly: false), sort: .added)
        let titles: [(String, String)] = all.map { ($0.id, norm($0.meta.title)) }.filter { $0.1.count >= 25 }
        var byDOI = 0, byTitle = 0, already = 0
        var unmatched: [String] = []
        for (i, url) in pdfs.enumerated() {
            status = "Matching PDFs: \(i + 1) of \(pdfs.count)"
            progress = Double(i) / Double(max(1, pdfs.count))
            // read off the main thread; a PDF in iCloud Drive is downloaded first
            let found: (data: Data, id: String, how: Int)?? = await Task.detached {
                guard let data = try? ImportSource.coordinated(url, { try Data(contentsOf: $0) }) else { return nil }
                if let doi = Paper2Epub.doi(inPDF: data), let p = lib.findByDoi(doi: doi) { return (data, p.id, 0) }
                let text = norm(Paper2Epub.firstPagesText(ofPDF: data))
                // longest title found on the first pages wins (avoids short generic titles)
                if let hit = titles.filter({ text.contains($0.1) }).max(by: { $0.1.count < $1.1.count }) { return (data, hit.0, 1) }
                return .some(nil)
            }.value
            guard let read = found else { continue }
            guard let match = read else { unmatched.append(url.lastPathComponent); continue }
            if lib.paper(id: match.id)?.hasPdf == true { already += 1; continue }
            let data = match.data
            try? await Task.detached { try lib.attachFile(id: match.id, kind: .pdf, data: data) }.value
            if match.how == 0 { byDOI += 1 } else { byTitle += 1 }
            if (byDOI + byTitle) % 25 == 0 { reload() }
        }
        progress = nil
        reload()
        NSLog("Lunta: PDF import: %d by DOI, %d by title, %d already had a PDF, %d unmatched", byDOI, byTitle, already, unmatched.count)
        if !unmatched.isEmpty { NSLog("Lunta: unmatched PDFs: %@", unmatched.joined(separator: ", ")) }
        let text = "PDFs: \(byDOI + byTitle) attached (\(byDOI) by DOI, \(byTitle) by title), \(unmatched.count) not matched to a paper."
        status = text
        return (byDOI + byTitle, text)
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
        let key = Self.hitKey(hit)
        guard !discoverAdding.contains(key) else { return nil }
        discoverAdding.insert(key)
        defer { discoverAdding.remove(key) }
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
        let had = preparing[id]
        preparing[id] = "Finding the PDF…"
        defer { preparing[id] = had }
        let source = await Task.detached(priority: .userInitiated) { (try? lib.findPdf(id: id)) ?? nil }.value
        // the paper can be opened (as its PDF) now: the reading view is made after
        reload()
        if source != nil, lib.paper(id: id)?.hasEpub == false {
            preparing[id] = "Making the reading view…"
            await makeReadingView(id)
            reload()
        }
        return source
    }

    /// Makes the reading view of a paper that has just come in, marked as being prepared in the list.
    func preparingReadingView(_ id: String) async {
        guard lib.paper(id: id)?.hasEpub == false else { return }
        preparing[id] = "Making the reading view…"
        defer { preparing[id] = nil }
        await makeReadingView(id)
        reload()
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
        // a PMC paper without a DOI: its XML beats the PDF
        if result == nil, p.meta.doi.isEmpty, !p.meta.pmcid.isEmpty {
            result = try? await Paper2Epub.convert(p.meta.pmcid, into: tmp)
        }
        if result != nil {
        } else if let pdf = await ensureLocal(id, .pdf) {
            // the paper's own DOI finds its XML version even when the PDF doesn't show it
            result = try? await Paper2Epub.$knownDOI.withValue(p.meta.doi) { try await Paper2Epub.convert(file: pdf, into: tmp) }
        } else {
            for input in [p.meta.pmcid, p.meta.doi] where !input.isEmpty {
                if let r = try? await Paper2Epub.convert(input, into: tmp) { result = r; break }
            }
            // no open XML: look for a legal open-access PDF (published, accepted or preprint) and use that
            if result == nil, !p.meta.doi.isEmpty || !p.meta.arxiv.isEmpty {
                let lib = self.lib
                if (await Task.detached { (try? lib.findPdf(id: id)) ?? nil }.value) != nil, let pdf = fileURL(id, .pdf) {
                    result = try? await Paper2Epub.$knownDOI.withValue(p.meta.doi) { try await Paper2Epub.convert(file: pdf, into: tmp) }
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
        // a reader that has the paper open shows the new reading view
        NotificationCenter.default.post(name: .luntaReadingViewMade, object: id)
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
        let onDemand = self.onDemand != nil && DriveLibrarySync.onDemand
        let todo: [Paper] = await Task.detached(priority: .utility) {
            all.filter { p in
                guard p.hasEpub, p.hasPdf, !UserDefaults.standard.bool(forKey: "figuresFromPDF.\(p.id)"),
                      !onDemand || lib.fileStatus(id: p.id, kind: .pdf) != .inCloud,   // (a PDF left in Google Drive isn't downloaded for this)
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
    /// Makes (or remakes) the reading views of the chosen papers, with progress in the status bar:
    /// "Making reading views: 3 of 18 — <title>", then how many were made.
    func makeReadingViews(_ ids: [String]) async {
        guard !ids.isEmpty else { return }
        let one = ids.count == 1
        var ok = 0, failed: [String] = []
        // remaking: a failure keeps the reading view the paper had
        let remaking = ids.contains { lib.paper(id: $0)?.hasEpub == true }
        let verb = remaking ? "Remaking" : "Making"
        for (i, id) in ids.enumerated() {
            let title = lib.paper(id: id)?.meta.title ?? ""
            let short = title.count > 60 ? String(title.prefix(57)) + "…" : title
            status = one ? "\(verb) the reading view: \(short)" : "\(verb) reading views: \(i + 1) of \(ids.count) — \(short)"
            progress = one ? nil : Double(i) / Double(ids.count)
            if await makeReadingView(id) { ok += 1 } else { failed.append(short) }
            if !one, (i + 1) % 5 == 0 { reload() }
        }
        reload()
        if one {
            status = ok == 1 ? (remaking ? "Reading view remade." : "Reading view ready.")
                : remaking ? "Couldn’t make the reading view again; the current one is kept." : "No open full text found for this paper."
        } else {
            status = "Reading views: \(ok) of \(ids.count) \(remaking ? "remade" : "made")."
                + (failed.isEmpty ? "" : remaking ? " \(failed.count) couldn’t be made (a reading view they had is kept)." : " No open full text for \(failed.count).")
        }
        // the result stays a moment (a full bar, not a spinner), then the status bar goes
        progress = 1
        let shown = status
        try? await Task.sleep(for: .seconds(8))
        if status == shown { status = nil; progress = nil }
    }

    /// The library's DOIs and arXiv numbers ("arxiv:<id>"), lower-cased: for the citation card.
    func libraryIDs() -> [String] {
        lib.papers(filter: PaperFilter(collection: nil, tag: nil, starredOnly: false, unreadOnly: false), sort: .added).flatMap { p -> [String] in
            var out: [String] = []
            if !p.meta.doi.isEmpty { out.append(p.meta.doi.lowercased()) }
            if !p.meta.arxiv.isEmpty { out.append("arxiv:" + p.meta.arxiv.lowercased()) }
            return out
        }
    }

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

/// The cloud on a file's pill in the lists: in Google Drive, not on this device.
enum PaperRowCloud {
    static let icon = "icloud.and.arrow.down"
}

extension FileKind {
    /// The file's name in the paper's folder (the core's `FileKind::file_name`).
    var fileName: String {
        switch self {
        case .pdf: return "paper.pdf"
        case .epub: return "book.epub"
        case .fulltext: return "fulltext.txt"
        case .audio: return "narration.m4a"
        case .audioIndex: return "narration.json"
        }
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
