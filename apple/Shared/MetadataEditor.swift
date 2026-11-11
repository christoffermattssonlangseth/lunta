import SwiftUI
import MarginsCore

/// Edit a paper's details. Can re-read them from the PDF (on-device) or fetch them by DOI.
struct MetadataEditor: View {
    @EnvironmentObject var model: LibraryModel
    @Environment(\.dismiss) private var dismiss
    let paperID: String
    @State private var meta: PaperMeta = PaperMeta.empty
    @State private var authors = ""
    @State private var keywords = ""
    @State private var busy: String?
    @State private var note: String?
    @State private var loaded = false

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    field("Title", $meta.title, axis: .vertical)
                    VStack(alignment: .leading, spacing: 4) {
                        Text("Authors, one per line").font(.caption).foregroundStyle(.secondary)
                        TextEditor(text: $authors).frame(minHeight: 90).font(.body)
                    }
                }
                Section("Publication") {
                    field("Journal", $meta.journal)
                    field("Date or year", $meta.date)
                    HStack {
                        field("Volume", $meta.volume)
                        field("Issue", $meta.issue)
                        field("Pages", $meta.pages)
                    }
                    field("Publisher", $meta.publisher)
                }
                Section("Identifiers") {
                    field("DOI", $meta.doi)
                    field("PubMed ID", $meta.pmid)
                    field("PMCID", $meta.pmcid)
                    field("arXiv", $meta.arxiv)
                    field("Link", $meta.url)
                }
                Section("Abstract") {
                    TextEditor(text: $meta.abstractText).frame(minHeight: 120)
                }
                Section {
                    field("Keywords, comma separated", $keywords)
                }
                Section {
                    Button { Task { await fromPDF() } } label: {
                        Label("Read Title and Authors from the PDF", systemImage: "doc.text.viewfinder")
                    }
                    .disabled(busy != nil || model.paper(paperID)?.hasPdf != true || !PDFDetails.available)
                    Button { Task { await fromDOI() } } label: {
                        Label("Fill In from the DOI (online)", systemImage: "globe")
                    }
                    .disabled(busy != nil || meta.doi.trimmingCharacters(in: .whitespaces).isEmpty)
                    if let busy { HStack { ProgressView().controlSize(.small); Text(busy).foregroundStyle(.secondary) } }
                    if let note { Text(note).font(.caption).foregroundStyle(.secondary) }
                } footer: {
                    Text("Reading from the PDF uses Apple's on-device model: nothing leaves this device.")
                }
            }
            .formStyle(.grouped)
            .navigationTitle("Edit Details")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
                ToolbarItem(placement: .confirmationAction) { Button("Save") { save() }.disabled(meta.title.trimmingCharacters(in: .whitespaces).isEmpty) }
            }
        }
        #if os(macOS)
        .frame(minWidth: 560, idealWidth: 620, minHeight: 640, idealHeight: 760)
        #endif
        .onAppear {
            guard !loaded, let p = model.paper(paperID) else { return }
            loaded = true
            set(p.meta)
        }
    }

    private func field(_ label: String, _ text: Binding<String>, axis: Axis = .horizontal) -> some View {
        TextField(label, text: text, axis: axis)
    }

    private func set(_ m: PaperMeta) {
        meta = m
        authors = m.authors.joined(separator: "\n")
        keywords = m.keywords.joined(separator: ", ")
    }

    private func save() {
        var m = meta
        m.title = m.title.trimmingCharacters(in: .whitespacesAndNewlines)
        m.authors = authors.split(whereSeparator: \.isNewline).map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }
        m.keywords = keywords.split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }
        m.doi = m.doi.trimmingCharacters(in: .whitespaces)
        model.updateMeta(paperID, m)
        dismiss()
    }

    private func fromPDF() async {
        busy = "Reading the first page…"
        defer { busy = nil }
        guard let url = await model.ensureLocal(paperID, .pdf), let data = try? Data(contentsOf: url),
              let x = await PDFDetails.extract(fromPDF: data) else { note = "Couldn't read details from the PDF."; return }
        var found: [String] = []
        if !x.title.isEmpty { meta.title = x.title; found.append("title") }
        if !x.authors.isEmpty { authors = x.authors.joined(separator: "\n"); found.append("\(x.authors.count) authors") }
        if !x.journal.isEmpty { meta.journal = x.journal; found.append("journal") }
        if !x.year.isEmpty, meta.date.isEmpty || !meta.date.hasPrefix(x.year) { meta.date = x.year; found.append("year") }
        if !x.doi.isEmpty { meta.doi = x.doi; found.append("DOI") }
        note = found.isEmpty ? "Nothing found on the first page." : "Found " + found.joined(separator: ", ") + ". Check, then Save."
    }

    private func fromDOI() async {
        busy = "Looking up the DOI…"
        defer { busy = nil }
        let doi = meta.doi.trimmingCharacters(in: .whitespaces)
        guard let m = await Task.detached(operation: { try? resolveMetadata(input: doi) }).value, !m.title.isEmpty else {
            note = "No record found for that DOI."
            return
        }
        let abstract = meta.abstractText
        set(m)
        if meta.abstractText.isEmpty { meta.abstractText = abstract }
        note = "Filled in from the DOI. Check, then Save."
    }
}
