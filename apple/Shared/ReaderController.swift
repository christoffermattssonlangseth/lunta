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
    /// Called when the user taps a paragraph while listening.
    var onTapParagraph: ((String) -> Void)?
    /// Called when the user taps a highlight (its annotation id).
    var onAnnotation: ((String) -> Void)?
    /// Called when the page has loaded (to draw highlights).
    var onPageReady: (() -> Void)?
    /// Highlight mode: the user finished a selection.
    var onSelectionDone: ((PageSelection?) -> Void)?
    /// Called from the text selection menu.
    var onMenu: ((ReaderMenuAction) -> Void)?

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
