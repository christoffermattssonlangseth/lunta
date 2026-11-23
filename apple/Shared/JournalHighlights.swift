import SwiftUI
import LuntaCore

// "From the journals you follow" in New for You: the latest research papers in journals you
// choose (Nature, Science, Cell, …), whatever their topic, every Monday morning (or every day),
// with a few sentences on the notable ones by the on-device model. The core does the work
// (core/src/foryou/journals.rs, `JournalHighlights`: Crossref by ISSN, Europe PMC for abstracts,
// research papers only, ranked by your interests) and keeps it in the library's
// journal-highlights.json, shared with every device. Here: when to look, the digest with Apple's
// model (the core writes the prompt and checks every sentence of the answer against the cited
// papers' titles and abstracts), and the views.

@MainActor
final class HighlightsCenter: ObservableObject {
    static let shared = HighlightsCenter()
    @Published private(set) var view: HighlightsView?
    @Published private(set) var choices: [JournalChoice] = []
    @Published private(set) var running = false
    @Published private(set) var writing = false
    private var core: JournalHighlights?
    private var coreFor: URL?
    private weak var model: LibraryModel?
    private var timer: Timer?
    /// the digest last asked for (once per set of papers and run of the app)
    private var digestTried: String?

    private static var tz: Int32 { Int32(TimeZone.current.secondsFromGMT() / 60) }
    private static var nowMs: Int64 { Int64(Date().timeIntervalSince1970 * 1000) }

    func start(_ model: LibraryModel) {
        self.model = model
        reload()
        Task { await runIfDue() }
        timer?.invalidate()
        timer = Timer.scheduledTimer(withTimeInterval: 10 * 60, repeats: true) { _ in
            Task { @MainActor in await HighlightsCenter.shared.runIfDue() }
        }
    }

    /// The core's handle on the open library (made again when another library opens).
    private func handle() -> JournalHighlights? {
        guard let model else { return nil }
        if core == nil || coreFor != model.libraryURL {
            let local = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
                .appendingPathComponent("Margins/ForYou", isDirectory: true)
            try? FileManager.default.createDirectory(at: local, withIntermediateDirectories: true)
            CloudLibrary.startDownload(model.libraryURL.appendingPathComponent("journal-highlights.json"))
            core = JournalHighlights(library: model.lib, libraryDir: model.libraryURL.path, localDir: local.path)
            coreFor = model.libraryURL
        }
        return core
    }

    func reload() {
        guard let h = handle() else { return }
        view = h.view(nowMs: Self.nowMs, tzMin: Self.tz)
        choices = h.choices()
    }

    /// At start and every ten minutes: the period's papers when they are due (a phone leaves the
    /// first two hours to a computer), else the digest if it is missing.
    func runIfDue() async {
        guard let h = handle() else { return }
        #if os(iOS)
        let phone = true
        #else
        let phone = false
        #endif
        if h.isDue(nowMs: Self.nowMs, tzMin: Self.tz, phone: phone) { await run() } else { reload(); await writeDigest() }
    }

    /// Looks for the period's papers now (seconds: Crossref and Europe PMC).
    func run() async {
        guard let h = handle(), !running else { return }
        running = true
        let now = Self.nowMs, tz = Self.tz
        _ = try? await Task.detached { try h.run(nowMs: now, tzMin: tz) }.value
        running = false
        reload()
        await writeDigest()
    }

    /// A few sentences on the notable papers by the on-device model; the core keeps only the
    /// sentences that cite papers of the list and say what their titles and abstracts say.
    func writeDigest() async {
        guard let h = handle(), !writing, let req = h.digestRequest(), digestTried != req.key else { return }
        let engine = AskEngine(scope: .paper("__for-you__"))
        guard engine.unavailableReason == nil else { return }
        digestTried = req.key
        writing = true
        defer { writing = false }
        guard let answer = await engine.plainAnswer(req.prompt, instructions: req.instructions) else { return }
        _ = try? h.setDigest(key: req.key, answer: answer)
        reload()
    }

    // MARK: choosing journals

    /// In the highlights or out (a journal not in Lunta's list is looked up on Crossref). The
    /// problem to show, if any.
    func setJournal(_ name: String, on: Bool) async -> String? {
        guard let h = handle() else { return nil }
        do {
            try await Task.detached { try h.setJournal(name: name, on: on) }.value
        } catch {
            return error.readable
        }
        reload()
        Task { await run() }
        return nil
    }

    func turnOnDefaults() async {
        guard let h = handle() else { return }
        running = true
        _ = try? await Task.detached { try h.turnOnDefaults() }.value
        running = false
        reload()
        await run()
    }

    func setCadence(_ cadence: String) {
        guard let h = handle() else { return }
        try? h.setCadence(cadence: cadence)
        reload()
        Task { await runIfDue() }
    }

    func isOn(_ journal: String) -> Bool {
        choices.contains { $0.on && $0.name.caseInsensitiveCompare(journal) == .orderedSame }
            || (view?.journals ?? []).contains { $0.caseInsensitiveCompare(journal) == .orderedSame }
    }

    // MARK: feedback

    func dismiss(_ p: HighlightPaper) {
        try? handle()?.dismiss(key: p.key)
        reload()
    }

    /// A paper (from here or its detail) was added to the library.
    func added(key: String, id: String) {
        guard view?.papers.contains(where: { $0.key == key }) == true else { return }
        try? handle()?.added(key: key, id: id)
        reload()
    }

    func paper(_ key: String) -> HighlightPaper? { view?.papers.first { $0.key == key } }

    nonisolated static func key(from url: URL) -> String? {
        guard url.scheme == "foryou", url.host == "highlight" else { return nil }
        return URLComponents(url: url, resolvingAgainstBaseURL: false)?.queryItems?.first { $0.name == "key" }?.value
    }
}

extension HighlightPaper: Identifiable {
    public var id: String { "hl:" + key }

    /// As a search result, for the paper's detail and for adding it.
    var hit: DiscoverHit {
        DiscoverHit(meta: PaperMeta(title: title, authors: authors, date: date, journal: journal, volume: "", issue: "",
                                    pages: "", publisher: "", doi: doi, pmid: "", pmcid: "", arxiv: "", url: link,
                                    abstractText: abstractText, keywords: [], license: ""),
                    citedBy: 0, openAccess: false, inLibrary: inLibrary)
    }
}

/// "From the journals you follow" in the New for You list.
struct JournalHighlightsSection: View {
    @EnvironmentObject var model: LibraryModel
    @ObservedObject private var center = HighlightsCenter.shared
    @Binding var selection: String?
    /// iPhone and iPad: push a paper's detail
    var push: (HighlightPaper) -> Void = { _ in }
    var openSettings: () -> Void = {}
    @State private var showAll = false

    var body: some View {
        if let v = center.view {
            if !v.on {
                Section { intro } header: { header(nil) }
            } else {
                Section {
                    if let p = v.problem {
                        VStack(alignment: .leading, spacing: 6) {
                            Label("Couldn’t get the journals’ papers", systemImage: "exclamationmark.triangle")
                                .font(.subheadline.weight(.semibold)).foregroundStyle(.orange)
                            Text(p).font(.callout).foregroundStyle(.secondary)
                            Button { Task { await center.run() } } label: { Label("Retry", systemImage: "arrow.clockwise") }
                                .buttonStyle(.bordered).tint(.luntaTeal)
                        }
                        .padding(.vertical, 4)
                    }
                    if !v.digest.isEmpty || center.writing { digest(v) }
                    if v.papers.isEmpty, v.problem == nil {
                        Text(center.running || v.lastRun == nil ? "Looking for the latest papers…"
                                                                 : "No research papers in these journals since the last look.")
                            .font(.callout).foregroundStyle(.secondary)
                    }
                    ForEach(showAll ? v.papers : Array(v.papers.prefix(10))) { p in row(p).tag(p.id) }
                    if v.papers.count > 10 {
                        Button(showAll ? "Fewer" : "All \(v.papers.count) Papers") { withAnimation { showAll.toggle() } }
                            .buttonStyle(.borderless).tint(.luntaTeal)
                    }
                } header: { header(v) }
            }
        }
    }

    private func header(_ v: HighlightsView?) -> some View {
        HStack {
            VStack(alignment: .leading, spacing: 1) {
                Text("From the journals you follow")
                if let v, !v.subtitle.isEmpty {
                    Text(v.subtitle).font(.caption).foregroundStyle(.secondary).textCase(nil)
                }
            }
            Spacer()
            if center.running { ProgressView().controlSize(.small) }
            else if v?.on == true {
                Button { Task { await center.run() } } label: { Image(systemName: "arrow.clockwise") }
                    .buttonStyle(.borderless).help("Look for the latest papers now")
            }
        }
    }

    private var intro: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Every Monday, the week’s research papers in journals you choose, whatever their topic, with a few sentences on the notable ones.")
                .font(.callout).foregroundStyle(.secondary)
            HStack {
                Button { Task { await center.turnOnDefaults() } } label: { Label("Nature, Science and Cell", systemImage: "newspaper") }
                    .buttonStyle(.borderedProminent).tint(.luntaTeal).disabled(center.running)
                Button("Choose Journals…", action: openSettings).buttonStyle(.borderless).tint(.luntaTeal)
            }
        }
        .padding(.vertical, 4)
    }

    private func digest(_ v: HighlightsView) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Label(v.cadence == "daily" ? "Since yesterday" : "This week", systemImage: "text.alignleft")
                    .font(.caption.weight(.semibold)).foregroundStyle(Color.luntaTeal).textCase(.uppercase)
                Spacer()
                if !v.digestShare.isEmpty {
                    ShareLink(item: v.digestShare, subject: Text("From the journals you follow")) { Image(systemName: "square.and.arrow.up") }
                        .buttonStyle(.borderless).tint(.luntaTeal).help("Share the summary and its papers")
                }
            }
            if v.digest.isEmpty {
                HStack(spacing: 6) { ProgressView().controlSize(.small); Text("Summarizing the papers…").font(.callout).foregroundStyle(.secondary) }
            } else {
                Text((try? AttributedString(markdown: v.digestMarkdown, options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace)))
                     ?? AttributedString(v.digestMarkdown))
                    .font(.callout).tint(.luntaTeal)
                    .environment(\.openURL, OpenURLAction { url in
                        guard let key = HighlightsCenter.key(from: url), let p = center.paper(key) else { return .systemAction }
                        #if os(iOS)
                        push(p)
                        #else
                        selection = p.id
                        #endif
                        return .handled
                    })
            }
        }
        .padding(.vertical, 4)
    }

    @ViewBuilder private func row(_ p: HighlightPaper) -> some View {
        #if os(iOS)
        NavigationLink { DiscoverDetail(hit: p.hit) } label: { HighlightRow(paper: p) }
            .swipeActions { Button("Not for Me", systemImage: "hand.thumbsdown") { center.dismiss(p) }.tint(.gray) }
        #else
        HighlightRow(paper: p)
            .contextMenu { Button("Not for Me", systemImage: "hand.thumbsdown") { center.dismiss(p) } }
        #endif
    }
}

/// One paper: its number (what the summary cites), title, journal, one line from the abstract, Add / Not for Me.
struct HighlightRow: View {
    @EnvironmentObject var model: LibraryModel
    @ObservedObject private var center = HighlightsCenter.shared
    let paper: HighlightPaper
    @State private var adding = false

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("\(paper.number). \(paper.title)").font(.headline).lineLimit(3)
            Text(paper.byline).font(.caption).foregroundStyle(.secondary).lineLimit(1)
            if !paper.line.isEmpty { Text(paper.line).font(.subheadline).lineLimit(3) }
            HStack(spacing: 4) {
                if let id = paper.inLibrary ?? model.libraryID(of: paper.hit) {
                    OpenAddedPaper(id: id, compact: true).tint(.luntaTeal).fixedSize()
                } else {
                    Button { add() } label: {
                        HStack(spacing: 4) {
                            if adding { ProgressView().controlSize(.mini).tint(.white) } else { Image(systemName: "plus") }
                            Text(adding ? "Adding…" : "Add")
                        }
                        .font(.subheadline.weight(.semibold)).foregroundStyle(.white)
                        .padding(.horizontal, 14).padding(.vertical, 6)
                        .background(Capsule().fill(Color.luntaTeal))
                        .fixedSize()
                    }
                    .buttonStyle(.borderless).disabled(adding)
                }
                Spacer(minLength: 8)
                Button { center.dismiss(paper) } label: {
                    Image(systemName: "hand.thumbsdown").foregroundStyle(.secondary)
                        .frame(width: 40, height: 32).contentShape(Rectangle())
                }
                .buttonStyle(.borderless)
                .accessibilityLabel("Not for Me")
                .help("Not for me: hides it")
            }
        }
        .padding(.vertical, 6)
    }

    private func add() {
        adding = true
        Task {
            if let id = await model.addDiscovered(paper.hit) { center.added(key: paper.key, id: id) }
            adding = false
        }
    }
}

/// The settings: weekly or daily, and which journals.
struct JournalHighlightsSettings: View {
    @ObservedObject private var center = HighlightsCenter.shared
    @State private var other = ""
    @State private var looking = false
    @State private var problem: String?

    var body: some View {
        Section {
            Picker("Papers come", selection: Binding(get: { center.view?.cadence ?? "weekly" }, set: { center.setCadence($0) })) {
                Text("Every week, on Monday morning").tag("weekly")
                Text("Every morning").tag("daily")
            }
            ForEach(center.choices, id: \.name) { c in
                Toggle(isOn: Binding(get: { c.on }, set: { on in Task { problem = await center.setJournal(c.name, on: on) } })) {
                    HStack(spacing: 6) {
                        Text(c.name)
                        if c.followed { Text("you follow it").font(.caption).foregroundStyle(.secondary) }
                    }
                }
            }
            HStack {
                TextField("Another journal, e.g. Cell Systems", text: $other).autocorrectionDisabled().onSubmit(addOther)
                Button(looking ? "Looking…" : "Add", action: addOther)
                    .buttonStyle(.borderless).tint(.luntaTeal)
                    .disabled(looking || other.trimmingCharacters(in: .whitespaces).isEmpty)
            }
            if let problem { Text(problem).font(.caption).foregroundStyle(.red) }
        } header: { Text("From the journals you follow") } footer: {
            Text("The latest research papers in these journals, whatever their topic (no news, editorials or corrections), with a few sentences on the notable ones when Apple Intelligence is on. Each sentence cites the papers it is about, and says only what their titles and abstracts say.")
        }
        .onAppear { center.reload() }
    }

    private func addOther() {
        let name = other.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !name.isEmpty, !looking else { return }
        looking = true
        problem = nil
        Task {
            problem = await center.setJournal(name, on: true)
            looking = false
            if problem == nil { other = "" }
        }
    }
}
