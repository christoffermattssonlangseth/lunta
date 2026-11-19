import SwiftUI
import LuntaCore

// "Import BibTeX (Keep Citation Keys)…": a LaTeX project's .bib into a folder, each entry found by
// DOI, arXiv id or Crossref's search, its key kept as the paper's citation key so \cite{oldkey}
// keeps working in the .bib Lunta keeps for the folder. Entries that aren't papers (software, web
// pages) are kept as written with the folder and written into its .bib. The work is the core's
// (`Library.importBibtexKeepKeys`, core/src/bibimport.rs); this is the apps' part.

/// What an import of a .bib gave: shown in a sheet with the report.
struct BibImportResult: Identifiable {
    let id = UUID()
    let file: String
    let folderID: String
    let folder: String
    let report: BibImportReport
}

/// Passes the core's "Looking up 3 of 42: …" to the status line.
final class BibImportProgress: ProgressListener, @unchecked Sendable {
    private let update: @Sendable (String) -> Void
    init(_ update: @escaping @Sendable (String) -> Void) { self.update = update }
    func onProgress(message: String) { update(message) }
}

extension LibraryModel {
    /// Imports the .bib into the folder `folderID`, or a folder called `folderName` (made if there
    /// is none). The file is read with file coordination (downloaded first from iCloud Drive).
    func importBibKeepingKeys(file: URL, folderID: String? = nil, folderName: String? = nil) async {
        let lib = self.lib
        status = "Reading \(file.lastPathComponent)…"
        do {
            let text = try await Task.detached { try ImportSource.coordinated(file) { try String(contentsOf: $0, encoding: .utf8) } }.value
            let name = folderName?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
            let folder = try { () throws -> PaperCollection in
                if let folderID, let c = lib.collections().first(where: { $0.id == folderID }) { return c }
                let n = name.isEmpty ? file.deletingPathExtension().lastPathComponent : name
                return try lib.collections().first(where: { $0.name.caseInsensitiveCompare(n) == .orderedSame })
                    ?? lib.createCollection(name: n, parent: nil)
            }()
            let listener = BibImportProgress { [weak self] m in Task { @MainActor in self?.status = m } }
            let id = folder.id
            let report = try await Task.detached { try lib.importBibtexKeepKeys(text: text, collection: id, online: true, progress: listener) }.value
            reload()
            status = "Imported \(file.lastPathComponent) into “\(folder.name)”: \(report.keysKept) citation keys kept."
            bibImport = BibImportResult(file: file.lastPathComponent, folderID: folder.id, folder: folder.name, report: report)
        } catch {
            status = nil
            errorMessage = "Couldn't import “\(file.lastPathComponent)”: \(error.readable)"
        }
    }
}

/// The import's report: how each entry was found, what to check, every entry; Save or Share it.
struct BibImportReportView: View {
    @Environment(\.dismiss) private var dismiss
    let result: BibImportResult
    /// The Mac's "Keep BibTeX File Updated…" for the folder (nil on iPhone and iPad).
    var keepUpdated: (() -> Void)? = nil

    private var r: BibImportReport { result.report }
    private var toCheck: [BibImportItem] { r.items.filter { !$0.notes.isEmpty } }

    var body: some View {
        NavigationStack {
            List {
                Section {
                    LabeledContent("Found by DOI", value: "\(r.byDoi)")
                    LabeledContent("Found by arXiv id", value: "\(r.byArxiv)")
                    LabeledContent("Found by searching Crossref", value: "\(r.bySearch)")
                    LabeledContent("Added as they are (not found online)", value: "\(r.asIs)")
                    if r.extra > 0 { LabeledContent("Kept as written (software, web pages)", value: "\(r.extra)") }
                    if r.skipped > 0 { LabeledContent("Skipped (no title or DOI)", value: "\(r.skipped)") }
                    LabeledContent("Citation keys kept", value: "\(r.keysKept) of \(r.items.filter { !$0.paperId.isEmpty }.count)")
                } footer: {
                    Text("The papers are in “\(result.folder)” with the keys from \(result.file). Keep the folder's BibTeX file updated and use it in your LaTeX project: entries that aren't papers are written into it too.")
                }
                if !toCheck.isEmpty {
                    Section("To check") {
                        ForEach(Array(toCheck.enumerated()), id: \.offset) { _, i in
                            VStack(alignment: .leading, spacing: 2) {
                                Text(i.key).font(.body.monospaced())
                                ForEach(i.notes, id: \.self) { Text($0).font(.callout).foregroundStyle(.secondary) }
                            }
                        }
                    }
                }
                Section("Entries") {
                    ForEach(Array(r.items.enumerated()), id: \.offset) { _, i in
                        HStack(alignment: .firstTextBaseline) {
                            Text(i.key).font(.callout.monospaced()).frame(minWidth: 120, alignment: .leading)
                            Text(Self.how(i.found)).font(.caption).foregroundStyle(.secondary).frame(width: 64, alignment: .leading)
                            Text(i.title).font(.callout).lineLimit(2)
                        }
                    }
                }
            }
            .navigationTitle("Imported \(result.file)")
            .toolbar {
                ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } }
                ToolbarItem(placement: .automatic) {
                    ShareLink(item: r.text, preview: SharePreview("Import report")) { Label("Share Report", systemImage: "square.and.arrow.up") }
                }
                if let keepUpdated {
                    ToolbarItem(placement: .automatic) {
                        Button("Keep BibTeX File Updated…") { dismiss(); keepUpdated() }
                    }
                }
            }
        }
        #if os(macOS)
        .frame(minWidth: 640, minHeight: 520)
        #endif
    }

    static func how(_ f: BibFound) -> String {
        switch f {
        case .doi: "DOI"
        case .arxiv: "arXiv"
        case .search: "Crossref"
        case .asIs: "as is"
        case .extra: "kept"
        case .skipped: "skipped"
        }
    }
}
