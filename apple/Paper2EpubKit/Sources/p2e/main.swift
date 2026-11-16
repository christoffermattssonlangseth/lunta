import Foundation
import Paper2EpubKit

// Usage: p2e <link | DOI | PMCID | file> [output-dir]
var argv = Array(CommandLine.arguments.dropFirst())
let offline = argv.contains("--offline")
argv.removeAll { $0 == "--offline" }
// --pdf <file.pdf> with a JATS file: figures missing from the XML are cut from that PDF
var pdfPath: String?
if let i = argv.firstIndex(of: "--pdf"), i + 1 < argv.count { pdfPath = argv[i + 1]; argv.removeSubrange(i...(i + 1)) }
// --page <url> with a saved HTML file: convert it as the browser extension's "Use this page's full
// text" does; --images <dir>: the figure images the browser would send (matched by file name)
var pageURL: String?
if let i = argv.firstIndex(of: "--page"), i + 1 < argv.count { pageURL = argv[i + 1]; argv.removeSubrange(i...(i + 1)) }
var imagesDir: String?
if let i = argv.firstIndex(of: "--images"), i + 1 < argv.count { imagesDir = argv[i + 1]; argv.removeSubrange(i...(i + 1)) }
// --highlights <file.json>: [{quote,prefix,suffix,start}]; reports which the reader would find in the result
var highlightsPath: String?
if let i = argv.firstIndex(of: "--highlights"), i + 1 < argv.count { highlightsPath = argv[i + 1]; argv.removeSubrange(i...(i + 1)) }
// --reader-text <file>: writes the text of the reading view as the reader sees it
var readerTextPath: String?
if let i = argv.firstIndex(of: "--reader-text"), i + 1 < argv.count { readerTextPath = argv[i + 1]; argv.removeSubrange(i...(i + 1)) }
let args = argv[...]
guard let input = args.first else {
    FileHandle.standardError.write(Data("usage: p2e <link|DOI|PMCID|file> [output-dir]\n".utf8))
    exit(2)
}
let outDir = URL(fileURLWithPath: args.dropFirst().first ?? FileManager.default.currentDirectoryPath)
let log: Paper2Epub.Log = { FileHandle.standardError.write(Data("\($0)\n".utf8)) }

do {
    let r: ConversionResult
    if let pageURL {
        let data = try Data(contentsOf: URL(fileURLWithPath: input))
        let html = String(data: data, encoding: .utf8) ?? String(decoding: data, as: UTF8.self)
        var imgs: [PageImage] = []
        if let imagesDir {
            let dir = URL(fileURLWithPath: imagesDir)
            for f in (try? FileManager.default.contentsOfDirectory(at: dir, includingPropertiesForKeys: nil)) ?? [] {
                if let d = try? Data(contentsOf: f) { imgs.append(PageImage(urls: [f.lastPathComponent], data: d)) }
            }
        }
        let u = URL(string: pageURL)
        log("reader: \(Paper2Epub.pageReader(url: u, html: html)); \(imgs.count) image file(s) offered")
        r = try await Paper2Epub.convert(page: html, url: u, images: imgs, into: outDir, log: log)
    } else if let pdfPath {
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
    if let readerTextPath { try r.readerText.write(toFile: readerTextPath, atomically: true, encoding: .utf8) }
    if let highlightsPath {
        let list = (try JSONSerialization.jsonObject(with: Data(contentsOf: URL(fileURLWithPath: highlightsPath))) as? [[String: Any]]) ?? []
        let loc = HighlightLocator(text: r.readerText)
        var lost = 0
        for h in list {
            let a = HighlightAnchor(quote: h["quote"] as? String ?? "", prefix: h["prefix"] as? String ?? "",
                                    suffix: h["suffix"] as? String ?? "", start: h["start"] as? Int ?? 0)
            let at = loc.locate(a)
            if at == nil { lost += 1 }
            print("  highlight \(at.map { "found at \($0.lowerBound)" } ?? "NOT FOUND"): \(a.quote.prefix(70))")
        }
        print("  highlights: \(list.count - lost) of \(list.count) found")
    }
} catch {
    print("error: \(error.localizedDescription)")
    exit(1)
}
