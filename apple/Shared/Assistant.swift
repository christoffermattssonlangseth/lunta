import Foundation
import SwiftUI
import FoundationModels
import NaturalLanguage
import MarginsCore

// Margins Assistant: a small agent on Apple's own language models.
// It uses Private Cloud Compute (Apple's larger server model) when the OS offers it and the
// user allows it, otherwise the on-device model. Both are free and need no account.
// The model only sees what its tools hand it: passages of the open paper, or search results
// from the library. Changes to the library are proposed, never applied without the user.

enum AskScope: Equatable {
    case library
    case paper(String)
}

/// Something an answer can cite as [n].
struct AskSource: Identifiable, Equatable, Codable {
    enum Kind: Equatable, Codable {
        case passage(String)     // element id in the reading view
        case paper(String)       // paper id in the library
        case external(String)    // a paper beyond the library (DOI or title)
    }
    let id: Int
    let kind: Kind
    let label: String
}

struct AskMessage: Identifiable, Equatable, Codable {
    enum Role: String, Codable { case user, assistant, activity, notice }
    var id = UUID()
    let role: Role
    var text: String
}

struct ProposedChange: Identifiable, Equatable, Codable {
    enum Kind: Equatable, Codable {
        case tag(String, [String])
        case collection(String, [String])
        case markRead([String], Bool)
        case add(Int)                     // external source number
    }
    enum State: String, Codable { case pending, applied, dismissed, failed }
    var id = UUID()
    let kind: Kind
    let summary: String
    var state: State = .pending
}

enum AskModelChoice: String, CaseIterable, Identifiable {
    case automatic, onDevice, openModel, privateCloud
    var id: String { rawValue }
    var title: String {
        switch self {
        case .automatic: return "Best available"
        case .onDevice: return "Apple Intelligence (on device)"
        case .openModel: return "Open model (Qwen3 4B, on device)"
        case .privateCloud: return "Private Cloud Compute"
        }
    }
}

// MARK: - Shared state between the tools and the panel

/// Tool calls run off the main actor; they register sources and proposals here.
final class AskContext: @unchecked Sendable {
    let lib: Library
    let passages: [NarrationItem]
    private let lock = NSLock()
    private var sources: [AskSource] = []
    private(set) var external: [Int: DiscoverHit] = [:]
    let emit: @Sendable (AskEvent) -> Void
    /// Characters of tool output the model may read per question (its context window is small).
    var budget = 4200
    private var spent = 0
    let fulltext: @Sendable (String) async -> String?

    init(lib: Library, passages: [NarrationItem], earlier: [AskSource] = [], emit: @escaping @Sendable (AskEvent) -> Void,
         fulltext: @escaping @Sendable (String) async -> String?) {
        self.lib = lib
        self.sources = earlier.sorted { $0.id < $1.id }
        self.passages = passages
        self.emit = emit
        self.fulltext = fulltext
        let items = passages.filter { $0.kind != "heading" }
        let index = HybridIndex(items.map(\.text))
        index.warm()
        paperIndex = (items, index)
    }

    /// Returns the citation number for a source, reusing it if it was already handed out.
    func register(_ kind: AskSource.Kind, _ label: String, hit: DiscoverHit? = nil) -> Int {
        lock.lock(); defer { lock.unlock() }
        if let s = sources.first(where: { $0.kind == kind }) { return s.id }
        let s = AskSource(id: (sources.map(\.id).max() ?? 0) + 1, kind: kind, label: label)
        sources.append(s)
        if let hit { external[s.id] = hit }
        emit(.source(s))
        return s.id
    }

    /// Paragraphs of the open paper and their search index (built once, in init).
    let paperIndex: (items: [NarrationItem], index: HybridIndex)

    func newQuestion() { lock.lock(); spent = 0; lock.unlock() }

    /// Passes tool output through the per-question reading budget.
    func take(_ text: String) -> String {
        lock.lock(); defer { lock.unlock() }
        if spent >= budget { return "You have already read enough for this question. Answer now from what you have, citing numbers." }
        let room = budget - spent
        let out = text.count <= room ? text : String(text.prefix(room)) + "…"
        spent += out.count
        return out
    }

    func source(_ n: Int) -> AskSource? {
        lock.lock(); defer { lock.unlock() }
        return sources.first { $0.id == n }
    }

    func hit(_ n: Int) -> DiscoverHit? {
        lock.lock(); defer { lock.unlock() }
        return external[n]
    }

    /// Paper ids for a list of source numbers (ignores passages and papers beyond the library).
    func paperIDs(_ refs: [Int]) -> [String] {
        refs.compactMap { n in
            switch source(n)?.kind {
            case .paper(let id): return id
            case .external: return hit(n)?.inLibrary
            default: return nil
            }
        }
    }
}

enum AskEvent: Sendable {
    case activity(String)
    case source(AskSource)
    case proposal(ProposedChange)
}

// MARK: - Retrieval

/// Small BM25 ranker over paragraphs, so the model reads only the passages that matter.
struct Retriever {
    let docs: [[String]]
    private var df: [String: Int] = [:]
    private let avg: Double

    static let stop: Set<String> = ["the", "and", "for", "are", "was", "were", "with", "that", "this", "from", "have", "has",
        "what", "which", "who", "how", "why", "when", "does", "did", "they", "their", "there", "these", "those", "into",
        "about", "than", "then", "also", "can", "not", "but", "all", "any", "our", "its", "his", "her", "you", "your",
        "paper", "study", "authors", "use", "used", "using", "show", "shows", "find", "found", "between", "tell", "explain"]

    static func terms(_ s: String) -> [String] {
        s.lowercased().split(whereSeparator: { !$0.isLetter && !$0.isNumber && $0 != "-" })
            .map(String.init).filter { $0.count > 2 && !stop.contains($0) }
    }

    init(_ texts: [String]) {
        docs = texts.map(Self.terms)
        for d in docs { for t in Set(d) { df[t, default: 0] += 1 } }
        avg = max(1, Double(docs.reduce(0) { $0 + $1.count }) / Double(max(1, docs.count)))
    }

    func rank(_ query: String, top: Int) -> [Int] {
        let q = Set(Self.terms(query))
        guard !q.isEmpty else { return [] }
        let n = Double(docs.count)
        var scores: [(Int, Double)] = []
        for (i, d) in docs.enumerated() where !d.isEmpty {
            var tf: [String: Int] = [:]
            for t in d where q.contains(t) { tf[t, default: 0] += 1 }
            guard !tf.isEmpty else { continue }
            var s = 0.0
            for (t, f) in tf {
                let idf = log(1 + (n - Double(df[t] ?? 0) + 0.5) / (Double(df[t] ?? 0) + 0.5))
                let ff = Double(f)
                s += idf * ff * 2.2 / (ff + 1.2 * (0.25 + 0.75 * Double(d.count) / avg))
            }
            scores.append((i, s))
        }
        return scores.sorted { $0.1 > $1.1 }.prefix(top).map(\.0)
    }
}

/// Meaning-based ranking with Apple's on-device sentence embeddings, fused with BM25 keyword ranking,
/// so "how many patients" finds "118 subjects were included".
final class HybridIndex: @unchecked Sendable {
    private let texts: [String]
    private let bm25: Retriever
    private var vectors: [[Double]]?
    private let lock = NSLock()
    private static let embedding = NLEmbedding.sentenceEmbedding(for: .english)
    /// NLEmbedding isn't safe to use from several threads at once (parallel tool calls deadlock it).
    private static let embedLock = NSLock()

    private static func vector(_ s: String) -> [Double]? {
        embedLock.lock(); defer { embedLock.unlock() }
        return embedding?.vector(for: s)
    }

    func warm() { if !texts.isEmpty { _ = vecs() } }

    init(_ texts: [String]) {
        self.texts = texts
        bm25 = Retriever(texts)
    }

    private func vecs() -> [[Double]] {
        lock.lock(); defer { lock.unlock() }
        if let vectors { return vectors }
        let v = texts.map { Self.vector(String($0.prefix(1200))) ?? [] }
        vectors = v
        return v
    }

    func rank(_ query: String, top: Int) -> [Int] {
        var fused: [Int: Double] = [:]
        for (r, i) in bm25.rank(query, top: 12).enumerated() { fused[i, default: 0] += 1.0 / Double(60 + r) }
        if let q = Self.vector(query) {
            let sims = vecs().enumerated().compactMap { (i, v) -> (Int, Double)? in
                guard v.count == q.count, !v.isEmpty else { return nil }
                var dot = 0.0, a = 0.0, b = 0.0
                for k in 0..<v.count { dot += v[k] * q[k]; a += v[k] * v[k]; b += q[k] * q[k] }
                return (i, dot / max(1e-9, (a * b).squareRoot()))
            }.sorted { $0.1 > $1.1 }
            for (r, (i, _)) in sims.prefix(12).enumerated() { fused[i, default: 0] += 1.0 / Double(60 + r) }
        }
        return fused.sorted { $0.value > $1.value }.prefix(top).map(\.key)
    }
}

/// "One-hundred eighteen subjects" → "118 subjects": small models misread spelled-out numbers.
func digitize(_ text: String) -> String {
    let units: [String: Int] = ["zero": 0, "one": 1, "two": 2, "three": 3, "four": 4, "five": 5, "six": 6, "seven": 7, "eight": 8,
        "nine": 9, "ten": 10, "eleven": 11, "twelve": 12, "thirteen": 13, "fourteen": 14, "fifteen": 15, "sixteen": 16,
        "seventeen": 17, "eighteen": 18, "nineteen": 19, "twenty": 20, "thirty": 30, "forty": 40, "fifty": 50, "sixty": 60,
        "seventy": 70, "eighty": 80, "ninety": 90]
    let scales: [String: Int] = ["hundred": 100, "thousand": 1000]
    let pattern = /(?i)\b(?:(?:zero|one|two|three|four|five|six|seven|eight|nine|ten|eleven|twelve|thirteen|fourteen|fifteen|sixteen|seventeen|eighteen|nineteen|twenty|thirty|forty|fifty|sixty|seventy|eighty|ninety|hundred|thousand|and)(?:[\s-]+|\b)){2,}/
    return text.replacing(pattern) { m in
        let raw = String(m.0)
        let words = raw.lowercased().split(whereSeparator: { $0 == " " || $0 == "-" || $0 == "\n" }).map(String.init)
        guard words.contains(where: { scales[$0] != nil || ($0.hasSuffix("ty") && units[$0] != nil) }),
              words.first != "and", words.last != "and" else { return raw }
        var total = 0, current = 0
        for w in words where w != "and" {
            if let u = units[w] { current += u }
            else if let sc = scales[w] { current = max(current, 1) * sc; if sc == 1000 { total += current; current = 0 } }
        }
        let trailing = raw.last.map { $0 == " " || $0 == "-" } == true ? " " : ""
        return "\(total + current)" + trailing
    }
}

private func clip(_ s: String, _ n: Int) -> String {
    let t = s.trimmingCharacters(in: .whitespacesAndNewlines)
    return t.count <= n ? t : String(t.prefix(n)) + "…"
}

private func paperLine(_ p: Paper) -> String {
    let a = p.meta.authors.first.map { $0.split(separator: ",").first.map(String.init) ?? $0 } ?? "Unknown"
    var flags: [String] = []
    if !p.read { flags.append("unread") }
    if p.starred { flags.append("starred") }
    if !p.tags.isEmpty { flags.append("tags: " + p.tags.joined(separator: ", ")) }
    let year = String(p.meta.date.prefix(4))
    return "\(clip(p.meta.title, 160)) — \(a)\(p.meta.authors.count > 1 ? " et al." : "") \(year)"
        + (p.meta.journal.isEmpty ? "" : ", \(p.meta.journal)")
        + (flags.isEmpty ? "" : " (\(flags.joined(separator: "; ")))")
}

// MARK: - Tools: inside one paper

struct FindInPaperTool: Tool {
    let ctx: AskContext
    let name = "find_in_paper"
    let description = "Finds the passages of the open paper that best match a query. Returns numbered passages to cite."

    @Generable
    struct Arguments {
        @Guide(description: "What to look for, as a short phrase, e.g. 'number of subjects included' or 'statistical analysis'")
        var query: String
    }

    func call(arguments: Arguments) async throws -> String { ctx.take(await run(arguments)) }

    private func run(_ arguments: Arguments) async -> String {
        ctx.emit(.activity("Looking in the paper for “\(arguments.query)”"))
        let (items, index) = ctx.paperIndex
        let hits = index.rank(arguments.query, top: 3)
        guard !hits.isEmpty else { return "No passage matches “\(arguments.query)”. Try other keywords." }
        return hits.map { i in
            let it = items[i]
            let n = ctx.register(.passage(it.id), clip(it.text, 80))
            return "[\(n)] \(digitize(clip(it.text, 600)))"
        }.joined(separator: "\n\n")
    }
}

struct ReadSectionTool: Tool {
    let ctx: AskContext
    let name = "read_section"
    let description = "Reads the start of a section of the open paper by its heading, e.g. Methods, Results, Discussion."

    @Generable
    struct Arguments {
        @Guide(description: "The section heading or part of it, e.g. 'Methods' or 'Limitations'")
        var heading: String
    }

    func call(arguments: Arguments) async throws -> String { ctx.take(await run(arguments)) }

    private func run(_ arguments: Arguments) async -> String {
        ctx.emit(.activity("Reading “\(arguments.heading)”"))
        let items = ctx.passages
        let want = arguments.heading.lowercased()
        guard let start = items.firstIndex(where: { $0.kind == "heading" && $0.text.lowercased().contains(want) })
                ?? items.firstIndex(where: { $0.kind == "heading" && want.contains($0.text.lowercased()) }) else {
            let headings = items.filter { $0.kind == "heading" }.map(\.text).prefix(30).joined(separator: "; ")
            return "No section called “\(arguments.heading)”. Sections: \(headings)"
        }
        var out: [String] = ["Section: \(items[start].text)"]
        var used = 0
        for it in items[(start + 1)...] {
            if it.kind == "heading" && used > 0 { break }
            if it.kind == "heading" { continue }
            let n = ctx.register(.passage(it.id), clip(it.text, 80))
            let t = digitize(clip(it.text, 900))
            out.append("[\(n)] \(t)")
            used += t.count
            if used > 2400 { break }
        }
        return out.joined(separator: "\n\n")
    }
}

// MARK: - Tools: the library

struct SearchLibraryTool: Tool {
    let ctx: AskContext
    let name = "search_library"
    let description = "Searches the user's own library (titles, authors, abstracts, full text, notes). Returns numbered papers."

    @Generable
    struct Arguments {
        @Guide(description: "Keywords, e.g. 'CSF lactate multiple sclerosis'")
        var query: String
        @Guide(description: "How many papers to return", .range(1...8))
        var limit: Int
    }

    func call(arguments: Arguments) async throws -> String { ctx.take(await run(arguments)) }

    private func run(_ arguments: Arguments) async -> String {
        ctx.emit(.activity("Searching your library for “\(arguments.query)”"))
        let limit = UInt32(max(1, min(8, arguments.limit)))
        var hits = (try? ctx.lib.search(query: arguments.query, limit: limit)) ?? []
        if hits.count < Int(limit) {
            // the index ANDs every word; widen to papers matching most of the words
            var count: [String: (Int, SearchHit)] = [:]
            for w in Retriever.terms(arguments.query) {
                for h in (try? ctx.lib.search(query: w, limit: 40)) ?? [] {
                    let c = count[h.paper.id]?.0 ?? 0
                    count[h.paper.id] = (c + 1, h)
                }
            }
            let seen = Set(hits.map(\.paper.id))
            let more = count.values.filter { !seen.contains($0.1.paper.id) && $0.0 >= 2 }.sorted { $0.0 > $1.0 }.map(\.1)
            hits += more.prefix(Int(limit) - hits.count)
        }
        guard !hits.isEmpty else { return "Nothing in the library matches “\(arguments.query)”." }
        return hits.map { h in
            let n = ctx.register(.paper(h.paper.id), h.paper.meta.title)
            let snip = h.snippet.isEmpty ? "" : "\n   match: " + clip(h.snippet, 120)
            return "[\(n)] \(paperLine(h.paper))\(snip)"
        }.joined(separator: "\n")
    }
}

struct ListPapersTool: Tool {
    let ctx: AskContext
    let name = "list_papers"
    let description = "Lists papers in the user's library: unread, starred, most recently added, or with a tag."

    @Generable
    struct Arguments {
        @Guide(description: "Which papers", .anyOf(["unread", "starred", "recent", "tag"]))
        var which: String
        @Guide(description: "The tag name, only when which is 'tag'")
        var tag: String?
        @Guide(description: "How many papers to return", .range(1...15))
        var limit: Int
    }

    func call(arguments: Arguments) async throws -> String { ctx.take(await run(arguments)) }

    private func run(_ arguments: Arguments) async -> String {
        var f = PaperFilter(collection: nil, tag: nil, starredOnly: false, unreadOnly: false)
        switch arguments.which {
        case "unread": f.unreadOnly = true
        case "starred": f.starredOnly = true
        case "tag": f.tag = arguments.tag
        default: break
        }
        ctx.emit(.activity("Listing \(arguments.which == "tag" ? "#\(arguments.tag ?? "")" : arguments.which) papers"))
        let papers = ctx.lib.papers(filter: f, sort: .added).prefix(max(1, min(15, arguments.limit)))
        guard !papers.isEmpty else { return "No papers." }
        return papers.map { p in "[\(ctx.register(.paper(p.id), p.meta.title))] \(paperLine(p))" }.joined(separator: "\n")
    }
}

struct PaperDetailsTool: Tool {
    let ctx: AskContext
    let name = "paper_details"
    let description = "Gets the abstract and details of a numbered paper from earlier results."

    @Generable
    struct Arguments {
        @Guide(description: "The paper's number, e.g. 3 for [3]")
        var number: Int
    }

    func call(arguments: Arguments) async throws -> String { ctx.take(await run(arguments)) }

    private func run(_ arguments: Arguments) async -> String {
        guard let s = ctx.source(arguments.number) else { return "There is no [\(arguments.number)]." }
        if case .external = s.kind, let hit = ctx.hit(arguments.number) {
            return "[\(s.id)] (beyond the library) \(hit.meta.title) — \(hit.meta.authors.prefix(3).joined(separator: ", ")) \(hit.meta.date.prefix(4))\nAbstract: \(clip(hit.meta.abstractText, 1400))"
        }
        guard case .paper(let id) = s.kind, let p = ctx.lib.paper(id: id) else { return "[\(arguments.number)] is not a paper." }
        ctx.emit(.activity("Reading the abstract of “\(clip(p.meta.title, 60))”"))
        return """
        [\(s.id)] \(paperLine(p))
        Authors: \(p.meta.authors.prefix(8).joined(separator: "; "))
        DOI: \(p.meta.doi)
        Abstract: \(p.meta.abstractText.isEmpty ? "(none)" : clip(p.meta.abstractText, 1000))
        """
    }
}

struct SearchInsidePaperTool: Tool {
    let ctx: AskContext
    let name = "search_inside_paper"
    let description = "Finds passages in the full text of a numbered library paper that match a query."

    @Generable
    struct Arguments {
        @Guide(description: "The paper's number, e.g. 3 for [3]")
        var number: Int
        @Guide(description: "A few specific keywords")
        var query: String
    }

    func call(arguments: Arguments) async throws -> String { ctx.take(await run(arguments)) }

    private func run(_ arguments: Arguments) async -> String {
        guard case .paper(let id)? = ctx.source(arguments.number)?.kind, let p = ctx.lib.paper(id: id) else {
            return "[\(arguments.number)] is not a paper in the library."
        }
        ctx.emit(.activity("Reading “\(clip(p.meta.title, 50))” for “\(arguments.query)”"))
        guard let text = await ctx.fulltext(id), !text.isEmpty else {
            return "No full text for [\(arguments.number)]. Abstract: \(clip(p.meta.abstractText, 600))"
        }
        let paras = text.components(separatedBy: "\n\n").map { $0.replacingOccurrences(of: "\n", with: " ") }.filter { $0.count > 60 }
        let top = HybridIndex(paras).rank(arguments.query, top: 2)
        guard !top.isEmpty else { return "No passage in [\(arguments.number)] matches “\(arguments.query)”." }
        return "From [\(arguments.number)]:\n" + top.map { "• " + digitize(clip(paras[$0], 400)) }.joined(separator: "\n")
    }
}

struct FindNewPapersTool: Tool {
    let ctx: AskContext
    let name = "find_new_papers"
    let description = "Searches published literature beyond the user's library (OpenAlex). Results are NOT in the library."

    @Generable
    struct Arguments {
        @Guide(description: "Keywords for the topic")
        var query: String
    }

    func call(arguments: Arguments) async throws -> String { ctx.take(await run(arguments)) }

    private func run(_ arguments: Arguments) async -> String {
        ctx.emit(.activity("Searching beyond your library for “\(arguments.query)”"))
        let lib = ctx.lib
        let q = arguments.query
        guard let hits = try? lib.discover(query: q, limit: 6), !hits.isEmpty else { return "No papers found." }
        return hits.map { h in
            let year = String(h.meta.date.prefix(4))
            if let id = h.inLibrary {
                let n = ctx.register(.paper(id), h.meta.title)
                return "[\(n)] ALREADY IN LIBRARY: \(clip(h.meta.title, 150)) (\(year))"
            }
            let n = ctx.register(.external(h.meta.doi.isEmpty ? h.meta.title : h.meta.doi), h.meta.title, hit: h)
            return "[\(n)] NEW: \(clip(h.meta.title, 150)) — \(h.meta.authors.first ?? "") \(year), \(h.meta.journal); cited by \(h.citedBy)"
        }.joined(separator: "\n")
    }
}

struct ProposeChangeTool: Tool {
    let ctx: AskContext
    let name = "propose_change"
    let description = "Proposes a change to the library for the user to confirm: tag papers, put them in a collection, mark read or unread, or add a new paper."

    @Generable
    struct Arguments {
        @Guide(description: "The change", .anyOf(["tag", "collection", "mark_read", "mark_unread", "add_to_library"]))
        var action: String
        @Guide(description: "Tag or collection name; empty for other actions")
        var name: String
        @Guide(description: "Numbers of the papers, e.g. [1, 4]")
        var numbers: [Int]
    }

    func call(arguments: Arguments) async throws -> String { ctx.take(await run(arguments)) }

    private func run(_ arguments: Arguments) async -> String {
        let name = arguments.name.trimmingCharacters(in: .whitespaces).replacingOccurrences(of: "#", with: "")
        let ids = Array(Set(ctx.paperIDs(arguments.numbers)))
        let count = "\(ids.count) paper\(ids.count == 1 ? "" : "s")"
        var proposals: [ProposedChange] = []
        switch arguments.action {
        case "tag":
            guard !name.isEmpty, !ids.isEmpty else { return "Need a tag name and library papers." }
            proposals.append(ProposedChange(kind: .tag(name, ids), summary: "Tag \(count) #\(name)"))
        case "collection":
            guard !name.isEmpty, !ids.isEmpty else { return "Need a collection name and library papers." }
            proposals.append(ProposedChange(kind: .collection(name, ids), summary: "Put \(count) in the collection “\(name)”"))
        case "mark_read", "mark_unread":
            guard !ids.isEmpty else { return "Need library papers." }
            let read = arguments.action == "mark_read"
            proposals.append(ProposedChange(kind: .markRead(ids, read), summary: "Mark \(count) as \(read ? "read" : "unread")"))
        case "add_to_library":
            for n in arguments.numbers {
                if let h = ctx.hit(n), h.inLibrary == nil {
                    proposals.append(ProposedChange(kind: .add(n), summary: "Add “\(clip(h.meta.title, 70))” to your library"))
                }
            }
            if proposals.isEmpty { return "Only papers from find_new_papers can be added." }
        default:
            return "Unknown action."
        }
        for p in proposals { ctx.emit(.proposal(p)) }
        return "Proposed. The user will see a button to confirm it; it has not been applied yet."
    }
}

// MARK: - The engine behind a panel

@MainActor
final class AskEngine: ObservableObject {
    let scope: AskScope
    @Published var messages: [AskMessage] = []
    @Published var sources: [Int: AskSource] = [:]
    @Published var proposals: [ProposedChange] = []
    @Published var busy = false
    @Published var usingCloud = false
    @Published var choice = AskModelChoice(rawValue: UserDefaults.standard.string(forKey: "askModel") ?? "") ?? .automatic {
        didSet { UserDefaults.standard.set(choice.rawValue, forKey: "askModel"); reset() }
    }

    weak var model: LibraryModel?
    /// Paper scope: supplies the reading view's paragraphs (ids match the page, so citations can scroll there).
    var passages: (() async -> [NarrationItem])?
    var selection: (() async -> String?)?

    private var session: LanguageModelSession?
    private var ctx: AskContext?
    private var task: Task<Void, Never>?

    init(scope: AskScope) {
        self.scope = scope
        restore()
    }

    // MARK: keeping the conversation

    /// Conversations survive quitting the app for a while (per device, not synced).
    static let keepFor: TimeInterval = 3 * 24 * 3600

    private struct Saved: Codable {
        var saved: Date
        var messages: [AskMessage]
        var sources: [AskSource]
        var proposals: [ProposedChange]
    }

    private static var folder: URL {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        return base.appendingPathComponent("Margins/Ask", isDirectory: true)
    }

    private var saveURL: URL {
        switch scope {
        case .library: return Self.folder.appendingPathComponent("library.json")
        case .paper(let id):
            let safe = id.map { $0.isLetter || $0.isNumber || $0 == "-" ? $0 : "_" }
            return Self.folder.appendingPathComponent("paper-\(String(safe)).json")
        }
    }

    /// True when this panel picked up an earlier conversation the model hasn't seen yet in this run.
    private var restored = false

    private func restore() {
        Self.pruneOld()
        guard let data = try? Data(contentsOf: saveURL),
              let s = try? JSONDecoder().decode(Saved.self, from: data),
              Date().timeIntervalSince(s.saved) < Self.keepFor else { return }
        messages = s.messages.filter { !($0.role == .assistant && $0.text.isEmpty) }
        sources = Dictionary(s.sources.map { ($0.id, $0) }, uniquingKeysWith: { a, _ in a })
        proposals = s.proposals
        restored = !messages.isEmpty
    }

    func save() {
        if messages.isEmpty { try? FileManager.default.removeItem(at: saveURL); return }
        let s = Saved(saved: Date(), messages: messages, sources: Array(sources.values), proposals: proposals)
        guard let data = try? JSONEncoder().encode(s) else { return }
        try? FileManager.default.createDirectory(at: Self.folder, withIntermediateDirectories: true)
        try? data.write(to: saveURL, options: .atomic)
    }

    private static func pruneOld() {
        let fm = FileManager.default
        guard let files = try? fm.contentsOfDirectory(at: folder, includingPropertiesForKeys: [.contentModificationDateKey]) else { return }
        for f in files {
            let d = (try? f.resourceValues(forKeys: [.contentModificationDateKey]))?.contentModificationDate ?? .distantPast
            if Date().timeIntervalSince(d) > keepFor { try? fm.removeItem(at: f) }
        }
    }

    /// The earlier exchange, condensed, so a fresh model session can carry on from it.
    private func earlierConversation(limit: Int) -> String {
        var lines: [String] = []
        for m in messages.dropLast() where m.role == .user || m.role == .assistant {
            lines.append("\(m.role == .user ? "User" : "You"): \(clip(m.text, limit / 2))")
        }
        var out = "", used = 0
        for l in lines.reversed() {
            if used + l.count > limit { break }
            out = l + "\n" + out
            used += l.count
        }
        let cites = sources.keys.sorted().prefix(30).map { "[\($0)] \(clip(sources[$0]!.label, 90))" }.joined(separator: "; ")
        return "\nEarlier in this conversation (continue from it; numbers keep their meaning):\n\(out)" +
            (cites.isEmpty ? "" : "Sources so far: \(cites)\n")
    }

    // MARK: availability

    /// nil when a model is ready, otherwise why not.
    var unavailableReason: String? {
        if cloudAvailable { return nil }
        if usesOpenModel { return needsOpenModel ? Self.openModelPending : nil }
        switch SystemLanguageModel.default.availability {
        case .available: return nil
        case .unavailable(.deviceNotEligible): return "This device can't run Apple Intelligence."
        case .unavailable(.appleIntelligenceNotEnabled): return "Turn on Apple Intelligence in System Settings to use the assistant."
        case .unavailable(.modelNotReady):
            #if os(iOS)
            return "Apple Intelligence is still getting its model ready on this iPhone. It downloads in the background over Wi‑Fi, fastest while charging (see Settings › Apple Intelligence & Siri). This panel opens by itself as soon as it's ready."
            #else
            return "Apple Intelligence is still getting its model ready on this Mac (see System Settings › Apple Intelligence & Siri). This panel opens by itself as soon as it's ready."
            #endif
        default: return "Apple's language model isn't available right now."
        }
    }

    private var cloudAvailable: Bool {
        guard Self.cloudEntitled, choice != .onDevice, !Self.cloudRefused else { return false }
        if #available(macOS 27.0, iOS 27.0, *) { return PrivateCloudComputeLanguageModel().isAvailable }
        return false
    }

    var modelLabel: String {
        if usesOpenModel { return "Qwen3 4B · on this device" }
        return usingCloud ? "Apple · Private Cloud Compute" : "Apple · on this device"
    }

    static let openModelPending = "open-model"

    /// Apple Intelligence isn't usable here (e.g. iPhones in the EU) or the user picked the open model.
    var usesOpenModel: Bool {
        guard #available(iOS 27.0, macOS 27.0, *) else { return false }
        if choice == .openModel { return true }
        if choice == .onDevice || cloudAvailable { return false }
        return !SystemLanguageModel.default.isAvailable
    }

    /// The open model still has to be downloaded (the panel offers the download).
    var needsOpenModel: Bool {
        guard #available(iOS 27.0, macOS 27.0, *) else { return false }
        return !OpenModel.shared.isReady
    }

    func reset() {
        task?.cancel()
        session = nil
        ctx = nil
        messages = []
        sources = [:]
        proposals = []
        busy = false
        restored = false
        save()
    }

    // MARK: asking

    func ask(_ question: String, shown: String? = nil) {
        let q = question.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !q.isEmpty, !busy else { return }
        messages.append(AskMessage(role: .user, text: shown ?? q))
        busy = true
        task = Task { await run(q) }
    }

    func stop() {
        task?.cancel()
        busy = false
        save()
    }

    private func run(_ q: String) async {
        defer { busy = false; save() }
        do {
            if session == nil { try await start() }
            let before = proposals.count
            try await stream(q)
            // small models sometimes describe a change instead of proposing it: nudge once
            if scope == .library, proposals.count == before, Self.asksForChange(q), !Task.isCancelled {
                try await stream("Call propose_change now for the papers you listed (use their numbers). Then confirm in one short sentence.")
            }
        } catch let e where Self.isOverflow(e) {
            // the model's memory is full: start a fresh conversation, read less, and try once more
            messages.removeAll { $0.role == .assistant && $0.text.isEmpty }
            messages.append(AskMessage(role: .notice, text: "That filled the model's memory, so it started a fresh conversation."))
            session = nil
            do {
                try await start()
                ctx?.budget = 2400
                try await stream(q)
            } catch { fail(error) }
        } catch is CancellationError {
        } catch {
            if usingCloud && !Task.isCancelled {
                // Private Cloud Compute refused (quota, network, not enabled): continue on the device
                Self.cloudRefused = true
                NSLog("Margins Ask: Private Cloud Compute failed, using on-device model: %@", String(describing: error))
                messages.removeAll { $0.role == .assistant && $0.text.isEmpty }
                session = nil
                do { try await start(); try await stream(q) } catch { fail(error) }
            } else {
                fail(error)
            }
        }
    }

    /// Private Cloud Compute needs Apple's managed entitlement (com.apple.developer.private-cloud-compute);
    /// calling it without one crashes. Set MarginsPrivateCloudCompute = YES in Info.plist once it's granted.
    static let cloudEntitled = Bundle.main.object(forInfoDictionaryKey: "MarginsPrivateCloudCompute") as? Bool == true

    /// Set when Private Cloud Compute failed this run; later sessions go straight to the device model.
    static var cloudRefused = false

    static func asksForChange(_ q: String) -> Bool {
        q.range(of: #"\b(tag|tags|tagging|collection|organi[sz]e|mark (them|these|those|as)|add (it|them|these|this)|propose)\b"#,
                options: [.regularExpression, .caseInsensitive]) != nil
    }

    /// Open "thinking" models may stream a <think>…</think> trace before the answer; show only the answer.
    static func withoutThinking(_ s: String) -> String {
        guard s.contains("<think>") else { return s }
        if let end = s.range(of: "</think>") { return String(s[end.upperBound...]).trimmingCharacters(in: .whitespacesAndNewlines) }
        return ""
    }

    static func isOverflow(_ e: Error) -> Bool {
        if let g = e as? LanguageModelSession.GenerationError, case .exceededContextWindowSize = g { return true }
        let d = String(describing: e)
        return d.contains("contextSizeExceeded") || d.contains("exceededContextWindowSize") || d.contains("maximum allowed")
    }

    private func fail(_ error: Error) {
        if Self.isOverflow(error) {
            messages.append(AskMessage(role: .notice, text: "That needs more reading than Apple's on-device model can hold. Try a narrower question."))
            return
        }
        if Task.isCancelled { return }
        var text = error.localizedDescription
        if let e = error as? LanguageModelSession.GenerationError, case .guardrailViolation = e {
            text = "Apple's model declined to answer that (its safety filter). Try rephrasing."
        }
        messages.append(AskMessage(role: .notice, text: text))
    }

    private func stream(_ q: String) async throws {
        guard let session else { return }
        ctx?.newQuestion()
        let i = messages.count
        messages.append(AskMessage(role: .assistant, text: ""))
        for try await snap in session.streamResponse(to: q, options: GenerationOptions(temperature: 0.2)) {
            if Task.isCancelled { break }
            messages[i].text = Self.withoutThinking(snap.content)
        }
        if messages[i].text.trimmingCharacters(in: .whitespaces).isEmpty { messages.remove(at: i) }
    }

    private func start() async throws {
        guard let model else { return }
        let items = await passages?() ?? []
        let lib = model.lib
        let emit: @Sendable (AskEvent) -> Void = { [weak self] e in Task { @MainActor in self?.handle(e) } }
        let fulltext: @Sendable (String) async -> String? = { [weak model] id in
            guard let url = await model?.ensureLocal(id, .fulltext) else { return nil }
            return try? String(contentsOf: url, encoding: .utf8)
        }
        let earlier = Array(sources.values)
        let c = await Task.detached { AskContext(lib: lib, passages: items, earlier: earlier, emit: emit, fulltext: fulltext) }.value
        ctx = c
        c.budget = budget + 1600
        let tools: [any Tool]
        var instructions: String
        switch scope {
        case .paper(let id):
            tools = [FindInPaperTool(ctx: c), ReadSectionTool(ctx: c), SearchLibraryTool(ctx: c)]
            instructions = paperInstructions(model.paper(id), items: items)
        case .library:
            tools = [SearchLibraryTool(ctx: c), ListPapersTool(ctx: c), PaperDetailsTool(ctx: c),
                     SearchInsidePaperTool(ctx: c), FindNewPapersTool(ctx: c), ProposeChangeTool(ctx: c)]
            instructions = libraryInstructions(model)
        }
        // picked up after a relaunch: give the new session the gist of what was said
        let history = messages.dropLast().contains { $0.role == .assistant }
        if restored, history { instructions += earlierConversation(limit: budget / 2) }
        restored = false
        usingCloud = false
        if #available(macOS 27.0, iOS 27.0, *), usesOpenModel {
            await OpenModel.shared.prepare()
            session = LanguageModelSession(model: OpenModel.shared.model, tools: tools, instructions: instructions)
        } else if #available(macOS 27.0, iOS 27.0, *), cloudAvailable {
            session = LanguageModelSession(model: PrivateCloudComputeLanguageModel(), tools: tools, instructions: instructions)
            usingCloud = true
        } else {
            session = LanguageModelSession(tools: tools, instructions: instructions)
        }
    }

    private func handle(_ e: AskEvent) {
        switch e {
        case .activity(let t):
            NSLog("Margins Ask: %@", t)
            messages.append(AskMessage(role: .activity, text: t))
        case .source(let s):
            sources[s.id] = s
        case .proposal(let p):
            proposals.append(p)
        }
    }

    private var budget: Int { usingCloud || cloudAvailable ? 9000 : usesOpenModel ? 6000 : 2600 }

    private func paperInstructions(_ p: Paper?, items: [NarrationItem]) -> String {
        let title = p?.meta.title ?? "this paper"
        let who = p.map { "\($0.meta.authors.first ?? "")\($0.meta.authors.count > 1 ? " et al." : ""), \($0.meta.date.prefix(4)), \($0.meta.journal)" } ?? ""
        let headings = items.filter { $0.kind == "heading" }.map(\.text).prefix(25).joined(separator: " · ")
        let abstract = digitize(clip(p?.meta.abstractText ?? "", budget / 2))
        return """
        You are the reading assistant in Margins, helping a researcher understand one scientific paper:
        “\(title)” (\(who)).
        Answer from the paper itself. Use find_in_paper or read_section to look up the passages you need \
        before answering anything specific; do not answer specifics from memory. Each passage has a number; cite \
        what you use like [2] or [2][5]. Copy numbers exactly as they appear in the passages and never state a number \
        that isn't in them; if the passages you found don't answer the question, look again with other words or say \
        the paper doesn't say. search_library searches the user's \
        other papers. Be concise and precise; use short paragraphs or bullet points; keep numbers and units exact.
        Sections: \(headings.isEmpty ? "unknown" : headings)
        Abstract: \(abstract.isEmpty ? "(none)" : abstract)
        """
    }

    private func libraryInstructions(_ m: LibraryModel) -> String {
        let tags = m.tags.prefix(40).joined(separator: ", ")
        let cols = m.collections.prefix(30).map(\.name).joined(separator: ", ")
        return """
        You are the research assistant in Margins, the user's reference manager. The library has \(m.counts.all) papers \
        (\(m.counts.unread) unread, \(m.counts.starred) starred). Tags: \(tags.isEmpty ? "none" : tags). Collections: \(cols.isEmpty ? "none" : cols).
        Always use tools to look things up; never invent papers, findings or citations. Tool results number each paper; \
        cite them like [3], once per paper, right after its title. Papers from find_new_papers are NOT in the library: always call them new papers beyond the \
        library. You cannot change the library yourself: when the user asks to tag, collect, mark or add papers, call \
        propose_change right away (don't ask first); the app shows the user a button to confirm it. \
        Be concise; use short bullet lists of papers with their numbers.
        """
    }

    /// Runs one question to completion and returns the transcript (for the -ask launch hook).
    func transcript(for q: String) async -> String {
        ask(q)
        while busy { try? await Task.sleep(for: .milliseconds(200)) }
        let src = sources.keys.sorted().map { "  [\($0)] \(sources[$0]!.label)" }.joined(separator: "\n")
        return (["model: \(modelLabel)"] + messages.map { "\($0.role): \($0.text)" } + proposals.map { "proposal: \($0.summary)" }).joined(separator: "\n") + "\nsources:\n" + src
    }

    // MARK: quick actions

    func summarize() {
        Task {
            guard let digest = await digest() else { return ask("Summarize this paper.") }
            ask("Here are excerpts from the paper, in order:\n\n\(digest)\n\nSummarize the paper for a researcher in 5–7 bullet points: the question, the data and methods, the main findings with key numbers, and the main limitation. Cite excerpts by number.",
                shown: "Summarize this paper")
        }
    }

    func explainSelection() {
        Task {
            guard let s = await selection?(), !s.trimmingCharacters(in: .whitespaces).isEmpty else {
                messages.append(AskMessage(role: .notice, text: "Select some text in the paper first."))
                return
            }
            ask("Explain this passage from the paper in plain language, including any jargon, and say why it matters for the paper:\n\n“\(clip(s, budget / 2))”",
                shown: "Explain: “\(clip(s, 140))”")
        }
    }

    /// Opening sentences of each section, numbered as citable passages, within the model's budget.
    private func digest() async -> String? {
        if session == nil { try? await start() }
        guard let ctx, !ctx.passages.isEmpty else { return nil }
        let items = ctx.passages
        let paras = items.filter { $0.kind == "text" }
        guard !paras.isEmpty else { return nil }
        let per = max(160, budget / max(1, min(paras.count, 30)))
        var out: [String] = [], used = 0
        var lastHeading = ""
        for it in items {
            if it.kind == "heading" { lastHeading = it.text; continue }
            guard it.kind == "text" else { continue }
            let low = lastHeading.lowercased()
            if low.contains("reference") || low.contains("acknowledg") || low.contains("funding") || low.contains("author contrib") { continue }
            let n = ctx.register(.passage(it.id), clip(it.text, 80))
            let t = "[\(n)] " + digitize(clip(it.text, per))
            out.append(t)
            used += t.count
            if used > budget { break }
        }
        return out.joined(separator: "\n")
    }

    // MARK: confirming proposals

    func apply(_ p: ProposedChange) async {
        guard let model, let i = proposals.firstIndex(where: { $0.id == p.id }) else { return }
        let lib = model.lib
        do {
            switch p.kind {
            case .tag(let tag, let ids):
                for id in ids { try lib.setTag(id: id, tag: tag, present: true) }
            case .collection(let name, let ids):
                let col = try lib.collections().first(where: { $0.name.caseInsensitiveCompare(name) == .orderedSame })
                    ?? lib.createCollection(name: name, parent: nil)
                for id in ids { try lib.setMembership(collection: col.id, paper: id, present: true) }
            case .markRead(let ids, let read):
                for id in ids { try lib.setRead(id: id, read: read) }
            case .add(let n):
                guard let hit = ctx?.hit(n) else {
                    proposals[i].state = .failed
                    messages.append(AskMessage(role: .notice, text: "That search result is from an earlier session. Ask again to find it, then add it."))
                    save()
                    return
                }
                if let id = await model.addDiscovered(hit) {
                    let c = ctx?.register(.paper(id), hit.meta.title)
                    _ = c
                }
            }
            proposals[i].state = .applied
            model.reload()
            save()
        } catch {
            proposals[i].state = .failed
            messages.append(AskMessage(role: .notice, text: "Couldn't apply: \(error.localizedDescription)"))
        }
    }

    func dismiss(_ p: ProposedChange) {
        if let i = proposals.firstIndex(where: { $0.id == p.id }) { proposals[i].state = .dismissed }
        save()
    }
}
