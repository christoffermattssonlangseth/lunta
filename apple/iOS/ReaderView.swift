import SwiftUI
import WebKit
import PDFKit
import LuntaCore

/// Full-screen reader: the continuous reading view (reflowed text) or the original PDF.
struct ReaderView: View {
    @EnvironmentObject var model: LibraryModel
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
    @State private var figure: ReaderFigure?
    @State private var sharing = false
    @State private var organizing = false
    @State private var editing = false
    @State private var chromeHidden = false
    @State private var editingAnnotation: Annotation?
    @State private var showHighlights = false
    @State private var current: String?
    @State private var scrollRequest = ScrollRequest()
    @StateObject private var narrator = Narrator()
    @StateObject private var controller = ReaderController()
    @StateObject private var position = PositionKeeper()
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
                               fontScale: fontScale, paged: paged, scrollRequest: scrollRequest, controller: controller,
                               onPinch: { step in fontScale = min(2.2, max(0.7, ((fontScale + step * 0.1) * 10).rounded() / 10)) },
                               onToggleChrome: {
                                   withAnimation(.easeInOut(duration: 0.22)) { chromeHidden.toggle() }
                               },
                               onChrome: { hide in
                                   guard hide != chromeHidden, !(hide && listening) else { return }
                                   withAnimation(.easeInOut(duration: 0.22)) { chromeHidden = hide }
                               }) { current = $0 }
                    // pages stay put when the bars come and go: the view always fills the screen
                    .ignoresSafeArea(edges: paged ? (listening ? .top : .all) : (listening ? [] : .bottom))
                    .safeAreaInset(edge: .bottom) {
                        if listening { NarrationBar(narrator: narrator) { stopListening() } }
                    }
                    .overlay(alignment: .top) { ResumeNote(text: position.note).animation(.easeOut, value: position.note) }
            } else if mode == .pdf, let pdfURL {
                PDFKitViewIOS(url: pdfURL).ignoresSafeArea(edges: .bottom)
            } else {
                PaperInfoView(paperID: paperID, embedded: true)
            }
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
                if mode == .reading, book != nil {
                    Button { listening ? stopListening() : startListening() } label: {
                        Image(systemName: listening ? "headphones.circle.fill" : "headphones")
                    }
                    Button { asking = true } label: { Image(systemName: "sparkles") }
                    Button { showContents = true } label: { Image(systemName: "list.bullet") }
                }
                Menu {
                    if mode == .reading, book != nil {
                        ControlGroup {
                            Button { fontScale = max(0.7, fontScale - 0.1) } label: { Label("Smaller", systemImage: "textformat.size.smaller") }
                            Button { fontScale = min(2.2, fontScale + 0.1) } label: { Label("Larger", systemImage: "textformat.size.larger") }
                        } label: { Text("Text Size") }
                        Picker(selection: $paged) {
                            Label("Scroll", systemImage: "scroll").tag(false)
                            Label("Turn Pages", systemImage: "book").tag(true)
                        } label: { Label("Layout", systemImage: paged ? "book" : "scroll") }
                        .pickerStyle(.menu)
                        Picker(selection: Binding(get: { PhoneVoice.shared.voice }, set: { PhoneVoice.shared.voice = $0 })) {
                            ForEach(PhoneVoice.voices, id: \.id) { Text($0.name).tag($0.id) }
                        } label: { Label("Natural Voice", systemImage: "waveform") }
                        .pickerStyle(.menu)
                        Divider()
                    }
                    if mode == .reading, book != nil {
                        Button { showHighlights = true } label: { Label("Highlights & Notes", systemImage: "highlighter") }
                    }
                    Button { organizing = true } label: { Label("Folders & Tags…", systemImage: "folder") }
                    Button { editing = true } label: { Label("Edit Details…", systemImage: "pencil") }
                    Button { sharing = true } label: { Label("Share…", systemImage: "square.and.arrow.up") }
                    Button { showInfo = true } label: { Label("Paper Info", systemImage: "info.circle") }
                } label: { Image(systemName: "ellipsis.circle") }
            }
        }
        .sheet(item: $editingAnnotation) { a in AnnotationEditor(annotation: a) { refreshHighlights() }.environmentObject(model) }
        .sheet(isPresented: $showHighlights) {
            NavigationStack {
                AnnotationsList(paperID: paperID, onShow: { a in
                    showHighlights = false
                    Task { try? await Task.sleep(for: .milliseconds(350)); controller.showAnnotation(a.id) }
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
        .fullScreenCover(item: $figure) { f in FigureViewer(figure: f) }
        .sheet(isPresented: $sharing) {
            ScrollView { SharePanel(ids: [paperID]) }.environmentObject(model).presentationDetents([.medium, .large])
        }
        .sheet(isPresented: $showInfo) { NavigationStack { PaperInfoView(paperID: paperID, embedded: false) } }
        .onChange(of: mode) { _, m in if m == .pdf, pdfURL == nil { Task { await loadPDF() } } }
        .task {
            controller.onFigure = { figure = $0 }
            controller.onPageReady = {
                refreshHighlights(); position.pageReady(paperID, model: model, controller: controller)
            }
            controller.onAnnotation = { id in editingAnnotation = model.annotations(paperID).first { $0.id == id } }
            controller.onMenu = { action in
                Task {
                    guard let sel = await controller.pageSelection() else { return }
                    switch action {
                    case .highlight(let c): model.addHighlight(paperID, sel, color: c)
                    case .note: editingAnnotation = model.addHighlight(paperID, sel)
                    }
                    controller.clearSelection()
                    refreshHighlights()
                }
            }
            if PhoneVoice.shared.downloaded { Task { _ = await PhoneVoice.shared.prepare() } }
            ask.model = model
            ask.passages = { [weak controller] in await controller?.narrationItems(includeCaptions: true) ?? [] }
            ask.selection = { [weak controller] in await controller?.selectedText() }
            await load()
        }
        .onDisappear { if listening { stopListening() }; position.stop(paperID, model: model, controller: controller) }
    }

    private func refreshHighlights() { controller.showAnnotations(model.annotations(paperID)) }

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
        if !p.read, book != nil || pdfURL != nil { model.setRead(p, true) }
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
    var paged = false
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
        cfg.addAnnotations(controller)
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
        return PagedState(on: paged, top: inset.top + 14, bottom: max(inset.bottom, 12) + 22, native: true)
    }

    private func configureScroll(_ sv: UIScrollView) {
        guard sv.isPagingEnabled != paged else { return }
        sv.isPagingEnabled = paged
        sv.contentInsetAdjustmentBehavior = paged ? .never : .automatic
        sv.alwaysBounceVertical = !paged
        sv.showsVerticalScrollIndicator = !paged
        sv.isDirectionalLockEnabled = paged
    }

    func updateUIView(_ v: WKWebView, context: Context) {
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

struct PDFKitViewIOS: UIViewRepresentable {
    let url: URL
    func makeUIView(context: Context) -> PDFView {
        let v = PDFView()
        v.autoScales = true
        v.displayMode = .singlePageContinuous
        v.document = PDFDocument(url: url)
        return v
    }
    func updateUIView(_ v: PDFView, context: Context) {}
}
