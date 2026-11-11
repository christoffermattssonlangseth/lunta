import SwiftUI
import MarginsCore
import UniformTypeIdentifiers

// Both MarginsCore and Paper2EpubKit define a PaperMeta; in the app it means the library's.
typealias PaperMeta = MarginsCore.PaperMeta

@main
struct MarginsApp: App {
    @StateObject private var holder = ModelHolder()

    var body: some Scene {
        WindowGroup("Margins") {
            Group {
                if let model = holder.model {
                    LibraryWindow().environmentObject(model)
                        .handlesExternalEvents(preferring: ["margins"], allowing: ["*"])
                } else {
                    Text(holder.error ?? "Opening library…").padding()
                }
            }
            .frame(minWidth: 980, minHeight: 600)
        }
        .commands {
            MarginsCommands(model: holder.model)
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

        Window("Connect AI Agents", id: "agents") { AgentConnectView().tint(.marginsTeal) }
        Window("Citations in Word & Google Docs", id: "citations") { CitationsView().tint(.marginsTeal) }
        Window("Clean Up Library", id: "cleanup") {
            if let model = holder.model { NavigationStack { CleanupView() }.environmentObject(model).tint(.marginsTeal) }
        }
            .windowResizability(.contentSize)

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
        let (url, cloud) = await Task.detached { CloudLibrary.resolve() }.value
        if cloud { await CloudLibrary.prepareLogs(url) { _ in } }
        do {
            let lib = try await Task.detached { try LibraryModel.openLibrary(root: url) }.value
            model = LibraryModel(lib: lib, root: url, inCloud: cloud)
        } catch { self.error = "Could not open the library: \(error.localizedDescription)" }
        if let model {
            server = ExtensionServer(model: model)
            server?.start()
            citations = CitationServer(model: model)
            citations?.start()
            model.watchForOutsideChanges()
            NarrationRenderer.shared.model = model
            if !ProcessInfo.processInfo.arguments.contains(where: { $0.hasPrefix("-ask") || $0 == "-cleanupReport" }) {
                WatchCenter.shared.start(model)
            }
        }
        // scripting hook: `Margins -import <file.bib|.ris> [-pdfs <folder>]`
        let args = ProcessInfo.processInfo.arguments
        if let model, let i = args.firstIndex(of: "-import"), i + 1 < args.count {
            let file = URL(fileURLWithPath: args[i + 1])
            let pdfs = args.firstIndex(of: "-pdfs").flatMap { $0 + 1 < args.count ? URL(fileURLWithPath: args[$0 + 1]) : nil }
            Task { await model.importExport(file: file, pdfFolder: pdfs) }
        } else if let i = args.firstIndex(of: "-narrate"), i + 1 < args.count {
            NarrationRenderer.shared.enqueue([args[i + 1]])
        } else if let model, let i = args.firstIndex(of: "-ask"), i + 1 < args.count {
            // test hook: `Margins -ask "<question>" [-paper <id>]` writes the transcript to /tmp/margins-ask.txt
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
                try? t.write(toFile: "/tmp/margins-ask.txt", atomically: true, encoding: .utf8)
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
            try? out.write(toFile: "/tmp/margins-cleanup.txt", atomically: true, encoding: .utf8)
            exit(0)
        } else if let model, let i = args.firstIndex(of: "-importPDFs"), i + 1 < args.count {
            let folder = URL(fileURLWithPath: args[i + 1])
            Task { await model.importPDFFolder(folder); await model.makeReadingViews() }
        } else if let model {
            // keep making reading views in the background whenever the app is open
            Task { try? await Task.sleep(nanoseconds: 3_000_000_000); await model.makeReadingViews() }
        }
    }
}

struct MarginsCommands: Commands {
    let model: LibraryModel?
    @Environment(\.openWindow) private var openWindow

    var body: some Commands {
        CommandGroup(after: .appSettings) {
            Button("Connect AI Agents…") { openWindow(id: "agents") }
            Button("Citations in Word & Google Docs…") { openWindow(id: "citations") }
            Button("Clean Up Library…") { openWindow(id: "cleanup") }
        }
        CommandGroup(after: .newItem) {
            Button("Add Paper…") { NotificationCenter.default.post(name: .marginsAddPaper, object: nil) }
                .keyboardShortcut("n", modifiers: [.command, .shift])
            Button("Add PDF…") { addPDFs() }
                .keyboardShortcut("o", modifiers: [.command])
            Button("Add PDF Privately…") { addPDFs(privately: true) }
                .keyboardShortcut("o", modifiers: [.command, .option])
            Divider()
            Button("Import from ReadCube Papers / BibTeX / RIS…") { importExport() }
                .keyboardShortcut("i", modifiers: [.command, .shift])
            Button("Import Folder of PDFs…") { importPDFFolder() }
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
        Task { await model.importExport(file: file, pdfFolder: folder) }
    }

    /// Pick PDFs; each becomes a library entry with metadata from its DOI or an OpenAlex title match.
    /// Privately: nothing about the PDF leaves this Mac (no DOI or title lookups), e.g. for manuscripts under review.
    private func addPDFs(privately: Bool = false) {
        guard let model else { return }
        let p = NSOpenPanel()
        p.title = privately ? "Add PDFs privately (no online lookups)" : "Add PDFs to Margins"
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
    static let marginsAddPaper = Notification.Name("marginsAddPaper")
}
