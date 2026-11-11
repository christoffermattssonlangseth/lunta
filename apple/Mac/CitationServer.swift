import Foundation
import Network
import Security
import MarginsCore

/// HTTPS server on https://localhost:47481 for the citation add-ins (Word desktop + web, Google Docs).
///
/// Serves the add-in pages from the app bundle (`citations/…`) and a small JSON API:
///   GET  /api/search?q=…               → [{id,title,authors,year,journal,doi}]
///   POST /api/bibliography {style,ids}  → {entries:[…]}   (Markdown-style *italic* / **bold**)
///   GET  /api/status                    → {ok:true,papers:n}
///
/// Pages served from this origin can call the API directly. The Google Docs sidebar (served by
/// Google from *.googleusercontent.com) must also send the pairing token shown in Margins.
@MainActor
final class CitationServer {
    static let port: UInt16 = 47481
    private var listener: NWListener?
    private weak var model: LibraryModel?
    let token: String

    nonisolated static var tlsDir: URL {
        CloudLibrary.localURL.deletingLastPathComponent().appendingPathComponent("tls", isDirectory: true)
    }

    init(model: LibraryModel) {
        self.model = model
        token = Self.token
    }

    /// Pairing key the Google Docs sidebar must send.
    nonisolated static var token: String {
        if let t = UserDefaults.standard.string(forKey: "citationToken") { return t }
        let t = String(UUID().uuidString.replacingOccurrences(of: "-", with: "").prefix(12)).lowercased()
        UserDefaults.standard.set(t, forKey: "citationToken")
        return t
    }

    func start() {
        guard let identity = Self.loadIdentity() else {
            NSLog("Margins: no TLS identity; citation add-ins unavailable")
            return
        }
        let tls = NWProtocolTLS.Options()
        sec_protocol_options_set_local_identity(tls.securityProtocolOptions, sec_identity_create(identity)!)
        sec_protocol_options_set_min_tls_protocol_version(tls.securityProtocolOptions, .TLSv12)
        let params = NWParameters(tls: tls)
        params.requiredLocalEndpoint = NWEndpoint.hostPort(host: "127.0.0.1", port: NWEndpoint.Port(rawValue: Self.port)!)
        params.allowLocalEndpointReuse = true
        guard let l = try? NWListener(using: params) else { return }
        l.newConnectionHandler = { [weak self] c in Task { @MainActor in self?.accept(c) } }
        l.start(queue: .main)
        listener = l
    }

    // MARK: - Certificate (local CA + localhost certificate, made with the system's openssl)

    /// Creates the local certificate authority and the localhost certificate on first use.
    nonisolated static func ensureCertificates() -> Bool {
        let dir = tlsDir
        let fm = FileManager.default
        if fm.fileExists(atPath: dir.appendingPathComponent("server.p12").path) { return true }
        try? fm.createDirectory(at: dir, withIntermediateDirectories: true)
        let ext = """
        basicConstraints=CA:FALSE
        keyUsage=digitalSignature,keyEncipherment
        extendedKeyUsage=serverAuth
        subjectAltName=DNS:localhost,IP:127.0.0.1
        """
        try? ext.write(to: dir.appendingPathComponent("server.ext"), atomically: true, encoding: .utf8)
        let steps: [[String]] = [
            ["req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "3650", "-keyout", "ca.key", "-out", "ca.pem",
             "-subj", "/CN=Margins Local CA (this Mac only)"],
            ["req", "-newkey", "rsa:2048", "-nodes", "-keyout", "server.key", "-out", "server.csr", "-subj", "/CN=localhost"],
            ["x509", "-req", "-in", "server.csr", "-CA", "ca.pem", "-CAkey", "ca.key", "-CAcreateserial",
             "-out", "server.pem", "-days", "820", "-sha256", "-extfile", "server.ext"],
            ["pkcs12", "-export", "-in", "server.pem", "-inkey", "server.key", "-out", "server.p12", "-passout", "pass:margins"],
        ]
        for args in steps {
            let p = Process()
            p.executableURL = URL(fileURLWithPath: "/usr/bin/openssl")
            p.arguments = args
            p.currentDirectoryURL = dir
            p.standardOutput = FileHandle.nullDevice
            p.standardError = FileHandle.nullDevice
            do { try p.run(); p.waitUntilExit() } catch { return false }
            if p.terminationStatus != 0 { return false }
        }
        try? fm.removeItem(at: dir.appendingPathComponent("ca.key"))   // the CA can sign nothing else
        return true
    }

    private static func loadIdentity() -> SecIdentity? {
        guard ensureCertificates(), let data = try? Data(contentsOf: tlsDir.appendingPathComponent("server.p12")) else { return nil }
        var items: CFArray?
        let status = SecPKCS12Import(data as CFData, [kSecImportExportPassphrase as String: "margins"] as CFDictionary, &items)
        guard status == errSecSuccess, let arr = items as? [[String: Any]], let first = arr.first,
              let id = first[kSecImportItemIdentity as String] else { return nil }
        return (id as! SecIdentity)
    }

    // MARK: - HTTP

    private struct Request {
        var method = "", path = ""
        var query: [String: String] = [:]
        var headers: [String: String] = [:]
        var body = Data()
    }

    private func accept(_ c: NWConnection) {
        c.start(queue: .main)
        read(c, Data())
    }

    private func read(_ c: NWConnection, _ buffer: Data) {
        c.receive(minimumIncompleteLength: 1, maximumLength: 1 << 20) { [weak self] data, _, done, error in
            Task { @MainActor in
                guard let self else { return }
                var buf = buffer
                if let data { buf.append(data) }
                if let req = Self.parse(buf) {
                    self.respond(c, req)
                } else if done || error != nil || buf.count > 10 << 20 {
                    c.cancel()
                } else {
                    self.read(c, buf)
                }
            }
        }
    }

    private static func parse(_ d: Data) -> Request? {
        guard let end = d.range(of: Data("\r\n\r\n".utf8)) else { return nil }
        var lines = String(decoding: d[..<end.lowerBound], as: UTF8.self).components(separatedBy: "\r\n")
        let first = lines.removeFirst().split(separator: " ")
        guard first.count >= 2 else { return nil }
        var r = Request()
        r.method = String(first[0])
        let comps = URLComponents(string: "https://x\(first[1])")
        r.path = comps?.path ?? "/"
        for q in comps?.queryItems ?? [] { r.query[q.name] = q.value ?? "" }
        for l in lines {
            if let i = l.firstIndex(of: ":") { r.headers[l[..<i].lowercased()] = l[l.index(after: i)...].trimmingCharacters(in: .whitespaces) }
        }
        let len = Int(r.headers["content-length"] ?? "0") ?? 0
        guard d.count - end.upperBound >= len else { return nil }
        r.body = d.subdata(in: end.upperBound..<(end.upperBound + len))
        return r
    }

    private func send(_ c: NWConnection, _ status: Int, _ body: Data, type: String, origin: String?) {
        var h = "HTTP/1.1 \(status) \(status == 200 ? "OK" : "Error")\r\nContent-Type: \(type)\r\n"
        h += "Content-Length: \(body.count)\r\nConnection: close\r\nCache-Control: no-store\r\n"
        if let origin, Self.isGoogleSidebar(origin) {
            h += "Access-Control-Allow-Origin: \(origin)\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\n"
            h += "Access-Control-Allow-Headers: Content-Type, X-Margins-Token\r\nAccess-Control-Allow-Private-Network: true\r\n"
        }
        h += "\r\n"
        var out = Data(h.utf8)
        out.append(body)
        c.send(content: out, completion: .contentProcessed { _ in c.cancel() })
    }

    private func json(_ c: NWConnection, _ status: Int, _ obj: Any, origin: String?) {
        send(c, status, (try? JSONSerialization.data(withJSONObject: obj)) ?? Data(), type: "application/json", origin: origin)
    }

    static func isGoogleSidebar(_ origin: String) -> Bool {
        guard let host = URL(string: origin)?.host else { return false }
        return origin.hasPrefix("https://") && host.hasSuffix(".googleusercontent.com")
    }

    private func respond(_ c: NWConnection, _ r: Request) {
        let origin = r.headers["origin"]
        if r.method == "OPTIONS" { send(c, 200, Data(), type: "text/plain", origin: origin); return }
        if r.path.hasPrefix("/api/") {
            // same-origin add-in pages send no Origin (or ours); the Google sidebar must present the token
            if let origin, origin != "https://localhost:\(Self.port)" {
                guard Self.isGoogleSidebar(origin), r.headers["x-margins-token"] == token else {
                    json(c, 403, ["error": "Not paired. Copy Sidebar.html again from Margins › Citations in Word & Google Docs."], origin: origin)
                    return
                }
            }
            api(c, r, origin: origin)
            return
        }
        serveFile(c, r.path, origin: origin)
    }

    private func api(_ c: NWConnection, _ r: Request, origin: String?) {
        guard let model else { json(c, 503, ["error": "library not open"], origin: origin); return }
        switch (r.method, r.path) {
        case ("GET", "/api/status"):
            json(c, 200, ["ok": true, "papers": model.counts.all], origin: origin)
        case ("GET", "/api/search"):
            let q = (r.query["q"] ?? "").trimmingCharacters(in: .whitespaces)
            let papers: [Paper] = q.isEmpty ? [] : ((try? model.lib.search(query: q, limit: 30)) ?? []).map(\.paper)
            json(c, 200, papers.map(Self.item), origin: origin)
        case ("POST", "/api/bibliography"):
            guard let j = try? JSONSerialization.jsonObject(with: r.body) as? [String: Any], let ids = j["ids"] as? [String] else {
                json(c, 400, ["error": "expected {style, ids}"], origin: origin); return
            }
            let style: CitationStyle = ["vancouver": .vancouver, "nature": .nature][(j["style"] as? String) ?? ""] ?? .apa
            let items = ids.map { id -> [String: Any] in
                var d = model.paper(id).map(Self.item) ?? ["id": id, "missing": true]
                d["entry"] = model.citation(id, style)
                return d
            }
            json(c, 200, ["entries": items], origin: origin)
        default:
            json(c, 404, ["error": "not found"], origin: origin)
        }
    }

    static func item(_ p: Paper) -> [String: Any] {
        let fams = p.meta.authors.map { a -> String in
            if let comma = a.firstIndex(of: ",") { return String(a[..<comma]) }
            return a.split(separator: " ").last.map(String.init) ?? a
        }
        return ["id": p.id, "title": p.meta.title, "authors": p.meta.authors, "families": fams, "year": p.meta.year,
                "journal": p.meta.journal, "doi": p.meta.doi]
    }

    // MARK: - Static add-in files

    private func serveFile(_ c: NWConnection, _ path: String, origin: String?) {
        guard let root = Bundle.main.resourceURL?.appendingPathComponent("citations") else {
            send(c, 404, Data(), type: "text/plain", origin: origin); return
        }
        var rel = path.removingPercentEncoding ?? path
        if rel.hasSuffix("/") { rel += "index.html" }
        let url = root.appendingPathComponent(rel).standardizedFileURL
        guard url.path.hasPrefix(root.standardizedFileURL.path), let data = try? Data(contentsOf: url) else {
            send(c, 404, Data("not found".utf8), type: "text/plain", origin: origin); return
        }
        let types = ["html": "text/html; charset=utf-8", "js": "text/javascript; charset=utf-8", "css": "text/css",
                     "png": "image/png", "xml": "application/xml", "json": "application/json"]
        send(c, 200, data, type: types[url.pathExtension.lowercased()] ?? "application/octet-stream", origin: origin)
    }
}
