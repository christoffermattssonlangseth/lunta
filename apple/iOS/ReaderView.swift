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
    @State private var pdfURL: URL?
    @State private var loading = true
    @State private var message: String?
    @State private var showContents = false
    @State private var showInfo = false
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
    @StateObject private var pdfHighlights = PDFHighlighter()
    @StateObject private var find = PaperFind()
    @State private var pdfFinder = PDFFinder()
    @State private var showHighlights = false
    @State private var current: String?
    @State private var scrollRequest = ScrollRequest()
    @StateObject private var narrator = Narrator()
    @StateObject private var controller = ReaderController()
    @StateObject private var position = PositionKeeper()
    @Environment(\.scenePhase) private var phase
    @State private var listening = false
    @State private var asking = false
    @StateObject private var ask: AskEngine

    init(paperID: String) {
        self.paperID = paperID
        _ask = StateObject(wrappedValue: AskEngine(scope: .paper(paperID)))
    }

    enum Mode: String, CaseIterable { case reading = "Reading", pdf = "PDF" }

    var body: some View {
        let paper = model.paper(paperID)
        Group {
            if loading {
                ProgressView(message ?? "Opening…")
            } else if mode == .reading, let book {
                EpubWebViewIOS(url: URL(fileURLWithPath: book.combined), readAccess: URL(fileURLWithPath: book.dir),
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
                    .ignoresSafeArea(edges: paged ? (listening ? .top : .all) : (listening ? [] : .bottom))
                    .safeAreaInset(edge: .bottom) {
                        if listening { NarrationBar(narrator: narrator) { stopListening() } }
                    }
                    .overlay(alignment: .top) { ResumeNote(text: position.note).animation(.easeOut, value: position.note) }
            } else if mode == .pdf, let pdfURL {
                PDFReader(url: pdfURL, paperID: paperID, highlighter: pdfHighlights) { a in editingAnnotation = a }
                    .ignoresSafeArea(edges: .bottom)
            } else {
                PaperInfoView(paperID: paperID, embedded: true)
            }
        }
        // find in paper: over the page, so the page keeps its size (and its pages)
        .modifier(FindOverlay(find: find, theme: mode == .reading ? theme : nil))
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
            if book != nil && paper?.hasPdf == true {
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
                    Button { asking = true } label: { Image(systemName: "sparkles") }
                    Button { showContents = true } label: { Image(systemName: "list.bullet") }
                }
                Menu {
                    if mode == .reading, book != nil {
                        Button { listening ? stopListening() : startListening() } label: {
                            Label(listening ? "Stop Listening" : "Listen", systemImage: listening ? "stop.circle" : "headphones")
                        }
                        ControlGroup {
                            Button { fontScale = max(0.7, fontScale - 0.1) } label: { Label("Smaller", systemImage: "textformat.size.smaller") }
                            Button { fontScale = min(2.2, fontScale + 0.1) } label: { Label("Larger", systemImage: "textformat.size.larger") }
                        } label: { Text("Text Size") }
                        Picker(selection: $paged) {
                            Label("Scroll", systemImage: "scroll").tag(false)
                            Label("Turn Pages", systemImage: "book").tag(true)
                        } label: { Label("Layout", systemImage: paged ? "book" : "scroll") }
                        .pickerStyle(.menu)
                        FigureSettingPicker()
                        Picker(selection: $themeRaw) {
                            ForEach(ReaderTheme.allCases) { t in Text(t.name).tag(t.rawValue) }
                        } label: { Label("Theme", systemImage: "circle.lefthalf.filled") }
                        .pickerStyle(.menu)
                        Picker(selection: Binding(get: { PhoneVoice.shared.voice }, set: { PhoneVoice.shared.voice = $0 })) {
                            ForEach(PhoneVoice.voices, id: \.id) { Text($0.name).tag($0.id) }
                        } label: { Label("Natural Voice", systemImage: "waveform") }
                        .pickerStyle(.menu)
                        Divider()
                    }
                    if (mode == .reading && book != nil) || (mode == .pdf && pdfURL != nil) {
                        Button { openFind() } label: { Label("Find in Paper", systemImage: "magnifyingglass") }
                    }
                    if (mode == .reading && book != nil) || mode == .pdf {
                        Button { showHighlights = true } label: { Label("Highlights & Notes", systemImage: "highlighter") }
                    }
                    Button { organizing = true } label: { Label("Folders & Tags…", systemImage: "folder") }
                    Button { editing = true } label: { Label("Edit Details…", systemImage: "pencil") }
                    Button { sharing = true } label: { Label("Share…", systemImage: "square.and.arrow.up") }
                    Button { showInfo = true } label: { Label("Paper Info", systemImage: "info.circle") }
                } label: { Image(systemName: "ellipsis.circle") }
            }
        }
        .environment(\.highlightUndo, undoManager)
        // the theme's light/dark (reading view only; the PDF keeps the system's)
        // (iPhone only: on iPad the reader shares the window with the library, so only the page changes)
        .preferredColorScheme(mode == .reading && UIDevice.current.userInterfaceIdiom == .phone ? theme.scheme : nil)
        .onReceive(NotificationCenter.default.publisher(for: .NSUndoManagerDidUndoChange)) { _ in refreshHighlights() }
        .onReceive(NotificationCenter.default.publisher(for: .NSUndoManagerDidRedoChange)) { _ in refreshHighlights() }
        .sheet(item: $editingAnnotation) { a in AnnotationEditor(annotation: a) { refreshHighlights() }.environmentObject(model) }
        .sheet(isPresented: $showHighlights) {
            NavigationStack {
                AnnotationsList(paperID: paperID, onShow: { a in
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
            if let book { ContentsSheet(book: book, current: current) { a in current = a; scrollRequest = ScrollRequest(anchor: a); showContents = false } }
        }
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
        .figureOverlay($figure)
        .readerFigures(controller, browsing: $showFigures)
        .sheet(isPresented: $sharing) {
            ScrollView { SharePanel(ids: [paperID]) }.environmentObject(model).presentationDetents([.medium, .large])
        }
        .sheet(isPresented: $showInfo) { NavigationStack { PaperInfoView(paperID: paperID, embedded: false) } }
        .onChange(of: highlightMode) { _, on in controller.setHighlightMode(on, color: HighlightColor(rawValue: highlightModeColor) ?? .yellow) }
        .onChange(of: highlightModeColor) { controller.setHighlightMode(highlightMode, color: HighlightColor(rawValue: highlightModeColor) ?? .yellow) }
        .onChange(of: mode) { _, m in
            if m == .pdf, pdfURL == nil { Task { await loadPDF(); find.use(findBackend) } }
            find.use(findBackend)
        }
        // finding: in pages the bar takes the navigation bar's place (the pages make room for it);
        // scrolling, the bars stay
        .onChange(of: find.shown) { _, shown in withAnimation { chromeHidden = shown && paged && mode == .reading } }
        .task {
            controller.onFigure = { figure = $0 }
            controller.onPageReady = {
                controller.setHighlightMode(highlightMode, color: HighlightColor(rawValue: highlightModeColor) ?? .yellow)
                refreshHighlights(); position.pageReady(paperID, model: model, controller: controller)
            }
            controller.onSelectionDone = { pageSel in
                guard highlightMode, mode == .reading else { return }
                Task {
                    var sel = pageSel
                    if sel == nil { sel = await controller.pageSelection() }
                    guard let sel else { refreshHighlights(); return }
                    autoHighlighter.apply(sel, paper: paperID, color: HighlightColor(rawValue: highlightModeColor) ?? .yellow, model: model, undo: undoManager)
                    controller.clearSelection()
                    refreshHighlights()
                }
            }
            controller.onAnnotation = { id in editingAnnotation = model.annotations(paperID).first { $0.id == id } }
            controller.onMenu = { action in
                Task {
                    guard let sel = await controller.pageSelection() else { return }
                    switch action {
                    case .highlight(let c): model.addHighlight(paperID, sel, color: c, undo: undoManager)
                    case .note: editingAnnotation = model.addHighlight(paperID, sel, undo: undoManager)
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
            // opened from a library search that found this paper: find the search in it
            if let q = PaperFind.libraryQuery(for: paperID, in: model) { find.prefill(q) }
            position.onSettled = { [weak find] in find?.pageSettled() }
            await load()
            find.use(findBackend)
            if mode == .pdf { try? await Task.sleep(for: .milliseconds(500)); find.pageSettled() }
            #if DEBUG
            await positionTest()
            await controller.figureTest()
            await findTest()
            #endif
        }
        .onAppear { OpenReader.set(paperID); position.resume() }
        .onDisappear {
            // a figure opened over the reader: it's still there, at the same place
            guard figure == nil else { return }
            if listening { stopListening() }
            position.stop(paperID, model: model, controller: controller)
            OpenReader.clear(paperID)
        }
        // leaving the app: save the place now (iOS may close Lunta in the background to free memory)
        .onChange(of: phase) { _, p in if p != .active { position.saveNow(paperID, model: model, controller: controller) } }
    }

    private func refreshHighlights() { controller.showAnnotations(model.annotations(paperID)) }

    /// What the find bar searches: the reading view or the PDF.
    private var findBackend: FindBackend? { mode == .pdf ? (pdfURL != nil ? pdfFinder : nil) : (book != nil ? controller : nil) }

    private func openFind() {
        find.use(findBackend)
        find.open()
    }

    #if DEBUG
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
            if await model.loadNeuralNarration(paperID, into: narrator, pageItems: items) {
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
            narrator.onHighlight = { [weak controller] id in controller?.highlight(id) }
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
        controller.setListening(false)
        listening = false
    }

    private func load() async {
        guard let p = model.paper(paperID) else { loading = false; return }
        if p.hasEpub {
            message = model.needsDownload(paperID, .epub) ? "Downloading from iCloud…" : nil
            if model.inCloud, !model.needsDownload(paperID, .epub),
               CloudLibrary.isStale(URL(fileURLWithPath: model.lib.expectedFilePath(id: paperID, kind: .epub))) {
                message = "Getting the newest version from iCloud…"
            }
            if await model.ensureLocal(paperID, .epub) != nil {
                let lib = model.lib
                let id = paperID
                if let b = await Task.detached(operation: { try? lib.openEpub(id: id) }).value {
                    book = b
                    mode = .reading
                }
            }
        }
        if book == nil, p.hasPdf {
            mode = .pdf
            await loadPDF()
        }
        if book != nil || pdfURL != nil { model.markOpened(p) }
        loading = false
    }

    private func loadPDF() async {
        message = model.needsDownload(paperID, .pdf) ? "Downloading PDF from iCloud…" : nil
        pdfURL = await model.ensureLocal(paperID, .pdf)
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
        v.navigationDelegate = ExternalLinks.shared   // web links open in the browser, not in the reader
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
        return PagedState(on: paged, top: inset.top + 14 + (findBar && paged ? 52 : 0), bottom: max(inset.bottom, 12) + 22, native: true)
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
