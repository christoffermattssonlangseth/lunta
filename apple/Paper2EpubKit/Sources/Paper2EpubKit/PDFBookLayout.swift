import Foundation
import PDFKit

// Book mode (DocumentKind.book / .thesis): what a book has and a paper hasn't. Running heads that
// change with the chapter and carry the page number, a printed table of contents, an index, and
// an outline (bookmarks) that gives the chapters. Papers never come here.

extension PDFAnalyzer {
    /// Decides paper or book from the lines read and the document, and in book mode finds the
    /// running heads, the table of contents and the index.
    func decideKind() {
        let entries = Self.outline(doc)
        outline = entries
        (kind, kindReasons) = DocumentClassifier.classify(kindSignals(workType: workType))
        log("Document kind: \(kind.rawValue) (\(kindReasons.joined(separator: "; ")))")
        guard kind.isBookLike else { outline = []; return }
        findBookRunning()
        findTOCPages()
        findIndex()
    }

    /// A margin line without its page number, roman or arabic, for comparing heads across pages.
    func headKey(_ t: String) -> String {
        norm(t).replacingOccurrences(of: #"^[ivxlcdm]{1,7}\s+|\s+[ivxlcdm]{1,7}$"#, with: "", options: .regularExpression)
    }

    /// Running heads and feet of a book: the top line (or the bottom line) of a page when it carries
    /// the printed page number where the body pages carry it (page index minus a fixed offset), or
    /// when the same words stand there on a page close by. Books change their heads with the
    /// chapter or section ("28 3 Shallow neural networks" on the left, "3.1 Neural network example
    /// 29" on the right), which the paper rule (the same words on a third of all pages) misses.
    /// Linear in the pages.
    func findBookRunning() {
        let n = pagesLines.count
        // (a page's top or bottom line with the pieces on its baseline: PDFKit may give the page number of
        // "406 20 Why does deep learning work?" as a line of its own)
        var cands: [[(lines: [PLine], text: String)]] = Array(repeating: [], count: n)
        for pno in 0..<n {
            let h = boxes[pno].height
            if let top = marginLine(pno, top: true), top.lines[0].bbox.y0 < 0.2 * h { cands[pno].append(top) }
            if topDown(pno).count > 1, let bottom = marginLine(pno, top: false), bottom.lines[0].bbox.y1 > 0.8 * h,
               !(cands[pno].first.map { $0.lines.map(ObjectIdentifier.init) == bottom.lines.map(ObjectIdentifier.init) } ?? false) {
                cands[pno].append(bottom)
            }
        }
        // the offset between the page index and the printed number
        var offsets: [Int: Int] = [:]
        for pno in 0..<n {
            for c in cands[pno] { if let k = Self.headNumber(c.text) { offsets[pno + 1 - k, default: 0] += 1 } }
        }
        let best = offsets.max { ($0.value, -$0.key) < ($1.value, -$1.key) }
        let offset = best.flatMap { $0.value >= max(4, n / 10) ? $0.key : nil }
        let keys = cands.map { Set($0.map { headKey($0.text) }.filter { !$0.isEmpty }) }
        var count = 0
        for pno in 0..<n {
            for c in cands[pno] {
                let t = c.text.trimmingCharacters(in: .whitespaces)
                // (a chapter's title and its "Chapter 3" label stand there too, in larger type)
                guard t.count <= 150, c.lines.allSatisfy({ $0.size <= bodySize + 1 }), !Self.chapterLabelRE.test(t) else { continue }
                var running = false
                if let offset, let k = Self.headNumber(t), pno + 1 - k == offset { running = true }
                if !running {
                    let key = headKey(t)
                    if !key.isEmpty {
                        for d in [-4, -3, -2, -1, 1, 2, 3, 4] where pno + d >= 0 && pno + d < n && keys[pno + d].contains(key) {
                            running = true; break
                        }
                    }
                }
                if running {
                    for l in c.lines { dropped.insert(ObjectIdentifier(l)) }
                    count += 1
                    if ProcessInfo.processInfo.environment["P2E_DEBUG"] != nil { print("running p\(pno + 1): \(t)") }
                }
            }
        }
        log("Book: \(count) running heads and feet\(offset.map { ", printed page = page − \($0)" } ?? "")")
    }

    /// The top (or bottom) line of a page with the pieces level with it, left to right, and their text.
    func marginLine(_ pno: Int, top: Bool) -> (lines: [PLine], text: String)? {
        let ls = topDown(pno)
        guard let first = top ? ls.first : ls.last else { return nil }
        let fb = first.bbox
        let same = ls.filter { l in min(l.bbox.y1, fb.y1) - max(l.bbox.y0, fb.y0) > 0.5 * min(l.bbox.h, fb.h) }
            .sorted { $0.bbox.x0 < $1.bbox.x0 }
        // (the top or bottom line first: its position is checked)
        return ([first] + same.filter { $0 !== first }, same.map { $0.text.trimmingCharacters(in: .whitespaces) }.joined(separator: " "))
    }

    /// The printed table of contents (and lists of figures and tables): pages near the front with
    /// lines in dot leaders and page numbers, and the pages that go on with them. Left out: the
    /// book's own outline (or its headings) gives the contents.
    func findTOCPages() {
        let n = pagesLines.count
        // the outline's own "Contents" entries: their pages, to the next entry
        for (k, e) in outline.enumerated() where Self.isContentsTitle(e.title) {
            let end = outline[(k + 1)...].first { $0.page > e.page }?.page ?? e.page + 1
            if end - e.page <= max(40, n / 10) { for pno in e.page..<end { tocPages.insert(pno) } }
        }
        var prev = false
        for pno in 0..<min(n, max(40, n / 8)) {
            let ls = topDown(pno).filter { !dropped.contains(ObjectIdentifier($0)) }
            guard ls.count >= 3 else { prev = false; continue }
            let leaders = ls.filter { isTOCLine($0.text) }.count
            let numbered = ls.filter { Self.endNumberRE.test($0.text.trimmingCharacters(in: .whitespaces)) }.count
            let headed = ls.prefix(3).contains { Self.isContentsTitle($0.text) }
            let toc = (leaders >= 3 && Double(numbered) >= 0.4 * Double(ls.count))
                || ((prev || headed) && ls.count >= 4 && Double(numbered) >= 0.5 * Double(ls.count))
            if toc { tocPages.insert(pno) }
            prev = toc || tocPages.contains(pno)
        }
        for pno in tocPages { for l in pagesLines[pno] { dropped.insert(ObjectIdentifier(l)) } }
        if !tocPages.isEmpty { log("Book: table of contents on page(s) \(tocPages.sorted().map { String($0 + 1) }.joined(separator: ", ")) left out") }
    }

    /// The index: from the outline's "Index" entry (to the next top-level entry), or from a page
    /// headed "Index" among the last pages to the end.
    static func isContentsTitle(_ t: String) -> Bool {
        t.trimmingCharacters(in: .whitespaces).matches(#"(?i)^((table of )?contents( in brief)?|brief contents|contents at a glance)$"#)
    }

    static func isIndexTitle(_ t: String) -> Bool {
        t.trimmingCharacters(in: .whitespaces).matches(#"(?i)^(subject |general |author |name )?index$"#)
    }

    func findIndex() {
        let n = pagesLines.count
        for (k, e) in outline.enumerated() where Self.isIndexTitle(e.title) {
            let end = outline[(k + 1)...].first { $0.level <= e.level && $0.page > e.page }?.page ?? n
            indexPages.append(e.page..<max(e.page + 1, end))
        }
        if indexPages.isEmpty {
            for pno in max(0, n - max(30, n / 6))..<n {
                let top = topDown(pno).filter { !dropped.contains(ObjectIdentifier($0)) }.prefix(3)
                if top.contains(where: { Self.isIndexTitle($0.text) }) { indexPages = [pno..<n]; break }
            }
        }
        for r in indexPages { log("Book: index on pages \(r.lowerBound + 1)–\(r.upperBound)") }
    }

    func onIndexPage(_ pno: Int) -> Bool { indexPages.contains { $0.contains(pno) } }
}

// MARK: - Assembly

extension PDFBook {
    /// The chapters of a book: from its outline (top level = chapter, second level = section in the
    /// navigation, third = sub-heading), or, without one, from the headings found. The headings on
    /// the pages that the outline's entries stand for are left out (the entry carries them), with
    /// the number they are printed with ("1.1 Supervised learning", "3 Shallow neural networks").
    struct BookPlan {
        var entries: [OutlineEntry]
        var titles: [String]
        /// paragraphs that print an entry's title (or its "Chapter 3" label)
        var consumed: Set<Int>
    }

    static let numberedTitleRE = try! NSRegularExpression(pattern: #"^((?:\d{1,3}|[A-Z])(?:\.\d{1,3})*)\.?\s+(\S.*)$"#)

    /// Whether paragraph `p` lies before outline entry `e` in reading order.
    static func before(_ p: Para, _ e: OutlineEntry) -> Bool {
        p.page < e.page || (p.page == e.page && p.bbox.y1 < e.y + 2)
    }

    /// A paragraph's text as a title: compatibility forms ("coeﬀicients"), the "1.2 " or
    /// "Chapter 3" before it left out.
    static func titleKey(_ t: String) -> String {
        var s = t.precomposedStringWithCompatibilityMapping.trimmingCharacters(in: .whitespaces)
        let ns = s as NSString
        if let m = numberedTitleRE.firstMatch(in: s, range: NSRange(location: 0, length: ns.length)) { s = ns.substring(with: m.range(at: 2)) }
        return s.lowercased().replacingOccurrences(of: #"\W+"#, with: "", options: .regularExpression)
    }

    /// The same title, allowing a letter or two in long ones ("Acknowledgments" for "Acknowledgements").
    static func sameTitle(_ text: String, _ title: String) -> Bool {
        let a = titleKey(text), b = titleKey(title)
        guard !a.isEmpty, !b.isEmpty, abs(a.count - b.count) <= 2 else { return false }
        if a == b { return true }
        guard b.count >= 8 else { return false }
        // edit distance, at most 2
        let x = Array(a), y = Array(b)
        var row = Array(0...y.count)
        for i in 1...x.count {
            var prev = row[0]; row[0] = i
            for j in 1...y.count {
                let cur = row[j]
                row[j] = min(row[j] + 1, row[j - 1] + 1, prev + (x[i - 1] == y[j - 1] ? 0 : 1))
                prev = cur
            }
        }
        return row[y.count] <= (b.count >= 12 ? 2 : 1)
    }

    static func plan(_ paras: [Para], outline: [OutlineEntry]) -> BookPlan {
        var byPage: [Int: [Int]] = [:]
        for (i, p) in paras.enumerated() { byPage[p.page, default: []].append(i) }
        func printed(_ i: Int, _ title: String) -> Bool {
            let p = paras[i]
            let t = p.text.trimmingCharacters(in: .whitespaces)
            return [.heading, .text, .title, .front].contains(p.kind) && t.count < 200 && sameTitle(t, title)
        }
        // a chapter whose bookmark points to the wrong page (hyperref with \chapter*: "Preface" on the
        // table of contents): to the page that prints its title, within the next eight pages
        // An entry goes to where its title is printed: on its page (the match nearest the point it
        // gives; a destination without one points to the top of the page), or for a chapter within
        // the next eight pages.
        var outline = outline
        for k in outline.indices {
            let e = outline[k]
            let last = e.level == 1 ? e.page + 8 : e.page + 1
            for pg in e.page...last {
                let hits = (byPage[pg] ?? []).filter { printed($0, e.title) }
                guard !hits.isEmpty else { continue }
                let i = pg == e.page ? hits.min { abs(paras[$0].bbox.y0 - e.y) < abs(paras[$1].bbox.y0 - e.y) }! : hits[0]
                outline[k].page = pg; outline[k].y = max(0, pg == e.page && e.y > 0 ? min(e.y, paras[i].bbox.y0 - 1) : paras[i].bbox.y0 - 1)
                break
            }
        }
        outline = outline.enumerated().sorted { ($0.element.page, $0.element.y, $0.offset) < ($1.element.page, $1.element.y, $1.offset) }
            .map(\.element)
        var titles = outline.map(\.title)
        var consumed = Set<Int>()
        var k = 0
        for (ei, e) in outline.enumerated() {
            while k < paras.count && before(paras[k], e) { k += 1 }
            var label: String?
            var j = k
            while j < paras.count && j < k + 8 && paras[j].page <= e.page + 1 {
                if ei + 1 < outline.count && !before(paras[j], outline[ei + 1]) && j > k { break }
                let p = paras[j]
                let t = p.text.trimmingCharacters(in: .whitespaces)
                guard [.heading, .text, .title, .front].contains(p.kind), t.count < 200 else { j += 1; continue }
                if let m = PDFAnalyzer.chapterLabelRE.firstMatch(in: t, range: NSRange(t.startIndex..., in: t)),
                   let r = Range(m.range(at: 1), in: t) {
                    label = String(t[r]); consumed.insert(j); j += 1; continue
                }
                let ns = t as NSString
                if numberedTitleRE.firstMatch(in: t, range: NSRange(location: 0, length: ns.length)) != nil, sameTitle(t, e.title) {
                    titles[ei] = t.precomposedStringWithCompatibilityMapping; consumed.insert(j); break
                }
                if sameTitle(t, e.title) {
                    if let label { titles[ei] = "\(label) \(e.title)" }
                    consumed.insert(j); break
                }
                j += 1
            }
        }
        return BookPlan(entries: outline, titles: titles, consumed: consumed)
    }

    /// The authors of a book from its title page: the short line of a name or two under the title.
    static func titlePageAuthors(_ paras: [Para]) -> [String] {
        guard let ti = paras.firstIndex(where: { $0.kind == .title }) else { return [] }
        for p in paras[(ti + 1)...].prefix(3) where p.page == paras[ti].page {
            let t = p.text.trimmingCharacters(in: .whitespaces)
            if t.matches(#"^(?:[A-Z][\p{L}.'’-]*\.?\s+){1,4}[A-Z][\p{L}'’-]+(?:\s*(?:,|and|&)\s*(?:[A-Z][\p{L}.'’-]*\.?\s+){1,4}[A-Z][\p{L}'’-]+)*$"#) {
                return t.components(separatedBy: #","#).flatMap { $0.components(separatedBy: " and ") }
                    .flatMap { $0.components(separatedBy: "&") }
                    .map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }
            }
        }
        return []
    }
}
