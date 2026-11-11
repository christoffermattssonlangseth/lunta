import SwiftUI
import AppKit

/// Lets AI agents (Claude Code, Claude Desktop, Cursor, any MCP client) work with the library through
/// the bundled `margins-mcp` server, and installs the `margins` command-line tool.
struct AgentConnectView: View {
    @State private var status: [Client: Bool] = [:]
    @State private var cliInstalled = false
    @State private var message: String?
    @State private var working: Client?

    enum Client: String, CaseIterable, Identifiable {
        case claudeCode = "Claude Code", claudeDesktop = "Claude Desktop", cursor = "Cursor"
        var id: String { rawValue }
        var icon: String {
            switch self { case .claudeCode: return "terminal"; case .claudeDesktop: return "bubble.left.and.text.bubble.right"; case .cursor: return "cursorarrow.rays" }
        }
        var configFile: URL? {
            let home = FileManager.default.homeDirectoryForCurrentUser
            switch self {
            case .claudeCode: return nil
            case .claudeDesktop: return home.appendingPathComponent("Library/Application Support/Claude/claude_desktop_config.json")
            case .cursor: return home.appendingPathComponent(".cursor/mcp.json")
            }
        }
    }

    static var server: URL { Bundle.main.executableURL!.deletingLastPathComponent().appendingPathComponent("margins-mcp") }
    static var cliLink: URL { FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".local/bin/margins") }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack(spacing: 12) {
                Image(systemName: "sparkles.rectangle.stack").font(.system(size: 30)).foregroundStyle(Color.marginsTeal)
                VStack(alignment: .leading, spacing: 3) {
                    Text("Connect AI Agents").font(.title2.weight(.semibold))
                    Text("Give an AI agent access to your papers: it can search and read them, cite them, add new papers, and tag, collect or annotate them. Its changes show up here and on your iPhone.")
                        .font(.callout).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                }
            }
            VStack(spacing: 0) {
                ForEach(Client.allCases) { c in
                    row(c)
                    Divider()
                }
                cliRow
            }
            .background(.background.secondary, in: RoundedRectangle(cornerRadius: 10))

            DisclosureGroup("Other apps (any MCP client)") {
                VStack(alignment: .leading, spacing: 8) {
                    Text("Add this server to the app's MCP settings:").font(.callout).foregroundStyle(.secondary)
                    Text(Self.configJSON).font(.system(.caption, design: .monospaced)).textSelection(.enabled)
                        .padding(8).frame(maxWidth: .infinity, alignment: .leading)
                        .background(.background.secondary, in: RoundedRectangle(cornerRadius: 6))
                    Button("Copy Configuration") { copyToPasteboard(Self.configJSON) }
                }
                .padding(.top, 6)
            }
            if let message { Text(message).font(.callout).foregroundStyle(Color.marginsCopper) }
            Text("Agents run on this Mac and reach the library only through Margins' server. What they read is sent to the AI service you use, as with anything you share with it.")
                .font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
        }
        .padding(22)
        .frame(width: 560)
        .task { await refresh() }
    }

    private func row(_ c: Client) -> some View {
        HStack(spacing: 12) {
            Image(systemName: c.icon).frame(width: 22).foregroundStyle(Color.marginsTeal)
            Text(c.rawValue).font(.body.weight(.medium))
            Spacer()
            if working == c {
                ProgressView().controlSize(.small)
            } else if status[c] == true {
                Label("Connected", systemImage: "checkmark.circle.fill").foregroundStyle(Color.marginsTeal)
                Button("Remove") { Task { await set(c, on: false) } }.buttonStyle(.borderless)
            } else {
                Button("Connect") { Task { await set(c, on: true) } }.buttonStyle(.borderedProminent).tint(.marginsTeal)
            }
        }
        .padding(.horizontal, 14).padding(.vertical, 10)
    }

    private var cliRow: some View {
        HStack(spacing: 12) {
            Image(systemName: "chevron.left.forwardslash.chevron.right").frame(width: 22).foregroundStyle(Color.marginsTeal)
            VStack(alignment: .leading, spacing: 2) {
                Text("Command-line tool").font(.body.weight(.medium))
                Text("margins search … · margins read <id> … · margins cite <id>").font(.caption.monospaced()).foregroundStyle(.secondary)
            }
            Spacer()
            if cliInstalled {
                Label("Installed", systemImage: "checkmark.circle.fill").foregroundStyle(Color.marginsTeal)
            } else {
                Button("Install") { installCLI() }.buttonStyle(.borderedProminent).tint(.marginsTeal)
            }
        }
        .padding(.horizontal, 14).padding(.vertical, 10)
    }

    static var configJSON: String {
        """
        {
          "mcpServers": {
            "margins": { "command": "\(server.path)" }
          }
        }
        """
    }

    // MARK: actions

    private func refresh() async {
        for c in Client.allCases { status[c] = await isConnected(c) }
        cliInstalled = (try? FileManager.default.destinationOfSymbolicLink(atPath: Self.cliLink.path)) == Self.server.path
    }

    private func isConnected(_ c: Client) async -> Bool {
        if c == .claudeCode {
            guard let claude = Self.claudeCLI else { return false }
            return (try? await Self.run(claude, ["mcp", "get", "margins"])) != nil
        }
        guard let f = c.configFile, let d = try? Data(contentsOf: f),
              let j = try? JSONSerialization.jsonObject(with: d) as? [String: Any],
              let servers = j["mcpServers"] as? [String: Any] else { return false }
        return servers["margins"] != nil
    }

    private func set(_ c: Client, on: Bool) async {
        working = c
        defer { working = nil }
        message = nil
        do {
            if c == .claudeCode {
                guard let claude = Self.claudeCLI else {
                    message = "Claude Code's “claude” command wasn't found. Install Claude Code, then try again."
                    return
                }
                if on {
                    _ = try? await Self.run(claude, ["mcp", "remove", "--scope", "user", "margins"])
                    try await Self.run(claude, ["mcp", "add", "--scope", "user", "margins", "--", Self.server.path])
                } else {
                    try await Self.run(claude, ["mcp", "remove", "--scope", "user", "margins"])
                }
            } else if let f = c.configFile {
                var j: [String: Any] = [:]
                if let d = try? Data(contentsOf: f) {
                    guard let parsed = try? JSONSerialization.jsonObject(with: d) as? [String: Any] else {
                        message = "\(f.lastPathComponent) isn't valid JSON; not changing it."
                        return
                    }
                    j = parsed
                }
                var servers = j["mcpServers"] as? [String: Any] ?? [:]
                servers["margins"] = on ? ["command": Self.server.path] : nil
                j["mcpServers"] = servers
                try FileManager.default.createDirectory(at: f.deletingLastPathComponent(), withIntermediateDirectories: true)
                if FileManager.default.fileExists(atPath: f.path) {
                    try? FileManager.default.removeItem(at: f.appendingPathExtension("margins-backup"))
                    try? FileManager.default.copyItem(at: f, to: f.appendingPathExtension("margins-backup"))
                }
                try JSONSerialization.data(withJSONObject: j, options: [.prettyPrinted, .sortedKeys]).write(to: f, options: .atomic)
                if on { message = "Restart \(c.rawValue) to load Margins." }
            }
        } catch {
            message = error.localizedDescription
        }
        await refresh()
    }

    private func installCLI() {
        do {
            let link = Self.cliLink
            try FileManager.default.createDirectory(at: link.deletingLastPathComponent(), withIntermediateDirectories: true)
            try? FileManager.default.removeItem(at: link)
            try FileManager.default.createSymbolicLink(at: link, withDestinationURL: Self.server)
            cliInstalled = true
            message = "Installed as ~/.local/bin/margins. Try: margins search lactate"
        } catch {
            message = error.localizedDescription
        }
    }

    // MARK: helpers

    static var claudeCLI: String? {
        let home = FileManager.default.homeDirectoryForCurrentUser.path
        return ["\(home)/.local/bin/claude", "\(home)/.claude/local/claude", "/opt/homebrew/bin/claude", "/usr/local/bin/claude"]
            .first(where: FileManager.default.isExecutableFile)
    }

    @discardableResult
    static func run(_ exe: String, _ args: [String]) async throws -> String {
        try await Task.detached {
            let p = Process()
            p.executableURL = URL(fileURLWithPath: exe)
            p.arguments = args
            var env = ProcessInfo.processInfo.environment
            env["PATH"] = "\(FileManager.default.homeDirectoryForCurrentUser.path)/.local/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin"
            p.environment = env
            let out = Pipe(), err = Pipe()
            p.standardOutput = out
            p.standardError = err
            try p.run()
            p.waitUntilExit()
            let o = String(decoding: out.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
            guard p.terminationStatus == 0 else {
                let e = String(decoding: err.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
                throw NSError(domain: "Margins", code: Int(p.terminationStatus), userInfo: [NSLocalizedDescriptionKey: e.isEmpty ? o : e])
            }
            return o
        }.value
    }
}
