import Foundation
import Paper2EpubKit

// Usage: p2e <link | DOI | PMCID | file> [output-dir]
var argv = Array(CommandLine.arguments.dropFirst())
let offline = argv.contains("--offline")
argv.removeAll { $0 == "--offline" }
let args = argv[...]
guard let input = args.first else {
    FileHandle.standardError.write(Data("usage: p2e <link|DOI|PMCID|file> [output-dir]\n".utf8))
    exit(2)
}
let outDir = URL(fileURLWithPath: args.dropFirst().first ?? FileManager.default.currentDirectoryPath)
let log: Paper2Epub.Log = { FileHandle.standardError.write(Data("\($0)\n".utf8)) }

do {
    let r: ConversionResult
    if FileManager.default.fileExists(atPath: input) {
        r = try await Paper2Epub.$offline.withValue(offline) {
            try await Paper2Epub.convert(file: URL(fileURLWithPath: input), into: outDir, log: log)
        }
    } else {
        r = try await Paper2Epub.convert(input, into: outDir, log: log)
    }
    print("✓ \(r.file.path)")
    print("  source: \(r.source)   words: \(r.words)   images: \(r.images)")
    for n in r.notes { print("  note: \(n)") }
} catch {
    print("error: \(error.localizedDescription)")
    exit(1)
}
