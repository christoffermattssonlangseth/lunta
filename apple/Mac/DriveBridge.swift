import SwiftUI
import AuthenticationServices
import LuntaCore

/// Keeps a copy of the iCloud library in Google Drive (a "Lunta" folder), so Android (and other
/// devices that sync through the Drive API) share the library with the Mac and the iPhone.
///
/// The Mac is the bridge: the core's Syncer in bridge mode passes every device's changes both
/// ways (the iPhone's arrive here through iCloud, Android's through Drive). Signed in with the
/// same Google OAuth client as the Android app; tokens and sync state stay on this Mac.
///
/// Access is Google's `drive.file` scope: Lunta sees only the files its clients created (and what
/// the user picks for it in Google's picker, "Choose Your Lunta Folder…"). A token from an older
/// version still has the full scope; while it does, syncs adopt files other apps put into the
/// folder (app-made copies, originals kept aside), and "Sign In Again" then narrows the access.
@MainActor
final class DriveBridge: ObservableObject {
    static let shared = DriveBridge()

    /// The Google OAuth client (an "iOS" client, which allows the app's own redirect scheme; the
    /// Android app uses it too). Not a secret.
    nonisolated static var clientID: String { GoogleDriveClient.clientID }
    nonisolated static var redirectScheme: String { GoogleDriveClient.redirectScheme }
    nonisolated static var redirectURI: String { GoogleDriveClient.redirectURI }
    static let folderName = "Lunta"
    static let interval: TimeInterval = 5 * 60

    @Published private(set) var connected = false
    @Published private(set) var syncing = false
    @Published private(set) var progress: String?
    @Published private(set) var lastSync: Date?
    @Published private(set) var lastSummary: String?
    @Published private(set) var error: String?
    /// The last sync could not open the Drive folder: offer "Choose Your Lunta Folder…".
    @Published private(set) var folderNotVisible = false
    /// Something the last sync wants the user to know (files adopted for the narrower access).
    @Published private(set) var notice: String?
    /// Signed in by an older version with access to the whole Drive.
    @Published private(set) var fullAccess = false
    /// The folder chosen in Google's picker, if any (otherwise "Lunta" in My Drive, by name).
    private var folderID: String? {
        get { UserDefaults.standard.string(forKey: "driveFolderId").flatMap { $0.isEmpty ? nil : $0 } }
        set { UserDefaults.standard.set(newValue, forKey: "driveFolderId") }
    }

    private weak var model: LibraryModel?
    private var auth: DriveAuth?
    private var sync: LibrarySync?
    private var timer: Timer?
    private var signIn: ASWebAuthenticationSession?
    private let anchor = AuthAnchor()

    private static var dir: URL {
        let d = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent("Lunta", isDirectory: true)
        try? FileManager.default.createDirectory(at: d, withIntermediateDirectories: true)
        return d
    }

    /// Called once the library is open: resumes syncing if this Mac was connected before.
    func start(_ model: LibraryModel) {
        // a library that lives in Google Drive syncs itself (DriveLibrarySync): nothing to bridge
        guard LibraryStorage.stored != .googleDrive else { return }
        self.model = model
        let auth = DriveAuth(clientId: Self.clientID, tokenFile: Self.dir.appendingPathComponent("google-tokens.json").path, fullAccess: false)
        self.auth = auth
        connected = auth.isConnected()
        fullAccess = connected && auth.hasFullAccess()
        lastSync = (UserDefaults.standard.object(forKey: "driveLastSync") as? Date)
        if connected { begin() }
    }

    private func begin() {
        guard let model, let auth else { return }
        let listener = Listener { [weak self] line in Task { @MainActor in self?.progress = line } }
        do {
            sync = try LibrarySync.googleDriveBridge(root: model.libraryURL.path, device: LibraryModel.deviceID,
                                                     statePath: Self.dir.appendingPathComponent("drive-bridge-state.json").path,
                                                     auth: auth, folderName: Self.folderName, folderId: folderID, progress: listener)
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

    func syncNow() {
        guard let sync, !syncing else { return }
        syncing = true
        error = nil
        let auth = auth
        Task.detached {
            let result = Result { try sync.syncNow() }
            await MainActor.run {
                self.syncing = false
                self.progress = nil
                switch result {
                case .success(let s):
                    self.lastSync = Date()
                    UserDefaults.standard.set(self.lastSync, forKey: "driveLastSync")
                    var parts: [String] = []
                    if s.uploaded > 0 { parts.append("\(s.uploaded) sent to Drive") }
                    if s.downloaded > 0 { parts.append("\(s.downloaded) received") }
                    if s.deletedLocal + s.deletedRemote > 0 { parts.append("\(s.deletedLocal + s.deletedRemote) removed") }
                    self.lastSummary = parts.isEmpty ? "Up to date" : parts.joined(separator: ", ")
                    if !s.errors.isEmpty { self.error = "\(s.errors.count) file(s) failed: \(s.errors.first ?? "")" }
                    if let n = s.notices.first { self.notice = n }
                    self.folderNotVisible = false
                    self.fullAccess = auth?.hasFullAccess() ?? false
                    if s.changed, let model = self.model {
                        try? model.lib.refresh()
                        model.reload()
                    }
                case .failure(let e):
                    self.error = e.readable
                    self.folderNotVisible = driveFolderNotVisible(message: e.readable)
                }
            }
        }
    }

    /// Google sign-in in a system web sheet; the redirect comes back to the client's scheme.
    func connect() {
        guard let auth else { return }
        error = nil
        let req: SignInRequest
        do { req = try auth.beginSignIn(redirectUri: Self.redirectURI) } catch { self.error = error.readable; return }
        signIn(req, picking: false)
    }

    /// "Choose Your Lunta Folder…": Google's picker inside the sign-in. Needed when the Drive folder
    /// was made by another app (or Lunta's access to it was removed). Picking a folder grants the
    /// folder, not files other apps put into it; those can be selected in the picker too.
    func chooseFolder() {
        guard let auth else { return }
        error = nil
        let req: SignInRequest
        do { req = try auth.beginPick(redirectUri: Self.redirectURI, fileIds: []) } catch { self.error = error.readable; return }
        signIn(req, picking: true)
    }

    private func signIn(_ req: SignInRequest, picking: Bool) {
        guard let auth, let url = URL(string: req.url) else { return }
        let session = ASWebAuthenticationSession(url: url, callback: .customScheme(Self.redirectScheme)) { [weak self] callback, err in
            Task { @MainActor in
                guard let self else { return }
                self.signIn = nil
                guard let callback else {
                    if let err = err as? ASWebAuthenticationSessionError, err.code == .canceledLogin { return }
                    self.error = err?.localizedDescription ?? "Sign-in didn't finish."
                    return
                }
                do {
                    let code = try auth.codeFromRedirect(redirectUrl: callback.absoluteString, expectedState: req.state)
                    self.syncing = true
                    let picked = picking ? auth.pickedIdsFromRedirect(redirectUrl: callback.absoluteString) : []
                    let ok = await Task.detached { () -> Result<[String], Error> in
                        Result {
                            try auth.completeSignIn(code: code, verifier: req.verifier, redirectUri: Self.redirectURI)
                            return picking ? try auth.pickedFolder(ids: picked) : []
                        }
                    }.value
                    self.syncing = false
                    let folder = try ok.get()
                    if folder.count == 2 { self.folderID = folder[0] }
                    else if picking { self.notice = "No folder was chosen, so Lunta keeps using its own “\(Self.folderName)” folder." }
                    self.connected = true
                    self.fullAccess = auth.hasFullAccess()
                    self.begin()
                } catch {
                    self.error = error.readable
                }
            }
        }
        session.presentationContextProvider = anchor
        session.prefersEphemeralWebBrowserSession = false
        signIn = session
        session.start()
    }

    func disconnect() {
        timer?.invalidate()
        timer = nil
        sync = nil
        let auth = auth
        Task.detached { try? auth?.disconnect() }   // forgets the tokens here; the grant stays (see the core)
        connected = false
        lastSummary = nil
        notice = nil
        folderNotVisible = false
        fullAccess = false
        folderID = nil
    }

    /// Progress lines from the core's Syncer ("upload papers/…/paper.pdf").
    final class Listener: ProgressListener, @unchecked Sendable {
        let handler: (String) -> Void
        init(_ handler: @escaping (String) -> Void) { self.handler = handler }
        func onProgress(message: String) { handler(message) }
    }

    final class AuthAnchor: NSObject, ASWebAuthenticationPresentationContextProviding {
        func presentationAnchor(for session: ASWebAuthenticationSession) -> ASPresentationAnchor {
            NSApp.keyWindow ?? NSApp.windows.first ?? ASPresentationAnchor()
        }
    }
}

/// Lunta › Also Sync with Google Drive… (for an iCloud library; a library kept in Google Drive
/// syncs by itself, see Library Storage)
struct DriveBridgeView: View {
    @ObservedObject var bridge = DriveBridge.shared
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Label("Also Sync with Google Drive", systemImage: "externaldrive.badge.icloud").font(.title2.weight(.semibold))
            if LibraryStorage.stored == .googleDrive {
                Text("Your library already lives in Google Drive, so there is nothing more to sync here. Where it lives is in Library Storage.")
                    .foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                Button("Library Storage…") { openWindow(id: "storage") }
            } else {
                bridgeBody
            }
        }
        .padding(20)
        .frame(width: 560)
    }

    @ViewBuilder private var bridgeBody: some View {
            Text("Your library stays in iCloud for the Mac and the iPhone. This Mac also keeps a copy in a “Lunta” folder in your Google Drive, which the Android app (and Lunta on Windows and Linux) use. Changes go both ways: papers, notes, stars, Must Read, reading positions.")
                .foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            GroupBox {
                VStack(alignment: .leading, spacing: 10) {
                    if bridge.connected {
                        HStack {
                            Image(systemName: "checkmark.circle.fill").foregroundStyle(Color.luntaTeal)
                            Text("Connected · syncs every 5 minutes while Lunta is open")
                            Spacer()
                            Button("Sync Now") { bridge.syncNow() }.disabled(bridge.syncing)
                        }
                        if bridge.syncing {
                            // "312 of 2676 · upload papers/…": a bar with the count, the file below
                            SyncProgressLine(progress: bridge.progress)
                            Text("The first sync copies the whole library to Drive, which can take a while.").font(.caption).foregroundStyle(.secondary)
                        } else if let last = bridge.lastSync {
                            Text("Last sync \(last.formatted(.relative(presentation: .named)))\(bridge.lastSummary.map { ": \($0)" } ?? "")")
                                .foregroundStyle(.secondary)
                        }
                        if let n = bridge.notice {
                            Text(n).font(.callout).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                        }
                        if bridge.fullAccess && !bridge.syncing && bridge.error == nil && bridge.lastSync != nil {
                            HStack {
                                Text("Lunta now asks Google only for access to the files it makes, not your whole Drive. The Drive folder has been prepared for it; sign in again to switch.")
                                    .font(.callout).fixedSize(horizontal: false, vertical: true)
                                Spacer()
                                Button("Sign In Again…") { bridge.connect() }
                            }
                        }
                        HStack {
                            Button("Disconnect", role: .destructive) { bridge.disconnect() }.buttonStyle(.link)
                            Spacer()
                            Button("Choose Your Lunta Folder…") { bridge.chooseFolder() }.buttonStyle(.link).disabled(bridge.syncing)
                        }
                    } else {
                        HStack {
                            Text("Sign in with the Google account the Android app uses.")
                            Spacer()
                            Button("Connect Google Drive…") { bridge.connect() }.buttonStyle(.borderedProminent).disabled(bridge.syncing)
                        }
                        if let note = GoogleDriveClient.unverifiedNote {
                            Text(note).font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                        }
                    }
                    if let e = bridge.error { Text(e).foregroundStyle(.red).font(.callout) }
                    if bridge.folderNotVisible {
                        Button("Choose Your Lunta Folder…") { bridge.chooseFolder() }.disabled(bridge.syncing)
                    }
                }
                .padding(6)
            }
    }
}
