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
            }
        }

        .handlesExternalEvents(matching: ["library"])

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
        }
        // scripting hook: `Margins -import <file.bib|.ris> [-pdfs <folder>]`
        let args = ProcessInfo.processInfo.arguments
        if let model, let i = args.firstIndex(of: "-import"), i + 1 < args.count {
            let file = URL(fileURLWithPath: args[i + 1])
            let pdfs = args.firstIndex(of: "-pdfs").flatMap { $0 + 1 < args.count ? URL(fileURLWithPath: args[$0 + 1]) : nil }
            Task { await model.importExport(file: file, pdfFolder: pdfs) }
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

    var body: some Commands {
        CommandGroup(after: .newItem) {
            Button("Add Paper…") { NotificationCenter.default.post(name: .marginsAddPaper, object: nil) }
                .keyboardShortcut("n", modifiers: [.command, .shift])
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
