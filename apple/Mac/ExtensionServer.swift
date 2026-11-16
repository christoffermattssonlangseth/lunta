import Foundation
import Network
import LuntaCore
import Paper2EpubKit

/// Local endpoint for the Lunta browser extension, on 127.0.0.1:47480 only.
///
///   GET  /status?doi=…&title=…   → {"inLibrary":bool,"id":…,"hasPdf":bool,"hasReadingView":bool,"title":…}
///   POST /save   {doi,title,authors,journal,date,url,abstract}  → status JSON of the saved paper
///   POST /pdf?id=…   (body: PDF bytes)                          → status JSON
///   POST /pdf?name=… (no id: add / match a standalone PDF)        → status JSON
///   POST /fulltext[?dryRun=1]  {url, html, liveHtml?, doi?, title?, images:[{urls:[…], base64}],
///                               imagesSkipped?, dryRun?}
///        "Use this page's full text": the article page (as the server sent it; `liveHtml` as the
///        browser shows it, tried if `html` can't be converted) becomes the paper's reading view;
///        figures come from `images` (fetched by the browser with the reader's access).
///        → status JSON + {outcome: added|attached|replaced|kept|unchanged, message, words, images,
///          highlights, unmatched:[quotes], backup?, notes, reader, dryRun}; 422 + {error} if the
///          page can't be made into a reading view.
///
/// Only requests from a browser extension (Origin chrome-extension://…) are accepted, so ordinary
/// web pages cannot use it.
@MainActor
final class ExtensionServer {
    /// 47480; a Debug build may be given another with `-extensionPort <n>` (to test beside the real app).
    static let port: UInt16 = {
        #if DEBUG
        let a = ProcessInfo.processInfo.arguments
        if let i = a.firstIndex(of: "-extensionPort"), i + 1 < a.count, let p = UInt16(a[i + 1]) { return p }
        #endif
        return 47480
    }()
    private var listener: NWListener?
    private var pagesInProgress = Set<String>()
    private weak var model: LibraryModel?

    init(model: LibraryModel) {
        self.model = model
    }

    func start() {
        let params = NWParameters.tcp
        params.requiredLocalEndpoint = NWEndpoint.hostPort(host: "127.0.0.1", port: NWEndpoint.Port(rawValue: Self.port)!)
        params.allowLocalEndpointReuse = true
        guard let l = try? NWListener(using: params) else { return }
        l.newConnectionHandler = { [weak self] conn in
            Task { @MainActor in self?.handle(conn) }
        }
        l.start(queue: .main)
        listener = l
    }

    // MARK: - HTTP plumbing

    private struct Request {
        var method = ""
        var path = ""
        var query: [String: String] = [:]
        var headers: [String: String] = [:]
        var body = Data()
    }

    private func handle(_ conn: NWConnection) {
        conn.start(queue: .main)
        receive(conn, buffer: Data())
    }

    private func receive(_ conn: NWConnection, buffer: Data) {
        conn.receive(minimumIncompleteLength: 1, maximumLength: 1 << 20) { [weak self] data, _, done, error in
            Task { @MainActor in
                guard let self else { return }
                var buf = buffer
                if let data { buf.append(data) }
                if let req = Self.parse(buf) {
                    let (status, json) = await self.route(req)
                    self.respond(conn, status: status, json: json, origin: req.headers["origin"])
                } else if done || error != nil || buf.count > 300 * 1024 * 1024 {
                    conn.cancel()
                } else {
                    self.receive(conn, buffer: buf)
                }
            }
        }
    }

    /// Returns a request once headers and the full body (per Content-Length) have arrived.
    private static func parse(_ d: Data) -> Request? {
        guard let headEnd = d.range(of: Data("\r\n\r\n".utf8)) else { return nil }
        let head = String(decoding: d[..<headEnd.lowerBound], as: UTF8.self)
        var lines = head.components(separatedBy: "\r\n")
        let first = lines.removeFirst().split(separator: " ")
        guard first.count >= 2 else { return nil }
        var r = Request()
        r.method = String(first[0])
        let target = String(first[1])
        let comps = URLComponents(string: "http://x\(target)")
        r.path = comps?.path ?? target
        for q in comps?.queryItems ?? [] { r.query[q.name] = q.value ?? "" }
        for l in lines {
            if let i = l.firstIndex(of: ":") {
                r.headers[l[..<i].lowercased()] = l[l.index(after: i)...].trimmingCharacters(in: .whitespaces)
            }
        }
        let len = Int(r.headers["content-length"] ?? "0") ?? 0
        let bodyStart = headEnd.upperBound
        guard d.count - bodyStart >= len else { return nil }
        r.body = d.subdata(in: bodyStart..<(bodyStart + len))
        return r
    }

    private func respond(_ conn: NWConnection, status: Int, json: Any, origin: String?) {
        let body = (try? JSONSerialization.data(withJSONObject: json)) ?? Data("{}".utf8)
        var head = "HTTP/1.1 \(status) \(status == 200 ? "OK" : "Error")\r\n"
        head += "Content-Type: application/json\r\nContent-Length: \(body.count)\r\nConnection: close\r\n"
        if let origin, origin.hasPrefix("chrome-extension://") || origin.hasPrefix("safari-web-extension://") {
            head += "Access-Control-Allow-Origin: \(origin)\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\n"
            head += "Access-Control-Allow-Headers: Content-Type, X-Margins-Client\r\nAccess-Control-Allow-Private-Network: true\r\n"
        }
        head += "\r\n"
        var out = Data(head.utf8)
        out.append(body)
        conn.send(content: out, completion: .contentProcessed { _ in conn.cancel() })
    }

    // MARK: - Routes

    private func route(_ r: Request) async -> (Int, Any) {
        // Web pages cannot send custom headers to another origin without a CORS preflight, which we
        // never approve for web origins; the extension sends this header. Pages with an http(s) Origin
        // are refused outright.
        let origin = r.headers["origin"] ?? ""
        let fromWebPage = origin.hasPrefix("http://") || origin.hasPrefix("https://") || origin == "null"
        if r.method == "OPTIONS" { return fromWebPage ? (403, [:]) : (200, [:]) }
        guard !fromWebPage, r.headers["x-margins-client"] == "extension" else {
            return (403, ["error": "only the Lunta browser extension may use this endpoint"])
        }
        guard let model else { return (503, ["error": "library not open"]) }
        switch (r.method, r.path) {
        case ("GET", "/status"):
            return (200, status(of: find(model, doi: r.query["doi"] ?? "", title: r.query["title"] ?? "")))
        case ("POST", "/save"):
            guard let j = try? JSONSerialization.jsonObject(with: r.body) as? [String: Any] else { return (400, ["error": "bad JSON"]) }
            let p = await save(model, j)
            return (p == nil ? 500 : 200, status(of: p))
        case ("POST", "/pdf"):
            guard r.body.starts(with: Data("%PDF".utf8)) else { return (400, ["error": "not a PDF"]) }
            guard let id = r.query["id"], !id.isEmpty else {
                // a PDF on its own (e.g. open in Chrome's viewer): add it as a new paper or match it by DOI
                let name = r.query["name"] ?? "paper.pdf"
                let p = await model.addPDF(r.body, name: name)
                model.reload()
                return (p == nil ? 500 : 200, status(of: p))
            }
            guard model.paper(id) != nil else { return (404, ["error": "unknown paper"]) }
            let lib = model.lib
            let data = r.body
            try? await Task.detached { try lib.attachFile(id: id, kind: .pdf, data: data) }.value
            model.reload()
            Task { if model.paper(id)?.hasEpub == false { await model.makeReadingView(id); model.reload() } }
            return (200, status(of: model.paper(id)))
        case ("POST", "/fulltext"):
            return await fullText(model, r)
        #if DEBUG
        // test hooks, scratch library only (`-testLibrary`): highlight a quote, inspect a paper
        case ("POST", "/debug/highlight") where LibraryModel.testLibrary != nil:
            guard let j = try? JSONSerialization.jsonObject(with: r.body) as? [String: Any],
                  let id = j["id"] as? String, let quote = j["quote"] as? String else { return (400, ["error": "bad JSON"]) }
            do {
                let a = try model.lib.addHighlight(paper: id, quote: quote, before: nil, color: "yellow", note: "")
                return (200, ["id": a.id, "position": a.position, "location": a.location])
            } catch { return (422, ["error": error.readable]) }
        case ("GET", "/debug/paper") where LibraryModel.testLibrary != nil:
            guard let id = r.query["id"], let p = model.paper(id) else { return (404, ["error": "unknown paper"]) }
            let epubSize = model.fileURL(id, .epub).flatMap { try? FileManager.default.attributesOfItem(atPath: $0.path)[.size] as? Int } ?? 0
            return (200, ["title": p.meta.title, "doi": p.meta.doi, "hasEpub": p.hasEpub, "epubBytes": epubSize,
                          "annotations": model.lib.annotations(paper: id).count])
        #endif
        default:
            return (404, ["error": "not found"])
        }
    }

    private func fullText(_ model: LibraryModel, _ r: Request) async -> (Int, Any) {
        let body = r.body
        // large (the page and its figures): decoded off the main thread
        let parsed: (PageSubmission, Bool)? = await Task.detached { () -> (PageSubmission, Bool)? in
            guard let j = try? JSONSerialization.jsonObject(with: body) as? [String: Any],
                  let html = j["html"] as? String ?? j["liveHtml"] as? String, !html.isEmpty else { return nil }
            var images: [PageImage] = []
            for i in (j["images"] as? [[String: Any]]) ?? [] {
                guard let b = i["base64"] as? String, let d = Data(base64Encoded: b), !d.isEmpty else { continue }
                let urls = ((i["urls"] as? [String]) ?? []) + [(i["url"] as? String)].compactMap { $0 }
                images.append(PageImage(urls: urls, data: d))
            }
            let sub = PageSubmission(html: html, liveHTML: j["liveHtml"] as? String, url: (j["url"] as? String).flatMap(URL.init(string:)),
                                     doi: (j["doi"] as? String) ?? "", title: (j["title"] as? String) ?? "", images: images,
                                     imagesSkipped: (j["imagesSkipped"] as? Int) ?? 0)
            return (sub, (j["dryRun"] as? Bool) ?? false)
        }.value
        guard let (sub, dryInBody) = parsed else { return (400, ["error": "Expected JSON with the page's html."]) }
        let dry = dryInBody || r.query["dryRun"] == "1"
        let key = sub.url?.absoluteString ?? sub.doi
        guard pagesInProgress.insert(key).inserted else { return (409, ["error": "Lunta is already working on this page."]) }
        defer { pagesInProgress.remove(key) }
        do {
            let o = try await model.usePageFullText(sub, dryRun: dry)
            var j = status(of: o.paperID.flatMap { model.paper($0) })
            for (k, v) in o.json { j[k] = v }
            return (200, j)
        } catch {
            return (422, ["error": error.readable])
        }
    }

    private func find(_ model: LibraryModel, doi: String, title: String) -> Paper? {
        if !doi.isEmpty, let p = model.lib.findByDoi(doi: doi) { return p }
        guard title.count > 12 else { return nil }
        func norm(_ s: String) -> String { s.lowercased().filter { $0.isLetter || $0.isNumber } }
        let t = norm(title)
        return model.lib.papers(filter: PaperFilter(collection: nil, tag: nil, starredOnly: false, unreadOnly: false), sort: .added)
            .first { norm($0.meta.title) == t }
    }

    private func status(of p: Paper?) -> [String: Any] {
        guard let p else { return ["inLibrary": false] }
        return ["inLibrary": true, "id": p.id, "title": p.meta.title, "hasPdf": p.hasPdf, "hasReadingView": p.hasEpub]
    }

    private func save(_ model: LibraryModel, _ j: [String: Any]) async -> Paper? {
        let doi = (j["doi"] as? String) ?? ""
        let title = (j["title"] as? String) ?? ""
        if let existing = find(model, doi: doi, title: title) { return existing }
        if !doi.isEmpty {
            await model.add(doi)
            if let p = model.lib.findByDoi(doi: doi) { return p }
        }
        // no DOI (or lookup failed): save what the page told us
        var m = PaperMeta.empty
        m.title = title
        m.authors = (j["authors"] as? [String]) ?? []
        m.journal = (j["journal"] as? String) ?? ""
        m.date = (j["date"] as? String) ?? ""
        m.doi = doi
        m.url = (j["url"] as? String) ?? ""
        m.abstractText = (j["abstract"] as? String) ?? ""
        guard !m.title.isEmpty else { return nil }
        let lib = model.lib
        let meta = m
        let p = try? await Task.detached { try lib.addPaper(meta: meta, pdf: nil, epub: nil, fulltext: nil) }.value
        model.reload()
        return p
    }
}
