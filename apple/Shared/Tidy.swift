import SwiftUI
import LuntaCore

// Tidy the library: the suggestions Clean Up Library makes beyond duplicates and wrong details,
// all from the core (core/src/tidy): papers missing a DOI (looked up on request), a PDF or a
// reading view, preprints with a published version, topics to make into tags or folders (the
// on-device model when there is one, else words the papers share), and existing tags for
// untagged papers. Lunta proposes, the researcher decides: nothing changes until a suggestion is
// applied, and tags, folders and DOIs can be undone.

/// What was just applied, for Undo.
struct TidyUndo {
    let summary: String
    let change: TidyChange
}

extension LibraryModel {
    /// The suggestions that need no network.
    func tidyReport() async -> TidyReport {
        let lib = self.lib
        return await Task.detached { lib.tidyReport() }.value
    }

    func findMissingDOIs(_ ids: [String], progress: @escaping @Sendable (String) -> Void) async -> DoiReport {
        let lib = self.lib
        let listener = BibImportProgress(progress)
        return await Task.detached { lib.findMissingDois(ids: ids, progress: listener) }.value
    }

    /// Applies an approved suggestion; reloads the library. nil (and the error shown) on failure.
    func applyTidy(_ apply: (Library) throws -> TidyChange) -> TidyUndo? {
        do {
            let ch = try apply(lib)
            reload()
            return TidyUndo(summary: ch.summary, change: ch)
        } catch {
            errorMessage = "Couldn't apply it: \(error.readable)"
            return nil
        }
    }

    func undoTidy(_ u: TidyUndo) {
        do { try lib.undoTidyChange(change: u.change) } catch { errorMessage = "Couldn't undo: \(error.readable)" }
        reload()
    }

    /// Topics from the on-device model (the latest papers' titles; fewer for Apple's model, whose
    /// context is small). Empty when there is no model or its answer had nothing usable.
    func modelTopics(_ engine: AskEngine) async -> [GroupProposal] {
        guard engine.unavailableReason == nil else { return [] }
        let p = lib.tidyTopicPrompt(maxPapers: engine.usesOpenModel ? 150 : 60)
        guard let answer = await engine.plainAnswer(p.prompt, instructions: p.instructions) else { return [] }
        return lib.tidyGroupsFromAnswer(prompt: p, answer: answer)
    }
}

/// The suggestion sections of Clean Up Library (in its list, after the duplicates).
struct TidySections: View {
    @EnvironmentObject var model: LibraryModel
    @StateObject private var engine = AskEngine(scope: .paper("__tidy__"))
    @State private var report: TidyReport?
    @State private var dois: DoiReport?
    @State private var doiPicked: Set<String> = []
    @State private var lookingUp: String?
    @State private var findingPDFs: String?
    @State private var topics: [GroupProposal] = []
    @State private var topicsByModel = false
    @State private var topicPicked: Set<Int> = []
    @State private var expanded: Set<Int> = []
    @State private var asking = false
    @State private var tagPicked: Set<String> = []
    @State private var undo: TidyUndo?
    @State private var switching: String?
    /// Bumped by Clean Up Library after a merge or fix, to refresh these suggestions.
    var refresh: Int = 0

    var body: some View {
        if let undo {
            Section {
                HStack {
                    Image(systemName: "checkmark.circle").foregroundStyle(Color.luntaTeal)
                    Text(undo.summary).font(.callout)
                    Spacer()
                    Button("Undo") { model.undoTidy(undo); self.undo = nil; load() }.buttonStyle(.bordered)
                }
            }
        }
        missingSection
        if let r = report, !r.missing.published.isEmpty { publishedSection(r) }
        topicSection
            .task(id: refresh) { load() }
        tagSection
    }

    private func load() {
        Task {
            let r = await model.tidyReport()
            report = r
            if !topicsByModel { topics = r.topics; topicPicked = [] }
            let valid = Set(r.tagSuggestions.map(key))
            tagPicked = tagPicked.intersection(valid)
        }
    }

    private func title(_ id: String) -> String {
        let t = model.paper(id)?.meta.title ?? ""
        return t.isEmpty ? "Untitled" : t
    }

    private func line(_ m: PaperMeta) -> String {
        [m.authorLine, m.year, m.journal].filter { !$0.isEmpty }.joined(separator: " · ")
    }

    // MARK: missing details

    @ViewBuilder private var missingSection: some View {
        if let m = report?.missing {
            Section {
                if m.noDoi.isEmpty && m.noPdf.isEmpty && m.noReadingView.isEmpty {
                    Text("Every paper has a DOI, a PDF and a reading view.").foregroundStyle(.secondary)
                }
                if !m.noDoi.isEmpty {
                    actionRow("No DOI", count: m.noDoi.count, icon: "number",
                              button: lookingUp ?? (dois == nil ? "Find DOIs" : "Look Up Again"), busy: lookingUp != nil) { findDOIs(m.noDoi) }
                    if let d = dois { doiResults(d) }
                }
                if !m.noPdf.isEmpty {
                    actionRow("No PDF", count: m.noPdf.count, icon: "doc", button: findingPDFs ?? "Find PDFs", busy: findingPDFs != nil) { findPDFs(m.noPdf) }
                }
                if !m.noReadingView.isEmpty {
                    actionRow("No reading view", count: m.noReadingView.count, icon: "book", button: "Make Reading Views", busy: false) {
                        Task { await model.makeReadingViews(m.noReadingView); load() }
                    }
                }
            } header: {
                Text("Missing details")
            } footer: {
                Text("Find DOIs asks Crossref for each paper's title, first author and year and takes only a record that plainly is the paper. Find PDFs looks for legal open-access copies. Nothing is changed until you apply it.")
            }
        }
    }

    private func actionRow(_ label: String, count: Int, icon: String, button: String, busy: Bool, action: @escaping () -> Void) -> some View {
        HStack {
            Label("\(label) (\(count))", systemImage: icon)
            Spacer()
            if busy { ProgressView().controlSize(.small) }
            Button(button, action: action).buttonStyle(.bordered).disabled(busy)
        }
        .font(.callout)
    }

    @ViewBuilder private func doiResults(_ d: DoiReport) -> some View {
        if d.suggestions.isEmpty {
            Text(d.errors.isEmpty ? "No DOI was plainly found." : "Couldn't ask Crossref: \(d.errors.joined(separator: "; "))")
                .font(.caption).foregroundStyle(.secondary)
        } else {
            ForEach(d.suggestions, id: \.paperId) { s in
                Toggle(isOn: Binding(get: { doiPicked.contains(s.paperId) }, set: { on in if on { doiPicked.insert(s.paperId) } else { doiPicked.remove(s.paperId) } })) {
                    VStack(alignment: .leading, spacing: 2) {
                        Text(title(s.paperId)).lineLimit(2)
                        if !s.found.title.isEmpty, s.found.title.lowercased() != title(s.paperId).lowercased() {
                            Text("Found: \(s.found.title)").font(.caption).foregroundStyle(.secondary).lineLimit(2)
                        }
                        Text([s.doi, line(s.found), s.source].filter { !$0.isEmpty }.joined(separator: " · ")).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                        if s.duplicateOf != nil {
                            Text("Another paper in the library has this DOI: it is a duplicate to merge.").font(.caption).foregroundStyle(Color.luntaCopper)
                        }
                    }
                }
                #if os(macOS)
                .toggleStyle(.checkbox)
                #endif
            }
            if !d.notFound.isEmpty {
                Text("Nothing plainly found for \(d.notFound.count) paper\(d.notFound.count == 1 ? "" : "s").").font(.caption).foregroundStyle(.secondary)
            }
            HStack {
                Button("Apply Selected (\(doiPicked.count))") { applyDOIs(d.suggestions.filter { doiPicked.contains($0.paperId) }) }
                    .disabled(doiPicked.isEmpty)
                Button("Apply All") { applyDOIs(d.suggestions) }
                Spacer()
            }
            .buttonStyle(.bordered).font(.callout)
        }
    }

    private func findDOIs(_ ids: [String]) {
        lookingUp = "Looking up…"
        Task {
            let r = await model.findMissingDOIs(ids) { m in Task { @MainActor in lookingUp = m } }
            dois = r
            doiPicked = Set(r.suggestions.filter { $0.duplicateOf == nil }.map(\.paperId))
            lookingUp = nil
        }
    }

    private func applyDOIs(_ s: [DoiSuggestion]) {
        guard let u = model.applyTidy({ try $0.applyDoiSuggestions(suggestions: s) }) else { return }
        undo = u
        let done = Set(s.map(\.paperId))
        if let d = dois {
            dois = DoiReport(suggestions: d.suggestions.filter { !done.contains($0.paperId) }, notFound: d.notFound, errors: d.errors)
        }
        doiPicked.subtract(done)
        load()
    }

    private func findPDFs(_ ids: [String]) {
        Task {
            var found = 0
            for (i, id) in ids.enumerated() {
                findingPDFs = "\(i + 1) of \(ids.count)…"
                if await model.findPDF(id) != nil { found += 1 }
            }
            findingPDFs = nil
            model.status = "Found open-access PDFs for \(found) of \(ids.count) papers."
            load()
        }
    }

    // MARK: published preprints

    private func publishedSection(_ r: TidyReport) -> some View {
        Section {
            ForEach(r.missing.published, id: \.paperId) { s in
                HStack(alignment: .top) {
                    VStack(alignment: .leading, spacing: 2) {
                        Text(title(s.paperId)).lineLimit(2)
                        Text("Published in \(s.version.journal.isEmpty ? "a journal" : s.version.journal)\(s.version.date.isEmpty ? "" : ", \(String(s.version.date.prefix(4)))")")
                            .font(.caption).foregroundStyle(.secondary)
                    }
                    Spacer()
                    if switching == s.paperId { ProgressView().controlSize(.small) }
                    Button("Switch…") { switching = s.paperId }.buttonStyle(.bordered).disabled(switching != nil)
                }
                .font(.callout)
            }
        } header: {
            Text("Preprints that have been published (\(r.missing.published.count))")
        } footer: {
            Text("Switch to the Published Version takes the published details and keeps your highlights, notes, tags and citation key (it can be undone in the paper's details).")
        }
        .confirmationDialog("Switch to the published version?", isPresented: Binding(get: { switching != nil && !switchingNow }, set: { if !$0 { switching = nil } })) {
            Button("Switch") {
                guard let id = switching else { return }
                switchingNow = true
                Task { _ = await model.usePublishedVersion(id); switchingNow = false; switching = nil; load() }
            }
        } message: {
            Text("The paper's details become the published version's. Highlights, notes, tags and the citation key stay.")
        }
    }
    @State private var switchingNow = false

    // MARK: topics

    @ViewBuilder private var topicSection: some View {
        Section {
            if topics.isEmpty {
                Text(asking ? "Asking the on-device model…" : "No topics to suggest: the library is small, or its papers share few words that aren't tags or folders already.")
                    .foregroundStyle(.secondary).font(.callout)
            }
            ForEach(Array(topics.enumerated()), id: \.offset) { i, t in topicRow(i, t) }
            if !topics.isEmpty {
                HStack {
                    Button("Create as Tags") { createTopics(asFolders: false) }.disabled(topicPicked.isEmpty)
                    Button("Create as Folders") { createTopics(asFolders: true) }.disabled(topicPicked.isEmpty)
                    Spacer()
                }
                .buttonStyle(.bordered).font(.callout)
            }
        } header: {
            HStack {
                Text("Suggested tags and folders")
                Spacer()
                if engine.unavailableReason == nil {
                    Button(asking ? "Asking…" : topicsByModel ? "Suggest Again" : "Suggest with On-Device AI") { askModel() }
                        .disabled(asking).font(.callout)
                }
            }
        } footer: {
            Text(topicsByModel
                 ? "Grouped by the on-device model from the latest papers' titles; it can be wrong. Tick the ones to create; names can be changed first."
                 : "Found from words the papers share (titles, keywords, abstracts) and journals. Tick the ones to create; names can be changed first.")
        }
    }

    private func topicRow(_ i: Int, _ t: GroupProposal) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Toggle(isOn: Binding(get: { topicPicked.contains(i) }, set: { on in if on { topicPicked.insert(i) } else { topicPicked.remove(i) } })) {
                    TextField("Name", text: Binding(get: { topics.indices.contains(i) ? topics[i].name : "" },
                                                    set: { if topics.indices.contains(i) { topics[i].name = $0 } }))
                        .textFieldStyle(.plain)
                }
                #if os(macOS)
                .toggleStyle(.checkbox)
                #endif
                Text("— \(t.paperIds.count) papers").foregroundStyle(.secondary)
                Button { if expanded.contains(i) { expanded.remove(i) } else { expanded.insert(i) } } label: {
                    Image(systemName: expanded.contains(i) ? "chevron.up" : "chevron.down")
                }
                .buttonStyle(.borderless)
            }
            Text(t.why.capitalizedFirst).font(.caption).foregroundStyle(.secondary)
            if expanded.contains(i) {
                ForEach(t.paperIds, id: \.self) { id in
                    Text("• " + title(id)).font(.caption).lineLimit(1)
                }
            }
        }
        .font(.callout)
    }

    private func askModel() {
        asking = true
        Task {
            let got = await model.modelTopics(engine)
            asking = false
            if got.isEmpty {
                model.errorMessage = "The on-device model gave no usable topics; the ones found from shared words are shown."
            } else {
                topics = got
                topicsByModel = true
                topicPicked = []
                expanded = []
            }
        }
    }

    private func createTopics(asFolders: Bool) {
        let picked = topicPicked.sorted().compactMap { topics.indices.contains($0) ? topics[$0] : nil }
        guard let u = model.applyTidy({ try $0.createGroups(proposals: picked, asFolders: asFolders) }) else { return }
        undo = u
        topics = topics.enumerated().filter { !topicPicked.contains($0.offset) }.map(\.element)
        topicPicked = []
        expanded = []
        load()
    }

    // MARK: tags for untagged papers

    private func key(_ s: TagSuggestion) -> String { s.paperId + "\u{1}" + s.tag }

    @ViewBuilder private var tagSection: some View {
        if let r = report, !r.tagSuggestions.isEmpty {
            Section {
                ForEach(r.tagSuggestions, id: \.paperId_tag) { s in
                    HStack(alignment: .top) {
                        Toggle(isOn: Binding(get: { tagPicked.contains(key(s)) }, set: { on in if on { tagPicked.insert(key(s)) } else { tagPicked.remove(key(s)) } })) {
                            VStack(alignment: .leading, spacing: 2) {
                                HStack(spacing: 6) {
                                    Text(title(s.paperId)).lineLimit(1)
                                    Image(systemName: "arrow.right").font(.caption).foregroundStyle(.secondary)
                                    Pill(text: s.tag, icon: "tag", color: .luntaTeal)
                                }
                                Text(s.because.capitalizedFirst).font(.caption).foregroundStyle(.secondary).lineLimit(2)
                            }
                        }
                        #if os(macOS)
                        .toggleStyle(.checkbox)
                        #endif
                        Spacer()
                        Button { try? model.lib.tidyDismiss(key: "tag:\(s.paperId):\(s.tag)"); load() } label: { Image(systemName: "xmark") }
                            .buttonStyle(.borderless).help("Don't suggest this again")
                    }
                    .font(.callout)
                }
                HStack {
                    Button("Apply Selected (\(tagPicked.count))") { applyTags(r.tagSuggestions.filter { tagPicked.contains(key($0)) }) }
                        .disabled(tagPicked.isEmpty)
                    Button("Apply All") { applyTags(r.tagSuggestions) }
                    Spacer()
                }
                .buttonStyle(.bordered).font(.callout)
            } header: {
                Text("Tags for untagged papers (\(Set(r.tagSuggestions.map(\.paperId)).count))")
            } footer: {
                Text("Your tags, suggested for papers without any: the tag's name is in the paper's title or keywords, or the paper shares distinctive words with the papers that have it.")
            }
        }
    }

    private func applyTags(_ s: [TagSuggestion]) {
        guard let u = model.applyTidy({ try $0.applyTagSuggestions(suggestions: s) }) else { return }
        undo = u
        tagPicked = []
        load()
    }
}

extension TagSuggestion {
    var paperId_tag: String { paperId + "\u{1}" + tag }
}
