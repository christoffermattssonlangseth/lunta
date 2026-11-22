// Test driver for apple/Shared/StorageDecision.swift (run by storage-decision.test.js).
import Foundation

var failures = 0
func check<T: Equatable>(_ got: T, _ want: T, _ what: String) {
    if got == want { print("ok   \(what)") } else { print("FAIL \(what): got \(got), want \(want)"); failures += 1 }
}

func facts(stored: LibraryStorage? = nil, ran: Bool = false, local: Bool = false, iCloud: Bool = true, inICloud: Bool? = nil) -> StorageFacts {
    StorageFacts(stored: stored, hasRunBefore: ran, localLibraryExists: local, iCloudAvailable: iCloud, iCloudLibraryExists: inICloud)
}

@main
struct Main {
    static func main() {
        // a fresh device: look in iCloud first, then ask (iCloud recommended); without iCloud, Google Drive
        check(StartupDecision.decide(facts()), .lookInICloud, "fresh device with iCloud: looks in iCloud first")
        check(StartupDecision.decide(facts(inICloud: false)), .ask(recommended: .iCloud), "fresh device, nothing in iCloud: asks, iCloud recommended")
        check(StartupDecision.decide(facts(iCloud: false)), .ask(recommended: .googleDrive), "fresh device without iCloud: asks, Google Drive recommended")
        // a new device whose iCloud already has the library: opens it, never hidden behind the question
        check(StartupDecision.decide(facts(inICloud: true)), .openFoundInICloud, "new device, library in iCloud: opens it and says so")
        // people who used Lunta before the choice: as before, no question
        check(StartupDecision.decide(facts(ran: true)), .open(nil), "existing iCloud user: automatic (iCloud)")
        check(StartupDecision.decide(facts(ran: true, iCloud: false)), .open(nil), "existing user without iCloud: automatic (local)")
        check(StartupDecision.decide(facts(local: true, iCloud: false)), .open(nil), "existing local library: kept")
        check(StartupDecision.decide(facts(local: true)), .open(nil), "existing local library with iCloud now on: automatic, as before")
        // a choice made once is kept
        for s in LibraryStorage.allCases {
            check(StartupDecision.decide(facts(stored: s)), .open(s), "stored \(s.rawValue)")
            check(StartupDecision.decide(facts(stored: s, ran: true, local: true, iCloud: false, inICloud: true)), .open(s), "stored \(s.rawValue) wins over everything")
        }

        // copying a library to its new place (the destination may hold an older copy of it)
        func rule(_ p: String, there: Bool, newer: Bool = false) -> LibraryCopyRule { LibraryCopyRule.rule(for: p, destinationHasIt: there, sourceIsNewer: newer) }
        check(rule("ops/mac-1.jsonl", there: false), .copy, "own log, not there: copy")
        check(rule("ops/mac-1.jsonl", there: true), .mergeLines, "own log in both: merge lines")
        check(rule("ops/ios-2.jsonl", there: true), .mergeLines, "another device's log in both: merged too (an old prefix at the destination)")
        check(rule("ops/ios-2.jsonl", there: true, newer: true), .mergeLines, "a log is merged whatever its date")
        check(rule("ops/ios-2.jsonl", there: false), .copy, "another device's log: copied if missing")
        check(rule("papers/abc/paper.pdf", there: false), .copy, "paper file: copied")
        check(rule("papers/abc/paper.pdf", there: true), .skip, "paper file there and as new: kept")
        check(rule("papers/abc/paper.pdf", there: true, newer: true), .replace, "paper file replaced since: overwritten")
        check(rule("positions/mac-1.json", there: false), .copy, "positions: copied")
        check(rule("positions/mac-1.json", there: true), .mergePositions, "positions in both: merged per paper")
        check(rule("positions/mac-1 2.json", there: false), .skip, "iCloud conflict copy: skipped")
        check(rule("smart-folders.json", there: false), .copy, "root json: copied")
        check(rule("smart-folders.json", there: true), .skip, "root json there and as new: kept")
        check(rule("smart-folders.json", there: true, newer: true), .replace, "root json newer at the source: overwritten")
        check(rule("papers/abc/.paper.pdf.icloud", there: false), .skip, "iCloud placeholder: skipped")
        check(rule(".lunta-sync/state.json", there: false), .skip, "sync bookkeeping: skipped")
        check(rule(".lunta-sync/state.json", there: true, newer: true), .skip, "sync bookkeeping: never overwritten")
        check(rule("Cache/index.sqlite", there: false), .skip, "unknown folder: skipped")
        check(LibraryCopyRule.mergedLog(destination: "a\nb\n", source: "a\nb\nc\n"), "a\nb\nc\n", "merge adds the missing lines")
        check(LibraryCopyRule.mergedLog(destination: "a\nb", source: "c\n"), "a\nb\nc\n", "merge after a line without newline")
        check(LibraryCopyRule.mergedLog(destination: "a\nb\nc\n", source: "a\nb\n"), nil, "nothing missing: unchanged")

        // reading positions: per paper, the later one wins; papers from both are kept
        func pos(_ d: [String: (Int, Double)]) -> Data {
            try! JSONSerialization.data(withJSONObject: d.mapValues { ["offset": $0.0, "at": $0.1, "device": "Mac"] as [String: Any] }, options: [.sortedKeys])
        }
        func offsets(_ d: Data?) -> [String: Int]? {
            guard let d, let o = try? JSONSerialization.jsonObject(with: d) as? [String: [String: Any]] else { return nil }
            return o.mapValues { ($0["offset"] as? NSNumber)?.intValue ?? -1 }
        }
        let staleDst = pos(["a": (10, 100), "b": (50, 500), "old": (7, 50)])
        let src = pos(["a": (90, 900), "b": (20, 200), "new": (5, 300)])
        check(offsets(LibraryCopyRule.mergedPositions(destination: staleDst, source: src)), ["a": 90, "b": 50, "old": 7, "new": 5],
              "positions merged: newer per paper, both sides' papers kept")
        check(offsets(LibraryCopyRule.mergedPositions(destination: src, source: pos(["a": (1, 10)]))), nil, "positions: nothing newer, unchanged")
        check(offsets(LibraryCopyRule.mergedPositions(destination: Data("junk".utf8), source: src)), ["a": 90, "b": 20, "new": 5],
              "unreadable destination positions: the source's")

        // a folder the user chose (OneDrive, a network drive): opened when it's there, never an empty library in its place
        var f = facts(stored: .folder, ran: true, local: true)
        f.chosenFolder = .ready
        check(StartupDecision.decide(f), .open(.folder), "chosen folder there: opened")
        f.chosenFolder = .missing
        check(StartupDecision.decide(f), .folderUnavailable(.missing), "chosen folder gone (share not connected): said, not replaced")
        f.chosenFolder = .emptied
        check(StartupDecision.decide(f), .folderUnavailable(.emptied), "chosen folder emptied (empty mount point): said")
        f.chosenFolder = .notChosen
        check(StartupDecision.decide(f), .folderUnavailable(.notChosen), "folder setting without a folder: asks for it")
        var g = facts(stored: .device)
        g.chosenFolder = .missing
        check(StartupDecision.decide(g), .open(.device), "a folder's state matters only when the folder is the choice")
        check(ChosenFolderState.of(chosen: true, exists: true, hasLibrary: true, hadLibrary: true), .ready, "folder with its library: ready")
        check(ChosenFolderState.of(chosen: true, exists: true, hasLibrary: false, hadLibrary: false), .ready, "new empty folder, never had a library: ready (a new library)")
        check(ChosenFolderState.of(chosen: true, exists: true, hasLibrary: false, hadLibrary: true), .emptied, "folder lost its library: emptied")
        check(ChosenFolderState.of(chosen: true, exists: false, hasLibrary: false, hadLibrary: true), .missing, "folder not there: missing")
        check(ChosenFolderState.of(chosen: false, exists: true, hasLibrary: true, hadLibrary: true), .notChosen, "nothing remembered: not chosen")

        // where in the picked folder the library goes
        func pick(_ p: String, cur: String? = "/Users/a/Library/Application Support/Margins/Library", has: Bool = false, _ e: [String] = []) -> FolderPick {
            FolderPick.decide(picked: p, currentLibrary: cur, pickedHasLibrary: has, entries: e)
        }
        check(pick("/Users/a/Library/CloudStorage/OneDrive-KI/Lunta", has: true, ["ops", "papers"]), .useIt, "folder with a library: used as it is")
        check(pick("/Users/a/Library/CloudStorage/OneDrive-KI/New", [".DS_Store", "Icon\r"]), .useIt, "empty folder (hidden files only): used")
        check(pick("/Users/a/Library/CloudStorage/OneDrive-KI", ["Documents", "Desktop"]), .useSubfolder, "folder with other things: a Lunta folder inside")
        check(pick("/Users/a/Library/Application Support/Margins/Library"), .insideLibrary, "the library's own folder: refused")
        check(pick("/Users/a/Library/Application Support/Margins/Library/papers"), .insideLibrary, "inside the library: refused")
        check(pick("/Users/a/Library/Application Support/Margins/Library 2", cur: "/Users/a/Library/Application Support/Margins/Library"), .useIt, "a sibling with a similar name: fine")
        check(pick("/Volumes/share/lab", cur: nil, ["x.docx"]), .useSubfolder, "first launch (no library yet): other things, a Lunta folder")
        check(FolderPick.isInside("/a/b/", "/a/b"), true, "trailing slash ignored")
        check(FolderPick.isInside("/a/bc", "/a/b"), false, "prefix of a name isn't inside")

        // the service that syncs a folder, for the words in the dialogs
        check(FolderPick.service(forPath: "/Users/a/Library/CloudStorage/OneDrive-KarolinskaInstitutet/Lunta"), "OneDrive", "OneDrive (File Provider)")
        check(FolderPick.service(forPath: "/Users/a/Library/CloudStorage/OneDrive-SharedLibraries-KarolinskaInstitutet/Lab/Lunta"), "SharePoint", "SharePoint library through OneDrive")
        check(FolderPick.service(forPath: "/Users/a/Library/CloudStorage/Dropbox/Lunta"), "Dropbox", "Dropbox (File Provider)")
        check(FolderPick.service(forPath: "/Users/a/Dropbox (Personal)/Lunta"), "Dropbox", "Dropbox (older folder)")
        check(FolderPick.service(forPath: "/Users/a/Library/CloudStorage/Box-Box/Lunta"), "Box", "Box")
        check(FolderPick.service(forPath: "/Users/a/Library/CloudStorage/GoogleDrive-a@b.se/My Drive/Lunta"), "Google Drive", "Google Drive for desktop")
        check(FolderPick.service(forPath: "/Volumes/labshare/Lunta"), "a network or external drive", "network share")
        check(FolderPick.service(forPath: "/Users/a/Documents/Lunta"), nil, "a plain folder")
        check(LibraryStorage.offered.contains(.folder), true, "the Mac offers a folder")

        if failures > 0 { print("\(failures) failed"); exit(1) }
        print("all passed")
    }
}
