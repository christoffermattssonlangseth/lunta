import Foundation
import Network
import LuntaCore

/// Local endpoint for the Lunta browser extension, on 127.0.0.1:47480 only.
///
///   GET  /status?doi=…&title=…   → {"inLibrary":bool,"id":…,"hasPdf":bool,"hasReadingView":bool,"title":…}
///   POST /save   {doi,title,authors,journal,date,url,abstract}  → status JSON of the saved paper
///   POST /pdf?id=…   (body: PDF bytes)                          → status JSON
///   POST /pdf?name=… (no id: add / match a standalone PDF)        → status JSON
///
/// Only requests from a browser extension (Origin chrome-extension://…) are accepted, so ordinary
/// web pages cannot use it.
@MainActor
final class ExtensionServer {
    static let port: UInt16 = 47480
    private var listener: NWListener?
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
        default:
            return (404, ["error": "not found"])
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
