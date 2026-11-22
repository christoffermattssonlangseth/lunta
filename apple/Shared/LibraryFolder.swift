import Foundation
#if os(macOS)
import AppKit
import SwiftUI
#endif

/// The library in a folder the user chose (LibraryStorage.folder): inside OneDrive, SharePoint,
/// Dropbox, Box or Google Drive for desktop, or on a network drive. That service syncs the folder,
/// not Lunta; the library's layout is safe for it (every device writes only its own change log).
///
/// Mac only. On iPhone and iPad a folder from another app's Files provider would have to be read
/// and written through file coordination (the core writes plain files), and providers such as
/// OneDrive keep files online until they're opened, so changes could stay unsent and other
/// devices' changes unseen (docs/development.md, "A folder you choose").
///
/// Remembered as its path and a bookmark (follows the folder when it's renamed or moved; the Mac
/// app isn't sandboxed, so a plain bookmark; a sandboxed build would use a security-scoped one).
enum ChosenFolder {
    static let pathKey = "libraryFolderPath"
    static let bookmarkKey = "libraryFolderBookmark"
    /// The folder had a library when Lunta last opened it: an empty folder there later means it's not really there.
    static let hadLibraryKey = "libraryFolderHadLibrary"

    private static var sandboxed: Bool { ProcessInfo.processInfo.environment["APP_SANDBOX_CONTAINER_ID"] != nil }

    static var storedPath: String? { UserDefaults.standard.string(forKey: pathKey) }

    /// Remembers `url` as the library's folder.
    static func save(_ url: URL) {
        let d = UserDefaults.standard
        d.set(url.path, forKey: pathKey)
        #if os(macOS)
        let options: URL.BookmarkCreationOptions = sandboxed ? [.withSecurityScope] : []
        d.set(try? url.bookmarkData(options: options, includingResourceValuesForKeys: nil, relativeTo: nil), forKey: bookmarkKey)
        #endif
        d.set(CloudLibrary.hasLogs(url), forKey: hadLibraryKey)
    }

    /// The library opened from the folder: remember that it has a library now.
    static func noteOpened(_ url: URL) {
        if CloudLibrary.hasLogs(url) { UserDefaults.standard.set(true, forKey: hadLibraryKey) }
    }

    /// The chosen folder where it is now, nil when none is remembered. Follows a renamed or moved
    /// folder (never into the Trash). Doesn't mount a network drive or ask anything. Blocks a
    /// moment on a network drive: off the main thread.
    static func url() -> URL? {
        let d = UserDefaults.standard
        guard let path = d.string(forKey: pathKey), !path.isEmpty else { return nil }
        #if os(macOS)
        if let data = d.data(forKey: bookmarkKey) {
            var stale = false
            var options: URL.BookmarkResolutionOptions = [.withoutUI, .withoutMounting]
            if sandboxed { options.insert(.withSecurityScope) }
            if let u = try? URL(resolvingBookmarkData: data, options: options, relativeTo: nil, bookmarkDataIsStale: &stale),
               !u.path.contains("/.Trash/"), !u.path.hasSuffix("/.Trash") {
                _ = u.startAccessingSecurityScopedResource()   // only means something in a sandbox
                if stale || u.path != path {
                    d.set(u.path, forKey: pathKey)
                    let o: URL.BookmarkCreationOptions = sandboxed ? [.withSecurityScope] : []
                    if let b = try? u.bookmarkData(options: o, includingResourceValuesForKeys: nil, relativeTo: nil) { d.set(b, forKey: bookmarkKey) }
                }
                return u
            }
        }
        #endif
        return URL(fileURLWithPath: path, isDirectory: true)
    }

    /// The chosen folder as start-up finds it. Blocks: off the main thread.
    static func state() -> ChosenFolderState {
        guard let u = url() else { return .notChosen }
        var dir: ObjCBool = false
        let exists = FileManager.default.fileExists(atPath: u.path, isDirectory: &dir) && dir.boolValue
        return ChosenFolderState.of(chosen: true, exists: exists, hasLibrary: exists && CloudLibrary.hasLogs(u),
                                    hadLibrary: UserDefaults.standard.bool(forKey: hadLibraryKey))
    }

    /// "in OneDrive", "on a network or external drive", "" for a plain folder.
    static func serviceWords(_ path: String) -> String {
        guard let s = FolderPick.service(forPath: path) else { return "" }
        return s.hasPrefix("a ") ? "on \(s)" : "in \(s)"
    }

    /// The copy of the library in this Mac's own folder (Only on This Mac), if it has one: what
    /// "Use the Copy on This Mac" opens when the chosen folder isn't there.
    static func localCopy() -> (url: URL, changed: Date?)? {
        let local = CloudLibrary.localURL
        guard CloudLibrary.hasLogs(local) else { return nil }
        let ops = local.appendingPathComponent("ops")
        let dates = ((try? FileManager.default.contentsOfDirectory(at: ops, includingPropertiesForKeys: [.contentModificationDateKey])) ?? [])
            .compactMap { try? $0.resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate }
        return (local, dates.max())
    }

    /// Where in the picked folder the library goes, or why it can't. Blocks: off the main thread.
    enum Pick: Equatable {
        /// The library's folder (the picked one or a "Lunta" folder in it); `existing`: it has a library.
        case use(URL, existing: Bool)
        case refused(String)
    }

    static func place(picked: URL, currentLibrary: URL?) -> Pick {
        let fm = FileManager.default
        let p = picked.resolvingSymlinksInPath().standardizedFileURL
        let entries = (try? fm.contentsOfDirectory(atPath: p.path)) ?? []
        let cur = currentLibrary?.resolvingSymlinksInPath().standardizedFileURL.path
        switch FolderPick.decide(picked: p.path, currentLibrary: cur, pickedHasLibrary: CloudLibrary.hasLogs(p), entries: entries) {
        case .insideLibrary:
            return .refused("That folder is your library’s own folder, or inside it. Choose a folder outside it.")
        case .useIt:
            return .use(p, existing: CloudLibrary.hasLogs(p))
        case .useSubfolder:
            let sub = p.appendingPathComponent(FolderPick.subfolder, isDirectory: true)
            if let cur, FolderPick.isInside(sub.path, cur) {
                return .refused("That folder is your library’s own folder, or inside it. Choose a folder outside it.")
            }
            var dir: ObjCBool = false
            if fm.fileExists(atPath: sub.path, isDirectory: &dir), !dir.boolValue {
                return .refused("That folder has a file called “\(FolderPick.subfolder)” where Lunta would put its folder. Choose an empty folder, or make one with New Folder.")
            }
            return .use(sub, existing: CloudLibrary.hasLogs(sub))
        }
    }

    /// The words about the library's folder in the dialogs: "“Lunta” in OneDrive (~/Library/CloudStorage/…)".
    static func describe(_ url: URL) -> String {
        #if os(macOS)
        let path = (url.path as NSString).abbreviatingWithTildeInPath
        #else
        let path = url.path
        #endif
        let service = serviceWords(url.path)
        return "“\(url.lastPathComponent)”\(service.isEmpty ? "" : " \(service)") (\(path))"
    }

    static let syncNote = "OneDrive, SharePoint, Dropbox, Box and Google Drive for desktop sync it like any other folder, and a network drive works as it is: each device writes only its own change log, so devices never overwrite each other’s changes."
}

#if os(macOS)
// MARK: - Choosing the folder (Mac)

@MainActor
enum FolderChooser {
    /// Asks for a folder (New Folder works there). nil: cancelled.
    static func ask(message: String, prompt: String = "Use This Folder") -> URL? {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.canCreateDirectories = true
        panel.allowsMultipleSelection = false
        panel.treatsFilePackagesAsDirectories = false
        panel.prompt = prompt
        panel.message = message
        let cloud = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Library/CloudStorage")
        if FileManager.default.fileExists(atPath: cloud.path) { panel.directoryURL = cloud }
        return panel.runModal() == .OK ? panel.url : nil
    }

    static let message = "Choose a folder for your Lunta library: in OneDrive, SharePoint, Dropbox, Box, Google Drive for desktop or on a network drive. A folder that already has a Lunta library (from another computer) is opened as it is."

    /// Asks, then works out where in the folder the library goes; says why when it can't.
    static func choose(currentLibrary: URL?) async -> ChosenFolder.Pick? {
        guard let picked = ask(message: message) else { return nil }
        return await Task.detached { ChosenFolder.place(picked: picked, currentLibrary: currentLibrary) }.value
    }

    /// A new library in an empty folder: asked first, so a folder chosen by mistake (an unsynced
    /// cloud folder, say) doesn't quietly become an empty library.
    static func confirmNew(_ url: URL) -> Bool {
        let a = NSAlert()
        a.messageText = "Start a new library here?"
        a.informativeText = "\(ChosenFolder.describe(url)) has no Lunta library yet. Lunta starts an empty one there.\n\nIf your library should already be in this folder (synced from another computer), it may not have arrived yet: cancel, let the folder finish syncing, then choose it again."
        a.addButton(withTitle: "Start New Library")
        a.addButton(withTitle: "Cancel")
        return a.runModal() == .alertFirstButtonReturn
    }

    static func refuse(_ text: String) {
        let a = NSAlert()
        a.messageText = "Lunta can’t use that folder"
        a.informativeText = text
        a.runModal()
    }
}

// MARK: - The folder isn't there (Mac)

/// Start-up found the chosen folder missing or emptied: never an empty library in its place.
struct FolderUnavailableView: View {
    let state: ChosenFolderState
    /// Try Again: look for the folder again.
    let tryAgain: () -> Void
    /// Choose Again…: a folder was chosen (and, if it's new, confirmed) and saved.
    let chosen: () -> Void
    /// Use the Copy on This Mac: switches to Only on This Mac.
    let useLocal: () -> Void
    @State private var local: (url: URL, changed: Date?)?
    @State private var checking = false

    private var path: String { (ChosenFolder.storedPath.map { ($0 as NSString).abbreviatingWithTildeInPath }) ?? "" }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Label(title, systemImage: "externaldrive.badge.exclamationmark").font(.title.weight(.bold))
            Text(message).fixedSize(horizontal: false, vertical: true)
            if let local {
                Text("Use the Copy on This Mac opens the library in this Mac’s own folder (\((local.url.path as NSString).abbreviatingWithTildeInPath))\(local.changed.map { ", last changed \($0.formatted(date: .abbreviated, time: .shortened))" } ?? ""), and keeps it there from now on. It doesn’t have the changes made in the folder since. When the folder is back, Library Storage › Move to Another Folder… combines the two.")
                    .font(.callout).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            }
            HStack {
                Button("Quit Lunta") { NSApp.terminate(nil) }
                Spacer()
                if checking { ProgressView().controlSize(.small) }
                if local != nil { Button("Use the Copy on This Mac") { useLocal() } }
                Button("Choose Again…") { Task { await chooseAgain() } }
                Button("Try Again") { tryAgain() }.buttonStyle(.borderedProminent).keyboardShortcut(.defaultAction)
            }
        }
        .padding(28)
        .frame(maxWidth: 620)
        .tint(.luntaTeal)
        .task { local = await Task.detached { ChosenFolder.localCopy() }.value }
    }

    private var title: String {
        state == .emptied ? "Your library folder is empty" : "Lunta can’t find your library folder"
    }

    private var message: String {
        switch state {
        case .missing:
            return "Your library is in \(path), and that folder isn’t there right now. If it’s on a network drive or an external disk, connect it (Finder › Go › Connect to Server…); if it’s in OneDrive, Dropbox or another cloud folder, make sure its app is running and signed in. Then choose Try Again.\n\nIf the folder was moved or renamed, choose it again with Choose Again…. Lunta doesn’t start an empty library in its place."
        case .emptied:
            return "Your library is in \(path), but that folder has no library in it now. A network drive that isn’t connected can leave an empty folder behind, and a cloud folder may still be syncing. Connect the drive or wait for the sync, then choose Try Again.\n\nIf the library was moved, choose its new folder with Choose Again…. Lunta doesn’t start an empty library here."
        case .notChosen, .ready:
            return "Lunta keeps your library in a folder you choose, but it doesn’t know which folder. Choose it with Choose Again…."
        }
    }

    private func chooseAgain() async {
        checking = true
        defer { checking = false }
        guard let pick = await FolderChooser.choose(currentLibrary: nil) else { return }
        switch pick {
        case .refused(let t): FolderChooser.refuse(t)
        case .use(let url, let existing):
            if !existing, !FolderChooser.confirmNew(url) { return }
            ChosenFolder.save(url)
            chosen()
        }
    }
}

/// While the library is open: if its folder goes away (a network drive disconnected, a disk
/// unplugged), say so at once, since nothing can be saved until it's back.
@MainActor
final class FolderWatch {
    static let shared = FolderWatch()
    private var timer: Timer?
    private var showing = false
    private var warned = false

    func start(_ root: URL) {
        timer?.invalidate()
        timer = Timer.scheduledTimer(withTimeInterval: 15, repeats: true) { [weak self] _ in
            Task { @MainActor in await self?.check(root) }
        }
    }

    private func check(_ root: URL) async {
        guard !showing else { return }
        let ops = root.appendingPathComponent("ops").path
        let here = await Task.detached { FileManager.default.fileExists(atPath: ops) }.value
        if here { warned = false; return }
        guard !warned else { return }
        showing = true
        defer { showing = false }
        while true {
            let a = NSAlert()
            a.alertStyle = .critical
            a.messageText = "Lunta can’t reach your library folder"
            a.informativeText = "\((root.path as NSString).abbreviatingWithTildeInPath) isn’t available any more: a network drive that was disconnected, a disk that was unplugged, or a cloud folder that went offline. Changes you make now can’t be saved.\n\nReconnect it and choose Try Again, or quit Lunta."
            a.addButton(withTitle: "Try Again")
            a.addButton(withTitle: "Quit Lunta")
            a.addButton(withTitle: "Continue Anyway")
            switch a.runModal() {
            case .alertFirstButtonReturn:
                if FileManager.default.fileExists(atPath: ops) { warned = false; return }
            case .alertSecondButtonReturn:
                NSApp.terminate(nil)
                return
            default:
                warned = true   // not again until it has been back
                return
            }
        }
    }
}
#endif
