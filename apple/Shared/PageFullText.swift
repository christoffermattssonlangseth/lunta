import Foundation
import LuntaCore
import Paper2EpubKit

/// "Use this page's full text" (browser extension): the article page the reader has open, with
/// their access, becomes the paper's reading view.
///
/// - No reading view yet: the page's version is attached.
/// - A reading view with highlights: every highlight must still be found in the page's text (the
///   reader's own `locate`, ported as `HighlightLocator`). If one wouldn't be, the current reading
///   view stays and the page's version is only saved as a backup, with the highlights that didn't
///   match listed. Otherwise the current reading view is backed up and replaced.
/// Backups: ~/Library/Application Support/Lunta/backups/<paper id>-<date>/ (on iPhone and iPad,
/// the same folder in the app's own container).
///
/// Shared by Lunta for Mac (the extension's /fulltext, ExtensionServer) and the iPhone and iPad app
/// (Safari's requests from the app group, SafariInboxImport).
struct PageSubmission {
    var html: String
    /// The page as the browser shows it (after scripts ran), tried when `html` can't be converted.
    var liveHTML: String?
    var url: URL?
    var doi: String
    var title: String
    var images: [PageImage]
    /// Figures the browser couldn't send (too large, refused).
    var imagesSkipped: Int
}

struct PageOutcome {
    enum Kind: String { case added, attached, replaced, kept, unchanged }
    var kind: Kind
    var paperID: String?
    var message: String
    var words = 0
    var images = 0
    var notes: [String] = []
    var source = ""
    var reader = ""
    var highlights = 0
    var unmatched: [String] = []
    var backup: String?
    var dryRun = false

    var json: [String: Any] {
        var j: [String: Any] = ["outcome": kind.rawValue, "message": message, "words": words, "images": images, "notes": notes,
                                "source": source, "reader": reader, "highlights": highlights, "unmatched": unmatched, "dryRun": dryRun]
        if let paperID { j["paperId"] = paperID }
        if let backup { j["backup"] = backup }
        return j
    }
}

extension LibraryModel {
    nonisolated static var backupsURL: URL {
        if let t = testLibrary { return t.appendingPathComponent("backups", isDirectory: true) }
        return FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("Lunta/backups", isDirectory: true)
    }

    /// Where a backup went, in words for the extension's panel: the folder on the Mac; on iPhone
    /// and iPad (where the path means nothing to the reader) the app's backups.
    nonisolated static func backupPlace(_ folder: URL) -> String {
        #if os(macOS)
        return folder.path
        #else
        return "Lunta's backups (\(folder.lastPathComponent))"
        #endif
    }

    /// "Save to Lunta" from an article page: the paper if it's already in the library; else added
    /// by its DOI, or (no DOI, or the lookup failed) with what the page said about it
    /// ({doi, title, authors, journal, date, url, abstract}).
    func saveFromPage(_ j: [String: Any]) async -> Paper? {
        let doi = (j["doi"] as? String) ?? ""
        let title = (j["title"] as? String) ?? ""
        if let existing = findPaper(doi: doi, title: title) { return existing }
        if !doi.isEmpty {
            await add(doi)
            if let p = lib.findByDoi(doi: doi) { return p }
        }
        var m = PaperMeta.empty
        m.title = title
        m.authors = (j["authors"] as? [String]) ?? []
        m.journal = (j["journal"] as? String) ?? ""
        m.date = (j["date"] as? String) ?? ""
        m.doi = doi
        m.url = (j["url"] as? String) ?? ""
        m.abstractText = (j["abstract"] as? String) ?? ""
        guard !m.title.isEmpty else { return nil }
        let lib = self.lib
        let meta = m
        let p = try? await Task.detached { try lib.addPaper(meta: meta, pdf: nil, epub: nil, fulltext: nil) }.value
        reload()
        return p
    }

    /// Finds a paper by DOI, else by exact (normalised) title.
    func findPaper(doi: String, title: String) -> Paper? {
        if !doi.isEmpty, let p = lib.findByDoi(doi: doi) { return p }
        guard title.count > 12 else { return nil }
        func norm(_ s: String) -> String { s.lowercased().filter { $0.isLetter || $0.isNumber } }
        let t = norm(title)
        return lib.papers(filter: PaperFilter(collection: nil, tag: nil, starredOnly: false, unreadOnly: false), sort: .added)
            .first { norm($0.meta.title) == t }
    }

    /// Converts the page and attaches it as described above. `dryRun`: converts and checks, but
    /// changes nothing (no paper added, no file written to the library, no backup).
    func usePageFullText(_ s: PageSubmission, dryRun: Bool) async throws -> PageOutcome {
        let tmp = FileManager.default.temporaryDirectory.appendingPathComponent("lunta-page-\(UUID())")
        defer { try? FileManager.default.removeItem(at: tmp) }
        status = "Making a reading view from \(s.url?.host ?? "the page")…"
        defer { status = nil }

        // 1. convert: the page as the server sent it, else as the browser shows it
        var result: ConversionResult
        var used = s.html
        do {
            result = try await Paper2Epub.convert(page: s.html, url: s.url, images: s.images, into: tmp)
        } catch {
            guard let live = s.liveHTML, !live.isEmpty, live != s.html else { throw error }
            NSLog("Lunta: page as sent didn't convert (%@); trying the page as shown", error.localizedDescription)
            result = try await Paper2Epub.convert(page: live, url: s.url, images: s.images, into: tmp)
            used = live
        }
        let reader = Paper2Epub.pageReader(url: s.url, html: used)
        guard let epub = try? Data(contentsOf: result.file) else { throw ConversionError.message("The reading view couldn't be written.") }
        let text = result.fulltext
        var notes = result.notes
        if s.imagesSkipped > 0 { notes.append("\(s.imagesSkipped) figure image(s) were too large to send.") }
        var out = PageOutcome(kind: .attached, paperID: nil, message: "", words: result.words, images: result.images, notes: notes,
                              source: result.source, reader: reader, dryRun: dryRun)
        let figures = result.images == 1 ? "1 figure" : "\(result.images) figures"

        // 2. the paper
        let doi = !result.meta.doi.isEmpty ? result.meta.doi : s.doi
        let lib = self.lib
        guard let paper = findPaper(doi: doi, title: result.meta.title.isEmpty ? s.title : result.meta.title) else {
            out.kind = .added
            if dryRun { out.message = "Dry run: would add this paper with the page's full text (\(result.words) words, \(figures))."; return out }
            var meta = PaperMeta.empty
            if !doi.isEmpty { meta = (await Task.detached { try? resolveMetadata(input: doi) }.value) ?? meta }
            if meta.title.isEmpty { meta = Self.meta(from: result) }
            if meta.doi.isEmpty { meta.doi = doi }
            if meta.url.isEmpty { meta.url = s.url?.absoluteString ?? "" }
            let m = meta
            let added = try await Task.detached { try lib.addPaper(meta: m, pdf: nil, epub: epub, fulltext: text) }.value
            reload()
            out.paperID = added.id
            out.message = "Added to Lunta with the page's full text (\(result.words) words, \(figures))."
            return out
        }
        out.paperID = paper.id
        let id = paper.id

        // 3. no reading view yet: attach
        guard paper.hasEpub else {
            out.kind = .attached
            if dryRun { out.message = "Dry run: would make the reading view from this page (\(result.words) words, \(figures))."; return out }
            try await Task.detached {
                try lib.attachFile(id: id, kind: .epub, data: epub)
                try lib.attachFile(id: id, kind: .fulltext, data: Data(text.utf8))
            }.value
            reload()
            out.message = "Reading view made from this page (\(result.words) words, \(figures))."
            return out
        }

        // 4. a reading view already: its highlights must all be found in the new text
        let anchored = lib.annotations(paper: id).filter {
            $0.location == "epub" && !$0.quote.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        }
        let readerText = result.readerText
        let unmatched: [Annotation] = await Task.detached {
            let loc = HighlightLocator(text: readerText)
            return anchored.filter { a in
                loc.locate(HighlightAnchor(quote: a.quote, prefix: a.prefix, suffix: a.suffix, start: Int(a.position) ?? 0)) == nil
            }
        }.value
        out.highlights = anchored.count
        out.unmatched = unmatched.map { String($0.quote.prefix(160)) }
        let stamp = Self.backupStamp()
        let folder = Self.backupsURL.appendingPathComponent("\(id)-\(stamp)", isDirectory: true)

        if !unmatched.isEmpty {
            out.kind = .kept
            let n = unmatched.count
            let which = unmatched.prefix(3).map { "“\(String($0.quote.prefix(60)))\($0.quote.count > 60 ? "…" : "")”" }.joined(separator: ", ")
            out.message = "Kept the current reading view: \(n) of \(anchored.count) highlight\(anchored.count == 1 ? "" : "s") wouldn't be found in this page's text (\(which)\(n > 3 ? ", …" : ""))."
            if dryRun { out.message = "Dry run: " + out.message; return out }
            // the page's version is kept beside it, in case it's wanted after all
            let folder = Self.backupsURL.appendingPathComponent("\(id)-\(stamp)-page-not-used", isDirectory: true)
            try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
            try epub.write(to: folder.appendingPathComponent("page-book.epub"))
            try Data(text.utf8).write(to: folder.appendingPathComponent("page-fulltext.txt"))
            let list = unmatched.map { "- \($0.quote.replacingOccurrences(of: "\n", with: " "))" }.joined(separator: "\n")
            try Data("Highlights not found in the page's text (\(s.url?.absoluteString ?? "")):\n\(list)\n".utf8)
                .write(to: folder.appendingPathComponent("unmatched-highlights.txt"))
            out.backup = folder.path
            out.message += " The page's version is saved in \(Self.backupPlace(folder))."
            return out
        }

        out.kind = .replaced
        if dryRun {
            out.message = "Dry run: would replace the reading view (\(anchored.count) highlight\(anchored.count == 1 ? "" : "s") all found in the new text) after backing up the current one."
            return out
        }
        // back up the current reading view (and its text), then replace it
        guard let oldEpub = await ensureLocal(id, .epub), let oldData = try? Data(contentsOf: oldEpub) else {
            throw ConversionError.message("The current reading view isn't downloaded from iCloud yet, so it can't be backed up. Try again in a moment.")
        }
        if oldData == epub {
            out.kind = .unchanged
            out.message = "The reading view already matches this page."
            return out
        }
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        try oldData.write(to: folder.appendingPathComponent("book.epub"))
        if let ft = await ensureLocal(id, .fulltext), let d = try? Data(contentsOf: ft) {
            try d.write(to: folder.appendingPathComponent("fulltext.txt"))
        }
        try await Task.detached {
            try lib.attachFile(id: id, kind: .epub, data: epub)
            try lib.attachFile(id: id, kind: .fulltext, data: Data(text.utf8))
        }.value
        reload()
        out.backup = folder.path
        let kept = anchored.isEmpty ? "" : " All \(anchored.count) highlight\(anchored.count == 1 ? "" : "s") found in the new text."
        out.message = "Reading view updated from this page (\(result.words) words, \(figures)).\(kept) The previous one is saved in \(Self.backupPlace(folder))."
        return out
    }

    nonisolated static func backupStamp() -> String {
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        f.dateFormat = "yyyy-MM-dd-HHmmss"
        return f.string(from: Date())
    }
}
