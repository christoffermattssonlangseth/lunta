import SwiftUI
import WebKit
import PDFKit
import LuntaCore

/// Full-screen reader: the continuous reading view (reflowed text) or the original PDF.
struct ReaderView: View {
    @EnvironmentObject var model: LibraryModel
    @Environment(\.undoManager) private var undoManager
    let paperID: String
    @State private var mode: Mode = .reading
    @State private var book: EpubBook?
    /// A book read chapter by chapter (too big for one page: ChapterReader.swift); nil for a paper.
    @State private var chapters: ChapterReader?
    @State private var bookFind: BookFind?
    @State private var pdfURL: URL?
    @State private var loading = true
    @State private var message: String?
    /// The paper's file couldn't be downloaded (offline, say): why, with Try Again.
    @State private var downloadProblem: String?
    @State private var showContents = false
    @State private var showReferences = false
    @StateObject private var references = ReferencesModel()
    @State private var showAppearance = false
    @State private var showInfo = false
    /// Outline from Highlights of this paper (shown over the reader: the library's sheet is behind it).
    @State private var outline: OutlineRequest?
    @AppStorage("readerFontScale") private var fontScale = 1.0
    @AppStorage("readerPaged") private var paged = false
    @AppStorage("readerTheme") private var themeRaw = ReaderTheme.automatic.rawValue
    private var theme: ReaderTheme { ReaderTheme(rawValue: themeRaw) ?? .automatic }
    @AppStorage("highlightMode") private var highlightMode = false
    @AppStorage("highlightModeColor") private var highlightModeColor = HighlightColor.yellow.rawValue
    @State private var autoHighlighter = AutoHighlighter()
    @State private var figure: ReaderFigure?
    @State private var showFigures = false
    @State private var sharing = false
    @State private var organizing = false
    @State private var editing = false
    @State private var chromeHidden = false
    @State private var editingAnnotation: Annotation?
    /// Extend… / Highlight to… : waiting for the tap that ends the passage.
    @State private var extending: Extending?
    struct Extending: Equatable {
        /// The highlight being extended; nil: a new one from the selection (Highlight to…).
        var id: String?
        var color: HighlightColor
        /// The bars as they were (in pages they make way for the hint).
        var chromeWasHidden: Bool
    }
    @StateObject private var pdfHighlights = PDFHighlighter()
    @StateObject private var find = PaperFind()
    @State private var goToPage = 0
    @State private var pdfFinder = PDFFinder()
    @State private var showHighlights = false
    @State private var current: String?
    @State private var scrollRequest = ScrollRequest()
    @StateObject private var narrator = Narrator()
    @ObservedObject private var nowPlaying = NowPlaying.shared
    @StateObject private var controller = ReaderController()
    @StateObject private var position = PositionKeeper()
    @StateObject private var summary = SummaryMaker()
    @AppStorage(SummaryMaker.key) private var summariesOn = true
    @Environment(\.scenePhase) private var phase
    @Environment(\.horizontalSizeClass) private var sizeClass
    @State private var listening = false
    @State private var asking = false
    @StateObject private var ask: AskEngine
    /// Remake Reading View…: waiting for Remake; while it runs, what it is doing; then how it went.
    @State private var remakeRequest: RemakeRequest?
    @State private var remaking: String?
    @State private var remakeNote: String?

    init(paperID: String) {
        self.paperID = paperID
        _ask = StateObject(wrappedValue: AskEngine(scope: .paper(paperID)))
        // this paper is being read aloud (its reader was left): take the narration over
        let playing = NowPlaying.shared.narrator(for: paperID)
        _narrator = StateObject(wrappedValue: playing ?? Narrator())
        _listening = State(initialValue: playing != nil)
    }

    enum Mode: String, CaseIterable { case reading = "Reading", pdf = "PDF" }

    var body: some View {
        let paper = model.paper(paperID)
        Group {
            if loading {
                ProgressView(message ?? "Opening…")
            } else if mode == .reading, controller.pageGone {
                PageGoneView(hasPDF: model.paper(paperID)?.hasPdf == true, retry: { controller.retryPage() }, showPDF: { mode = .pdf })
            } else if mode == .reading, let book {
                EpubWebViewIOS(url: chapters?.url ?? URL(fileURLWithPath: book.combined), readAccess: URL(fileURLWithPath: book.dir),
                               fontScale: fontScale, theme: theme, paged: paged, findBar: find.shown, scrollRequest: scrollRequest, controller: controller,
                               onPinch: { step in fontScale = min(2.2, max(0.7, ((fontScale + step * 0.1) * 10).rounded() / 10)) },
                               onToggleChrome: {
                                   withAnimation(.easeInOut(duration: 0.22)) { chromeHidden.toggle() }
                               },
                               onChrome: { hide in
                                   guard hide != chromeHidden, !(hide && listening), !find.shown else { return }
                                   withAnimation(.easeInOut(duration: 0.22)) { chromeHidden = hide }
                               }) { current = $0 }
                    // pages stay put when the bars come and go: the view always fills the screen
                    .ignoresSafeArea(edges: paged ? (bottomBar ? .top : .all) : (bottomBar ? [] : .bottom))
                    .safeAreaInset(edge: .bottom) {
                        if listening { NarrationBar(narrator: narrator) { stopListening() } }
                        else { NowPlayingBar() }   // another paper being read aloud
                    }
                    .overlay(alignment: .top) {
                        VStack(spacing: 0) {
                            if extending != nil { ExtendHint { cancelExtend() } }
                            ResumeNote(text: position.note).animation(.easeOut, value: position.note)
                        }
                        .animation(.easeOut(duration: 0.2), value: extending)
                    }
            } else if mode == .pdf, let pdfURL {
                PDFReader(url: pdfURL, paperID: paperID, highlighter: pdfHighlights) { a in editingAnnotation = a }
                    .ignoresSafeArea(edges: .bottom)
                    // a paper being read aloud (this one too: its player is in the reading view)
                    .overlay(alignment: .bottom) { NowPlayingBar(style: .capsule).padding(.horizontal, 12).padding(.bottom, 12) }
            } else if let downloadProblem {
                ContentUnavailableView {
                    Label("Not Downloaded", systemImage: "icloud.slash")
                } description: {
                    Text(downloadProblem)
                } actions: {
                    Button("Try Again") { Task { await retryDownload() } }.buttonStyle(.borderedProminent)
                    Button("Paper Info") { showInfo = true }
                }
            } else {
                PaperInfoView(paperID: paperID, embedded: true)
            }
        }
        // retracted, corrected, or a preprint since published (Checks.swift)
        .safeAreaInset(edge: .top, spacing: 0) { if !chromeHidden { UpdatesBanner(paperID: paperID) } }
        // Remake Reading View…: asks first, shows progress, then how it went; the new view when it's made
        .modifier(RemakeProgress(paperID: paperID, request: $remakeRequest, text: remaking ?? remakeNote, busy: remaking != nil,
                                 remake: remake, made: reloadBook))
        // find in paper: over the page, so the page keeps its size (and its pages)
        .modifier(FindOverlay(find: find, theme: mode == .reading ? theme : nil))
        .modifier(goToPageModifier)
        // ⌘F, ⌘G and ⇧⌘G from a hardware keyboard (buttons the keyboard can reach, not seen)
        .background {
            Group {
                Button("Find in Paper") { openFind() }.keyboardShortcut("f", modifiers: .command)
                if find.shown {
                    Button("Find Next") { find.next() }.keyboardShortcut("g", modifiers: .command)
                    Button("Find Previous") { find.previous() }.keyboardShortcut("g", modifiers: [.command, .shift])
                }
            }
            .opacity(0).allowsHitTesting(false).accessibilityHidden(true)
        }
        .navigationBarTitleDisplayMode(.inline)
        .toolbarVisibility(chromeHidden && mode == .reading ? .hidden : .visible, for: .navigationBar)
        .statusBarHidden(chromeHidden && mode == .reading)
        .onChange(of: listening) { _, on in if on { chromeHidden = false } }
        .toolbar {
            // iPad: the Reading/PDF switch in the bar. iPhone: in the ⋯ menu, so the buttons fit (with the
            // switch, iOS folded them into one control and every button took two taps)
            if canSwitchView && sizeClass == .regular {
                ToolbarItem(placement: .principal) {
                    Picker("View", selection: $mode) { ForEach(Mode.allCases, id: \.self) { Text($0.rawValue) } }
                        .pickerStyle(.segmented).frame(width: 170)
                }
            }
            ToolbarItemGroup(placement: .topBarTrailing) {
                // iPad: draw on the PDF's pages with Apple Pencil (PencilInk.swift)
                if mode == .pdf, pdfURL != nil, UIDevice.current.userInterfaceIdiom == .pad { PDFMarkupButton(ink: pdfHighlights.ink) }
                if mode == .reading, book != nil {
                    // figures up front; listening is in the … menu (the narration bar has its stop button)
                    FiguresButton(controller: controller) { showFigures = true }
                }
                // find in the paper: one tap (reading view and PDF)
                if (mode == .reading && book != nil) || (mode == .pdf && pdfURL != nil) {
                    Button { openFind() } label: { Image(systemName: "magnifyingglass") }
                        .accessibilityLabel("Find in Paper")
                }
                if mode == .reading, book != nil {
                    // highlight mode: tap to turn on/off; hold for the colour
                    Menu {
                        Toggle(isOn: $highlightMode) { Label("Highlight Mode", systemImage: "highlighter") }
                        Picker("Color", selection: $highlightModeColor) {
                            ForEach(HighlightColor.allCases) { c in
                                Label { Text(c.name) } icon: { Image(systemName: "circle.fill").foregroundStyle(c.color) }.tag(c.rawValue)
                            }
                        }
                    } label: {
                        Image(systemName: "highlighter")
                            .foregroundStyle(highlightMode ? (HighlightColor(rawValue: highlightModeColor) ?? .yellow).color : Color.accentColor)
                            .padding(4)
                            .background(highlightMode ? (HighlightColor(rawValue: highlightModeColor) ?? .yellow).color.opacity(0.22) : .clear, in: Circle())
                    } primaryAction: { highlightMode.toggle() }
                    .accessibilityLabel(highlightMode ? "Highlight mode on" : "Highlight mode off")
                    // iPad: Ask in the bar; iPhone: in the ⋯ menu (iOS fits four buttons; a fifth went into
                    // its own overflow menu, and the ⋯ menu took two taps)
                    if sizeClass == .regular { Button { asking = true } label: { Image(systemName: "sparkles") } }
                }
                Menu {
                    // every item works with one tap (appearance settings are in Text & Theme)
                    if canSwitchView && sizeClass != .regular { ViewSwitchButton(mode: $mode) }
                    if mode == .reading, book != nil, sizeClass != .regular {
                        Button { asking = true } label: { Label("Ask about this Paper", systemImage: "sparkles") }
                    }
                    if mode == .reading, book != nil {
                        Button { showContents = true } label: { Label("Contents", systemImage: "list.bullet") }
                        Button { showReferences = true } label: { Label("References", systemImage: "text.book.closed") }
                        Button { showAppearance = true } label: { Label("Text & Theme…", systemImage: "textformat.size") }
                        Button { listening ? stopListening() : startListening() } label: {
                            Label(listening ? "Stop Listening" : "Listen", systemImage: listening ? "stop.circle" : "headphones")
                        }
                        Divider()
                    }
                    if canGoToPage { GoToPageButton(request: $goToPage) }
                    if (mode == .reading && book != nil) || mode == .pdf {
                        Button { showHighlights = true } label: { Label("Highlights & Notes", systemImage: "highlighter") }
                    }
                    Button { organizing = true } label: { Label("Folders & Tags…", systemImage: "folder") }
                    Button { editing = true } label: { Label("Edit Details…", systemImage: "pencil") }
                    Button { sharing = true } label: { Label("Share…", systemImage: "square.and.arrow.up") }
                    if let paper, paper.hasEpub || paper.hasPdf || !paper.meta.pmcid.isEmpty || !paper.meta.doi.isEmpty || !paper.meta.arxiv.isEmpty {
                        // "Remake Reading View…" asks first (it replaces the one there is)
                        Button { askRemake() } label: { Label(RemakeReadingView.title([paperID], model: model), systemImage: "wand.and.stars") }
                            .disabled(remaking != nil)
                    }
                    Button { outline = OutlineRequest(ids: [paperID], scope: "this paper", inReader: true) } label: { Label("Outline from Highlights…", systemImage: "list.bullet.indent") }
                    Button { showInfo = true } label: { Label("Paper Info", systemImage: "info.circle") }
                } label: { Label("More", systemImage: "ellipsis.circle").labelStyle(.iconOnly) }
            }
        }
        .environment(\.highlightUndo, undoManager)
        // the theme's light/dark (reading view only; the PDF keeps the system's)
        // (iPhone only: on iPad the reader shares the window with the library, so only the page changes)
        .preferredColorScheme(mode == .reading && UIDevice.current.userInterfaceIdiom == .phone ? theme.scheme : nil)
        .onReceive(NotificationCenter.default.publisher(for: .NSUndoManagerDidUndoChange)) { _ in refreshHighlights() }
        .onReceive(NotificationCenter.default.publisher(for: .NSUndoManagerDidRedoChange)) { _ in refreshHighlights() }
        .sheet(item: $editingAnnotation) { a in
            AnnotationEditor(annotation: a, onExtend: mode == .reading && a.location == "epub" ? { startExtend(a.id) } : nil) { refreshHighlights() }
                .environmentObject(model)
        }
        .sheet(isPresented: $showHighlights) {
            NavigationStack {
                AnnotationsList(paperID: paperID, missing: mode == .reading ? controller.missingAnnotations : [], onShow: { a in
                    showHighlights = false
                    Task {
                        try? await Task.sleep(for: .milliseconds(350))
                        if mode == .pdf { pdfHighlights.show(a) } else { controller.showAnnotation(a.id) }
                    }
                }, onEdit: { a in showHighlights = false; editingAnnotation = a })
                .navigationTitle("Highlights & Notes").navigationBarTitleDisplayMode(.inline)
                .toolbar { ToolbarItem(placement: .confirmationAction) { Button("Done") { showHighlights = false } } }
            }
            .environmentObject(model)
            .presentationDetents([.medium, .large])
        }
        .onChange(of: paper?.annotationCount) { refreshHighlights() }
        .sheet(isPresented: $organizing) { OrganizeView(ids: [paperID]).environmentObject(model) }
        .sheet(isPresented: $editing) { MetadataEditor(paperID: paperID).environmentObject(model) }
        .sheet(isPresented: $showContents) {
            if let book {
                ContentsSheet(book: book, current: current) { a in
                    current = a
                    // a book read chapter by chapter: in another chapter, that one opens there
                    if chapters?.show(anchor: a) != true { scrollRequest = ScrollRequest(anchor: a) }
                    showContents = false
                }
            }
        }
        .modifier(ReferencesSheet.Presenter(shown: $showReferences, refs: references, controller: controller))
        .sheet(isPresented: $asking) {
            AssistantPanel(engine: ask, onPassage: { id in
                asking = false
                mode = .reading
                Task { try? await Task.sleep(for: .milliseconds(350)); controller.flash(id) }
            })
            .environmentObject(model)
            .presentationDetents([.medium, .large])
            .presentationBackgroundInteraction(.enabled(upThrough: .medium))
        }
        .figureOverlay($figure, controller: controller)
        .readerFigures(controller, browsing: $showFigures)
        .sheet(isPresented: $sharing) {
            ScrollView { SharePanel(ids: [paperID]) }.environmentObject(model).presentationDetents([.medium, .large])
        }
        .sheet(isPresented: $showAppearance) {
            ReaderAppearanceSheet(fontScale: $fontScale, paged: $paged, themeRaw: $themeRaw)
                .presentationDetents([.medium, .large])
                .presentationBackgroundInteraction(.enabled(upThrough: .medium))
        }
        .sheet(isPresented: $showInfo) { NavigationStack { PaperInfoView(paperID: paperID, embedded: false) } }
        .sheet(item: $outline) { r in OutlineView(request: r).environmentObject(model) }
        .onChange(of: highlightMode) { _, on in controller.setHighlightMode(on, color: HighlightColor(rawValue: highlightModeColor) ?? .yellow) }
        .onChange(of: highlightModeColor) { controller.setHighlightMode(highlightMode, color: HighlightColor(rawValue: highlightModeColor) ?? .yellow) }
        .onChange(of: mode) { _, m in
            if extending != nil { cancelExtend() }
            if m == .pdf, pdfURL == nil { Task { await loadPDF(); find.use(findBackend) } }
            find.use(findBackend)
        }
        // finding: in pages the bar takes the navigation bar's place (the pages make room for it);
        // scrolling, the bars stay
        .onChange(of: find.shown) { _, shown in withAnimation { chromeHidden = shown && paged && mode == .reading } }
        .task { await setUp() }
        .onAppear { OpenReader.set(paperID); model.reading.insert(paperID); position.resume(); NowPlaying.shared.readerAppeared() }
        .showsPendingHighlight(paperID) { pdfOnly, delay in if !pdfOnly || mode == .pdf { showPendingHighlight(after: delay) } }
        .onDisappear {
            model.reading.remove(paperID)
            NowPlaying.shared.readerDisappeared()
            // a figure opened over the reader: it's still there, at the same place
            guard figure == nil else { return }
            // listening goes on: the Now Playing bar shows it in the library (NowPlaying.swift)
            position.stop(paperID, model: model, controller: controller)
            summary.stop()
            OpenReader.clear(paperID)
        }
        .onChange(of: summariesOn) { _, on in summary.setEnabled(on) }
        // stopped from the Now Playing bar, or another paper is read aloud now
        .onReceive(NowPlaying.shared.settled) { if listening, Narrator.active !== narrator { closePlayer() } }
        // leaving the app: save the place now (iOS may close Lunta in the background to free memory)
        .onChange(of: phase) { _, p in if p != .active { position.saveNow(paperID, model: model, controller: controller) } }
    }

    /// Extend… (a highlight, `id`) or Highlight to… (the selection): the hint shows until the tap
    /// that ends the passage (Highlights.swift, luntaExtendStart).
    private func startExtend(_ id: String?) {
        let color = id.flatMap { i in model.annotations(paperID).first { $0.id == i } }.flatMap { HighlightColor(rawValue: $0.color) } ?? .yellow
        Task {
            // (the highlight's sheet closes first)
            if id != nil { try? await Task.sleep(for: .milliseconds(350)) }
            guard await controller.startExtend(id: id, color: color) else { return }
            extendStarted(id: id, color: color)
        }
    }

    /// The page is waiting for the tap that ends the passage: the hint shows.
    private func extendStarted(id: String?, color: HighlightColor) {
        let was = extending?.chromeWasHidden ?? chromeHidden
        withAnimation(.easeInOut(duration: 0.22)) {
            extending = Extending(id: id, color: color, chromeWasHidden: was)
            // pages: the hint takes the bars' place, so it covers no text
            if paged { chromeHidden = true }
        }
    }

    private func endExtend() {
        guard let e = extending else { return }
        controller.endExtend()
        withAnimation(.easeInOut(duration: 0.22)) {
            extending = nil
            if paged { chromeHidden = e.chromeWasHidden }
        }
    }

    private func cancelExtend() {
        endExtend()
        refreshHighlights()   // Highlight to…: the passage coloured meanwhile goes
    }

    /// A highlight Open in Paper asked for (a summary's citation, HighlightJump): shown once the page is ready.
    private func showPendingHighlight(after delay: Duration = .milliseconds(500)) {
        guard HighlightJump.has(paperID) else { return }
        Task { @MainActor in
            try? await Task.sleep(for: delay)
            guard let id = HighlightJump.take(paperID), let a = model.annotations(paperID).first(where: { $0.id == id }) else { return }
            if mode == .pdf { pdfHighlights.show(a) } else { controller.showAnnotation(a.id) }
        }
    }

    private func refreshHighlights() {
        controller.showAnnotations(model.annotations(paperID))
        controller.setLibraryIDs(model.libraryIDs())   // the citation card: "In library" for those
    }

    /// The reader's hooks into the page, the assistant and the voice (run once the view appears).
    private func setUp() async {
        controller.onFigure = { figure = $0 }
        // the figure viewer's caption: highlights made there are this reader's (FigureViewer.swift)
        controller.model = model; controller.paperID = paperID; controller.undo = undoManager
        controller.onPageReady = {
            if extending != nil { endExtend() }   // the page was loaded again: its passage is gone
            controller.setHighlightMode(highlightMode, color: HighlightColor(rawValue: highlightModeColor) ?? .yellow)
            refreshHighlights(); position.pageReady(paperID, model: model, controller: controller)
            showPendingHighlight()
            // (a book read chapter by chapter has no summary: one chapter isn't the paper)
            if let chapters { chapters.pageReady() } else { summary.pageReady(paperID, model: model, controller: controller) }
            resumeListening()
        }
        controller.onSelectionDone = { pageSel in
            guard highlightMode, mode == .reading else { return }
            Task {
                var sel = pageSel
                if sel == nil { sel = await controller.pageSelection() }
                guard let sel else { refreshHighlights(); return }
                await autoHighlighter.apply(sel, paper: paperID, color: HighlightColor(rawValue: highlightModeColor) ?? .yellow,
                                            model: model, controller: controller, undo: undoManager)
                controller.clearSelection()
                refreshHighlights()
            }
        }
        controller.onAnnotation = { id in editingAnnotation = model.annotations(paperID).first { $0.id == id } }
        controller.onHighlightTo = { startExtend(nil) }
        // a selection dragged over the page's edge: the page turned and waits for the end word
        controller.onExtendBegan = { color in extendStarted(id: nil, color: color) }
        controller.onExtendDone = { sel in
            guard let e = extending else { return }
            endExtend()
            Task {
                if let id = e.id { _ = await model.extendHighlight(paperID, id: id, to: sel, controller: controller, undo: undoManager) }
                else { _ = await model.addHighlightMerging(paperID, sel, color: e.color, controller: controller, undo: undoManager) }
                refreshHighlights()
            }
        }
        controller.onMenu = { action in
            Task {
                guard let sel = await controller.pageSelection() else { return }
                switch action {
                case .highlight(let c): _ = await model.addHighlightMerging(paperID, sel, color: c, controller: controller, undo: undoManager)
                case .note: editingAnnotation = await model.addHighlightMerging(paperID, sel, controller: controller, undo: undoManager)
                }
                controller.clearSelection()
                refreshHighlights()
            }
        }
        if PhoneVoice.shared.downloaded { Task { _ = await PhoneVoice.shared.prepare() } }
        ask.model = model
        ask.passages = { [weak controller] in await controller?.narrationItems(includeCaptions: true) ?? [] }
        ask.selection = { [weak controller] in await controller?.selectedText() }
        pdfFinder.highlighter = pdfHighlights
        position.onSettled = { [weak find] in find?.pageSettled() }
        await load()
        find.use(findBackend)
        if mode == .pdf { try? await Task.sleep(for: .milliseconds(500)); find.pageSettled() }
        #if DEBUG
        Task { await shotHooks() }
        await positionTest()
        await controller.figureTest()
        await findTest()
        #endif
    }

    private var canSwitchView: Bool { book != nil && model.paper(paperID)?.hasPdf == true }
    private var canGoToPage: Bool { (mode == .reading && book != nil) || (mode == .pdf && pdfURL != nil) }
    private var goToPageModifier: GoToPageModifier {
        GoToPageModifier(request: goToPage, pdf: mode == .pdf, webView: { [controller] in controller.webView }, pdfView: { [pdfHighlights] in pdfHighlights.view })
    }
    /// What the find bar searches: the reading view or the PDF.
    private var findBackend: FindBackend? { mode == .pdf ? (pdfURL != nil ? pdfFinder : nil) : (book != nil ? (bookFind as FindBackend?) ?? controller : nil) }

    private func openFind() {
        find.use(findBackend)
        find.open()
    }

    #if DEBUG
    /// Screenshots: -pdfMode shows the PDF, -hideChrome hides the bars, -showAppearance opens Text & Theme, -readerJS <js> runs
    /// in the page after -readerJSDelay <seconds> (default 8).
    private func shotHooks() async {
        let args = ProcessInfo.processInfo.arguments
        if args.contains("-pdfMode") { mode = .pdf }
        if args.contains("-hideChrome") { try? await Task.sleep(for: .seconds(4)); chromeHidden = true }
        if args.contains("-showAppearance") { try? await Task.sleep(for: .seconds(5)); showAppearance = true }
        if let i = args.firstIndex(of: "-readerJS"), args.indices.contains(i + 1) {
            let delay = args.firstIndex(of: "-readerJSDelay").flatMap { args.indices.contains($0 + 1) ? Double(args[$0 + 1]) : nil } ?? 8
            try? await Task.sleep(for: .seconds(delay))
            let r = try? await controller.webView?.evaluateJavaScript(args[i + 1])
            print("SHOTJS", r.map { "\($0)" } ?? "nil"); fflush(stdout)
        }
    }

    /// -findTest <query> (see PaperFind.selfTest); -findPDF searches the PDF instead.
    private func findTest() async {
        let args = ProcessInfo.processInfo.arguments
        guard args.contains("-findTest") else { return }
        if args.contains("-findPDF") {
            mode = .pdf
            for _ in 0..<40 where pdfFinder.highlighter?.view?.document == nil { try? await Task.sleep(for: .milliseconds(250)) }
            find.use(findBackend)
        } else {
            // the page loaded and at its place (the restore holds it for a moment after)
            for _ in 0..<80 { if await controller.readingOffset() != nil { break }; try? await Task.sleep(for: .milliseconds(250)) }
        }
        await find.selfTest(reader: {
            if mode == .pdf {
                guard let v = pdfHighlights.view, let doc = v.document else { return "no pdf" }
                let page = v.currentPage.map { doc.index(for: $0) } ?? -1
                return "pdf page=\(page + 1)/\(doc.pageCount) highlighted=\(v.highlightedSelections?.count ?? 0) \(pdfFinder.currentMatchInfo)"
            }
            let sv = controller.webView?.scrollView
            let place = await controller.readingOffset() ?? -1
            let js = (try? await controller.webView?.evaluateJavaScript("JSON.stringify(luntaFindState())")) as? String ?? "-"
            return "paged=\(paged) x=\(Int(sv?.contentOffset.x ?? -1)) y=\(Int(sv?.contentOffset.y ?? -1)) place=\(place) \(js)"
        }, annotations: {
            let n = (try? await controller.webView?.evaluateJavaScript("document.querySelectorAll('mark.m-hl').length")) as? Int ?? -1
            return "annotationMarks=\(n) saved=\(model.annotations(paperID).count)"
        })
    }
    #endif

    #if DEBUG
    /// Simulator test of the saved place: -posTrace logs the place every second for a while after
    /// opening; -posScrollY <points> (scroll) or -posPage <n> (pages) then moves there 12 s in, as a
    /// finger would, and logs what was saved. Quit and relaunch with -posTrace to see the place restored.
    private func positionTest() async {
        let args = ProcessInfo.processInfo.arguments
        guard args.contains("-posTrace") else { return }
        func arg(_ k: String) -> Int? { args.firstIndex(of: k).flatMap { args.indices.contains($0 + 1) ? Int(args[$0 + 1]) : nil } }
        let t0 = Date()
        func log(_ s: String) { print(String(format: "POS %5.1fs ", Date().timeIntervalSince(t0)) + s); fflush(stdout) }
        func trace(_ label: String) async {
            let o = await controller.readingOffset() ?? -1
            let sv = controller.webView?.scrollView
            log("\(label) offset=\(o) x=\(Int(sv?.contentOffset.x ?? -1)) y=\(Int(sv?.contentOffset.y ?? -1)) h=\(Int(sv?.contentSize.height ?? -1)) w=\(Int(sv?.contentSize.width ?? -1))")
        }
        log("open paged=\(paged)")
        for i in 0..<12 { try? await Task.sleep(for: .seconds(1)); await trace("t\(i + 1)") }
        if let y = arg("-posScrollY"), let sv = controller.webView?.scrollView {
            sv.setContentOffset(CGPoint(x: 0, y: CGFloat(y)), animated: false)
            log("moved to y=\(y)")
        } else if let n = arg("-posPage"), let v = controller.webView {
            PageMessageHandler.show(page: n, in: v, animated: false)
            log("moved to page \(n)")
        } else { return }
        for i in 0..<8 { try? await Task.sleep(for: .seconds(1)); await trace("after\(i + 1)") }
        let file = model.libraryURL.appendingPathComponent("positions/\(LibraryModel.deviceID).json")
        log("saved \(ReadingPositions.read(file)[paperID].map { "\($0.offset)" } ?? "none")")
    }
    #endif

    private func startListening() {
        Task {
            let items = await controller.narrationItems()
            guard !items.isEmpty else { return }
            // natural (neural) narration when it has been rendered, otherwise the system voice
            let title = model.paper(paperID)?.meta.title ?? ""
            // (a book read chapter by chapter is read one chapter at a time: a narration rendered on
            // the Mac for the whole book doesn't follow its pages)
            if chapters == nil, await model.loadNeuralNarration(paperID, into: narrator, pageItems: items) {
                // a full natural-voice narration was rendered on the Mac
            } else {
                // natural voice made on this iPhone, paragraph by paragraph as it plays
                narrator.loadLive(items, title: title) { item in
                    await PhoneVoice.shared.clip(Narrator.spoken(item), kind: item.kind)
                }
                if !PhoneVoice.shared.downloaded {
                    narrator.note = "Downloading the natural voice (one time only)…"
                    Task {
                        let ok = await PhoneVoice.shared.prepare()
                        narrator.note = ok ? nil : "Couldn't load the natural voice; using Apple's voice."
                        if !ok, listening {
                            let i = narrator.index
                            narrator.load(items, title: title)
                            narrator.onHighlight = { [weak controller] id in controller?.highlight(id) }
                            narrator.play(from: i)
                        }
                    }
                }
            }
            narrator.paperID = paperID
            narrator.onHighlight = { [weak controller] id in controller?.highlight(id) }
            narrator.onEnd = chapterEnd
            controller.onTapParagraph = { [weak narrator] id in narrator?.play(fromID: id) }
            controller.setListening(true)
            listening = true
            if let first = await controller.firstVisibleID(), let i = items.firstIndex(where: { $0.id == first }), i > 0 {
                narrator.play(from: i)
            } else {
                narrator.play(from: 0)
            }
        }
    }

    private func stopListening() {
        narrator.stop()
        closePlayer()
    }

    private func closePlayer() {
        controller.setListening(false)
        listening = false
    }

    /// Reopened while its narration plays on: the page follows the voice again, from the paragraph being read.
    private func resumeListening() {
        guard listening else { return }
        narrator.onHighlight = { [weak controller] id in controller?.highlight(id) }
        narrator.onEnd = chapterEnd
        controller.onTapParagraph = { [weak narrator] id in narrator?.play(fromID: id) }
        controller.setListening(true)
        if narrator.items.indices.contains(narrator.index) { controller.highlight(narrator.items[narrator.index].id) }
    }

    /// A book read chapter by chapter: at a chapter's end, Listen goes on into the next one (its
    /// page opens and its paragraphs are read, with the same voice).
    private var chapterEnd: (() -> Void)? {
        guard chapters != nil else { return nil }
        return { [weak chapters, weak controller, weak narrator] in
            guard let chapters, let controller, let narrator, chapters.current + 1 < chapters.parts.count else { return }
            Task {
                guard await chapters.open(chapters.current + 1, then: .top) else { return }
                let items = await controller.narrationItems()
                guard !items.isEmpty, Narrator.active == nil || Narrator.active === narrator else { return }
                let title = narrator.title
                if narrator.live {
                    narrator.loadLive(items, title: title) { item in await PhoneVoice.shared.clip(Narrator.spoken(item), kind: item.kind) }
                } else {
                    narrator.load(items, title: title)
                }
                controller.setListening(true)
                narrator.play(from: 0)
            }
        }
    }

    /// A player at the bottom of the reading view: this paper's, or the Now Playing bar of another.
    private var bottomBar: Bool { listening || (nowPlaying.narrator.map { $0 !== narrator } ?? false) }

    private func load() async {
        guard let p = model.paper(paperID) else { loading = false; return }
        downloadProblem = nil
        var problem: String?
        if p.hasEpub {
            message = model.needsDownload(paperID, .epub) ? "Downloading from \(model.downloadSource)…" : nil
            if model.inCloud, !model.needsDownload(paperID, .epub),
               CloudLibrary.isStale(URL(fileURLWithPath: model.lib.expectedFilePath(id: paperID, kind: .epub))) {
                message = "Getting the newest version from iCloud…"
            }
            switch await model.fetchFile(paperID, .epub) {
            case .ready:
                let lib = model.lib
                let id = paperID
                if let b = await Task.detached(operation: { try? lib.openEpub(id: id) }).value {
                    await openChapters(b)
                    book = b
                    mode = .reading
                }
            case .failed(let why): problem = why
            case .missing: break
            }
        }
        if book == nil, p.hasPdf {
            mode = .pdf
            await loadPDF()   // (a reading view that couldn't be downloaded: the PDF instead, if it can be)
        } else if book == nil {
            downloadProblem = problem
        }
        if book != nil || pdfURL != nil { model.markOpened(p) }
        loading = false
    }

    /// A big reading view (a book) is read chapter by chapter, from the chapter holding the place
    /// last read (on any device); a paper is shown as one page (`chapters` nil).
    private func openChapters(_ b: EpubBook) async {
        let lib = model.lib
        let id = paperID
        let info = await Task.detached(operation: { try? lib.readingParts(id: id) }).value
        guard let info, info.chaptered else { chapters = nil; bookFind = nil; controller.chapters = nil; return }
        let spot = await ReadingPositions.shared.latest(paperID, root: model.libraryURL)
        guard let ch = ChapterReader(book: b, parts: info, at: spot?.offset) else { return }
        ch.controller = controller
        ch.willOpen = { [weak position] offset, scroll in position?.partChanged(to: offset, scroll: scroll) }
        controller.chapters = ch
        controller.part = ch.current
        chapters = ch
        bookFind = BookFind(chapters: ch, controller: controller)
    }

    /// Make Reading View, or Remake Reading View… (asks first).
    private func askRemake() {
        let n = RemakeReadingView.existing([paperID], model: model)
        if n == 0 { remake() } else { remakeRequest = RemakeRequest(ids: [paperID], existing: n) }
    }

    /// Makes the reading view (again); the new one is shown when it's there (`luntaReadingViewMade`).
    private func remake() {
        let had = model.paper(paperID)?.hasEpub == true
        remakeNote = nil
        chromeHidden = false
        remaking = had ? "Remaking the reading view…" : "Making the reading view…"
        Task {
            let ok = await model.makeReadingView(paperID)
            model.reload()
            remaking = nil
            let note = ok ? (had ? "Reading view remade." : "Reading view ready.")
                : had ? "Couldn’t make the reading view again; the current one is kept." : "No open full text found for this paper."
            remakeNote = note
            try? await Task.sleep(for: .seconds(6))
            if remakeNote == note { remakeNote = nil }
        }
    }

    /// Opens the paper's reading view again (it was remade), at the same place.
    private func reloadBook() {
        if listening { stopListening() }
        if extending != nil { cancelExtend() }
        book = nil
        chapters = nil; bookFind = nil; controller.chapters = nil
        message = "Opening the new reading view…"
        loading = true
        Task { await load() }
    }

    private func loadPDF() async {
        message = model.needsDownload(paperID, .pdf) ? "Downloading the PDF from \(model.downloadSource)…" : nil
        switch await model.fetchFile(paperID, .pdf) {
        case .ready(let u): pdfURL = u; downloadProblem = nil
        case .failed(let why): downloadProblem = why
        case .missing: break
        }
    }

    /// Try Again after a failed download: the whole opening again (reading view first).
    private func retryDownload() async {
        downloadProblem = nil
        if book == nil && pdfURL == nil {
            loading = true
            message = nil
            await load()
        } else if mode == .pdf, pdfURL == nil {
            await loadPDF()
        }
        find.use(findBackend)
    }
}

struct ScrollRequest: Equatable {
    var anchor: String?
    var serial = UUID()
}

struct ContentsSheet: View {
    let book: EpubBook
    let current: String?
    let go: (String) -> Void

    var body: some View {
        NavigationStack {
            List(Array(book.toc.enumerated()), id: \.offset) { _, t in
                let a = anchor(t)
                Button { go(a) } label: {
                    Text(t.title)
                        .font(t.level <= 1 ? .body.weight(.semibold) : .subheadline)
                        .foregroundStyle(a == current ? Color.luntaTeal : (t.level <= 1 ? .primary : .secondary))
                        .padding(.leading, CGFloat(max(0, Int(t.level) - 1)) * 14)
                }
            }
            .listStyle(.plain)
            .navigationTitle("Contents")
            .navigationBarTitleDisplayMode(.inline)
        }
        .presentationDetents([.medium, .large])
    }

    private func anchor(_ t: TocEntry) -> String {
        let parts = t.target.split(separator: "#", maxSplits: 1).map(String.init)
        if parts.count == 2 { return parts[1] }
        return "section-\(book.chapters.firstIndex { $0.path == parts[0] } ?? 0)"
    }
}

/// The paper's reference list (⋯ › References): jump to one, read it if it's in the library,
/// add it, or add all that are missing.
struct ReferencesSheet: View {
    @ObservedObject var refs: ReferencesModel
    let onShow: (PaperReference) -> Void
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            // a reference in the library: the sheet closes and that paper opens in the reader
            // (iPhone: on top of this one, Back returns; iPad: selected in the library)
            ReferencesList(refs: refs, onShow: onShow, onOpen: { id in
                dismiss()
                Task {
                    try? await Task.sleep(for: .milliseconds(350))
                    NotificationCenter.default.post(name: .luntaReadPaper, object: id)
                }
            })
                .navigationTitle("References")
                .navigationBarTitleDisplayMode(.inline)
                .toolbar { ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } } }
        }
        .presentationDetents([.medium, .large])
    }

    /// The sheet over the reader: tapping a reference closes it and jumps there.
    struct Presenter: ViewModifier {
        @EnvironmentObject var model: LibraryModel
        @Binding var shown: Bool
        let refs: ReferencesModel
        let controller: ReaderController

        func body(content: Content) -> some View {
            content.sheet(isPresented: $shown) {
                ReferencesSheet(refs: refs, onShow: { r in
                    shown = false
                    Task {
                        try? await Task.sleep(for: .milliseconds(350))
                        _ = try? await controller.webView?.evaluateJavaScript("window.luntaShowReference && luntaShowReference(\(r.index))")
                    }
                })
                .environmentObject(model)
                .task { await refs.load(controller, model: model) }
            }
        }
    }
}

struct EpubWebViewIOS: UIViewRepresentable {
    let url: URL
    let readAccess: URL
    let fontScale: Double
    var theme: ReaderTheme = .automatic
    var paged = false
    /// The find bar is over the page: pages start below it.
    var findBar = false
    let scrollRequest: ScrollRequest
    let controller: ReaderController
    var onPinch: (Double) -> Void = { _ in }
    var onToggleChrome: () -> Void = {}
    var onChrome: (Bool) -> Void = { _ in }
    let onPosition: (String) -> Void

    func makeUIView(context: Context) -> WKWebView {
        let cfg = WKWebViewConfiguration()
        cfg.userContentController.addUserScript(WKUserScript(source: ReaderAssets.styleScript, injectionTime: .atDocumentEnd, forMainFrameOnly: true))
        cfg.userContentController.addUserScript(WKUserScript(source: ReaderAssets.trackingJS, injectionTime: .atDocumentEnd, forMainFrameOnly: true))
        cfg.userContentController.addUserScript(WKUserScript(source: ReaderAssets.narrationJS, injectionTime: .atDocumentEnd, forMainFrameOnly: true))
        cfg.userContentController.add(context.coordinator, name: "pos")
        let narr = NarrationMessageHandler()
        narr.controller = controller
        cfg.userContentController.add(narr, name: "narr")
        let fig = FigureMessageHandler()
        fig.controller = controller
        cfg.userContentController.add(fig, name: "fig")
        cfg.userContentController.addUserScript(WKUserScript(source: theme.js, injectionTime: .atDocumentStart, forMainFrameOnly: true))
        context.coordinator.theme = theme
        cfg.addAnnotations(controller)
        cfg.addSummary(controller)
        // find in paper; a match counts as in view between the bars and the keyboard
        cfg.userContentController.addUserScript(WKUserScript(source: "window.luntaFindZone = [0.18, 0.55];" + ReaderAssets.findJS, injectionTime: .atDocumentEnd, forMainFrameOnly: true))
        // text size: the font scales, the page itself is never zoomed
        cfg.userContentController.addUserScript(WKUserScript(source: "document.documentElement.style.setProperty('--m-scale', '\(fontScale)');", injectionTime: .atDocumentStart, forMainFrameOnly: true))
        context.coordinator.textScale = fontScale
        let state = pagedState()
        context.coordinator.paging = cfg.addPaging(state)
        context.coordinator.applied = state
        let v = ReaderWebView(frame: .zero, configuration: cfg)
        v.controller = controller
        controller.webView = v
        v.isOpaque = false
        // web links open in the browser, not in the reader; a book's other chapters open in it; a
        // web process that ended (out of memory) is reloaded, or the reader says so
        let nav = ReaderNavigation(controller: controller)
        context.coordinator.navigation = nav
        v.navigationDelegate = nav
        context.coordinator.paging?.onPartEdge = { [weak controller] d in controller?.chapters?.edge(d) }
        v.overrideUserInterfaceStyle = Self.style(theme)   // the page follows the theme (on iPad the window doesn't)
        v.backgroundColor = .clear
        v.scrollView.alwaysBounceHorizontal = false
        v.scrollView.showsHorizontalScrollIndicator = false
        v.loadFileURL(url, allowingReadAccessTo: readAccess)
        context.coordinator.loaded = url
        context.coordinator.onChrome = onChrome
        context.coordinator.watch(v.scrollView)
        configureScroll(v.scrollView)
        // pinching changes the text size (the text reflows) instead of magnifying the page
        v.scrollView.minimumZoomScale = 1
        v.scrollView.maximumZoomScale = 1
        let pinch = UIPinchGestureRecognizer(target: context.coordinator, action: #selector(Coordinator.pinched(_:)))
        pinch.delegate = context.coordinator
        v.addGestureRecognizer(pinch)
        return v
    }

    /// Lunta for page mode, in CSS pixels: clear of the notch / Dynamic Island and the home bar,
    /// with room for the page number. Taken from the window so they don't change with the bars.
    private func pagedState() -> PagedState {
        let win = UIApplication.shared.connectedScenes.compactMap { ($0 as? UIWindowScene)?.keyWindow }.first
        let inset = win?.safeAreaInsets ?? UIEdgeInsets(top: 50, left: 0, bottom: 34, right: 0)
        // pages start below the toolbar (the back button and the tools, ~52 pt under the status bar),
        // which otherwise covered each page's first line while shown; the find bar takes its place
        return PagedState(on: paged, top: inset.top + 14 + (paged ? 52 : 0), bottom: max(inset.bottom, 12) + 22, native: true)
    }

    private func configureScroll(_ sv: UIScrollView) {
        guard sv.isPagingEnabled != paged else { return }
        sv.isPagingEnabled = paged
        sv.contentInsetAdjustmentBehavior = paged ? .never : .automatic
        sv.alwaysBounceVertical = !paged
        sv.showsVerticalScrollIndicator = !paged
        sv.isDirectionalLockEnabled = paged
    }

    static func style(_ t: ReaderTheme) -> UIUserInterfaceStyle {
        switch t.scheme { case .some(.dark): return .dark; case .some(.light): return .light; default: return .unspecified }
    }

    func updateUIView(_ v: WKWebView, context: Context) {
        if context.coordinator.theme != theme {
            context.coordinator.theme = theme
            v.evaluateJavaScript(theme.js)
            v.overrideUserInterfaceStyle = Self.style(theme)
        }
        context.coordinator.onPosition = onPosition
        context.coordinator.onChrome = onChrome
        context.coordinator.paged = paged
        context.coordinator.paging?.onChrome = onToggleChrome
        context.coordinator.onPinch = onPinch
        configureScroll(v.scrollView)
        if v.scrollView.zoomScale != 1 { v.scrollView.setZoomScale(1, animated: false) }
        if context.coordinator.textScale != fontScale {
            context.coordinator.textScale = fontScale
            v.evaluateJavaScript("luntaSetTextScale(\(fontScale))")
        }
        let state = pagedState()
        if state != context.coordinator.applied {
            context.coordinator.applied = state
            context.coordinator.paging?.state = state
            v.evaluateJavaScript(state.call)
        }
        if context.coordinator.loaded != url {
            context.coordinator.loaded = url
            v.loadFileURL(url, allowingReadAccessTo: readAccess)
        }
        if scrollRequest != context.coordinator.lastScroll, let a = scrollRequest.anchor {
            context.coordinator.lastScroll = scrollRequest
            v.evaluateJavaScript(ReaderAssets.scrollJS(a))
        }
    }

    func makeCoordinator() -> Coordinator { Coordinator(onPosition: onPosition) }

    final class Coordinator: NSObject, WKScriptMessageHandler, UIGestureRecognizerDelegate {
        var theme: ReaderTheme = .automatic
        var loaded: URL?
        var lastScroll = ScrollRequest()
        var onPosition: (String) -> Void
        var onChrome: (Bool) -> Void = { _ in }
        var paging: PageMessageHandler?
        var navigation: ReaderNavigation?
        var applied: PagedState?
        var paged = false
        var onPinch: (Double) -> Void = { _ in }
        var textScale: Double?
        private var pinchBase: CGFloat = 1

        /// One text-size step for every ~15% of pinch, so the text grows as you pinch.
        @objc func pinched(_ g: UIPinchGestureRecognizer) {
            switch g.state {
            case .began: pinchBase = 1
            case .changed:
                let r = g.scale / pinchBase
                if r > 1.15 { onPinch(1); pinchBase = g.scale }
                else if r < 0.87 { onPinch(-1); pinchBase = g.scale }
            default: break
            }
        }
        func gestureRecognizer(_ g: UIGestureRecognizer, shouldRecognizeSimultaneouslyWith other: UIGestureRecognizer) -> Bool { true }

        private var observation: NSKeyValueObservation?
        private var lastY: CGFloat = 0
        private var quietUntil = Date.distantPast
        init(onPosition: @escaping (String) -> Void) { self.onPosition = onPosition }

        /// Hides the bars while scrolling down through the text and brings them back on scrolling up.
        func watch(_ sv: UIScrollView) {
            observation = sv.observe(\.contentOffset, options: [.new]) { [weak self] sv, _ in
                MainActor.assumeIsolated { self?.scrolled(sv) }
            }
        }

        @MainActor private func scrolled(_ sv: UIScrollView) {
            let y = sv.contentOffset.y + sv.adjustedContentInset.top
            defer { lastY = y }
            if paged {
                // pages only move sideways (text-size changes can leave the page a bit taller)
                if sv.contentOffset.y != 0 { sv.contentOffset.y = 0 }
                return                        // the middle tap shows and hides the bars
            }
            guard sv.isTracking || sv.isDecelerating, Date() > quietUntil else { return }
            let dy = y - lastY
            var hide: Bool?
            if y < 40 { hide = false }
            else if dy > 6 { hide = true }
            else if dy < -10 { hide = false }
            if let hide {
                // the bar's own animation shifts the content: ignore that
                quietUntil = Date().addingTimeInterval(0.35)
                onChrome(hide)
            }
        }
        func userContentController(_ c: WKUserContentController, didReceive m: WKScriptMessage) {
            if let id = m.body as? String { onPosition(id) }
        }
    }
}



/// The paper open in the reader, so Lunta reopens it after iOS closed the app in the background
/// (going back to the list forgets it).
enum OpenReader {
    private static let key = "openReader"
    static func set(_ id: String) { UserDefaults.standard.set(["id": id, "at": Date().timeIntervalSince1970], forKey: key) }
    static func clear(_ id: String) {
        if (UserDefaults.standard.dictionary(forKey: key)?["id"] as? String) == id { UserDefaults.standard.removeObject(forKey: key) }
    }
    /// The paper to reopen: open within the last day.
    static var pending: String? {
        guard let d = UserDefaults.standard.dictionary(forKey: key), let id = d["id"] as? String,
              let at = d["at"] as? Double, Date().timeIntervalSince1970 - at < 24 * 3600 else { return nil }
        return id
    }
}

/// iPhone: Reading View ⇄ PDF, in the reader's ⋯ menu.
private struct ViewSwitchButton: View {
    @Binding var mode: ReaderView.Mode
    var body: some View {
        if mode == .pdf {
            Button { mode = .reading } label: { Label("Reading View", systemImage: "doc.plaintext") }
        } else {
            Button { mode = .pdf } label: { Label("Show PDF", systemImage: "doc.richtext") }
        }
    }
}

extension Notification.Name {
    /// Open this paper (object: its id) in the reader, from inside a reader (References › In library).
    static let luntaReadPaper = Notification.Name("luntaReadPaper")
}

/// Extend… / Highlight to… : what the next tap in the page does, and Cancel.
private struct ExtendHint: View {
    var onCancel: () -> Void
    var body: some View {
        HStack(spacing: 10) {
            Image(systemName: "hand.tap").foregroundStyle(.secondary)
            Text("Tap where the highlight should end").font(.footnote.weight(.medium))
            Spacer(minLength: 4)
            Button("Cancel", action: onCancel).font(.footnote.weight(.semibold))
        }
        .padding(.leading, 14).padding(.trailing, 6).padding(.vertical, 4)
        .frame(minHeight: 40)
        .background(.regularMaterial, in: Capsule())
        .padding(.horizontal, 12).padding(.top, 8)
        .frame(maxWidth: 520)
        .transition(.move(edge: .top).combined(with: .opacity))
    }
}
