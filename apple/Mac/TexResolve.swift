import AppKit
import SwiftUI
import UniformTypeIdentifiers
import LuntaCore

/// "Resolve Citations in a .tex File…": write first, cite later in LaTeX. The writer types
/// `\cite{?langseth2025}` (or `\cite{langseth2025?}`, "(Langseth et al. 2025)", "[Langseth 2025
/// spatial]") while writing; Lunta finds them (core/src/placeholders), proposes papers for each
/// (the library first, then Crossref / OpenAlex) and the writer confirms or skips each one. Then
/// the confirmed papers are put in the chosen folder (outside papers added to the library first),
/// whose .bib Lunta keeps updated, and the .tex is written back with the papers' citation keys,
/// the original kept next to it as .tex.bak. Nothing is written before the writer clicks Write.
@MainActor
func resolveTexCitations(model: LibraryModel?, folder: String? = nil) {
    guard let model else { return }
    let p = NSOpenPanel()
    p.title = "Choose the .tex file"
    p.message = "Lunta finds the placeholders in it, like \\cite{?langseth2025} or (Smith et al. 2019), and proposes papers; you confirm each one."
    p.allowedContentTypes = [UTType(filenameExtension: "tex")].compactMap { $0 }
    guard p.runModal() == .OK, let file = p.url else { return }
    guard let text = try? String(contentsOf: file, encoding: .utf8) else {
        alert("Couldn’t read “\(file.lastPathComponent)”", "Lunta reads .tex files written in UTF-8.")
        return
    }
    let found = findTexPlaceholders(text: text)
    if found.isEmpty {
        alert("No placeholders in “\(file.lastPathComponent)”",
              "Write \\cite{?name2025} (or \\cite{name2025?}, with words after the year as hints: \\cite{?name2025spatial}), (Name et al. 2025) or [Name 2025 hint words] where a citation goes, then resolve them here.")
        return
    }
    TexResolveWindow.show(TexResolveModel(file: file, text: text, placeholders: found, folder: folder, library: model))
}

@MainActor
private func alert(_ title: String, _ text: String) {
    let a = NSAlert()
    a.messageText = title
    a.informativeText = text
    a.runModal()
}

/// One placeholder's part (a cite) and what the writer decided for it.
struct TexCite: Identifiable {
    enum State: Equatable { case loading, open, confirmed, skipped }
    let id: String
    let cite: PlaceholderCite
    var state: State = .loading
    var candidates: [CitationCandidate] = []
    var pick = 0
    var exact = false
    var note = ""
    var query = ""
    var searching = false
}

struct TexPlaceholder: Identifiable {
    let id: Int
    let placeholder: Placeholder
    var cites: [TexCite]
    var decided: Bool { cites.allSatisfy { $0.state == .confirmed || $0.state == .skipped } }
}

@MainActor
final class TexResolveModel: ObservableObject {
    let file: URL
    let text: String
    let library: LibraryModel
    @Published var items: [TexPlaceholder]
    /// The folder the confirmed papers go into ("" = none: the library only).
    @Published var folder: String
    @Published var writing = false
    @Published var report: String?
    @Published var error: String?

    init(file: URL, text: String, placeholders: [Placeholder], folder: String?, library: LibraryModel) {
        self.file = file
        self.text = text
        self.library = library
        items = placeholders.enumerated().map { i, p in
            TexPlaceholder(id: i, placeholder: p, cites: p.cites.enumerated().map { j, c in TexCite(id: "\(i)-\(j)", cite: c) })
        }
        // the folder asked for, else the first whose .bib Lunta keeps
        let kept = BibFiles.shared.links.map(\.source).filter { $0 != BibFiles.allPapers }
        self.folder = folder ?? library.collections.first { kept.contains($0.id) }?.id ?? ""
    }

    /// Asks Lunta for each part's candidates, two at a time.
    func load() {
        let lib = library.lib
        let jobs = items.flatMap { it in it.cites.indices.map { (it.id, $0) } }
        Task {
            await withTaskGroup(of: (Int, Int, CitationSuggestions).self) { group in
                var it = jobs.makeIterator()
                for _ in 0..<2 {
                    if let (i, j) = it.next() {
                        let c = items[i].cites[j].cite
                        group.addTask { (i, j, await Task.detached { lib.suggestCitation(cite: c, external: true) }.value) }
                    }
                }
                for await (i, j, s) in group {
                    fill(i, j, s)
                    if let (i2, j2) = it.next() {
                        let c = items[i2].cites[j2].cite
                        group.addTask { (i2, j2, await Task.detached { lib.suggestCitation(cite: c, external: true) }.value) }
                    }
                }
            }
        }
    }

    private func fill(_ i: Int, _ j: Int, _ s: CitationSuggestions) {
        var c = items[i].cites[j]
        c.candidates = s.candidates
        c.exact = s.exact && s.candidates.first.map { $0.exact && !$0.paperId.isEmpty } == true
        c.note = !s.note.isEmpty ? s.note : (s.candidates.isEmpty ? "No paper found. Search, or skip it." : "")
        c.pick = 0
        c.state = .open
        items[i].cites[j] = c
    }

    func confirm(_ i: Int, _ j: Int) {
        guard items[i].cites[j].state == .open, !items[i].cites[j].candidates.isEmpty else { return }
        items[i].cites[j].state = .confirmed
    }

    func skip(_ i: Int, _ j: Int) { items[i].cites[j].state = .skipped }
    func reopen(_ i: Int, _ j: Int) { items[i].cites[j].state = .open }

    var exactCount: Int { items.reduce(0) { $0 + $1.cites.filter { $0.state == .open && $0.exact }.count } }

    /// Every part whose first candidate is the only library paper that fits; others stay.
    func confirmExact() {
        for i in items.indices {
            for j in items[i].cites.indices where items[i].cites[j].state == .open && items[i].cites[j].exact {
                items[i].cites[j].pick = 0
                items[i].cites[j].state = .confirmed
            }
        }
    }

    func search(_ i: Int, _ j: Int) {
        let q = items[i].cites[j].query.trimmingCharacters(in: .whitespaces)
        guard !q.isEmpty else { return }
        items[i].cites[j].state = .loading
        let lib = library.lib
        Task {
            let s = await Task.detached { lib.searchCitation(query: q, external: true) }.value
            fill(i, j, s)
            items[i].cites[j].exact = false
            items[i].cites[j].searching = false
        }
    }

    var confirmedCount: Int { items.filter { $0.decided && $0.cites.contains { $0.state == .confirmed } }.count }

    /// Adds what has to be added, puts the papers in the folder, writes the .tex (and its .bak).
    func write() {
        guard !writing else { return }
        writing = true
        error = nil
        let lib = library.lib, folder = self.folder, text = self.text, file = self.file
        // only placeholders with every part decided and one confirmed
        let todo = items.filter { $0.decided && $0.cites.contains { $0.state == .confirmed } }
        Task {
            do {
                let (added, papers) = try await Task.detached { () throws -> ([String], Int) in
                    var choices: [PlaceholderChoice] = []
                    var addedIds = Set<String>(), all = Set<String>()
                    var byDoi: [String: String] = [:]
                    for it in todo {
                        var keys: [String] = []
                        for c in it.cites {
                            guard c.state == .confirmed else { keys.append(""); continue }
                            let cand = c.candidates[c.pick]
                            var id = cand.paperId
                            if id.isEmpty {
                                if let known = byDoi[cand.meta.doi.lowercased() + "|" + cand.meta.title] { id = known } else {
                                    let had = lib.findCited(doi: cand.meta.doi, title: cand.meta.title) != nil
                                    let p = try lib.addFoundPaper(meta: cand.meta)
                                    if !had { addedIds.insert(p.id) }
                                    id = p.id
                                    byDoi[cand.meta.doi.lowercased() + "|" + cand.meta.title] = id
                                }
                            }
                            all.insert(id)
                            if !folder.isEmpty { try lib.setMembership(collection: folder, paper: id, present: true) }
                            keys.append(lib.citeKeys(ids: [id]).first ?? "")
                        }
                        choices.append(PlaceholderChoice(start: it.placeholder.start, end: it.placeholder.end, keys: keys))
                    }
                    let out = rewriteTexPlaceholders(text: text, choices: choices)
                    if out != text {
                        // the file as it was, then the new text in its place
                        let bak = file.appendingPathExtension("bak")
                        try? FileManager.default.removeItem(at: bak)
                        try FileManager.default.copyItem(at: file, to: bak)
                        try Data(out.utf8).write(to: file, options: .atomic)
                    }
                    return (Array(addedIds), all.count)
                }.value
                library.reload()
                BibFiles.shared.update(library, delay: 0)
                report = summary(written: todo.count, added: added.count, papers: papers)
                // papers added: their open-access PDFs and reading views, as when adding by DOI
                let model = library
                Task { for id in added { await model.findPDF(id) } }
            } catch {
                self.error = "Nothing was written to “\(file.lastPathComponent)”: \(error.readable)"
            }
            writing = false
        }
    }

    private func summary(written: Int, added: Int, papers: Int) -> String {
        let left = items.count - written
        var s = written == 1 ? "1 placeholder resolved" : "\(written) placeholders resolved"
        s += " in “\(file.lastPathComponent)” (the original is “\(file.lastPathComponent).bak”)"
        if left > 0 { s += "; \(left) left as written" }
        s += ". "
        if added > 0 { s += added == 1 ? "1 paper added to your library. " : "\(added) papers added to your library. " }
        if let c = library.collections.first(where: { $0.id == folder }) {
            if let l = BibFiles.shared.link(folder) {
                s += "\(papers == 1 ? "The paper is" : "The \(papers) papers are") in “\(c.name)”, whose BibTeX file Lunta keeps updated: \(l.path)."
            } else {
                s += "\(papers == 1 ? "The paper is" : "The \(papers) papers are") in “\(c.name)”. Keep its BibTeX file updated (the folder’s menu) to use them in the LaTeX project."
            }
        } else {
            s += "Keep a BibTeX file updated for a folder with these papers (the folder’s menu) to use them in the LaTeX project."
        }
        return s
    }
}

/// The window listing the .tex file's placeholders with Lunta's candidates.
@MainActor
enum TexResolveWindow {
    private static var windows: [NSWindow] = []

    static func show(_ m: TexResolveModel) {
        let w = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 640, height: 640), styleMask: [.titled, .closable, .resizable, .miniaturizable],
                         backing: .buffered, defer: false)
        w.title = "Resolve Citations: \(m.file.lastPathComponent)"
        w.isReleasedWhenClosed = false
        w.contentView = NSHostingView(rootView: TexResolveView(m: m, close: { [weak w] in w?.close() }).environmentObject(m.library))
        w.center()
        windows.append(w)
        NotificationCenter.default.addObserver(forName: NSWindow.willCloseNotification, object: w, queue: .main) { n in
            MainActor.assumeIsolated { windows.removeAll { $0 === n.object as? NSWindow } }
        }
        w.makeKeyAndOrderFront(nil)
        m.load()
    }
}

struct TexResolveView: View {
    @ObservedObject var m: TexResolveModel
    @ObservedObject private var files = BibFiles.shared
    let close: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("Lunta proposes, you decide: each placeholder is rewritten as \\cite{key} only when you confirm it. Papers not in your library are added when the file is written.")
                .font(.callout).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            HStack {
                Picker("Put the papers in", selection: $m.folder) {
                    Text("No folder").tag("")
                    ForEach(m.library.collections, id: \.id) { c in
                        Text(files.link(c.id) != nil ? "\(c.name) (.bib kept)" : c.name).tag(c.id)
                    }
                }
                .frame(maxWidth: 380)
                Spacer()
                if m.exactCount > 0 {
                    Button("Confirm All Exact Matches (\(m.exactCount))") { m.confirmExact() }
                }
            }
            List {
                ForEach(m.items) { it in
                    VStack(alignment: .leading, spacing: 6) {
                        Text(it.placeholder.text).font(.system(.body, design: .monospaced)).bold()
                        ForEach(it.cites.indices, id: \.self) { j in
                            citeRow(it.id, j, it.cites[j], label: it.cites.count > 1)
                        }
                    }
                    .padding(.vertical, 4)
                }
            }
            if let e = m.error { Text(e).foregroundStyle(.red).fixedSize(horizontal: false, vertical: true) }
            if let r = m.report { Text(r).fixedSize(horizontal: false, vertical: true).textSelection(.enabled) }
            HStack {
                Text("Overleaf: download the project (or use its Git or Dropbox sync), resolve the .tex here, and upload it with the kept .bib.")
                    .font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                Spacer()
                Button(m.report == nil ? "Cancel" : "Done") { close() }
                Button("Write \(m.confirmedCount == 1 ? "1 Citation" : "\(m.confirmedCount) Citations")") { m.write() }
                    .keyboardShortcut(.defaultAction)
                    .disabled(m.writing || m.confirmedCount == 0 || m.report != nil)
            }
        }
        .padding(16)
        .frame(minWidth: 560, minHeight: 480)
    }

    @ViewBuilder
    private func citeRow(_ i: Int, _ j: Int, _ c: TexCite, label: Bool) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            if label { Text(c.cite.text).font(.caption).foregroundStyle(.teal) }
            switch c.state {
            case .loading:
                ProgressView().controlSize(.small)
            case .confirmed:
                HStack {
                    Text("✓ " + (c.candidates[c.pick].meta.title.isEmpty ? "Untitled" : c.candidates[c.pick].meta.title)).lineLimit(2)
                    Spacer()
                    Button("Change") { m.reopen(i, j) }.buttonStyle(.link)
                }
            case .skipped:
                HStack {
                    Text("Skipped: stays as written").foregroundStyle(.secondary)
                    Spacer()
                    Button("Change") { m.reopen(i, j) }.buttonStyle(.link)
                }
            case .open:
                ForEach(c.candidates.indices, id: \.self) { k in
                    let cand = c.candidates[k]
                    Button { m.items[i].cites[j].pick = k } label: {
                        HStack(alignment: .top) {
                            Image(systemName: k == c.pick ? "largecircle.fill.circle" : "circle").foregroundStyle(.teal)
                            VStack(alignment: .leading, spacing: 1) {
                                Text(cand.meta.title.isEmpty ? "Untitled" : cand.meta.title).lineLimit(2)
                                Text(Self.byline(cand)).font(.caption).foregroundStyle(.secondary)
                                Text(Self.source(cand)).font(.caption2).foregroundStyle(.secondary)
                            }
                        }
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                }
                if !c.note.isEmpty { Text(c.note).font(.caption).foregroundStyle(.secondary) }
                HStack {
                    Button("Confirm") { m.confirm(i, j) }.disabled(c.candidates.isEmpty)
                    Button("Skip") { m.skip(i, j) }
                    Button("Search…") {
                        m.items[i].cites[j].searching.toggle()
                        if m.items[i].cites[j].query.isEmpty { m.items[i].cites[j].query = (c.cite.surnames + c.cite.hints).joined(separator: " ") }
                    }
                }
                if c.searching {
                    HStack {
                        TextField("Title words, authors, a DOI…", text: $m.items[i].cites[j].query).onSubmit { m.search(i, j) }
                        Button("Search") { m.search(i, j) }
                    }
                }
            }
        }
    }

    static func byline(_ c: CitationCandidate) -> String {
        let f = c.meta.authors.map { familyName(name: $0) }
        let who = f.count > 2 ? "\(f[0]) et al." : f.joined(separator: " & ")
        return [who, String(c.meta.date.prefix(4)), c.meta.journal].filter { !$0.isEmpty }.joined(separator: " · ")
    }

    static func source(_ c: CitationCandidate) -> String {
        let where_ = c.paperId.isEmpty ? (c.source == "openalex" ? "OpenAlex" : "Crossref") + " · added to your library when written" : "In your library · \\cite{\(c.citeKey)}"
        return where_ + " · \(Int((c.confidence * 100).rounded()))%" + (c.exact ? " · exact match" : "")
    }
}
