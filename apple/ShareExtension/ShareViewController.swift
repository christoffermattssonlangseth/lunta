import UIKit
import SwiftUI
import UniformTypeIdentifiers

/// "Lunta" in the share sheet: takes PDFs, links (DOI, PubMed, journal pages) or a DOI in text.
final class ShareViewController: UIViewController {
    private let state = ShareState()

    override func viewDidLoad() {
        super.viewDidLoad()
        let host = UIHostingController(rootView: ShareCard(state: state) { [weak self] in self?.finish() })
        addChild(host)
        host.view.frame = view.bounds
        host.view.autoresizingMask = [.flexibleWidth, .flexibleHeight]
        host.view.backgroundColor = .clear
        view.addSubview(host.view)
        host.didMove(toParent: self)
        Task { await collect() }
    }

    private func finish() { extensionContext?.completeRequest(returningItems: nil) }

    private func collect() async {
        let providers = (extensionContext?.inputItems as? [NSExtensionItem] ?? []).flatMap { $0.attachments ?? [] }
        var pdfs = 0, links = 0
        for p in providers {
            if p.hasItemConformingToTypeIdentifier(UTType.pdf.identifier) {
                if let url = await loadFile(p) {
                    if (try? ShareInbox.put(pdf: url)) != nil { pdfs += 1 }
                    continue
                }
                if let data = try? await p.loadItem(forTypeIdentifier: UTType.pdf.identifier) as? Data,
                   (try? ShareInbox.put(pdfData: data, name: "Shared PDF")) != nil { pdfs += 1; continue }
            }
            if p.hasItemConformingToTypeIdentifier(UTType.url.identifier),
               let u = try? await p.loadItem(forTypeIdentifier: UTType.url.identifier) as? URL {
                if u.isFileURL, u.pathExtension.lowercased() == "pdf" {
                    if (try? ShareInbox.put(pdf: u)) != nil { pdfs += 1 }
                } else if (try? ShareInbox.put(link: u.absoluteString)) != nil { links += 1 }
                continue
            }
            if p.hasItemConformingToTypeIdentifier(UTType.plainText.identifier),
               let s = try? await p.loadItem(forTypeIdentifier: UTType.plainText.identifier) as? String,
               let ref = Self.reference(in: s), (try? ShareInbox.put(link: ref)) != nil {
                links += 1
            }
        }
        await MainActor.run { state.done(pdfs: pdfs, links: links) }
        try? await Task.sleep(for: .seconds(pdfs + links > 0 ? 1.6 : 3))
        await MainActor.run { finish() }
    }

    private func loadFile(_ p: NSItemProvider) async -> URL? {
        await withCheckedContinuation { c in
            _ = p.loadFileRepresentation(forTypeIdentifier: UTType.pdf.identifier) { url, _ in
                guard let url else { c.resume(returning: nil); return }
                // the file is deleted when this returns: copy it somewhere we own
                let tmp = FileManager.default.temporaryDirectory.appendingPathComponent(url.lastPathComponent)
                try? FileManager.default.removeItem(at: tmp)
                c.resume(returning: (try? FileManager.default.copyItem(at: url, to: tmp)) != nil ? tmp : nil)
            }
        }
    }

    /// A DOI, PMCID, arXiv id or link in shared text.
    static func reference(in s: String) -> String? {
        let t = s.trimmingCharacters(in: .whitespacesAndNewlines)
        for pattern in [#"https?://\S+"#, #"10\.\d{4,9}/\S+"#, #"PMC\d+"#, #"arXiv:\s?\d{4}\.\d{4,5}"#] {
            if let r = t.range(of: pattern, options: [.regularExpression, .caseInsensitive]) { return String(t[r]) }
        }
        return nil
    }
}

@MainActor
final class ShareState: ObservableObject {
    @Published var message = "Adding to Lunta…"
    @Published var finished = false
    @Published var ok = true

    func done(pdfs: Int, links: Int) {
        finished = true
        if pdfs + links == 0 {
            ok = false
            message = "Nothing Lunta can add here. Share a PDF, a paper's web page or a DOI."
        } else {
            var parts: [String] = []
            if pdfs > 0 { parts.append(pdfs == 1 ? "PDF" : "\(pdfs) PDFs") }
            if links > 0 { parts.append(links == 1 ? "link" : "\(links) links") }
            message = "Saved \(parts.joined(separator: " and ")). Lunta adds it to your library when you open the app."
        }
    }
}

struct ShareCard: View {
    @ObservedObject var state: ShareState
    let close: () -> Void

    var body: some View {
        VStack {
            Spacer()
            VStack(spacing: 14) {
                if state.finished {
                    Image(systemName: state.ok ? "checkmark.circle.fill" : "exclamationmark.circle")
                        .font(.system(size: 40))
                        .foregroundStyle(state.ok ? Color(red: 0.16, green: 0.48, blue: 0.55) : .secondary)
                } else {
                    ProgressView()
                }
                Text(state.message).font(.callout).multilineTextAlignment(.center)
                Button("Done", action: close).buttonStyle(.bordered)
            }
            .padding(24)
            .frame(maxWidth: 360)
            .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 22))
            .padding()
            Spacer()
        }
    }
}
