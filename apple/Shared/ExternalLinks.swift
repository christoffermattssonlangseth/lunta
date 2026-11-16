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

    /// Handles lunta:// links from the page (the citation card's "Add to Lunta"); set by the app.
    /// Report back to the card with `webView.evaluateJavaScript("luntaCiteDone('…')")`.
    @MainActor static var onLunta: ((URL, WKWebView) -> Void)?

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
        // lunta://cite?doi=… from the citation card: the app adds or opens that paper
        if scheme == "lunta" || scheme == "margins" {
            MainActor.assumeIsolated { Self.onLunta?(url, webView) }
            return
        }
        #if os(iOS)
        UIApplication.shared.open(url)
        #else
        NSWorkspace.shared.open(url)
        #endif
    }
}

extension LibraryModel {
    /// The citation card's lunta://cite?doi=…: the paper's id if it is already in the library, else
    /// it is added (with its open-access full text when there is one). Returns the id and what to
    /// show on the card's button.
    func paperForCitation(_ url: URL) async -> (id: String?, label: String) {
        let items = URLComponents(url: url, resolvingAgainstBaseURL: false)?.queryItems ?? []
        let doi = items.first { $0.name == "doi" }?.value ?? ""
        let arxiv = items.first { $0.name == "arxiv" }?.value ?? ""
        guard !doi.isEmpty || !arxiv.isEmpty else { return (nil, "Nothing to add") }
        let key = doi.isEmpty ? "10.48550/arXiv.\(arxiv)" : doi
        if let p = lib.findByDoi(doi: key) { return (p.id, "In your library") }
        if !arxiv.isEmpty, let p = papers.first(where: { $0.meta.arxiv.hasPrefix(arxiv) }) { return (p.id, "In your library") }
        guard let id = await add(doi.isEmpty ? "arXiv:\(arxiv)" : doi) else { return (nil, "Couldn’t add it") }
        return (id, "Added to your library")
    }
}

extension WKWebView {
    /// Tells the citation card how "Add to Lunta" went.
    func citationDone(_ label: String) {
        let safe = label.replacingOccurrences(of: "\\", with: "").replacingOccurrences(of: "'", with: "’")
        evaluateJavaScript("window.luntaCiteDone && luntaCiteDone('\(safe)')")
    }
}
