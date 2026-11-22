import SwiftUI
import LuntaCore

struct LibraryView: View {
    @EnvironmentObject var model: LibraryModel
    @ObservedObject private var nowPlaying = NowPlaying.shared
    @State private var adding = false
    @State private var input = ""
    @State private var asking = false
    @State private var askPath: [String] = []
    @State private var path = NavigationPath()
    @State private var opened = false
    @State private var reveal: String?
    @State private var deleting: Paper?
    @State private var sharing: ShareTargetIOS?
    @State private var organizing: ShareTargetIOS?
    @State private var prompt: NameRequest?
    @StateObject private var ask = AskEngine(scope: .library)

    // Mail-style: the first screen lists the places (all, starred, folders, …); each opens its
    // papers, and the back swipe from the left edge returns to the list. Opens on the last place.
    var body: some View {
        NavigationStack(path: $path) {
            LibraryHome()
                .navigationDestination(for: SidebarItem.self) { item in
                    papersScreen(item).onAppear { if model.source != item { model.source = item } }
                }
                .navigationDestination(for: String.self) { id in PaperInfoView(paperID: id, embedded: true, canRead: true) }
                .navigationDestination(for: ReadTarget.self) { t in ReaderView(paperID: t.id) }
        }
        .onReceive(NotificationCenter.default.publisher(for: .luntaReadPaper)) { n in
            if let id = n.object as? String, model.paper(id) != nil { path.append(ReadTarget(id: id)) }
        }
        .onReceive(MorningNotice.shared.$openForYou) { open in
            // the morning notification was tapped
            guard open else { return }
            MorningNotice.shared.openForYou = false
            model.source = .alerts
            var p = NavigationPath(); p.append(SidebarItem.alerts); path = p
        }
        .environment(\.showInLibrary) { id in
            // back to All Papers, scrolled to the paper and briefly highlighted
            model.query = ""
            model.source = .all
            var p = NavigationPath(); p.append(SidebarItem.all)
            path = p
            reveal = id
        }
        .onAppear {
            guard !opened else { return }
            opened = true
            path.append(model.source)
            // the paper you were reading when iOS closed Lunta in the background
            if let id = OpenReader.pending, model.paper(id) != nil { path.append(ReadTarget(id: id)) }
        }
        #if DEBUG
        .task {
            // testing in the Simulator: -openFirstPaper opens the first paper's reader
            let args = ProcessInfo.processInfo.arguments
            if args.contains("-openForYou") {
                try? await Task.sleep(for: .milliseconds(600))
                model.source = .alerts
                var p = NavigationPath(); p.append(SidebarItem.alerts); path = p
                return
            }
            if let i = args.firstIndex(of: "-openPaper"), args.indices.contains(i + 1) {   // -openPaper <id>
                for _ in 0..<40 where model.paper(args[i + 1]) == nil { try? await Task.sleep(for: .milliseconds(250)) }
                var p = NavigationPath(); p.append(model.source); p.append(ReadTarget(id: args[i + 1])); path = p
                return
            }
            guard args.contains("-openFirstPaper") || args.contains("-openFirstInfo") else { return }
            for _ in 0..<40 where model.papers.isEmpty { try? await Task.sleep(for: .milliseconds(250)) }
            guard let p = model.papers.first else { return }
            if args.contains("-openFirstInfo") { path.append(p.id) } else { path.append(ReadTarget(id: p.id)) }
        }
        #endif
        .safeAreaInset(edge: .bottom) {
            VStack(spacing: 0) {
                // a paper being read aloud, its reader left: Now Playing (readers show their own)
                if nowPlaying.readersShown == 0 { NowPlayingBar() }
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
        }
    }

    private func papersScreen(_ item: SidebarItem) -> some View {
            Group {
                if model.source == .discover { DiscoverListIOS() }
                else if model.source == .alerts { WatchInbox(selection: .constant(nil)).listStyle(.insetGrouped) }
                else { libraryList }
            }
            .navigationTitle(model.sourceTitle)
            // from the destination, not model.source: toggling .searchable after the push left an empty
            // search capsule at the bottom of New for You
            .modifier(LibrarySearch(enabled: item != .alerts))
            .onSubmit(of: .search) { if model.source == .discover { Task { await model.discover(model.query) } } }
            .toolbar {
                if model.source != .alerts && model.source != .discover {
                ToolbarItem(placement: .topBarTrailing) {
                    Menu {
                        Picker("Sort", selection: $model.sort) {
                            Text("Recently added").tag(SortKey.added)
                            Text("Year").tag(SortKey.year)
                            Text("Title").tag(SortKey.title)
                            Text("First author").tag(SortKey.firstAuthor)
                        }
                    } label: { Image(systemName: "arrow.up.arrow.down") }
                }
                ToolbarItem(placement: .topBarTrailing) { ListMoreMenu() }
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Button { asking = true } label: { Image(systemName: "sparkles") }
                }
                ToolbarItem(placement: .topBarTrailing) { AddMenu(adding: $adding) }
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
                TextField("DOI, PMCID or link (web pages too)", text: $input)
                    .textInputAutocapitalization(.never).autocorrectionDisabled()
                Button("Add") { let i = input; input = ""; Task { await model.add(i) } }
                Button("Cancel", role: .cancel) { input = "" }
            }
            .alert("Lunta", isPresented: Binding(get: { model.errorMessage != nil }, set: { if !$0 { model.errorMessage = nil } })) {
                Button("OK") { model.errorMessage = nil }
            } message: { Text(model.errorMessage ?? "") }
    }

    private var libraryList: some View {
          ScrollViewReader { proxy in
            List {
              ForEach(model.papers) { p in
                NavigationLink(value: p.id) { PaperRow(paper: p, snippet: model.snippets[p.id], updates: model.paperUpdates[p.id], inDrive: model.inDrive(p.id)) }
                    .id(p.id)
                    .listRowBackground(reveal == p.id ? Color.luntaTeal.opacity(0.18) : nil)
                    .modifier(PaperRowActions(paper: p, deleting: $deleting, organizing: $organizing, sharing: $sharing, prompt: $prompt))
              }
              if !model.query.trimmingCharacters(in: .whitespaces).isEmpty {
                  Button { model.searchBeyondLibrary() } label: {
                      Label("Search beyond your library for “\(model.query)”", systemImage: "globe.europe.africa")
                          .foregroundStyle(Color.luntaTeal)
                  }
              }
            }
            .listStyle(.plain)
            .refreshable { model.refresh() }
            .modifier(DeletePaperConfirm(paper: $deleting))
            .task(id: reveal) {
                guard let id = reveal else { return }
                for _ in 0..<20 where !model.papers.contains(where: { $0.id == id }) {
                    try? await Task.sleep(for: .milliseconds(100))
                }
                try? await Task.sleep(for: .milliseconds(350))   // let the pop finish
                withAnimation { proxy.scrollTo(id, anchor: .center) }
                try? await Task.sleep(for: .seconds(2.5))
                withAnimation { if reveal == id { reveal = nil } }
            }
            .overlay {
                if model.papers.isEmpty {
                    if !model.query.isEmpty {
                        ContentUnavailableView.search(text: model.query)
                    } else if model.counts.all == 0 {
                        ContentUnavailableView("No papers yet", systemImage: "books.vertical",
                                               description: Text(EmptyLibraryText.text(model, otherwise: "Tap + to add a paper by DOI or link, or to import your library from Zotero, Mendeley…")))
                    }
                }
            }
          }
    }
}


/// The first screen: every place papers can be listed from, like Mail's list of mailboxes.
struct LibraryHome: View {
    @EnvironmentObject var model: LibraryModel
    /// The iPad's sidebar selection; nil on the iPhone, where each place is pushed.
    var selection: Binding<SidebarItem?>? = nil
    @ObservedObject private var center = WatchCenter.shared
    @State private var managing = false
    @State private var cleaning = false
    @State private var newSmart = false
    @State private var editingSmart: SmartFolder?
    @State private var prompt: NameRequest?
    @State private var storage = false

    var body: some View {
        Group {
            if let selection {
                List(selection: selection) { places }.listStyle(.sidebar)
            } else {
                List { places }.listStyle(.insetGrouped)
            }
        }
        .navigationTitle("Lunta")
        .namePrompt($prompt)
        .sheet(isPresented: $managing) { ManageFoldersView().environmentObject(model) }
        .sheet(isPresented: $storage) { LibraryStorageSheet().environmentObject(model) }
        .sheet(isPresented: $newSmart) { SmartFolderEditor(folder: SmartFolder(), isNew: true).environmentObject(model) }
        .sheet(item: $editingSmart) { f in SmartFolderEditor(folder: f).environmentObject(model) }
        .sheet(isPresented: $cleaning) {
            NavigationStack {
                CleanupView().toolbar { ToolbarItem(placement: .confirmationAction) { Button("Done") { cleaning = false } } }
            }
            .environmentObject(model)
        }
    }

    @ViewBuilder private var places: some View {
        Section {
            place(.all, "All Papers", "books.vertical", count: model.counts.all)
            place(.starred, "Starred", "star", count: model.counts.starred)
            place(.mustRead, "Must Read", "bookmark", count: model.counts.mustRead)
            place(.unread, "Unread", "smallcircle.filled.circle", count: model.counts.unread)
        }
        Section {
            place(.alerts, "New for You", "bell.badge", count: center.newCount, badge: true)
            place(.discover, "Discover New Papers", "globe.europe.africa")
        }
        Section("Folders") {
            ForEach(model.foldersFlat, id: \.folder.id) { item in
                place(.collection(item.folder.id), item.folder.name, item.depth == 0 ? "folder" : "folder.fill", indent: item.depth)
                    .contextMenu {
                        Button("New Subfolder…", systemImage: "folder.badge.plus") {
                            prompt = NameRequest(title: "New Folder", action: "Create") { _ = model.newFolder($0, parent: item.folder.id) }
                        }
                        Button("Rename…", systemImage: "pencil") {
                            prompt = NameRequest(title: "Rename Folder", initial: item.folder.name, action: "Rename") { model.renameFolder(item.folder.id, $0) }
                        }
                        Button("Outline from Highlights…", systemImage: "list.bullet.indent") {
                            OutlineUI.shared.open(model.outlineIDs(folder: item.folder.id), scope: "“\(item.folder.name)”")
                        }
                        Button("Delete Folder", systemImage: "trash", role: .destructive) { model.deleteFolder(item.folder.id) }
                    }
            }
            Button {
                prompt = NameRequest(title: "New Folder", action: "Create") { _ = model.newFolder($0) }
            } label: { Label("New Folder", systemImage: "plus").foregroundStyle(Color.luntaTeal) }
        }
        Section("Smart Folders") {
            ForEach(model.smartFolders) { f in
                place(.smart(f.id), f.name, "gearshape.2")
                    .contextMenu {
                        Button("Edit…", systemImage: "slider.horizontal.3") { editingSmart = f }
                        Button("Delete", systemImage: "trash", role: .destructive) { model.deleteSmartFolder(f.id) }
                    }
            }
            Button { newSmart = true } label: { Label("New Smart Folder", systemImage: "plus").foregroundStyle(Color.luntaTeal) }
        }
        if !model.tags.isEmpty {
            Section("Tags") {
                ForEach(model.tags, id: \.self) { t in
                    place(.tag(t), t, "tag")
                        .contextMenu {
                            Button("Outline from Highlights…", systemImage: "list.bullet.indent") {
                                OutlineUI.shared.open(model.outlineIDs(tag: t), scope: "the tag “\(t)”")
                            }
                        }
                }
            }
        }
        Section {
            Button { managing = true } label: { Label("Manage Folders & Tags…", systemImage: "folder.badge.gearshape") }
            Button { cleaning = true } label: { Label("Clean Up Library…", systemImage: "wand.and.sparkles") }
            Menu {
                ImportMenuItems()
            } label: { Label("Import Library…", systemImage: "square.and.arrow.down.on.square") }
            AppearancePicker().pickerStyle(.menu)
            Button { storage = true } label: {
                Label {
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Library Storage…")
                        if LibraryStorage.stored == .googleDrive {
                            DriveSyncSummary().font(.caption).foregroundStyle(.secondary)
                        } else {
                            Text(LibraryLocation.current(inCloud: model.inCloud).name).font(.caption).foregroundStyle(.secondary)
                        }
                    }
                } icon: { Image(systemName: StorageText.icon(LibraryLocation.current(inCloud: model.inCloud))) }
            }
        }
    }

    @ViewBuilder private func place(_ item: SidebarItem, _ title: String, _ icon: String, count: Int? = nil,
                                    indent: Int = 0, badge: Bool = false) -> some View {
        if selection != nil {
            placeLabel(title, icon, count: count, indent: indent, badge: badge).tag(item)
        } else {
            NavigationLink(value: item) { placeLabel(title, icon, count: count, indent: indent, badge: badge) }
        }
    }

    private func placeLabel(_ title: String, _ icon: String, count: Int?, indent: Int, badge: Bool) -> some View {
        HStack {
            Label {
                Text(title)
            } icon: {
                Image(systemName: icon).foregroundStyle(Color.luntaTeal)
            }
            .padding(.leading, CGFloat(indent) * 18)
            Spacer()
            if let count, count > 0 {
                if badge {
                    Text("\(count)").font(.footnote.weight(.semibold).monospacedDigit()).foregroundStyle(.white)
                        .padding(.horizontal, 7).padding(.vertical, 2)
                        .background(Capsule().fill(Color.luntaTeal))
                } else {
                    Text("\(count)").font(.callout.monospacedDigit()).foregroundStyle(.secondary)
                }
            }
        }
    }
}

struct PaperRow: View {
    let paper: Paper
    var snippet: String?
    /// What the checks found (retracted, corrected, published): a badge.
    var updates: PaperUpdates? = nil
    /// A Google Drive library synced on demand: the files that are in Drive and not on this device
    /// (their pills get a cloud).
    var inDrive: Set<FileKind> = []

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            Group {
                if paper.mustRead {
                    Image(systemName: "bookmark.fill").font(.caption2).foregroundStyle(Color.luntaTeal)
                } else if paper.starred {
                    Image(systemName: "star.fill").font(.caption2).foregroundStyle(Color.luntaCopper)
                } else if !paper.read {
                    Circle().fill(Color.luntaTeal).frame(width: 8, height: 8)
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
                    UpdatesPill(updates: updates)
                    if paper.hasEpub { Pill(text: "Read", icon: inDrive.contains(.epub) ? Self.cloud : "book", color: .luntaTeal) }
                    if paper.hasPdf { Pill(text: "PDF", icon: inDrive.contains(.pdf) ? Self.cloud : nil, color: .luntaCopper) }
                    if paper.hasAudio { Pill(text: "Audio", icon: inDrive.contains(.audio) ? Self.cloud : "waveform", color: .purple) }
                    if paper.annotationCount > 0 { Pill(text: "\(paper.annotationCount)", icon: "highlighter", color: .secondary) }
                }
            }
        }
        .padding(.vertical, 4)
        .accessibilityHint(inDrive.isEmpty ? "" : "In Google Drive, downloaded when opened")
    }

    static let cloud = PaperRowCloud.icon
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
        .background(Color.luntaTeal.opacity(0.05))
    }
}

struct ShareTargetIOS: Identifiable { let id: String }

/// The swipe actions and context menu of a paper in the list (iPhone and iPad).
struct PaperRowActions: ViewModifier {
    @EnvironmentObject var model: LibraryModel
    let paper: Paper
    @Binding var deleting: Paper?
    @Binding var organizing: ShareTargetIOS?
    @Binding var sharing: ShareTargetIOS?
    @Binding var prompt: NameRequest?

    func body(content: Content) -> some View {
        let p = paper
        content
            // only from the right, so a swipe in from the left can't change a paper by accident
            .swipeActions(edge: .trailing, allowsFullSwipe: false) {
                Button { deleting = p } label: { Label("Delete", systemImage: "trash") }
                    .tint(.red)
                Button { organizing = ShareTargetIOS(id: p.id) } label: { Label("Folders & Tags", systemImage: "folder") }
                    .tint(.luntaTeal)
                Button { model.toggleStar(p) } label: { Label(p.starred ? "Unstar" : "Star", systemImage: p.starred ? "star.slash" : "star") }
                    .tint(.luntaCopper)
                Button { model.toggleMustRead(p) } label: { Label(p.mustRead ? "Not Must Read" : "Must Read", systemImage: p.mustRead ? "bookmark.slash" : "bookmark") }
                    .tint(.luntaTeal)
                Button { model.setRead(p, !p.read) } label: { Label(p.read ? "Unread" : "Read", systemImage: p.read ? "circle" : "checkmark.circle") }
                    .tint(.gray)
            }
            .contextMenu {
                Button(p.starred ? "Unstar" : "Star", systemImage: p.starred ? "star.slash" : "star") { model.toggleStar(p) }
                Button(p.mustRead ? "Remove from Must Read" : "Must Read", systemImage: p.mustRead ? "bookmark.slash" : "bookmark") { model.toggleMustRead(p) }
                Button(p.read ? "Mark as Unread" : "Mark as Read", systemImage: p.read ? "circle" : "checkmark.circle") { model.setRead(p, !p.read) }
                Divider()
                Button("Folders & Tags…", systemImage: "folder") { organizing = ShareTargetIOS(id: p.id) }
                Button("Outline from Highlights…", systemImage: "list.bullet.indent") { OutlineUI.shared.open([p.id], scope: "this paper") }
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
                Button("Copy \\cite", systemImage: "doc.on.doc") { model.copyLatexCite([p.id]) }
                DownloadMenuItems(ids: [p.id])
                Menu("Watch", systemImage: "bell") { WatchMenu(paper: p) }
                Menu("Check", systemImage: "checkmark.seal") {
                    Button("Retractions and Published Version", systemImage: "exclamationmark.shield") { Task { await model.checkUpdates([p.id]) } }
                    if p.hasEpub || p.hasPdf {
                        Button("This Paper's References", systemImage: "list.number") { Task { await model.checkPaperReferences(p.id) } }
                    }
                }
                Divider()
                Button("Delete Paper…", systemImage: "trash", role: .destructive) { deleting = p }
            }
    }
}

/// The library's search field (not on New for You, which is a feed).
struct LibrarySearch: ViewModifier {
    @EnvironmentObject var model: LibraryModel
    let enabled: Bool
    func body(content: Content) -> some View {
        if enabled {
            content.searchable(text: $model.query, prompt: model.source == .discover ? "Search new papers, then tap Search" : "Titles, authors, full text, notes")
                .modifier(RecentSearchSuggestions())
                .onSubmit(of: .search) { if model.source != .discover { RecentSearches.shared.remember(model.query) } }
        } else {
            content
        }
    }
}
