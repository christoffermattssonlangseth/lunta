import SwiftUI
import AuthenticationServices
import LuntaCore
#if os(iOS)
import UIKit
#endif

/// Lunta's Google OAuth client (an "iOS" client, which allows the app's own redirect scheme; the
/// Android app and the Mac's bridge use it too). Not a secret. ASWebAuthenticationSession catches
/// the redirect itself, so the scheme needs no entry in Info.plist.
enum GoogleDriveClient {
    static let clientID = "340661790250-jvrurgnq8d8ctjf4mhq1p4nd9sugh4en.apps.googleusercontent.com"
    static var redirectScheme: String { "com.googleusercontent.apps." + clientID.replacingOccurrences(of: ".apps.googleusercontent.com", with: "") }
    static var redirectURI: String { redirectScheme + ":/oauth2redirect" }

    /// Said wherever Lunta sends people to Google's sign-in, until Google has verified the app.
    /// Set it to nil (or delete it) once verified: every place that shows it then shows nothing.
    static let unverifiedNote: String? = "Google shows a warning that it hasn't verified Lunta yet; choose Advanced › Go to Lunta to continue."
}

/// Syncs the library with the "Lunta" folder in Google Drive when the user chose Google Drive for
/// it (LibraryStorage.googleDrive): the library lives in a folder on this device
/// (CloudLibrary.driveURL), and the core's Syncer keeps it and the Drive folder the same, like the
/// Android app (LibrarySync.google_drive, not the Mac's bridge mode).
///
/// While Lunta is open: at start, when it comes to the front, every few minutes, and shortly after
/// a change made here. "Download papers when you open them" (`onDemand`; on by default on iPhone
/// and iPad, off on the Mac): like Android, change logs, metadata, full texts and reading positions
/// always come down, a paper's PDF, reading view and narration when it is opened (`fetch`).
/// Otherwise every file is downloaded. Either way, files not here (yet) count as present
/// (`Library.setRemoteFiles`), so the app never takes them for missing, and opening one fetches it;
/// "Remove Download" frees space without the next sync taking it for a deletion. The Mac also tells
/// lunta-mcp which files are only in Drive (remote-files.json).
@MainActor
final class DriveLibrarySync: ObservableObject {
    static let shared = DriveLibrarySync()

    static let folderName = "Lunta"
    static let interval: TimeInterval = 5 * 60
    /// A sync follows this long after the last change made on this device.
    static let afterChange: TimeInterval = 20

    /// The library lives in Google Drive and is open (so there is something to sync).
    @Published private(set) var active = false
    @Published private(set) var connected = false
    @Published private(set) var syncing = false
    @Published private(set) var signingIn = false
    @Published private(set) var progress: String?
    @Published private(set) var lastSync: Date?
    @Published private(set) var lastSummary: String?
    @Published private(set) var error: String?
    /// The last sync could not open the Drive folder: offer "Choose Your Lunta Folder…".
    @Published private(set) var folderNotVisible = false
    @Published private(set) var notice: String?
    /// On demand: what is on this device and what only in Drive (Library Storage).
    @Published private(set) var downloads: DownloadSummary?
    /// Download All: "Downloading 12 of 340…" while it runs.
    @Published private(set) var downloadingAll: String?

    /// "Download papers when you open them": a Google Drive library's PDFs, reading views and
    /// narrations stay in Drive until they are needed. On by default on iPhone and iPad.
    nonisolated static let onDemandKey = "driveDownloadOnOpen"
    nonisolated static var onDemandDefault: Bool {
        #if os(iOS)
        return true
        #else
        return false
        #endif
    }
    nonisolated static var onDemand: Bool {
        UserDefaults.standard.object(forKey: onDemandKey) as? Bool ?? onDemandDefault
    }

    /// The setting changed: later syncs leave papers in Drive (nothing here is deleted), or
    /// download the rest.
    func onDemandChanged() {
        guard active, syncLazy != Self.onDemand else { return }
        if connected { begin() } else { sync = makeSync() }
        refreshRemoteFiles()
    }

    /// Said when a file the library lists isn't in the Drive folder (yet).
    nonisolated static let notInDrive = "It isn’t in Google Drive yet (another device may still be uploading it). Try again after the next sync."

    /// What the Drive folder has that this device doesn't (`remoteFiles`).
    struct RemoteFiles: Sendable {
        /// "<id>/paper.pdf"
        var only: Set<String>
        /// Every paper file not on this device, "papers/<id>/paper.pdf" (what the core was told).
        var all: [String]
        var summary: DownloadSummary
        /// The core's PDF / reading view flags changed.
        var changed: Bool
    }
    /// Read while the library opened (`preload`), applied by `start`.
    var preloaded: RemoteFiles?

    struct DownloadSummary: Equatable, Sendable {
        /// Papers with a PDF, reading view or narration, and how many of them have all of those here.
        var papers = 0
        var papersHere = 0
        var bytesHere: UInt64 = 0
        var bytesInDrive: UInt64 = 0

        @MainActor var text: String {
            let here = ByteCountFormatter.string(fromByteCount: Int64(bytesHere), countStyle: .file)
            let drive = ByteCountFormatter.string(fromByteCount: Int64(bytesInDrive), countStyle: .file)
            if bytesInDrive == 0 { return "All \(papers.formatted()) papers are downloaded to \(StorageText.thisDevice) (\(here))." }
            let rest = DriveLibrarySync.onDemand ? "\(drive) more in Google Drive, downloaded when you open a paper." : "\(drive) still to download from Google Drive."
            return "\(papersHere.formatted()) of \(papers.formatted()) papers downloaded to \(StorageText.thisDevice) (\(here)); " + rest
        }
    }

    private weak var model: LibraryModel?
    private var sync: LibrarySync?
    private var downloadAllTask: Task<Void, Never>?
    /// The `onDemand` the current sync was made with.
    private var syncLazy: Bool?
    private var timer: Timer?
    private var watcher: Timer?
    private var pending: Task<Void, Never>?
    private var again = false
    private var lastSeen: [String: Int] = [:]
    private var session: ASWebAuthenticationSession?
    private let anchor = AuthAnchor()
    private var waiters: [CheckedContinuation<Void, Never>] = []

    private var folderID: String? {
        get { UserDefaults.standard.string(forKey: "driveLibraryFolderId").flatMap { $0.isEmpty ? nil : $0 } }
        set { UserDefaults.standard.set(newValue, forKey: "driveLibraryFolderId") }
    }

    /// Tokens and sync state, on this device only (not in backups).
    nonisolated static var dir: URL {
        var d = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent("Lunta", isDirectory: true)
        try? FileManager.default.createDirectory(at: d, withIntermediateDirectories: true)
        #if os(iOS)
        var v = URLResourceValues()
        v.isExcludedFromBackup = true
        try? d.setResourceValues(v)
        #endif
        return d
    }
    nonisolated static var tokenFile: URL { dir.appendingPathComponent("google-tokens-library.json") }
    nonisolated static var statePath: URL { dir.appendingPathComponent("drive-library-state.json") }

    private lazy var auth = DriveAuth(clientId: GoogleDriveClient.clientID, tokenFile: Self.tokenFile.path, fullAccess: false)

    init() {
        lastSync = UserDefaults.standard.object(forKey: "driveLibraryLastSync") as? Date
        connected = auth.isConnected()
    }

    // MARK: - Running

    /// Called once the Google Drive library is open.
    func start(_ model: LibraryModel) {
        self.model = model
        active = true
        connected = auth.isConnected()
        // files not here are fetched when they're needed (downloading on open, or not downloaded yet)
        model.onDemand = self
        if let p = preloaded { applied(p, model) }
        if connected {
            begin()
        } else {
            sync = makeSync()   // what's in Drive, from the last sync's records (offline)
        }
        if preloaded == nil { refreshRemoteFiles() }
        preloaded = nil
        // a change made here (its change log grew, or the AI agent's on the Mac): sync soon after
        watcher?.invalidate()
        lastSeen = logSizes()
        watcher = Timer.scheduledTimer(withTimeInterval: 5, repeats: true) { [weak self] _ in
            Task { @MainActor in self?.checkForChanges() }
        }
        #if os(macOS)
        NotificationCenter.default.addObserver(forName: NSApplication.didBecomeActiveNotification, object: nil, queue: .main) { [weak self] _ in
            Task { @MainActor in self?.cameToFront() }
        }
        #endif
    }

    /// Stops syncing (the library is moving elsewhere). The sign-in stays.
    func stop() {
        timer?.invalidate(); timer = nil
        watcher?.invalidate(); watcher = nil
        pending?.cancel(); pending = nil
        downloadAllTask?.cancel(); downloadAllTask = nil
        downloadingAll = nil
        if model?.onDemand === self { model?.onDemand = nil }
        sync = nil
        downloads = nil
        active = false
        #if os(macOS)
        try? FileManager.default.removeItem(at: Self.agentRemoteFiles)
        Self.lastPublished = nil
        #endif
    }

    private func makeSync() -> LibrarySync? {
        guard let model else { return nil }
        let listener = SyncProgress { [weak self] line in Task { @MainActor in self?.progress = line } }
        var also: [String] = []
        #if os(macOS)
        if let a = LibraryLocation.agentDeviceID() { also.append(a) }   // lunta-mcp's log, written into this folder
        #endif
        syncLazy = Self.onDemand
        do {
            return try LibrarySync.googleDriveAlsoOwn(root: model.libraryURL.path, device: LibraryModel.deviceID,
                                                      statePath: Self.statePath.path, auth: auth, folderName: Self.folderName,
                                                      folderId: folderID, lazy: Self.onDemand, alsoOwn: also, progress: listener)
        } catch {
            self.error = error.readable
            return nil
        }
    }

    private func begin() {
        guard let s = makeSync() else { return }
        sync = s
        timer?.invalidate()
        timer = Timer.scheduledTimer(withTimeInterval: Self.interval, repeats: true) { [weak self] _ in
            Task { @MainActor in self?.syncNow() }
        }
        syncNow()
    }

    /// Lunta came to the front: sync unless it just did.
    func cameToFront() {
        guard active, connected else { return }
        if let last = lastSync, Date().timeIntervalSince(last) < 60 { return }
        syncNow()
    }

    private func logSizes() -> [String: Int] {
        guard let root = model?.libraryURL else { return [:] }
        var out: [String: Int] = [:]
        for id in LibraryLocation.ownLogs {
            let u = root.appendingPathComponent("ops/\(id).jsonl")
            out[id] = (try? FileManager.default.attributesOfItem(atPath: u.path)[.size] as? Int) ?? 0
        }
        return out
    }

    private func checkForChanges() {
        let now = logSizes()
        guard now != lastSeen else { return }
        lastSeen = now
        guard connected, sync != nil else { return }
        pending?.cancel()
        pending = Task { @MainActor [weak self] in
            try? await Task.sleep(nanoseconds: UInt64(Self.afterChange * 1_000_000_000))
            guard !Task.isCancelled else { return }
            self?.syncNow()
        }
    }

    func syncNow() {
        guard let sync, connected else { return }
        if syncing { again = true; return }
        syncing = true
        again = false
        error = nil
        #if os(iOS)
        // a sync that's running when Lunta goes to the background gets the time iOS allows to finish
        let bg = UIApplication.shared.beginBackgroundTask(withName: "Google Drive sync")
        #endif
        Task.detached {
            let result = Result { try sync.syncNow() }
            await MainActor.run {
                #if os(iOS)
                UIApplication.shared.endBackgroundTask(bg)
                #endif
                self.finished(result)
            }
        }
    }

    private func finished(_ result: Result<SyncSummary, Error>) {
        syncing = false
        progress = nil
        switch result {
        case .success(let s):
            lastSync = Date()
            UserDefaults.standard.set(lastSync, forKey: "driveLibraryLastSync")
            var parts: [String] = []
            if s.uploaded > 0 { parts.append("\(s.uploaded) sent to Drive") }
            if s.downloaded > 0 { parts.append("\(s.downloaded) received") }
            if s.deletedLocal + s.deletedRemote > 0 { parts.append("\(s.deletedLocal + s.deletedRemote) removed") }
            lastSummary = parts.isEmpty ? "Up to date" : parts.joined(separator: ", ")
            if !s.errors.isEmpty { error = "\(s.errors.count) file(s) failed: \(s.errors.first ?? "")" }
            if let n = s.notices.first { notice = n }
            folderNotVisible = false
            if s.changed, let model {
                _ = try? model.lib.refresh()
                model.reload()
                model.indexFullTextInBackground()
            }
            refreshRemoteFiles()
        case .failure(let e):
            error = e.readable
            folderNotVisible = driveFolderNotVisible(message: e.readable)
        }
        lastSeen = logSizes()
        let w = waiters
        waiters = []
        w.forEach { $0.resume() }
        if again, active { syncNow() }
    }

    /// One sync, waited for (before the library moves elsewhere).
    func syncAndWait() async {
        guard sync != nil, connected else { return }
        await withCheckedContinuation { (c: CheckedContinuation<Void, Never>) in
            waiters.append(c)
            if !syncing { syncNow() }
        }
    }

    // MARK: - On demand (iPhone and iPad)

    /// What the Drive folder has that this device doesn't, from the sync's records: told to the
    /// core (those files count as present) and to the lists (cloud marks). Off the main thread.
    /// While a Google Drive library opens on iPhone or iPad (before its first listing): tells the
    /// core which files are in Drive, from the last sync's records, so no paper looks as if it
    /// had lost its PDF or reading view. Offline.
    nonisolated static func preload(_ lib: Library, root: URL) -> RemoteFiles? {
        let auth = DriveAuth(clientId: GoogleDriveClient.clientID, tokenFile: tokenFile.path, fullAccess: false)
        let folder = UserDefaults.standard.string(forKey: "driveLibraryFolderId").flatMap { $0.isEmpty ? nil : $0 }
        guard let sync = try? LibrarySync.googleDrive(root: root.path, device: LibraryModel.deviceID, statePath: statePath.path, auth: auth,
                                                      folderName: folderName, folderId: folder, lazy: onDemand, progress: nil) else { return nil }
        return remoteFiles(sync, lib)
    }

    nonisolated private static func remoteFiles(_ sync: LibrarySync, _ lib: Library) -> RemoteFiles {
        let files = sync.remotePaperFiles()
        let notHere = files.filter { !$0.local }.map(\.path)
        let changed = lib.setRemoteFiles(paths: notHere)
        let ids = Set(lib.papers(filter: PaperFilter(collection: nil, tag: nil, starredOnly: false, unreadOnly: false), sort: .added).map(\.id))
        let big: Set<String> = [FileKind.pdf.fileName, FileKind.epub.fileName, FileKind.audio.fileName]
        var only = Set<String>()
        var summary = DownloadSummary()
        var perPaper: [String: Bool] = [:]   // all here?
        for f in files {
            let rel = f.path.dropFirst("papers/".count)
            guard let slash = rel.firstIndex(of: "/") else { continue }
            let id = String(rel[..<slash]), name = String(rel[rel.index(after: slash)...])
            guard ids.contains(id), big.contains(name) else { continue }   // (files of deleted papers left in Drive don't count)
            if f.local { summary.bytesHere += f.size } else { summary.bytesInDrive += f.size; only.insert(String(rel)) }
            perPaper[id] = (perPaper[id] ?? true) && f.local
        }
        summary.papers = perPaper.count
        summary.papersHere = perPaper.values.filter { $0 }.count
        return RemoteFiles(only: only, all: notHere, summary: summary, changed: changed)
    }

    /// Refreshes what's only in Drive (after a sync, a download, a removed download).
    func refreshRemoteFiles() {
        guard let sync, let model else { return }
        let lib = model.lib
        Task.detached(priority: .utility) {
            let r = Self.remoteFiles(sync, lib)
            await MainActor.run { self.applied(r, model) }
        }
    }

    private func applied(_ r: RemoteFiles, _ model: LibraryModel) {
        if model.remoteOnly != r.only { model.remoteOnly = r.only }
        downloads = r.summary
        if r.changed { model.reload() }
        #if os(macOS)
        Self.publishForAgent(r.all, root: model.libraryURL)
        #endif
    }

    #if os(macOS)
    /// What lunta-mcp (the AI agent) reads: the paper files only in Google Drive, for this library.
    nonisolated static var agentRemoteFiles: URL {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent("Lunta/remote-files.json")
    }

    private static var lastPublished: [String]?
    private static func publishForAgent(_ paths: [String], root: URL) {
        guard paths != lastPublished else { return }
        lastPublished = paths
        let json: [String: Any] = ["root": root.path, "files": paths]
        if let d = try? JSONSerialization.data(withJSONObject: json) { try? d.write(to: agentRemoteFiles, options: .atomic) }
    }
    #endif

    // MARK: - Downloads not used for a while

    /// When a paper's PDF, reading view or narration was last asked for here (LibraryModel.fetchFile).
    nonisolated static let usedKey = "driveFilesUsed"
    nonisolated static func used(_ id: String) {
        var d = UserDefaults.standard.dictionary(forKey: usedKey) as? [String: Double] ?? [:]
        d[id] = Date().timeIntervalSince1970
        UserDefaults.standard.set(d, forKey: usedKey)
    }

    /// Downloaded papers not opened here for `days` days (or never, as far as Lunta knows), and the
    /// space their downloads take.
    func unusedDownloads(days: Int) async -> (ids: [String], bytes: UInt64) {
        guard let sync, let model else { return ([], 0) }
        let lib = model.lib
        let open = model.reading
        let cutoff = Date().timeIntervalSince1970 - Double(days) * 86400
        let used = UserDefaults.standard.dictionary(forKey: Self.usedKey) as? [String: Double] ?? [:]
        return await Task.detached {
            let inLibrary = Set(lib.papers(filter: PaperFilter(collection: nil, tag: nil, starredOnly: false, unreadOnly: false), sort: .added).map(\.id))
            let big: Set<String> = [FileKind.pdf.fileName, FileKind.epub.fileName, FileKind.audio.fileName]
            var bytes: [String: UInt64] = [:]
            for f in sync.remotePaperFiles() where f.local {
                let parts = f.path.split(separator: "/")
                guard parts.count == 3, big.contains(String(parts[2])) else { continue }
                let id = String(parts[1])
                guard inLibrary.contains(id), !open.contains(id), (used[id] ?? 0) < cutoff else { continue }
                bytes[id, default: 0] += f.size
            }
            return (bytes.keys.sorted(), bytes.values.reduce(0, +))
        }.value
    }

    /// Removes the downloads of these papers (they stay in Google Drive). A message if some stayed.
    func removeDownloads(_ ids: [String]) async -> String? {
        var kept = 0
        for id in ids { if await removeDownload(id) != nil { kept += 1 } }
        return kept == 0 ? nil : "\(kept) paper\(kept == 1 ? "" : "s") stayed on \(StorageText.thisDevice): changes not in Google Drive yet, or open now. Try again after the next sync."
    }

    /// Downloads a paper's files (`kinds`, or all of them) from Google Drive. nil when they're
    /// here, otherwise what went wrong, for the user.
    func fetch(_ id: String, _ kinds: [FileKind]) async -> String? {
        guard let sync, let model else {
            return "Lunta isn’t syncing with Google Drive right now. Open Library Storage to check, then try again."
        }
        if !connected { return "Not signed in to Google, so this can’t be downloaded. Sign in in Library Storage, then try again." }
        #if os(iOS)
        let bg = UIApplication.shared.beginBackgroundTask(withName: "Download from Google Drive")
        defer { UIApplication.shared.endBackgroundTask(bg) }
        #endif
        let lib = model.lib
        let result = await Task.detached { () -> Result<RemoteFiles, Error> in
            Result {
                _ = try sync.fetchPaper(paperId: id, kinds: kinds)
                return Self.remoteFiles(sync, lib)
            }
        }.value
        switch result {
        case .success(let r):
            applied(r, model)
            return nil
        case .failure(let e):
            return Self.fetchProblem(e)
        }
    }

    nonisolated static func fetchProblem(_ e: Error) -> String {
        if let c = e as? CoreError, case .NotFound = c { return notInDrive }
        return "It couldn’t be downloaded from Google Drive (\(e.readable)). Check that you’re online, then try again."
    }

    /// "Remove Download": the paper's PDF, reading view and narration go from this device and stay
    /// in Google Drive (downloaded again when opened). nil when done, otherwise a message.
    func removeDownload(_ id: String) async -> String? {
        guard let sync, let model else { return nil }
        if model.reading.contains(id) { return "Close the paper first, then remove its download." }
        let lib = model.lib
        let result = await Task.detached { () -> Result<(RemovedDownload, RemoteFiles), Error> in
            Result {
                let removed = try sync.removeDownload(paperId: id)
                return (removed, Self.remoteFiles(sync, lib))
            }
        }.value
        switch result {
        case .success(let (removed, r)):
            applied(r, model)
            if removed.kept > 0 {
                return "Part of this paper stays on \(StorageText.thisDevice): it has changes that aren’t in Google Drive yet. Remove the download again after the next sync."
            }
            return nil
        case .failure(let e):
            return e.readable
        }
    }

    /// Download All: every paper's files, for reading offline. Runs until done or stopped.
    func downloadAll() {
        guard downloadAllTask == nil, sync != nil else { return }
        downloadAllTask = Task { @MainActor in
            defer { downloadAllTask = nil; downloadingAll = nil }
            if let problem = await fetchEverything(report: { self.downloadingAll = $0 }) { error = problem }
        }
    }

    /// Downloads every file of the library's papers that is only in Google Drive (Download All,
    /// and before the library moves away from Google Drive). nil when all are here (or the task
    /// was cancelled), otherwise what went wrong.
    func fetchEverything(report: @escaping @MainActor (String) -> Void) async -> String? {
        guard let sync, let model else { return nil }
        let lib = model.lib
        report("Looking for papers to download…")
        let ids: [String] = await Task.detached {
            let inLibrary = Set(lib.papers(filter: PaperFilter(collection: nil, tag: nil, starredOnly: false, unreadOnly: false), sort: .added).map(\.id))
            let missing = sync.remotePaperFiles().filter { !$0.local }.compactMap { f -> String? in
                let parts = f.path.split(separator: "/")
                return parts.count == 3 ? String(parts[1]) : nil
            }
            return Array(Set(missing).intersection(inLibrary)).sorted()
        }.value
        for (i, id) in ids.enumerated() {
            if Task.isCancelled { break }
            report("Downloading \((i + 1).formatted()) of \(ids.count.formatted()) papers from Google Drive…")
            if let problem = await fetch(id, []) { return problem }
        }
        refreshRemoteFiles()
        return nil
    }

    func cancelDownloadAll() {
        downloadAllTask?.cancel()
    }

    // MARK: - Signing in

    /// Google sign-in in a system web sheet. True when signed in.
    @discardableResult
    func signIn() async -> Bool {
        error = nil
        let req: SignInRequest
        do { req = try auth.beginSignIn(redirectUri: GoogleDriveClient.redirectURI) } catch { self.error = error.readable; return false }
        return await finishSignIn(req, picking: false)
    }

    /// "Choose Your Lunta Folder…": Google's picker inside the sign-in, for a Drive folder Lunta
    /// can't open (made by another app, or its access was removed).
    func chooseFolder() async {
        error = nil
        let req: SignInRequest
        do { req = try auth.beginPick(redirectUri: GoogleDriveClient.redirectURI, fileIds: []) } catch { self.error = error.readable; return }
        _ = await finishSignIn(req, picking: true)
    }

    private func finishSignIn(_ req: SignInRequest, picking: Bool) async -> Bool {
        guard let url = URL(string: req.url) else { return false }
        signingIn = true
        defer { signingIn = false }
        let callback: URL?
        do { callback = try await webAuth(url) } catch { self.error = error.readable; return false }
        guard let callback else { return false }   // cancelled
        let auth = self.auth
        let redirect = callback.absoluteString
        let result = await Task.detached { () -> Result<[String], Error> in
            Result {
                let code = try auth.codeFromRedirect(redirectUrl: redirect, expectedState: req.state)
                try auth.completeSignIn(code: code, verifier: req.verifier, redirectUri: GoogleDriveClient.redirectURI)
                return picking ? try auth.pickedFolder(ids: auth.pickedIdsFromRedirect(redirectUrl: redirect)) : []
            }
        }.value
        do {
            let folder = try result.get()
            if folder.count == 2 { folderID = folder[0] }
            else if picking { notice = "No folder was chosen, so Lunta keeps using its own “\(Self.folderName)” folder." }
        } catch {
            self.error = error.readable
            return false
        }
        connected = true
        if active { begin() }
        return true
    }

    private func webAuth(_ url: URL) async throws -> URL? {
        try await withCheckedThrowingContinuation { (cont: CheckedContinuation<URL?, Error>) in
            let s = ASWebAuthenticationSession(url: url, callback: .customScheme(GoogleDriveClient.redirectScheme)) { callback, err in
                if let callback { cont.resume(returning: callback); return }
                if let e = err as? ASWebAuthenticationSessionError, e.code == .canceledLogin { cont.resume(returning: nil); return }
                cont.resume(throwing: err ?? SignInFailed())
            }
            s.presentationContextProvider = anchor
            s.prefersEphemeralWebBrowserSession = false
            session = s
            if !s.start() { cont.resume(throwing: SignInFailed()) }
        }
    }

    struct SignInFailed: LocalizedError {
        var errorDescription: String? { "Google sign-in didn't open. Try again." }
    }

    /// Forgets the sign-in on this device (the library stays here, and in Drive). Syncing stops
    /// until the user signs in again.
    func disconnect() {
        timer?.invalidate(); timer = nil
        downloadAllTask?.cancel()
        let auth = self.auth
        Task.detached { try? auth.disconnect() }
        connected = false
        lastSummary = nil
        notice = nil
        folderNotVisible = false
        folderID = nil
    }

    #if os(macOS)
    /// Moving an iCloud library that this Mac already copies to Google Drive (the bridge): use the
    /// bridge's sign-in and folder, so there's no second sign-in.
    func takeOverBridgeSignIn() {
        guard !auth.isConnected() else { return }
        let bridgeTokens = DriveLibrarySync.dir.appendingPathComponent("google-tokens.json")
        guard FileManager.default.fileExists(atPath: bridgeTokens.path) else { return }
        try? FileManager.default.removeItem(at: Self.tokenFile)
        try? FileManager.default.copyItem(at: bridgeTokens, to: Self.tokenFile)
        if let f = UserDefaults.standard.string(forKey: "driveFolderId"), !f.isEmpty { folderID = f }
        auth = DriveAuth(clientId: GoogleDriveClient.clientID, tokenFile: Self.tokenFile.path, fullAccess: false)
        connected = auth.isConnected()
    }
    #endif

    // MARK: -

    final class SyncProgress: ProgressListener, @unchecked Sendable {
        let handler: (String) -> Void
        init(_ handler: @escaping (String) -> Void) { self.handler = handler }
        func onProgress(message: String) { handler(message) }
    }

    final class AuthAnchor: NSObject, ASWebAuthenticationPresentationContextProviding {
        func presentationAnchor(for session: ASWebAuthenticationSession) -> ASPresentationAnchor {
            #if os(macOS)
            return NSApp.keyWindow ?? NSApp.windows.first ?? ASPresentationAnchor()
            #else
            let scenes = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }
            return scenes.flatMap(\.windows).first(where: \.isKeyWindow) ?? scenes.first?.windows.first ?? ASPresentationAnchor()
            #endif
        }
    }
}
