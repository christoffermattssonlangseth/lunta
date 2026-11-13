import SwiftUI
import AuthenticationServices
import LuntaCore

/// Keeps a copy of the iCloud library in Google Drive (a "Lunta" folder), so Android (and other
/// devices that sync through the Drive API) share the library with the Mac and the iPhone.
///
/// The Mac is the bridge: the core's Syncer in bridge mode passes every device's changes both
/// ways (the iPhone's arrive here through iCloud, Android's through Drive). Signed in with the
/// same Google OAuth client as the Android app; tokens and sync state stay on this Mac.
@MainActor
final class DriveBridge: ObservableObject {
    static let shared = DriveBridge()

    /// The Google OAuth client (an "iOS" client, which allows the app's own redirect scheme; the
    /// Android app uses it too). Not a secret.
    nonisolated static let clientID = "340661790250-jvrurgnq8d8ctjf4mhq1p4nd9sugh4en.apps.googleusercontent.com"
    nonisolated static var redirectScheme: String { "com.googleusercontent.apps." + clientID.replacingOccurrences(of: ".apps.googleusercontent.com", with: "") }
    nonisolated static var redirectURI: String { redirectScheme + ":/oauth2redirect" }
    static let folderName = "Lunta"
    static let interval: TimeInterval = 5 * 60

    @Published private(set) var connected = false
    @Published private(set) var syncing = false
    @Published private(set) var progress: String?
    @Published private(set) var lastSync: Date?
    @Published private(set) var lastSummary: String?
    @Published private(set) var error: String?

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
        self.model = model
        let auth = DriveAuth(clientId: Self.clientID, tokenFile: Self.dir.appendingPathComponent("google-tokens.json").path, fullAccess: true)
        self.auth = auth
        connected = auth.isConnected()
        lastSync = (UserDefaults.standard.object(forKey: "driveLastSync") as? Date)
        if connected { begin() }
    }

    private func begin() {
        guard let model, let auth else { return }
        let listener = Listener { [weak self] line in Task { @MainActor in self?.progress = line } }
        do {
            sync = try LibrarySync.googleDriveBridge(root: model.libraryURL.path, device: LibraryModel.deviceID,
                                                     statePath: Self.dir.appendingPathComponent("drive-bridge-state.json").path,
                                                     auth: auth, folderName: Self.folderName, progress: listener)
        } catch {
            self.error = error.localizedDescription
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
                    if s.changed, let model = self.model {
                        try? model.lib.refresh()
                        model.reload()
                    }
                case .failure(let e):
                    self.error = e.localizedDescription
                }
            }
        }
    }

    /// Google sign-in in a system web sheet; the redirect comes back to the client's scheme.
    func connect() {
        guard let auth else { return }
        error = nil
        let req: SignInRequest
        do { req = try auth.beginSignIn(redirectUri: Self.redirectURI) } catch { self.error = error.localizedDescription; return }
        guard let url = URL(string: req.url) else { return }
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
                    let ok = await Task.detached { Result { try auth.completeSignIn(code: code, verifier: req.verifier, redirectUri: Self.redirectURI) } }.value
                    self.syncing = false
                    try ok.get()
                    self.connected = true
                    self.begin()
                } catch {
                    self.error = error.localizedDescription
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
        Task.detached { try? auth?.disconnect() }
        connected = false
        lastSummary = nil
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

/// Lunta › Google Drive for Android…
struct DriveBridgeView: View {
    @ObservedObject var bridge = DriveBridge.shared

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Label("Google Drive for Android", systemImage: "externaldrive.badge.icloud").font(.title2.weight(.semibold))
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
                            if let p = bridge.progress, let m = p.firstMatch(of: /^(\d+) of (\d+) · (.*)$/),
                               let done = Double(m.1), let total = Double(m.2), total > 0 {
                                ProgressView(value: done, total: total) {
                                    Text("\(Int(done).formatted()) of \(Int(total).formatted()) files · \(Int(total - done).formatted()) left")
                                } currentValueLabel: {
                                    Text(String(m.3)).lineLimit(1).truncationMode(.middle)
                                }
                            } else {
                                HStack(spacing: 8) {
                                    ProgressView().controlSize(.small)
                                    Text(bridge.progress ?? "Syncing…").lineLimit(1).truncationMode(.middle).foregroundStyle(.secondary)
                                }
                            }
                            Text("The first sync copies the whole library to Drive, which can take a while.").font(.caption).foregroundStyle(.secondary)
                        } else if let last = bridge.lastSync {
                            Text("Last sync \(last.formatted(.relative(presentation: .named)))\(bridge.lastSummary.map { ": \($0)" } ?? "")")
                                .foregroundStyle(.secondary)
                        }
                        Button("Disconnect", role: .destructive) { bridge.disconnect() }.buttonStyle(.link)
                    } else {
                        HStack {
                            Text("Sign in with the Google account the Android app uses.")
                            Spacer()
                            Button("Connect Google Drive…") { bridge.connect() }.buttonStyle(.borderedProminent).disabled(bridge.syncing)
                        }
                    }
                    if let e = bridge.error { Text(e).foregroundStyle(.red).font(.callout) }
                }
                .padding(6)
            }
        }
        .padding(20)
        .frame(width: 560)
    }
}
