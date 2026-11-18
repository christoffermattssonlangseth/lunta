import Foundation

// Test driver for apple/Mac/GoogleDocsScript.swift (compiled with it by gdocs-safari-swift.test.js):
// Lunta for Mac's answers to the Safari extension's Google Docs requests, with a stand-in Google
// account whose Docs API is the test's HTTP server (FakeDocsAPI), and Lunta's citation API there too.
//   arguments: <the extension's gdocs folder> <the test server's base URL>
//   stdin: one request per line ({op, …} as POST /gdocs gets it; {"_set": {configured, connected}}
//   changes the account); stdout: one answer per line.

@MainActor
final class StandInAccount: GoogleDocsAccount {
    var configured = true
    var connected = false
    var connecting = false
    var lastError: String?
    let base: String
    init(base: String) { self.base = base }

    func connect(selectAccount: Bool) throws { connected = true }
    func disconnect() async { connected = false }

    func send(method: String, url: String, body: Data?) async throws -> (Int, Data) {
        guard connected else { throw GoogleDocsFailure(message: "Connect your Google account to cite in this document.", kind: "auth") }
        var req = URLRequest(url: URL(string: base + "/docs/" + url.dropFirst(GoogleDocsScript.docsPrefix.count))!)
        req.httpMethod = method
        req.httpBody = body
        let (data, resp) = try await URLSession.shared.data(for: req)
        return ((resp as! HTTPURLResponse).statusCode, data)
    }
}

@main
struct Driver {
    @MainActor
    static func main() async throws {
        let a = CommandLine.arguments
        let folder = URL(fileURLWithPath: a[1], isDirectory: true), base = a[2]
        let account = StandInAccount(base: base)
        let safari = GoogleDocsSafari(account: account, scripts: { folder }, lunta: { path, body in
            var req = URLRequest(url: URL(string: base + "/lunta" + path)!)
            req.httpMethod = body == nil ? "GET" : "POST"
            req.httpBody = body
            guard let (data, resp) = try? await URLSession.shared.data(for: req) else { return (503, Data()) }
            return ((resp as! HTTPURLResponse).statusCode, data)
        }, papers: { 2 })
        for try await line in FileHandle.standardInput.bytes.lines {
            guard let msg = try JSONSerialization.jsonObject(with: Data(line.utf8)) as? [String: Any] else { continue }
            var out: [String: Any]
            if let set = msg["_set"] as? [String: Any] {
                if let c = set["configured"] as? Bool { account.configured = c }
                if let c = set["connected"] as? Bool { account.connected = c }
                out = ["ok": true]
            } else {
                out = await safari.answer(msg)
            }
            let data = try JSONSerialization.data(withJSONObject: out)
            FileHandle.standardOutput.write(data + Data("\n".utf8))
        }
    }
}
