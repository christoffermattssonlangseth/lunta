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
    @State private var figure: ReaderFigure?
    @State private var error: String?
    @StateObject private var narrator = Narrator()
    @StateObject private var controller = ReaderController()
    @StateObject private var position = PositionKeeper()
    @State private var listening = false
    @State private var asking = false
    @State private var editingAnnotation: Annotation?
    @StateObject private var pdfHighlights = PDFHighlighter()
    @State private var showHighlights = false
    @StateObject private var ask: AskEngine
    @Environment(\.openWindow) private var openWindow
    /// Undo for highlights and notes in this window (⌘Z / ⇧⌘Z, see ReaderUndoKeys).
    @State private var undo = UndoManager()

    init(paperID: String) {
        self.paperID = paperID
        _ask = StateObject(wrappedValue: AskEngine(scope: .paper(paperID)))
    }

    enum Mode: String, CaseIterable { case reading = "Reading", pdf = "PDF" }

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
                                    fontScale: fontScale, paged: paged, scrollRequest: scrollRequest, controller: controller) { id in current = id }
                            .safeAreaInset(edge: .bottom) {
                                if listening { NarrationBar(narrator: narrator) { stopListening() } }
                            }
                            .overlay(alignment: .top) { ResumeNote(text: position.note).animation(.easeOut, value: position.note) }
                    } else if let error {
                        ContentUnavailableView("No reading view", systemImage: "book", description: Text(error))
                    } else {
                        ProgressView()
                    }
                case .pdf:
                    if let url = model.fileURL(paperID, .pdf) {
                        PDFReader(url: url, paperID: paperID, highlighter: pdfHighlights) { a in editingAnnotation = a }
                    } else {
                        ContentUnavailableView("No PDF", systemImage: "doc", description: Text("Use Find PDF in the library, or drop the PDF onto the library window."))
                    }
                }
            }
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
                        Button { highlightSelection(.yellow) } label: { Image(systemName: "highlighter") }
                            .help("Highlight the selected text (⇧⌘H). Right-click a selection for colors and notes.")
                            .keyboardShortcut("h", modifiers: [.command, .shift])
                            .disabled(book == nil && mode != .pdf)
                        Button { showHighlights.toggle() } label: {
                            Image(systemName: "list.bullet.rectangle")
                        }
                        .help("Highlights and notes")
                        .disabled(book == nil && mode != .pdf)
                        .popover(isPresented: $showHighlights, arrowEdge: .bottom) {
                            AnnotationsList(paperID: paperID, onShow: { a in
                                                if mode == .pdf { pdfHighlights.show(a) } else { controller.showAnnotation(a.id) } },
                                            onEdit: { a in showHighlights = false; editingAnnotation = a })
                                .frame(width: 380, height: 460)
                                .environmentObject(model)
                        }
                        Button { asking.toggle() } label: { Image(systemName: asking ? "sparkles.rectangle.stack.fill" : "sparkles") }
                            .help("Ask about this paper (Apple Intelligence)")
                            .keyboardShortcut("j", modifiers: .command)
                            .disabled(book == nil)
                        Button { paged.toggle() } label: { Image(systemName: paged ? "book.fill" : "book") }
                            .help(paged ? "Scroll instead of turning pages" : "Turn pages (arrow keys, space or swipe)")
                            .keyboardShortcut("p", modifiers: [.command, .option])
                            .disabled(book == nil)
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
        .sheet(item: $figure) { f in FigureViewer(figure: f) }
        .sheet(item: $editingAnnotation) { a in AnnotationEditor(annotation: a) { refreshHighlights() } }
        .onChange(of: paper?.annotationCount) { refreshHighlights() }
        .onDisappear { position.stop(paperID, model: model, controller: controller) }
        .onReceive(NotificationCenter.default.publisher(for: .luntaReaderMode)) { n in
            if n.object as? String == paperID, let m = Self.requestedMode.removeValue(forKey: paperID) { mode = m }
        }
        .environment(\.highlightUndo, undo)
        .background(ReaderUndoKeys(undo: undo))
        // an undone or redone highlight: redraw (the reading view; the PDF follows the model)
        .onReceive(NotificationCenter.default.publisher(for: .NSUndoManagerDidUndoChange, object: undo)) { _ in refreshHighlights() }
        .onReceive(NotificationCenter.default.publisher(for: .NSUndoManagerDidRedoChange, object: undo)) { _ in refreshHighlights() }
        .navigationTitle(paper?.meta.title ?? "Reader")
        .task {
            controller.onFigure = { figure = $0 }
            controller.onPageReady = { refreshHighlights(); position.pageReady(paperID, model: model, controller: controller) }
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
            // warm up the natural voice so Listen starts right away
            if KokoroLive.shared.installed { Task { _ = await KokoroLive.shared.start() } }
            await load()
        }
    }

    private func load() async {
        guard let p = model.paper(paperID) else { return }
        if let m = Self.requestedMode.removeValue(forKey: paperID) { mode = m }
        if !p.hasEpub { mode = p.hasPdf ? .pdf : .reading; error = "Use “Make Reading View” in the library."; return }
        guard await model.ensureLocal(paperID, .epub) != nil else { error = "Couldn't download the reading view from iCloud."; return }
        do {
            book = try model.lib.openEpub(id: paperID)
            model.markOpened(p)
        } catch {
            self.error = error.readable
        }
    }

    private func refreshHighlights() { controller.showAnnotations(model.annotations(paperID)) }

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
            model.addHighlight(paperID, sel, color: c, undo: undo)
            controller.clearSelection()
            refreshHighlights()
        }
    }

    private func noteOnSelection() {
        Task {
            guard let sel = await controller.pageSelection(), let a = model.addHighlight(paperID, sel, undo: undo) else { return }
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
        controller.setListening(false)
        listening = false
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
    var paged = false
    let scrollRequest: ScrollRequest
    let controller: ReaderController
    let onPosition: (String) -> Void

    private func pagedState(_ zoom: Double) -> PagedState {
        let z = max(zoom, 0.5)
        return PagedState(on: paged, top: 30 / z, bottom: 40 / z, native: false)
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
        cfg.addAnnotations(controller)
        let state = pagedState(fontScale)
        context.coordinator.paging = cfg.addPaging(state)
        context.coordinator.applied = state
        let v = ReaderWebView(frame: .zero, configuration: cfg)
        v.controller = controller
        v.setValue(false, forKey: "drawsBackground")
        controller.webView = v
        v.loadFileURL(url, allowingReadAccessTo: readAccess)
        context.coordinator.loaded = url
        return v
    }

    func updateNSView(_ v: WKWebView, context: Context) {
        context.coordinator.onPosition = onPosition
        v.pageZoom = fontScale
        let state = pagedState(fontScale)
        if state != context.coordinator.applied {
            context.coordinator.applied = state
            context.coordinator.paging?.state = state
            v.evaluateJavaScript(state.call)
            // keys go to the page
            if state.on { v.window?.makeFirstResponder(v) }
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
