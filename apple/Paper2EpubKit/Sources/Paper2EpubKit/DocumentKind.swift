import Foundation
import PDFKit

/// What a PDF is, which decides how it is laid out as a book: a paper (an article, a preprint, a
/// chapter on its own) or a book (a monograph, a textbook; a thesis is converted like a book).
public enum DocumentKind: String, Sendable {
    case paper, book, thesis

    /// Converted with the book pipeline: chapters from the PDF's outline, running heads per
    /// chapter, no printed table of contents, the index as one chapter, no abstract or affiliations.
    public var isBookLike: Bool { self != .paper }
}

/// The signals the classifier weighs, all cheap to read: the document, its outline, the text of a
/// few pages and the margin lines of every page.
struct KindSignals: Equatable {
    var pages = 0
    /// entries in the PDF's outline (bookmarks), and those at its top level (below a lone root)
    var outlineEntries = 0, outlineTop = 0, outlineDepth = 0
    /// an ISBN in the first or last pages or in the document info
    var isbn = false
    /// a page headed "Contents" / "Table of contents" near the front
    var contentsPage = false
    /// pages of a printed table of contents: lines with dot leaders and page numbers
    var tocPages = 0
    /// pages that open with "Chapter N"
    var chapterPages = 0
    /// an "Index" page near the end, or a "Preface" / "Foreword" page near the front
    var indexPage = false, preface = false
    /// pages whose top or bottom line carries the printed page number with words ("28 3 Shallow
    /// neural networks", "3.1 Neural network example 29"), as a fraction of the pages
    var numberedHeads = 0.0
    /// the work type the library or the metadata gives ("book", "dissertation", "journal-article"…)
    var workType = ""
    /// paper markers on the first pages: an abstract, a DOI line, "Received … Accepted"
    var abstract = false, doiLine = false, received = false
    /// "thesis" / "dissertation" / "submitted in partial fulfilment" on the first pages
    var thesisWords = false
}

enum DocumentClassifier {
    /// The kind, with the signals that decided it (for the log and the conversion result).
    static func classify(_ s: KindSignals) -> (DocumentKind, [String]) {
        var why: [String] = []
        let type = s.workType.lowercased().trimmingCharacters(in: .whitespaces)
        let bookType = ["book", "monograph", "edited-book", "reference-book", "book-set"].contains(type)
        let thesisType = ["dissertation", "thesis", "phdthesis", "mastersthesis"].contains(type)
        let paperType = ["journal-article", "article-journal", "article", "proceedings-article", "paper-conference",
                         "posted-content", "preprint", "book-chapter", "chapter", "review", "letter"].contains(type)
        // short documents are papers, whatever else they show (a 25-page review has a contents box,
        // a chapter has running heads)
        if s.pages < 40 && !(bookType && s.pages >= 20) && !(thesisType && s.pages >= 20) {
            return (.paper, ["\(s.pages) pages"])
        }
        var score = 0
        func add(_ n: Int, _ reason: String) { score += n; why.append("\(n > 0 ? "+" : "")\(n) \(reason)") }
        if bookType { add(4, "work type \(type)") }
        if thesisType { add(4, "work type \(type)") }
        if paperType { add(-3, "work type \(type)") }
        if s.pages >= 150 { add(2, "\(s.pages) pages") } else if s.pages >= 80 { add(1, "\(s.pages) pages") }
        if s.outlineTop >= 5 && s.outlineEntries >= 15 && s.outlineDepth >= 2 {
            add(2, "outline of \(s.outlineTop) chapters, \(s.outlineEntries) entries")
        } else if s.outlineTop >= 8 {
            add(1, "outline of \(s.outlineTop) entries")
        }
        if s.isbn { add(2, "ISBN") }
        if s.tocPages >= 2 { add(2, "\(s.tocPages) table-of-contents pages") }
        else if s.tocPages == 1 || s.contentsPage { add(1, "table of contents") }
        if s.chapterPages >= 3 { add(2, "\(s.chapterPages) \"Chapter N\" pages") }
        if s.indexPage { add(1, "index") }
        if s.preface { add(1, "preface") }
        if s.numberedHeads >= 0.3 { add(1, String(format: "running heads with page numbers on %.0f%% of pages", 100 * s.numberedHeads)) }
        if s.abstract { add(-2, "abstract") }
        if s.doiLine { add(-1, "DOI line on the first page") }
        if s.received { add(-2, "received/accepted dates") }
        let bookLike = score >= 4
        if !bookLike { return (.paper, why + ["score \(score) < 4"]) }
        let thesis = thesisType || (s.thesisWords && !bookType)
        return (thesis ? .thesis : .book, why + ["score \(score)"])
    }
}

/// One entry of a PDF's outline (bookmarks), with where it points: `y` from the top of the page
/// (0 when the destination gives no height).
struct OutlineEntry: Equatable {
    var title: String
    var level: Int
    var page: Int
    var y: CGFloat
}

extension PDFAnalyzer {
    /// The PDF's outline as a flat list in reading order, levels from 1. A lone top entry with
    /// entries below it (the book's title) is skipped. Up to three levels.
    static func outline(_ doc: PDFDocument) -> [OutlineEntry] {
        guard var root = doc.outlineRoot else { return [] }
        while root.numberOfChildren == 1, let only = root.child(at: 0), only.numberOfChildren > 1 { root = only }
        var out: [OutlineEntry] = []
        func walk(_ o: PDFOutline, _ level: Int) {
            guard level <= 3 else { return }
            for i in 0..<o.numberOfChildren {
                guard let c = o.child(at: i) else { continue }
                let title = (c.label ?? "").replacingOccurrences(of: #"\s+"#, with: " ", options: .regularExpression)
                    .trimmingCharacters(in: .whitespaces)
                if let d = c.destination, let pg = d.page {
                    let pno = doc.index(for: pg)
                    let mb = pg.bounds(for: .mediaBox)
                    let py = d.point.y
                    // kPDFDestinationUnspecifiedValue (FLT_MAX) or nonsense: the top of the page
                    let y = py.isFinite && py > mb.minY - 1 && py <= mb.maxY + 1 ? max(0, mb.maxY - py) : 0
                    if pno >= 0 && pno < doc.pageCount && !title.isEmpty { out.append(OutlineEntry(title: title, level: level, page: pno, y: y)) }
                }
                // a part's chapters are chapters: "PART I Analyzing DNA" is a short chapter of its own
                let part = level == 1 && c.numberOfChildren > 0 && title.matches(#"(?i)^part\s+([ivxlc]+|\d+|one|two|three|four|five|six|seven|eight|nine|ten)\b"#)
                walk(c, part ? level : level + 1)
            }
        }
        walk(root, 1)
        // reading order (a destination may be off by a line; entries that go back a page are kept in place)
        return out.enumerated().sorted { ($0.element.page, $0.offset) < ($1.element.page, $1.offset) }.map(\.element)
    }

    static let isbnRE = try! NSRegularExpression(pattern: #"\bISBN(?:-1[03])?:?\s*(?:97[89][\s\-‐‑–]?)?\d[\d\s\-‐‑–]{8,15}[\dXx]\b"#)
    static let leaderRE = try! NSRegularExpression(pattern: #"(?:\.\s?){4,}\s*(?:\d{1,4}|[ivxlcdm]{1,7})$"#)
    static let endNumberRE = try! NSRegularExpression(pattern: #"[\s,](?:\d{1,4}|[ivxlcdm]{1,7})$"#)
    static let chapterLabelRE = try! NSRegularExpression(pattern: #"^(?:chapter|appendix|part|lecture)\s+(\d{1,3}|[A-Z]|[IVXLC]{1,6})\.?$"#, options: .caseInsensitive)

    /// The lines of a page in reading position (top to bottom), without the empty ones.
    func topDown(_ pno: Int) -> [PLine] {
        pagesLines[pno].filter { !$0.text.trimmingCharacters(in: .whitespaces).isEmpty }.sorted { $0.bbox.y0 < $1.bbox.y0 }
    }

    /// The printed page number a margin line carries at either end ("28 3 Shallow neural networks",
    /// "3.1 Neural network example 29"), if it has words besides.
    static func headNumber(_ t: String) -> Int? {
        let s = t.trimmingCharacters(in: .whitespaces)
        guard s.count <= 150, s.contains(where: \.isLetter) else { return nil }
        if let m = Resolver.firstMatch(#"^(\d{1,4})\s"#, in: s), let n = Int(m) { return n }
        if let m = Resolver.firstMatch(#"\s(\d{1,4})$"#, in: s), let n = Int(m) { return n }
        return nil
    }

    func isTOCLine(_ t: String) -> Bool {
        let s = t.trimmingCharacters(in: .whitespaces)
        return Self.leaderRE.test(s)
    }

    /// Reads the signals (no layout analysis needed: the lines of each page, the outline, a few pages' text).
    func kindSignals(workType: String) -> KindSignals {
        var s = KindSignals()
        let n = doc.pageCount
        s.pages = n
        s.workType = workType
        if s.pages < 20 { return s }   // decided by the page count alone
        let entries = outline.isEmpty ? Self.outline(doc) : outline
        s.outlineEntries = entries.count
        s.outlineTop = entries.filter { $0.level == 1 }.count
        s.outlineDepth = entries.map(\.level).max() ?? 0
        let front = Array(0..<min(n, 20)), back = Array(max(min(n, 20), n - 30)..<n)
        var firstText = ""
        for i in 0..<min(n, 6) { firstText += (doc.page(at: i)?.string ?? "") + "\n" }
        let lastText = doc.page(at: n - 1)?.string ?? ""
        let info = ((doc.documentAttributes?[PDFDocumentAttribute.subjectAttribute] as? String) ?? "")
            + " " + ((doc.documentAttributes?[PDFDocumentAttribute.keywordsAttribute] as? [String])?.joined(separator: " ") ?? "")
        s.isbn = Self.isbnRE.test(firstText + "\n" + lastText + "\n" + info)
        let first2 = (doc.page(at: 0)?.string ?? "") + "\n" + (n > 1 ? doc.page(at: 1)?.string ?? "" : "")
        s.abstract = first2.matches(#"(?im)^\s*(abstract|summary)\b"#) && !first2.matches(#"(?i)table of contents"#)
        s.doiLine = first2.matches(#"(?im)^\s*(doi:|https?://(dx\.)?doi\.org/)"#)
        s.received = first2.matches(#"(?i)\breceived:?\s+\d{1,2}\s+\w+\s+\d{4}|\baccepted:?\s+\d{1,2}\s+\w+\s+\d{4}|\breceived\b[^.\n]{0,40}\baccepted\b"#)
        s.thesisWords = firstText.matches(#"(?i)\b(doctoral|phd|master'?s|licentiate)\s+(thesis|dissertation)\b|\bdissertation\b|\bthesis\b[^.\n]{0,60}\b(submitted|presented)\b|\bin partial fulfil+ment\b"#)
        func heads(_ pno: Int) -> [String] { topDown(pno).prefix(2).map { $0.text.trimmingCharacters(in: .whitespaces) } }
        for pno in front {
            let ls = topDown(pno)
            guard !ls.isEmpty else { continue }
            let top = ls.prefix(3).map { $0.text.trimmingCharacters(in: .whitespaces) }
            if top.contains(where: { $0.matches(#"(?i)^(table of )?contents$"#) }) { s.contentsPage = true }
            if top.contains(where: { $0.matches(#"(?i)^(preface|foreword)$"#) }) { s.preface = true }
            if ls.filter({ isTOCLine($0.text) }).count >= 3 { s.tocPages += 1 }
        }
        for pno in back where heads(pno).contains(where: { $0.matches(#"(?i)^(subject |general )?index$"#) }) { s.indexPage = true; break }
        var numbered = 0
        for pno in 0..<n {
            let ls = topDown(pno)
            guard let top = ls.first else { continue }
            if ls.prefix(2).contains(where: { Self.chapterLabelRE.test($0.text.trimmingCharacters(in: .whitespaces)) && $0.text.lowercased().hasPrefix("chapter") }) {
                s.chapterPages += 1
            }
            let h = boxes[pno].height
            if (top.bbox.y0 < 0.2 * h && Self.headNumber(top.text) != nil)
                || (ls.last.map { $0.bbox.y1 > 0.85 * h && Self.headNumber($0.text) != nil } ?? false) { numbered += 1 }
        }
        s.numberedHeads = Double(numbered) / Double(max(1, n))
        return s
    }
}
