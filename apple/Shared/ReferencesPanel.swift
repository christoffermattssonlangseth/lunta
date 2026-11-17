import SwiftUI
import LuntaCore

/// One entry of the paper's reference list, as the page reports it (`luntaReferences()`,
/// References.swift).
struct PaperReference: Decodable, Identifiable, Hashable {
    let index: Int
    let label: String
    let text: String
    let doi: String
    let arxiv: String
    let pmid: String
    let pmcid: String
    let anchor: String
    let cited: Int

    var id: Int { index }

    /// What `LibraryModel.add` is given for it; nil when it has no identifier.
    var addInput: String? {
        if !doi.isEmpty { return doi }
        if !arxiv.isEmpty { return "arXiv:\(arxiv)" }
        if !pmcid.isEmpty { return pmcid }
        if !pmid.isEmpty { return pmid }
        return nil
    }

    var scholarURL: URL? {
        let q = text.split(whereSeparator: \.isWhitespace).joined(separator: " ").prefix(200)
        var c = URLComponents(string: "https://scholar.google.com/scholar")
        c?.queryItems = [URLQueryItem(name: "q", value: String(q))]
        return c?.url
    }
}

/// The References panel's state: the list from the page, which entries are in the library
/// (looked up locally by DOI, arXiv, PubMed and PMC ids) and what is being added.
@MainActor
final class ReferencesModel: ObservableObject {
    @Published private(set) var refs: [PaperReference] = []
    @Published private(set) var loaded = false
    /// reference index → paper id in the library
    @Published private(set) var inLibrary: [Int: String] = [:]
    @Published private(set) var adding: Set<Int> = []
    @Published private(set) var failed: Set<Int> = []
    /// Add All Missing: how far it got (nil when not running)
    @Published private(set) var bulk: (done: Int, total: Int)?
    private var bulkTask: Task<Void, Never>?

    var missing: [PaperReference] { refs.filter { $0.addInput != nil && inLibrary[$0.index] == nil } }

    /// Reads the list from the reading view (waits for the page's script if it isn't there yet).
    func load(_ controller: ReaderController, model: LibraryModel) async {
        for _ in 0..<40 {
            if let v = controller.webView,
               let json = try? await v.evaluateJavaScript("window.luntaReferences ? luntaReferences() : null") as? String,
               let data = json.data(using: .utf8) {
                refs = (try? JSONDecoder().decode([PaperReference].self, from: data)) ?? []
                loaded = true
                refresh(model)
                return
            }
            try? await Task.sleep(for: .milliseconds(300))
        }
        loaded = true
    }

    /// Which references are in the library, from the library's own ids (no network).
    func refresh(_ model: LibraryModel) {
        guard !refs.isEmpty else { if !inLibrary.isEmpty { inLibrary = [:] }; return }
        var index: [String: String] = [:]
        let all = model.lib.papers(filter: PaperFilter(collection: nil, tag: nil, starredOnly: false, unreadOnly: false), sort: .added)
        for p in all {
            let m = p.meta
            if !m.doi.isEmpty {
                let d = Self.doiKey(m.doi)
                index["doi:" + d] = p.id
                if d.hasPrefix("10.48550/arxiv.") { index["arxiv:" + Self.arxivKey(String(d.dropFirst(15)))] = p.id }
            }
            if !m.arxiv.isEmpty { index["arxiv:" + Self.arxivKey(m.arxiv)] = p.id }
            if !m.pmid.isEmpty { index["pmid:" + m.pmid.trimmingCharacters(in: .whitespaces)] = p.id }
            if !m.pmcid.isEmpty { index["pmcid:" + m.pmcid.uppercased().trimmingCharacters(in: .whitespaces)] = p.id }
        }
        var found: [Int: String] = [:]
        for r in refs {
            let keys = [r.doi.isEmpty ? nil : "doi:" + Self.doiKey(r.doi),
                        r.arxiv.isEmpty ? nil : "arxiv:" + Self.arxivKey(r.arxiv),
                        r.pmid.isEmpty ? nil : "pmid:" + r.pmid,
                        r.pmcid.isEmpty ? nil : "pmcid:" + r.pmcid.uppercased()].compactMap { $0 }
            if let id = keys.lazy.compactMap({ index[$0] }).first { found[r.index] = id }
        }
        if found != inLibrary { inLibrary = found }
    }

    static func doiKey(_ d: String) -> String {
        var s = d.trimmingCharacters(in: .whitespaces).lowercased()
        for p in ["https://doi.org/", "http://doi.org/", "https://dx.doi.org/", "doi:"] where s.hasPrefix(p) { s = String(s.dropFirst(p.count)) }
        return s
    }

    static func arxivKey(_ a: String) -> String {
        a.lowercased().replacingOccurrences(of: "arxiv:", with: "")
            .replacingOccurrences(of: #"v\d+$"#, with: "", options: .regularExpression)
    }

    /// Adds one reference to the library (Add).
    func add(_ r: PaperReference, model: LibraryModel, quiet: Bool = false) async {
        guard let input = r.addInput, inLibrary[r.index] == nil, !adding.contains(r.index) else { return }
        adding.insert(r.index)
        failed.remove(r.index)
        let id = await model.add(input)
        adding.remove(r.index)
        if let id { inLibrary[r.index] = id } else {
            failed.insert(r.index)
            // Add All Missing: the row says it, not an alert for each one
            if quiet { model.errorMessage = nil }
        }
        refresh(model)
    }

    /// Add All Missing: three at a time, until done or cancelled.
    func addAllMissing(model: LibraryModel) {
        let todo = missing
        guard !todo.isEmpty, bulkTask == nil else { return }
        bulk = (0, todo.count)
        bulkTask = Task { [weak self] in
            var i = 0
            while i < todo.count, !Task.isCancelled {
                let chunk = Array(todo[i..<min(i + 3, todo.count)])
                i += chunk.count
                await withTaskGroup(of: Void.self) { group in
                    for r in chunk {
                        group.addTask { @MainActor [weak self] in await self?.add(r, model: model, quiet: true) }
                    }
                }
                self?.bulk = (i, todo.count)
            }
            self?.bulk = nil
            self?.bulkTask = nil
        }
    }

    func cancelAll() {
        bulkTask?.cancel()
    }
}

/// The list in the References tab (Mac) and sheet (iPhone, iPad).
struct ReferencesList: View {
    @EnvironmentObject var model: LibraryModel
    @ObservedObject var refs: ReferencesModel
    /// Jump to the reference in the reading view.
    var onShow: (PaperReference) -> Void
    /// Open a paper that is in the library.
    var onOpen: (String) -> Void
    @State private var query = ""
    @State private var confirming = false
    @Environment(\.openURL) private var openURL

    private var shown: [PaperReference] {
        let q = query.trimmingCharacters(in: .whitespaces)
        guard !q.isEmpty else { return refs.refs }
        return refs.refs.filter { r in
            r.text.localizedStandardContains(q) || r.label == q || r.doi.localizedCaseInsensitiveContains(q)
        }
    }

    var body: some View {
        VStack(spacing: 0) {
            if !refs.loaded {
                ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if refs.refs.isEmpty {
                ContentUnavailableView("No references found", systemImage: "text.book.closed",
                                       description: Text("This reading view has no reference list Lunta can read."))
            } else {
                header
                searchField
                Divider()
                List(shown) { r in row(r) }
                    .listStyle(.plain)
                    .overlay { if shown.isEmpty { ContentUnavailableView.search(text: query) } }
            }
        }
        .onReceive(model.$papers) { _ in refs.refresh(model) }
        .confirmationDialog(confirmTitle, isPresented: $confirming, titleVisibility: .visible) {
            Button("Add \(refs.missing.count)") { refs.addAllMissing(model: model) }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text(skipped > 0 ? "\(skipped) without a DOI, arXiv or PubMed id are skipped." : "They are added a few at a time.")
        }
    }

    private var confirmTitle: String {
        refs.missing.count == 1 ? "Add 1 reference to your library?" : "Add \(refs.missing.count) references to your library?"
    }

    private var skipped: Int { refs.refs.filter { $0.addInput == nil }.count }

    private var header: some View {
        VStack(alignment: .leading, spacing: 6) {
            let n = refs.refs.count, m = refs.inLibrary.count
            Text("\(n) \(n == 1 ? "reference" : "references") · \(m) in your library")
                .font(.callout).foregroundStyle(.secondary)
            if let b = refs.bulk {
                HStack(spacing: 8) {
                    ProgressView(value: Double(b.done), total: Double(max(b.total, 1)))
                    Text("\(b.done) of \(b.total)").font(.caption).monospacedDigit().foregroundStyle(.secondary)
                    Button("Cancel") { refs.cancelAll() }.controlSize(.small)
                }
            } else if !refs.missing.isEmpty {
                Button { confirming = true } label: { Label("Add All Missing", systemImage: "plus.circle") }
                    .controlSize(.small)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.horizontal, 14).padding(.top, 4).padding(.bottom, 8)
    }

    private var searchField: some View {
        HStack(spacing: 6) {
            Image(systemName: "magnifyingglass").foregroundStyle(.secondary)
            TextField("Filter", text: $query).textFieldStyle(.plain)
            if !query.isEmpty {
                Button { query = "" } label: { Image(systemName: "xmark.circle.fill").foregroundStyle(.secondary) }
                    .buttonStyle(.plain).accessibilityLabel("Clear")
            }
        }
        .padding(.horizontal, 8).padding(.vertical, 5)
        .background(.quaternary.opacity(0.6), in: RoundedRectangle(cornerRadius: 7))
        .padding(.horizontal, 12).padding(.bottom, 8)
    }

    private func row(_ r: PaperReference) -> some View {
        let addable = r.addInput != nil
        return HStack(alignment: .firstTextBaseline, spacing: 8) {
            Text(r.label.isEmpty ? "•" : r.label)
                .font(.caption.monospacedDigit()).foregroundStyle(.secondary)
                .lineLimit(1).frame(minWidth: 18, alignment: .trailing)
                .frame(maxWidth: 70, alignment: .trailing)
            VStack(alignment: .leading, spacing: 4) {
                Button { onShow(r) } label: {
                    Text(r.text).font(.callout).lineLimit(3)
                        .foregroundStyle(addable || refs.inLibrary[r.index] != nil ? .primary : .secondary)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .help("Show in the paper")
                HStack(spacing: 8) {
                    if r.cited > 0 { Text("Cited \(r.cited)×").font(.caption).foregroundStyle(.tertiary) }
                    Spacer(minLength: 0)
                    status(r)
                }
            }
        }
        .padding(.vertical, 3)
    }

    @ViewBuilder
    private func status(_ r: PaperReference) -> some View {
        if let id = refs.inLibrary[r.index] {
            Button { onOpen(id) } label: { Label("In library", systemImage: "checkmark.circle.fill") }
                .buttonStyle(.borderless).foregroundStyle(.green).font(.caption.weight(.medium))
                .help("In your library. Open it")
        } else if refs.adding.contains(r.index) {
            HStack(spacing: 4) { ProgressView().controlSize(.mini); Text("Adding…") }
                .font(.caption).foregroundStyle(.secondary)
        } else if r.addInput != nil {
            HStack(spacing: 6) {
                if refs.failed.contains(r.index) { Text("Couldn’t add").font(.caption).foregroundStyle(.orange) }
                Button { Task { await refs.add(r, model: model) } } label: { Label("Add", systemImage: "plus.circle") }
                    .buttonStyle(.borderless).tint(.luntaTeal).font(.caption.weight(.medium))
                    .help("Add to your library")
            }
        } else {
            Button { if let u = r.scholarURL { openURL(u) } } label: { Label("Search", systemImage: "magnifyingglass") }
                .buttonStyle(.borderless).foregroundStyle(.secondary).font(.caption)
                .help("No DOI, arXiv or PubMed id: search Google Scholar")
        }
    }
}
