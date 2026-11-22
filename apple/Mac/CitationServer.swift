import Foundation
import Network
import Security
import LuntaCore

/// HTTPS server on https://localhost:47481 for the citation add-ins (Word desktop + web, Google Docs).
///
/// Serves the add-in pages from the app bundle (`citations/…`) and the JSON API in `CitationAPI`
/// (LuntaCore), which decides who may ask and what to answer; this class owns the socket, TLS
/// and the certificates (`CitationCertificates`, renewed before they run out).
@MainActor
final class CitationServer {
    static let port = CitationAPI.port
    private var listener: NWListener?
    private weak var model: LibraryModel?
    private var api: CitationAPI?
    private var renewal: Timer?
    private var restarts = 0
    let token: String

    /// Why the add-ins can't connect, for the Citations window (nil when serving).
    static var problem: String?

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
        guard let model else { return }
        api = CitationAPI(library: model.lib, cacheDir: Self.cslCache, token: token)
        api?.paperAdded = Self.paperAdded(model)
        // Check Citations: the on-device model (never Private Cloud Compute), else the passages alone
        api?.judge = ClaimModel.blocking()
        api?.openPaper = { id, find in
            Task { @MainActor in
                if !find.isEmpty { HighlightJump.request(paper: id, find: find) }
                NotificationCenter.default.post(name: .luntaOpenPaper, object: id)
            }
        }
        listen()
        // the certificate is renewed a month before it runs out; the app may run that long
        renewal = Timer.scheduledTimer(withTimeInterval: 24 * 3600, repeats: true) { [weak self] _ in
            Task { @MainActor in self?.renewIfNeeded() }
        }
    }

    /// Resolve Citations added a paper the writer confirmed (POST /api/add): it shows in the
    /// library, and its open-access PDF and reading view are looked for, as when adding by DOI.
    nonisolated static func paperAdded(_ model: LibraryModel) -> @Sendable (String) -> Void {
        { [weak model] id in
            Task { @MainActor in
                guard let model else { return }
                model.reload()
                if model.paper(id)?.hasPdf == false { await model.findPDF(id) }
            }
        }
    }

    private func renewIfNeeded() {
        switch CitationCertificates.ensure(in: Self.tlsDir) {
        case .current: break
        case .renewed, .newAuthority:
            listener?.cancel()
            listener = nil
            listen()
        case .failed(let e):
            NSLog("Lunta: renewing the citation certificate failed: \(e)")
        }
    }

    private func listen() {
        if case .failed(let e) = CitationCertificates.ensure(in: Self.tlsDir) {
            Self.problem = "Couldn’t make the certificate for the add-ins (\(e))."
            NSLog("Lunta: \(Self.problem!)")
        }
        guard let identity = CitationCertificates.identity(in: Self.tlsDir), let secIdentity = sec_identity_create(identity) else {
            Self.problem = Self.problem ?? "The add-ins’ certificate couldn’t be read."
            NSLog("Lunta: no TLS identity; citation add-ins unavailable")
            return
        }
        let tls = NWProtocolTLS.Options()
        sec_protocol_options_set_local_identity(tls.securityProtocolOptions, secIdentity)
        sec_protocol_options_set_min_tls_protocol_version(tls.securityProtocolOptions, .TLSv12)
        let params = NWParameters(tls: tls)
        params.requiredLocalEndpoint = NWEndpoint.hostPort(host: "127.0.0.1", port: NWEndpoint.Port(rawValue: Self.port)!)
        params.allowLocalEndpointReuse = true
        let l: NWListener
        do { l = try NWListener(using: params) } catch {
            Self.problem = "Couldn’t listen on port \(Self.port): \(error.localizedDescription)"
            NSLog("Lunta: \(Self.problem!)")
            return
        }
        l.newConnectionHandler = { [weak self] c in Task { @MainActor in self?.accept(c) } }
        l.stateUpdateHandler = { [weak self] state in
            Task { @MainActor in self?.listenerChanged(state) }
        }
        l.start(queue: .main)
        listener = l
    }

    private func listenerChanged(_ state: NWListener.State) {
        switch state {
        case .ready:
            Self.problem = nil
            restarts = 0
        case .failed(let error):
            // the port taken (another copy of Lunta?), or the network stack reset after sleep: try again
            Self.problem = "The add-ins can’t connect: port \(Self.port) is unavailable (\(error.localizedDescription)). Is another copy of Lunta running?"
            NSLog("Lunta: citation server failed: \(error)")
            listener?.cancel()
            listener = nil
            guard restarts < 20 else { return }
            restarts += 1
            DispatchQueue.main.asyncAfter(deadline: .now() + Double(min(restarts * 3, 60))) { [weak self] in
                if self?.listener == nil { self?.listen() }
            }
        default:
            break
        }
    }

    // MARK: - HTTP

    private func accept(_ c: NWConnection) {
        c.start(queue: .main)
        // a connection that never sends a whole request is closed after a while
        DispatchQueue.main.asyncAfter(deadline: .now() + 30) {
            if c.state != .cancelled { c.cancel() }
        }
        read(c, Data())
    }

    private func read(_ c: NWConnection, _ buffer: Data) {
        c.receive(minimumIncompleteLength: 1, maximumLength: 1 << 20) { [weak self] data, _, done, error in
            Task { @MainActor in
                guard let self else { c.cancel(); return }
                var buf = buffer
                if let data { buf.append(data) }
                switch CitationHTTP.parse(buf) {
                case .request(let req):
                    self.respond(c, req)
                case .bad(let status):
                    self.send(c, .error(status, status == 413 ? "Too large." : "Bad request."))
                case .incomplete:
                    if done || error != nil { c.cancel() } else { self.read(c, buf) }
                }
            }
        }
    }

    private func send(_ c: NWConnection, _ r: CitationResponse) {
        c.send(content: r.encoded(), completion: .contentProcessed { _ in c.cancel() })
    }

    private func respond(_ c: NWConnection, _ r: CitationRequest) {
        guard let api else { send(c, .error(503, "Lunta is starting.")); return }
        if r.path.hasPrefix("/api/") {
            // off the main thread: the first use of a journal style downloads it
            let count = model?.counts.all ?? 0
            Task.detached {
                let out = api.respond(r, paperCount: count)
                await MainActor.run { self.send(c, out) }
            }
            return
        }
        if let no = api.refusal(r) { send(c, no); return }
        serveFile(c, r.path)
    }

    /// Where journal styles are kept (next to the TLS files, on this Mac only).
    nonisolated static var cslCache: String { tlsDir.deletingLastPathComponent().path }

    // MARK: - Static add-in files

    private func serveFile(_ c: NWConnection, _ path: String) {
        guard let root = Bundle.main.resourceURL?.appendingPathComponent("citations") else {
            send(c, .error(404, "Not found.")); return
        }
        var rel = path.removingPercentEncoding ?? path
        if rel.hasSuffix("/") { rel += "index.html" }
        let url = root.appendingPathComponent(rel).standardizedFileURL
        let base = root.standardizedFileURL.path + "/"
        guard url.path.hasPrefix(base), let data = try? Data(contentsOf: url) else {
            send(c, .error(404, "Not found.")); return
        }
        let types = ["html": "text/html; charset=utf-8", "js": "text/javascript; charset=utf-8", "css": "text/css; charset=utf-8",
                     "png": "image/png", "xml": "application/xml", "json": "application/json"]
        send(c, CitationResponse(status: 200, body: data, contentType: types[url.pathExtension.lowercased()] ?? "application/octet-stream"))
    }
}
