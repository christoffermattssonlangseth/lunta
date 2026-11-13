import SwiftUI
import LuntaCore

/// A PDF waiting for "Replace PDF?" before it goes onto a paper that already has one.
struct PendingPDF: Identifiable {
    let id = UUID()
    let url: URL
    let paperID: String
    let title: String
}

extension LibraryModel {
    /// Attaches a PDF to this paper (replacing any it had) and makes the reading view if it has none.
    @discardableResult
    func attachPDF(_ url: URL, to id: String) async -> Bool {
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        guard let data = try? Data(contentsOf: url), data.starts(with: Array("%PDF".utf8)) else {
            errorMessage = "“\(url.lastPathComponent)” isn't a readable PDF."
            return false
        }
        status = "Adding the PDF…"
        defer { status = nil }
        let lib = self.lib
        do {
            try await Task.detached { try lib.attachFile(id: id, kind: .pdf, data: data) }.value
        } catch {
            errorMessage = error.readable
            return false
        }
        reload()
        if lib.paper(id: id)?.hasEpub == false {
            status = "Making the reading view…"
            await makeReadingView(id)
            reload()
        }
        return true
    }

    /// Attaches right away, or asks first when the paper already has a PDF.
    func offerPDF(_ url: URL, to p: Paper, pending: Binding<PendingPDF?>) {
        if p.hasPdf {
            pending.wrappedValue = PendingPDF(url: url, paperID: p.id, title: p.meta.title)
        } else {
            Task { await attachPDF(url, to: p.id) }
        }
    }
}

/// The "Replace PDF?" question for `offerPDF`.
struct ReplacePDFConfirm: ViewModifier {
    @EnvironmentObject var model: LibraryModel
    @Binding var pending: PendingPDF?

    func body(content: Content) -> some View {
        content.confirmationDialog("Replace the PDF of “\(pending?.title ?? "")”?",
                                   isPresented: Binding(get: { pending != nil }, set: { if !$0 { pending = nil } }),
                                   titleVisibility: .visible) {
            Button("Replace PDF") {
                if let p = pending { pending = nil; Task { await model.attachPDF(p.url, to: p.paperID) } }
            }
            Button("Cancel", role: .cancel) { pending = nil }
        } message: {
            Text("The paper keeps its details, reading view, highlights and notes; only the PDF changes.")
        }
    }
}
