import Foundation
import Paper2EpubKit

// Usage: p2e <link | DOI | PMCID | file> [output-dir]
var argv = Array(CommandLine.arguments.dropFirst())
let offline = argv.contains("--offline")
argv.removeAll { $0 == "--offline" }
// --pdf <file.pdf> with a JATS file: figures missing from the XML are cut from that PDF
var pdfPath: String?
if let i = argv.firstIndex(of: "--pdf"), i + 1 < argv.count { pdfPath = argv[i + 1]; argv.removeSubrange(i...(i + 1)) }
let args = argv[...]
guard let input = args.first else {
    FileHandle.standardError.write(Data("usage: p2e <link|DOI|PMCID|file> [output-dir]\n".utf8))
    exit(2)
}
let outDir = URL(fileURLWithPath: args.dropFirst().first ?? FileManager.default.currentDirectoryPath)
let log: Paper2Epub.Log = { FileHandle.standardError.write(Data("\($0)\n".utf8)) }

do {
    let r: ConversionResult
    if let pdfPath {
        r = try await Paper2Epub.convert(jats: URL(fileURLWithPath: input), pdf: URL(fileURLWithPath: pdfPath), into: outDir, log: log)
    } else if FileManager.default.fileExists(atPath: input) {
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
