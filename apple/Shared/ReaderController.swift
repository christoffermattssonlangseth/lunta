import Foundation
import WebKit

/// Lets SwiftUI views run scripts in the reading view's WKWebView (narration, highlighting).
@MainActor
final class ReaderController: ObservableObject {
    weak var webView: WKWebView?
    /// Called when the user taps a figure.
    var onFigure: ((ReaderFigure) -> Void)?
    /// The paper's figures (label, title, thumbnail), sent by the page when it loads.
    @Published var figures: [ReaderFigure] = []
    /// Highlights the reading view couldn't find in its text (e.g. after it was rebuilt from
    /// another source): shown as such in the highlights list instead of silently disappearing.
    @Published var missingAnnotations: Set<String> = []
    /// Called when the user taps a paragraph while listening.
    var onTapParagraph: ((String) -> Void)?
    /// Called when the user taps a highlight (its annotation id).
    var onAnnotation: ((String) -> Void)?
    /// Called when the page has loaded (to draw highlights).
    var onPageReady: (() -> Void)?
    /// The Summary card's menu: "regenerate" or "hide" (Summary.swift).
    var onSummary: ((String) -> Void)?
    /// Highlight mode: the user finished a selection.
    var onSelectionDone: ((PageSelection?) -> Void)?
    /// Extend… / Highlight to… : the passage up to the word that was tapped (`startExtend`).
    var onExtendDone: ((PageSelection) -> Void)?
    /// The page started a Highlight to… itself (a selection dragged over the page's edge, in pages).
    var onExtendBegan: ((HighlightColor) -> Void)?
    /// "Highlight to…" in the text selection menu (iPhone, iPad).
    var onHighlightTo: (() -> Void)?
    /// Called from the text selection menu (and by the Pencil, with `pencilSelection` set).
    var onMenu: ((ReaderMenuAction) -> Void)?
    /// A passage marked with the Apple Pencil, used once by `pageSelection()` instead of the
    /// selection (so a Pencil highlight or note is saved the way the menu's are).
    var pencilSelection: (PageSelection, Date)?
    /// A book read chapter by chapter (ChapterReader.swift): the part shown, and the way to the others.
    weak var chapters: ChapterReader?
    /// The page's web process ended twice in a short time (a page too big for the device's memory):
    /// the reader says so instead of showing an empty page (ReaderNavigation).
    @Published var pageGone = false
    /// The part shown of a book read chapter by chapter (the readers load its page when it changes).
    @Published var part = 0
    /// Pages of a book read chapter by chapter loaded so far (the Mac's References list is read again for each).
    @Published var partLoads = 0

    /// Try Again after `pageGone`: the page loads again.
    func retryPage() {
        pageGone = false
        webView?.reload()
    }

    func narrationItems(includeCaptions: Bool = false) async -> [NarrationItem] {
        guard let v = webView,
              let json = try? await v.evaluateJavaScript("luntaNarration(\(includeCaptions))") as? String,
              let data = json.data(using: .utf8) else { return [] }
        return (try? JSONDecoder().decode([NarrationItem].self, from: data)) ?? []
    }

    func firstVisibleID() async -> String? {
        (try? await webView?.evaluateJavaScript("luntaFirstVisible()")) as? String
    }

    func highlight(_ id: String) {
        let safe = id.replacingOccurrences(of: "'", with: "")
        webView?.evaluateJavaScript("luntaHighlight('\(safe)')")
    }

    func selectedText() async -> String? {
        (try? await webView?.evaluateJavaScript("window.getSelection().toString()")) as? String
    }

    /// Scrolls to a cited passage and flashes it.
    func flash(_ id: String) {
        highlight(id)
        let safe = id.replacingOccurrences(of: "'", with: "")
        webView?.evaluateJavaScript("setTimeout(function(){ var e=document.getElementById('\(safe)'); if(e && !document.body.classList.contains('listening')) e.classList.remove('narr-current'); }, 2600)")
    }

    func setListening(_ on: Bool) {
        webView?.evaluateJavaScript("luntaListening(\(on))")
    }
}

/// Receives "narr" messages (tapped paragraph ids) from the page.
final class NarrationMessageHandler: NSObject, WKScriptMessageHandler {
    weak var controller: ReaderController?
    func userContentController(_ c: WKUserContentController, didReceive m: WKScriptMessage) {
        guard let id = m.body as? String else { return }
        Task { @MainActor in self.controller?.onTapParagraph?(id) }
    }
}
