import SwiftUI
import Security
import LuntaCore

/// Window › Citations in Word & Google Docs…: sets up the add-ins that cite from the library.
struct CitationsView: View {
    @State private var trusted = CitationsSetup.isTrusted()
    @State private var wordInstalled = CitationsSetup.wordManifestInstalled()
    @State private var message: String?
    @State private var copied: String?

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                Text("Cite papers from your Lunta library while you write. Citations update when you change style, and the reference list is built for you (APA, Vancouver or Nature).")
                    .foregroundStyle(.secondary)

                GroupBox {
                    VStack(alignment: .leading, spacing: 10) {
                        Label("Microsoft Word", systemImage: "doc.richtext").font(.headline)
                        step(1, "Let Word talk to Lunta securely (adds Lunta' own certificate, for this Mac only; macOS asks for your password).",
                             done: trusted) {
                            Button(trusted ? "Done" : "Trust Certificate…") { trust() }.disabled(trusted)
                        }
                        step(2, "Add the Lunta add-in to Word.", done: wordInstalled) {
                            Button(wordInstalled ? "Reinstall" : "Install in Word") { installWord() }
                        }
                        step(3, "Quit and reopen Word. Click **Cite** on the Home tab (or Insert › Add-ins › My Add-ins › Lunta Citations). Search, pick papers, Insert Citation. Insert Bibliography where the references go.", done: false) { EmptyView() }
                        Text("Lunta has to be open while you cite.").font(.caption).foregroundStyle(.secondary)
                    }
                    .padding(6)
                }

                GroupBox {
                    VStack(alignment: .leading, spacing: 10) {
                        Label("Google Docs", systemImage: "doc.text").font(.headline)
                        step(1, "Do step 1 above (Chrome and Safari use the same certificate).", done: trusted) { EmptyView() }
                        step(2, "In your document: Extensions › Apps Script. Replace the code in Code.gs with this:", done: false) {
                            Button(copied == "code" ? "Copied" : "Copy Code.gs") { copy(CitationsSetup.googleCode(), "code") }
                        }
                        step(3, "Click + › HTML, name it **Sidebar**, and replace its contents with this (it includes your pairing key):", done: false) {
                            Button(copied == "html" ? "Copied" : "Copy Sidebar.html") { copy(CitationsSetup.googleSidebar(), "html") }
                        }
                        step(4, "Save, reload the document, then use the new **Lunta › Cite…** menu. Allow the script when Google asks; if Chrome asks to let the page reach devices on your network, allow it.", done: false) { EmptyView() }
                    }
                    .padding(6)
                }

                if let message { Text(message).font(.callout).foregroundStyle(.secondary) }
            }
            .padding(20)
        }
        .frame(minWidth: 560, minHeight: 560)
    }

    private func step<V: View>(_ n: Int, _ text: LocalizedStringKey, done: Bool, @ViewBuilder action: () -> V) -> some View {
        HStack(alignment: .top, spacing: 10) {
            Image(systemName: done ? "checkmark.circle.fill" : "\(n).circle")
                .foregroundStyle(done ? Color.luntaTeal : .secondary).font(.title3)
            Text(text).fixedSize(horizontal: false, vertical: true)
            Spacer(minLength: 8)
            action()
        }
    }

    private func copy(_ s: String, _ key: String) {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(s, forType: .string)
        copied = key
    }

    private func trust() {
        Task {
            let ok = await CitationsSetup.trustCertificate()
            trusted = CitationsSetup.isTrusted()
            message = ok && trusted ? "Certificate trusted." : "The certificate wasn't trusted. Try again, and enter your Mac password when asked."
        }
    }

    private func installWord() {
        do {
            try CitationsSetup.installWordManifest()
            wordInstalled = true
            message = "Installed. Quit and reopen Word."
        } catch {
            message = "Couldn't install into Word: \(error.readable). If macOS asked whether Lunta may access other apps' data, allow it and try again."
        }
    }
}

enum CitationsSetup {
    static var resources: URL { Bundle.main.resourceURL!.appendingPathComponent("citations") }
    static var caURL: URL { CitationServer.tlsDir.appendingPathComponent("ca.pem") }

    static var wordWef: URL {
        FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Library/Containers/com.microsoft.Word/Data/Documents/wef", isDirectory: true)
    }

    static func wordManifestInstalled() -> Bool {
        FileManager.default.fileExists(atPath: wordWef.appendingPathComponent("margins-citations.xml").path)
    }

    static func installWordManifest() throws {
        try FileManager.default.createDirectory(at: wordWef, withIntermediateDirectories: true)
        let dest = wordWef.appendingPathComponent("margins-citations.xml")
        try? FileManager.default.removeItem(at: dest)
        try FileManager.default.copyItem(at: resources.appendingPathComponent("word/manifest.xml"), to: dest)
    }

    /// True when this Mac trusts the localhost certificate Lunta serves.
    static func isTrusted() -> Bool {
        guard CitationServer.ensureCertificates(),
              let pem = try? String(contentsOf: CitationServer.tlsDir.appendingPathComponent("server.pem"), encoding: .utf8),
              let der = Data(base64Encoded: pem.components(separatedBy: "\n").filter { !$0.hasPrefix("-----") }.joined()),
              let cert = SecCertificateCreateWithData(nil, der as CFData) else { return false }
        var trust: SecTrust?
        guard SecTrustCreateWithCertificates(cert, SecPolicyCreateSSL(true, "localhost" as CFString), &trust) == errSecSuccess, let trust else { return false }
        return SecTrustEvaluateWithError(trust, nil)
    }

    /// Adds Lunta' local CA to the login keychain as trusted for SSL (macOS asks for the password).
    static func trustCertificate() async -> Bool {
        guard CitationServer.ensureCertificates() else { return false }
        return await Task.detached {
            let p = Process()
            p.executableURL = URL(fileURLWithPath: "/usr/bin/security")
            let keychain = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Library/Keychains/login.keychain-db").path
            p.arguments = ["add-trusted-cert", "-r", "trustRoot", "-p", "ssl", "-k", keychain, caURL.path]
            do { try p.run(); p.waitUntilExit() } catch { return false }
            return p.terminationStatus == 0
        }.value
    }

    static func googleCode() -> String {
        (try? String(contentsOf: resources.appendingPathComponent("gdocs/Code.gs"), encoding: .utf8)) ?? ""
    }

    static func googleSidebar() -> String {
        let html = (try? String(contentsOf: resources.appendingPathComponent("gdocs/Sidebar.html"), encoding: .utf8)) ?? ""
        let common = (try? String(contentsOf: resources.appendingPathComponent("common.js"), encoding: .utf8)) ?? ""
        let css = (try? String(contentsOf: resources.appendingPathComponent("panel.css"), encoding: .utf8)) ?? ""
        return html.replacingOccurrences(of: "{{COMMON}}", with: common)
            .replacingOccurrences(of: "{{CSS}}", with: css)
            .replacingOccurrences(of: "{{TOKEN}}", with: CitationServer.token)
    }
}
