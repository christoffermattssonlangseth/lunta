import SwiftUI
import LuntaCore

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
            #if os(macOS)
            return "For when you also use Lunta on Android, Windows or Linux, or can’t use iCloud (on a work Mac, say). The library is kept on this device and synced with a “Lunta” folder in your own Google Drive while Lunta is open."
            #else
            return "For when you also use Lunta on Android, Windows or Linux, or can’t use iCloud. The library is synced with a “Lunta” folder in your own Google Drive while Lunta is open; a paper’s PDF and reading view are downloaded when you open it."
            #endif
        case .device:
            return "No syncing: the library stays on this device. Good for trying Lunta out; you can move it to iCloud or Google Drive later."
        case .folder:
            return "For a work Mac with OneDrive, SharePoint, Dropbox, Box or a network drive instead of iCloud or Google: the library is kept in a folder you choose there, and that app syncs it (Lunta on Windows and Linux can open the same folder)."
        }
    }

    /// Google Drive's space, and this library's size when it's known.
    static func driveSpace(_ bytes: UInt64?) -> String {
        var t = "Uses space in your Google Drive (free accounts have 15 GB, shared with Gmail and Photos)."
        if let bytes, bytes > 0 {
            t += " This library: about \(ByteCountFormatter.string(fromByteCount: Int64(bytes), countStyle: .file))."
        }
        return t
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
        case .folder: return "folder"
        #if os(macOS)
        case .device: return "desktopcomputer"
        #else
        case .device: return UIDevice.current.userInterfaceIdiom == .pad ? "ipad" : "iphone"
        #endif
        }
    }

    /// "in …"
    static func place(_ s: LibraryStorage) -> String {
        switch s {
        case .device: return "a folder on \(thisDevice) only"
        case .folder: return "the folder you chose"
        default: return s.name
        }
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
                ForEach(LibraryStorage.offered.filter { $0 != .folder }, id: \.self) { s in option(s) }
                #if os(macOS)
                // a work Mac without iCloud or Google: a folder in OneDrive, SharePoint, Dropbox or on a network drive
                VStack(alignment: .leading, spacing: 4) {
                    Button("Another Folder… (OneDrive, a shared drive)") { Task { await chooseFolder() } }
                        .buttonStyle(.link)
                        .disabled(drive.signingIn)
                    Text("Keep the library in a folder you choose; OneDrive, Dropbox, Box or the network drive syncs it.")
                        .font(.footnote).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                }
                .padding(.horizontal, 14)
                #endif
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
                    if s == .googleDrive {
                        // (no library here yet, so no size)
                        Text(StorageText.driveSpace(nil)).font(.footnote).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                    }
                    if s == .googleDrive, let note = GoogleDriveClient.signInNote {
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

    #if os(macOS)
    /// Another Folder…: pick it, and (an empty one) confirm starting a new library there.
    private func chooseFolder() async {
        guard let pick = await FolderChooser.choose(currentLibrary: nil) else { return }
        switch pick {
        case .refused(let t): FolderChooser.refuse(t)
        case .use(let url, let existing):
            if !existing, !FolderChooser.confirmNew(url) { return }
            ChosenFolder.save(url)
            onChosen(.folder)
        }
    }
    #endif

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

    /// `folder`: the folder to move to, for `.folder` (picked in Library Storage).
    func move(_ model: LibraryModel, to target: LibraryStorage, folder: URL? = nil) async {
        let from = LibraryLocation.current(inCloud: model.inCloud)
        guard target != from || (target == .folder && folder != nil), !working else { return }
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
            // the copy is made from this device's files: they must have every change from the other devices first
            if let problem = await drive.syncAndWait() {
                error = "Your library stays in Google Drive, because the last sync with it didn’t finish, so \(StorageText.thisDevice) may not have every change yet. \(problem)"
                return
            }
            // papers whose files are only in Drive (downloaded when opened) come down first, so the new place has them
            if let problem = await drive.fetchEverything(report: { self.progress = $0 }) {
                error = "Your library stays in Google Drive, because not every paper could be downloaded to \(StorageText.thisDevice) first. \(problem)"
                return
            }
            let left = await drive.papersNotDownloaded()
            if left > 0 {
                error = "Your library stays in Google Drive, because \(left) paper(s) still have files only in Google Drive. Try again when you’re online."
                return
            }
            drive.stop()
        }
        progress = "Copying your library…"
        let src = model.libraryURL
        guard let dst = await Task.detached(operation: { folder ?? LibraryLocation.root(for: target) }).value else {
            error = target == .iCloud ? StorageText.iCloudUnavailable : "Lunta couldn’t find the folder to move the library to."
            if from == .googleDrive { drive.start(model) }
            return
        }
        let joining = target == .folder && CloudLibrary.hasLogs(dst)   // a library from another computer there already
        let result = await Task.detached {
            await LibraryLocation.copyLibrary(from: src, to: dst) { line in
                Task { @MainActor in LibraryMover.shared.progress = line }
            }
        }.value
        if !result.failed.isEmpty {
            error = "\(result.failed.count) file(s) couldn’t be copied (for example \(result.failed[0])), so your library stays where it is, in \(StorageText.place(from)). Try again when you’re online."
            if from == .googleDrive { drive.start(model) }
            return
        }
        if target == .folder { ChosenFolder.save(dst) }
        LibraryStorage.stored = target
        let place = target == .folder ? ChosenFolder.describe(dst) : StorageText.place(target)
        var text = "Your library is now in \(place). The old copy is still \(LibraryLocation.oldCopyDescription(from, root: src)); Lunta doesn’t use it any more, and nothing in it was changed."
        if target == .googleDrive {
            text += " If your Google Drive already had a Lunta library (from Android, say), the two are combined at the first sync."
        }
        if joining {
            text += " The folder already had a Lunta library (from another computer, say): the two were combined."
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
    /// The folder picked for Move to a Folder (asked before the confirmation), and whether it has a library.
    @State private var pickedFolder: (url: URL, existing: Bool)?
    @State private var iCloudAvailable = true
    /// The library's size on disk (iCloud: including what isn't downloaded), for the Google Drive line.
    @State private var librarySize: UInt64?
    #if os(macOS)
    @Environment(\.openWindow) private var openWindow
    #endif

    private var current: LibraryStorage { LibraryLocation.current(inCloud: model.inCloud) }

    var body: some View {
        Form {
            Section {
                Label(current.name, systemImage: StorageText.icon(current)).font(.headline)
                Text(where_).font(.callout).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                if current == .googleDrive {
                    DriveSyncStatusView()
                    DriveDownloadsView()
                }
                #if os(macOS)
                if current == .folder {
                    Button("Show in Finder") { NSWorkspace.shared.activateFileViewerSelecting([model.libraryURL]) }
                        .buttonStyle(.link)
                }
                #endif
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
                    ForEach(LibraryStorage.offered.filter { $0 != current || $0 == .folder }, id: \.self) { s in
                        VStack(alignment: .leading, spacing: 4) {
                            Button(moveTitle(s)) { if s == .folder { pickFolder() } else { confirming = s } }
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
        .task {
            guard current != .googleDrive else { return }
            let root = model.libraryURL
            librarySize = await Task.detached(priority: .utility) { LibraryLocation.size(of: root) }.value
        }
        .alert(confirming.map { confirmTitle($0) } ?? "", isPresented: Binding(get: { confirming != nil }, set: { if !$0 { confirming = nil } }), presenting: confirming) { s in
            Button(s == .googleDrive ? "Sign In and Move" : "Move") { start(s) }
            #if os(macOS)
            if current == .iCloud && s == .googleDrive {
                Button("Keep iCloud and Also Sync with Google Drive") { openWindow(id: "drive") }
            }
            #endif
            Button("Cancel", role: .cancel) { pickedFolder = nil }
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
            if DriveLibrarySync.onDemand {
                return "Synced with the “\(DriveLibrarySync.folderName)” folder in your Google Drive. Papers, notes and highlights are kept on \(StorageText.thisDevice); a paper’s PDF and reading view are downloaded when you open it."
            }
            return "Kept on \(StorageText.thisDevice) and synced with the “\(DriveLibrarySync.folderName)” folder in your Google Drive."
        case .device:
            #if os(macOS)
            return "Only on this Mac, in \((model.libraryURL.path as NSString).abbreviatingWithTildeInPath). Not synced."
            #else
            return "Only on \(StorageText.thisDevice). Not synced."
            #endif
        case .folder:
            let service = FolderPick.service(forPath: model.libraryURL.path)
            let by = service.map { $0.hasPrefix("a ") ? "It’s \(ChosenFolder.serviceWords(model.libraryURL.path)), so it’s shared with every computer that has it." : "\($0) syncs it with your other computers." }
                ?? "Lunta doesn’t sync it itself: put it in a folder that OneDrive, Dropbox, Box or Google Drive for desktop syncs, or on a network drive, to use it on other computers."
            return "In \(ChosenFolder.describe(model.libraryURL)). \(by)"
        }
    }

    private func confirmTitle(_ s: LibraryStorage) -> String {
        if s == .folder, let f = pickedFolder { return "Move to “\(f.url.lastPathComponent)”?" }
        return moveTitle(s).replacingOccurrences(of: "…", with: "?")
    }

    /// Move to a Folder: the folder first (the confirmation names it).
    private func pickFolder() {
        #if os(macOS)
        Task {
            guard let pick = await FolderChooser.choose(currentLibrary: model.libraryURL) else { return }
            switch pick {
            case .refused(let t): FolderChooser.refuse(t)
            case .use(let url, let existing):
                if url.standardizedFileURL == model.libraryURL.standardizedFileURL {
                    FolderChooser.refuse("Your library is already in that folder.")
                    return
                }
                pickedFolder = (url, existing)
                confirming = .folder
            }
        }
        #endif
    }

    private func moveTitle(_ s: LibraryStorage) -> String {
        switch s {
        case .iCloud: return "Move to iCloud Drive…"
        case .googleDrive: return "Move to Google Drive…"
        case .device: return "Keep Only on \(StorageText.thisDevice.prefix(1).uppercased() + StorageText.thisDevice.dropFirst())…"
        case .folder: return current == .folder ? "Move to Another Folder…" : "Move to a Folder You Choose…"
        }
    }

    private func confirmText(_ s: LibraryStorage) -> String {
        if s == .folder, let f = pickedFolder {
            var t = "Lunta copies your library (papers, notes, highlights, reading positions) to \(ChosenFolder.describe(f.url)) and opens it there. The copy where it is now is left as it is, so nothing is lost."
            if f.existing { t += "\n\nThat folder already has a Lunta library (from another computer, say): the two are combined." }
            if current == .iCloud { t += "\n\nYour other Apple devices keep using the iCloud library unless you move them too." }
            t += "\n\n" + ChosenFolder.syncNote
            if librarySize.map({ $0 > 0 }) == true {
                t += " This library: about \(ByteCountFormatter.string(fromByteCount: Int64(librarySize!), countStyle: .file))."
            }
            return t
        }
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
            t += "\n\n" + StorageText.driveSpace(librarySize)
            t += "\n\nIf your Google Drive already has a Lunta library (from Android, say), the two are combined."
            if !drive.connected, let note = GoogleDriveClient.signInNote { t += "\n\n" + note }
        }
        return t
    }

    private func start(_ s: LibraryStorage) {
        let folder = s == .folder ? pickedFolder?.url : nil
        pickedFolder = nil
        Task { await mover.move(model, to: s, folder: folder) }
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
                        Text(DriveLibrarySync.onDemand
                             ? "The first sync gets the library’s papers, notes and highlights; PDFs and reading views stay in Google Drive until you open them."
                             : "The first sync copies the whole library, which can take a while.").font(.caption).foregroundStyle(.secondary)
                    }
                } else if let last = drive.lastSync {
                    Text("Last sync \(last.formatted(.relative(presentation: .named)))\(drive.lastSummary.map { ": \($0)" } ?? "")")
                        .foregroundStyle(.secondary)
                }
                if let n = drive.notice {
                    Text(n).font(.callout).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                }
                if drive.fullAccess && !drive.adoptionPending && !drive.needsSignIn && !drive.syncing && drive.error == nil && drive.lastSync != nil {
                    // signed in by 1.2 or earlier (whole Drive); the sync just prepared the folder
                    Text("Lunta now asks Google only for access to the files it makes, not your whole Drive. The Drive folder has been prepared for it; sign in again to switch.")
                        .font(.callout).fixedSize(horizontal: false, vertical: true)
                    Button("Sign In Again…") { Task { await drive.signIn() } }.disabled(drive.syncing || drive.signingIn)
                }
                HStack {
                    Button("Sign Out of Google", role: .destructive) { drive.disconnect() }
                    Spacer()
                    // Google's picker grants only drive.file: offered only while Lunta asks for that
                    // scope, or it would replace a full-scope token
                    if narrowDriveAccess() {
                        Button("Choose Your Lunta Folder…") { Task { await drive.chooseFolder() } }.disabled(drive.syncing || drive.signingIn)
                    }
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
                if let note = GoogleDriveClient.signInNote {
                    Text(note).font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                }
            }
            if let e = drive.error { Text(e).foregroundStyle(.red).font(.callout).fixedSize(horizontal: false, vertical: true) }
            if drive.needsSignIn && drive.connected {
                Button("Sign In Again…") { Task { await drive.signIn() } }.disabled(drive.syncing || drive.signingIn)
            }
            if drive.folderNotVisible {
                if narrowDriveAccess() {
                    Button("Choose Your Lunta Folder…") { Task { await drive.chooseFolder() } }.disabled(drive.syncing || drive.signingIn)
                } else {
                    // a drive.file token from an earlier sign-in: the full scope reaches the folder
                    Button("Sign In Again…") { Task { await drive.signIn() } }.disabled(drive.syncing || drive.signingIn)
                }
            }
        }
    }
}

/// A Google Drive library: "Download papers when you open them", how much is on this device,
/// Download All, and removing downloads not opened for a while.
struct DriveDownloadsView: View {
    @ObservedObject private var drive = DriveLibrarySync.shared
    @AppStorage(DriveLibrarySync.onDemandKey) private var onDemand = DriveLibrarySync.onDemandDefault
    @State private var unused: (ids: [String], bytes: UInt64)?
    @State private var removing = false
    @State private var removed: String?

    static var explanation: String {
        #if os(macOS)
        return "Only the papers you open are kept on this Mac; the rest stay in Google Drive. Good for a work computer or a small disk. Full texts are always kept on the Mac, so search and the AI assistant still see every paper."
        #else
        return "Only the papers you open are kept on \(StorageText.thisDevice); the rest stay in Google Drive. Full texts are always kept here, so search still finds every paper."
        #endif
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Toggle("Download papers when you open them", isOn: $onDemand)
                .onChange(of: onDemand) { _, _ in removed = nil; drive.onDemandChanged() }
            Text(onDemand ? Self.explanation : Self.explanation + " Off: every paper is downloaded.")
                .font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            if let d = drive.downloads, d.papers > 0 {
                Text(d.text).font(.callout).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            }
            if let line = drive.downloadingAll {
                HStack(spacing: 8) {
                    ProgressView().controlSize(.small)
                    Text(line).foregroundStyle(.secondary)
                    Spacer()
                    Button("Stop") { drive.cancelDownloadAll() }
                }
            } else if onDemand {
                HStack {
                    if let d = drive.downloads, d.bytesInDrive > 0 {
                        Button("Download All for Offline Reading") { drive.downloadAll() }.disabled(!drive.connected)
                    }
                    Spacer()
                    Button("Remove Downloads Not Opened in 30 Days…") {
                        Task { unused = await drive.unusedDownloads(days: 30) }
                    }
                    .disabled(removing || drive.downloads?.bytesHere ?? 0 == 0)
                }
                #if os(macOS)
                .buttonStyle(.link)
                #else
                .buttonStyle(.borderless)
                #endif
                if removing { ProgressView().controlSize(.small) }
                if let removed { Text(removed).font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true) }
                Text("To free space paper by paper, choose Remove Download in its menu: it stays in Google Drive and downloads again when you open it.")
                    .font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            }
        }
        .alert(unusedTitle, isPresented: Binding(get: { unused != nil }, set: { if !$0 { unused = nil } }), presenting: unused) { u in
            if !u.ids.isEmpty {
                Button("Remove Downloads", role: .destructive) {
                    removing = true
                    Task {
                        removed = await drive.removeDownloads(u.ids) ?? "Removed. Those papers stay in Google Drive and download again when you open them."
                        removing = false
                    }
                }
            }
            Button(u.ids.isEmpty ? "OK" : "Cancel", role: .cancel) {}
        } message: { u in
            Text(u.ids.isEmpty ? "Every paper downloaded to \(StorageText.thisDevice) was opened in the last 30 days."
                 : "\(u.ids.count.formatted()) paper\(u.ids.count == 1 ? "" : "s") not opened here in 30 days take \(ByteCountFormatter.string(fromByteCount: Int64(u.bytes), countStyle: .file)). Their PDFs and reading views are removed from \(StorageText.thisDevice) and stay in Google Drive; notes and highlights stay too.")
        }
    }

    private var unusedTitle: String { unused?.ids.isEmpty == false ? "Remove Downloads Not Opened in 30 Days?" : "Nothing to Remove" }
}

/// Papers' Download / Remove Download, for their context menu (a Google Drive library; nothing
/// otherwise). Remove Download only when papers are downloaded as they're opened.
struct DownloadMenuItems: View {
    @EnvironmentObject var model: LibraryModel
    let ids: [String]

    var body: some View {
        if model.onDemand != nil {
            let remote = ids.filter { model.hasRemoteOnly($0) }
            if !remote.isEmpty {
                Button(remote.count == 1 ? "Download" : "Download \(remote.count) Papers", systemImage: "icloud.and.arrow.down") {
                    Task { for id in remote { await model.download(id) } }
                }
            }
            if DriveLibrarySync.onDemand {
                let here = ids.filter { model.hasDownload($0) && !model.reading.contains($0) }
                if !here.isEmpty {
                    Button("Remove Download", systemImage: "xmark.icloud") { Task { for id in here { await model.removeDownload(id) } } }
                }
            }
        }
    }
}

/// A paper's info: in Google Drive (Download) or downloaded (Remove Download), for a Google Drive
/// library synced on demand; nothing otherwise.
struct PaperDownloadLine: View {
    @EnvironmentObject var model: LibraryModel
    let paperID: String
    @State private var busy = false

    var body: some View {
        if model.onDemand != nil {
            if !model.inDrive(paperID).isEmpty {
                line("In Google Drive; downloaded when you open it", icon: "icloud.and.arrow.down", button: "Download") { await model.download(paperID) }
            } else if DriveLibrarySync.onDemand, model.hasDownload(paperID), !model.reading.contains(paperID) {
                line("Downloaded to \(StorageText.thisDevice)", icon: "checkmark.icloud", button: "Remove Download") { await model.removeDownload(paperID) }
            }
        }
    }

    private func line(_ text: String, icon: String, button: String, action: @escaping () async -> Void) -> some View {
        HStack(spacing: 8) {
            Label(text, systemImage: icon).font(.footnote).foregroundStyle(.secondary)
            Spacer(minLength: 4)
            if busy { ProgressView().controlSize(.small) }
            Button(button) { busy = true; Task { await action(); busy = false } }
                .font(.footnote).disabled(busy)
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
        } else if drive.needsSignIn {
            Text("Google Drive: sign in again").foregroundStyle(.orange)
        } else if drive.error != nil {
            Text("Google Drive: sync problem").foregroundStyle(.red)
        } else if let last = drive.lastSync {
            Text("Google Drive · synced \(last.formatted(.relative(presentation: .named)))")
        } else {
            Text("Google Drive")
        }
    }
}
