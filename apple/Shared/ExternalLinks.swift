import WebKit
import LuntaCore
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
    /// The citation card's lunta://cite?doi=… (or ?arxiv=…, or ?ref=<the reference's text> when it
    /// has neither: its DOI is looked up by Crossref's bibliographic search): the paper's id if it
    /// is already in the library, else it is added (with its open-access full text when there is
    /// one). Returns the id and what to show on the card's button.
    func paperForCitation(_ url: URL) async -> (id: String?, label: String) {
        let items = URLComponents(url: url, resolvingAgainstBaseURL: false)?.queryItems ?? []
        var doi = items.first { $0.name == "doi" }?.value ?? ""
        var arxiv = items.first { $0.name == "arxiv" }?.value ?? ""
        let ref = items.first { $0.name == "ref" }?.value ?? ""
        if doi.isEmpty, arxiv.isEmpty, !ref.isEmpty {
            guard let ids = await Self.lookUpReference(ref) else { return (nil, "Couldn’t find its DOI") }
            (doi, arxiv) = (ids.doi, ids.arxiv)
        }
        return await paperFor(doi: doi, arxiv: arxiv)
    }

    /// The paper with this DOI or arXiv number: found in the library, else added.
    func paperFor(doi: String, arxiv: String) async -> (id: String?, label: String) {
        guard !doi.isEmpty || !arxiv.isEmpty else { return (nil, "Nothing to add") }
        if let id = paperInLibrary(doi: doi, arxiv: arxiv) { return (id, "In your library") }
        guard let id = await add(doi.isEmpty ? "arXiv:\(arxiv)" : doi) else { return (nil, "Couldn’t add it") }
        return (id, "Added to your library")
    }

    /// The paper with this DOI or arXiv number, if it is in the library (no network).
    func paperInLibrary(doi: String, arxiv: String) -> String? {
        let key = doi.isEmpty ? "10.48550/arXiv.\(arxiv)" : doi
        if !key.isEmpty, let p = lib.findByDoi(doi: key) { return p.id }
        if !arxiv.isEmpty, let p = papers.first(where: { $0.meta.arxiv.hasPrefix(arxiv) }) { return p.id }
        return nil
    }

    /// A reference's DOI or arXiv number: printed in it, else Crossref's match (nil: not found).
    nonisolated static func lookUpReference(_ text: String) async -> ReferenceIds? {
        await Task.detached { (try? resolveReference(text: text)) ?? nil }.value
    }
}

extension WKWebView {
    /// Tells the citation card how "Add to Lunta" went.
    func citationDone(_ label: String) {
        let safe = label.replacingOccurrences(of: "\\", with: "").replacingOccurrences(of: "'", with: "’")
        evaluateJavaScript("window.luntaCiteDone && luntaCiteDone('\(safe)')")
    }
}
