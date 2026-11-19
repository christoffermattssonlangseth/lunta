import AppKit
import SwiftUI
import UniformTypeIdentifiers
import LuntaCore

/// .bib files Lunta keeps up to date for a folder or the whole library ("Keep BibTeX File
/// Updated…"), e.g. in a Dropbox folder Overleaf syncs or a LaTeX project's Git repository.
/// Rewritten (atomically, and only when the text changed) a moment after papers in the folder are
/// added, removed or edited, on this or another device. The links are this Mac's own (paths differ
/// per device): kept in its settings, not in the synced library.
@MainActor
final class BibFiles: ObservableObject {
    static let shared = BibFiles()
    /// The source for the whole library; other sources are folder (collection) ids.
    static let allPapers = "*"

    struct Link: Codable, Equatable {
        var source: String
        var path: String
    }

    @Published private(set) var links: [Link] = []
    /// Last problem writing a file, per source (shown in its menu).
    @Published private(set) var problems: [String: String] = [:]
    private var pending: Task<Void, Never>?
    private var writing = false
    private var again = false

    /// Scratch libraries (-testLibrary) keep their own list.
    private var defaultsKey: String { LibraryModel.testLibrary == nil ? "bibFileLinks" : "bibFileLinks-test" }

    private init() {
        if let d = UserDefaults.standard.data(forKey: defaultsKey), let l = try? JSONDecoder().decode([Link].self, from: d) {
            links = l
        }
    }

    func link(_ source: String) -> Link? { links.first { $0.source == source } }

    private func save() {
        UserDefaults.standard.set(try? JSONEncoder().encode(links), forKey: defaultsKey)
    }

    /// Asks where to save the folder's .bib file, writes it and keeps it updated.
    func choose(source: String, name: String, model: LibraryModel) {
        let panel = NSSavePanel()
        panel.title = "Keep BibTeX File Updated"
        panel.message = "Lunta rewrites this file whenever papers in “\(name)” change."
        panel.prompt = "Keep Updated"
        panel.allowedContentTypes = [UTType(filenameExtension: "bib") ?? .plainText]
        if let old = link(source) {
            let u = URL(fileURLWithPath: old.path)
            panel.directoryURL = u.deletingLastPathComponent()
            panel.nameFieldStringValue = u.lastPathComponent
        } else {
            panel.nameFieldStringValue = Self.fileName(name)
        }
        guard panel.runModal() == .OK, let url = panel.url else { return }
        links.removeAll { $0.source == source }
        links.append(Link(source: source, path: url.path))
        save()
        update(model, delay: 0)
    }

    func stop(_ source: String) {
        links.removeAll { $0.source == source }
        problems[source] = nil
        save()
    }

    /// "Spatial Omics" → "Spatial Omics.bib" (no slashes or colons).
    static func fileName(_ name: String) -> String {
        let n = name.components(separatedBy: CharacterSet(charactersIn: "/:")).joined(separator: "-").trimmingCharacters(in: .whitespaces)
        return (n.isEmpty ? "library" : n) + ".bib"
    }

    /// The library changed (called from `LibraryModel.reload`): rewrite the files shortly after.
    func libraryChanged(_ model: LibraryModel) {
        guard !links.isEmpty else { return }
        update(model, delay: 1.5)
    }

    /// Writes every linked file whose text changed, after `delay` seconds without further changes.
    func update(_ model: LibraryModel, delay: Double) {
        pending?.cancel()
        pending = Task { [weak self, weak model] in
            if delay > 0 { try? await Task.sleep(nanoseconds: UInt64(delay * 1e9)) }
            guard !Task.isCancelled, let self, let model else { return }
            await self.writeAll(model)
        }
    }

    private func writeAll(_ model: LibraryModel) async {
        if writing { again = true; return }
        writing = true
        defer { writing = false }
        // a folder that isn't there (deleted, or not synced yet) leaves its file as it was: an
        // empty .bib would break the document
        let folders = Set(model.collections.map(\.id))
        let lib = model.lib, todo = links.filter { $0.source == Self.allPapers || folders.contains($0.source) }
        let results = await Task.detached(priority: .utility) {
            todo.map { l in (l.source, Self.write(lib, l)) }
        }.value
        for (source, problem) in results { problems[source] = problem }
        if again {
            again = false
            await writeAll(model)
        }
    }

    /// The .bib text for a source: its papers sorted by citation key, so the file changes only
    /// where papers did (friendly to Git and Overleaf).
    /// Nil for an empty library (still opening, or syncing to a new Mac): the file is left as it was.
    /// Extra entries kept from an imported .bib (software, web pages) come after the papers.
    nonisolated static func bibtex(_ lib: Library, source: String) -> String? {
        lib.keptBibtex(collection: source == allPapers ? nil : source)
    }

    /// Writes the file if its text changed; returns the problem, if any.
    nonisolated private static func write(_ lib: Library, _ l: Link) -> String? {
        let url = URL(fileURLWithPath: l.path)
        guard let text = bibtex(lib, source: l.source) else { return nil }
        if let old = try? String(contentsOf: url, encoding: .utf8), old == text { return nil }
        do {
            try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
            try Data(text.utf8).write(to: url, options: .atomic)
            return nil
        } catch {
            return error.localizedDescription
        }
    }
}

/// "Keep BibTeX File Updated…", or for a linked folder: where the file is, and Stop Updating.
struct BibFileMenu: View {
    @EnvironmentObject var model: LibraryModel
    @ObservedObject private var files = BibFiles.shared
    let source: String
    let name: String

    var body: some View {
        if let l = files.link(source) {
            let url = URL(fileURLWithPath: l.path)
            Menu("BibTeX File: \(url.lastPathComponent)") {
                if let p = files.problems[source] { Text("Couldn't write: \(p)") }
                Button("Show in Finder") { NSWorkspace.shared.activateFileViewerSelecting([url]) }
                Button("Update Now") { files.update(model, delay: 0) }
                Button("Save Elsewhere…") { files.choose(source: source, name: name, model: model) }
                Divider()
                Button("Stop Updating") { files.stop(source) }
            }
        } else {
            Button("Keep BibTeX File Updated…") { files.choose(source: source, name: name, model: model) }
        }
        if source != BibFiles.allPapers {
            Button("Import BibTeX (Keep Citation Keys)…") { importBibKeepingKeys(model: model, folder: source) }
        }
    }
}

/// "Import BibTeX (Keep Citation Keys)…": choose a LaTeX project's .bib and import it into a folder
/// (`folder`, or one named after the file, which can be changed), keeping its keys.
@MainActor
func importBibKeepingKeys(model: LibraryModel?, folder: String? = nil) {
    guard let model else { return }
    let p = NSOpenPanel()
    p.title = "Choose the LaTeX project's .bib file"
    p.message = "Each entry is looked up (DOI, arXiv, Crossref) and added with its citation key, so \\cite{…} keeps working."
    p.allowedContentTypes = [UTType(filenameExtension: "bib")].compactMap { $0 }
    guard p.runModal() == .OK, let file = p.url else { return }
    var name: String?
    if folder == nil {
        let ask = NSAlert()
        ask.messageText = "Import into which folder?"
        ask.informativeText = "The papers go into this folder (made if there is none). Keep its BibTeX file updated afterwards and use that file in the LaTeX project."
        let field = NSTextField(frame: NSRect(x: 0, y: 0, width: 260, height: 24))
        field.stringValue = file.deletingPathExtension().lastPathComponent
        ask.accessoryView = field
        ask.addButton(withTitle: "Import")
        ask.addButton(withTitle: "Cancel")
        guard ask.runModal() == .alertFirstButtonReturn else { return }
        name = field.stringValue
    }
    Task { await model.importBibKeepingKeys(file: file, folderID: folder, folderName: name) }
}

/// A sidebar name, with a mark when Lunta keeps a .bib file for it.
struct BibLinkedName: View {
    @ObservedObject private var files = BibFiles.shared
    let name: String
    let source: String

    var body: some View {
        if let l = files.link(source) {
            HStack(spacing: 4) {
                Text(name)
                Image(systemName: files.problems[source] == nil ? "doc.text" : "exclamationmark.triangle")
                    .font(.caption2).foregroundStyle(.secondary)
            }
            .help("Kept updated: \(l.path)")
        } else {
            Text(name)
        }
    }
}
