import Foundation
import WebKit

/// Lets SwiftUI views run scripts in the reading view's WKWebView (narration, highlighting).
@MainActor
final class ReaderController: ObservableObject {
    weak var webView: WKWebView?
    /// Called when the user taps a paragraph while listening.
    var onTapParagraph: ((String) -> Void)?

    func narrationItems(includeCaptions: Bool = false) async -> [NarrationItem] {
        guard let v = webView,
              let json = try? await v.evaluateJavaScript("marginsNarration(\(includeCaptions))") as? String,
              let data = json.data(using: .utf8) else { return [] }
        return (try? JSONDecoder().decode([NarrationItem].self, from: data)) ?? []
    }

    func firstVisibleID() async -> String? {
        (try? await webView?.evaluateJavaScript("marginsFirstVisible()")) as? String
    }

    func highlight(_ id: String) {
        let safe = id.replacingOccurrences(of: "'", with: "")
        webView?.evaluateJavaScript("marginsHighlight('\(safe)')")
    }

    func setListening(_ on: Bool) {
        webView?.evaluateJavaScript("marginsListening(\(on))")
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
