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
/// a change made here. Every file is downloaded, on the Mac and on iPhone/iPad (Android leaves PDFs
/// and reading views on Drive until opened; the Apple apps don't do that yet).
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

    private weak var model: LibraryModel?
    private var sync: LibrarySync?
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
        if connected { begin() }
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
        sync = nil
        active = false
    }

    private func begin() {
        guard let model else { return }
        let listener = SyncProgress { [weak self] line in Task { @MainActor in self?.progress = line } }
        var also: [String] = []
        #if os(macOS)
        if let a = LibraryLocation.agentDeviceID() { also.append(a) }   // lunta-mcp's log, written into this folder
        #endif
        do {
            sync = try LibrarySync.googleDriveAlsoOwn(root: model.libraryURL.path, device: LibraryModel.deviceID,
                                                      statePath: Self.statePath.path, auth: auth, folderName: Self.folderName,
                                                      folderId: folderID, lazy: false, alsoOwn: also, progress: listener)
        } catch {
            self.error = error.readable
            return
        }
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
        guard let sync else { return }
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
        sync = nil
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
