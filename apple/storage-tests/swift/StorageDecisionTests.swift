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

        if failures > 0 { print("\(failures) failed"); exit(1) }
        print("all passed")
    }
}
