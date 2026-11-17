import SwiftUI
import LuntaCore

/// The paper's citation key for LaTeX, editable, with a button that copies `\cite{key}`. The key is
/// fixed when the paper is added and the same on every device (core: `Library::set_cite_key`).
struct CiteKeyRow: View {
    @EnvironmentObject var model: LibraryModel
    let paper: Paper
    @State private var text = ""
    @State private var error: String?
    @State private var copied = false
    @FocusState private var focused: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(spacing: 8) {
                Text("Cite key").foregroundStyle(.secondary)
                TextField("Key", text: $text)
                    .font(.callout.monospaced())
                    .focused($focused)
                    .autocorrectionDisabled()
                    #if os(iOS)
                    .textInputAutocapitalization(.never)
                    .submitLabel(.done)
                    #else
                    .textFieldStyle(.plain)
                    #endif
                    .onSubmit(save)
                    .onChange(of: focused) { _, now in if !now { save() } }
                Button {
                    model.copyLatexCite([paper.id])
                    copied = true
                } label: { Image(systemName: copied ? "checkmark" : "doc.on.doc") }
                    .buttonStyle(.borderless)
                    .help("Copy \\cite{\(paper.citeKey)}")
                    .accessibilityLabel("Copy \\cite")
            }
            if let error { Text(error).font(.caption).foregroundStyle(.red) }
        }
        .font(.callout)
        .onAppear { text = paper.citeKey }
        .onChange(of: paper.id) { _, _ in text = paper.citeKey; error = nil; copied = false }
        .onChange(of: paper.citeKey) { _, k in if !focused { text = k } }
    }

    private func save() {
        let k = text.trimmingCharacters(in: .whitespaces)
        guard k != paper.citeKey else { error = nil; text = k; return }
        if let e = model.setCiteKey(paper.id, k) {
            error = e
        } else {
            error = nil
        }
    }
}

extension LibraryModel {
    /// `\cite{a,b}` for the papers, on the clipboard.
    func copyLatexCite(_ ids: [String]) {
        let s = lib.latexCite(ids: ids)
        guard !s.isEmpty else { return }
        #if os(macOS)
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(s, forType: .string)
        #else
        UIPasteboard.general.string = s
        #endif
    }

    /// Changes a paper's citation key; returns why it can't be that key, or nil.
    func setCiteKey(_ id: String, _ key: String) -> String? {
        do {
            _ = try lib.setCiteKey(id: id, key: key)
            reload()
            return nil
        } catch {
            return error.readable
        }
    }

    /// "Copy \cite{Kukanja2024cellular}" for one paper, "Copy \cite{…}" for several.
    func latexCiteMenuTitle(_ ids: [String]) -> String {
        if ids.count == 1, let k = paper(ids[0])?.citeKey, !k.isEmpty { return "Copy \\cite{\(k)}" }
        return "Copy \\cite{…}"
    }
}
