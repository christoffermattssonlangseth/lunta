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
        }
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

    static func decide(_ f: StorageFacts) -> StartupDecision {
        if let s = f.stored { return .open(s) }
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
/// with each file (paths relative to the library folder), given whether the destination has it.
enum LibraryCopyRule: Equatable, Sendable {
    case copy
    /// This device's own change log, present in both places: add the lines the destination lacks
    /// (logs are append-only, so nothing is lost either way).
    case mergeLines
    case skip

    static func rule(for path: String, destinationHasIt: Bool, ownLogs: Set<String>) -> LibraryCopyRule {
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
        if parts.count == 2, parts[0] == "ops", ownLogs.contains(String(parts[1].dropLast(".jsonl".count))) { return .mergeLines }
        // never overwrite: the destination's copy is another device's newer one, or the same file
        return .skip
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
}
