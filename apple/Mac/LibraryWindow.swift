import SwiftUI
import MarginsCore
import UniformTypeIdentifiers

struct LibraryWindow: View {
    @EnvironmentObject var model: LibraryModel
    @Environment(\.openWindow) private var openWindow
    @State private var selection = Set<String>()
    @State private var discoverSelection: String?
    @State private var addingPaper = false
    @State private var newCollection = false

    var body: some View {
        NavigationSplitView {
            Sidebar(newCollection: $newCollection)
                .navigationSplitViewColumnWidth(min: 180, ideal: 210)
        } content: {
            Group {
                if model.source == .discover {
                    DiscoverList(selection: $discoverSelection)
                } else {
                    PaperTable(selection: $selection)
                }
            }
            .navigationSplitViewColumnWidth(min: 420, ideal: 560)
        } detail: {
            if model.source == .discover {
                if let id = discoverSelection, let hit = model.discoverResults.first(where: { $0.id == id }) {
                    DiscoverDetail(hit: hit)
                } else {
                    ContentUnavailableView("Find new papers", systemImage: "globe",
                                           description: Text("Search above to explore papers beyond your library."))
                }
            } else if selection.count == 1, let id = selection.first, let p = model.paper(id) {
                PaperDetail(paper: p)
            } else if selection.count > 1 {
                MultiSelection(ids: Array(selection))
            } else {
                ContentUnavailableView("No paper selected", systemImage: "doc.text",
                                       description: Text("Add papers with ⇧⌘N, or import your ReadCube library with ⇧⌘I."))
            }
        }
        .searchable(text: $model.query, placement: .toolbar,
                    prompt: model.source == .discover ? "Search new papers (OpenAlex) — press Return" : "Search your library: titles, authors, full text, notes")
        .onSubmit(of: .search) {
            if model.source == .discover { Task { await model.discover(model.query) } }
        }
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Button { addingPaper = true } label: { Label("Add Paper", systemImage: "plus") }
            }
        }
        .safeAreaInset(edge: .bottom) { StatusBar() }
        .sheet(isPresented: $addingPaper) { AddPaperSheet() }
        .onReceive(NotificationCenter.default.publisher(for: .marginsAddPaper)) { _ in addingPaper = true }
        .onOpenURL { url in
            // margins://open?id=<paper id>  (from the browser extension)
            if url.host == "open", let id = URLComponents(url: url, resolvingAgainstBaseURL: false)?
                .queryItems?.first(where: { $0.name == "id" })?.value, model.paper(id) != nil {
                model.query = ""
                model.source = .all
                selection = [id]
                openWindow(value: id)
            }
        }
        .onDrop(of: [.pdf, .fileURL], isTargeted: nil) { providers in
            Task { await model.addPDFs(await pdfURLs(providers)) }
            return true
        }
        .alert("Margins", isPresented: Binding(get: { model.errorMessage != nil }, set: { if !$0 { model.errorMessage = nil } })) {
            Button("OK") { model.errorMessage = nil }
        } message: { Text(model.errorMessage ?? "") }
        .alert("New Collection", isPresented: $newCollection) { NewCollectionField() }
        .tint(.marginsTeal)
    }

    private func pdfURLs(_ providers: [NSItemProvider]) async -> [URL] {
        var out: [URL] = []
        for p in providers where p.hasItemConformingToTypeIdentifier(UTType.fileURL.identifier) {
            if let data = try? await p.loadItem(forTypeIdentifier: UTType.fileURL.identifier) as? Data,
               let url = URL(dataRepresentation: data, relativeTo: nil), url.pathExtension.lowercased() == "pdf" {
                out.append(url)
            }
        }
        return out
    }
}

struct NewCollectionField: View {
    @EnvironmentObject var model: LibraryModel
    @State private var name = ""
    var body: some View {
        TextField("Name", text: $name)
        Button("Create") { model.newCollection(name) }
        Button("Cancel", role: .cancel) {}
    }
}

// MARK: - Sidebar

struct Sidebar: View {
    @EnvironmentObject var model: LibraryModel
    @Binding var newCollection: Bool

    var body: some View {
        List(selection: Binding(get: { model.source }, set: { model.source = $0 ?? .all })) {
            Section("Library") {
                Label("All Papers", systemImage: "books.vertical").badge(model.counts.all).tag(SidebarItem.all)
                Label("Starred", systemImage: "star").badge(model.counts.starred).tag(SidebarItem.starred)
                Label("Unread", systemImage: "circle.badge").badge(model.counts.unread).tag(SidebarItem.unread)
            }
            Section("Beyond your library") {
                Label("Discover New Papers", systemImage: "globe.europe.africa").tag(SidebarItem.discover)
            }
            Section {
                ForEach(model.collections) { c in
                    Label(c.name, systemImage: "folder").badge(Int(c.paperCount)).tag(SidebarItem.collection(c.id))
                        .contextMenu {
                            Button("Delete Collection", role: .destructive) {
                                try? model.lib.deleteCollection(id: c.id); model.reload()
                            }
                        }
                }
            } header: {
                HStack {
                    Text("Collections")
                    Spacer()
                    Button { newCollection = true } label: { Image(systemName: "plus") }.buttonStyle(.borderless)
                }
            }
            if !model.tags.isEmpty {
                Section("Tags") {
                    ForEach(model.tags, id: \.self) { t in
                        Label(t, systemImage: "tag").tag(SidebarItem.tag(t))
                    }
                }
            }
        }
        .listStyle(.sidebar)
    }
}

// MARK: - Paper list

struct PaperTable: View {
    @EnvironmentObject var model: LibraryModel
    @Binding var selection: Set<String>
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        Table(model.papers, selection: $selection) {
            TableColumn("") { p in
                Group {
                    if p.starred {
                        Image(systemName: "star.fill").font(.caption).foregroundStyle(Color.marginsCopper)
                    } else if !p.read {
                        Circle().fill(Color.marginsTeal).frame(width: 7, height: 7)
                    }
                }
                .frame(maxWidth: .infinity)
            }
            .width(18)
            TableColumn("Title") { p in
                VStack(alignment: .leading, spacing: 3) {
                    Text(p.meta.title.isEmpty ? "Untitled" : p.meta.title)
                        .font(.body.weight(p.read ? .regular : .semibold))
                        .lineLimit(2)
                    if let snip = model.snippets[p.id], !snip.isEmpty {
                        SnippetText(snippet: snip)
                    } else if !p.meta.journal.isEmpty {
                        Text(p.meta.journal).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                    }
                }
                .padding(.vertical, 5)
            }
            .width(min: 240, ideal: 380)
            TableColumn("Authors") { p in Text(p.meta.authorLine).lineLimit(1).foregroundStyle(.secondary) }
                .width(min: 90, ideal: 140)
            TableColumn("Year") { p in Text(p.meta.year).monospacedDigit().foregroundStyle(.secondary) }.width(42)
            TableColumn("") { p in
                HStack(spacing: 4) {
                    if p.hasEpub { Pill(text: "Read", icon: "book", color: .marginsTeal) }
                    if p.hasPdf { Pill(text: "PDF", color: .marginsCopper) }
                    if p.hasAudio { Pill(text: "Audio", icon: "waveform", color: .purple) }
                    if p.annotationCount > 0 { Pill(text: "\(p.annotationCount)", icon: "highlighter", color: .secondary) }
                }
            }
            .width(min: 60, ideal: 110)
        }
        .tableStyle(.inset(alternatesRowBackgrounds: false))
        .safeAreaInset(edge: .bottom) {
            if !model.query.trimmingCharacters(in: .whitespaces).isEmpty {
                Button { model.searchBeyondLibrary() } label: {
                    Label("Search beyond your library for “\(model.query)”", systemImage: "globe.europe.africa")
                        .frame(maxWidth: .infinity)
                }
                .buttonStyle(.bordered).tint(.marginsTeal).controlSize(.large)
                .padding(10)
                .background(.bar)
            }
        }
        .contextMenu(forSelectionType: String.self) { ids in
            PaperMenu(ids: Array(ids))
        } primaryAction: { ids in
            if let id = ids.first { openWindow(value: id) }
        }
        .overlay {
            if model.papers.isEmpty {
                if model.query.isEmpty {
                    ContentUnavailableView("No papers", systemImage: "books.vertical",
                                           description: Text("Add a paper (⇧⌘N), drop PDFs here, or import from ReadCube Papers (⇧⌘I)."))
                } else {
                    ContentUnavailableView.search(text: model.query)
                }
            }
        }
        .navigationTitle(model.sourceTitle)
        .navigationSubtitle("\(model.papers.count) papers")
    }
}

struct SnippetText: View {
    let snippet: String
    var body: some View {
        // the core marks hits as [hit]
        var a = AttributedString()
        var rest = Substring(snippet)
        while let open = rest.firstIndex(of: "["), let close = rest[open...].firstIndex(of: "]") {
            a += AttributedString(String(rest[..<open]))
            var hit = AttributedString(String(rest[rest.index(after: open)..<close]))
            hit.font = .caption.bold()
            hit.foregroundColor = .primary
            hit.backgroundColor = Color.marginsHighlight.opacity(0.35)
            a += hit
            rest = rest[rest.index(after: close)...]
        }
        a += AttributedString(String(rest))
        return Text(a).font(.caption).foregroundStyle(.secondary).lineLimit(2)
    }
}

struct PaperMenu: View {
    @EnvironmentObject var model: LibraryModel
    @Environment(\.openWindow) private var openWindow
    let ids: [String]

    var body: some View {
        if ids.count == 1, let id = ids.first, let p = model.paper(id) {
            Button("Open") { openWindow(value: id) }
            Button(p.starred ? "Unstar" : "Star") { model.toggleStar(p) }
            Button(p.read ? "Mark as Unread" : "Mark as Read") { model.setRead(p, !p.read) }
            Divider()
            CiteMenu(ids: ids)
        } else {
            Button("Copy BibTeX") { copyToPasteboard(model.bibtex(ids)) }
        }
        if !model.collections.isEmpty {
            Menu("Add to Collection") {
                ForEach(model.collections) { c in Button(c.name) { model.setMembership(ids, collection: c.id, true) } }
            }
        }
        if case .collection(let cid) = model.source {
            Button("Remove from Collection") { model.setMembership(ids, collection: cid, false) }
        }
        Divider()
        Button("Find PDF") { Task { for id in ids { await model.findPDF(id) } } }
        Button("Make Reading View") { Task { for id in ids { await model.makeReadingView(id) }; model.reload() } }
        Button("Make Natural Narration") { NarrationRenderer.shared.enqueue(ids.filter { model.paper($0)?.hasEpub == true }) }
        Divider()
        Button("Delete", role: .destructive) { model.delete(ids) }
    }
}

struct CiteMenu: View {
    @EnvironmentObject var model: LibraryModel
    let ids: [String]
    var body: some View {
        Menu {
            if let id = ids.first, ids.count == 1 {
                Button("APA") { copyToPasteboard(model.citation(id, .apa)) }
                Button("Vancouver") { copyToPasteboard(model.citation(id, .vancouver)) }
                Button("Nature") { copyToPasteboard(model.citation(id, .nature)) }
                Divider()
            }
            Button("BibTeX") { copyToPasteboard(model.bibtex(ids)) }
            Button("RIS") { copyToPasteboard(model.ris(ids)) }
        } label: {
            Label("Cite", systemImage: "quote.opening")
        }
    }
}

func copyToPasteboard(_ s: String) {
    NSPasteboard.general.clearContents()
    NSPasteboard.general.setString(s, forType: .string)
}

// MARK: - Detail

struct PaperDetail: View {
    @EnvironmentObject var model: LibraryModel
    @Environment(\.openWindow) private var openWindow
    let paper: Paper
    @State private var busy: String?
    @State private var newTag = ""
    @State private var note: String?

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                header
                actions
                if let note { Text(note).font(.callout).foregroundStyle(.secondary) }
                if !paper.meta.abstractText.isEmpty {
                    Card(title: "Abstract") {
                        Text(paper.meta.abstractText).textSelection(.enabled).lineSpacing(4)
                    }
                }
                Card(title: "Tags") {
                    FlowTags(paper: paper, newTag: $newTag)
                }
                Card(title: "Identifiers") { LinksRow(meta: paper.meta) }
            }
            .padding(22)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .onChange(of: paper.id) { _, _ in note = nil }
    }

    private var header: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 6) {
                if !paper.meta.journal.isEmpty { Pill(text: paper.meta.journal, color: .marginsTeal) }
                if !paper.meta.year.isEmpty { Pill(text: paper.meta.year, color: .secondary) }
                Spacer()
                CiteMenu(ids: [paper.id]).menuStyle(.borderlessButton).fixedSize()
                Button { model.toggleStar(paper) } label: {
                    Image(systemName: paper.starred ? "star.fill" : "star")
                        .foregroundStyle(paper.starred ? Color.marginsCopper : .secondary)
                }
                .buttonStyle(.borderless).help(paper.starred ? "Unstar" : "Star")
            }
            Text(paper.meta.title)
                .font(.system(.title2, design: .default).weight(.bold))
                .textSelection(.enabled)
                .fixedSize(horizontal: false, vertical: true)
            if !paper.meta.authors.isEmpty {
                Text(paper.meta.authors.joined(separator: ", "))
                    .font(.callout).foregroundStyle(.secondary).textSelection(.enabled).lineLimit(4)
            }
            let vol = [paper.meta.volume.isEmpty ? nil : "Vol. \(paper.meta.volume)\(paper.meta.issue.isEmpty ? "" : "(\(paper.meta.issue))")",
                       paper.meta.pages.isEmpty ? nil : "pp. \(paper.meta.pages)"].compactMap { $0 }
            if !vol.isEmpty { Text(vol.joined(separator: " · ")).font(.caption).foregroundStyle(.tertiary) }
        }
    }

    private var actions: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 8) {
                Button { openWindow(value: paper.id) } label: { Label("Read", systemImage: "book.fill") }
                    .buttonStyle(.borderedProminent).tint(.marginsTeal)
                    .disabled(!paper.hasEpub && !paper.hasPdf)
                if let pdf = model.fileURL(paper.id, .pdf) {
                    Button { NSWorkspace.shared.open(pdf) } label: { Label("PDF", systemImage: "doc.richtext") }
                } else {
                    Button { run("Searching open-access sources…") { await findPDF() } } label: {
                        Label("Find PDF", systemImage: "magnifyingglass")
                    }
                    .disabled(busy != nil)
                }
                if !paper.hasEpub && paper.hasPdf {
                    Button { run("Making reading view…") { _ = await model.makeReadingView(paper.id); model.reload() } } label: {
                        Label("Reading View", systemImage: "wand.and.stars")
                    }
                    .disabled(busy != nil)
                }
            }
            .controlSize(.large)
            .labelStyle(.titleAndIcon)
            .fixedSize()
            if let busy {
                HStack(spacing: 6) { ProgressView().controlSize(.small); Text(busy).font(.caption).foregroundStyle(.secondary) }
            }
        }
    }

    private func findPDF() async {
        if let source = await model.findPDF(paper.id) {
            note = "PDF found via \(source)."
        } else {
            note = "No open-access PDF found. Download it with your institution's access and drop it onto the window."
            if !paper.meta.doi.isEmpty, let u = URL(string: "https://doi.org/\(paper.meta.doi)") { NSWorkspace.shared.open(u) }
        }
    }

    private func run(_ label: String, _ op: @escaping () async -> Void) {
        busy = label
        Task { await op(); busy = nil }
    }
}

struct FlowTags: View {
    @EnvironmentObject var model: LibraryModel
    let paper: Paper
    @Binding var newTag: String
    var body: some View {
        HStack(spacing: 6) {
            ForEach(paper.tags, id: \.self) { t in
                Pill(text: t, icon: "tag", color: .marginsTeal)
                    .contextMenu { Button("Remove Tag") { model.setTag(paper, t, false) } }
            }
            TextField("Add tag", text: $newTag).textFieldStyle(.plain).frame(width: 110)
                .onSubmit { model.setTag(paper, newTag, true); newTag = "" }
        }
    }
}

struct LinksRow: View {
    let meta: PaperMeta
    var body: some View {
        HStack(spacing: 14) {
            if !meta.doi.isEmpty, let u = URL(string: "https://doi.org/\(meta.doi)") { Link("doi:\(meta.doi)", destination: u) }
            if !meta.pmid.isEmpty, let u = URL(string: "https://pubmed.ncbi.nlm.nih.gov/\(meta.pmid)") { Link("PubMed", destination: u) }
            if !meta.pmcid.isEmpty, let u = URL(string: "https://pmc.ncbi.nlm.nih.gov/articles/\(meta.pmcid)/") { Link("PMC", destination: u) }
            if meta.doi.isEmpty && meta.pmid.isEmpty && meta.pmcid.isEmpty { Text("None").foregroundStyle(.secondary) }
        }
        .font(.callout)
        .tint(.marginsTeal)
    }
}

struct MultiSelection: View {
    @EnvironmentObject var model: LibraryModel
    let ids: [String]
    var body: some View {
        VStack(spacing: 12) {
            Text("\(ids.count) papers selected").font(.title3)
            Button("Copy BibTeX") { copyToPasteboard(model.bibtex(ids)) }
            Button("Find PDFs") { Task { for id in ids { await model.findPDF(id) } } }
            Button("Make Reading Views") { Task { for id in ids { await model.makeReadingView(id) }; model.reload() } }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

struct StatusBar: View {
    @EnvironmentObject var model: LibraryModel
    var body: some View {
        if let s = model.status {
            HStack(spacing: 10) {
                if let p = model.progress { ProgressView(value: p).frame(width: 140) } else { ProgressView().controlSize(.small) }
                Text(s).font(.callout).foregroundStyle(.secondary).lineLimit(1)
                Spacer()
            }
            .padding(.horizontal, 12).padding(.vertical, 6)
            .background(.bar)
        }
    }
}

struct AddPaperSheet: View {
    @EnvironmentObject var model: LibraryModel
    @Environment(\.dismiss) private var dismiss
    @State private var input = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Add Paper").font(.headline)
            TextField("DOI, link, PMCID, PubMed or arXiv ID", text: $input)
                .textFieldStyle(.roundedBorder).frame(width: 440)
                .onSubmit(add)
            Text("You can also drag PDFs onto the window.").font(.caption).foregroundStyle(.secondary)
            HStack {
                Spacer()
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
                Button("Add", action: add).keyboardShortcut(.defaultAction).disabled(input.isEmpty)
            }
        }
        .padding(20)
    }

    private func add() {
        let i = input
        dismiss()
        Task { await model.add(i) }
    }
}

// MARK: - Discover

struct DiscoverList: View {
    @EnvironmentObject var model: LibraryModel
    @Binding var selection: String?

    var body: some View {
        VStack(spacing: 0) {
            BeyondBanner().padding(12)
            if model.discovering {
                ProgressView("Searching OpenAlex…").frame(maxHeight: .infinity)
            } else if let e = model.discoverError {
                ContentUnavailableView("Search failed", systemImage: "wifi.exclamationmark", description: Text(e))
            } else if model.discoverResults.isEmpty {
                ContentUnavailableView(model.discoverQuery.isEmpty ? "Search for new papers" : "No results",
                                       systemImage: "magnifyingglass",
                                       description: Text(model.discoverQuery.isEmpty ? "Type in the search field and press Return." : "Try other words."))
            } else {
                List(model.discoverResults, selection: $selection) { hit in
                    DiscoverRow(hit: hit).tag(hit.id)
                }
                .listStyle(.inset)
                .scrollContentBackground(.hidden)
            }
        }
        .background(Color.marginsTeal.opacity(0.04))
        .navigationTitle("Discover")
        .navigationSubtitle(model.discoverQuery.isEmpty ? "Beyond your library" : "\(model.discoverResults.count) results for “\(model.discoverQuery)”")
    }
}
