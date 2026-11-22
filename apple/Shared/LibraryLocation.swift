import Foundation

/// Start-up's question (where should the library live?) and moving the library when the user
/// changes their answer in Settings. The decisions themselves are in StorageDecision.swift.
enum LibraryLocation {
    /// What start-up does on this device. Looks in iCloud for an existing library when it matters
    /// (a new device), reporting "Looking for your library in iCloud…" meanwhile. Off the main thread.
    static func startupDecision(report: @escaping @Sendable (String) -> Void) async -> (decision: StartupDecision, iCloudAvailable: Bool) {
        // the device id is made when a library is first opened (LibraryModel.deviceID): read before anything opens one
        let hasRunBefore = UserDefaults.standard.string(forKey: "deviceID") != nil
        let cloud = await Task.detached { CloudLibrary.iCloudURL() }.value
        var facts = StorageFacts(stored: LibraryStorage.stored,
                                 hasRunBefore: hasRunBefore,
                                 localLibraryExists: CloudLibrary.hasLogs(CloudLibrary.localURL),
                                 iCloudAvailable: cloud != nil,
                                 iCloudLibraryExists: nil)
        if facts.stored == .folder { facts.chosenFolder = ChosenFolder.state() }
        var decision = StartupDecision.decide(facts)
        if decision == .lookInICloud, let cloud {
            report("Looking for your library in iCloud…")
            facts.iCloudLibraryExists = await lookForLibrary(in: cloud, timeout: 12)
            decision = StartupDecision.decide(facts)
        }
        return (decision, cloud != nil)
    }

    /// True once the iCloud folder lists a library (change logs, or iCloud's placeholders for them).
    /// A brand-new device may take a few seconds to get the listing from iCloud.
    static func lookForLibrary(in lib: URL, timeout: TimeInterval) async -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        let fm = FileManager.default
        while true {
            if CloudLibrary.hasLogs(lib) { return true }
            // ask iCloud for the folders (a no-op when they are here or don't exist)
            for sub in ["", "ops"] {
                let u = sub.isEmpty ? lib : lib.appendingPathComponent(sub)
                if fm.fileExists(atPath: u.path) { try? fm.startDownloadingUbiquitousItem(at: u) }
            }
            if Date() > deadline { return false }
            try? await Task.sleep(nanoseconds: 500_000_000)
        }
    }

    /// Where the library is now, as the user sees it (their choice, or what the automatic way gave).
    static func current(inCloud: Bool) -> LibraryStorage {
        LibraryStorage.stored ?? (inCloud ? .iCloud : .device)
    }

    /// The folder a storage uses on this device (iCloud: nil when iCloud isn't available). Blocks.
    static func root(for s: LibraryStorage) -> URL? {
        switch s {
        case .iCloud: return CloudLibrary.iCloudURL()
        case .googleDrive: return CloudLibrary.driveURL
        case .device: return CloudLibrary.localURL
        case .folder: return ChosenFolder.url()
        }
    }

    // MARK: - Moving the library

    struct CopyResult: Sendable {
        var copied = 0
        var merged = 0
        /// Files that could not be copied (e.g. not downloaded from iCloud in time).
        var failed: [String] = []
    }

    /// Copies the library at `src` into `dst` (papers, change logs, reading positions, smart
    /// folders, New for You): what `dst` lacks is copied, change logs are merged line by line,
    /// reading positions per paper, other files `dst` already has are replaced when `src`'s copy is
    /// newer (`dst` may hold an older copy of this library), and nothing at `src` is changed or
    /// deleted. Files still in iCloud are downloaded first. Slow for a big library: reports progress.
    static func copyLibrary(from src: URL, to dst: URL,
                            report: @escaping @Sendable (String) -> Void) async -> CopyResult {
        let fm = FileManager.default
        try? fm.createDirectory(at: dst, withIntermediateDirectories: true)
        // every file, by its real name (an iCloud placeholder ".x.icloud" stands for "x")
        var files: [String] = []
        func list(_ rel: String) -> [String] {
            let dir = rel.isEmpty ? src : src.appendingPathComponent(rel)
            return ((try? fm.contentsOfDirectory(atPath: dir.path)) ?? []).map { name in
                if name.hasPrefix("."), name.hasSuffix(".icloud") { return String(name.dropFirst().dropLast(".icloud".count)) }
                return name
            }
        }
        for name in list("") {
            var isDir: ObjCBool = false
            let p = src.appendingPathComponent(name).path
            if fm.fileExists(atPath: p, isDirectory: &isDir), isDir.boolValue {
                if name == "ops" || name == "positions" { files += list(name).map { "\(name)/\($0)" } }
                if name == "papers" {
                    for id in list("papers") { files += list("papers/\(id)").map { "papers/\(id)/\($0)" } }
                }
            } else {
                files.append(name)
            }
        }
        func modified(_ u: URL) -> Date? { try? u.resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate }
        let plan: [(String, LibraryCopyRule)] = files.compactMap { rel in
            let from = src.appendingPathComponent(rel), to = dst.appendingPathComponent(rel)
            let there = fm.fileExists(atPath: to.path)
            // unknown dates (an iCloud placeholder on an older system): not newer, the destination's is kept
            let newer = there && (modified(from).flatMap { a in modified(to).map { a > $0 } } ?? false)
            let rule = LibraryCopyRule.rule(for: rel, destinationHasIt: there, sourceIsNewer: newer)
            return rule == .skip ? nil : (rel, rule)
        }
        // ask iCloud for everything at once; each file is then waited for in turn
        for (rel, _) in plan where !CloudLibrary.isDownloaded(src.appendingPathComponent(rel)) {
            CloudLibrary.startDownload(src.appendingPathComponent(rel))
        }
        var result = CopyResult()
        for (i, (rel, rule)) in plan.enumerated() {
            if i % 10 == 0 || i == plan.count - 1 { report("Copying your library… \(i + 1) of \(plan.count)") }
            let from = src.appendingPathComponent(rel), to = dst.appendingPathComponent(rel)
            if !CloudLibrary.isDownloaded(from) {
                guard await CloudLibrary.download(from, timeout: 300) else { result.failed.append(rel); continue }
            }
            do {
                try fm.createDirectory(at: to.deletingLastPathComponent(), withIntermediateDirectories: true)
                switch rule {
                case .copy:
                    try fm.copyItem(at: from, to: to)
                    result.copied += 1
                case .replace:
                    // copied next to it first, so a failed copy leaves the destination's as it was
                    let tmp = to.deletingLastPathComponent().appendingPathComponent(".lunta-move-\(UUID().uuidString)")
                    try fm.copyItem(at: from, to: tmp)
                    do { _ = try fm.replaceItemAt(to, withItemAt: tmp) } catch { try? fm.removeItem(at: tmp); throw error }
                    result.copied += 1
                case .mergePositions:
                    let have = try Data(contentsOf: to)
                    let add = try Data(contentsOf: from)
                    if let merged = LibraryCopyRule.mergedPositions(destination: have, source: add) {
                        try merged.write(to: to, options: .atomic)
                        result.merged += 1
                    }
                case .mergeLines:
                    let have = try String(contentsOf: to, encoding: .utf8)
                    let add = try String(contentsOf: from, encoding: .utf8)
                    if let merged = LibraryCopyRule.mergedLog(destination: have, source: add) {
                        try merged.write(to: to, atomically: true, encoding: .utf8)
                        result.merged += 1
                    }
                case .skip: break
                }
            } catch {
                result.failed.append(rel)
            }
        }
        return result
    }

    /// The library's size in bytes: every file under `root`, iCloud's not-downloaded files
    /// included (at their real size). nil if nothing could be read. Slow for a big library: off
    /// the main thread.
    static func size(of root: URL) -> UInt64? {
        let keys: [URLResourceKey] = [.isRegularFileKey, .isDirectoryKey, .fileSizeKey, .totalFileSizeKey]
        guard let e = FileManager.default.enumerator(at: root, includingPropertiesForKeys: keys, options: []) else { return nil }
        var total: UInt64 = 0
        for case let u as URL in e {
            let name = u.lastPathComponent
            guard let v = try? u.resourceValues(forKeys: Set(keys)) else { continue }
            if v.isDirectory == true, name.hasPrefix(".") { e.skipDescendants(); continue }   // sync bookkeeping
            guard v.isRegularFile == true else { continue }
            if name.hasPrefix("."), name.hasSuffix(".icloud") {
                // an iCloud placeholder (older systems): the real size is inside it
                if let d = try? Data(contentsOf: u),
                   let plist = try? PropertyListSerialization.propertyList(from: d, format: nil) as? [String: Any],
                   let n = plist["NSURLFileSizeKey"] as? NSNumber { total += n.uint64Value }
                continue
            }
            if name.hasPrefix(".") { continue }   // sync bookkeeping, partial downloads
            total += UInt64(max(0, v.totalFileSize ?? v.fileSize ?? 0))
        }
        return total
    }

    /// Where the copy a move leaves behind is, in words for the user.
    static func oldCopyDescription(_ s: LibraryStorage, root: URL) -> String {
        #if os(macOS)
        let path = (root.path as NSString).abbreviatingWithTildeInPath
        switch s {
        case .iCloud: return "in iCloud Drive (\(path)), where your other Apple devices keep using it"
        case .googleDrive: return "in \(path) and in the “Lunta” folder in your Google Drive, which this Mac no longer syncs"
        case .device: return "on this Mac, in \(path)"
        case .folder: return "in \(path)\(ChosenFolder.serviceWords(root.path).isEmpty ? "" : " (\(ChosenFolder.serviceWords(root.path)))")"
        }
        #else
        switch s {
        case .iCloud: return "in iCloud Drive, where your other Apple devices keep using it"
        case .googleDrive: return "on this device and in the “Lunta” folder in your Google Drive, which this device no longer syncs"
        case .device: return "on this device, inside Lunta"
        case .folder: return "in the folder you chose"
        }
        #endif
    }

    // MARK: - The Mac's AI agent

    #if os(macOS)
    /// Tells lunta-mcp (the AI agent server) where the library is: it reads this file before
    /// guessing iCloud or the local folder (core/src/bin/lunta-mcp.rs).
    static func publishRoot(_ root: URL) {
        let dir = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent("Lunta", isDirectory: true)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        try? root.path.write(to: dir.appendingPathComponent("library-root"), atomically: true, encoding: .utf8)
    }

    /// The device id lunta-mcp writes its change log under (ops/<id>.jsonl), made here if the agent
    /// hasn't run yet (it then uses the same one).
    static func agentDeviceID() -> String? {
        let dir = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Library/Caches/Lunta/agent", isDirectory: true)
        let file = dir.appendingPathComponent("device-id")
        if let s = try? String(contentsOf: file, encoding: .utf8), !s.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            return s.trimmingCharacters(in: .whitespacesAndNewlines)
        }
        let id = "agent-\(UUID().uuidString.replacingOccurrences(of: "-", with: "").prefix(8).lowercased())"
        do {
            try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
            try id.write(to: file, atomically: true, encoding: .utf8)
            return id
        } catch { return nil }
    }
    #endif

    /// The change logs this device writes (its own, and on the Mac its AI agent's).
    static var ownLogs: Set<String> {
        var s: Set<String> = [LibraryModel.deviceID]
        #if os(macOS)
        if let a = agentDeviceID() { s.insert(a) }
        #endif
        return s
    }
}
