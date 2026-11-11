import SwiftUI
import MarginsCore

struct LibraryView: View {
    @EnvironmentObject var model: LibraryModel
    @State private var adding = false
    @State private var input = ""
    @State private var asking = false
    @State private var askPath: [String] = []
    @State private var path: [String] = []
    @State private var panel = false
    @State private var sharing: ShareTargetIOS?
    @State private var organizing: ShareTargetIOS?
    @State private var prompt: NameRequest?
    @StateObject private var ask = AskEngine(scope: .library)

    var body: some View {
        SidePanelHost(shown: $panel, edgeEnabled: path.isEmpty) {
            stack
        } panel: {
            SourcePanel { withAnimation(.spring(response: 0.32, dampingFraction: 0.9)) { panel = false } }
        }
    }

    private var stack: some View {
        NavigationStack(path: $path) {
            Group {
                if model.source == .discover { DiscoverListIOS() }
                else if model.source == .alerts { WatchInbox(selection: .constant(nil)).listStyle(.insetGrouped) }
                else { libraryList }
            }
            .navigationTitle(model.sourceTitle)
            .searchable(text: $model.query, prompt: model.source == .discover ? "Search new papers, then tap Search" : "Titles, authors, full text, notes")
            .onSubmit(of: .search) { if model.source == .discover { Task { await model.discover(model.query) } } }
            .navigationDestination(for: String.self) { id in ReaderView(paperID: id) }
            #if DEBUG
            .task {
                // testing in the Simulator: -openFirstPaper opens the first paper's reader
                guard ProcessInfo.processInfo.arguments.contains("-openFirstPaper") else { return }
                for _ in 0..<40 where model.papers.isEmpty { try? await Task.sleep(for: .milliseconds(250)) }
                if let p = model.papers.first { path = [p.id] }
            }
            #endif
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button { withAnimation(.spring(response: 0.32, dampingFraction: 0.9)) { panel = true } } label: {
                        Image(systemName: "sidebar.leading")
                    }
                    .overlay(alignment: .topTrailing) {
                        // something new from a watch
                        if WatchCenter.shared.newCount > 0 { Circle().fill(Color.marginsCopper).frame(width: 8, height: 8).offset(x: 3, y: -2) }
                    }
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Button { asking = true } label: { Image(systemName: "sparkles") }
                }
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
            .sheet(isPresented: $asking) {
                NavigationStack(path: $askPath) {
                    AssistantPanel(engine: ask, onPaper: { id in askPath.append(id) })
                        .navigationDestination(for: String.self) { id in ReaderView(paperID: id) }
                        .toolbar(.hidden, for: .navigationBar)
                }
                .environmentObject(model)
            }
            .sheet(item: $organizing) { t in OrganizeView(ids: [t.id]).environmentObject(model) }
            .namePrompt($prompt)
            .sheet(item: $sharing) { t in
                ScrollView { SharePanel(ids: [t.id]) }.environmentObject(model).presentationDetents([.medium, .large])
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
                    // only from the right: a swipe from the left edge opens the side panel
                    .swipeActions(edge: .trailing, allowsFullSwipe: false) {
                        Button { organizing = ShareTargetIOS(id: p.id) } label: { Label("Folders & Tags", systemImage: "folder") }
                            .tint(.marginsTeal)
                        Button { model.toggleStar(p) } label: { Label(p.starred ? "Unstar" : "Star", systemImage: p.starred ? "star.slash" : "star") }
                            .tint(.marginsCopper)
                        Button { model.setRead(p, !p.read) } label: { Label(p.read ? "Unread" : "Read", systemImage: p.read ? "circle" : "checkmark.circle") }
                            .tint(.gray)
                    }
                    .contextMenu {
                        Button(p.starred ? "Unstar" : "Star", systemImage: p.starred ? "star.slash" : "star") { model.toggleStar(p) }
                        Button(p.read ? "Mark as Unread" : "Mark as Read", systemImage: p.read ? "circle" : "checkmark.circle") { model.setRead(p, !p.read) }
                        Divider()
                        Button("Folders & Tags…", systemImage: "folder") { organizing = ShareTargetIOS(id: p.id) }
                        FolderMenu(ids: [p.id]) {
                            prompt = NameRequest(title: "New Folder", action: "Create") { model.newFolder($0, with: [p.id]) }
                        }
                        TagMenu(ids: [p.id]) {
                            prompt = NameRequest(title: "New Tag", action: "Add") { model.setTag([p.id], $0, true) }
                        }
                        if case .collection(let cid) = model.source {
                            Button("Remove from This Folder", systemImage: "folder.badge.minus") { model.setMembership([p.id], collection: cid, false) }
                        }
                        Button("Share…", systemImage: "square.and.arrow.up") { sharing = ShareTargetIOS(id: p.id) }
                        Menu("Watch", systemImage: "bell") { WatchMenu(paper: p) }
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
                    if paper.hasAudio { Pill(text: "Audio", icon: "waveform", color: .purple) }
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

struct ShareTargetIOS: Identifiable { let id: String }
