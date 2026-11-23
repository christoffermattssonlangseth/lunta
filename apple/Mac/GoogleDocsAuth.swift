import AppKit
import CryptoKit
import Foundation
import Network
import Security

/// The Google OAuth client Lunta for Mac signs in with to edit Google Docs for the extension in
/// Safari: type **Desktop app** ("Lunta for Mac", in the Lunta Docs project), scope `drive.file`
/// only. Empty: not set up in this build, and the Safari panel points to the Apps Script sidebar.
/// Setting it up: docs/google-docs-extension.md, "Safari".
enum GoogleDocsClient {
    static let id = ""
    /// Google gives a Desktop client a "secret" too and wants it when tokens are fetched. It isn't
    /// confidential (it ships in the app; Google says so for installed apps); PKCE protects the sign-in.
    static let secret = ""
}

/// Lunta for Mac's sign-in to Google for Google Docs (Safari's extension can't sign in itself):
/// OAuth for installed apps (RFC 8252), Google's page in the default browser, PKCE, and a loopback
/// redirect to http://127.0.0.1:<a free port> that this app answers. The refresh token is kept in
/// the Keychain; access tokens only in memory.
///
/// Scope `drive.file`: Lunta may open only the documents the user gave it. A document is given in
/// Google's file picker shown during the sign-in ("OnePick", the picker for desktop apps:
/// `trigger_onepick=true`, `file_ids=<the document>`; no API key and no page of ours, unlike the
/// extension's picker on getlunta.app), from the panel's Connect or "Give Lunta access to this
/// document". The redirect carries `picked_file_ids`; the ids given are remembered in UserDefaults.
@MainActor
final class GoogleDocsAuth: GoogleDocsAccount {
    static let shared = GoogleDocsAuth()

    static let scope = "https://www.googleapis.com/auth/drive.file"
    private static let authURL = "https://accounts.google.com/o/oauth2/v2/auth"
    private static let tokenURL = URL(string: "https://oauth2.googleapis.com/token")!
    private static let revokeURL = URL(string: "https://oauth2.googleapis.com/revoke")!
    private static let keychainService = "com.cmlangseth.margins.google-docs"
    private static let keychainAccount = "refresh-token"
    private static let grantsKey = "GoogleDocsGrantedDocuments"

    private(set) var connecting = false
    private(set) var lastError: String?
    private var access: (token: String, expires: Date)?
    private var refreshing: Task<String, Error>?
    private var cachedRefresh: String??

    /// The sign-in under way: the loopback listener, its redirect, what Google's answer must carry.
    private struct Flow {
        let id = UUID()
        let listener: NWListener
        var redirect = ""
        let verifier: String
        let state: String
        /// the document to give Lunta in Google's picker (nil: only signing in)
        let document: String?
        let selectAccount: Bool
    }
    private var flow: Flow?

    var configured: Bool { !GoogleDocsClient.id.isEmpty }
    var connected: Bool { refreshToken != nil }

    // MARK: - Signing in

    func connect(selectAccount: Bool, document: String?) throws {
        guard configured else { throw GoogleDocsFailure(message: GoogleDocsSafari.setupMessage, kind: "setup") }
        stopFlow()
        lastError = nil
        let params = NWParameters.tcp
        params.requiredLocalEndpoint = .hostPort(host: "127.0.0.1", port: .any)
        let listener: NWListener
        do { listener = try NWListener(using: params) } catch {
            throw GoogleDocsFailure(message: "Lunta couldn’t get ready for Google’s sign-in (\(error.localizedDescription)).", kind: "auth")
        }
        let f = Flow(listener: listener, verifier: Self.randomString(48), state: Self.randomString(16), document: document, selectAccount: selectAccount)
        flow = f
        connecting = true
        listener.newConnectionHandler = { [weak self] conn in
            Task { @MainActor in
                conn.start(queue: .main)
                self?.receive(conn, flow: f.id, buffer: Data())
            }
        }
        listener.stateUpdateHandler = { [weak self] st in
            Task { @MainActor in
                guard let self, self.flow?.id == f.id else { return }
                switch st {
                case .ready:
                    guard let port = listener.port?.rawValue else { return }
                    self.openSignIn(port: port)
                case .failed(let e):
                    self.finish(f.id, error: "Lunta couldn’t get ready for Google’s sign-in (\(e.localizedDescription)).")
                default:
                    break
                }
            }
        }
        listener.start(queue: .main)
        // not finished in 5 minutes: given up
        Task { @MainActor [weak self] in
            try? await Task.sleep(nanoseconds: 300 * 1_000_000_000)
            self?.finish(f.id, error: "Google sign-in didn’t finish in time. Try again.")
        }
    }

    private func openSignIn(port: UInt16) {
        guard var f = flow else { return }
        f.redirect = "http://127.0.0.1:\(port)"
        flow = f
        let challenge = Data(SHA256.hash(data: Data(f.verifier.utf8))).base64URL
        var q = URLComponents(string: Self.authURL)!
        q.queryItems = [
            .init(name: "client_id", value: GoogleDocsClient.id),
            .init(name: "redirect_uri", value: f.redirect),
            .init(name: "response_type", value: "code"),
            .init(name: "scope", value: Self.scope),
            .init(name: "code_challenge", value: challenge),
            .init(name: "code_challenge_method", value: "S256"),
            .init(name: "state", value: f.state),
            .init(name: "access_type", value: "offline"),
            // consent: Google gives a refresh token only then (and the picker needs it)
            .init(name: "prompt", value: f.selectAccount ? "select_account consent" : "consent"),
        ]
        if let doc = f.document {
            // Google's file picker in the consent, showing only this document
            q.queryItems! += [
                .init(name: "trigger_onepick", value: "true"),
                .init(name: "file_ids", value: doc),
                .init(name: "mimetypes", value: "application/vnd.google-apps.document"),
            ]
        }
        if let url = q.url { NSWorkspace.shared.open(url) }
    }

    /// Google's redirect to the loopback address: GET /?code=…&state=… (or ?error=…).
    private func receive(_ conn: NWConnection, flow id: UUID, buffer: Data) {
        conn.receive(minimumIncompleteLength: 1, maximumLength: 64 * 1024) { [weak self] data, _, done, error in
            Task { @MainActor in
                guard let self else { return }
                var buf = buffer
                if let data { buf.append(data) }
                guard buf.range(of: Data("\r\n\r\n".utf8)) != nil else {
                    if done || error != nil || buf.count > 64 * 1024 { conn.cancel() } else { self.receive(conn, flow: id, buffer: buf) }
                    return
                }
                let line = String(decoding: buf.prefix(4096), as: UTF8.self).components(separatedBy: "\r\n").first ?? ""
                let parts = line.split(separator: " ")
                let target = parts.count >= 2 ? String(parts[1]) : "/"
                let comps = URLComponents(string: "http://127.0.0.1" + target)
                guard comps?.path == "/" || comps?.path == "" else { self.reply(conn, status: 404, page: "Not found."); return }
                var q: [String: String] = [:]
                for i in comps?.queryItems ?? [] { q[i.name] = i.value ?? "" }
                guard let f = self.flow, f.id == id, q["state"] == f.state else {
                    self.reply(conn, status: 400, page: "This sign-in isn’t the one Lunta is waiting for. Start again from Lunta’s panel in Google Docs.")
                    return
                }
                if let e = q["error"] {
                    let why = e == "access_denied" ? "You didn’t allow Lunta to reach your Google Docs documents." : "Google said: \(e)."
                    self.reply(conn, status: 200, page: why + " Nothing was changed; you can close this tab.")
                    self.finish(id, error: why)
                    return
                }
                guard let code = q["code"], !code.isEmpty else { self.reply(conn, status: 400, page: "Google’s answer had no code."); return }
                do {
                    try await self.exchange(code: code, flow: f)
                    if let doc = f.document {
                        let picked = (q["picked_file_ids"] ?? "").split(separator: ",").map(String.init)
                        guard picked.contains(doc) else {
                            let why = "Lunta is connected, but you didn’t choose this document in Google’s picker, so Lunta still can’t open it. Click Give Lunta access to this document and choose it (it is the one shown)."
                            self.reply(conn, status: 200, page: why)
                            self.finish(id, error: why)
                            return
                        }
                        self.rememberGrant(doc)
                    }
                    self.reply(conn, status: 200, page: "Lunta is connected to Google Docs\(f.document == nil ? "" : " and may now write citations into your document"). You can close this tab and go back to your document.")
                    self.finish(id, error: nil)
                } catch {
                    let why = (error as? GoogleDocsFailure)?.message ?? "Google sign-in didn’t finish (\(error.localizedDescription))."
                    self.reply(conn, status: 200, page: why)
                    self.finish(id, error: why)
                }
            }
        }
    }

    private func reply(_ conn: NWConnection, status: Int, page: String) {
        let esc = page.replacingOccurrences(of: "&", with: "&amp;").replacingOccurrences(of: "<", with: "&lt;")
        let html = "<!doctype html><meta charset=utf-8><title>Lunta</title><body style=\"font:16px -apple-system,sans-serif;margin:3em auto;max-width:34em;color:#1d1d1f\"><h2 style=\"color:#2a6478\">Lunta</h2><p>\(esc)</p>"
        let body = Data(html.utf8)
        var head = "HTTP/1.1 \(status) \(status == 200 ? "OK" : "Error")\r\nContent-Type: text/html; charset=utf-8\r\n"
        head += "Content-Length: \(body.count)\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n"
        conn.send(content: Data(head.utf8) + body, completion: .contentProcessed { _ in conn.cancel() })
    }

    private func finish(_ id: UUID, error: String?) {
        guard let f = flow, f.id == id else { return }
        f.listener.cancel()
        flow = nil
        connecting = false
        lastError = error
    }

    private func stopFlow() {
        flow?.listener.cancel()
        flow = nil
        connecting = false
    }

    private func exchange(code: String, flow f: Flow) async throws {
        let j = try await tokenRequest([
            "code": code, "client_id": GoogleDocsClient.id, "redirect_uri": f.redirect,
            "grant_type": "authorization_code", "code_verifier": f.verifier,
        ])
        guard let token = j["access_token"] as? String else {
            throw GoogleDocsFailure(message: "Google said: \(j["error_description"] as? String ?? j["error"] as? String ?? "no token").", kind: "auth")
        }
        let scopes = (j["scope"] as? String ?? "").split(separator: " ").map(String.init)
        guard scopes.contains(Self.scope) else {
            throw GoogleDocsFailure(message: "Lunta needs the permission to reach the Google Docs documents you give it to write citations (it only touches the document you cite in). Connect again and tick it.", kind: "auth")
        }
        guard let refresh = j["refresh_token"] as? String else {
            throw GoogleDocsFailure(message: "Google didn’t let Lunta stay signed in. Try again.", kind: "auth")
        }
        let old = refreshToken
        saveRefreshToken(refresh)
        access = (token, Date().addingTimeInterval(Double(j["expires_in"] as? Int ?? 3600)))
        // another account chosen: the old one's permission goes, and what it was given says nothing
        // about this one. Not when only a document was given (the same account: revoking its older
        // token could take back the whole permission, the new token too).
        if f.selectAccount {
            saveGrants([])
            if let old, old != refresh { revoke(old) }
        }
    }

    func disconnect() async {
        stopFlow()
        lastError = nil
        if let r = refreshToken { revoke(r) }
        saveRefreshToken(nil)
        saveGrants([])
        access = nil
    }

    // MARK: - The documents given to Lunta (drive.file)

    private var grants: [String] { UserDefaults.standard.stringArray(forKey: Self.grantsKey) ?? [] }
    private func saveGrants(_ ids: [String]) {
        if ids.isEmpty { UserDefaults.standard.removeObject(forKey: Self.grantsKey) } else { UserDefaults.standard.set(Array(ids.suffix(500)), forKey: Self.grantsKey) }
    }
    func isGranted(_ document: String) -> Bool { grants.contains(document) }
    func rememberGrant(_ document: String) { saveGrants(grants.filter { $0 != document } + [document]) }
    func forgetGrant(_ document: String) { saveGrants(grants.filter { $0 != document }) }

    private func revoke(_ token: String) {
        var req = URLRequest(url: Self.revokeURL)
        req.httpMethod = "POST"
        req.setValue("application/x-www-form-urlencoded", forHTTPHeaderField: "Content-Type")
        req.httpBody = Self.form(["token": token])
        URLSession.shared.dataTask(with: req).resume()
    }

    // MARK: - Tokens

    /// An access token: the one in memory, else a new one with the refresh token.
    private func token(fresh: Bool) async throws -> String {
        if !fresh, let a = access, a.expires > Date().addingTimeInterval(60) { return a.token }
        if let t = refreshing { return try await t.value }
        guard let refresh = refreshToken else { throw GoogleDocsFailure(message: "Connect your Google account to cite in this document.", kind: "auth") }
        let t = Task { @MainActor () throws -> String in
            let j = try await self.tokenRequest(["refresh_token": refresh, "client_id": GoogleDocsClient.id, "grant_type": "refresh_token"])
            guard let token = j["access_token"] as? String else {
                if (j["error"] as? String) == "invalid_grant" {
                    // the permission was taken back (Google account › Security), or it ran out
                    self.saveRefreshToken(nil)
                    throw GoogleDocsFailure(message: "Google no longer accepts Lunta’s sign-in. Connect your Google account again.", kind: "auth")
                }
                throw GoogleDocsFailure(message: "Google said: \(j["error_description"] as? String ?? j["error"] as? String ?? "no token").", kind: "auth")
            }
            self.access = (token, Date().addingTimeInterval(Double(j["expires_in"] as? Int ?? 3600)))
            return token
        }
        refreshing = t
        defer { refreshing = nil }
        return try await t.value
    }

    private func tokenRequest(_ fields: [String: String]) async throws -> [String: Any] {
        var f = fields
        if !GoogleDocsClient.secret.isEmpty { f["client_secret"] = GoogleDocsClient.secret }
        var req = URLRequest(url: Self.tokenURL)
        req.httpMethod = "POST"
        req.setValue("application/x-www-form-urlencoded", forHTTPHeaderField: "Content-Type")
        req.httpBody = Self.form(f)
        let (data, _) = try await URLSession.shared.data(for: req)
        return (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] ?? [:]
    }

    func send(method: String, url: String, body: Data?) async throws -> (Int, Data) {
        guard url.hasPrefix(GoogleDocsScript.docsPrefix), let u = URL(string: url) else {
            throw GoogleDocsFailure(message: "not a Docs API request", kind: "http")
        }
        func once(_ token: String) async throws -> (Int, Data) {
            var req = URLRequest(url: u)
            req.httpMethod = method
            req.setValue("Bearer " + token, forHTTPHeaderField: "Authorization")
            if let body { req.httpBody = body; req.setValue("application/json", forHTTPHeaderField: "Content-Type") }
            let (data, resp) = try await URLSession.shared.data(for: req)
            return ((resp as? HTTPURLResponse)?.statusCode ?? 0, data)
        }
        let r = try await once(try await token(fresh: false))
        guard r.0 == 401 else { return r }
        access = nil
        return try await once(try await token(fresh: true))
    }

    // MARK: - Keychain

    private var refreshToken: String? {
        if let c = cachedRefresh { return c }
        let q: [String: Any] = [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: Self.keychainService,
                                kSecAttrAccount as String: Self.keychainAccount, kSecReturnData as String: true,
                                kSecMatchLimit as String: kSecMatchLimitOne]
        var out: CFTypeRef?
        let v = SecItemCopyMatching(q as CFDictionary, &out) == errSecSuccess ? (out as? Data).map { String(decoding: $0, as: UTF8.self) } : nil
        cachedRefresh = .some(v)
        return v
    }

    private func saveRefreshToken(_ token: String?) {
        cachedRefresh = .some(token)
        let q: [String: Any] = [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: Self.keychainService,
                                kSecAttrAccount as String: Self.keychainAccount]
        SecItemDelete(q as CFDictionary)
        guard let token else { return }
        var add = q
        add[kSecValueData as String] = Data(token.utf8)
        add[kSecAttrLabel as String] = "Lunta: Google Docs"
        add[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlock
        SecItemAdd(add as CFDictionary, nil)
    }

    // MARK: -

    private static func randomString(_ bytes: Int) -> String {
        var b = [UInt8](repeating: 0, count: bytes)
        _ = SecRandomCopyBytes(kSecRandomDefault, bytes, &b)
        return Data(b).base64URL
    }

    private static func form(_ f: [String: String]) -> Data {
        var allowed = CharacterSet.alphanumerics
        allowed.insert(charactersIn: "-._~")
        return Data(f.map { "\($0.key)=\($0.value.addingPercentEncoding(withAllowedCharacters: allowed) ?? "")" }.joined(separator: "&").utf8)
    }
}

private extension Data {
    var base64URL: String {
        base64EncodedString().replacingOccurrences(of: "+", with: "-").replacingOccurrences(of: "/", with: "_").replacingOccurrences(of: "=", with: "")
    }
}
