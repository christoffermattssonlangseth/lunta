import SwiftUI
import AppKit
import LuntaCore

/// Journal citation styles on the Mac: the ~10,000 CSL styles, searched and formatted by the core
/// (styles are fetched once and kept with the citation add-ins' files, `CitationServer.cslCache`).
enum JournalStyles {
    struct Recent: Codable, Hashable { let id: String; let title: String }

    /// Journal styles used before, most recent first.
    static var recent: [Recent] {
        (UserDefaults.standard.data(forKey: "journalStyles")).flatMap { try? JSONDecoder().decode([Recent].self, from: $0) } ?? []
    }

    static func remember(_ id: String, _ title: String) {
        let list = [Recent(id: id, title: title)] + recent.filter { $0.id != id }
        UserDefaults.standard.set(try? JSONEncoder().encode(Array(list.prefix(8))), forKey: "journalStyles")
    }

    /// The papers' references in a journal style (blocking the first time a style is used).
    nonisolated static func references(_ lib: Library, ids: [String], style: String) throws -> String {
        let out = try lib.citeCsl(cacheDir: CitationServer.cslCache, styleId: style, citations: [ids])
        var entries = out.bibliography.map(\.text)
        // one paper: no "1." in front of it
        if entries.count == 1, let r = entries[0].range(of: #"^\[?\d+[\].]?\s+"#, options: .regularExpression) {
            entries[0].removeSubrange(r)
        }
        return entries.joined(separator: "\n\n")
    }

    /// Copies the references in a journal style, then remembers the style.
    @MainActor static func copy(_ model: LibraryModel, ids: [String], style: Recent) {
        let lib = model.lib
        Task.detached {
            let result = Result { try references(lib, ids: ids, style: style.id) }
            await MainActor.run {
                switch result {
                case .success(let text):
                    copyCitation(text)
                    remember(style.id, style.title)
                case .failure(let e):
                    model.errorMessage = "Couldn't format in \(style.title): \(e.readable)"
                }
            }
        }
    }
}

/// Copies a citation or reference list with its *italics* and **bold**: as formatted text (for
/// Word, Pages, Mail) and as plain text without the markup.
func copyCitation(_ markdown: String) {
    let esc = markdown.replacingOccurrences(of: "&", with: "&amp;").replacingOccurrences(of: "<", with: "&lt;").replacingOccurrences(of: ">", with: "&gt;")
    var html = esc.replacingOccurrences(of: #"\*\*(.+?)\*\*"#, with: "<b>$1</b>", options: .regularExpression)
    html = html.replacingOccurrences(of: #"\*(.+?)\*"#, with: "<i>$1</i>", options: .regularExpression)
    html = html.components(separatedBy: "\n\n").map { "<p>\($0)</p>" }.joined()
    let plain = markdown.replacingOccurrences(of: #"\*\*(.+?)\*\*"#, with: "$1", options: .regularExpression)
        .replacingOccurrences(of: #"\*(.+?)\*"#, with: "$1", options: .regularExpression)
    let pb = NSPasteboard.general
    pb.clearContents()
    if let data = "<meta charset=\"utf-8\"><div style=\"font-family: -apple-system, Helvetica\">\(html)</div>".data(using: .utf8),
       let rich = NSAttributedString(html: data, documentAttributes: nil),
       let rtf = rich.rtf(from: NSRange(location: 0, length: rich.length)) {
        pb.setData(rtf, forType: .rtf)
    }
    pb.setString(html, forType: .html)
    pb.setString(plain, forType: .string)
}

/// Find a journal's citation style and copy the papers' references in it.
struct JournalStylePicker: View {
    @EnvironmentObject var model: LibraryModel
    @Environment(\.dismiss) private var dismiss
    let ids: [String]
    @State private var query = ""
    @State private var hits: [CslStyleHit] = []
    @State private var searching = false
    @State private var error: String?
    @State private var copying: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Copy in a Journal’s Style").font(.headline)
            Text("Search about 10,000 journal styles (Citation Style Language). The style is downloaded the first time.")
                .font(.callout).foregroundStyle(.secondary)
            TextField("Journal name, e.g. Neuron, eLife, PLOS ONE", text: $query)
                .textFieldStyle(.roundedBorder)
            List(hits, id: \.id) { h in
                Button { pick(h) } label: {
                    HStack {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(h.title)
                            Text(h.format == "numeric" ? "numbered" : h.format).font(.caption).foregroundStyle(.secondary)
                        }
                        Spacer()
                        if copying == h.id { ProgressView().controlSize(.small) }
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
            }
            .overlay {
                if searching { ProgressView() }
                else if hits.isEmpty && !query.trimmingCharacters(in: .whitespaces).isEmpty {
                    Text(error ?? "No journal styles match.").foregroundStyle(.secondary)
                }
            }
            HStack {
                Text("\(ids.count) paper\(ids.count == 1 ? "" : "s")").font(.caption).foregroundStyle(.secondary)
                Spacer()
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
            }
        }
        .padding(20)
        .frame(width: 460, height: 460)
        .task(id: query) {
            let q = query.trimmingCharacters(in: .whitespaces)
            guard !q.isEmpty else { hits = []; return }
            try? await Task.sleep(for: .milliseconds(250))
            guard !Task.isCancelled else { return }
            searching = true
            let result = await Task.detached { Result { try cslSearch(cacheDir: CitationServer.cslCache, query: q, limit: 50) } }.value
            searching = false
            switch result {
            case .success(let h): hits = h; error = nil
            case .failure(let e): hits = []; error = "Couldn't load the journal styles: \(e.readable)"
            }
        }
    }

    private func pick(_ h: CslStyleHit) {
        copying = h.id
        let lib = model.lib, ids = ids
        Task.detached {
            let result = Result { try JournalStyles.references(lib, ids: ids, style: h.id) }
            await MainActor.run {
                copying = nil
                switch result {
                case .success(let text):
                    copyCitation(text)
                    JournalStyles.remember(h.id, h.title)
                    dismiss()
                case .failure(let e):
                    error = "Couldn't format in \(h.title): \(e.readable)"
                    hits = []
                }
            }
        }
    }
}
