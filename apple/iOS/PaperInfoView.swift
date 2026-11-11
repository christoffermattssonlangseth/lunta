import SwiftUI
import MarginsCore

/// Paper details: metadata, abstract, Find PDF, Make Reading View, citations.
struct PaperInfoView: View {
    @EnvironmentObject var model: LibraryModel
    @Environment(\.dismiss) private var dismiss
    @Environment(\.openURL) private var openURL
    let paperID: String
    let embedded: Bool
    @State private var busy: String?
    @State private var note: String?
    @State private var copied = false
    @State private var editing = false
    @State private var sharing = false

    var body: some View {
        if let p = model.paper(paperID) {
            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    HStack(spacing: 6) {
                        if !p.meta.journal.isEmpty { Pill(text: p.meta.journal, color: .marginsTeal) }
                        if !p.meta.year.isEmpty { Pill(text: p.meta.year, color: .secondary) }
                    }
                    Text(p.meta.title).font(.title3.weight(.bold)).textSelection(.enabled)
                    if !p.meta.authors.isEmpty {
                        Text(p.meta.authors.joined(separator: ", ")).font(.subheadline).foregroundStyle(.secondary).lineLimit(5)
                    }
                    actions(p)
                    if let busy { HStack { ProgressView(); Text(busy).font(.footnote).foregroundStyle(.secondary) } }
                    if let note { Text(note).font(.footnote).foregroundStyle(.secondary) }
                    if !p.meta.abstractText.isEmpty {
                        Card(title: "Abstract") { Text(p.meta.abstractText).font(.callout).lineSpacing(3).textSelection(.enabled) }
                    }
                    Card(title: "Folders & Tags") {
                        PaperFolders(paper: p)
                        if !p.tags.isEmpty {
                            TagChips(tags: p.tags.sorted()) { _ in true } toggle: { t, on in model.setTag([p.id], t, on) }
                        }
                    }
                    Card(title: "Cite") {
                        HStack {
                            ForEach([("APA", CitationStyle.apa), ("Vancouver", .vancouver), ("Nature", .nature)], id: \.0) { name, style in
                                Button(name) {
                                    UIPasteboard.general.string = model.citation(p.id, style)
                                    copied = true
                                }
                                .buttonStyle(.bordered)
                            }
                        }
                        if copied { Text("Copied to the clipboard.").font(.caption).foregroundStyle(.secondary) }
                    }
                    if !p.meta.doi.isEmpty, let u = URL(string: "https://doi.org/\(p.meta.doi)") {
                        Link("Open publisher page", destination: u).font(.callout)
                    }
                }
                .padding()
            }
            .navigationTitle(embedded ? "" : "Paper")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                if !embedded { ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } } }
                ToolbarItem(placement: .topBarLeading) {
                    Button { model.toggleStar(p) } label: { Image(systemName: p.starred ? "star.fill" : "star") }
                        .tint(.marginsCopper)
                }
                ToolbarItem(placement: .topBarLeading) {
                    Button { editing = true } label: { Image(systemName: "pencil") }
                }
                ToolbarItem(placement: .topBarLeading) {
                    Button { sharing = true } label: { Image(systemName: "square.and.arrow.up") }
                }
            }
            .sheet(isPresented: $editing) { MetadataEditor(paperID: paperID).environmentObject(model) }
            .sheet(isPresented: $sharing) {
                ScrollView { SharePanel(ids: [paperID]) }.environmentObject(model).presentationDetents([.medium, .large])
            }
        }
    }

    @ViewBuilder
    private func actions(_ p: Paper) -> some View {
        HStack(spacing: 10) {
            if !p.hasPdf {
                Button {
                    run("Searching open-access sources…") {
                        if let s = await model.findPDF(p.id) { note = "PDF found via \(s)." }
                        else { note = "No open-access PDF found. Open the publisher page and share the PDF to Margins." }
                    }
                } label: { Label("Find PDF", systemImage: "magnifyingglass") }
                .buttonStyle(.borderedProminent).tint(.marginsTeal)
            }
            if !p.hasEpub && (p.hasPdf || !p.meta.pmcid.isEmpty || !p.meta.doi.isEmpty) {
                Button {
                    run("Making reading view…") {
                        note = await model.makeReadingView(p.id) ? "Reading view ready." : "No open full text found for this paper."
                        model.reload()
                    }
                } label: { Label("Reading View", systemImage: "wand.and.stars") }
                .buttonStyle(.bordered)
            }
        }
        .disabled(busy != nil)
    }

    private func run(_ label: String, _ op: @escaping () async -> Void) {
        busy = label
        Task { await op(); busy = nil }
    }
}
