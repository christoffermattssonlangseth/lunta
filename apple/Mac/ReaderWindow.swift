import SwiftUI
import WebKit
import PDFKit
import MarginsCore

/// One paper, two views: the reflowable reading view (converted EPUB) and the original PDF.
struct ReaderWindow: View {
    @EnvironmentObject var model: LibraryModel
    let paperID: String
    @State private var mode: Mode = .reading
    @State private var book: EpubBook?
    @State private var current: String?          // anchor of the section being read
    @State private var scrollRequest = ScrollRequest()
    @State private var fontScale = 1.0
    @State private var error: String?
    @StateObject private var narrator = Narrator()
    @StateObject private var controller = ReaderController()
    @State private var listening = false

    enum Mode: String, CaseIterable { case reading = "Reading", pdf = "PDF" }

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
            } else {
                Text(paper?.meta.title ?? "").padding().navigationSplitViewColumnWidth(min: 180, ideal: 250)
            }
        } detail: {
            Group {
                switch mode {
                case .reading:
                    if let book {
                        EpubWebView(url: URL(fileURLWithPath: book.combined), readAccess: URL(fileURLWithPath: book.dir),
                                    fontScale: fontScale, scrollRequest: scrollRequest, controller: controller) { id in current = id }
                            .safeAreaInset(edge: .bottom) {
                                if listening { NarrationBar(narrator: narrator) { stopListening() } }
                            }
                    } else if let error {
                        ContentUnavailableView("No reading view", systemImage: "book", description: Text(error))
                    } else {
                        ProgressView()
                    }
                case .pdf:
                    if let url = model.fileURL(paperID, .pdf) {
                        PDFKitView(url: url)
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
                        Button { fontScale = max(0.7, fontScale - 0.1) } label: { Image(systemName: "textformat.size.smaller") }
                            .help("Smaller text")
                        Button { fontScale = min(1.8, fontScale + 0.1) } label: { Image(systemName: "textformat.size.larger") }
                            .help("Larger text")
                    }
                }
            }
        }
        .navigationTitle(paper?.meta.title ?? "Reader")
        .task { await load() }
    }

    private func load() async {
        guard let p = model.paper(paperID) else { return }
        if !p.hasEpub { mode = p.hasPdf ? .pdf : .reading; error = "Use “Make Reading View” in the library."; return }
        guard await model.ensureLocal(paperID, .epub) != nil else { error = "Couldn't download the reading view from iCloud."; return }
        do {
            book = try model.lib.openEpub(id: paperID)
            if !p.read { model.setRead(p, true) }
        } catch {
            self.error = error.localizedDescription
        }
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
    let scrollRequest: ScrollRequest
    let controller: ReaderController
    let onPosition: (String) -> Void

    func makeNSView(context: Context) -> WKWebView {
        let cfg = WKWebViewConfiguration()
        cfg.userContentController.addUserScript(WKUserScript(source: ReaderAssets.styleScript, injectionTime: .atDocumentEnd, forMainFrameOnly: true))
        cfg.userContentController.addUserScript(WKUserScript(source: ReaderAssets.trackingJS, injectionTime: .atDocumentEnd, forMainFrameOnly: true))
        cfg.userContentController.addUserScript(WKUserScript(source: ReaderAssets.narrationJS, injectionTime: .atDocumentEnd, forMainFrameOnly: true))
        cfg.userContentController.add(context.coordinator, name: "pos")
        let narr = NarrationMessageHandler()
        narr.controller = controller
        cfg.userContentController.add(narr, name: "narr")
        let v = WKWebView(frame: .zero, configuration: cfg)
        v.setValue(false, forKey: "drawsBackground")
        controller.webView = v
        v.loadFileURL(url, allowingReadAccessTo: readAccess)
        context.coordinator.loaded = url
        return v
    }

    func updateNSView(_ v: WKWebView, context: Context) {
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

struct PDFKitView: NSViewRepresentable {
    let url: URL
    func makeNSView(context: Context) -> PDFView {
        let v = PDFView()
        v.autoScales = true
        v.displayMode = .singlePageContinuous
        v.document = PDFDocument(url: url)
        return v
    }
    func updateNSView(_ v: PDFView, context: Context) {
        if v.document?.documentURL != url { v.document = PDFDocument(url: url) }
    }
}
