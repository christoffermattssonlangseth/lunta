import SwiftUI
import UIKit
import UniformTypeIdentifiers
import LuntaCore

/// Importing a library on iPhone and iPad: a BibTeX / RIS export from Zotero, Mendeley, ReadCube,
/// EndNote…, the folder Zotero's "Export Files" makes (the .bib and its PDFs), or a ZIP of it; and a
/// folder of PDFs matched to the papers already in Lunta. Started from + and the library's list of places.
@MainActor
final class LibraryImporter: ObservableObject {
    static let shared = LibraryImporter()

    enum Kind { case library, pdfFolder, bibKeys }

    /// The file picker is open for this.
    @Published var picking: Kind?
    /// Picked and opened, waiting for "Put the imported papers in a folder?".
    @Published var asking: Pending?
    /// A LaTeX project's .bib picked, waiting for the folder to import it into (its keys kept).
    @Published var askingBib: PendingBib?

    struct PendingBib: Identifiable {
        let id = UUID()
        let file: URL
        let scoped: Bool
    }
    /// The alert at the end (or the error).
    @Published var result: (title: String, message: String)?
    @Published var showingHelp = false
    @Published private(set) var running = false

    struct Pending: Identifiable {
        let id = UUID()
        let source: ImportSource
        let picked: URL
        let scoped: Bool
    }

    func start(_ kind: Kind) {
        guard !running else {
            result = ("Import in progress", "Wait for the import that is running to finish.")
            return
        }
        picking = kind
    }

    /// The picker's choice. Access to it (its security scope) is kept until the import is done:
    /// the export's PDFs are read from the picked folder.
    func picked(_ url: URL, kind: Kind, model: LibraryModel) {
        let scoped = url.startAccessingSecurityScopedResource()
        running = true
        switch kind {
        case .bibKeys:
            askingBib = PendingBib(file: url, scoped: scoped)
        case .pdfFolder:
            Task {
                let bg = BackgroundWork("Import PDFs")
                let out = await model.importPDFFolder(url)
                if scoped { url.stopAccessingSecurityScopedResource() }
                bg.end()
                running = false
                result = ("PDFs imported", out.text)
                await model.makeReadingViews()
            }
        case .library:
            model.status = "Opening \(url.lastPathComponent)…"
            Task {
                let bg = BackgroundWork("Open library export")
                defer { bg.end() }
                do {
                    let source = try await Task.detached { try ImportSource.open(url) }.value
                    model.status = nil
                    asking = Pending(source: source, picked: url, scoped: scoped)
                } catch {
                    model.status = nil
                    if scoped { url.stopAccessingSecurityScopedResource() }
                    running = false
                    result = ("Can't import “\(url.lastPathComponent)”", error.readable)
                }
            }
        }
    }

    /// Imports what was picked, into `folder` if given; reading views are made afterwards, in the background.
    func run(_ p: Pending, folder: String?, model: LibraryModel) {
        asking = nil
        Task {
            // a large library takes a while: keep going if the app is sent to the background briefly
            let bg = BackgroundWork("Import library")
            var summary: ImportSummary?
            var failure: Error?
            do {
                summary = try await model.runImport(file: p.source.export, pdfFolder: p.source.root, folder: folder,
                                                    roots: p.source.root.map { [$0] } ?? [])
            } catch {
                failure = error
                model.status = nil
            }
            finish(p)
            bg.end()
            if let summary {
                var text = summary.text
                if summary.missingPDFs > 0 && p.source.root == nil {
                    text += "\n\nTo bring the PDFs too, choose the export's whole folder (or a ZIP of it) instead of the file."
                }
                result = ("Library imported", text + "\n\nReading views are made in the background.")
                await model.makeReadingViews()
            } else if let failure {
                result = ("Import failed", failure.readable)
            }
        }
    }

    /// Imports the .bib into the folder `folder` with its citation keys; the report is shown by
    /// the library (`LibraryModel.bibImport`).
    func runBib(_ p: PendingBib, folder: String, model: LibraryModel) {
        askingBib = nil
        Task {
            let bg = BackgroundWork("Import BibTeX")
            await model.importBibKeepingKeys(file: p.file, folderName: folder)
            if p.scoped { p.file.stopAccessingSecurityScopedResource() }
            bg.end()
            running = false
        }
    }

    func cancelBib(_ p: PendingBib) {
        askingBib = nil
        if p.scoped { p.file.stopAccessingSecurityScopedResource() }
        running = false
    }

    func cancel(_ p: Pending) {
        asking = nil
        finish(p)
    }

    private func finish(_ p: Pending) {
        if p.scoped { p.picked.stopAccessingSecurityScopedResource() }
        if let t = p.source.temporary { try? FileManager.default.removeItem(at: t) }
        running = false
    }
}

/// Time to finish when the app goes to the background (iOS gives about half a minute).
@MainActor
private final class BackgroundWork {
    private var id: UIBackgroundTaskIdentifier = .invalid
    init(_ name: String) {
        id = UIApplication.shared.beginBackgroundTask(withName: name) { [weak self] in self?.end() }
    }
    func end() {
        guard id != .invalid else { return }
        UIApplication.shared.endBackgroundTask(id)
        id = .invalid
    }
}

/// The file picker, the folder question, the result and the help, on the whole library (iPhone and iPad).
struct LibraryImportFlow: ViewModifier {
    @EnvironmentObject var model: LibraryModel
    @ObservedObject private var importer = LibraryImporter.shared
    @State private var kind: LibraryImporter.Kind = .library
    @State private var folderName = ""

    func body(content: Content) -> some View {
        content
            .onChange(of: importer.picking) { _, k in if let k { kind = k } }
            .fileImporter(isPresented: Binding(get: { importer.picking != nil }, set: { if !$0 { importer.picking = nil } }),
                          allowedContentTypes: Self.types(importer.picking ?? kind)) { r in
                if case .success(let url) = r { importer.picked(url, kind: kind, model: model) }
            }
            .alert("Put the imported papers in a folder?",
                   isPresented: Binding(get: { importer.asking != nil }, set: { if !$0 { importer.asking = nil } }),
                   presenting: importer.asking) { p in
                TextField("Folder name", text: $folderName)
                Button("Import into Folder") { importer.run(p, folder: folderName, model: model) }
                Button("Import Without Folder") { importer.run(p, folder: nil, model: model) }
                Button("Cancel", role: .cancel) { importer.cancel(p) }
            } message: { p in
                Text("Importing “\(p.source.export.lastPathComponent)”. Exports don't include your folders: to keep a collection, export it on its own and import it into a folder of the same name.")
            }
            .onChange(of: importer.asking?.id) { folderName = importer.asking?.source.suggestedFolder ?? "" }
            .alert("Import into which folder?",
                   isPresented: Binding(get: { importer.askingBib != nil }, set: { if !$0 { importer.askingBib = nil } }),
                   presenting: importer.askingBib) { p in
                TextField("Folder name", text: $folderName)
                Button("Import") { importer.runBib(p, folder: folderName, model: model) }
                Button("Cancel", role: .cancel) { importer.cancelBib(p) }
            } message: { p in
                Text("Each entry of “\(p.file.lastPathComponent)” is looked up (DOI, arXiv, Crossref) and added with its citation key, so \\cite{…} keeps working in the folder's BibTeX file.")
            }
            .onChange(of: importer.askingBib?.id) { folderName = importer.askingBib?.file.deletingPathExtension().lastPathComponent ?? "" }
            .sheet(item: $model.bibImport) { r in BibImportReportView(result: r) }
            .alert(importer.result?.title ?? "", isPresented: Binding(get: { importer.result != nil }, set: { if !$0 { importer.result = nil } })) {
                Button("OK") { importer.result = nil }
            } message: { Text(importer.result?.message ?? "") }
            .sheet(isPresented: $importer.showingHelp) { MovingToLuntaHelp() }
    }
}

extension LibraryImportFlow {
    static func types(_ k: LibraryImporter.Kind) -> [UTType] {
        switch k {
        case .pdfFolder: [.folder]
        case .bibKeys: [.bibtex, .plainText]
        case .library: ImportSource.pickable
        }
    }
}

extension View {
    func libraryImport() -> some View { modifier(LibraryImportFlow()) }
}

/// The + menu of the paper list (iPhone and iPad).
struct AddMenu: View {
    @Binding var adding: Bool

    var body: some View {
        Menu {
            Button("Add Paper by DOI or Link…", systemImage: "link") { adding = true }
            Divider()
            ImportMenuItems()
        } label: { Label("Add", systemImage: "plus") }
    }
}

struct ImportMenuItems: View {
    var body: some View {
        Button("Import Library (Zotero, Mendeley, ReadCube, EndNote…)", systemImage: "square.and.arrow.down.on.square") {
            LibraryImporter.shared.start(.library)
        }
        Button("Import Folder of PDFs", systemImage: "folder.badge.plus") { LibraryImporter.shared.start(.pdfFolder) }
        Button("Import BibTeX (Keep Citation Keys)", systemImage: "curlybraces") { LibraryImporter.shared.start(.bibKeys) }
        Button("Moving from Zotero, Mendeley…", systemImage: "questionmark.circle") { LibraryImporter.shared.showingHelp = true }
    }
}

/// How to bring a library from another reference manager (the steps of docs/importing.md, for iPhone and iPad).
struct MovingToLuntaHelp: View {
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            List {
                Section {
                    Text("Lunta imports BibTeX (.bib) and RIS (.ris) exports from any reference manager. Titles, authors, DOIs, tags and notes come along, PDFs too where the export has them, and duplicates are merged.")
                }
                Section("1. Export on your computer") {
                    step("Zotero", "File › Export Library… (or right-click a collection › Export Collection…), format BibTeX, tick Export Files. Zotero makes a folder with the .bib file and a “files” folder with the PDFs.")
                    step("Mendeley", "Select the papers, File › Export… as BibTeX. The export has no PDFs: put the PDFs you have in a folder and use Import Folder of PDFs afterwards.")
                    step("ReadCube Papers", "Export › BibTeX, and Export PDFs into the same folder.")
                    step("EndNote", "File › Export…, output style RefMan (RIS) Export. Put the .ris in a folder together with the library's .Data/PDF folder.")
                    step("Paperpile, JabRef, BibDesk", "Export or use the .bib file; put it in a folder with the PDFs.")
                }
                Section("2. Get it to this device") {
                    Text("Put the export's folder in iCloud Drive (or another place the Files app shows), or zip it (in the Finder: right-click › Compress) and AirDrop the ZIP here.")
                }
                Section("3. Import") {
                    Text("Tap + › Import Library and choose the export's folder, the ZIP, or just the .bib or .ris file (then without its PDFs). Lunta finds the export in the folder, takes the PDFs it names, and matches any other PDFs in the folder to the papers by DOI or title.")
                    Text("Exports don't include your folders. To keep a collection, export it on its own and import it into a folder of the same name; Lunta suggests the export's name.")
                    Text("Reading views are made in the background afterwards. The library syncs to your other devices through iCloud.")
                }
                Section {
                    Link(destination: URL(string: "https://christoffermattssonlangseth.github.io/lunta/getting-started.html#import")!) {
                        Label("More about importing", systemImage: "safari")
                    }
                    Button {
                        dismiss()
                        // the picker after the sheet has gone
                        Task { try? await Task.sleep(for: .milliseconds(600)); LibraryImporter.shared.start(.library) }
                    } label: {
                        Label("Import Library…", systemImage: "square.and.arrow.down.on.square")
                    }
                }
            }
            .navigationTitle("Moving to Lunta")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } } }
        }
    }

    private func step(_ app: String, _ text: String) -> some View {
        VStack(alignment: .leading, spacing: 3) {
            Text(app).font(.headline)
            Text(text).font(.callout).foregroundStyle(.secondary)
        }
        .padding(.vertical, 2)
    }
}
