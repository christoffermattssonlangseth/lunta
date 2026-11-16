import WebKit
#if os(iOS)
import UIKit
#else
import AppKit
#endif

/// The reading view only ever shows the paper: a tapped web link (a DOI in the references, a data
/// repository) opens in the browser instead of replacing the paper inside the reader. Links within
/// the paper (figures, references, sections) and the paper's own files load as before.
final class ExternalLinks: NSObject, WKNavigationDelegate {
    static let shared = ExternalLinks()

    func webView(_ webView: WKWebView, decidePolicyFor action: WKNavigationAction,
                 decisionHandler: @escaping @MainActor @Sendable (WKNavigationActionPolicy) -> Void) {
        guard let url = action.request.url, let scheme = url.scheme?.lowercased() else { decisionHandler(.allow); return }
        if scheme == "file" || scheme == "about" || scheme == "data" || scheme == "blob" {
            decisionHandler(.allow)
            return
        }
        // anything else (http, https, mailto, …): only ever by the user's tap, and then outside the app
        decisionHandler(.cancel)
        guard action.navigationType == .linkActivated else { return }
        #if os(iOS)
        UIApplication.shared.open(url)
        #else
        NSWorkspace.shared.open(url)
        #endif
    }
}
