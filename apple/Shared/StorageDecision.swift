import Foundation

// Where the library lives on this Mac, iPhone or iPad, and what Lunta does at start-up to find out.
// Plain Foundation only: apple/storage-tests compiles this file on its own and tests the decision.

/// The place the user chose for the library (UserDefaults `libraryStorage`). Not set for people
/// who used Lunta before the choice existed: they keep the automatic behaviour (iCloud when it's
/// available, else the folder on this device) and never see the question.
enum LibraryStorage: String, CaseIterable, Sendable {
    /// The app's iCloud Drive container: syncs the Mac, iPhone and iPad by itself.
    case iCloud = "icloud"
    /// A folder on this device, synced with the "Lunta" folder in the user's Google Drive through
    /// the core's Drive sync (DriveLibrarySync.swift), like the Android app.
    case googleDrive = "googleDrive"
    /// A folder on this device, not synced.
    case device = "device"
    /// A folder the user chose (Mac only): inside OneDrive, Dropbox, Box or Google Drive for
    /// desktop, or on a network drive. That service syncs it, not Lunta (ChosenFolder, LibraryFolder.swift).
    case folder = "folder"

    static let defaultsKey = "libraryStorage"

    static var stored: LibraryStorage? {
        get { UserDefaults.standard.string(forKey: defaultsKey).flatMap(LibraryStorage.init(rawValue:)) }
        set { UserDefaults.standard.set(newValue?.rawValue, forKey: defaultsKey) }
    }

    var name: String {
        switch self {
        case .iCloud: return "iCloud Drive"
        case .googleDrive: return "Google Drive"
        case .device: return "Only on This Device"
        case .folder: return "A Folder You Choose"
        }
    }

    /// The places this platform offers. A chosen folder is the Mac's: on iPhone and iPad a folder
    /// from another app's Files provider (OneDrive, Dropbox) isn't reliable for a library (docs/development.md).
    static var offered: [LibraryStorage] {
        #if os(macOS)
        return allCases
        #else
        return [.iCloud, .googleDrive, .device]
        #endif
    }
}

/// A chosen folder (LibraryStorage.folder) as start-up finds it.
enum ChosenFolderState: Equatable, Sendable {
    case ready
    /// Not there: a network drive that isn't connected, an external disk that isn't plugged in, a
    /// cloud folder whose app is signed out, or the folder was deleted (in the Trash counts as gone).
    case missing
    /// There, but without the library it had: an empty mount point, a cloud folder emptied or not synced back yet.
    case emptied
    /// The setting says "a folder", but no folder is remembered.
    case notChosen

    static func of(chosen: Bool, exists: Bool, hasLibrary: Bool, hadLibrary: Bool) -> ChosenFolderState {
        guard chosen else { return .notChosen }
        guard exists else { return .missing }
        if !hasLibrary && hadLibrary { return .emptied }
        return .ready
    }
}

/// The folder the user picked for the library: used as it is, or a "Lunta" folder made inside it.
enum FolderPick: Equatable, Sendable {
    /// The folder has a library, or is empty: the library goes right there.
    case useIt
    /// The folder has other things in it (OneDrive's top folder, Documents): the library goes into
    /// a "Lunta" folder inside it (which may already hold it).
    case useSubfolder
    /// The folder is the library's own folder or inside it.
    case insideLibrary

    static let subfolder = "Lunta"

    /// `entries`: the picked folder's names. Paths without symlinks.
    static func decide(picked: String, currentLibrary: String?, pickedHasLibrary: Bool, entries: [String]) -> FolderPick {
        if let cur = currentLibrary, isInside(picked, cur) { return .insideLibrary }
        if pickedHasLibrary { return .useIt }
        let ignored: Set<String> = ["Icon\r", "desktop.ini", "Thumbs.db"]
        let visible = entries.filter { !$0.hasPrefix(".") && !ignored.contains($0) }
        return visible.isEmpty ? .useIt : .useSubfolder
    }

    /// `path` is `root` or inside it.
    static func isInside(_ path: String, _ root: String) -> Bool {
        func trim(_ p: String) -> String { p.count > 1 && p.hasSuffix("/") ? String(p.dropLast()) : p }
        let p = trim(path), r = trim(root)
        return p == r || p.hasPrefix(r == "/" ? "/" : r + "/")
    }

    /// The service that syncs a folder, from its path ("OneDrive", "SharePoint", "Dropbox", "Box",
    /// "Google Drive", "iCloud Drive"), "a network or external drive" under /Volumes, nil for a plain folder.
    static func service(forPath path: String) -> String? {
        let parts = path.split(separator: "/").map(String.init)
        if let i = parts.firstIndex(of: "CloudStorage"), i > 0, parts[i - 1] == "Library", i + 1 < parts.count {
            let name = parts[i + 1]
            if name.hasPrefix("OneDrive-SharedLibraries") { return "SharePoint" }
            if name.hasPrefix("OneDrive") { return "OneDrive" }
            if name.hasPrefix("Dropbox") { return "Dropbox" }
            if name.hasPrefix("Box") { return "Box" }
            if name.hasPrefix("GoogleDrive") { return "Google Drive" }
            return name.split(separator: "-").first.map(String.init)
        }
        if parts.count >= 3, parts[0] == "Users", parts[2] == "Dropbox" || parts[2].hasPrefix("Dropbox (") { return "Dropbox" }
        if parts.count >= 3, parts[0] == "Users", parts[2].hasPrefix("OneDrive") { return "OneDrive" }
        if parts.count >= 3, parts[0] == "Users", parts[2] == "Box" { return "Box" }
        if path.contains("/Library/Mobile Documents/com~apple~CloudDocs") { return "iCloud Drive" }
        if parts.first == "Volumes" { return "a network or external drive" }
        return nil
    }
}

/// What start-up knows about this device. `iCloudLibraryExists` is nil until Lunta has looked
/// (looking can take a few seconds on a new device, so it's done only when it matters).
struct StorageFacts: Equatable, Sendable {
    var stored: LibraryStorage?
    /// An earlier version of Lunta ran here (it made this device's id when it opened the library).
    var hasRunBefore: Bool
    /// The folder on this device has a library (change logs).
    var localLibraryExists: Bool
    var iCloudAvailable: Bool
    var iCloudLibraryExists: Bool?
    /// The chosen folder's state, when the stored choice is a folder (nil: not looked at).
    var chosenFolder: ChosenFolderState? = nil
}

enum StartupDecision: Equatable, Sendable {
    /// Open the library: in the stored place, or (nil) the automatic way as before the choice existed.
    case open(LibraryStorage?)
    /// Look in iCloud for a library first (show "Looking for your library in iCloud…"), then decide again.
    case lookInICloud
    /// A new device whose iCloud already has a Lunta library: open it (and remember iCloud), and
    /// say so, with the option to use Google Drive instead.
    case openFoundInICloud
    /// Nothing here yet: ask where the library should live. `recommended` is preselected.
    case ask(recommended: LibraryStorage)
    /// The library is in a folder the user chose, and the folder isn't there (or lost the library):
    /// say so, with Try Again, Choose Again and the copy on this Mac. Never an empty library in its place.
    case folderUnavailable(ChosenFolderState)

    static func decide(_ f: StorageFacts) -> StartupDecision {
        if let s = f.stored {
            if s == .folder, let c = f.chosenFolder, c != .ready { return .folderUnavailable(c) }
            return .open(s)
        }
        // people who used Lunta before the choice existed: as before, never the question
        if f.hasRunBefore || f.localLibraryExists { return .open(nil) }
        if f.iCloudAvailable {
            switch f.iCloudLibraryExists {
            case nil: return .lookInICloud
            case true?: return .openFoundInICloud
            case false?: return .ask(recommended: .iCloud)
            }
        }
        // no iCloud (signed out, turned off for Lunta, or a managed Mac): Google Drive syncs, the device doesn't
        return .ask(recommended: .googleDrive)
    }
}

/// Copying a library from one place to another when the user changes where it lives. What to do
/// with each file (paths relative to the library folder), given whether the destination has it and
/// whether the source's copy is newer. The destination may hold an older copy of the same library
/// (a folder used before iCloud, or left by an earlier move), so nothing the source has is lost:
/// change logs and reading positions are merged, other files are replaced when the source's is newer.
enum LibraryCopyRule: Equatable, Sendable {
    case copy
    /// A change log (ops/<device>.jsonl), present in both places: add the lines the destination
    /// lacks (logs are append-only, so nothing is lost either way).
    case mergeLines
    /// A device's reading positions (positions/<device>.json), present in both places: per paper,
    /// the newer position wins.
    case mergePositions
    /// Present in both places and the source's copy is newer: overwrite the destination's.
    case replace
    case skip

    static func rule(for path: String, destinationHasIt: Bool, sourceIsNewer: Bool) -> LibraryCopyRule {
        let parts = path.split(separator: "/", omittingEmptySubsequences: false).map(String.init)
        if parts.contains(where: { $0.isEmpty || $0.hasPrefix(".") || $0 == ".." }) { return .skip }
        let wanted: Bool
        switch parts.count {
        case 1: wanted = parts[0].hasSuffix(".json")                                  // smart-folders.json, watches.json, foryou.json
        case 2: wanted = (parts[0] == "ops" && parts[1].hasSuffix(".jsonl"))
                      || (parts[0] == "positions" && parts[1].hasSuffix(".json") && !parts[1].contains(" "))
        case 3: wanted = parts[0] == "papers"
        default: wanted = false
        }
        guard wanted else { return .skip }
        if !destinationHasIt { return .copy }
        if parts.count == 2, parts[0] == "ops" { return .mergeLines }
        if parts.count == 2, parts[0] == "positions" { return .mergePositions }
        // the same file, or an older copy at the destination: the newer one wins
        return sourceIsNewer ? .replace : .skip
    }

    /// The destination log with the source's missing lines added at the end, or nil if it has them all.
    static func mergedLog(destination: String, source: String) -> String? {
        let have = Set(destination.split(separator: "\n", omittingEmptySubsequences: true).map(String.init))
        let missing = source.split(separator: "\n", omittingEmptySubsequences: true).map(String.init)
            .filter { !$0.trimmingCharacters(in: .whitespaces).isEmpty && !have.contains($0) }
        guard !missing.isEmpty else { return nil }
        var out = destination
        if !out.isEmpty && !out.hasSuffix("\n") { out += "\n" }
        return out + missing.joined(separator: "\n") + "\n"
    }

    /// Two copies of one device's positions file ({paper: {offset, at, device}}) merged: every
    /// paper from both, the one with the later `at` where both have it. nil if the destination
    /// already has everything (or the source can't be read); a destination that can't be read is
    /// replaced by the source.
    static func mergedPositions(destination: Data, source: Data) -> Data? {
        guard let src = (try? JSONSerialization.jsonObject(with: source)) as? [String: Any] else { return nil }
        guard var out = (try? JSONSerialization.jsonObject(with: destination)) as? [String: Any] else { return source }
        func at(_ v: Any?) -> Double? { ((v as? [String: Any])?["at"] as? NSNumber)?.doubleValue }
        var changed = false
        for (paper, spot) in src {
            if let mine = out[paper], let a = at(mine), let b = at(spot), a >= b { continue }
            if out[paper] != nil, at(spot) == nil { continue }
            out[paper] = spot
            changed = true
        }
        guard changed else { return nil }
        return try? JSONSerialization.data(withJSONObject: out, options: [.sortedKeys])
    }
}
