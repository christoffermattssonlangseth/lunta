import SwiftUI

// Where the library lives: the first-launch question, Settings' Library Storage (see what it is,
// move it), and Google Drive's sync status. Decisions: StorageDecision.swift; moving:
// LibraryLocation.swift; syncing: DriveLibrarySync.swift.

/// Words used in the question and in Settings.
enum StorageText {
    static let privacy = "Lunta has no server and no account: your papers, highlights and notes go only to the place you choose."

    static func explanation(_ s: LibraryStorage) -> String {
        switch s {
        case .iCloud:
            return "Recommended if all your devices are Apple’s. Your Mac, iPhone and iPad keep the library in sync by themselves, through your own iCloud Drive."
        case .googleDrive:
            return "For when you also use Lunta on Android, Windows or Linux, or can’t use iCloud (on a work Mac, say). The library is kept on this device and synced with a “Lunta” folder in your own Google Drive while Lunta is open."
        case .device:
            return "No syncing: the library stays on this device. Good for trying Lunta out; you can move it to iCloud or Google Drive later."
        }
    }

    static var iCloudUnavailable: String {
        #if os(macOS)
        return "iCloud isn’t available on this Mac. To use it, sign in to iCloud with iCloud Drive turned on in System Settings, then open Lunta again."
        #else
        return "iCloud isn’t available on this device. To use it, sign in to iCloud with iCloud Drive turned on in Settings, then open Lunta again."
        #endif
    }

    static func icon(_ s: LibraryStorage) -> String {
        switch s {
        case .iCloud: return "icloud"
        case .googleDrive: return "externaldrive.badge.icloud"
        #if os(macOS)
        case .device: return "desktopcomputer"
        #else
        case .device: return UIDevice.current.userInterfaceIdiom == .pad ? "ipad" : "iphone"
        #endif
        }
    }

    /// "in …"
    static func place(_ s: LibraryStorage) -> String {
        s == .device ? "a folder on \(thisDevice) only" : s.name
    }

    static var thisDevice: String {
        #if os(macOS)
        return "this Mac"
        #else
        return UIDevice.current.userInterfaceIdiom == .pad ? "this iPad" : "this iPhone"
        #endif
    }
}

// MARK: - First launch

/// "Where should your library live?": shown on a device with no library yet.
struct StorageChoiceView: View {
    let recommended: LibraryStorage
    let iCloudAvailable: Bool
    /// The user chose (and, for Google Drive, signed in): open the library there.
    let onChosen: (LibraryStorage) -> Void
    @State private var selection: LibraryStorage?
    @ObservedObject private var drive = DriveLibrarySync.shared

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                VStack(alignment: .leading, spacing: 8) {
                    Text("Where should your library live?").font(.largeTitle.weight(.bold))
                    Text(StorageText.privacy).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                }
                ForEach(LibraryStorage.allCases, id: \.self) { s in option(s) }
                if let e = drive.error, (selection ?? recommended) == .googleDrive {
                    Text(e).font(.callout).foregroundStyle(.red).fixedSize(horizontal: false, vertical: true)
                }
                HStack {
                    Text(changeLater).font(.footnote).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                    Spacer()
                    if drive.signingIn { ProgressView().controlSize(.small) }
                    Button(chosen == .googleDrive ? "Sign In with Google…" : "Continue") { go() }
                        .buttonStyle(.borderedProminent)
                        .disabled(drive.signingIn || (chosen == .iCloud && !iCloudAvailable))
                        .keyboardShortcut(.defaultAction)
                }
            }
            .padding(28)
            .frame(maxWidth: 620)
            .frame(maxWidth: .infinity)
        }
        .tint(.luntaTeal)
    }

    private var chosen: LibraryStorage { selection ?? recommended }

    private var changeLater: String {
        #if os(macOS)
        return "You can change this later in Lunta › Settings."
        #else
        return "You can change this later in Library Storage, at the end of the library’s list."
        #endif
    }

    private func option(_ s: LibraryStorage) -> some View {
        let on = chosen == s
        let off = s == .iCloud && !iCloudAvailable
        return Button { selection = s } label: {
            HStack(alignment: .top, spacing: 14) {
                Image(systemName: on ? "largecircle.fill.circle" : "circle")
                    .font(.title3).foregroundStyle(on ? Color.luntaTeal : .secondary)
                VStack(alignment: .leading, spacing: 6) {
                    HStack(spacing: 8) {
                        Label(s.name, systemImage: StorageText.icon(s)).font(.headline)
                        if s == recommended && !off {
                            Text("Recommended").font(.caption.weight(.semibold)).foregroundStyle(.white)
                                .padding(.horizontal, 7).padding(.vertical, 2).background(Capsule().fill(Color.luntaTeal))
                        }
                    }
                    Text(off ? StorageText.iCloudUnavailable : StorageText.explanation(s))
                        .font(.callout).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                    if s == .googleDrive, let note = GoogleDriveClient.unverifiedNote {
                        Text(note).font(.footnote).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                    }
                }
                Spacer(minLength: 0)
            }
            .padding(14)
            .contentShape(Rectangle())
            .background(RoundedRectangle(cornerRadius: 12).strokeBorder(on ? Color.luntaTeal : Color.secondary.opacity(0.3), lineWidth: on ? 2 : 1))
        }
        .buttonStyle(.plain)
        .disabled(off || drive.signingIn)
        .opacity(off ? 0.6 : 1)
    }

    private func go() {
        let s = chosen
        if s == .googleDrive {
            Task { if await DriveLibrarySync.shared.signIn() { onChosen(.googleDrive) } }
        } else {
            onChosen(s)
        }
    }
}

/// Said once, on a new device that found the library in iCloud and opened it.
enum FoundInICloud {
    static let title = "Your library is in iCloud"
    static let message = "Lunta found the library your other Apple devices keep in iCloud Drive and opened it. If you’d rather keep it in Google Drive (to use it on Android, Windows or Linux too), you can move it in Library Storage."
    static let switchButton = "Use Google Drive Instead…"
}

// MARK: - Settings: Library Storage

/// Moves the library to another place: copies it (the old copy stays), remembers the new place
/// and reopens the library there.
@MainActor
final class LibraryMover: ObservableObject {
    static let shared = LibraryMover()
    /// Opens the library again in its new place: the Mac restarts Lunta, the iPhone and iPad reopen
    /// it in the running app. Gets the "your library is now in…" text. Set by the app's ModelHolder.
    var reopen: (@MainActor (String) -> Void)?

    @Published private(set) var working = false
    @Published private(set) var progress: String?
    @Published private(set) var done: String?
    @Published private(set) var error: String?

    func move(_ model: LibraryModel, to target: LibraryStorage) async {
        let from = LibraryLocation.current(inCloud: model.inCloud)
        guard target != from, !working else { return }
        error = nil
        done = nil
        let drive = DriveLibrarySync.shared
        if target == .googleDrive {
            #if os(macOS)
            drive.takeOverBridgeSignIn()   // the Mac already copies its iCloud library to Drive: same sign-in
            #endif
            if !drive.connected {
                guard await drive.signIn() else {
                    error = drive.error ?? "Lunta needs to be signed in to Google to keep the library in Google Drive."
                    return
                }
            }
        }
        working = true
        defer { working = false; progress = nil }
        if from == .googleDrive {
            progress = "Syncing with Google Drive first…"
            await drive.syncAndWait()
            drive.stop()
        }
        progress = "Copying your library…"
        let src = model.libraryURL
        guard let dst = await Task.detached(operation: { LibraryLocation.root(for: target) }).value else {
            error = StorageText.iCloudUnavailable
            if from == .googleDrive { drive.start(model) }
            return
        }
        let own = LibraryLocation.ownLogs
        let result = await Task.detached {
            await LibraryLocation.copyLibrary(from: src, to: dst, ownLogs: own) { line in
                Task { @MainActor in LibraryMover.shared.progress = line }
            }
        }.value
        if !result.failed.isEmpty {
            error = "\(result.failed.count) file(s) couldn’t be copied (for example \(result.failed[0])), so your library stays where it is, in \(StorageText.place(from)). Try again when you’re online."
            if from == .googleDrive { drive.start(model) }
            return
        }
        LibraryStorage.stored = target
        var text = "Your library is now in \(StorageText.place(target)). The old copy is still \(LibraryLocation.oldCopyDescription(from, root: src)); Lunta doesn’t use it any more, and nothing in it was changed."
        if target == .googleDrive {
            text += " If your Google Drive already had a Lunta library (from Android, say), the two are combined at the first sync."
        }
        done = text
        reopen?(text)
    }

    func clear() { done = nil; error = nil }

    func failedToReopen(_ message: String) {
        done = nil
        error = "The library was copied, but couldn’t be opened in its new place (\(message)). Lunta opens it there when it next starts."
    }
}

/// Settings › Library Storage: where the library is, its Google Drive sync, and moving it.
struct LibraryStorageView: View {
    @EnvironmentObject var model: LibraryModel
    @ObservedObject private var mover = LibraryMover.shared
    @ObservedObject private var drive = DriveLibrarySync.shared
    @State private var confirming: LibraryStorage?
    @State private var iCloudAvailable = true
    #if os(macOS)
    @Environment(\.openWindow) private var openWindow
    #endif

    private var current: LibraryStorage { LibraryLocation.current(inCloud: model.inCloud) }

    var body: some View {
        Form {
            Section {
                Label(current.name, systemImage: StorageText.icon(current)).font(.headline)
                Text(where_).font(.callout).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                if current == .googleDrive { DriveSyncStatusView() }
            } header: { Text("Your library is in") }

            Section {
                if mover.working {
                    HStack(spacing: 10) {
                        ProgressView().controlSize(.small)
                        Text(mover.progress ?? "Moving…").foregroundStyle(.secondary).lineLimit(2)
                    }
                } else if let d = mover.done {
                    Text(d).fixedSize(horizontal: false, vertical: true)
                } else {
                    ForEach(LibraryStorage.allCases.filter { $0 != current }, id: \.self) { s in
                        VStack(alignment: .leading, spacing: 4) {
                            Button(moveTitle(s)) { confirming = s }
                                .disabled(s == .iCloud && !iCloudAvailable)
                            Text(s == .iCloud && !iCloudAvailable ? StorageText.iCloudUnavailable : StorageText.explanation(s))
                                .font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                        }
                    }
                }
                if let e = mover.error { Text(e).font(.callout).foregroundStyle(.red).fixedSize(horizontal: false, vertical: true) }
            } header: { Text("Move it") } footer: {
                Text(StorageText.privacy)
            }
        }
        #if os(macOS)
        .formStyle(.grouped)
        .frame(width: 560)
        .frame(minHeight: 420)
        #endif
        .task { iCloudAvailable = await Task.detached { CloudLibrary.iCloudURL() != nil }.value }
        .alert(confirming.map { moveTitle($0).replacingOccurrences(of: "…", with: "?") } ?? "", isPresented: Binding(get: { confirming != nil }, set: { if !$0 { confirming = nil } }), presenting: confirming) { s in
            Button(s == .googleDrive ? "Sign In and Move" : "Move") { start(s) }
            #if os(macOS)
            if current == .iCloud && s == .googleDrive {
                Button("Keep iCloud and Also Sync with Google Drive") { openWindow(id: "drive") }
            }
            #endif
            Button("Cancel", role: .cancel) {}
        } message: { s in
            Text(confirmText(s))
        }
    }

    private var where_: String {
        switch current {
        case .iCloud:
            return model.inCloud ? "Synced with your other Apple devices through your iCloud Drive."
                : "iCloud isn’t available right now, so Lunta is using the copy on \(StorageText.thisDevice)."
        case .googleDrive:
            return "Kept on \(StorageText.thisDevice) and synced with the “\(DriveLibrarySync.folderName)” folder in your Google Drive."
        case .device:
            #if os(macOS)
            return "Only on this Mac, in \((model.libraryURL.path as NSString).abbreviatingWithTildeInPath). Not synced."
            #else
            return "Only on \(StorageText.thisDevice). Not synced."
            #endif
        }
    }

    private func moveTitle(_ s: LibraryStorage) -> String {
        switch s {
        case .iCloud: return "Move to iCloud Drive…"
        case .googleDrive: return "Move to Google Drive…"
        case .device: return "Keep Only on \(StorageText.thisDevice.prefix(1).uppercased() + StorageText.thisDevice.dropFirst())…"
        }
    }

    private func confirmText(_ s: LibraryStorage) -> String {
        var t = "Lunta copies your library (papers, notes, highlights, reading positions) to \(StorageText.place(s)) and opens it there. The copy where it is now is left as it is, so nothing is lost."
        if current == .iCloud && s != .iCloud {
            #if os(macOS)
            t += "\n\nYour other Apple devices keep using the iCloud library unless you move them too."
            if s == .googleDrive {
                t += " To use Android, Windows or Linux and keep iCloud, let this Mac also sync the iCloud library with Google Drive instead."
            }
            #else
            t += "\n\nYour other Apple devices keep using the iCloud library unless you move them too."
            if s == .googleDrive {
                t += " Keeping iCloud and also syncing with Google Drive is something a Mac does (Lunta › Also Sync with Google Drive… on the Mac); an iPhone or iPad can’t do both."
            }
            #endif
        }
        if s == .googleDrive {
            t += "\n\nIf your Google Drive already has a Lunta library (from Android, say), the two are combined."
            if !drive.connected, let note = GoogleDriveClient.unverifiedNote { t += "\n\n" + note }
        }
        return t
    }

    private func start(_ s: LibraryStorage) {
        Task { await mover.move(model, to: s) }
    }
}

/// The Google Drive library's sync: signed in or not, last sync, progress, errors.
struct DriveSyncStatusView: View {
    @ObservedObject private var drive = DriveLibrarySync.shared

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            if drive.connected {
                HStack {
                    Image(systemName: "checkmark.circle.fill").foregroundStyle(Color.luntaTeal)
                    Text("Signed in · syncs every 5 minutes while Lunta is open")
                    Spacer()
                    Button("Sync Now") { drive.syncNow() }.disabled(drive.syncing)
                }
                if drive.syncing {
                    SyncProgressLine(progress: drive.progress)
                    if drive.lastSync == nil {
                        Text("The first sync copies the whole library, which can take a while.").font(.caption).foregroundStyle(.secondary)
                    }
                } else if let last = drive.lastSync {
                    Text("Last sync \(last.formatted(.relative(presentation: .named)))\(drive.lastSummary.map { ": \($0)" } ?? "")")
                        .foregroundStyle(.secondary)
                }
                if let n = drive.notice {
                    Text(n).font(.callout).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                }
                HStack {
                    Button("Sign Out of Google", role: .destructive) { drive.disconnect() }
                    Spacer()
                    Button("Choose Your Lunta Folder…") { Task { await drive.chooseFolder() } }.disabled(drive.syncing || drive.signingIn)
                }
                #if os(macOS)
                .buttonStyle(.link)
                #else
                .buttonStyle(.borderless)
                #endif
            } else {
                Text("Not signed in to Google, so the library isn’t syncing. It stays on \(StorageText.thisDevice) meanwhile.")
                    .fixedSize(horizontal: false, vertical: true)
                Button("Sign In with Google…") { Task { await drive.signIn() } }.disabled(drive.signingIn)
                if let note = GoogleDriveClient.unverifiedNote {
                    Text(note).font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                }
            }
            if let e = drive.error { Text(e).foregroundStyle(.red).font(.callout).fixedSize(horizontal: false, vertical: true) }
            if drive.folderNotVisible {
                Button("Choose Your Lunta Folder…") { Task { await drive.chooseFolder() } }.disabled(drive.syncing || drive.signingIn)
            }
        }
    }
}

/// A sync's progress line from the core ("312 of 2676 · upload papers/…"): a bar with the count
/// and the file, or a spinner with the line.
struct SyncProgressLine: View {
    let progress: String?

    var body: some View {
        if let p = progress, let m = p.firstMatch(of: /^(\d+) of (\d+) · (.*)$/),
           let done = Double(m.1), let total = Double(m.2), total > 0 {
            ProgressView(value: done, total: total) {
                Text("\(Int(done).formatted()) of \(Int(total).formatted()) files · \(Int(total - done).formatted()) left")
            } currentValueLabel: {
                Text(String(m.3)).lineLimit(1).truncationMode(.middle)
            }
        } else {
            HStack(spacing: 8) {
                ProgressView().controlSize(.small)
                Text(progress ?? "Syncing…").lineLimit(1).truncationMode(.middle).foregroundStyle(.secondary)
            }
        }
    }
}

/// What an empty library says while it may still be arriving from iCloud or Google Drive.
@MainActor
enum EmptyLibraryText {
    static func text(_ model: LibraryModel, otherwise: String) -> String {
        if model.inCloud { return "Your library is syncing from iCloud…" }
        if LibraryStorage.stored == .googleDrive, DriveLibrarySync.shared.connected { return "Your library is syncing from Google Drive…" }
        return otherwise
    }
}

/// A short line about the Google Drive sync for the library's sidebar: syncing, last sync, or a problem.
struct DriveSyncSummary: View {
    @ObservedObject private var drive = DriveLibrarySync.shared

    var body: some View {
        if !drive.connected {
            Text("Google Drive: not signed in").foregroundStyle(.orange)
        } else if drive.syncing {
            Text(drive.lastSync == nil ? "Getting your library from Google Drive…" : "Syncing with Google Drive…")
        } else if drive.error != nil {
            Text("Google Drive: sync problem").foregroundStyle(.red)
        } else if let last = drive.lastSync {
            Text("Google Drive · synced \(last.formatted(.relative(presentation: .named)))")
        } else {
            Text("Google Drive")
        }
    }
}
