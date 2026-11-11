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
                                    fontScale: fontScale, scrollRequest: scrollRequest) { id in current = id }
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
                        Button { fontScale = max(0.7, fontScale - 0.1) } label: { Image(systemName: "textformat.size.smaller") }
                            .help("Smaller text")
                        Button { fontScale = min(1.8, fontScale + 0.1) } label: { Image(systemName: "textformat.size.larger") }
                            .help("Larger text")
                    }
                }
            }
        }
        .navigationTitle(paper?.meta.title ?? "Reader")
        .onAppear(perform: load)
    }

    private func load() {
        guard let p = model.paper(paperID) else { return }
        if !p.hasEpub { mode = p.hasPdf ? .pdf : .reading; error = "Use “Make Reading View” in the library."; return }
        do {
            book = try model.lib.openEpub(id: paperID)
            if !p.read { model.setRead(p, true) }
        } catch {
            self.error = error.localizedDescription
        }
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
    let onPosition: (String) -> Void

    static let readerCSS = """
    html { font-size: 112%; scroll-behavior: smooth; }
    body { max-width: 42em; margin: 2.2em auto !important; padding: 0 2em 6em; font-family: "Iowan Old Style", Georgia, serif;
           line-height: 1.6; color: #1d1d1f; background: #fffdf8; }
    @media (prefers-color-scheme: dark) { body { color: #e6e3dc; background: #1c1b19; } a { color: #9ec9d8; } img { opacity: .92; } }
    h1, h2, h3 { font-family: -apple-system, "Helvetica Neue", sans-serif; scroll-margin-top: 1em; }
    h1 { margin-top: 2.4em; padding-top: 1.2em; border-top: 1px solid rgba(127,127,127,.25); }
    .titlepage h1, .section-start + section > h1:first-child { border-top: none; }
    figure img, .table-wrap img { border-radius: 4px; }
    .section-start { scroll-margin-top: 1em; }
    """

    static let trackingJS = """
    (function(){
      var t = null;
      function report(){
        var cur = null, hs = document.querySelectorAll('h1[id],h2[id],h3[id],.section-start');
        for (var i = 0; i < hs.length; i++) { if (hs[i].getBoundingClientRect().top < 140) cur = hs[i].id; else break; }
        if (cur) window.webkit.messageHandlers.pos.postMessage(cur);
      }
      window.addEventListener('scroll', function(){ clearTimeout(t); t = setTimeout(report, 90); });
    })();
    """

    func makeNSView(context: Context) -> WKWebView {
        let cfg = WKWebViewConfiguration()
        let css = Self.readerCSS.replacingOccurrences(of: "`", with: "")
        let js = "var s=document.createElement('style');s.textContent=`\(css)`;document.documentElement.appendChild(s);"
        cfg.userContentController.addUserScript(WKUserScript(source: js, injectionTime: .atDocumentEnd, forMainFrameOnly: true))
        cfg.userContentController.addUserScript(WKUserScript(source: Self.trackingJS, injectionTime: .atDocumentEnd, forMainFrameOnly: true))
        cfg.userContentController.add(context.coordinator, name: "pos")
        let v = WKWebView(frame: .zero, configuration: cfg)
        v.setValue(false, forKey: "drawsBackground")
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
            let safe = a.replacingOccurrences(of: "'", with: "\\'")
            v.evaluateJavaScript("var e=document.getElementById('\(safe)'); if(e) e.scrollIntoView({behavior:'smooth', block:'start'});")
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
