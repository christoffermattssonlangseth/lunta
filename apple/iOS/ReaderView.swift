import SwiftUI
import WebKit
import PDFKit
import MarginsCore

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
    @State private var fontScale = 1.0
    @State private var current: String?
    @State private var scrollRequest = ScrollRequest()
    @StateObject private var narrator = Narrator()
    @StateObject private var controller = ReaderController()
    @State private var listening = false

    enum Mode: String, CaseIterable { case reading = "Reading", pdf = "PDF" }

    var body: some View {
        let paper = model.paper(paperID)
        Group {
            if loading {
                ProgressView(message ?? "Opening…")
            } else if mode == .reading, let book {
                EpubWebViewIOS(url: URL(fileURLWithPath: book.combined), readAccess: URL(fileURLWithPath: book.dir),
                               fontScale: fontScale, scrollRequest: scrollRequest, controller: controller) { current = $0 }
                    .ignoresSafeArea(edges: listening ? [] : .bottom)
                    .safeAreaInset(edge: .bottom) {
                        if listening { NarrationBar(narrator: narrator) { stopListening() } }
                    }
            } else if mode == .pdf, let pdfURL {
                PDFKitViewIOS(url: pdfURL).ignoresSafeArea(edges: .bottom)
            } else {
                PaperInfoView(paperID: paperID, embedded: true)
            }
        }
        .navigationBarTitleDisplayMode(.inline)
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
                    Button { showContents = true } label: { Image(systemName: "list.bullet") }
                    Menu {
                        Button { fontScale = min(1.8, fontScale + 0.1) } label: { Label("Larger Text", systemImage: "textformat.size.larger") }
                        Button { fontScale = max(0.7, fontScale - 0.1) } label: { Label("Smaller Text", systemImage: "textformat.size.smaller") }
                    } label: { Image(systemName: "textformat.size") }
                }
                Button { showInfo = true } label: { Image(systemName: "info.circle") }
            }
        }
        .sheet(isPresented: $showContents) {
            if let book { ContentsSheet(book: book, current: current) { a in current = a; scrollRequest = ScrollRequest(anchor: a); showContents = false } }
        }
        .sheet(isPresented: $showInfo) { NavigationStack { PaperInfoView(paperID: paperID, embedded: false) } }
        .onChange(of: mode) { _, m in if m == .pdf, pdfURL == nil { Task { await loadPDF() } } }
        .task { await load() }
        .onDisappear { if listening { stopListening() } }
    }

    private func startListening() {
        Task {
            let items = await controller.narrationItems()
            guard !items.isEmpty else { return }
            // natural (neural) narration when it has been rendered, otherwise the system voice
            if !(await model.loadNeuralNarration(paperID, into: narrator, pageItems: items)) {
                narrator.load(items, title: model.paper(paperID)?.meta.title ?? "")
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
                        .foregroundStyle(a == current ? Color.marginsTeal : (t.level <= 1 ? .primary : .secondary))
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
    let scrollRequest: ScrollRequest
    let controller: ReaderController
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
        let v = WKWebView(frame: .zero, configuration: cfg)
        controller.webView = v
        v.isOpaque = false
        v.backgroundColor = .clear
        v.loadFileURL(url, allowingReadAccessTo: readAccess)
        context.coordinator.loaded = url
        return v
    }

    func updateUIView(_ v: WKWebView, context: Context) {
        context.coordinator.onPosition = onPosition
        v.pageZoom = fontScale
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
        init(onPosition: @escaping (String) -> Void) { self.onPosition = onPosition }
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
