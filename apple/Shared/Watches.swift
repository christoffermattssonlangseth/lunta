import SwiftUI
import NaturalLanguage
import UserNotifications
import LuntaCore

// New for You: a daily, ranked feed of new papers for your research.
//
//  1. You describe your interests in a paragraph (Lunta can draft it from your library).
//  2. A wide net: OpenAlex searches written from that paragraph, papers citing recent papers in
//     your library, new work by authors you follow, and the last days of bioRxiv and medRxiv.
//  3. Ranking on the device: sentence embeddings score each candidate against your paragraph,
//     your library, and what you added or turned down before.
//  4. The language model reads the best ~20 abstracts, keeps the ones that really fit and says why.
//
// Shared state lives in the library folder, so it syncs: watches.json (authors / papers / topics
// you follow) and foryou.json (the paragraph, the feed, and your feedback). Whichever device runs
// first each day makes the feed; usually the Mac.

struct Watch: Codable, Identifiable, Hashable {
    enum Kind: String, Codable, CaseIterable { case topic, author, citing }
    var id = UUID().uuidString
    var kind: Kind
    var value: String          // words, an author's name, or a DOI
    var label: String          // shown in the app
    var created = Date()
    var lastChecked: Date?
    var seen: [String] = []    // (older versions) keys already shown

    var icon: String {
        switch kind {
        case .topic: return "text.magnifyingglass"
        case .author: return "person"
        case .citing: return "quote.bubble"
        }
    }

    var coreKind: WatchKind {
        switch kind {
        case .topic: return .topic
        case .author: return .author
        case .citing: return .citing
        }
    }

    var describe: String {
        switch kind {
        case .topic: return "Papers on “\(label)”"
        case .author: return "Papers by \(label)"
        case .citing: return "Papers citing “\(label)”"
        }
    }
}

/// A paper in the feed, as stored in foryou.json.
struct FeedRecord: Codable, Hashable {
    var key: String
    var title: String
    var authors: [String]
    var date: String
    var journal: String
    var doi: String
    var url: String
    var abstract: String
    var openAccess: Bool
    var citedBy: Int
    var reason: String
    var source: String
    var score: Double
    var found: Date
    var inLibrary: String?
    var boosted: Bool?              // thumbs up: more like this
    var summary: String?            // one sentence: what the paper does and finds

    init(_ h: DiscoverHit, reason: String, score: Double, source: String) {
        key = LibraryModel.hitKey(h)
        title = h.meta.title; authors = h.meta.authors; date = h.meta.date; journal = h.meta.journal
        doi = h.meta.doi; url = h.meta.url; abstract = String(h.meta.abstractText.prefix(3000))
        openAccess = h.openAccess; citedBy = Int(h.citedBy)
        self.reason = reason; self.source = source; self.score = score
        found = Date(); inLibrary = h.inLibrary
    }

    var hit: DiscoverHit {
        DiscoverHit(meta: PaperMeta(title: title, authors: authors, date: date, journal: journal, volume: "", issue: "",
                                    pages: "", publisher: "", doi: doi, pmid: "", pmcid: "", arxiv: "", url: url,
                                    abstractText: abstract, keywords: [], license: ""),
                    citedBy: UInt32(max(0, citedBy)), openAccess: openAccess, inLibrary: inLibrary)
    }
}

/// A short piece of a paper you added or turned down (to rank similar papers up or down).
struct FeedbackSnip: Codable, Hashable {
    var key: String
    var text: String
}

struct ForYouState: Codable {
    var interests = ""
    var queries: [String] = []
    var keywords: [String] = []
    var queriesFor = ""            // the paragraph the queries were written for
    var perDay = 8
    var items: [FeedRecord] = []
    var shown: [String] = []       // keys already offered (never offered twice)
    var liked: [FeedbackSnip] = []
    var disliked: [FeedbackSnip] = []
    var boosted: [FeedbackSnip] = []   // thumbs up: counts more than an add
    var lastRun: Date?
    var digest = ""                // a few sentences on the latest papers
    var digestFor: String?         // the feed (its paper keys) the digest describes

    init() {}
    init(from d: Decoder) throws {
        let c = try d.container(keyedBy: CodingKeys.self)
        interests = try c.decodeIfPresent(String.self, forKey: .interests) ?? ""
        queries = try c.decodeIfPresent([String].self, forKey: .queries) ?? []
        keywords = try c.decodeIfPresent([String].self, forKey: .keywords) ?? []
        queriesFor = try c.decodeIfPresent(String.self, forKey: .queriesFor) ?? ""
        perDay = try c.decodeIfPresent(Int.self, forKey: .perDay) ?? 8
        items = try c.decodeIfPresent([FeedRecord].self, forKey: .items) ?? []
        shown = try c.decodeIfPresent([String].self, forKey: .shown) ?? []
        liked = try c.decodeIfPresent([FeedbackSnip].self, forKey: .liked) ?? []
        disliked = try c.decodeIfPresent([FeedbackSnip].self, forKey: .disliked) ?? []
        boosted = try c.decodeIfPresent([FeedbackSnip].self, forKey: .boosted) ?? []
        lastRun = try c.decodeIfPresent(Date.self, forKey: .lastRun)
        digest = try c.decodeIfPresent(String.self, forKey: .digest) ?? ""
        digestFor = try? c.decodeIfPresent(String.self, forKey: .digestFor)
    }
}

/// A paper in the feed, ready for the views.
struct WatchFind: Identifiable {
    var record: FeedRecord
    var hit: DiscoverHit
    var id: String { record.key }
    init(_ r: FeedRecord) { record = r; hit = r.hit }
}

@MainActor
final class WatchCenter: ObservableObject {
    static let shared = WatchCenter()
    @Published var watches: [Watch] = []
    @Published private(set) var state = ForYouState()
    @Published var finds: [WatchFind] = []
    @Published var checking = false
    @Published var stage: String? { didSet { if folderOverride != nil, let stage { NSLog("FORYOU %@", stage) } } }
    @Published var lastOpened: Date = (UserDefaults.standard.object(forKey: "forYouOpened") as? Date) ?? .distantPast
    weak var model: LibraryModel?
    private var timer: Timer?
    private var summaryTimer: Timer?
    private var libraryVectors: [String: [Double]] = [:]

    /// Testing: keep the files somewhere else than the library.
    var folderOverride: URL?
    /// What the last run did (for the test report).
    private(set) var lastStats = ""
    private var folder: URL? { folderOverride ?? model?.libraryURL }
    private var watchesURL: URL? { folder?.appendingPathComponent("watches.json") }
    private var stateURL: URL? { folder?.appendingPathComponent("foryou.json") }

    /// Testing: sets the paragraph without starting a run.
    func setInterestsQuietly(_ text: String) { change { $0.interests = text } }

    func start(_ model: LibraryModel) {
        self.model = model
        load()
        Task { await runIfDue() }
        timer?.invalidate()
        timer = Timer.scheduledTimer(withTimeInterval: 3 * 3600, repeats: true) { _ in
            Task { @MainActor in await WatchCenter.shared.runIfDue() }
        }
        #if os(macOS)
        // the Mac writes the summaries: it picks up papers turned down or added on the phone
        summaryTimer?.invalidate()
        summaryTimer = Timer.scheduledTimer(withTimeInterval: 10 * 60, repeats: true) { _ in
            Task { @MainActor in
                WatchCenter.shared.load()
                WatchCenter.shared.summarizeSoon()
            }
        }
        #endif
    }

    // MARK: files

    func load() {
        if let url = watchesURL {
            CloudLibrary.startDownload(url)
            if let d = try? Data(contentsOf: url), let list = try? JSONDecoder.iso.decode([Watch].self, from: d), list != watches {
                watches = list
            }
        }
        if let url = stateURL {
            CloudLibrary.startDownload(url)
            if let d = try? Data(contentsOf: url), let s = try? JSONDecoder.iso.decode(ForYouState.self, from: d) {
                state = s
                rebuild()
            }
        }
    }

    private func saveWatches() {
        guard let url = watchesURL, let d = try? JSONEncoder.iso.encode(watches) else { return }
        try? d.write(to: url, options: .atomic)
    }

    private func saveState() {
        guard let url = stateURL, let d = try? JSONEncoder.iso.encode(state) else { return }
        try? d.write(to: url, options: .atomic)
    }

    /// Re-reads the file, changes the state and writes it back (another device may have written since).
    private func change(_ f: (inout ForYouState) -> Void) {
        load()
        f(&state)
        saveState()
        rebuild()
    }

    private func rebuild() {
        finds = state.items.map(WatchFind.init)
    }

    // MARK: settings

    var interests: String { state.interests }
    var perDay: Int { state.perDay }

    func setInterests(_ text: String, perDay: Int) {
        let t = text.trimmingCharacters(in: .whitespacesAndNewlines)
        let changed = t != state.interests
        change { $0.interests = t; $0.perDay = perDay }
        requestNotifications()
        if changed, !t.isEmpty { Task { await run() } }
    }

    func add(_ w: Watch) {
        load()
        guard !watches.contains(where: { $0.kind == w.kind && $0.value.caseInsensitiveCompare(w.value) == .orderedSame }) else { return }
        watches.append(w)
        saveWatches()
        requestNotifications()
        Task { await run() }
    }

    func remove(_ id: String) {
        load()
        watches.removeAll { $0.id == id }
        saveWatches()
    }

    // MARK: feedback

    /// "Not for me": removes it here and on the other devices, and ranks similar papers lower.
    func dismiss(_ f: WatchFind) {
        change { s in
            s.items.removeAll { $0.key == f.id }
            s.disliked.append(FeedbackSnip(key: f.id, text: Self.snip(f.record.title, f.record.abstract)))
            s.disliked = Array(s.disliked.suffix(150))
        }
    }

    /// Thumbs up ("more like this"): ranks similar papers higher and shows the model what you want.
    /// Tapping again takes it back.
    func boost(_ f: WatchFind) {
        change { s in
            guard let i = s.items.firstIndex(where: { $0.key == f.id }) else { return }
            let on = !(s.items[i].boosted ?? false)
            s.items[i].boosted = on
            s.boosted.removeAll { $0.key == f.id }
            if on {
                s.boosted.append(FeedbackSnip(key: f.id, text: Self.snip(f.record.title, f.record.abstract)))
                s.boosted = Array(s.boosted.suffix(100))
            }
        }
    }

    /// Called when a paper from the feed was added to the library: remembered as a paper you wanted.
    func added(key: String, id: String) {
        guard let r = state.items.first(where: { $0.key == key }) else { return }
        change { s in
            if let i = s.items.firstIndex(where: { $0.key == key }) { s.items[i].inLibrary = id }
            if !s.liked.contains(where: { $0.key == key }) {
                s.liked.append(FeedbackSnip(key: key, text: Self.snip(r.title, r.abstract)))
                s.liked = Array(s.liked.suffix(150))
            }
        }
    }

    func markOpened() {
        lastOpened = Date()
        UserDefaults.standard.set(lastOpened, forKey: "forYouOpened")
    }

    var newCount: Int {
        state.items.filter { $0.inLibrary == nil && $0.found > lastOpened }.count
    }

    nonisolated static func snip(_ title: String, _ abstract: String) -> String {
        title + ". " + String(abstract.prefix(400))
    }

    // MARK: making the feed

    func runIfDue() async {
        load()
        guard !state.interests.isEmpty || !watches.isEmpty else { return }
        // the Mac usually makes the day's feed; the iPhone steps in when it hasn't for a while
        #if os(iOS)
        let due: TimeInterval = 30 * 3600
        #else
        let due: TimeInterval = 20 * 3600
        #endif
        if let last = state.lastRun, Date().timeIntervalSince(last) < due { return }
        await run()
    }

    /// Finds, ranks and screens new papers, and puts the best at the top of the feed.
    func run() async {
        guard let model, !checking else { return }
        load()
        let interests = state.interests
        guard !interests.isEmpty || !watches.isEmpty else { return }
        checking = true
        defer { checking = false; stage = nil }
        let lib = model.lib
        let fmt = DateFormatter()
        fmt.dateFormat = "yyyy-MM-dd"
        fmt.timeZone = TimeZone(identifier: "UTC")
        let first = state.lastRun == nil
        let since = fmt.string(from: Date().addingTimeInterval((first ? -30 : -9) * 86400))

        // 1. searches written from the paragraph
        if !interests.isEmpty, state.queriesFor != interests || state.queries.isEmpty {
            stage = "Reading your interests…"
            let (q, k) = await Self.searches(for: interests)
            change { $0.queries = q; $0.keywords = k; $0.queriesFor = interests }
        }

        // 2. the wide net
        stage = "Looking for new papers…"
        struct Job: Sendable { let kind: WatchKind; let value: String; let source: String; let explicit: Bool; let limit: UInt32 }
        var jobs: [Job] = []
        for q in state.queries { jobs.append(Job(kind: .topic, value: q, source: "search “\(q)”", explicit: false, limit: 40)) }
        for w in watches {
            switch w.kind {
            case .topic: jobs.append(Job(kind: .topic, value: w.value, source: "your topic “\(w.label)”", explicit: false, limit: 25))
            case .author: jobs.append(Job(kind: .author, value: w.value, source: "New paper by \(w.label), whom you follow.", explicit: true, limit: 15))
            case .citing: jobs.append(Job(kind: .citing, value: w.value, source: "Cites “\(w.label)”, which you watch.", explicit: true, limit: 15))
            }
        }
        let every = lib.papers(filter: PaperFilter(collection: nil, tag: nil, starredOnly: false, unreadOnly: false), sort: .added)
        for p in every.filter({ !$0.meta.doi.isEmpty }).prefix(8) {
            jobs.append(Job(kind: .citing, value: p.meta.doi, source: "Cites “\(p.meta.title.prefix(80))” in your library.", explicit: false, limit: 10))
        }
        let keywords = state.keywords
        var pool: [(hit: DiscoverHit, source: String, explicit: Bool)] = []
        await withTaskGroup(of: [(DiscoverHit, String, Bool)].self) { group in
            for j in jobs {
                group.addTask {
                    let hits = (try? lib.watchNew(kind: j.kind, value: j.value, since: since, limit: j.limit)) ?? []
                    return hits.map { ($0, j.source, j.explicit) }
                }
            }
            if !keywords.isEmpty {
                let from = fmt.string(from: Date().addingTimeInterval((first ? -4 : -2) * 86400))
                let to = fmt.string(from: Date())
                for server in ["biorxiv", "medrxiv"] {
                    group.addTask {
                        await Self.preprints(server, from: from, to: to, keywords: keywords)
                            .map { ($0, server == "biorxiv" ? "bioRxiv" : "medRxiv", false) }
                    }
                }
            }
            for await batch in group { pool.append(contentsOf: batch.map { (hit: $0.0, source: $0.1, explicit: $0.2) }) }
        }

        // 3. new to you: not in the library, never offered before
        let offered = Set(state.shown).union(state.items.map(\.key))
        var seenKeys = Set<String>()
        var candidates: [(hit: DiscoverHit, source: String, explicit: Bool)] = []
        for c in pool {
            let key = LibraryModel.hitKey(c.hit)
            guard !key.isEmpty, c.hit.inLibrary == nil, !offered.contains(key), !seenKeys.contains(key),
                  c.hit.meta.title.count > 10 else { continue }
            if !c.hit.meta.doi.isEmpty, lib.findByDoi(doi: c.hit.meta.doi) != nil { continue }
            // papers only: no data or code deposits, theses servers or book series, and an abstract to judge by
            let venue = c.hit.meta.journal.lowercased()
            if ["zenodo", "figshare", "dryad", "osf", "repository", "publikationsserver", "book series", "dissertation", "thesis"]
                .contains(where: venue.contains) { continue }
            if !c.explicit, c.hit.meta.abstractText.count < 300 { continue }
            seenKeys.insert(key)
            candidates.append(c)
        }
        guard !candidates.isEmpty else { change { $0.lastRun = Date() }; return }

        // 4. ranking on the device
        stage = "Ranking \(candidates.count) papers…"
        await refreshLibraryVectors(every)
        let libVecs = Array(libraryVectors.values)
        let liked = state.liked.map(\.text), disliked = state.disliked.map(\.text), boosted = state.boosted.map(\.text)
        let texts = candidates.map { Self.snip($0.hit.meta.title, $0.hit.meta.abstractText) }
        let scores = await Task.detached(priority: .utility) {
            Self.score(texts, interests: interests, library: libVecs, liked: liked, disliked: disliked, boosted: boosted)
        }.value
        lastStats = "pool \(pool.count), new candidates \(candidates.count) (" +
            Dictionary(grouping: candidates, by: { $0.source.hasPrefix("search") ? "searches" : $0.source.hasPrefix("Cites") ? "citing library" : $0.source })
                .map { "\($0.key): \($0.value.count)" }.sorted().joined(separator: ", ") + ")"
        let ranked = zip(candidates, scores).map { (c: $0.0, s: $0.1 + ($0.0.explicit ? 0.15 : 0)) }.sorted { $0.s > $1.s }

        // 5. the language model reads the best abstracts
        var picked: [FeedRecord] = []
        var turnedDown: [String] = []
        for e in ranked.filter(\.c.explicit).prefix(10) {
            picked.append(FeedRecord(e.c.hit, reason: e.c.source, score: e.s, source: e.c.source))
        }
        let open = Array(ranked.filter { !$0.c.explicit }.prefix(40))
        var nearMisses: [(FeedRecord, Double)] = []      // scored 6: used on a quiet day
        let judge = interests.isEmpty ? nil : AskEngine(scope: .paper("__for-you__"))
        let examples = state.items.filter { $0.boosted == true }.map(\.title).suffix(8)
        var kept = 0
        for (i, r) in open.enumerated() where kept < state.perDay {
            stage = "Reading abstracts (\(i + 1) of \(open.count))…"
            if let judge, judge.unavailableReason == nil {
                guard let verdict = await Self.screen(r.c.hit, interests: interests, liked: Array(examples), with: judge) else { continue }
                if verdict.score >= 7 {
                    picked.append(FeedRecord(r.c.hit, reason: verdict.why, score: r.s, source: r.c.source)); kept += 1
                } else {
                    if verdict.score >= 6 { nearMisses.append((FeedRecord(r.c.hit, reason: verdict.why, score: r.s, source: r.c.source), r.s)) }
                    turnedDown.append(LibraryModel.hitKey(r.c.hit))
                }
            } else {
                // no language model here: trust the ranking
                let why = r.c.source.hasPrefix("search") || r.c.source.hasPrefix("your topic") || r.c.source.hasSuffix("Rxiv")
                    ? "Close to your interests (from \(r.c.source))." : r.c.source
                picked.append(FeedRecord(r.c.hit, reason: why, score: r.s, source: r.c.source)); kept += 1
            }
        }

        // a quiet day: the best near misses, up to three papers in all
        for (rec, _) in nearMisses.sorted(by: { $0.1 > $1.1 }).prefix(max(0, 3 - kept)) {
            picked.append(rec); turnedDown.removeAll { $0 == rec.key }
        }
        lastStats += "\nscreened \(open.count): kept \(kept), near misses \(nearMisses.count), turned down \(turnedDown.count)"
        // 6. the feed: the new picks on top, best first
        let fresh = picked.sorted { $0.score > $1.score }
        change { s in
            s.items = fresh + s.items.filter { old in !fresh.contains { $0.key == old.key } }
            s.items = Array(s.items.prefix(80))
            s.shown = Array((s.shown + fresh.map(\.key) + turnedDown).suffix(3000))
            s.lastRun = Date()
        }
        if !fresh.isEmpty { notify(fresh.count) }
        await summarize()
    }

    @Published var summarizing = false

    /// The papers the digest covers: what's in the feed now (best first, at most 15).
    var digestBatch: [FeedRecord] { Array(state.items.prefix(15)) }
    var digestKey: String { "v2|" + digestBatch.map(\.key).joined(separator: "|") }

    /// The link a digest uses for a paper in the feed.
    nonisolated static func feedLink(_ key: String) -> String {
        "foryou://paper?key=" + (key.addingPercentEncoding(withAllowedCharacters: .alphanumerics) ?? key)
    }
    nonisolated static func feedKey(from url: URL) -> String? {
        guard url.scheme == "foryou" else { return nil }
        return URLComponents(url: url, resolvingAgainstBaseURL: false)?.queryItems?.first { $0.name == "key" }?.value
    }

    /// Turns the model's "[short title](3)" references into links to the papers, and links the short
    /// names of papers it mentioned without a link (e.g. "HistoTx" for "HistoTx: Early fusion…").
    nonisolated static func linkDigest(_ text: String, batch: [FeedRecord]) -> String {
        var out = text
        // [name](n) → [name](foryou://…)
        if let re = try? NSRegularExpression(pattern: #"\[([^\]]+)\]\((\d+)\)"#) {
            for m in re.matches(in: out, range: NSRange(out.startIndex..., in: out)).reversed() {
                guard let whole = Range(m.range, in: out), let nameR = Range(m.range(at: 1), in: out),
                      let numR = Range(m.range(at: 2), in: out), let n = Int(out[numR]) else { continue }
                let name = String(out[nameR])
                out.replaceSubrange(whole, with: (1...batch.count).contains(n) ? "[\(name)](\(feedLink(batch[n - 1].key)))" : name)
            }
        }
        // stray "(3)" or "[3]" references
        out = out.replacingOccurrences(of: #"\s?[\(\[](paper\s)?\d{1,2}[\)\]]"#, with: "", options: [.regularExpression, .caseInsensitive])
        // unlinked short names
        for r in batch where !out.contains(feedLink(r.key)) {
            let short = r.title.components(separatedBy: ":").first?.trimmingCharacters(in: .whitespaces) ?? ""
            guard r.title.contains(":"), short.count >= 3, short.count <= 30,
                  let range = out.range(of: short), !isInsideLink(out, range) else { continue }
            out.replaceSubrange(range, with: "[\(short)](\(feedLink(r.key)))")
        }
        return out
    }

    nonisolated private static func isInsideLink(_ s: String, _ r: Range<String.Index>) -> Bool {
        let before = s[..<r.lowerBound]
        return before.filter { $0 == "[" }.count > before.filter { $0 == "]" }.count
            || (before.hasSuffix("(") && before.dropLast().hasSuffix("]"))
    }

    private var summaryJob: Task<Void, Never>?

    /// Starts summarize() unless it's running (not tied to a view, which would cancel it on redraw).
    func summarizeSoon() {
        guard summaryJob == nil, !checking else { return }
        #if os(iOS)
        // the Mac summarizes (after its daily run, and every 10 minutes); the phone only steps in
        // when no Mac has made the feed for a day and a half
        guard (state.lastRun ?? .distantPast) < Date().addingTimeInterval(-36 * 3600)
                || ProcessInfo.processInfo.arguments.contains("-summarizeHere") else { return }
        #endif
        summaryJob = Task { [weak self] in
            await self?.summarize()
            self?.summaryJob = nil
        }
    }

    /// One sentence for each paper without one, then a few sentences on the whole feed. Made by
    /// whichever device gets there first; both sync with the feed.
    func summarize() async {
        guard !summarizing, !state.items.isEmpty else { return }
        let engine = AskEngine(scope: .paper("__for-you__"))
        guard engine.unavailableReason == nil else { return }
        summarizing = true
        defer { summarizing = false }
        for r in state.items.prefix(30) where r.summary == nil && !r.abstract.isEmpty {
            let prompt = """
                Title: \(r.title)
                Abstract: \(String(r.abstract.prefix(1500)))

                In one plain sentence of at most 25 words, say what this paper does and what it finds. \
                Start with the method or finding itself (not "This paper" or "The authors"). No preamble.
                """
            guard let a = await engine.plainAnswer(prompt, instructions: "You summarize scientific papers in one accurate sentence, using only what the abstract says.")?
                .trimmingCharacters(in: .whitespacesAndNewlines.union(CharacterSet(charactersIn: "\"*"))), a.count > 15 else { continue }
            let line = a.components(separatedBy: .newlines).first { !$0.trimmingCharacters(in: .whitespaces).isEmpty } ?? a
            change { s in
                if let i = s.items.firstIndex(where: { $0.key == r.key }) { s.items[i].summary = line }
            }
        }
        // the whole feed in a few sentences
        let batch = digestBatch, key = digestKey
        guard state.digestFor != key, !batch.isEmpty else { return }
        let list = batch.enumerated().map { i, r in
            "\(i + 1). \(r.title)\n   \(r.summary ?? String(r.abstract.prefix(300)))"
        }.joined(separator: "\n")
        let prompt = """
            The researcher's interests:
            \(state.interests)

            The papers in their feed:
            \(list)

            Summarize this feed for the researcher in ONE short paragraph of two or three sentences, \
            at most 70 words: the main themes across the papers, then the one or two most worth their time \
            and why. Each time you mention a paper, write it as a link: a short form of its title in square \
            brackets followed by its number in parentheses, e.g. [scPolyA](2). No list, no heading, no preamble.
            """
        guard let a = await engine.plainAnswer(prompt, instructions: "You write short, concrete research digests. You never invent findings that aren't in the summaries.")?
            .trimmingCharacters(in: .whitespacesAndNewlines), a.count > 20 else { return }
        change { s in
            s.digest = Self.linkDigest(a.replacingOccurrences(of: "**", with: "")
                .components(separatedBy: .newlines).filter { !$0.trimmingCharacters(in: .whitespaces).isEmpty }
                .joined(separator: " "), batch: batch)
            s.digestFor = key
        }
    }

    private func refreshLibraryVectors(_ every: [Paper]) async {
        let todo = every.prefix(400).filter { libraryVectors[$0.id] == nil }
            .map { ($0.id, Self.snip($0.meta.title, $0.meta.abstractText)) }
        guard !todo.isEmpty else { return }
        let made = await Task.detached(priority: .utility) { () -> [(String, [Double])] in
            guard let e = NLEmbedding.sentenceEmbedding(for: .english) else { return [] }
            return todo.compactMap { id, t in e.vector(for: t).map { (id, $0) } }
        }.value
        for (id, v) in made { libraryVectors[id] = v }
    }

    nonisolated static func score(_ texts: [String], interests: String, library: [[Double]],
                                  liked: [String], disliked: [String], boosted: [String] = []) -> [Double] {
        guard let e = NLEmbedding.sentenceEmbedding(for: .english) else { return texts.map { _ in 0 } }
        let p = interests.isEmpty ? nil : e.vector(for: interests)
        let likedV = liked.compactMap { e.vector(for: $0) }
        let dislikedV = disliked.compactMap { e.vector(for: $0) }
        let boostedV = boosted.compactMap { e.vector(for: $0) }
        return texts.map { t in
            guard let v = e.vector(for: t) else { return 0 }
            let toInterests = p.map { cos($0, v) } ?? 0
            let near = library.map { cos($0, v) }.sorted(by: >).prefix(3)
            let toLibrary = near.isEmpty ? 0 : near.reduce(0, +) / Double(near.count)
            let up = likedV.map { cos($0, v) }.max() ?? 0
            let down = dislikedV.map { cos($0, v) }.max() ?? 0
            let base = p == nil ? toLibrary : 0.55 * toInterests + 0.35 * toLibrary
            let boost = boostedV.map { cos($0, v) }.max() ?? 0
            return base + 0.25 * (up - down) + 0.3 * boost
        }
    }

    nonisolated static func cos(_ a: [Double], _ b: [Double]) -> Double {
        var d = 0.0, na = 0.0, nb = 0.0
        for i in 0..<min(a.count, b.count) { d += a[i] * b[i]; na += a[i] * a[i]; nb += b[i] * b[i] }
        return na > 0 && nb > 0 ? d / (na.squareRoot() * nb.squareRoot()) : 0
    }

    // MARK: the language model

    /// Search queries and keywords from the paragraph (falls back to its phrases without a model).
    static func searches(for interests: String) async -> ([String], [String]) {
        let engine = AskEngine(scope: .paper("__for-you__"))
        let prompt = """
            A researcher describes their interests:

            \(interests)

            Write 6 queries for a scholarly search engine that together cover these interests, each 3 to 6 words, \
            one per line, each line starting with "Q: ". Each query covers a different part of the interests (together \
            they cover all of it, including the diseases and systems studied). Every query must name the researcher's field or kind of \
            data (for example "spatial transcriptomics cell segmentation", not "cell segmentation"), so that it finds \
            papers from their field rather than general ones. Then one line starting with "K: " with 12 to 20 \
            specific technical terms from their field (methods, data types, tools), separated by commas; no generic \
            words like "analysis", "data" or "AI".
            """
        if let text = await engine.plainAnswer(prompt, instructions: "You help a researcher find new papers. Answer only in the requested format.") {
            let lines = text.components(separatedBy: .newlines).map { $0.trimmingCharacters(in: .whitespaces) }
            let q = lines.filter { $0.uppercased().hasPrefix("Q:") }.map { clean(String($0.dropFirst(2))) }.filter { $0.count > 3 }
            let k = lines.first { $0.uppercased().hasPrefix("K:") }.map {
                String($0.dropFirst(2)).split(separator: ",").map { clean(String($0)).lowercased() }.filter { $0.count > 3 }
            } ?? []
            // drop near-repeats ("x y" when "x y z" is there, or the other way round)
            var distinct: [String] = []
            for query in q {
                let words = Set(query.lowercased().split(separator: " "))
                if distinct.contains(where: { let o = Set($0.lowercased().split(separator: " ")); return o.isSubset(of: words) || words.isSubset(of: o) }) { continue }
                distinct.append(query)
            }
            if distinct.count >= 2 { return (Array(distinct.prefix(8)), Array(k.prefix(24))) }
        }
        // no model: the paragraph's own phrases
        let phrases = interests.components(separatedBy: CharacterSet(charactersIn: ".,;:\n()"))
            .map { clean($0) }.filter { (2...6).contains($0.split(separator: " ").count) }
        let words = Set(interests.lowercased().split { !$0.isLetter && $0 != "-" }.map(String.init).filter { $0.count > 6 })
        return (Array(phrases.prefix(6)), Array(words.prefix(20)))
    }

    private static func clean(_ s: String) -> String {
        s.trimmingCharacters(in: CharacterSet(charactersIn: " \t\"'“”-*•0123456789."))
    }

    /// Does the paper really fit, and why (one sentence)? nil when the model didn't answer.
    static func screen(_ h: DiscoverHit, interests: String, liked: [String] = [],
                       with engine: AskEngine) async -> (score: Double, why: String)? {
        let abstract = h.meta.abstractText.isEmpty ? "(no abstract)" : String(h.meta.abstractText.prefix(1100))
        let examples = liked.isEmpty ? "" : "\n\nRecent papers they marked as exactly what they want:\n" +
            liked.map { "- \($0)" }.joined(separator: "\n")
        let prompt = """
            The researcher's interests:
            \(interests)\(examples)

            New paper:
            Title: \(h.meta.title)
            Abstract: \(abstract)

            Rate from 0 to 10 how well this paper matches the researcher's interests:
            10 = squarely in their field and directly useful to their work;
            5 = the same broad area but a different topic, organism, disease or question;
            0 = unrelated.
            Sharing only general words (such as "multi-omics", "AI", "tissue" or "integration") is worth at most 4.

            Reply with a single line: SCORE: <number> — <one sentence of at most 20 words on what specifically \
            connects this paper to their work, addressed to them as "you"; \
            do not start with "This paper" — start with the specific point, e.g. "A segmentation method for Xenium data, …">
            """
        guard let a = await engine.plainAnswer(prompt, instructions: "You screen new scientific papers for one researcher. You are strict: most papers score below 7.")?
            .trimmingCharacters(in: .whitespacesAndNewlines), !a.isEmpty else { return nil }
        // "SCORE: 8 — …" (tolerating "Score 8/10 - …")
        let digits = a.drop { !$0.isNumber }.prefix { $0.isNumber || $0 == "." }
        guard let score = Double(digits) else { return nil }
        var why = a
        for sep in ["—", " – ", " - "] {
            if let r = a.range(of: sep) { why = String(a[r.upperBound...]); break }
        }
        why = why.trimmingCharacters(in: .whitespacesAndNewlines.union(CharacterSet(charactersIn: "\"*")))
        if why.uppercased().hasPrefix("SCORE") || why.count < 8 { why = "Matches your interests." }
        return (score, why)
    }

    /// Drafts the interests paragraph from the library (the most recent papers).
    func draftInterests() async -> String? {
        guard let model else { return nil }
        let papers = model.lib.papers(filter: PaperFilter(collection: nil, tag: nil, starredOnly: false, unreadOnly: false), sort: .added)
        guard !papers.isEmpty else { return nil }
        let list = papers.prefix(60).map { "- " + $0.meta.title }.joined(separator: "\n")
        let prompt = """
            These are papers a researcher saved recently:

            \(list)

            Write their research interests as one paragraph of 3 to 5 sentences in the first person ("I work on…"). \
            Describe the field, the questions, the methods and the kinds of data in your own words, as a researcher \
            would describe their work. Be concrete: name the specific techniques (for example "spatial transcriptomics", \
            "cell segmentation"), tissues, organisms and diseases that recur in the titles; never use vague phrases \
            like "advanced techniques" or "complex biological data", never copy whole titles. Only the paragraph.
            """
        let engine = AskEngine(scope: .paper("__for-you__"))
        return await engine.plainAnswer(prompt, instructions: "You describe a researcher's interests precisely and briefly.")?
            .trimmingCharacters(in: .whitespacesAndNewlines)
    }

    // MARK: preprints

    /// The last days of bioRxiv or medRxiv, keeping papers that use your keywords. The API serves
    /// 30 papers a request, so after the first page the rest are fetched six at a time.
    nonisolated static func preprints(_ server: String, from: String, to: String, keywords: [String]) async -> [DiscoverHit] {
        let kws = keywords.map { $0.lowercased() }.filter { $0.count > 3 }
        guard !kws.isEmpty else { return [] }
        let config = URLSessionConfiguration.ephemeral
        config.timeoutIntervalForRequest = 30
        config.timeoutIntervalForResource = 60
        let session = URLSession(configuration: config)
        defer { session.finishTasksAndInvalidate() }

        func page(_ cursor: Int) async -> (items: [[String: Any]], total: Int) {
            guard let url = URL(string: "https://api.biorxiv.org/details/\(server)/\(from)/\(to)/\(cursor)/json"),
                  let (data, _) = try? await session.data(from: url),
                  let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { return ([], 0) }
            let msg = (json["messages"] as? [[String: Any]])?.first
            let total = (msg?["total"] as? Int) ?? Int(msg?["total"] as? String ?? "") ?? 0
            return ((json["collection"] as? [[String: Any]]) ?? [], total)
        }

        let first = await page(0)
        var all = first.items
        let size = max(first.items.count, 1)
        let cursors = Array(stride(from: size, to: min(first.total, 3000), by: size))
        for chunk in stride(from: 0, to: cursors.count, by: 6).map({ Array(cursors[$0..<min($0 + 6, cursors.count)]) }) {
            await withTaskGroup(of: [[String: Any]].self) { g in
                for c in chunk { g.addTask { await page(c).items } }
                for await items in g { all.append(contentsOf: items) }
            }
        }

        var out: [String: DiscoverHit] = [:]
        let name = server == "biorxiv" ? "bioRxiv" : "medRxiv"
        for it in all {
            let title = (it["title"] as? String ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
            let abstract = it["abstract"] as? String ?? ""
            let doi = it["doi"] as? String ?? ""
            let t = title.lowercased(), a = abstract.lowercased()
            let inTitle = kws.contains { t.contains($0) }
            let inAbstract = kws.filter { a.contains($0) }.count
            guard inTitle || inAbstract >= 2, !doi.isEmpty else { continue }
            let authors = (it["authors"] as? String ?? "").split(separator: ";").map { part -> String in
                // "Smith, J. A." → "J. A. Smith"
                let bits = part.split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) }
                return bits.count == 2 ? "\(bits[1]) \(bits[0])" : part.trimmingCharacters(in: .whitespaces)
            }.filter { !$0.isEmpty }
            out[doi] = DiscoverHit(meta: PaperMeta(title: title, authors: authors, date: it["date"] as? String ?? "", journal: name,
                                                   volume: "", issue: "", pages: "", publisher: "", doi: doi, pmid: "", pmcid: "",
                                                   arxiv: "", url: "https://www.\(server).org/content/\(doi)", abstractText: abstract,
                                                   keywords: [], license: ""),
                                   citedBy: 0, openAccess: true, inLibrary: nil)
        }
        return Array(out.values)
    }

    // MARK: notifications

    private func requestNotifications() {
        UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .badge, .sound]) { _, _ in }
    }

    private func notify(_ n: Int) {
        let c = UNMutableNotificationContent()
        c.title = "New for You"
        c.body = "\(n) new paper\(n == 1 ? "" : "s") picked for your research."
        UNUserNotificationCenter.current().add(UNNotificationRequest(identifier: "foryou", content: c, trigger: nil))
    }
}

extension JSONEncoder {
    static var iso: JSONEncoder { let e = JSONEncoder(); e.dateEncodingStrategy = .iso8601; e.outputFormatting = [.prettyPrinted]; return e }
}
extension JSONDecoder {
    static var iso: JSONDecoder { let d = JSONDecoder(); d.dateDecodingStrategy = .iso8601; return d }
}

extension LibraryModel {
    nonisolated static func hitKey(_ h: DiscoverHit) -> String {
        h.meta.doi.isEmpty ? titleKey(h.meta.title) : doiKey(h.meta.doi)
    }
}

// MARK: - Views

/// "New for You": the ranked feed.
struct WatchInbox: View {
    @EnvironmentObject var model: LibraryModel
    @ObservedObject var center = WatchCenter.shared
    @Binding var selection: String?
    @State private var settings = false
    @State private var pushed: FeedKey?
    struct FeedKey: Hashable, Identifiable { let id: String }

    var body: some View {
        List(selection: $selection) {
            if center.interests.isEmpty {
                Section { intro }
            } else {
                Section { status }
            }
            if !center.interests.isEmpty, !center.finds.isEmpty, center.summarizing || !center.state.digest.isEmpty {
                Section { digest }
            }
            if !center.finds.isEmpty {
                Section {
                    ForEach(center.finds) { f in
                        row(f).tag(f.id)
                    }
                }
            } else if !center.interests.isEmpty && !center.checking {
                Text("Nothing yet. New papers are picked once a day; refresh to look now.")
                    .font(.callout).foregroundStyle(.secondary)
            }
        }
        #if os(iOS)
        .navigationDestination(item: $pushed) { k in
            if let f = center.finds.first(where: { $0.id == k.id }) { DiscoverDetail(hit: f.hit) }
        }
        #endif
        .onAppear { center.load(); center.markOpened() }
        #if os(macOS)
        // the detail pane always shows a paper: the first one, or the next after one is turned down
        .onChange(of: center.finds.map(\.id), initial: true) { _, ids in
            if selection == nil || !ids.contains(selection!) { selection = ids.first }
        }
        #endif
        .onDisappear { center.markOpened() }
        .task(id: center.digestKey) {
            // after a pause, so turning down a few papers in a row doesn't redo it each time
            try? await Task.sleep(for: .seconds(3))
            center.summarizeSoon()
        }
        .sheet(isPresented: $settings) { ForYouSettings().environmentObject(model) }
    }

    private var intro: some View {
        VStack(alignment: .leading, spacing: 10) {
            Label("Papers picked for your research", systemImage: "sparkles").font(.headline).foregroundStyle(Color.luntaTeal)
            Text("Describe what you work on in a few sentences. Every day Lunta searches OpenAlex, bioRxiv and medRxiv, ranks what's new against your interests and your library, and has the on-device model read the best abstracts and say why each one fits you.")
                .font(.callout).foregroundStyle(.secondary)
            Button { settings = true } label: { Label("Describe Your Research…", systemImage: "pencil") }
                .buttonStyle(.borderedProminent).tint(.luntaTeal)
        }
        .padding(.vertical, 6)
    }

    private var digest: some View {
        VStack(alignment: .leading, spacing: 6) {
            Label("In short", systemImage: "text.alignleft").font(.caption.weight(.semibold))
                .foregroundStyle(Color.luntaTeal).textCase(.uppercase)
            if center.state.digest.isEmpty {
                HStack(spacing: 6) { ProgressView().controlSize(.small); Text("Summarizing the papers…").font(.callout).foregroundStyle(.secondary) }
            } else {
                Text((try? AttributedString(markdown: center.state.digest,
                                            options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace)))
                     ?? AttributedString(center.state.digest))
                    .font(.callout).tint(.luntaTeal)
                    .environment(\.openURL, OpenURLAction { url in
                        guard let key = WatchCenter.feedKey(from: url) else { return .systemAction }
                        #if os(iOS)
                        pushed = FeedKey(id: key)
                        #else
                        selection = key
                        #endif
                        return .handled
                    })
            }
        }
        .padding(.vertical, 4)
    }

    private var status: some View {
        HStack(spacing: 12) {
            VStack(alignment: .leading, spacing: 2) {
                if let s = center.stage {
                    Text(s).font(.subheadline)
                } else if let last = center.state.lastRun {
                    Text("Picked \(last.formatted(.relative(presentation: .named)))").font(.subheadline)
                } else {
                    Text("Getting your first papers…").font(.subheadline)
                }
                Text(center.interests).font(.caption).foregroundStyle(.secondary).lineLimit(1)
            }
            Spacer()
            if center.checking { ProgressView().controlSize(.small) }
            else {
                Button { Task { await center.run() } } label: { Image(systemName: "arrow.clockwise") }
                    .buttonStyle(.borderless).help("Look for new papers now")
            }
            Button { settings = true } label: { Image(systemName: "slider.horizontal.3") }
                .buttonStyle(.borderless).help("Your interests and what you follow")
        }
    }

    @ViewBuilder private func row(_ f: WatchFind) -> some View {
        #if os(iOS)
        NavigationLink { DiscoverDetail(hit: f.hit) } label: { FeedRow(find: f) }
            .swipeActions {
                Button("Not for Me", systemImage: "hand.thumbsdown") { center.dismiss(f) }.tint(.gray)
                Button("More Like This", systemImage: "hand.thumbsup") { center.boost(f) }.tint(.luntaTeal)
            }
        #else
        FeedRow(find: f)
            .contextMenu {
                Button(f.record.boosted == true ? "Undo More Like This" : "More Like This", systemImage: "hand.thumbsup") { center.boost(f) }
                Button("Not for Me", systemImage: "hand.thumbsdown") { center.dismiss(f) }
            }
        #endif
    }
}

/// One paper in the feed: what it is, why it was picked, the start of the abstract, and actions.
struct FeedRow: View {
    @EnvironmentObject var model: LibraryModel
    @ObservedObject private var center = WatchCenter.shared
    let find: WatchFind
    @State private var adding = false

    var body: some View {
        let r = find.record
        VStack(alignment: .leading, spacing: 7) {
            Text(r.title).font(.headline).lineLimit(3)
            HStack(spacing: 5) {
                if r.openAccess { Image(systemName: "lock.open").foregroundStyle(.green).help("Open access") }
                Text(byline(r)).lineLimit(1)
            }
            .font(.caption).foregroundStyle(.secondary)
            if let sum = r.summary {
                Text(sum).font(.subheadline)
            } else if !r.abstract.isEmpty {
                Text(r.abstract).font(.footnote).foregroundStyle(.secondary).lineLimit(3)
            }
            (Text(Image(systemName: "sparkles")) + Text(" ") + Text(r.reason))
                .font(.footnote).foregroundStyle(Color.luntaTeal).lineLimit(3)
            HStack(spacing: 4) {
                if let id = model.libraryID(of: find.hit) {
                    OpenAddedPaper(id: id, compact: true).tint(.luntaTeal).fixedSize()
                } else {
                    Button {
                        adding = true
                        Task {
                            if let id = await model.addDiscovered(find.hit) { center.added(key: find.id, id: id) }
                            adding = false
                        }
                    } label: {
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
                Button { center.boost(find) } label: {
                    Image(systemName: r.boosted == true ? "hand.thumbsup.fill" : "hand.thumbsup")
                        .foregroundStyle(r.boosted == true ? Color.luntaTeal : Color.secondary)
                        .frame(width: 40, height: 32).contentShape(Rectangle())
                }
                .buttonStyle(.borderless)
                .accessibilityLabel(r.boosted == true ? "Undo More Like This" : "More Like This")
                .help("More like this: papers like it rank higher")
                Button { center.dismiss(find) } label: {
                    Image(systemName: "hand.thumbsdown").foregroundStyle(.secondary)
                        .frame(width: 40, height: 32).contentShape(Rectangle())
                }
                .buttonStyle(.borderless)
                .accessibilityLabel("Not for Me")
                .help("Not for me: hides it, and papers like it rank lower")
            }
            .font(.body)
        }
        .padding(.vertical, 6)
    }

    private func byline(_ r: FeedRecord) -> String {
        let who = r.authors.first.map { r.authors.count > 1 ? "\($0) et al." : $0 } ?? ""
        return [who, r.journal, r.date].filter { !$0.isEmpty }.joined(separator: " · ")
    }
}

/// Your interests, how many papers a day, and the authors / papers / topics you follow.
struct ForYouSettings: View {
    @EnvironmentObject var model: LibraryModel
    @ObservedObject private var center = WatchCenter.shared
    @Environment(\.dismiss) private var dismiss
    @State private var text = ""
    @State private var perDay = 8
    @State private var drafting = false
    @State private var addingWatch = false

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextEditor(text: $text)
                        .frame(minHeight: 150)
                    Button {
                        drafting = true
                        Task { if let d = await center.draftInterests(), !d.isEmpty { text = d }; drafting = false }
                    } label: { Label(drafting ? "Reading your library…" : "Draft from My Library", systemImage: "wand.and.stars") }
                    .disabled(drafting)
                } header: { Text("Your research") } footer: {
                    Text("A few sentences on your topics, methods and systems. Lunta turns them into searches and uses them to rank and screen new papers.")
                }
                Section {
                    Stepper("Up to \(perDay) papers a day", value: $perDay, in: 3...20)
                }
                Section {
                    ForEach(center.watches) { w in
                        Label(w.describe, systemImage: w.icon)
                            .swipeActions { Button("Remove", role: .destructive) { center.remove(w.id) } }
                            .contextMenu { Button("Remove", role: .destructive) { center.remove(w.id) } }
                    }
                    Button { addingWatch = true } label: { Label("Follow an Author, Paper or Topic…", systemImage: "plus") }
                } header: { Text("Also follow") } footer: {
                    Text("New papers by authors you follow, and papers citing ones you watch, always show up.")
                }
            }
            .formStyle(.grouped)
            .navigationTitle("New for You")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Save") { center.setInterests(text, perDay: perDay); dismiss() }
                }
            }
            .sheet(isPresented: $addingWatch) { NewWatchView().environmentObject(model) }
            .onAppear { text = center.interests; perDay = center.perDay }
        }
        #if os(macOS)
        .frame(minWidth: 520, minHeight: 560)
        #endif
    }
}

struct NewWatchView: View {
    @EnvironmentObject var model: LibraryModel
    @Environment(\.dismiss) private var dismiss
    @State var kind: Watch.Kind = .author
    @State var value = ""

    var body: some View {
        NavigationStack {
            Form {
                Picker("Follow", selection: $kind) {
                    Text("An author").tag(Watch.Kind.author)
                    Text("Papers citing…").tag(Watch.Kind.citing)
                    Text("A topic").tag(Watch.Kind.topic)
                }
                .pickerStyle(.segmented)
                TextField(kind == .topic ? "Words, e.g. velocity-based training" : kind == .author ? "Author's name" : "DOI of the paper", text: $value)
                    .autocorrectionDisabled()
                if kind == .citing {
                    Text("Tip: long-press or right-click a paper in your library › Watch for Papers Citing This.").font(.caption).foregroundStyle(.secondary)
                }
            }
            .formStyle(.grouped)
            .navigationTitle("Follow")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Follow") {
                        let v = value.trimmingCharacters(in: .whitespacesAndNewlines)
                        WatchCenter.shared.add(Watch(kind: kind, value: v, label: v))
                        dismiss()
                    }
                    .disabled(value.trimmingCharacters(in: .whitespaces).isEmpty)
                }
            }
        }
        #if os(macOS)
        .frame(minWidth: 420, minHeight: 240)
        #endif
    }
}

/// Context-menu items to start following from a paper.
struct WatchMenu: View {
    let paper: Paper
    var body: some View {
        if !paper.meta.doi.isEmpty {
            Button("Watch for Papers Citing This") {
                WatchCenter.shared.add(Watch(kind: .citing, value: paper.meta.doi, label: String(paper.meta.title.prefix(70))))
            }
        }
        if let a = paper.meta.authors.first {
            Button("Follow \(a)") { WatchCenter.shared.add(Watch(kind: .author, value: a, label: a)) }
        }
        if let last = paper.meta.authors.last, paper.meta.authors.count > 1 {
            Button("Follow \(last)") { WatchCenter.shared.add(Watch(kind: .author, value: last, label: last)) }
        }
    }
}
