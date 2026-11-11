import SwiftUI
import MarginsCore

struct LibraryView: View {
    @EnvironmentObject var model: LibraryModel
    @State private var adding = false
    @State private var input = ""

    var body: some View {
        NavigationStack {
            Group {
                if model.source == .discover { DiscoverListIOS() } else { libraryList }
            }
            .navigationTitle(model.sourceTitle)
            .searchable(text: $model.query, prompt: model.source == .discover ? "Search new papers, then tap Search" : "Titles, authors, full text, notes")
            .onSubmit(of: .search) { if model.source == .discover { Task { await model.discover(model.query) } } }
            .navigationDestination(for: String.self) { id in ReaderView(paperID: id) }
            .toolbar {
                ToolbarItem(placement: .topBarLeading) { SourceMenu() }
                ToolbarItem(placement: .topBarTrailing) {
                    Button { adding = true } label: { Image(systemName: "plus") }
                }
            }
            .safeAreaInset(edge: .bottom) {
                if let s = model.status {
                    HStack(spacing: 8) {
                        ProgressView().controlSize(.small)
                        Text(s).font(.footnote).foregroundStyle(.secondary).lineLimit(1)
                    }
                    .padding(.horizontal).padding(.vertical, 8)
                    .frame(maxWidth: .infinity)
                    .background(.bar)
                }
            }
            .alert("Add Paper", isPresented: $adding) {
                TextField("DOI, link or PMCID", text: $input)
                    .textInputAutocapitalization(.never).autocorrectionDisabled()
                Button("Add") { let i = input; input = ""; Task { await model.add(i) } }
                Button("Cancel", role: .cancel) { input = "" }
            }
            .alert("Margins", isPresented: Binding(get: { model.errorMessage != nil }, set: { if !$0 { model.errorMessage = nil } })) {
                Button("OK") { model.errorMessage = nil }
            } message: { Text(model.errorMessage ?? "") }
        }
    }

    private var libraryList: some View {
            List {
              ForEach(model.papers) { p in
                NavigationLink(value: p.id) { PaperRow(paper: p, snippet: model.snippets[p.id]) }
                    .swipeActions(edge: .leading) {
                        Button { model.toggleStar(p) } label: { Label(p.starred ? "Unstar" : "Star", systemImage: p.starred ? "star.slash" : "star") }
                            .tint(.marginsCopper)
                        Button { model.setRead(p, !p.read) } label: { Label(p.read ? "Unread" : "Read", systemImage: p.read ? "circle" : "checkmark.circle") }
                            .tint(.marginsTeal)
                    }
              }
              if !model.query.trimmingCharacters(in: .whitespaces).isEmpty {
                  Button { model.searchBeyondLibrary() } label: {
                      Label("Search beyond your library for “\(model.query)”", systemImage: "globe.europe.africa")
                          .foregroundStyle(Color.marginsTeal)
                  }
              }
            }
            .listStyle(.plain)
            .refreshable { model.refresh() }
            .overlay {
                if model.papers.isEmpty {
                    if !model.query.isEmpty {
                        ContentUnavailableView.search(text: model.query)
                    } else if model.counts.all == 0 {
                        ContentUnavailableView("No papers yet", systemImage: "books.vertical",
                                               description: Text(model.inCloud ? "Your library is syncing from iCloud…" : "Tap + to add a paper by DOI or link."))
                    }
                }
            }
    }
}

struct SourceMenu: View {
    @EnvironmentObject var model: LibraryModel
    var body: some View {
        Menu {
            Picker("Show", selection: $model.source) {
                Label("All Papers", systemImage: "books.vertical").tag(SidebarItem.all)
                Label("Starred", systemImage: "star").tag(SidebarItem.starred)
                Label("Unread", systemImage: "circle.badge").tag(SidebarItem.unread)
                Label("Discover New Papers", systemImage: "globe.europe.africa").tag(SidebarItem.discover)
            }
            if !model.collections.isEmpty {
                Picker("Collections", selection: $model.source) {
                    ForEach(model.collections) { c in Label(c.name, systemImage: "folder").tag(SidebarItem.collection(c.id)) }
                }
            }
            if !model.tags.isEmpty {
                Menu("Tags") {
                    Picker("Tags", selection: $model.source) {
                        ForEach(model.tags, id: \.self) { t in Label(t, systemImage: "tag").tag(SidebarItem.tag(t)) }
                    }
                }
            }
            Divider()
            Picker("Sort", selection: $model.sort) {
                Text("Recently added").tag(SortKey.added)
                Text("Year").tag(SortKey.year)
                Text("Title").tag(SortKey.title)
                Text("First author").tag(SortKey.firstAuthor)
            }
        } label: {
            Image(systemName: "line.3.horizontal.decrease.circle")
        }
    }
}

struct PaperRow: View {
    let paper: Paper
    var snippet: String?

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            Group {
                if paper.starred {
                    Image(systemName: "star.fill").font(.caption2).foregroundStyle(Color.marginsCopper)
                } else if !paper.read {
                    Circle().fill(Color.marginsTeal).frame(width: 8, height: 8)
                } else {
                    Color.clear.frame(width: 8, height: 8)
                }
            }
            .frame(width: 12).padding(.top, 6)
            VStack(alignment: .leading, spacing: 4) {
                Text(paper.meta.title.isEmpty ? "Untitled" : paper.meta.title)
                    .font(.body.weight(paper.read ? .regular : .semibold))
                    .lineLimit(3)
                Text([paper.meta.authorLine, paper.meta.journal, paper.meta.year].filter { !$0.isEmpty }.joined(separator: " · "))
                    .font(.footnote).foregroundStyle(.secondary).lineLimit(1)
                if let snippet, !snippet.isEmpty {
                    Text(snippet.replacingOccurrences(of: "[", with: "").replacingOccurrences(of: "]", with: ""))
                        .font(.caption).foregroundStyle(.secondary).lineLimit(2)
                }
                HStack(spacing: 4) {
                    if paper.hasEpub { Pill(text: "Read", icon: "book", color: .marginsTeal) }
                    if paper.hasPdf { Pill(text: "PDF", color: .marginsCopper) }
                    if paper.annotationCount > 0 { Pill(text: "\(paper.annotationCount)", icon: "highlighter", color: .secondary) }
                }
            }
        }
        .padding(.vertical, 4)
    }
}

struct DiscoverListIOS: View {
    @EnvironmentObject var model: LibraryModel
    var body: some View {
        List {
            Section { BeyondBanner().listRowInsets(EdgeInsets()).listRowBackground(Color.clear) }
            if model.discovering {
                HStack { Spacer(); ProgressView("Searching OpenAlex…"); Spacer() }
            } else if let e = model.discoverError {
                Text(e).foregroundStyle(.secondary)
            } else if model.discoverResults.isEmpty {
                Text(model.discoverQuery.isEmpty ? "Type in the search field and tap Search." : "No results. Try other words.")
                    .foregroundStyle(.secondary)
            } else {
                ForEach(model.discoverResults) { hit in
                    NavigationLink { DiscoverDetail(hit: hit) } label: { DiscoverRow(hit: hit) }
                }
            }
        }
        .listStyle(.insetGrouped)
        .scrollContentBackground(.hidden)
        .background(Color.marginsTeal.opacity(0.05))
    }
}
