import SwiftUI
import LuntaCore
import UniformTypeIdentifiers

/// The library on iPhone or iPad: on a regular-width iPad window, the sidebar, the paper list and
/// the reader side by side (like the Mac's library window, with the reader in the third column);
/// on the iPhone and in narrow iPad windows (Split View, Slide Over), the iPhone's screens.
struct AdaptiveLibrary: View {
    @Environment(\.horizontalSizeClass) private var sizeClass

    var body: some View {
        // the idiom too: a Pro Max iPhone in landscape is also regular width
        Group {
            if UIDevice.current.userInterfaceIdiom == .pad, sizeClass == .regular {
                iPadLibrary()
            } else {
                LibraryView()
            }
        }
        .libraryImport()   // import from Zotero, Mendeley… (+ › Import Library)
    }
}

/// iPad: places in the sidebar, the papers in the middle, the reader on the right. The reader
/// can take the whole screen (⌃⌘F, or the button at the top left of the reader).
struct iPadLibrary: View {
    @EnvironmentObject var model: LibraryModel
    @ObservedObject private var nowPlaying = NowPlaying.shared
    @State private var columns: NavigationSplitViewVisibility = .all
    @State private var selected: String?
    @State private var reveal: String?
    @State private var opened = false
    @State private var narrow = false
    @State private var adding = false
    @State private var input = ""
    @State private var asking = false
    @State private var deleting: Paper?
    @State private var sharing: ShareTargetIOS?
    @State private var organizing: ShareTargetIOS?
    @State private var prompt: NameRequest?
    @State private var dropping = false
    @FocusState private var searching: Bool
    @StateObject private var ask = AskEngine(scope: .library)

    var body: some View {
        NavigationSplitView(columnVisibility: $columns) {
            LibraryHome(selection: Binding(get: { model.source }, set: { s in
                if let s, s != model.source { model.source = s }
            }))
            .navigationSplitViewColumnWidth(min: 220, ideal: 260, max: 320)
        } content: {
            papersColumn
                .navigationSplitViewColumnWidth(min: 320, ideal: 380, max: 480)
        } detail: {
            NavigationStack { detail }
                .inspector(isPresented: $asking) {
                    AssistantPanel(engine: ask, onPaper: { id in open(id) })
                        .environmentObject(model)
                        .inspectorColumnWidth(min: 300, ideal: 360, max: 480)
                }
        }
        // landscape: all three columns; portrait (or a smaller window): the list and the reader,
        // the sidebar a tap away. Reading full screen stays full screen.
        .onGeometryChange(for: Bool.self) { $0.size.width < 1100 } action: { isNarrow in
            narrow = isNarrow
            // after the split view's first layout: set during it, the change is lost
            Task { @MainActor in
                if columns != .detailOnly { columns = isNarrow ? .doubleColumn : .all }
            }
        }
        .onReceive(NotificationCenter.default.publisher(for: .luntaReadPaper)) { n in
            if let id = n.object as? String, model.paper(id) != nil { selected = id }
        }
        .onReceive(MorningNotice.shared.$openForYou) { open in
            // the morning notification was tapped
            guard open else { return }
            MorningNotice.shared.openForYou = false
            model.source = .alerts
            if columns == .detailOnly { withAnimation { columns = narrow ? .doubleColumn : .all } }
        }
        .environment(\.showInLibrary) { id in open(id, reveal: true) }
        .onAppear {
            guard !opened else { return }
            opened = true
            // the paper you were reading when iOS closed Lunta in the background
            if let id = OpenReader.pending, model.paper(id) != nil { selected = id }
        }
        #if DEBUG
        .task { await debugLaunch() }
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
        .alert("Add Paper", isPresented: $adding) {
            TextField("DOI, link or PMCID", text: $input)
                .textInputAutocapitalization(.never).autocorrectionDisabled()
            Button("Add") {
                let i = input; input = ""
                Task { if let id = await model.add(i) { open(id) } }
            }
            Button("Cancel", role: .cancel) { input = "" }
        }
        .alert("Lunta", isPresented: Binding(get: { model.errorMessage != nil }, set: { if !$0 { model.errorMessage = nil } })) {
            Button("OK") { model.errorMessage = nil }
        } message: { Text(model.errorMessage ?? "") }
        .sheet(item: $organizing) { t in OrganizeView(ids: [t.id]).environmentObject(model) }
        .sheet(item: $sharing) { t in
            ScrollView { SharePanel(ids: [t.id]) }.environmentObject(model).presentationDetents([.medium, .large])
        }
        .namePrompt($prompt)
    }

    // MARK: - Papers

    private var papersColumn: some View {
        Group {
            if model.source == .discover { DiscoverListIOS() }
            else if model.source == .alerts { WatchInbox(selection: .constant(nil)).listStyle(.insetGrouped) }
            else { paperList }
        }
        .navigationTitle(model.sourceTitle)
        .navigationBarTitleDisplayMode(.inline)
        .modifier(LibrarySearch(enabled: model.source != .alerts))
        .searchFocused($searching)
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
            }
            ToolbarItem(placement: .topBarTrailing) {
                Button { asking.toggle() } label: { Label("Ask", systemImage: "sparkles") }
                    .keyboardShortcut("j", modifiers: .command)
            }
            ToolbarItem(placement: .topBarTrailing) { AddMenu(adding: $adding) }
        }
        // ⌘F: to the search field (a button the keyboard can reach, not seen). With a paper open,
        // ⌘F finds in the paper (ReaderView) and the library's search is ⌥⌘F.
        .background {
            Button("Search Library") { if columns == .detailOnly { withAnimation { columns = narrow ? .doubleColumn : .all } }; searching = true }
                .keyboardShortcut("f", modifiers: selected.flatMap(model.paper) == nil ? .command : [.command, .option])
                .opacity(0).allowsHitTesting(false).accessibilityHidden(true)
            // ⌘N and ⇧⌘I: Add Paper and Import Library, which are in the + menu
            Button("Add Paper") { adding = true }
                .keyboardShortcut("n", modifiers: .command)
                .opacity(0).allowsHitTesting(false).accessibilityHidden(true)
            Button("Import Library") { LibraryImporter.shared.start(.library) }
                .keyboardShortcut("i", modifiers: [.command, .shift])
                .opacity(0).allowsHitTesting(false).accessibilityHidden(true)
        }
        // PDFs dragged in from Files or another app
        .onDrop(of: [.pdf, .fileURL], isTargeted: $dropping) { providers in
            Task {
                let urls = await Self.pdfURLs(providers)
                guard !urls.isEmpty else { model.errorMessage = "Only PDF files can be dropped here."; return }
                let out = await model.addPDFs(urls)
                if let id = out.first(where: { $0.paperID != nil })?.paperID { open(id, reveal: true) }
            }
            return true
        }
        .overlay {
            if dropping {
                RoundedRectangle(cornerRadius: 16)
                    .strokeBorder(Color.luntaTeal, style: StrokeStyle(lineWidth: 3, dash: [8, 6]))
                    .background(Color.luntaTeal.opacity(0.08), in: RoundedRectangle(cornerRadius: 16))
                    .overlay { Label("Add PDF to Lunta", systemImage: "doc.badge.plus").font(.headline).foregroundStyle(Color.luntaTeal) }
                    .padding(10)
                    .allowsHitTesting(false)
            }
        }
        .animation(.easeOut(duration: 0.18), value: dropping)
    }

    private var paperList: some View {
        ScrollViewReader { proxy in
            List(selection: $selected) {
                ForEach(model.papers) { p in
                    PaperRow(paper: p, snippet: model.snippets[p.id])
                        .tag(p.id)
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
            .onChange(of: model.papers.map(\.id)) { _, ids in
                // a deleted paper leaves the reader
                if let id = selected, model.paper(id) == nil, !ids.contains(id) { selected = nil }
            }
            .task(id: reveal) {
                guard let id = reveal else { return }
                for _ in 0..<20 where !model.papers.contains(where: { $0.id == id }) {
                    try? await Task.sleep(for: .milliseconds(100))
                }
                try? await Task.sleep(for: .milliseconds(200))
                withAnimation { proxy.scrollTo(id, anchor: .center) }
                try? await Task.sleep(for: .seconds(2.5))
                withAnimation { if reveal == id { reveal = nil } }
            }
            .task(id: selected) {
                // keep the paper being read in view (arrow keys, Ask, a paper added)
                guard let id = selected, columns != .detailOnly else { return }
                try? await Task.sleep(for: .milliseconds(150))
                if model.papers.contains(where: { $0.id == id }) { proxy.scrollTo(id) }
            }
            .overlay {
                if model.papers.isEmpty {
                    if !model.query.isEmpty {
                        ContentUnavailableView.search(text: model.query)
                    } else if model.counts.all == 0 {
                        ContentUnavailableView("No papers yet", systemImage: "books.vertical",
                                               description: Text(model.inCloud ? "Your library is syncing from iCloud…" : "Tap + to add a paper by DOI or link or to import your library from Zotero, Mendeley…, or drop a PDF here."))
                    }
                }
            }
        }
    }

    // MARK: - Reader

    @ViewBuilder private var detail: some View {
        if model.source == .alerts {
            ContentUnavailableView("New for You", systemImage: "sparkles",
                                   description: Text("Pick a paper to see what it's about and why it fits your research."))
        } else if model.source == .discover {
            ContentUnavailableView("Discover New Papers", systemImage: "globe.europe.africa",
                                   description: Text("Search for papers beyond your library, then pick one."))
        } else if let id = selected, model.paper(id) != nil {
            ReaderView(paperID: id)
                .id(id)
                .toolbar {
                    ToolbarItem(placement: .topBarLeading) {
                        Button { toggleFullScreen() } label: {
                            Label(columns == .detailOnly ? "Show Library" : "Read Full Screen",
                                  systemImage: columns == .detailOnly ? "arrow.down.right.and.arrow.up.left" : "arrow.up.left.and.arrow.down.right")
                        }
                        .keyboardShortcut("f", modifiers: [.command, .control])
                    }
                }
        } else {
            ContentUnavailableView("No paper selected", systemImage: "doc.text",
                                   description: Text("Pick a paper to read it here. Add papers with ⌘N, or drop PDFs on the list."))
        }
    }

    private func toggleFullScreen() {
        withAnimation { columns = columns == .detailOnly ? (narrow ? .doubleColumn : .all) : .detailOnly }
    }

    /// Opens a paper in the reader, from All Papers if it isn't in the place being shown.
    private func open(_ id: String, reveal r: Bool = false) {
        if model.source == .discover || model.source == .alerts || !model.papers.contains(where: { $0.id == id }) {
            model.query = ""
            model.source = .all
        }
        selected = id
        if r { reveal = id }
    }

    #if DEBUG
    /// Simulator testing: -openPaper <id>, -searchLibrary <query>, -openFirstPaper, -openForYou, -fullScreen, -allColumns.
    private func debugLaunch() async {
        let args = ProcessInfo.processInfo.arguments
        if args.contains("-openForYou") {
            try? await Task.sleep(for: .milliseconds(600))
            model.source = .alerts
            return
        }
        var id: String?
        if let i = args.firstIndex(of: "-openPaper"), args.indices.contains(i + 1) {
            for _ in 0..<40 where model.paper(args[i + 1]) == nil { try? await Task.sleep(for: .milliseconds(250)) }
            id = args[i + 1]
        } else if let i = args.firstIndex(of: "-searchLibrary"), args.indices.contains(i + 1) {   // a library search, then its first paper
            try? await Task.sleep(for: .milliseconds(800))
            model.query = args[i + 1]
            for _ in 0..<40 where model.snippets.isEmpty { try? await Task.sleep(for: .milliseconds(250)) }
            id = model.papers.first?.id
        } else if args.contains("-openFirstPaper") {
            for _ in 0..<40 where model.papers.isEmpty { try? await Task.sleep(for: .milliseconds(250)) }
            id = model.papers.first?.id
        }
        if let id { selected = id }
        if args.contains("-fullScreen") { try? await Task.sleep(for: .milliseconds(300)); columns = .detailOnly }
        if args.contains("-allColumns") { try? await Task.sleep(for: .milliseconds(300)); columns = .all }
    }
    #endif

    /// The PDFs among dropped items, as local files Lunta can read.
    static func pdfURLs(_ providers: [NSItemProvider]) async -> [URL] {
        var out: [URL] = []
        for p in providers {
            let type = p.hasItemConformingToTypeIdentifier(UTType.pdf.identifier) ? UTType.pdf
                : p.hasItemConformingToTypeIdentifier(UTType.fileURL.identifier) ? UTType.fileURL : nil
            guard let type else { continue }
            // the file is only there while the callback runs: copy it out
            let copy: URL? = await withCheckedContinuation { c in
                _ = p.loadFileRepresentation(forTypeIdentifier: type.identifier) { u, _ in
                    guard let u, type == .pdf || u.pathExtension.lowercased() == "pdf" else { c.resume(returning: nil); return }
                    let name = p.suggestedName.map { $0.hasSuffix(".pdf") ? $0 : $0 + ".pdf" } ?? u.lastPathComponent
                    let dir = FileManager.default.temporaryDirectory.appendingPathComponent("drop-\(UUID().uuidString)")
                    try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
                    let dest = dir.appendingPathComponent(name)
                    c.resume(returning: (try? FileManager.default.copyItem(at: u, to: dest)) != nil ? dest : nil)
                }
            }
            if let copy { out.append(copy) }
        }
        return out
    }
}
