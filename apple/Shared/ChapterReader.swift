import SwiftUI
import WebKit
import LuntaCore

// Big reading views (a textbook converted as a book: 29 chapters, ~200,000 words, ~700 images)
// are read chapter by chapter. As one page, WebKit had to lay out (and in pages, cut into
// columns) the whole book, and on an iPhone its web process ran out of memory: the reader went
// black. The core decides (`Library.readingParts`: more than a few sections and 60,000 words,
// 150 images or 30 MB of images) and writes each part (a chapter, or a few short sections
// together) as a page of its own, next to the continuous one. Papers are shown as before.
//
// A part's text is exactly a slice of the continuous view's text, starting at its
// `lunta-text-base` (a meta tag in the part). The page scripts add that to every offset they
// give the app and take it off what they get (Highlights.swift, ReadingPosition.swift), so
// highlights and reading positions are stored as before, in the whole view's text: a highlight
// made in chapter 5 on the iPhone is found by the Mac (both read books chapter by chapter, and
// either way the offset is the same), old highlights and places keep working, and the place
// saved on one device opens the right chapter on the other.
//
// What crosses chapters: the contents and links open the chapter (`go`); turning past a
// chapter's last page (or before its first) opens the next (previous) one, as does the
// "Next: …" link at its end; Find searches the whole book (`BookFind`: counted in the parts'
// texts, found in the page of the part it is in); Listen goes on into the next chapter (the
// readers); citations find their references in the reference list's chapter (`luntaSetShelf`,
// CitationPopover.swift).

@MainActor
final class ChapterReader: ObservableObject {
    let book: EpubBook
    let parts: [ReadingPart]
    /// The part holding the reference list.
    let bibliography: Int?
    /// The part shown.
    @Published private(set) var current: Int { didSet { controller?.part = current } }
    /// Pages loaded so far (one more at each `pageReady`).
    private(set) var generation = 0
    weak var controller: ReaderController?
    /// Called before another part opens: the place it will be at (in the whole view's text), and
    /// whether to go there once it has loaded (PositionKeeper.partChanged).
    var willOpen: ((Int, Bool) -> Void)?
    /// What to do once the part being opened is ready.
    private var after: After = .none
    private var waiting: [CheckedContinuation<Bool, Never>] = []
    private var lastEdge = Date.distantPast
    /// Highlight id → the part it is in (nil: in none), with what it was placed by.
    private var placed: [String: (key: String, part: Int?)] = [:]
    private var squashed: [String?]
    private var dense: [String?]
    private var folded: [String]?
    private var shelfText: String?

    enum After: Equatable {
        case none, top, end
        case offset(Int)
        case anchor(String)
        case reference(Int)
        case annotation(String)
    }

    /// nil for a reading view shown as one page.
    init?(book: EpubBook, parts info: ReadingParts, at offset: Int?) {
        guard info.chaptered, !info.parts.isEmpty else { return nil }
        self.book = book
        parts = info.parts
        bibliography = info.bibliography.map(Int.init)
        current = offset.map { Self.part(forOffset: $0, in: info.parts) } ?? 0
        squashed = Array(repeating: nil, count: info.parts.count)
        dense = Array(repeating: nil, count: info.parts.count)
    }

    /// The page to show.
    var url: URL { URL(fileURLWithPath: parts[current].path) }

    // MARK: where things are

    /// The part holding an offset in the whole view's text.
    nonisolated static func part(forOffset o: Int, in parts: [ReadingPart]) -> Int {
        var k = 0
        for (i, p) in parts.enumerated() where Int(p.start) <= o { k = i }
        return k
    }
    func part(forOffset o: Int) -> Int { Self.part(forOffset: o, in: parts) }

    /// The offset is in the part shown.
    func holds(_ o: Int) -> Bool {
        let p = parts[current]
        return o >= Int(p.start) && (o < Int(p.start + p.length) || current == parts.count - 1)
    }

    func part(forSection i: Int) -> Int? { parts.firstIndex { $0.sections.contains(UInt32(i)) } }

    /// The part a file is (a part's page) or is in (a section of the book).
    func part(forFile path: String) -> Int? {
        let name = (path as NSString).lastPathComponent
        if let k = parts.firstIndex(where: { ($0.path as NSString).lastPathComponent == name }) { return k }
        if let i = book.chapters.firstIndex(where: { ($0.path as NSString).lastPathComponent == name }) { return part(forSection: i) }
        return nil
    }

    /// The part holding an element id the contents point at ("section-<n>", or a heading's id).
    func part(forAnchor a: String) -> Int? {
        if a.hasPrefix("section-"), let i = Int(a.dropFirst(8)) { return part(forSection: i) }
        if let t = book.toc.first(where: { $0.target.hasSuffix("#" + a) }),
           let file = t.target.split(separator: "#", maxSplits: 1).first {
            return part(forFile: String(file))
        }
        return nil
    }

    // MARK: going to another part

    /// Opens part `k`, then does `a` there; returns once its page is ready (false: it didn't load in time).
    @discardableResult
    func open(_ k: Int, then a: After) async -> Bool {
        guard parts.indices.contains(k) else { return false }
        if k == current { perform(a); return true }
        after = a
        let p = parts[k]
        let start = Int(p.start), end = max(start, Int(p.start + p.length) - 2)
        switch a {
        case .offset(let o): willOpen?(o, true)
        case .end: willOpen?(end, true)
        default: willOpen?(start, false)
        }
        current = k
        let serial = generation
        return await withCheckedContinuation { c in
            waiting.append(c)
            Task { [weak self] in
                try? await Task.sleep(for: .seconds(12))
                guard let self, self.generation == serial else { return }
                self.resumeWaiting(false)
            }
        }
    }

    func go(_ k: Int, then a: After) { Task { await open(k, then: a) } }

    /// The contents (or the Mac's sidebar) chose an anchor: true if its part is another one, which opens.
    func show(anchor a: String) -> Bool {
        guard let k = part(forAnchor: a), k != current else { return false }
        go(k, then: .anchor(a))
        return true
    }

    /// Turned past the end of the part's last page (1) or before its first (-1).
    func edge(_ d: Int) {
        guard Date().timeIntervalSince(lastEdge) > 1.2 else { return }
        lastEdge = Date()
        if d > 0, current + 1 < parts.count { go(current + 1, then: .top) }
        else if d < 0, current > 0 { go(current - 1, then: .end) }
    }

    /// A link in the page to another part's file (`lunta-part-3.xhtml#s30`): true when it is one
    /// (it opens here, instead of the web view going there by itself).
    func open(link url: URL) -> Bool {
        guard url.isFileURL, let k = part(forFile: url.path) else { return false }
        if k == current, (url.path as NSString).lastPathComponent == (parts[k].path as NSString).lastPathComponent {
            return false   // this page: an anchor in it, or the load the reader asked for
        }
        let a: After
        if let f = url.fragment, !f.isEmpty {
            if f.hasPrefix("lunta-ref-"), let i = Int(f.dropFirst(10)) { a = .reference(i) } else { a = .anchor(f) }
        } else { a = .top }
        go(k, then: a)
        return true
    }

    /// The part's page has loaded (the reader's onPageReady, after the place is restored).
    func pageReady() {
        generation += 1
        // the reference list, for the citations in this chapter
        if let b = bibliography, b != current, let v = controller?.webView {
            if shelfText == nil { shelfText = try? String(contentsOfFile: parts[b].path, encoding: .utf8) }
            if let t = shelfText, let d = try? JSONSerialization.data(withJSONObject: [t, (parts[b].path as NSString).lastPathComponent]),
               let args = String(data: d, encoding: .utf8) {
                v.evaluateJavaScript("window.luntaSetShelf && luntaSetShelf.apply(null, \(args))")
            }
        }
        controller?.partLoads += 1
        let a = after
        after = .none
        if a != .none {
            // once the pages are laid out (and after the place keeper's own move)
            Task { [weak self] in
                try? await Task.sleep(for: .milliseconds(450))
                self?.perform(a)
            }
        }
        resumeWaiting(true)
    }

    private func resumeWaiting(_ ok: Bool) {
        let w = waiting
        waiting = []
        w.forEach { $0.resume(returning: ok) }
    }

    private func perform(_ a: After) {
        guard let c = controller, let v = c.webView else { return }
        switch a {
        case .none: break
        case .top: v.evaluateJavaScript("if (window.luntaPaged && luntaPaged()) luntaGoToPage(0, false); else window.scrollTo(0, 0);")
        case .end:
            let p = parts[current]
            c.scroll(toOffset: max(Int(p.start), Int(p.start + p.length) - 2))
        case .offset(let o): c.scroll(toOffset: o)
        case .anchor(let x): v.evaluateJavaScript(ReaderAssets.scrollJS(x))
        case .reference(let i): v.evaluateJavaScript("window.luntaShowReference && luntaShowReference(\(i))")
        case .annotation(let id): c.showAnnotation(id)
        }
    }

    // MARK: highlights

    /// The highlights in the part shown, and those in no part at all (missing). A highlight's part:
    /// the one its offset is in if its passage is there (exactly, or with other spacing); else any
    /// part where the passage is (made in a reading view made again, or in the PDF); else, made
    /// in the reading view, the part its offset is in (the page's looser matching may still find it).
    func split(_ all: [Annotation]) -> (here: [Annotation], unplaced: Set<String>) {
        var here: [Annotation] = [], unplaced = Set<String>()
        for a in all {
            let key = "\(a.location)|\(a.position)|\(a.quote)"
            let k: Int?
            if let p = placed[a.id], p.key == key { k = p.part }
            else { k = place(a); placed[a.id] = (key, k) }
            if k == current { here.append(a) } else if k == nil { unplaced.insert(a.id) }
        }
        return (here, unplaced)
    }

    func part(ofAnnotation id: String) -> Int? { placed[id]?.part ?? nil }

    private func place(_ a: Annotation) -> Int? {
        let at = a.location == "epub" ? Int(a.position) : nil
        let q = a.quote
        guard !q.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return at.map(part(forOffset:)) }
        if let at {
            let k = part(forOffset: at)
            if has(k, q) { return k }
        }
        if let k = parts.indices.first(where: { has($0, q) }) { return k }
        if let at { return part(forOffset: at) }
        // made in the PDF: spacing, case, quotes and dashes may differ
        let d = Self.dense(q)
        guard d.count >= 4 else { return nil }
        let probe = String(d.prefix(100))
        return parts.indices.first { k in
            if dense[k] == nil { dense[k] = Self.dense(parts[k].text) }
            return dense[k]!.contains(probe)
        }
    }

    private func has(_ k: Int, _ q: String) -> Bool {
        if parts[k].text.contains(q) { return true }
        let sq = Self.squash(q)
        guard sq.count >= 4 else { return false }
        if squashed[k] == nil { squashed[k] = Self.squash(parts[k].text) }
        return squashed[k]!.contains(sq)
    }

    nonisolated static func squash(_ s: String) -> String {
        s.split(whereSeparator: \.isWhitespace).joined(separator: " ")
    }

    nonisolated static func dense(_ s: String) -> String {
        let skip = CharacterSet.whitespacesAndNewlines.union(CharacterSet(charactersIn: "'\"\u{2018}\u{2019}\u{201A}\u{201B}\u{201C}\u{201D}\u{201E}\u{201F}\u{00B4}`\u{00AB}\u{00BB}"))
        var out = String.UnicodeScalarView()
        for u in s.lowercased().unicodeScalars where !skip.contains(u) {
            switch u.value {
            case 0x00B5: out.append("\u{03BC}")
            case 0x2010...0x2014, 0x2212: out.append("-")
            default: out.append(u)
            }
        }
        return String(out)
    }

    // MARK: find in the whole book

    /// How often each part's text has the query, folded the way the page's find folds it (case,
    /// accents, quotes, dashes and spacing don't count). The page's own count wins for a part it
    /// has searched (they agree but for rare details).
    func counts(of query: String) -> [Int] {
        if folded == nil { folded = parts.map { Self.fold($0.text) } }
        let q = Self.fold(query)
        guard !q.isEmpty else { return parts.map { _ in 0 } }
        return folded!.map { t in
            var n = 0, from = t.startIndex
            while let r = t.range(of: q, options: .literal, range: from..<t.endIndex) {
                n += 1
                from = r.upperBound
                if n >= 5000 { break }
            }
            return n
        }
    }

    nonisolated static func fold(_ s: String) -> String {
        let f = s.folding(options: [.caseInsensitive, .diacriticInsensitive, .widthInsensitive], locale: nil)
        var out = String.UnicodeScalarView(), space = true
        for u in f.unicodeScalars {
            var c = u
            switch u.value {
            case 0x00AD, 0x200B...0x200D, 0x2060, 0xFEFF: continue
            case 0x2018...0x201B, 0x2032: c = "'"
            case 0x201C...0x201F, 0x2033, 0x00AB, 0x00BB: c = "\""
            case 0x2010...0x2015, 0x2212, 0xFE58, 0xFE63, 0xFF0D: c = "-"
            default: break
            }
            if CharacterSet.whitespacesAndNewlines.contains(c) {
                if space { continue }
                space = true
                out.append(" ")
            } else {
                space = false
                out.append(c)
            }
        }
        if out.last == " " { out.removeLast() }
        return String(out)
    }
}

/// Find in a book read chapter by chapter: the whole book. The matches are counted in every
/// part's text; the page finds and marks those of the part shown, and stepping past its last
/// (first) match opens the next (previous) part that has one.
@MainActor
final class BookFind: FindBackend {
    private weak var chapters: ChapterReader?
    private weak var controller: ReaderController?
    private var query = ""
    private var counts: [Int] = []
    /// The page's last answer (its own matches), in which part, and which page load it was.
    private var last = FindResult()
    private var part = -1
    private var generation = -1

    init(chapters: ChapterReader, controller: ReaderController) {
        self.chapters = chapters
        self.controller = controller
    }

    func find(_ q: String, fromTop: Bool) async -> FindResult {
        guard let ch = chapters, let c = controller else { return FindResult() }
        query = q
        guard !q.isEmpty else { counts = []; return await c.find("", fromTop: false) }
        counts = ch.counts(of: q)
        var k = ch.current
        var r = await c.find(q, fromTop: fromTop)
        counts[k] = r.count
        if r.count == 0, let n = next(from: k, forward: true) {
            k = n
            r = await findIn(n)
        }
        return remember(r, in: k)
    }

    func findStep(_ forward: Bool) async -> FindResult {
        guard let ch = chapters, let c = controller, !query.isEmpty, counts.count == ch.parts.count else { return FindResult() }
        let k = ch.current
        // another part opened meanwhile (the contents, a link): its matches, from its start
        if k != part || ch.generation != generation {
            var r = await c.find(query, fromTop: true)
            counts[k] = r.count
            if r.count == 0, let n = next(from: k, forward: forward) { r = await findIn(n, last: !forward); return remember(r, in: n) }
            return remember(r, in: k)
        }
        let atEdge = forward ? last.index >= counts[k] - 1 : last.index <= 0
        if atEdge, let n = next(from: k, forward: forward) {
            return remember(await findIn(n, last: !forward), in: n)
        }
        return remember(await c.findStep(forward), in: k)
    }

    func findClear() {
        query = ""
        controller?.findClear()
    }

    func findPlace() async -> FindPlace? { await controller?.findPlace() }

    /// Opens part `n` and finds the query there, at its first match (or its last).
    private func findIn(_ n: Int, last toLast: Bool = false) async -> FindResult {
        guard let ch = chapters, let c = controller else { return FindResult() }
        await ch.open(n, then: .none)
        try? await Task.sleep(for: .milliseconds(300))   // (its pages laid out)
        var r = await c.find(query, fromTop: true)
        if toLast, r.count > 1 { r = await c.findStep(false) }
        counts[n] = r.count
        return r
    }

    /// The next part (forwards or backwards, round the book) with matches, other than `k`.
    private func next(from k: Int, forward: Bool) -> Int? {
        let n = counts.count
        guard n > 1 else { return nil }
        for step in 1..<n {
            let j = ((k + (forward ? step : -step)) % n + n) % n
            if counts[j] > 0 { return j }
        }
        return nil
    }

    private func remember(_ r: FindResult, in k: Int) -> FindResult {
        last = r
        part = k
        generation = chapters?.generation ?? -1
        let total = counts.reduce(0, +)
        let before = counts.prefix(k).reduce(0, +)
        return FindResult(count: total, index: r.index >= 0 ? before + r.index : -1, capped: r.capped || counts.contains { $0 >= 5000 })
    }
}

/// The reading view's navigation: web links open in the browser (ExternalLinks), links to
/// another part of a book read chapter by chapter open that part, and a web process that ended
/// (iOS ends it when the page takes too much memory) is reloaded once; when it ends again soon
/// after, the reader says so instead of showing an empty, black page.
final class ReaderNavigation: NSObject, WKNavigationDelegate {
    weak var controller: ReaderController?
    private var ended: [Date] = []

    init(controller: ReaderController) { self.controller = controller }

    func webView(_ webView: WKWebView, decidePolicyFor action: WKNavigationAction,
                 decisionHandler: @escaping @MainActor @Sendable (WKNavigationActionPolicy) -> Void) {
        if let url = action.request.url, url.isFileURL, action.targetFrame?.isMainFrame != false,
           MainActor.assumeIsolated({ controller?.chapters?.open(link: url) == true }) {
            decisionHandler(.cancel)
            return
        }
        ExternalLinks.shared.webView(webView, decidePolicyFor: action, decisionHandler: decisionHandler)
    }

    func webViewWebContentProcessDidTerminate(_ webView: WKWebView) {
        MainActor.assumeIsolated {
            let now = Date()
            ended = ended.filter { now.timeIntervalSince($0) < 45 } + [now]
            if ended.count >= 2 {
                controller?.pageGone = true
            } else {
                webView.reload()
            }
        }
    }
}

/// The reading view's web process ended twice in a short time (ReaderNavigation): instead of an
/// empty page, why, with Try Again and the PDF.
struct PageGoneView: View {
    let hasPDF: Bool
    let retry: () -> Void
    let showPDF: () -> Void

    var body: some View {
        ContentUnavailableView {
            Label("Couldn’t Show the Reading View", systemImage: "exclamationmark.triangle")
        } description: {
            Text("This reading view is too large to show at once on this device: the page ran out of memory and was closed."
                 + (hasPDF ? " The PDF can be read instead." : ""))
        } actions: {
            if hasPDF { Button("Show PDF", action: showPDF).buttonStyle(.borderedProminent) }
            Button("Try Again", action: retry)
        }
    }
}
