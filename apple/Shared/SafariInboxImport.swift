import Foundation
import LuntaCore
import Paper2EpubKit

/// The app's side of the Safari extension on iPhone and iPad (SafariInbox): what was sent from
/// Safari is done here, the same way Lunta for Mac does it for the extension
/// (save → add the paper, pdf → attach it, fulltext → "Use this page's full text", with the
/// highlight check and the backup). Runs when the app becomes active and on lunta://safari.
extension LibraryModel {
    /// The inbox: the App Group's on iPhone and iPad; a scratch library's own (`-testLibrary`) in tests.
    nonisolated static var safariRoot: URL? {
        if let t = testLibrary { return t.appendingPathComponent("SafariInbox", isDirectory: true) }
        #if os(iOS)
        return SafariInbox.groupRoot
        #else
        return nil
        #endif
    }

    /// Does what Safari sent, oldest first. Returns one line per request, and whether any of
    /// them should be shown (a reading view made or kept, something that failed).
    ///
    /// In two steps, so a paper saved from Safari is in the list (and readable, when its PDF was
    /// sent) within a second of opening the app: first what needs no network (the paper added
    /// with what its page said, the PDF attached), then, in the background (`safariFollowUp`),
    /// its details from Crossref and Europe PMC, a free PDF if asked for, and the reading view.
    /// Before, each request was done in full in turn (the details looked up, the reading view
    /// made) before the paper was added and the next request (its PDF) was looked at.
    /// `waitForFollowUp`: returns only when the background step is done too (tests).
    @discardableResult
    func importSafari(waitForFollowUp: Bool = false) async -> (lines: [String], show: Bool) {
        guard let root = Self.safariRoot else { return ([], false) }
        SafariInbox.cleanStaging(root: root, olderThan: 24 * 3600)
        let items = SafariInbox.pending(root: root)
        guard !items.isEmpty else { return ([], false) }
        var lines: [String] = [], show = false
        var follow: [SafariFollowUp] = []
        for (dir, req) in items {
            let (line, important) = await importSafariRequest(req, in: dir, followUp: &follow)
            try? FileManager.default.removeItem(at: dir)
            NSLog("Lunta: from Safari: %@", line)
            lines.append(line)
            show = show || important
        }
        reload()
        publishSafariIndex()
        if !follow.isEmpty {
            for f in follow { preparing[f.id] = f.findPdf ? "Finding the PDF…" : "Finding the details…" }
            let before = safariFollowUp
            safariFollowUp = Task {
                await before?.value
                await self.runSafariFollowUp(follow)
            }
        }
        if waitForFollowUp { await safariFollowUp?.value }
        return (lines, show)
    }

    /// The background step for one paper from Safari.
    struct SafariFollowUp {
        var id: String
        /// the page's details were used: look the paper up by its DOI and take Crossref's
        var doi = ""
        /// no PDF link on the page: look for a free one
        var findPdf = false
    }

    private func runSafariFollowUp(_ items: [SafariFollowUp]) async {
        // one entry per paper (its Save and its PDF came as two requests)
        var seen: [String: SafariFollowUp] = [:], order: [String] = []
        for f in items {
            if var x = seen[f.id] {
                if x.doi.isEmpty { x.doi = f.doi }
                x.findPdf = x.findPdf || f.findPdf
                seen[f.id] = x
            } else { seen[f.id] = f; order.append(f.id) }
        }
        let lib = self.lib
        for id in order {
            guard let f = seen[id] else { continue }
            defer { preparing[id] = nil }
            if !f.doi.isEmpty {
                preparing[id] = "Finding the details…"
                let doi = f.doi
                if var m = await Task.detached(priority: .userInitiated, operation: { try? resolveMetadata(input: doi) }).value,
                   !m.title.isEmpty, let now = lib.paper(id: id) {
                    // Crossref's details, with what the page said for what Crossref lacks
                    Self.fill(&m, from: now.meta)
                    try? await Task.detached { try lib.updateMeta(id: id, meta: m) }.value
                    reload()
                }
            }
            guard let p = lib.paper(id: id), !p.hasEpub else { continue }
            if !p.hasPdf, f.findPdf {
                let found = await findPDF(id)
                NSLog("Lunta: from Safari: %@", found != nil ? "found a free PDF of “\(p.meta.title)”" : "no free PDF of “\(p.meta.title)”")
            } else {
                await preparingReadingView(id)
            }
        }
        publishSafariIndex()
    }

    /// The page's details ({doi, title, authors, journal, date, url, abstract, pmid, pmcid}) as a
    /// paper's: nil without a title.
    static func pageMeta(_ j: [String: Any]) -> PaperMeta? {
        var m = PaperMeta.empty
        m.title = ((j["title"] as? String) ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        guard !m.title.isEmpty else { return nil }
        m.authors = (j["authors"] as? [String]) ?? []
        m.journal = (j["journal"] as? String) ?? ""
        m.date = (j["date"] as? String) ?? ""
        m.doi = (j["doi"] as? String) ?? ""
        m.pmid = (j["pmid"] as? String) ?? ""
        m.pmcid = (j["pmcid"] as? String) ?? ""
        m.url = (j["url"] as? String) ?? ""
        m.abstractText = (j["abstract"] as? String) ?? ""
        return m
    }

    /// The paper a Save from Safari is about: the one in the library, or added at once with what
    /// its page said (no network; its details are looked up in the background step). nil when
    /// the page gave no title: then it is added by its DOI (looked up now), as before.
    private func safariPaper(_ meta: [String: Any], doi: String, title: String, followUp: inout [SafariFollowUp]) async -> (Paper, new: Bool)? {
        if let p = findPaper(doi: doi, title: title) { return (p, false) }
        if let m = Self.pageMeta(meta.isEmpty ? ["doi": doi, "title": title] : meta) {
            let lib = self.lib
            guard let p = try? await Task.detached(operation: { try lib.addPaper(meta: m, pdf: nil, epub: nil, fulltext: nil) }).value else { return nil }
            followUp.append(SafariFollowUp(id: p.id, doi: m.doi))
            return (p, true)
        }
        guard let p = await saveFromPage(meta.isEmpty ? ["doi": doi, "title": title] : meta) else { return nil }
        return (p, true)
    }

    private func importSafariRequest(_ req: [String: Any], in dir: URL, followUp: inout [SafariFollowUp]) async -> (String, Bool) {
        let (doi, title) = SafariInbox.identity(req)
        let meta = req["meta"] as? [String: Any] ?? [:]
        switch req["kind"] as? String {
        case "save":
            guard let (p, new) = await safariPaper(meta, doi: doi, title: title, followUp: &followUp) else {
                return ("Couldn't add “\(title.isEmpty ? doi : title)” from Safari.", true)
            }
            if !p.hasPdf, meta["findPdf"] as? Bool == true { followUp.append(SafariFollowUp(id: p.id, findPdf: true)) }
            else if new { followUp.append(SafariFollowUp(id: p.id)) }   // (the reading view)
            return (new ? "Added “\(p.meta.title)”." : "“\(p.meta.title)” is already in your library.", false)

        case "findpdf":
            guard let p = (req["paperId"] as? String).flatMap({ $0.isEmpty ? nil : self.paper($0) }) ?? findPaper(doi: doi, title: title) else {
                return ("Couldn't find “\(title.isEmpty ? doi : title)” in your library.", true)
            }
            if p.hasPdf { return ("“\(p.meta.title)” already has its PDF.", false) }
            followUp.append(SafariFollowUp(id: p.id, findPdf: true))
            return ("Looking for a free PDF of “\(p.meta.title)”.", false)

        case "pdf":
            guard let file = SafariInbox.file(req["pdf"], in: dir), let data = try? Data(contentsOf: file),
                  data.starts(with: Array("%PDF".utf8)) else {
                return ("A PDF sent from Safari couldn't be read.", true)
            }
            var paper = (req["paperId"] as? String).flatMap { $0.isEmpty ? nil : self.paper($0) }
            if paper == nil, !doi.isEmpty || !title.isEmpty { paper = await safariPaper(meta, doi: doi, title: title, followUp: &followUp)?.0 }
            if let p = paper {
                let lib = self.lib, id = p.id
                do { try await Task.detached { try lib.attachFile(id: id, kind: .pdf, data: data) }.value } catch {
                    return ("Couldn't save the PDF of “\(p.meta.title)”: \(error.readable)", true)
                }
                reload()
                // readable now (its PDF); the reading view is made in the background step
                if self.paper(id)?.hasEpub == false { followUp.append(SafariFollowUp(id: id)) }
                return ("PDF saved to “\(p.meta.title)”.", false)
            }
            let name = (req["name"] as? String).flatMap { $0.isEmpty ? nil : $0 } ?? "paper.pdf"
            guard let p = await addPDF(data, name: name) else { return ("Couldn't add the PDF “\(name)”.", true) }
            return ("Added the PDF “\(p.meta.title)”.", false)

        case "webpage":
            guard let url = req["url"] as? String, url.hasPrefix("http") else { return ("A page sent from Safari had no address.", true) }
            let html = SafariInbox.file(req["html"], in: dir).flatMap { try? String(contentsOf: $0, encoding: .utf8) }
            let before = lib.findByUrl(url: url)
            switch await addWebPage(url, html: html) {
            case .success(let id):
                let t = paper(id)?.meta.title ?? url
                return (before == nil ? "Saved “\(t)” as a web page." : "“\(t)” is already in your library.", false)
            case .failure(let e):
                return ("Couldn't save the page \(url): \(e.readable)", true)
            }

        case "fulltext":
            let url = (req["url"] as? String).flatMap(URL.init(string:))
            guard let htmlFile = SafariInbox.file(req["html"], in: dir), let html = try? String(contentsOf: htmlFile, encoding: .utf8) else {
                return ("A page sent from Safari couldn't be read.", true)
            }
            let live = SafariInbox.file(req["liveHtml"], in: dir).flatMap { try? String(contentsOf: $0, encoding: .utf8) }
            var images: [PageImage] = []
            for i in (req["images"] as? [[String: Any]]) ?? [] {
                guard let f = SafariInbox.file(i["file"], in: dir), let d = try? Data(contentsOf: f), !d.isEmpty else { continue }
                images.append(PageImage(urls: (i["urls"] as? [String]) ?? [], data: d))
            }
            let sub = PageSubmission(html: html, liveHTML: live, url: url, doi: doi, title: title, images: images,
                                     imagesSkipped: (req["imagesSkipped"] as? Int) ?? 0)
            do {
                let o = try await usePageFullText(sub, dryRun: (req["dryRun"] as? Bool) ?? false)
                return (o.message, true)
            } catch {
                return ("The page from \(url?.host ?? "Safari") couldn't be made into a reading view: \(error.readable)", true)
            }

        default:
            return ("Lunta couldn't understand a request from Safari (made by a newer version?).", true)
        }
    }

    /// What the Safari extension needs to answer "is this paper in Lunta?" without the app:
    /// written to the inbox whenever the library changes (iPhone and iPad).
    func publishSafariIndex() {
        guard let root = Self.safariRoot else { return }
        let entries = lib.papers(filter: PaperFilter(collection: nil, tag: nil, starredOnly: false, unreadOnly: false), sort: .added)
            .map { SafariInbox.Entry(id: $0.id, doi: $0.meta.doi, title: $0.meta.title, pdf: $0.hasPdf, epub: $0.hasEpub) }
        guard entries != Self.lastSafariIndex else { return }
        Self.lastSafariIndex = entries
        Task.detached(priority: .utility) { try? SafariInbox.writeIndex(entries, root: root) }
    }

    private static var lastSafariIndex: [SafariInbox.Entry] = []

    #if DEBUG && os(macOS)
    /// Test hook (scratch library only): `-testLibrary <dir> -testSafariInbox <script.jsonl>` plays
    /// the messages background.js sent (recorded from the real script with a stand-in for Safari's
    /// native messaging) through SafariInbox.handle as the extension would, and lets the app do
    /// them as on the iPhone. Lines: a message ({op: …}); {debug: "process"}; {debug: "index"};
    /// {debug: "highlight", doi, quote}; {debug: "paper", doi}. Answers go to <script>.out, then exit.
    func runSafariInboxScript(_ script: URL) async {
        guard Self.testLibrary != nil, let root = Self.safariRoot,
              let text = try? String(contentsOf: script, encoding: .utf8) else { exit(2) }
        var out: [String] = []
        var tokens: [String: String] = [:]   // recorded token → the handler's
        var recorded = 0
        func line(_ j: Any) { out.append(String(decoding: (try? JSONSerialization.data(withJSONObject: j, options: [.sortedKeys])) ?? Data(), as: UTF8.self)) }
        for l in text.split(separator: "\n") where !l.trimmingCharacters(in: .whitespaces).isEmpty {
            guard var m = try? JSONSerialization.jsonObject(with: Data(l.utf8)) as? [String: Any] else { line(["bad line": String(l.prefix(80))]); continue }
            switch m["debug"] as? String {
            case "process":
                let r = await importSafari(waitForFollowUp: true)
                line(["process": r.lines])
            case "index":
                publishSafariIndex()
                try? await Task.sleep(for: .milliseconds(500))
                line(["index": "written"])
            case "highlight":
                guard let p = findPaper(doi: m["doi"] as? String ?? "", title: ""), let q = m["quote"] as? String else { line(["highlight": "no paper"]); continue }
                do { let a = try lib.addHighlight(paper: p.id, quote: q, before: nil, color: "yellow", note: ""); line(["highlight": a.location]) }
                catch { line(["highlight": error.readable]) }
            case "paper":
                guard let p = findPaper(doi: m["doi"] as? String ?? "", title: "") else { line(["paper": "none"]); continue }
                let size = fileURL(p.id, .epub).flatMap { try? FileManager.default.attributesOfItem(atPath: $0.path)[.size] as? Int } ?? 0
                line(["paper": p.meta.title, "hasEpub": p.hasEpub, "hasPdf": p.hasPdf, "epubBytes": size,
                      "annotations": lib.annotations(paper: p.id).count])
            default:
                if let t = m["token"] as? String { m["token"] = tokens[t] ?? t }
                let r = SafariInbox.handle(m, root: root)
                if m["op"] as? String == "begin", let real = r["token"] as? String {
                    recorded += 1
                    tokens["stub-\(recorded)"] = real
                }
                var summary = r
                summary["op"] = m["op"]
                if let f = m["file"] { summary["file"] = f }
                line(summary)
            }
        }
        try? out.joined(separator: "\n").write(to: script.appendingPathExtension("out"), atomically: true, encoding: .utf8)
        exit(0)
    }
    #endif
}
