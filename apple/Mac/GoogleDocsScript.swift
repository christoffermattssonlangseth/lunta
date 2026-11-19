import Foundation
import JavaScriptCore

// Citing in Google Docs from the browser extension in Safari: Lunta for Mac does the Google part.
// Safari has no Google sign-in for extensions, so the extension passes the panel's requests to the
// app (POST /gdocs on the extension endpoint, ExtensionServer); the app is signed in to Google
// (GoogleDocsAuth: an OAuth client for installed apps, the token in the Keychain) and works the
// document out with the extension's own code (flow.js and what it uses, through gdocs/app-host.js),
// run here in JavaScriptCore: the same citations as in Chrome and Firefox, from one implementation.
//
// This file needs only Foundation and JavaScriptCore, so it is tested on its own:
// apple/citations-tests/gdocs-safari-swift.test.js compiles it with a test driver and runs the
// requests against a stand-in for the Docs API.

/// An error with a kind the panel acts on ("auth": show Connect; "setup", "access", …).
struct GoogleDocsFailure: Error {
    let message: String
    let kind: String
}

/// The app's Google account for Google Docs (GoogleDocsAuth; a stand-in in tests).
@MainActor
protocol GoogleDocsAccount: AnyObject {
    /// A Google OAuth client is set in this build.
    var configured: Bool { get }
    var connected: Bool { get }
    /// Signing in (the browser shows Google's page).
    var connecting: Bool { get }
    /// Why the last sign-in didn't finish.
    var lastError: String? { get }
    /// Starts signing in (returns at once; `connecting` until done).
    func connect(selectAccount: Bool) throws
    func disconnect() async
    /// A Docs API request with the account's token (`url` on https://docs.googleapis.com/v1/documents/):
    /// status and body. Throws GoogleDocsFailure("auth") without a usable token; other errors: no network.
    func send(method: String, url: String, body: Data?) async throws -> (Int, Data)
}

/// The panel's Google Docs requests for Safari ({op, docId, …}, as background-gdocs.js gets them).
@MainActor
final class GoogleDocsSafari {
    static let setupMessage = "Citing in Google Docs from Safari isn’t set up yet in this version of Lunta for Mac. Cite with the Apps Script sidebar meanwhile: Lunta › Citations in Word & Google Docs…, or use Lunta’s extension in Chrome, Edge or Firefox."

    let account: GoogleDocsAccount
    private let scripts: () -> URL?
    private let lunta: @MainActor (String, Data?) async -> (Int, Data)
    private let papers: @MainActor () -> Int
    private var script: GoogleDocsScript?
    /// The citation API's version, for the panel (3: Resolve Citations); set by the app.
    var luntaAPI = 0

    /// `scripts`: the folder with the extension's gdocs/*.js; `lunta(path, body)`: Lunta's citation
    /// API (/bibliography, /render); `papers`: how many papers the library has.
    init(account: GoogleDocsAccount, scripts: @escaping () -> URL?, lunta: @escaping @MainActor (String, Data?) async -> (Int, Data),
         papers: @escaping @MainActor () -> Int) {
        self.account = account
        self.scripts = scripts
        self.lunta = lunta
        self.papers = papers
    }

    func answer(_ msg: [String: Any]) async -> [String: Any] {
        let op = msg["op"] as? String ?? ""
        switch op {
        case "status":
            var out: [String: Any] = ["viaApp": true, "configured": account.configured, "supported": true,
                                      "connected": account.configured && account.connected, "connecting": account.connecting,
                                      "lunta": ["papers": papers(), "api": luntaAPI]]
            if let e = account.lastError { out["authError"] = e }
            return out
        case "connect":
            guard account.configured else { return ["error": Self.setupMessage, "kind": "setup"] }
            do {
                try account.connect(selectAccount: msg["selectAccount"] as? Bool ?? false)
                return ["started": true]
            } catch {
                return Self.failure(error)
            }
        case "disconnect":
            await account.disconnect()
            return ["connected": false]
        case "state", "update", "prepareInsert", "finishInsert", "details", "editCitation", "scanPlaceholders", "writePlaceholder":
            guard account.configured else { return ["error": Self.setupMessage, "kind": "setup"] }
            guard account.connected else { return ["error": "Connect your Google account to cite in this document.", "kind": "auth"] }
            do {
                let s = try loadScript()
                return await s.handle(msg)
            } catch {
                return Self.failure(error)
            }
        default:
            return ["error": "unknown request", "kind": "http"]
        }
    }

    private func loadScript() throws -> GoogleDocsScript {
        if let script { return script }
        guard let dir = scripts() else {
            throw GoogleDocsFailure(message: "Lunta’s Safari extension wasn’t found in the app, so it can’t cite in Google Docs. Reinstall Lunta.", kind: "http")
        }
        let account = self.account, lunta = self.lunta
        let s = try GoogleDocsScript(folder: dir, docs: { m, u, b in try await account.send(method: m, url: u, body: b) }, lunta: lunta)
        script = s
        return s
    }

    static func failure(_ error: Error) -> [String: Any] {
        if let f = error as? GoogleDocsFailure { return ["error": f.message, "kind": f.kind] }
        return ["error": error.localizedDescription, "kind": ""]
    }
}

/// The extension's Google Docs code (citations-common.js, docs-model.js, flow.js, app-host.js) in a
/// JavaScriptCore context, with what JavaScriptCore lacks done here: the Docs API (with the app's
/// token), Lunta's citation API, gzip and waiting.
@MainActor
final class GoogleDocsScript {
    typealias Docs = @MainActor (_ method: String, _ url: String, _ body: Data?) async throws -> (Int, Data)
    typealias Lunta = @MainActor (_ path: String, _ body: Data?) async -> (Int, Data)

    static let files = ["citations-common.js", "docs-model.js", "flow.js", "app-host.js"]
    static let docsPrefix = "https://docs.googleapis.com/v1/documents/"

    private let context: JSContext
    private let docs: Docs
    private let lunta: Lunta
    private var waiting: [String: CheckedContinuation<[String: Any], Never>] = [:]
    private var next = 0
    private var lastException: String?

    init(folder: URL, docs: @escaping Docs, lunta: @escaping Lunta) throws {
        guard let ctx = JSContext() else { throw GoogleDocsFailure(message: "JavaScriptCore didn’t start.", kind: "http") }
        context = ctx
        self.docs = docs
        self.lunta = lunta
        ctx.name = "Lunta Google Docs"
        ctx.exceptionHandler = { [weak self] _, e in
            let text = e?.toString() ?? "error"
            let line = e?.objectForKeyedSubscript("line")?.toString() ?? ""
            MainActor.assumeIsolated { self?.lastException = text + (line.isEmpty ? "" : " (line \(line))") }
        }
        let native = JSValue(newObjectIn: ctx)!
        let call: @convention(block) (String, String, String) -> Void = { [weak self] id, name, args in
            MainActor.assumeIsolated { self?.perform(id, name, args) }
        }
        let done: @convention(block) (String, String) -> Void = { [weak self] id, json in
            MainActor.assumeIsolated { self?.finished(id, json) }
        }
        native.setObject(call, forKeyedSubscript: "call" as NSString)
        native.setObject(done, forKeyedSubscript: "done" as NSString)
        ctx.setObject(native, forKeyedSubscript: "LuntaNative" as NSString)
        for f in Self.files {
            let url = folder.appendingPathComponent(f)
            guard let src = try? String(contentsOf: url, encoding: .utf8) else {
                throw GoogleDocsFailure(message: "Lunta’s Google Docs code is missing (\(f)). Reinstall Lunta.", kind: "http")
            }
            lastException = nil
            ctx.evaluateScript(src, withSourceURL: url)
            if let e = lastException {
                throw GoogleDocsFailure(message: "Lunta’s Google Docs code didn’t load (\(f): \(e)).", kind: "http")
            }
        }
        guard ctx.objectForKeyedSubscript("LuntaAppHost")?.isObject == true else {
            throw GoogleDocsFailure(message: "Lunta’s Google Docs code didn’t load.", kind: "http")
        }
    }

    /// One document request ({op, docId, …}); the answer, or {error, kind}.
    func handle(_ msg: [String: Any]) async -> [String: Any] {
        guard let data = try? JSONSerialization.data(withJSONObject: msg), let json = String(data: data, encoding: .utf8) else {
            return ["error": "bad request", "kind": "http"]
        }
        next += 1
        let id = "r\(next)"
        return await withCheckedContinuation { c in
            waiting[id] = c
            lastException = nil
            context.objectForKeyedSubscript("LuntaAppHost").invokeMethod("handle", withArguments: [id, json])
            if let e = lastException, let c = waiting.removeValue(forKey: id) {
                c.resume(returning: ["error": "Lunta’s Google Docs code failed: \(e)", "kind": "http"])
            }
        }
    }

    private func finished(_ id: String, _ json: String) {
        guard let c = waiting.removeValue(forKey: id) else { return }
        let obj = (try? JSONSerialization.jsonObject(with: Data(json.utf8))) as? [String: Any]
        c.resume(returning: obj ?? ["error": "Lunta’s Google Docs code answered nothing.", "kind": "http"])
    }

    private func settle(_ id: String, ok: Bool, _ payload: String) {
        context.objectForKeyedSubscript("LuntaAppHost").invokeMethod("settle", withArguments: [id, ok, payload])
    }

    private func settle(_ id: String, value: Any) {
        let data = (try? JSONSerialization.data(withJSONObject: value, options: [.fragmentsAllowed])) ?? Data("null".utf8)
        settle(id, ok: true, String(decoding: data, as: UTF8.self))
    }

    private func settle(_ id: String, error: Error) {
        let f = error as? GoogleDocsFailure
        let obj: [String: Any] = ["message": f?.message ?? error.localizedDescription, "kind": f?.kind ?? ""]
        let data = (try? JSONSerialization.data(withJSONObject: obj)) ?? Data("{}".utf8)
        settle(id, ok: false, String(decoding: data, as: UTF8.self))
    }

    /// `{"status": s, "json": <the answer as it came, if it is JSON>}`, without decoding and encoding a large document again.
    private static func answer(_ status: Int, _ body: Data) -> String {
        let json = (try? JSONSerialization.jsonObject(with: body, options: [.fragmentsAllowed])) != nil ? String(decoding: body, as: UTF8.self) : "{}"
        return "{\"status\":\(status),\"json\":\(json)}"
    }

    private func perform(_ id: String, _ name: String, _ argsJSON: String) {
        let args = try? JSONSerialization.jsonObject(with: Data(argsJSON.utf8), options: [.fragmentsAllowed])
        Task { @MainActor in
            do {
                switch name {
                case "docs":
                    let a = args as? [String: Any] ?? [:]
                    let url = a["url"] as? String ?? ""
                    guard url.hasPrefix(Self.docsPrefix) else { throw GoogleDocsFailure(message: "not a Docs API request", kind: "http") }
                    let body = (a["body"]).flatMap { $0 is NSNull ? nil : try? JSONSerialization.data(withJSONObject: $0) }
                    let (status, data) = try await docs(a["method"] as? String ?? "GET", url, body)
                    settle(id, ok: true, Self.answer(status, data))
                case "lunta":
                    let a = args as? [String: Any] ?? [:]
                    let body = (a["body"]).flatMap { $0 is NSNull ? nil : try? JSONSerialization.data(withJSONObject: $0) }
                    let (status, data) = await lunta(a["path"] as? String ?? "", body)
                    settle(id, ok: true, Self.answer(status, data))
                case "gzip":
                    settle(id, value: try Gzip.compress(Data((args as? String ?? "").utf8)).base64EncodedString())
                case "gunzip":
                    guard let d = Data(base64Encoded: args as? String ?? "") else { throw GoogleDocsFailure(message: "not base64", kind: "") }
                    settle(id, value: String(decoding: try Gzip.decompress(d), as: UTF8.self))
                case "sleep":
                    let ms = min(max((args as? Double) ?? 0, 0), 10_000)
                    try? await Task.sleep(nanoseconds: UInt64(ms * 1_000_000))
                    settle(id, value: NSNull())
                default:
                    throw GoogleDocsFailure(message: "unknown native request \(name)", kind: "http")
                }
            } catch {
                settle(id, error: error)
            }
        }
    }
}

/// gzip (RFC 1952) over the system's raw DEFLATE, as Apps Script's Utilities and the browsers'
/// CompressionStream write it (the document's store of cited papers).
enum Gzip {
    struct Bad: Error {}

    private static let table: [UInt32] = (0..<256).map { n -> UInt32 in
        var c = UInt32(n)
        for _ in 0..<8 { c = (c & 1) != 0 ? 0xEDB8_8320 ^ (c >> 1) : c >> 1 }
        return c
    }

    static func crc32(_ d: Data) -> UInt32 {
        var c: UInt32 = 0xFFFF_FFFF
        for b in d { c = table[Int((c ^ UInt32(b)) & 0xFF)] ^ (c >> 8) }
        return c ^ 0xFFFF_FFFF
    }

    private static func le32(_ v: UInt32) -> [UInt8] { [UInt8(v & 0xFF), UInt8((v >> 8) & 0xFF), UInt8((v >> 16) & 0xFF), UInt8(v >> 24)] }

    static func compress(_ d: Data) throws -> Data {
        // NSData's .zlib is raw DEFLATE (RFC 1951), no zlib header
        let deflated = try (d as NSData).compressed(using: .zlib) as Data
        var out = Data([0x1F, 0x8B, 8, 0, 0, 0, 0, 0, 0, 0xFF])
        out.append(deflated)
        out.append(contentsOf: le32(crc32(d)))
        out.append(contentsOf: le32(UInt32(truncatingIfNeeded: d.count)))
        return out
    }

    static func decompress(_ data: Data) throws -> Data {
        let b = [UInt8](data)
        guard b.count >= 18, b[0] == 0x1F, b[1] == 0x8B, b[2] == 8 else { throw Bad() }
        let flags = b[3]
        var i = 10
        if flags & 4 != 0 {   // FEXTRA
            guard i + 2 <= b.count else { throw Bad() }
            i += 2 + Int(b[i]) + Int(b[i + 1]) << 8
        }
        if flags & 8 != 0 { while i < b.count, b[i] != 0 { i += 1 }; i += 1 }    // FNAME
        if flags & 16 != 0 { while i < b.count, b[i] != 0 { i += 1 }; i += 1 }   // FCOMMENT
        if flags & 2 != 0 { i += 2 }                                               // FHCRC
        guard i <= b.count - 8 else { throw Bad() }
        let body = Data(b[i..<(b.count - 8)])
        let out = try (body as NSData).decompressed(using: .zlib) as Data
        let crc = UInt32(b[b.count - 8]) | UInt32(b[b.count - 7]) << 8 | UInt32(b[b.count - 6]) << 16 | UInt32(b[b.count - 5]) << 24
        guard crc == crc32(out) else { throw Bad() }
        return out
    }
}
