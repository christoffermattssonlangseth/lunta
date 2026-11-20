import SwiftUI
import WebKit
import PDFKit
import LuntaCore

/// One paper, two views: the reflowable reading view (converted EPUB) and the original PDF.
struct ReaderWindow: View {
    @EnvironmentObject var model: LibraryModel
    let paperID: String
    @State private var mode: Mode = .reading
    @State private var book: EpubBook?
    @State private var current: String?          // anchor of the section being read
    @State private var scrollRequest = ScrollRequest()
    @AppStorage("readerFontScale") private var fontScale = 1.0
    @AppStorage("readerPagedMac") private var paged = false
    @AppStorage("readerTheme") private var themeRaw = ReaderTheme.automatic.rawValue
    private var theme: ReaderTheme { ReaderTheme(rawValue: themeRaw) ?? .automatic }
    @AppStorage("highlightMode") private var highlightMode = false
    @AppStorage("highlightModeColor") private var highlightModeColor = HighlightColor.yellow.rawValue
    @State private var autoHighlighter = AutoHighlighter()
    @State private var figure: ReaderFigure?
    @State private var showFigures = false
    @State private var error: String?
    /// The PDF is in iCloud or Google Drive and couldn't be downloaded: why (with Try Again).
    @State private var pdfProblem: String?
    /// Bumped when the PDF has been downloaded, to show it.
    @State private var pdfArrived = 0
    @StateObject private var narrator = Narrator()
    @StateObject private var controller = ReaderController()
    @StateObject private var position = PositionKeeper()
    @StateObject private var summary = SummaryMaker()
    @AppStorage(SummaryMaker.key) private var summariesOn = true
    @State private var listening = false
    @State private var asking = false
    @State private var editingAnnotation: Annotation?
    @StateObject private var pdfHighlights = PDFHighlighter()
    @StateObject private var find = PaperFind()
    @State private var goToPage = 0
    @State private var pdfFinder = PDFFinder()
    @State private var showHighlights = false
    @State private var sideTab: SideTab = .contents
    @StateObject private var references = ReferencesModel()
    @StateObject private var ask: AskEngine
    @Environment(\.openWindow) private var openWindow
    /// Undo for highlights and notes in this window (⌘Z / ⇧⌘Z, see ReaderUndoKeys).
    @State private var undo = UndoManager()

    init(paperID: String) {
        self.paperID = paperID
        _ask = StateObject(wrappedValue: AskEngine(scope: .paper(paperID)))
        // this paper is being read aloud (its window was closed): take the narration over
        let playing = NowPlaying.shared.narrator(for: paperID)
        _narrator = StateObject(wrappedValue: playing ?? Narrator())
        _listening = State(initialValue: playing != nil)
    }

    enum Mode: String, CaseIterable { case reading = "Reading", pdf = "PDF" }
    /// The reading view's sidebar: its contents, or its reference list.
    enum SideTab: String, CaseIterable { case contents = "Contents", references = "References" }

    /// The view a reader window should show when it opens (or switch to, if it's already open).
    static var requestedMode: [String: Mode] = [:]

    static func open(_ id: String, mode: Mode, _ openWindow: OpenWindowAction) {
        requestedMode[id] = mode
        NotificationCenter.default.post(name: .luntaReaderMode, object: id)
        openWindow(value: id)
    }

    var body: some View {
        let paper = model.paper(paperID)
        NavigationSplitView {
            if mode == .reading, let book {
                VStack(spacing: 0) {
                    Picker("", selection: $sideTab) { ForEach(SideTab.allCases, id: \.self) { Text($0.rawValue).tag($0) } }
                        .pickerStyle(.segmented).labelsHidden()
                        .padding(.horizontal, 12).padding(.vertical, 8)
                    switch sideTab {
                    case .contents:
                        List(selection: Binding(get: { current }, set: { id in
                            if let id { current = id; scrollRequest = ScrollRequest(anchor: id) }
                        })) {
                            ForEach(Array(book.toc.enumerated()), id: \.offset) { _, t in
                                Text(t.title).lineLimit(2)
                                    .padding(.leading, CGFloat(max(0, Int(t.level) - 1)) * 12)
                                    .font(t.level <= 1 ? .body.weight(.medium) : .callout)
                                    .foregroundStyle(t.level <= 1 ? .primary : .secondary)
                                    .tag(anchor(t, in: book))
                            }
                        }
                    case .references:
                        ReferencesList(refs: references,
                                       onShow: { r in controller.webView?.evaluateJavaScript("window.luntaShowReference && luntaShowReference(\(r.index))") },
                                       onOpen: { id in openWindow(value: id) })
                            .task(id: book.combined) { await references.load(controller, model: model) }
                    }
                }
                .navigationSplitViewColumnWidth(min: 180, ideal: 250)
            } else if mode == .pdf {
                PDFSidebar(paperID: paperID, highlighter: pdfHighlights) { a in editingAnnotation = a }
                    .navigationSplitViewColumnWidth(min: 180, ideal: 250)
            } else {
                Text(paper?.meta.title ?? "").padding().navigationSplitViewColumnWidth(min: 180, ideal: 250)
            }
        } detail: {
            Group {
                switch mode {
                case .reading:
                    if let book {
                        EpubWebView(url: URL(fileURLWithPath: book.combined), readAccess: URL(fileURLWithPath: book.dir),
                                    fontScale: fontScale, theme: theme, paged: paged, findBar: find.shown, scrollRequest: scrollRequest, controller: controller) { id in current = id }
                            .safeAreaInset(edge: .bottom) {
                                if listening { NarrationBar(narrator: narrator) { stopListening() } }
                            }
                            .overlay(alignment: .top) { ResumeNote(text: position.note).animation(.easeOut, value: position.note) }
                    } else if let error {
                        readingError(error)
                    } else {
                        ProgressView()
                    }
                case .pdf:
                    pdfPane
                }
            }
            // retracted, corrected, or a preprint since published (Checks.swift)
            .safeAreaInset(edge: .top, spacing: 0) { UpdatesBanner(paperID: paperID) }
            // another paper being read aloud (or this one, in the PDF): its Now Playing bar
            .overlay(alignment: .bottom) {
                NowPlayingBar(style: .capsule, except: listening && mode == .reading ? narrator : nil)
                    .padding(.horizontal, 20).padding(.bottom, 14)
            }
            // find in paper (⌘F): over the page, so the page keeps its size (and its pages)
            .modifier(FindOverlay(find: find, theme: mode == .reading ? theme : nil))
            .toolbar {
                ToolbarItem(placement: .principal) {
                    Picker("View", selection: $mode) { ForEach(Mode.allCases, id: \.self) { Text($0.rawValue) } }
                        .pickerStyle(.segmented).frame(width: 180)
                }
                ToolbarItemGroup {
                    if mode == .reading {
                        Button { listening ? stopListening() : startListening() } label: {
                            Image(systemName: listening ? "headphones.circle.fill" : "headphones")
                        }
                        .help(listening ? "Stop listening" : "Listen to this paper")
                        .disabled(book == nil)
                        if paper?.hasAudio != true {
                            Button { NarrationRenderer.shared.enqueue([paperID]) } label: {
                                Image(systemName: NarrationRenderer.shared.rendering.contains(paperID) ? "waveform.badge.ellipsis" : "waveform.badge.plus")
                            }
                            .help("Make natural-voice narration (renders on this Mac, syncs to iPhone)")
                            .disabled(book == nil || NarrationRenderer.shared.rendering.contains(paperID))
                        }
                        // click: highlight mode on/off (every selection is highlighted at once);
                        // the menu: highlight the selection now, and the colour
                        // highlight mode as an on/off button (the toolbar shows it filled while on);
                        // right-click: highlight the selection now, and the colour
                        Toggle(isOn: $highlightMode) {
                            Image(systemName: "highlighter")
                                .foregroundStyle(highlightMode ? hlColor.color : Color.primary)
                        }
                        .toggleStyle(.button)
                        .contextMenu {
                            Button("Highlight Selection") { highlightSelection(hlColor) }
                            Picker("Color", selection: $highlightModeColor) {
                                ForEach(HighlightColor.allCases) { c in Text(c.name).tag(c.rawValue) }
                            }
                        }
                        .help(highlightMode ? "Highlight mode is on: selections are highlighted at once (⌃⌘H). Right-click for the color."
                                            : "Highlight mode: highlight every selection at once (⌃⌘H). Right-click for the color; ⇧⌘H highlights one selection.")
                        .disabled(book == nil && mode != .pdf)
                        .background {
                            // keyboard shortcuts for the two actions above
                            Group {
                                Button("") { highlightSelection(hlColor) }.keyboardShortcut("h", modifiers: [.command, .shift])
                                Button("") { highlightMode.toggle() }.keyboardShortcut("h", modifiers: [.command, .control])
                            }
                            .opacity(0).frame(width: 0, height: 0).accessibilityHidden(true)
                        }
                        Button { showHighlights.toggle() } label: {
                            Image(systemName: "list.bullet.rectangle")
                        }
                        .help("Highlights and notes")
                        .disabled(book == nil && mode != .pdf)
                        .popover(isPresented: $showHighlights, arrowEdge: .bottom) {
                            AnnotationsList(paperID: paperID, missing: mode == .reading ? controller.missingAnnotations : [], onShow: { a in
                                                if mode == .pdf { pdfHighlights.show(a) } else { controller.showAnnotation(a.id) } },
                                            onEdit: { a in showHighlights = false; editingAnnotation = a })
                                .frame(width: 380, height: 460)
                                .environmentObject(model)
                        }
                        FiguresButton(controller: controller) { showFigures = true }
                        Button { asking.toggle() } label: { Image(systemName: asking ? "sparkles.rectangle.stack.fill" : "sparkles") }
                            .help("Ask about this paper (Apple Intelligence)")
                            .keyboardShortcut("j", modifiers: .command)
                            .disabled(book == nil)
                        Button { paged.toggle() } label: { Image(systemName: paged ? "book.fill" : "book") }
                            .help(paged ? "Scroll instead of turning pages" : "Turn pages (arrow keys, space or swipe)")
                            .keyboardShortcut("p", modifiers: [.command, .option])
                            .disabled(book == nil)
                        Menu {
                            Picker("Theme", selection: $themeRaw) {
                                ForEach(ReaderTheme.allCases) { t in Text(t.name).tag(t.rawValue) }
                            }
                            .pickerStyle(.inline)
                        } label: { Image(systemName: "circle.lefthalf.filled") }
                        .help("Theme: \(theme.name)")
                        Button { fontScale = max(0.7, fontScale - 0.1) } label: { Image(systemName: "textformat.size.smaller") }
                            .help("Smaller text")
                        Button { fontScale = min(1.8, fontScale + 0.1) } label: { Image(systemName: "textformat.size.larger") }
                            .help("Larger text")
                    }
                }
            }
            // on the detail, not the split view: there it kept the toolbar re-laying itself out
            .inspector(isPresented: $asking) {
                AssistantPanel(engine: ask, onPassage: { id in mode = .reading; controller.flash(id) },
                               onPaper: { id in openWindow(value: id) })
                    .inspectorColumnWidth(min: 300, ideal: 380, max: 560)
            }
        }
        .figureOverlay($figure)
        .readerFigures(controller, browsing: $showFigures)
        .sheet(item: $editingAnnotation) { a in AnnotationEditor(annotation: a) { refreshHighlights() } }
        .onChange(of: paper?.annotationCount) { refreshHighlights() }
        .onChange(of: highlightMode) { _, on in controller.setHighlightMode(on, color: hlColor) }
        .onChange(of: highlightModeColor) { controller.setHighlightMode(highlightMode, color: hlColor) }
        .onAppear { model.reading.insert(paperID) }
        .onDisappear { model.reading.remove(paperID); position.stop(paperID, model: model, controller: controller); summary.stop() }
        .onChange(of: summariesOn) { _, on in summary.setEnabled(on) }
        // stopped from the Now Playing bar or the Listen menu, or another paper is read aloud now
        .onReceive(NowPlaying.shared.settled) { if listening, Narrator.active !== narrator { closePlayer() } }
        .onReceive(NotificationCenter.default.publisher(for: .luntaReaderMode)) { n in
            if n.object as? String == paperID, let m = Self.requestedMode.removeValue(forKey: paperID) { mode = m }
        }
        .environment(\.highlightUndo, undo)
        .background(ReaderUndoKeys(undo: undo))
        .modifier(ReaderFindHooks(find: find, backend: findBackend, webView: mode == .reading ? controller.webView : nil))
        .modifier(GoToPageModifier(request: goToPage, pdf: mode == .pdf, webView: { controller.webView }, pdfView: { pdfHighlights.view }))
        .focusedSceneValue(\.goToPage, (mode == .pdf || book != nil) ? GoToPageAction { goToPage += 1 } : nil)
        // an undone or redone highlight: redraw (the reading view; the PDF follows the model)
        .onReceive(NotificationCenter.default.publisher(for: .NSUndoManagerDidUndoChange, object: undo)) { _ in refreshHighlights() }
        .onReceive(NotificationCenter.default.publisher(for: .NSUndoManagerDidRedoChange, object: undo)) { _ in refreshHighlights() }
        .navigationTitle(paper?.meta.title ?? "Reader")
        .task {
            controller.onFigure = { figure = $0 }
            controller.onPageReady = {
                controller.setHighlightMode(highlightMode, color: hlColor)
                refreshHighlights(); position.pageReady(paperID, model: model, controller: controller)
                summary.pageReady(paperID, model: model, controller: controller)
                resumeListening()
            }
            controller.onSelectionDone = { pageSel in
                guard highlightMode, mode == .reading else { return }
                Task {
                    var sel = pageSel
                    if sel == nil { sel = await controller.pageSelection() }
                    guard let sel else { refreshHighlights(); return }
                    await autoHighlighter.apply(sel, paper: paperID, color: hlColor, model: model, controller: controller, undo: undo)
                    controller.clearSelection()
                    refreshHighlights()
                }
            }
            controller.onAnnotation = { id in editingAnnotation = model.annotations(paperID).first { $0.id == id } }
            controller.onMenu = { action in
                switch action {
                case .highlight(let c): highlightSelection(c)
                case .note: noteOnSelection()
                }
            }
            ask.model = model
            ask.passages = { [weak controller] in await controller?.narrationItems(includeCaptions: true) ?? [] }
            ask.selection = { [weak controller] in await controller?.selectedText() }
            pdfFinder.highlighter = pdfHighlights
            position.onSettled = { [weak find] in find?.pageSettled() }
            // warm up the natural voice so Listen starts right away
            if KokoroLive.shared.installed { Task { _ = await KokoroLive.shared.start() } }
            await load()
            find.use(findBackend)
            if mode == .pdf { try? await Task.sleep(for: .milliseconds(500)); find.pageSettled() }
        }
    }

    /// The reading view couldn't be opened (not made yet, or not downloaded): why, with Try Again.
    private func readingError(_ error: String) -> some View {
        ContentUnavailableView {
            Label("No reading view", systemImage: "book")
        } description: {
            Text(error)
        } actions: {
            if model.paper(paperID)?.hasEpub == true { Button("Try Again") { self.error = nil; Task { await load() } } }
        }
    }

    /// The PDF: shown, downloaded first (iCloud or Google Drive), or not there.
    @ViewBuilder
    private var pdfPane: some View {
        if let url = model.fileURL(paperID, .pdf) {
            PDFReader(url: url, paperID: paperID, highlighter: pdfHighlights) { a in editingAnnotation = a }
                .id(pdfArrived)
        } else if model.paper(paperID)?.hasPdf == true, let pdfProblem {
            ContentUnavailableView {
                Label("PDF Not Downloaded", systemImage: "icloud.slash")
            } description: {
                Text(pdfProblem)
            } actions: {
                Button("Try Again") { self.pdfProblem = nil }
            }
        } else if model.paper(paperID)?.hasPdf == true {
            // in iCloud or Google Drive: downloaded now
            ProgressView("Downloading the PDF from \(model.downloadSource)…")
                .task {
                    switch await model.fetchFile(paperID, .pdf) {
                    case .ready: pdfArrived += 1
                    case .failed(let why): pdfProblem = why
                    case .missing: break
                    }
                }
        } else {
            ContentUnavailableView("No PDF", systemImage: "doc", description: Text("Use Find PDF in the library, or drop the PDF onto the library window."))
        }
    }

    private func load() async {
        guard let p = model.paper(paperID) else { return }
        // "Remove Downloads Not Opened in …": every open counts, also a PDF that is already here
        // (fetchFile records only what it is asked for, and the PDF pane doesn't ask when it's local)
        if model.onDemand != nil { DriveLibrarySync.used(paperID) }
        if let m = Self.requestedMode.removeValue(forKey: paperID) { mode = m }
        if !p.hasEpub { mode = p.hasPdf ? .pdf : .reading; error = "Use “Make Reading View” in the library."; return }
        switch await model.fetchFile(paperID, .epub) {
        case .ready: break
        case .failed(let why): error = "The reading view isn’t downloaded. \(why)"; return
        case .missing: error = "Use “Make Reading View” in the library."; return
        }
        do {
            book = try model.lib.openEpub(id: paperID)
            model.markOpened(p)
        } catch {
            self.error = error.readable
        }
    }

    /// What the find bar searches: the reading view or the PDF.
    private var findBackend: FindBackend? { mode == .pdf ? pdfFinder : (book != nil ? controller : nil) }

    private var hlColor: HighlightColor { HighlightColor(rawValue: highlightModeColor) ?? .yellow }

    private func refreshHighlights() {
        controller.showAnnotations(model.annotations(paperID))
        controller.setLibraryIDs(model.libraryIDs())   // the citation card: "In library" for those
    }

    private func highlightSelection(_ c: HighlightColor) {
        if mode == .pdf {
            guard let sel = pdfHighlights.selection() else { return }
            model.addHighlight(paperID, sel, color: c, location: "pdf", undo: undo)
            pdfHighlights.clearSelection()
            pdfHighlights.apply(model.annotations(paperID))
            return
        }
        Task {
            guard let sel = await controller.pageSelection() else { return }
            _ = await model.addHighlightMerging(paperID, sel, color: c, controller: controller, undo: undo)
            controller.clearSelection()
            refreshHighlights()
        }
    }

    private func noteOnSelection() {
        Task {
            guard let sel = await controller.pageSelection(),
                  let a = await model.addHighlightMerging(paperID, sel, controller: controller, undo: undo) else { return }
            controller.clearSelection()
            refreshHighlights()
            editingAnnotation = a
        }
    }

    private func startListening() {
        Task {
            let items = await controller.narrationItems()
            guard !items.isEmpty else { return }
            // natural (neural) narration when it has been rendered, otherwise the system voice
            let title = model.paper(paperID)?.meta.title ?? ""
            if await model.loadNeuralNarration(paperID, into: narrator, pageItems: items) {
                // a full natural-voice narration was rendered earlier
            } else if KokoroLive.shared.installed {
                // natural voice, made paragraph by paragraph as it plays
                narrator.loadLive(items, title: title) { item in
                    await KokoroLive.shared.clip(Narrator.spoken(item), kind: item.kind)
                }
            } else {
                narrator.load(items, title: title)
            }
            narrator.paperID = paperID
            narrator.onHighlight = { [weak controller] id in controller?.highlight(id) }
            controller.onTapParagraph = { [weak narrator] id in narrator?.play(fromID: id) }
            controller.setListening(true)
            listening = true
            // start where the reader is looking (skip the title page if at the top)
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
        controller.onTapParagraph = { [weak narrator] id in narrator?.play(fromID: id) }
        controller.setListening(true)
        if narrator.items.indices.contains(narrator.index) { controller.highlight(narrator.items[narrator.index].id) }
    }

    /// Contents entries point at "section.xhtml#id" or at a whole section; in the continuous
    /// document these become "#id" and "#section-<n>".
    private func anchor(_ t: TocEntry, in book: EpubBook) -> String {
        let parts = t.target.split(separator: "#", maxSplits: 1).map(String.init)
        if parts.count == 2 { return parts[1] }
        let i = book.chapters.firstIndex { $0.path == parts[0] } ?? 0
        return "section-\(i)"
    }
}

struct ScrollRequest: Equatable {
    var anchor: String?
    var serial = UUID()
}

/// The continuous reading view: the whole paper in one scrolling page with reader styling.
struct EpubWebView: NSViewRepresentable {
    let url: URL
    let readAccess: URL
    let fontScale: Double
    var theme: ReaderTheme = .automatic
    var paged = false
    /// The find bar is over the page: pages start below it.
    var findBar = false
    let scrollRequest: ScrollRequest
    let controller: ReaderController
    let onPosition: (String) -> Void

    private func pagedState(_ zoom: Double) -> PagedState {
        let z = max(zoom, 0.5)
        return PagedState(on: paged, top: (findBar && paged ? 64 : 30) / z, bottom: 40 / z, native: false)
    }

    func makeNSView(context: Context) -> WKWebView {
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
        cfg.userContentController.addUserScript(WKUserScript(source: ReaderAssets.findJS, injectionTime: .atDocumentEnd, forMainFrameOnly: true))
        let state = pagedState(fontScale)
        context.coordinator.paging = cfg.addPaging(state)
        context.coordinator.applied = state
        let v = ReaderWebView(frame: .zero, configuration: cfg)
        v.controller = controller
        v.setValue(false, forKey: "drawsBackground")
        v.navigationDelegate = ExternalLinks.shared   // web links open in the browser, not in the reader
        v.appearance = Self.appearance(theme)   // only the page: the window keeps the system's look
        controller.webView = v
        v.loadFileURL(url, allowingReadAccessTo: readAccess)
        context.coordinator.loaded = url
        return v
    }

    static func appearance(_ t: ReaderTheme) -> NSAppearance? {
        t.scheme.map { NSAppearance(named: $0 == .dark ? .darkAqua : .aqua) } ?? nil
    }

    func updateNSView(_ v: WKWebView, context: Context) {
        if context.coordinator.theme != theme {
            context.coordinator.theme = theme
            v.evaluateJavaScript(theme.js)
            v.appearance = Self.appearance(theme)
        }
        context.coordinator.onPosition = onPosition
        v.pageZoom = fontScale
        let state = pagedState(fontScale)
        if state != context.coordinator.applied {
            let turnedOn = state.on && context.coordinator.applied?.on != true
            context.coordinator.applied = state
            context.coordinator.paging?.state = state
            v.evaluateJavaScript(state.call)
            // keys go to the page (when pages are turned on; not when only the margins changed,
            // e.g. for the find bar, whose field has the keys)
            if turnedOn { v.window?.makeFirstResponder(v) }
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

    final class Coordinator: NSObject, WKScriptMessageHandler {
        var theme: ReaderTheme = .automatic
        var loaded: URL?
        var lastScroll = ScrollRequest()
        var onPosition: (String) -> Void
        var paging: PageMessageHandler?
        var applied: PagedState?
        init(onPosition: @escaping (String) -> Void) { self.onPosition = onPosition }
        func userContentController(_ c: WKUserContentController, didReceive m: WKScriptMessage) {
            if let id = m.body as? String { onPosition(id) }
        }
    }
}

extension Notification.Name {
    static let luntaReaderMode = Notification.Name("luntaReaderMode")
}

/// The sidebar in PDF mode: the paper, then its pages, its own outline (when it has one) and
/// your highlights.
struct PDFSidebar: View {
    @EnvironmentObject var model: LibraryModel
    let paperID: String
    @ObservedObject var highlighter: PDFHighlighter
    var onEdit: (Annotation) -> Void
    @State private var tab: Tab = .pages

    enum Tab: String, CaseIterable { case pages = "Pages", contents = "Contents", highlights = "Highlights" }

    private struct OutlineItem: Identifiable {
        let id: Int
        let label: String
        let level: Int
        let outline: PDFOutline
    }

    private var outline: [OutlineItem] {
        guard let root = highlighter.document?.outlineRoot else { return [] }
        var out: [OutlineItem] = []
        func walk(_ o: PDFOutline, _ level: Int) {
            for i in 0..<o.numberOfChildren {
                guard let c = o.child(at: i) else { continue }
                let label = c.label?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
                if !label.isEmpty { out.append(OutlineItem(id: out.count, label: label, level: level, outline: c)) }
                walk(c, level + 1)
            }
        }
        walk(root, 0)
        return out
    }

    var body: some View {
        let items = outline
        let tabs = Tab.allCases.filter { $0 != .contents || !items.isEmpty }
        VStack(alignment: .leading, spacing: 0) {
            if let p = model.paper(paperID) {
                VStack(alignment: .leading, spacing: 4) {
                    Text(p.meta.title.isEmpty ? "Untitled" : p.meta.title).font(.headline).lineLimit(4)
                    if !p.meta.authorLine.isEmpty {
                        Text(p.meta.authorLine).font(.caption).foregroundStyle(.secondary).lineLimit(2)
                    }
                    let venue = [p.meta.journal, p.meta.year].filter { !$0.isEmpty }.joined(separator: " · ")
                    if !venue.isEmpty { Text(venue).font(.caption).foregroundStyle(.secondary).lineLimit(1) }
                }
                .padding(.horizontal, 14).padding(.top, 8).padding(.bottom, 10)
            }
            Picker("", selection: $tab) { ForEach(tabs, id: \.self) { Text($0.rawValue).tag($0) } }
                .pickerStyle(.segmented).labelsHidden()
                .padding(.horizontal, 12).padding(.bottom, 8)
            Divider()
            switch tabs.contains(tab) ? tab : .pages {
            case .pages:
                if let view = highlighter.view, highlighter.document != nil {
                    PDFThumbnails(pdfView: view)
                } else {
                    ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
                }
            case .contents:
                List(items) { item in
                    Button { go(item.outline) } label: {
                        Text(item.label).lineLimit(2)
                            .font(item.level == 0 ? .body.weight(.medium) : .callout)
                            .foregroundStyle(item.level == 0 ? .primary : .secondary)
                            .padding(.leading, CGFloat(item.level) * 12)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                }
                .listStyle(.sidebar)
            case .highlights:
                AnnotationsList(paperID: paperID, onShow: { highlighter.show($0) }, onEdit: onEdit)
            }
        }
    }

    private func go(_ o: PDFOutline) {
        guard let v = highlighter.view else { return }
        if let d = o.destination { v.go(to: d) }
        else if let a = o.action as? PDFActionGoTo { v.go(to: a.destination) }
    }
}

/// Page thumbnails that follow (and move) a PDF view.
struct PDFThumbnails: NSViewRepresentable {
    let pdfView: PDFView
    func makeNSView(context: Context) -> PDFThumbnailView {
        let t = PDFThumbnailView()
        t.pdfView = pdfView
        t.maximumNumberOfColumns = 1
        t.thumbnailSize = CGSize(width: 150, height: 194)
        t.backgroundColor = .clear
        return t
    }
    func updateNSView(_ t: PDFThumbnailView, context: Context) {
        if t.pdfView !== pdfView { t.pdfView = pdfView }
    }
}

/// ⌘Z and ⇧⌘Z in a reader window undo and redo highlights and notes. Caught before the window
/// handles them, since the reading view (a web view) would take them for its own, empty, undo;
/// left alone while text is being typed (a note, the Ask field), where they undo the typing.
struct ReaderUndoKeys: NSViewRepresentable {
    let undo: UndoManager

    func makeNSView(context: Context) -> KeyView { let v = KeyView(); v.undo = undo; return v }
    func updateNSView(_ v: KeyView, context: Context) { v.undo = undo }

    final class KeyView: NSView {
        var undo: UndoManager?
        private var monitor: Any?

        override func hitTest(_ point: NSPoint) -> NSView? { nil }

        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            if let monitor { NSEvent.removeMonitor(monitor); self.monitor = nil }
            guard window != nil else { return }
            monitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] e in
                guard let self, let win = self.window, e.window === win, let undo = self.undo,
                      e.charactersIgnoringModifiers?.lowercased() == "z" else { return e }
                let mods = e.modifierFlags.intersection([.command, .shift, .option, .control])
                guard mods == .command || mods == [.command, .shift] else { return e }
                if let t = win.firstResponder as? NSTextView, t.isEditable { return e }   // typing
                if mods == .command { if undo.canUndo { undo.undo() } } else if undo.canRedo { undo.redo() }
                return nil
            }
        }

        deinit { if let monitor { NSEvent.removeMonitor(monitor) } }
    }
}
