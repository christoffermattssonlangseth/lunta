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
                Text("Cite papers from your Lunta library while you write. Citations update when you change style, and the reference list is built for you (APA, Vancouver, Nature and nine more styles, or any of ~10,000 journal styles).")
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
                        Text("Lunta has to be open to search and cite. Citations already in a document stay as they are without it, and Update All works from the copy of the cited papers each document keeps.").font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
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
                        HStack(spacing: 8) {
                            Text("Pairing key:").foregroundStyle(.secondary)
                            Text(CitationServer.token).font(.body.monospaced()).textSelection(.enabled)
                            Button(copied == "key" ? "Copied" : "Copy") { copy(CitationServer.token, "key") }
                        }
                        Text("A co-author with Lunta on their own Mac enters their key when the sidebar asks for it. After updating Lunta, copy both files again if Lunta’s release notes say the add-in changed.")
                            .font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                    }
                    .padding(6)
                }

                if let problem = CitationServer.problem {
                    Label(problem, systemImage: "exclamationmark.triangle").foregroundStyle(.orange)
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
            wordInstalled = CitationsSetup.wordManifestInstalled()
            message = "Installed. Quit Word (⌘Q) and open it again, then Home › Add-ins › Lunta Citations (once): Cite stays on the Home tab."
        } catch {
            // macOS guards other apps' folders (Word's included): copying there needs the user's
            // say-so. Choosing the folder in an open panel is that say-so.
            saveWordManifestWithPanel()
        }
    }

    /// macOS guards Word's folder: the user picks it in an open panel (that choice is the
    /// permission), then Lunta makes the "wef" folder in it and puts the add-in there.
    private func saveWordManifestWithPanel() {
        let panel = NSOpenPanel()
        panel.title = "Install Lunta Citations in Word"
        panel.message = "macOS needs your OK to add Lunta to Word. This is Word's folder: just click Install."
        panel.prompt = "Install"
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = false
        panel.canCreateDirectories = false
        panel.directoryURL = CitationsSetup.wordWef.deletingLastPathComponent()   // Word's Documents
        guard panel.runModal() == .OK, var dir = panel.url else {
            message = "Not installed. Click Install in Word again, and Install in the window that opens."
            return
        }
        if dir.lastPathComponent != "wef" { dir = dir.appendingPathComponent("wef", isDirectory: true) }
        do {
            try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
            let data = try Data(contentsOf: CitationsSetup.resources.appendingPathComponent("word/manifest.xml"))
            try data.write(to: dir.appendingPathComponent("margins-citations.xml"), options: .atomic)
            wordInstalled = true
            message = dir.path.contains("com.microsoft.Word")
                ? "Installed. Quit Word (⌘Q) and open it again, then Home › Add-ins › Lunta Citations (once): Cite stays on the Home tab."
                : "Installed into another folder than Word's, so Word won't find it. Click Install in Word again and choose Word's Documents folder."
        } catch {
            message = "Couldn't install into Word: \(error.readable)"
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
        // (creating the folder can succeed while writing into it is refused: check the copy below)
        try? FileManager.default.createDirectory(at: wordWef, withIntermediateDirectories: true)
        let dest = wordWef.appendingPathComponent("margins-citations.xml")
        try? FileManager.default.removeItem(at: dest)
        try FileManager.default.copyItem(at: resources.appendingPathComponent("word/manifest.xml"), to: dest)
        guard wordManifestInstalled() else { throw CocoaError(.fileWriteNoPermission) }
    }

    /// True when this Mac trusts the localhost certificate Lunta serves (also false once it has
    /// run out, or after Lunta had to make a new certificate authority).
    static func isTrusted() -> Bool {
        CitationCertificates.evaluate(in: CitationServer.tlsDir)   // made and renewed by CitationServer
    }

    /// Adds Lunta' local CA to the login keychain as trusted for SSL (macOS asks for the password).
    static func trustCertificate() async -> Bool {
        guard FileManager.default.fileExists(atPath: caURL.path) else { return false }
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
