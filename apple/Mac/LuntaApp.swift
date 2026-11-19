import SwiftUI
import LuntaCore
import UniformTypeIdentifiers

// Both LuntaCore and Paper2EpubKit define a PaperMeta; in the app it means the library's.
typealias PaperMeta = LuntaCore.PaperMeta

@main
struct LuntaApp: App {
    @StateObject private var holder = ModelHolder()

    init() {
        _ = AppUpdater.shared   // Sparkle's daily check starts with the app (Mac/Updates.swift)
    }

    var body: some Scene {
        WindowGroup("Lunta") {
            Group {
                if let model = holder.model {
                    LibraryWindow().environmentObject(model)
                        .handlesExternalEvents(preferring: ["lunta", "margins"], allowing: ["*"])
                } else {
                    Text(holder.error ?? "Opening library…").padding()
                }
            }
            .frame(minWidth: 980, minHeight: 600)
        }
        .commands {
            LuntaCommands(model: holder.model)
            PaperFindCommands()
            GoToPageCommands()
            CommandMenu("Listen") {
                Button("Play / Pause") { Narrator.active?.toggle() }.keyboardShortcut("p", modifiers: [.command, .option])
                Button("Next Paragraph") { Narrator.active?.next() }.keyboardShortcut(.rightArrow, modifiers: [.command, .option])
                Button("Previous Paragraph") { Narrator.active?.previous() }.keyboardShortcut(.leftArrow, modifiers: [.command, .option])
                Button("Next Section") { Narrator.active?.nextSection() }.keyboardShortcut(.downArrow, modifiers: [.command, .option])
                Divider()
                Button("Stop") { Narrator.active?.stop() }
                Divider()
                Picker("Natural Voice", selection: Binding(get: { NarrationRenderer.shared.voice },
                                                          set: { NarrationRenderer.shared.voice = $0 })) {
                    ForEach(NarrationRenderer.voices, id: \.id) { Text($0.name).tag($0.id) }
                }
            }
        }

        .handlesExternalEvents(matching: ["library"])

        Window("Connect AI Agents", id: "agents") { AgentConnectView().tint(.luntaTeal) }
        Window("Citations in Word & Google Docs", id: "citations") { CitationsView().tint(.luntaTeal) }
        Window("Google Drive for Android", id: "drive") { DriveBridgeView().tint(.luntaTeal) }
            .windowResizability(.contentSize)
        Window("Clean Up Library", id: "cleanup") {
            if let model = holder.model { NavigationStack { CleanupView() }.environmentObject(model).tint(.luntaTeal) }
        }
            .windowResizability(.contentSize)

        Settings { AppSettingsView() }
        WindowGroup("Reader", for: String.self) { $paperID in
            if let model = holder.model, let id = paperID {
                ReaderWindow(paperID: id).environmentObject(model)
            }
        }
        .defaultSize(width: 1000, height: 800)
    }
}

@MainActor
final class ModelHolder: ObservableObject {
    @Published var model: LibraryModel?
    @Published var error: String?
    private var server: ExtensionServer?
    private var citations: CitationServer?
    init() {
        Task { await open() }
    }

    private func open() async {
        let test = LibraryModel.testLibrary
        let (url, cloud) = test != nil ? (test!.appendingPathComponent("Library", isDirectory: true), false)
            : await Task.detached { CloudLibrary.resolve() }.value
        if cloud { await CloudLibrary.prepareLogs(url) { _ in } }
        do {
            let lib = try await Task.detached { try LibraryModel.openLibrary(root: url) }.value
            model = LibraryModel(lib: lib, root: url, inCloud: cloud)
        } catch { self.error = "Could not open the library: \(error.readable)" }
        if let model, test != nil {
            // scratch library (test hook): only the extension endpoint
            server = ExtensionServer(model: model)
            server?.start()
            #if DEBUG
            // the Safari extension's iPhone hand-off, played on the Mac (SafariInboxImport)
            let a = ProcessInfo.processInfo.arguments
            if let i = a.firstIndex(of: "-testSafariInbox"), i + 1 < a.count {
                Task { await model.runSafariInboxScript(URL(fileURLWithPath: a[i + 1])) }
            }
            #endif
            return
        }
        if let model {
            server = ExtensionServer(model: model)
            server?.start()
            citations = CitationServer(model: model)
            citations?.start()
            model.watchForOutsideChanges()
            NarrationRenderer.shared.model = model
            DriveBridge.shared.start(model)   // resumes the Google Drive copy (for Android) if connected
            if !ProcessInfo.processInfo.arguments.contains(where: { $0.hasPrefix("-ask") || $0 == "-cleanupReport" || $0 == "-forYouReport" }) {
                WatchCenter.shared.start(model)
            }
        }
        // scripting hook: `Lunta -import <file.bib|.ris> [-pdfs <folder>]`
        let args = ProcessInfo.processInfo.arguments
        if let model, let i = args.firstIndex(of: "-import"), i + 1 < args.count {
            let file = URL(fileURLWithPath: args[i + 1])
            let pdfs = args.firstIndex(of: "-pdfs").flatMap { $0 + 1 < args.count ? URL(fileURLWithPath: args[$0 + 1]) : nil }
            Task { await model.importExport(file: file, pdfFolder: pdfs) }
        } else if let i = args.firstIndex(of: "-narrate"), i + 1 < args.count {
            NarrationRenderer.shared.enqueue([args[i + 1]])
        } else if let model, let i = args.firstIndex(of: "-ask"), i + 1 < args.count {
            // test hook: `Lunta -ask "<question>" [-paper <id>]` writes the transcript to /tmp/lunta-ask.txt
            let q = args[i + 1]
            let paper = args.firstIndex(of: "-paper").flatMap { $0 + 1 < args.count ? args[$0 + 1] : nil }
            Task {
                let engine = AskEngine(scope: paper.map { .paper($0) } ?? .library)
                engine.model = model
                if let paper, let book = try? model.lib.openEpub(id: paper) {
                    engine.passages = { await PageTextExtractor.items(of: URL(fileURLWithPath: book.combined), readAccess: URL(fileURLWithPath: book.dir)) }
                }
                let t: String
                if let mode = args.firstIndex(of: "-askMode").flatMap({ $0 + 1 < args.count ? args[$0 + 1] : nil }) {
                    // q is a library search; its top hits are compared or summarized
                    let ids = ((try? model.lib.search(query: q, limit: 6)) ?? []).map(\.paper.id)
                    t = await engine.transcript {
                        switch mode {
                        case "compare": engine.compare(Array(ids.prefix(3)))
                        case "gaps": engine.gaps(ids, name: q)
                        default: engine.overview(ids, name: q)
                        }
                    }
                } else {
                    t = await engine.transcript(for: q)
                }
                try? t.write(toFile: "/tmp/lunta-ask.txt", atomically: true, encoding: .utf8)
                exit(0)
            }
        } else if let model, args.contains("-forYouReport") {
            // test hook: runs New for You with its files in /tmp/lunta-foryou (the library's own feed
            // is untouched) and writes what it found to /tmp/lunta-foryou.txt.
            // `-interests "<paragraph>"`, or the paragraph is drafted from the library.
            Task {
                let c = WatchCenter.shared
                let dir = URL(fileURLWithPath: "/tmp/lunta-foryou")
                try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
                c.folderOverride = dir
                c.model = model
                var out = ""
                var t = Date()
                var interests = args.firstIndex(of: "-interests").flatMap { $0 + 1 < args.count ? args[$0 + 1] : nil } ?? ""
                if interests.isEmpty {
                    interests = await c.draftInterests() ?? ""
                    out += "drafted in \(Int(Date().timeIntervalSince(t)))s:\n"
                }
                out += "INTERESTS: \(interests)\n\n"
                c.setInterestsQuietly(interests)
                t = Date()
                await c.run()
                out += "run: \(Int(Date().timeIntervalSince(t)))s\n\(c.lastStats)\n"
                out += "queries: \(c.state.queries.joined(separator: " | "))\nkeywords: \(c.state.keywords.joined(separator: ", "))\n\n"
                for f in c.finds {
                    let r = f.record
                    out += "- \(r.title.prefix(110))\n    \(r.journal) · \(r.date) · score \(String(format: "%.3f", r.score)) · from \(r.source.prefix(60))\n    why: \(r.reason)\n"
                }
                try? out.write(toFile: "/tmp/lunta-foryou.txt", atomically: true, encoding: .utf8)
                exit(0)
            }
        } else if let model, args.contains("-cleanupReport") {
            // test hook (read-only): what Clean Up would show
            let issues = model.detailIssues(), groups = model.duplicateGroups()
            var out = "issues: \(issues.count)\n"
            var byProblem: [String: Int] = [:]
            for i in issues { for p in i.problems { byProblem[p, default: 0] += 1 } }
            out += byProblem.map { "  \($0.key): \($0.value)" }.sorted().joined(separator: "\n") + "\n"
            for i in issues.prefix(12) { out += "  - \(i.paper.meta.title.prefix(60)) | \(i.paper.meta.authors.prefix(2).joined(separator: "; ").prefix(40)) | \(i.problems.joined(separator: ", "))\n" }
            out += "duplicates: \(groups.count)\n"
            for g in groups.prefix(12) { out += "  - [\(g.reason)] " + g.papers.map { "\($0.meta.title.prefix(50)) (\($0.meta.year))" }.joined(separator: " || ") + "\n" }
            Task {
                let spaced = await model.runTogetherPapers()
                out += "run-together: \(spaced.count)\n"
                try? out.write(toFile: "/tmp/lunta-cleanup.txt", atomically: true, encoding: .utf8)
                exit(0)
            }
        } else if let model, let i = args.firstIndex(of: "-importPDFs"), i + 1 < args.count {
            let folder = URL(fileURLWithPath: args[i + 1])
            Task { await model.importPDFFolder(folder); await model.makeReadingViews() }
        } else if let model {
            // keep making reading views in the background whenever the app is open
            Task {
                try? await Task.sleep(nanoseconds: 3_000_000_000)
                await model.makeReadingViews()
                await model.remakeMissingFigures()
            }
        }
    }
}

struct LuntaCommands: Commands {
    let model: LibraryModel?
    @Environment(\.openWindow) private var openWindow

    var body: some Commands {
        CommandGroup(after: .appInfo) {
            CheckForUpdatesButton()
        }
        CommandGroup(replacing: .help) {
            Button("Getting Started with Lunta") {
                NSWorkspace.shared.open(URL(string: "https://christoffermattssonlangseth.github.io/lunta/getting-started.html")!)
            }
            Button("Importing from Zotero, Mendeley, ReadCube…") {
                NSWorkspace.shared.open(URL(string: "https://christoffermattssonlangseth.github.io/lunta/getting-started.html#import")!)
            }
            Button("Lunta Support") {
                NSWorkspace.shared.open(URL(string: "https://christoffermattssonlangseth.github.io/lunta/support.html")!)
            }
        }
        CommandGroup(after: .appSettings) {
            Button("Connect AI Agents…") { openWindow(id: "agents") }
            Button("Citations in Word & Google Docs…") { openWindow(id: "citations") }
            Button("Google Drive for Android…") { openWindow(id: "drive") }
            Button("Clean Up Library…") { openWindow(id: "cleanup") }
        }
        CommandGroup(after: .newItem) {
            Button("Add Paper…") { NotificationCenter.default.post(name: .luntaAddPaper, object: nil) }
                .keyboardShortcut("n", modifiers: [.command, .shift])
            Button("Add PDF…") { addPDFs() }
                .keyboardShortcut("o", modifiers: [.command])
            Button("Add PDF Privately…") { addPDFs(privately: true) }
                .keyboardShortcut("o", modifiers: [.command, .option])
            Divider()
            Button("Import from Zotero, Mendeley, ReadCube, EndNote… (BibTeX / RIS)") { importExport() }
                .keyboardShortcut("i", modifiers: [.command, .shift])
            Button("Import Folder of PDFs…") { importPDFFolder() }
            Button("Import BibTeX (Keep Citation Keys)…") { importBibKeepingKeys(model: model) }
            Divider()
            Button("Find Missing PDFs") { Task { await model?.findMissingPDFs() } }
            Button("Make Missing Reading Views") { Task { await model?.makeReadingViews() } }
        }
    }

    private func importExport() {
        guard let model else { return }
        let p = NSOpenPanel()
        p.title = "Choose the exported library (.bib or .ris)"
        p.allowedContentTypes = [UTType(filenameExtension: "bib"), UTType(filenameExtension: "ris")].compactMap { $0 }
        guard p.runModal() == .OK, let file = p.url else { return }
        let f = NSOpenPanel()
        f.title = "Optional: choose the folder with the exported PDFs (Cancel to skip)"
        f.canChooseDirectories = true
        f.canChooseFiles = false
        let folder = f.runModal() == .OK ? f.url : nil
        // a folder for the imported papers: an exported collection keeps its name ("Thesis.bib" → "Thesis")
        let ask = NSAlert()
        ask.messageText = "Put the imported papers in a folder?"
        ask.informativeText = "Exports don't include your folders. To move a collection from Zotero or Mendeley, export it on its own and import it into a folder of the same name."
        let name = NSTextField(frame: NSRect(x: 0, y: 0, width: 260, height: 24))
        name.stringValue = file.deletingPathExtension().lastPathComponent
        ask.accessoryView = name
        ask.addButton(withTitle: "Import into Folder")
        ask.addButton(withTitle: "Import Without Folder")
        let into = ask.runModal() == .alertFirstButtonReturn
        let target = into ? name.stringValue : nil
        Task { await model.importExport(file: file, pdfFolder: folder, folder: target) }
    }

    /// Pick PDFs; each becomes a library entry with metadata from its DOI or an OpenAlex title match.
    /// Privately: nothing about the PDF leaves this Mac (no DOI or title lookups), e.g. for manuscripts under review.
    private func addPDFs(privately: Bool = false) {
        guard let model else { return }
        let p = NSOpenPanel()
        p.title = privately ? "Add PDFs privately (no online lookups)" : "Add PDFs to Lunta"
        p.allowedContentTypes = [.pdf]
        p.allowsMultipleSelection = true
        guard p.runModal() == .OK, !p.urls.isEmpty else { return }
        let urls = p.urls
        Task { await model.addPDFs(urls, privately: privately) }
    }

    private func importPDFFolder() {
        guard let model else { return }
        let f = NSOpenPanel()
        f.canChooseDirectories = true
        f.canChooseFiles = false
        guard f.runModal() == .OK, let folder = f.url else { return }
        Task { await model.importPDFFolder(folder); await model.makeReadingViews() }
    }
}

extension Notification.Name {
    static let luntaAddPaper = Notification.Name("luntaAddPaper")
}
