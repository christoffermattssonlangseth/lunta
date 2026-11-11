import SwiftUI
import MarginsCore

// Library cleanup: papers whose details look wrong (fixed from the DOI, or read from the PDF on
// the device) and papers that are in the library twice (merged into one, keeping everything).

struct DetailIssue: Identifiable {
    let paper: Paper
    let problems: [String]
    var id: String { paper.id }
}

struct DuplicateGroup: Identifiable {
    let papers: [Paper]
    let reason: String
    var id: String { papers.map(\.id).sorted().joined(separator: "+") }
}

extension PaperMeta {
    /// Fills empty fields from another record.
    mutating func fillEmpty(from o: PaperMeta) {
        if title.isEmpty { title = o.title }
        if authors.isEmpty { authors = o.authors }
        if date.isEmpty { date = o.date }
        if journal.isEmpty { journal = o.journal }
        if volume.isEmpty { volume = o.volume }
        if issue.isEmpty { issue = o.issue }
        if pages.isEmpty { pages = o.pages }
        if publisher.isEmpty { publisher = o.publisher }
        if doi.isEmpty { doi = o.doi }
        if pmid.isEmpty { pmid = o.pmid }
        if pmcid.isEmpty { pmcid = o.pmcid }
        if arxiv.isEmpty { arxiv = o.arxiv }
        if url.isEmpty { url = o.url }
        if abstractText.isEmpty { abstractText = o.abstractText }
        if keywords.isEmpty { keywords = o.keywords }
        if license.isEmpty { license = o.license }
    }
}

extension LibraryModel {
    private var everyPaper: [Paper] {
        lib.papers(filter: PaperFilter(collection: nil, tag: nil, starredOnly: false, unreadOnly: false), sort: .added)
    }

    static func problems(_ m: PaperMeta) -> [String] {
        var out: [String] = []
        let title = m.title.trimmingCharacters(in: .whitespacesAndNewlines)
        let badAuthors = m.authors.isEmpty || m.authors.contains { a in
            let t = a.trimmingCharacters(in: .whitespaces)
            return t.count < 3 || t.allSatisfy { $0.isNumber || $0.isPunctuation || $0 == " " }
        }
        if title.isEmpty { out.append("no title") }
        // a one- or two-word title is fine (e.g. "Hyperagents") unless the authors are wrong too
        else if title.count < 16 && title.split(separator: " ").count <= 2 && badAuthors { out.append("title looks wrong") }
        else if !m.journal.isEmpty && title.caseInsensitiveCompare(m.journal) == .orderedSame { out.append("title is the journal name") }
        else if title.lowercased().hasSuffix(".pdf") || title.range(of: #"^[\w-]+_\d+"#, options: .regularExpression) != nil { out.append("title is a file name") }
        if m.authors.isEmpty { out.append("no authors") }
        else if m.authors.contains(where: { a in
            let t = a.trimmingCharacters(in: .whitespaces)
            return t.count < 3 || t.allSatisfy { $0.isNumber || $0.isPunctuation || $0 == " " }
        }) { out.append("authors look wrong") }
        if m.date.trimmingCharacters(in: .whitespaces).isEmpty { out.append("no year") }
        return out
    }

    func detailIssues() -> [DetailIssue] {
        everyPaper.compactMap { p in
            let pr = Self.problems(p.meta)
            return pr.isEmpty ? nil : DetailIssue(paper: p, problems: pr)
        }
    }

    nonisolated static func titleKey(_ t: String) -> String {
        String(t.lowercased().unicodeScalars.filter { CharacterSet.alphanumerics.contains($0) }.map(Character.init).prefix(70))
    }

    nonisolated static func doiKey(_ d: String) -> String {
        var s = d.lowercased().trimmingCharacters(in: .whitespaces)
        for p in ["https://doi.org/", "http://doi.org/", "https://dx.doi.org/", "doi:"] where s.hasPrefix(p) { s.removeFirst(p.count) }
        return s
    }

    func duplicateGroups() -> [DuplicateGroup] {
        let ignored = Set(UserDefaults.standard.stringArray(forKey: "notDuplicates") ?? [])
        let papers = everyPaper
        var seen = Set<String>(), out: [DuplicateGroup] = []
        func add(_ ps: [Paper], _ reason: String) {
            let g = DuplicateGroup(papers: ps, reason: reason)
            guard ps.count > 1, !ignored.contains(g.id), !ps.contains(where: { seen.contains($0.id) }) else { return }
            ps.forEach { seen.insert($0.id) }
            out.append(g)
        }
        for (_, ps) in Dictionary(grouping: papers.filter { !$0.meta.doi.isEmpty }, by: { Self.doiKey($0.meta.doi) }) where ps.count > 1 {
            add(ps, "same DOI")
        }
        for (k, ps) in Dictionary(grouping: papers, by: { Self.titleKey($0.meta.title) }) where ps.count > 1 && k.count >= 20 {
            // same title, and no conflicting DOIs or years
            let dois = Set(ps.map { Self.doiKey($0.meta.doi) }.filter { !$0.isEmpty })
            let years = Set(ps.map(\.meta.year).filter { !$0.isEmpty })
            if dois.count <= 1 && years.count <= 1 { add(ps, "same title") }
        }
        return out
    }

    func ignoreDuplicates(_ g: DuplicateGroup) {
        var list = UserDefaults.standard.stringArray(forKey: "notDuplicates") ?? []
        list.append(g.id)
        UserDefaults.standard.set(list, forKey: "notDuplicates")
    }

    /// Tries to fix a paper's details: from its DOI online, otherwise from its PDF on the device
    /// (and then from a DOI found there). Returns a short note on what happened.
    func repairDetails(_ id: String) async -> String {
        guard let p = paper(id) else { return "Paper not found." }
        var meta = p.meta
        var how: [String] = []
        if meta.doi.isEmpty || !Self.problems(meta).isEmpty, p.hasPdf,
           let url = await ensureLocal(id, .pdf), let data = try? Data(contentsOf: url),
           let x = await PDFDetails.extract(fromPDF: data) {
            let bad = Set(Self.problems(meta))
            if !x.title.isEmpty, bad.contains("no title") || bad.contains("title looks wrong") || bad.contains("title is the journal name") || bad.contains("title is a file name") {
                meta.title = x.title
            }
            if !x.authors.isEmpty, bad.contains("no authors") || bad.contains("authors look wrong") { meta.authors = x.authors }
            if meta.journal.isEmpty { meta.journal = x.journal }
            if meta.date.isEmpty { meta.date = x.year }
            if meta.doi.isEmpty { meta.doi = x.doi }
            how.append("read from the PDF")
        }
        let doi = meta.doi.trimmingCharacters(in: .whitespaces)
        if !doi.isEmpty, let r = await Task.detached(operation: { try? resolveMetadata(input: doi) }).value, !r.title.isEmpty {
            var fixed = r
            fixed.fillEmpty(from: meta)
            meta = fixed
            how.append("looked up from the DOI")
        }
        guard meta != p.meta else { return how.isEmpty ? "No DOI or readable PDF to fix it from. Edit it by hand." : "Nothing better found." }
        updateMeta(id, meta)
        let left = Self.problems(meta)
        return "Fixed (\(how.joined(separator: ", ")))" + (left.isEmpty ? "." : "; still \(left.joined(separator: ", ")).")
    }

    /// Folds the other papers of a group into `keep`: details, tags, folders, stars, read state,
    /// highlights and a PDF it lacks. Then deletes the others.
    func merge(_ group: DuplicateGroup, keep: String) async {
        guard var keeper = paper(keep) else { return }
        var meta = keeper.meta
        for other in group.papers where other.id != keep {
            meta.fillEmpty(from: other.meta)
            for t in other.tags { try? lib.setTag(id: keep, tag: t, present: true) }
            for c in other.collections { try? lib.setMembership(collection: c, paper: keep, present: true) }
            if other.starred && !keeper.starred { try? lib.setStarred(id: keep, starred: true) }
            if other.read && !keeper.read { try? lib.setRead(id: keep, read: true) }
            for a in lib.annotations(paper: other.id) {
                var copy = a
                copy.id = ""
                copy.paperId = keep
                _ = try? lib.saveAnnotation(a: copy)
            }
            if !keeper.hasPdf, other.hasPdf, let u = await ensureLocal(other.id, .pdf), let data = try? Data(contentsOf: u) {
                try? lib.attachFile(id: keep, kind: .pdf, data: data)
            }
            if !keeper.hasEpub, other.hasEpub, let u = await ensureLocal(other.id, .epub), let data = try? Data(contentsOf: u) {
                try? lib.attachFile(id: keep, kind: .epub, data: data)
            }
            try? lib.deletePaper(id: other.id)
            keeper = paper(keep) ?? keeper
        }
        if meta != keeper.meta { try? lib.updateMeta(id: keep, meta: meta) }
        reload()
    }

    /// Papers whose text runs words together (PDFs that position words without space characters,
    /// converted before Margins could tell). Remaking their reading view fixes them.
    func runTogetherPapers() async -> [Paper] {
        let papers = everyPaper.filter { $0.hasPdf && $0.hasEpub }
        let lib = self.lib
        let ids = await Task.detached { () -> Set<String> in
            var out = Set<String>()
            for p in papers {
                let path = lib.expectedFilePath(id: p.id, kind: .fulltext)
                guard CloudLibrary.isDownloaded(URL(fileURLWithPath: path)),
                      let t = try? String(contentsOfFile: path, encoding: .utf8) else { continue }
                var words = 0, long = 0
                for w in t.split(whereSeparator: { !$0.isLetter }) {
                    words += 1
                    if w.count >= 22, w.contains(where: \.isLowercase) { long += 1 }
                }
                if words > 300, long * 1000 > words * 8 { out.insert(p.id) }
            }
            return out
        }.value
        return papers.filter { ids.contains($0.id) }
    }

    /// The most complete paper of a group (files, highlights, details).
    func bestOf(_ g: DuplicateGroup) -> String {
        g.papers.max { score($0) < score($1) }?.id ?? g.papers[0].id
    }

    private func score(_ p: Paper) -> Int {
        (p.hasPdf ? 8 : 0) + (p.hasEpub ? 4 : 0) + Int(p.annotationCount) * 2 + p.tags.count + p.collections.count
            + (p.meta.abstractText.isEmpty ? 0 : 2) + (p.meta.doi.isEmpty ? 0 : 2) - Self.problems(p.meta).count * 3
    }
}

// MARK: - The cleanup screen

struct CleanupView: View {
    @EnvironmentObject var model: LibraryModel
    @State private var issues: [DetailIssue] = []
    @State private var groups: [DuplicateGroup] = []
    @State private var keep: [String: String] = [:]
    @State private var notes: [String: String] = [:]
    @State private var working: Set<String> = []
    @State private var fixingAll = false
    @State private var editing: EditID?
    @State private var spacing: [Paper] = []
    @State private var remaking = false
    @State private var remade = 0

    struct EditID: Identifiable { let id: String }

    var body: some View {
        List {
            Section {
                if groups.isEmpty { Text("No duplicates found.").foregroundStyle(.secondary) }
                ForEach(groups) { g in duplicateRow(g) }
            } header: {
                Text("Possible duplicates (\(groups.count))")
            } footer: {
                Text("Merging keeps one paper and moves tags, folders, highlights, stars and a missing PDF onto it.")
            }
            if !spacing.isEmpty || remaking {
                Section {
                    ForEach(spacing, id: \.id) { p in
                        HStack {
                            Text(p.meta.title.isEmpty ? "Untitled" : p.meta.title).lineLimit(2)
                            Spacer()
                            if working.contains(p.id) { ProgressView().controlSize(.small) }
                        }
                        .font(.callout)
                    }
                } header: {
                    HStack {
                        Text("Words run together (\(spacing.count))")
                        Spacer()
                        Button(remaking ? "Remaking \(remade)/\(remade + spacing.count)…" : "Remake Reading Views") { remakeAll() }
                            .disabled(remaking).font(.callout)
                    }
                } footer: {
                    Text("These were converted from PDFs that leave out the spaces between words. Margins now puts them back; remaking the reading view fixes the text, search and narration.")
                }
            }
            Section {
                if issues.isEmpty { Text("All papers have a title, authors and a year.").foregroundStyle(.secondary) }
                ForEach(issues) { i in issueRow(i) }
            } header: {
                HStack {
                    Text("Details that look wrong (\(issues.count))")
                    Spacer()
                    if !issues.isEmpty {
                        Button(fixingAll ? "Fixing…" : "Fix All") { fixAll() }.disabled(fixingAll)
                            .font(.callout)
                    }
                }
            } footer: {
                Text("Fix looks the paper up by its DOI, or reads the title and authors from the PDF on this device (nothing is sent anywhere unless the paper has a DOI).")
            }
        }
        .navigationTitle("Clean Up Library")
        .task {
            refresh()
            spacing = await model.runTogetherPapers()
        }
        .sheet(item: $editing, onDismiss: refresh) { e in MetadataEditor(paperID: e.id).environmentObject(model) }
        #if os(macOS)
        .frame(minWidth: 640, minHeight: 560)
        #endif
    }

    private func refresh() {
        issues = model.detailIssues()
        groups = model.duplicateGroups()
        for g in groups where keep[g.id] == nil { keep[g.id] = model.bestOf(g) }
    }

    private func line(_ p: Paper) -> String {
        [p.meta.authorLine, p.meta.year, p.meta.journal].filter { !$0.isEmpty }.joined(separator: " · ")
    }

    private func duplicateRow(_ g: DuplicateGroup) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(g.reason.capitalized).font(.caption.weight(.semibold)).foregroundStyle(.secondary)
            ForEach(g.papers, id: \.id) { p in
                Button { keep[g.id] = p.id } label: {
                    HStack(alignment: .top, spacing: 8) {
                        Image(systemName: keep[g.id] == p.id ? "largecircle.fill.circle" : "circle")
                            .foregroundStyle(Color.marginsTeal)
                        VStack(alignment: .leading, spacing: 2) {
                            Text(p.meta.title.isEmpty ? "Untitled" : p.meta.title).lineLimit(2)
                            Text(line(p)).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                            HStack(spacing: 4) {
                                if p.hasPdf { Pill(text: "PDF", color: .marginsCopper) }
                                if p.hasEpub { Pill(text: "Read", icon: "book", color: .marginsTeal) }
                                if p.annotationCount > 0 { Pill(text: "\(p.annotationCount)", icon: "highlighter", color: .secondary) }
                            }
                        }
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
            }
            HStack {
                Button(working.contains(g.id) ? "Merging…" : "Merge, Keep Selected") {
                    guard let k = keep[g.id] else { return }
                    working.insert(g.id)
                    Task { await model.merge(g, keep: k); working.remove(g.id); refresh() }
                }
                .buttonStyle(.borderedProminent).tint(.marginsTeal).disabled(working.contains(g.id))
                Button("Not Duplicates") { model.ignoreDuplicates(g); refresh() }.buttonStyle(.bordered)
            }
            .font(.callout)
        }
        .padding(.vertical, 4)
    }

    private func issueRow(_ i: DetailIssue) -> some View {
        HStack(alignment: .top, spacing: 10) {
            VStack(alignment: .leading, spacing: 3) {
                Text(i.paper.meta.title.isEmpty ? "Untitled" : i.paper.meta.title).lineLimit(2)
                Text(line(i.paper)).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                Text(notes[i.id] ?? i.problems.joined(separator: ", ").capitalizedFirst)
                    .font(.caption).foregroundStyle(notes[i.id] == nil ? Color.marginsCopper : .secondary)
            }
            Spacer()
            if working.contains(i.id) {
                ProgressView().controlSize(.small)
            } else {
                Button("Fix") { fix(i.id) }.buttonStyle(.bordered)
                Button("Edit…") { editing = EditID(id: i.id) }.buttonStyle(.borderless)
            }
        }
        .font(.callout)
        .padding(.vertical, 2)
    }

    private func fix(_ id: String) {
        working.insert(id)
        Task {
            notes[id] = await model.repairDetails(id)
            working.remove(id)
            let fresh = model.detailIssues()
            // keep rows that still need work; drop fixed ones after a moment so the note is seen
            try? await Task.sleep(for: .seconds(1.2))
            issues = fresh
        }
    }

    private func remakeAll() {
        remaking = true
        remade = 0
        Task {
            while let p = spacing.first {
                working.insert(p.id)
                _ = await model.makeReadingView(p.id)
                working.remove(p.id)
                spacing.removeFirst()
                remade += 1
            }
            remaking = false
            model.reload()
        }
    }

    private func fixAll() {
        fixingAll = true
        Task {
            for i in issues {
                working.insert(i.id)
                notes[i.id] = await model.repairDetails(i.id)
                working.remove(i.id)
            }
            fixingAll = false
            issues = model.detailIssues()
        }
    }
}

extension String {
    var capitalizedFirst: String { prefix(1).uppercased() + dropFirst() }
}
