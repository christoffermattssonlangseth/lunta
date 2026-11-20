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

        // copying a library to its new place
        let own: Set<String> = ["mac-1"]
        check(LibraryCopyRule.rule(for: "ops/mac-1.jsonl", destinationHasIt: false, ownLogs: own), .copy, "own log, not there: copy")
        check(LibraryCopyRule.rule(for: "ops/mac-1.jsonl", destinationHasIt: true, ownLogs: own), .mergeLines, "own log in both: merge lines")
        check(LibraryCopyRule.rule(for: "ops/ios-2.jsonl", destinationHasIt: true, ownLogs: own), .skip, "another device's log in both: left alone")
        check(LibraryCopyRule.rule(for: "ops/ios-2.jsonl", destinationHasIt: false, ownLogs: own), .copy, "another device's log: copied if missing")
        check(LibraryCopyRule.rule(for: "papers/abc/paper.pdf", destinationHasIt: false, ownLogs: own), .copy, "paper file: copied")
        check(LibraryCopyRule.rule(for: "papers/abc/paper.pdf", destinationHasIt: true, ownLogs: own), .skip, "paper file there: never overwritten")
        check(LibraryCopyRule.rule(for: "positions/mac-1.json", destinationHasIt: false, ownLogs: own), .copy, "positions: copied")
        check(LibraryCopyRule.rule(for: "positions/mac-1 2.json", destinationHasIt: false, ownLogs: own), .skip, "iCloud conflict copy: skipped")
        check(LibraryCopyRule.rule(for: "smart-folders.json", destinationHasIt: false, ownLogs: own), .copy, "root json: copied")
        check(LibraryCopyRule.rule(for: "papers/abc/.paper.pdf.icloud", destinationHasIt: false, ownLogs: own), .skip, "iCloud placeholder: skipped")
        check(LibraryCopyRule.rule(for: ".lunta-sync/state.json", destinationHasIt: false, ownLogs: own), .skip, "sync bookkeeping: skipped")
        check(LibraryCopyRule.rule(for: "Cache/index.sqlite", destinationHasIt: false, ownLogs: own), .skip, "unknown folder: skipped")
        check(LibraryCopyRule.mergedLog(destination: "a\nb\n", source: "a\nb\nc\n"), "a\nb\nc\n", "merge adds the missing lines")
        check(LibraryCopyRule.mergedLog(destination: "a\nb", source: "c\n"), "a\nb\nc\n", "merge after a line without newline")
        check(LibraryCopyRule.mergedLog(destination: "a\nb\nc\n", source: "a\nb\n"), nil, "nothing missing: unchanged")

        if failures > 0 { print("\(failures) failed"); exit(1) }
        print("all passed")
    }
}
