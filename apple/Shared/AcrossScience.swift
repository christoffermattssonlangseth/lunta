import SwiftUI
import LuntaCore

// "Across science" in New for You: each morning the standout new research paper (or two) in each
// broad field of science, outside your own topics. The core does the work
// (core/src/foryou/across.rs, `AcrossScience`: one OpenAlex request for the top journals' new
// articles, research papers only, OpenAlex's field for each, your own topics left out, each
// field's paper picked by impact) and keeps it in the library's across-science.json, shared with
// every device. Here: when to look, the digest with Apple's model (one sentence a field; the core
// writes the prompt and checks every sentence against the paper it cites), and the views.

@MainActor
final class AcrossCenter: ObservableObject {
    static let shared = AcrossCenter()
    @Published private(set) var view: AcrossView?
    @Published private(set) var fields: [FieldChoice] = []
    @Published private(set) var running = false
    @Published private(set) var writing = false
    private var core: AcrossScience?
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
            Task { @MainActor in await AcrossCenter.shared.runIfDue() }
        }
    }

    /// The core's handle on the open library (made again when another library opens).
    private func handle() -> AcrossScience? {
        guard let model else { return nil }
        if core == nil || coreFor != model.libraryURL {
            let local = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
                .appendingPathComponent("Margins/ForYou", isDirectory: true)
            try? FileManager.default.createDirectory(at: local, withIntermediateDirectories: true)
            CloudLibrary.startDownload(model.libraryURL.appendingPathComponent("across-science.json"))
            core = AcrossScience(library: model.lib, libraryDir: model.libraryURL.path, localDir: local.path)
            coreFor = model.libraryURL
        }
        return core
    }

    func reload() {
        guard let h = handle() else { return }
        view = h.view(nowMs: Self.nowMs, tzMin: Self.tz)
        fields = h.fields()
    }

    /// At start and every ten minutes: today's papers when they are due (a phone leaves the first
    /// two hours to a computer), else the digest if it is missing.
    func runIfDue() async {
        guard let h = handle() else { return }
        #if os(iOS)
        let phone = true
        #else
        let phone = false
        #endif
        if h.isDue(nowMs: Self.nowMs, tzMin: Self.tz, phone: phone) { await run() } else { reload(); await writeDigest() }
    }

    /// Looks for today's papers now (one OpenAlex request, seconds), or picks again from today's.
    func run() async {
        guard let h = handle(), !running else { return }
        running = true
        let now = Self.nowMs, tz = Self.tz
        _ = try? await Task.detached { try h.run(nowMs: now, tzMin: tz) }.value
        running = false
        reload()
        await writeDigest()
    }

    /// One sentence a field by the on-device model; the core keeps only the sentences that cite a
    /// paper of the list and say what its title and abstract say.
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

    // MARK: settings

    func setOn(_ on: Bool) {
        guard let h = handle() else { return }
        try? h.setOn(on: on)
        reload()
        if on { Task { await run() } }
    }

    func setField(_ id: String, on: Bool) {
        guard let h = handle() else { return }
        try? h.setField(id: id, on: on)
        reload()
        Task { await run() }
    }

    func setPerField(_ n: UInt32) {
        guard let h = handle() else { return }
        try? h.setPerField(n: n)
        reload()
        Task { await run() }
    }

    // MARK: feedback

    func dismiss(_ p: AcrossPaper) {
        try? handle()?.dismiss(key: p.key)
        reload()
    }

    /// A paper (from here or its detail) was added to the library.
    func added(key: String, id: String) {
        guard view?.papers.contains(where: { $0.key == key }) == true else { return }
        try? handle()?.added(key: key, id: id)
        reload()
    }

    func paper(_ key: String) -> AcrossPaper? { view?.papers.first { $0.key == key } }

    nonisolated static func key(from url: URL) -> String? {
        guard url.scheme == "foryou", url.host == "across" else { return nil }
        return URLComponents(url: url, resolvingAgainstBaseURL: false)?.queryItems?.first { $0.name == "key" }?.value
    }
}

extension AcrossPaper: Identifiable {
    public var id: String { "as:" + key }

    /// As a search result, for the paper's detail and for adding it.
    var hit: DiscoverHit {
        DiscoverHit(meta: PaperMeta(title: title, authors: authors, date: date, journal: journal, volume: "", issue: "",
                                    pages: "", publisher: "", doi: doi, pmid: "", pmcid: "", arxiv: "", url: link,
                                    abstractText: abstractText, keywords: [], license: ""),
                    citedBy: 0, openAccess: false, inLibrary: inLibrary)
    }
}

/// "Across science" in the New for You list.
struct AcrossScienceSection: View {
    @EnvironmentObject var model: LibraryModel
    @ObservedObject private var center = AcrossCenter.shared
    @Binding var selection: String?
    /// iPhone and iPad: push a paper's detail
    var push: (AcrossPaper) -> Void = { _ in }

    var body: some View {
        if let v = center.view {
            if !v.on {
                Section { intro } header: { header(nil) }
            } else {
                Section {
                    if let p = v.problem {
                        VStack(alignment: .leading, spacing: 6) {
                            Label("Couldn’t get today’s papers across science", systemImage: "exclamationmark.triangle")
                                .font(.subheadline.weight(.semibold)).foregroundStyle(.orange)
                            Text(p).font(.callout).foregroundStyle(.secondary)
                            Button { Task { await center.run() } } label: { Label("Retry", systemImage: "arrow.clockwise") }
                                .buttonStyle(.bordered).tint(.luntaTeal)
                        }
                        .padding(.vertical, 4)
                    }
                    if !v.digest.isEmpty || center.writing { digest(v) }
                    if v.papers.isEmpty, v.problem == nil {
                        Text(center.running || v.lastRun == nil ? "Looking for today’s papers…"
                                                                 : "No new papers in these fields today.")
                            .font(.callout).foregroundStyle(.secondary)
                    }
                    ForEach(v.papers) { p in row(p).tag(p.id) }
                } header: { header(v) }
            }
        }
    }

    private func header(_ v: AcrossView?) -> some View {
        HStack {
            VStack(alignment: .leading, spacing: 1) {
                Text("Across science")
                if let v, !v.subtitle.isEmpty {
                    Text(v.subtitle).font(.caption).foregroundStyle(.secondary).textCase(nil)
                }
            }
            Spacer()
            if center.running { ProgressView().controlSize(.small) }
            else if v?.on == true {
                Button { Task { await center.run() } } label: { Image(systemName: "arrow.clockwise") }
                    .buttonStyle(.borderless).help("Look for today’s papers")
            }
        }
    }

    private var intro: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Every morning, the standout new paper in each field of science, outside your own: from cell biology and medicine to physics, mathematics and the social sciences, picked from the top journals.")
                .font(.callout).foregroundStyle(.secondary)
            Button { center.setOn(true) } label: { Label("Show Across Science", systemImage: "globe") }
                .buttonStyle(.borderedProminent).tint(.luntaTeal).disabled(center.running)
        }
        .padding(.vertical, 4)
    }

    private func digest(_ v: AcrossView) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Label("Today", systemImage: "text.alignleft")
                    .font(.caption.weight(.semibold)).foregroundStyle(Color.luntaTeal).textCase(.uppercase)
                Spacer()
                if !v.digestShare.isEmpty {
                    ShareLink(item: v.digestShare, subject: Text("Across science")) { Image(systemName: "square.and.arrow.up") }
                        .buttonStyle(.borderless).tint(.luntaTeal).help("Share the summary and its papers")
                }
            }
            if v.digest.isEmpty {
                HStack(spacing: 6) { ProgressView().controlSize(.small); Text("Summarizing the papers…").font(.callout).foregroundStyle(.secondary) }
            } else {
                // The field above its sentence (a small heading), not beside it: beside it, a long field
                // name ("Materials, energy & engineering") left the sentence a narrow column.
                ForEach(Array(v.digest.enumerated()), id: \.offset) { _, d in
                    VStack(alignment: .leading, spacing: 2) {
                        Text(d.fieldLabel.uppercased())
                            .font(.caption2.weight(.semibold)).foregroundStyle(.secondary)
                        HStack(alignment: .firstTextBaseline, spacing: 6) {
                            Text(d.text.trimmingCharacters(in: CharacterSet(charactersIn: ". ")))
                                .font(.subheadline)
                                .frame(maxWidth: .infinity, alignment: .leading)
                            ForEach(d.cites, id: \.key) { c in
                                Button("\(c.number)") { open(c.key) }.buttonStyle(.borderless).tint(.luntaTeal)
                                    .font(.subheadline.weight(.medium))
                            }
                        }
                    }
                    .padding(.vertical, 3)
                }
            }
        }
        .padding(.vertical, 4)
    }

    private func open(_ key: String) {
        guard let p = center.paper(key) else { return }
        #if os(iOS)
        push(p)
        #else
        selection = p.id
        #endif
    }

    @ViewBuilder private func row(_ p: AcrossPaper) -> some View {
        #if os(iOS)
        NavigationLink { DiscoverDetail(hit: p.hit) } label: { AcrossRow(paper: p) }
            .swipeActions { Button("Not for Me", systemImage: "hand.thumbsdown") { center.dismiss(p) }.tint(.gray) }
        #else
        AcrossRow(paper: p)
            .contextMenu { Button("Not for Me", systemImage: "hand.thumbsdown") { center.dismiss(p) } }
        #endif
    }
}

/// One paper: its field, number (what the summary cites), title, journal, one line from the
/// abstract, Add / Not for Me.
struct AcrossRow: View {
    @EnvironmentObject var model: LibraryModel
    @ObservedObject private var center = AcrossCenter.shared
    let paper: AcrossPaper
    @State private var adding = false

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(paper.fieldLabel.uppercased()).font(.caption2.weight(.semibold)).foregroundStyle(Color.luntaTeal)
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
                .help("Not for me: hides it (your feed isn't affected)")
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

/// The settings: on or off, 1 or 2 papers a field, and which fields.
struct AcrossScienceSettings: View {
    @ObservedObject private var center = AcrossCenter.shared

    var body: some View {
        Section {
            Toggle("Show Across science", isOn: Binding(get: { center.view?.on ?? false }, set: { center.setOn($0) }))
            if center.view?.on == true {
                Picker("Papers a field", selection: Binding(get: { center.view?.perField ?? 1 }, set: { center.setPerField($0) })) {
                    Text("1").tag(UInt32(1))
                    Text("2").tag(UInt32(2))
                }
                .pickerStyle(.segmented)
                ForEach(center.fields, id: \.id) { f in
                    Toggle(f.label, isOn: Binding(get: { f.on }, set: { center.setField(f.id, on: $0) }))
                }
            }
        } header: { Text("Across science") } footer: {
            Text("Each morning the standout new research paper in each field, from the top journals (by journal and early citations), leaving out your own topics: they are in your feed. One OpenAlex request a day. Not for Me here doesn’t change your feed.")
        }
        .onAppear { center.reload() }
    }
}
