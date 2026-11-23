import Foundation
import PDFKit
import CoreGraphics
#if canImport(UIKit)
import UIKit
typealias PFont = UIFont
#else
import AppKit
typealias PFont = NSFont
#endif

// Heuristic PDF → book conversion (no LLM). A port of PdfAnalyzer / pdf_heuristic_markdown in
// paper2epub.py, with PDFKit for text and PDFContentScanner for fonts, images and drawings.

struct Span {
    var text: String
    var bold = false, italic = false, sup = false, sub = false
    /// A footnote's marker in the text: the note's id (PDFNotes.swift)
    var noteRef: String?
}

final class PLine {
    var text: String
    var bbox: R
    var size: CGFloat
    var bold: CGFloat      // fraction of characters in a bold font
    var math: CGFloat      // fraction in a math font
    /// fraction in a Computer Modern roman font (math where the text is set in another font)
    var cmr: CGFloat = 0
    /// fraction in a monospaced font (code)
    var mono: CGFloat = 0
    /// the box of each character of `text` while the page's lines are read (for putting a piece of
    /// a line PDFKit gave as a line of its own back in its place); empty afterwards
    var charBoxes: [R] = []
    /// an equation's number split off this line's end (PDFEquations.swift), until the equations are found
    var tagOf: PLine?
    var spans: [Span]
    let page: Int

    init(text: String, bbox: R, size: CGFloat, bold: CGFloat, math: CGFloat, spans: [Span], page: Int) {
        self.text = text; self.bbox = bbox; self.size = size; self.bold = bold; self.math = math
        self.spans = spans; self.page = page
    }
}

final class Para {
    enum Kind { case text, heading, caption, front, title, abstract, equation, figure, table, footnote, sidenote, code }
    var lines: [PLine]
    let page: Int
    var kind: Kind
    var level = 0
    var asset: BookImage?
    var region: R?
    /// The framed or shaded box (sidebar) the paragraph sits in, numbered per document.
    var boxed: Int?
    /// A footnote's id and its marker (the marker is no longer in its text)
    var noteID: String?, noteLabel: String?
    /// A display equation found line by line (PDFEquations.swift): its number ("7.9"), the area its
    /// ink is cropped from, and the ink's width once cropped
    var eqLabel: String?, crop: R?, blank: R?, inkWidth: CGFloat?
    /// A picture's text alternative where it has text (an algorithm's box)
    var alt: String?

    init(_ lines: [PLine], page: Int, kind: Kind = .text, region: R? = nil) {
        self.lines = lines; self.page = page; self.kind = kind; self.region = region
    }

    var text: String { joinLines(lines.map(\.text)) }
    var bbox: R { region ?? R.union(lines.map(\.bbox)) }
    var size: CGFloat {
        var c: [CGFloat: Int] = [:]
        for l in lines { c[l.size, default: 0] += l.text.count }
        return c.max { ($0.value, $0.key) < ($1.value, $1.key) }?.key ?? 0
    }
}

func joinLines(_ parts: [String]) -> String {
    var out = ""
    for raw in parts {
        let p = raw.trimmingCharacters(in: .whitespaces)
        if p.isEmpty { continue }
        if out.isEmpty { out = p; continue }
        if out.hasSuffix(PDFAnalyzer.glue) {
            out.removeLast(); out += p
        } else if out.hasSuffix(PDFAnalyzer.hardHyphen) {
            out.removeLast(); out += "-" + p
        } else if urlContinues(out, p) {
            out += p
        } else if out.range(of: "[A-Za-z]-$", options: .regularExpression) != nil, p.first?.isLowercase == true {
            out.removeLast(); out += p
        } else if (out.hasSuffix("-") || out.hasSuffix("/") || out.hasSuffix("–")) && !out.hasSuffix(" -") {
            out += p
        } else {
            out += " " + p
        }
    }
    return lineEndsCleaned(out)
}

/// A web address broken across lines ("https://doi.org/10." + "1038/nature21350") goes on without a space.
func urlContinues(_ out: String, _ next: String) -> Bool {
    guard let lastWord = out.split(separator: " ").last.map(String.init), lastWord.matches(#"^\(?(https?://|www\.|doi\.org/)\S*$"#),
          let n = next.first else { return false }
    if n == "/" || n == "." { return true }
    return lastWord.last.map { ".-_/=?&#".contains($0) } == true && (n.isLowercase || n.isNumber)
}

/// Turns the line-end markers PDFAnalyzer puts in PLine.text back into plain text.
func lineEndsCleaned(_ s: String) -> String {
    guard s.contains(PDFAnalyzer.glue) || s.contains(PDFAnalyzer.hardHyphen) else { return s }
    return s.replacingOccurrences(of: PDFAnalyzer.glue, with: "").replacingOccurrences(of: PDFAnalyzer.hardHyphen, with: "-")
}

private func re(_ p: String, _ opts: NSRegularExpression.Options = []) -> NSRegularExpression {
    try! NSRegularExpression(pattern: p, options: opts)
}

extension NSRegularExpression {
    func test(_ s: String) -> Bool { firstMatch(in: s, range: NSRange(location: 0, length: (s as NSString).length)) != nil }
}

extension String {
    func matches(_ p: String, _ opts: NSRegularExpression.Options = []) -> Bool { re(p, opts).test(self) }
}

final class PDFAnalyzer {
    static let knownSections = re(
        #"^(?:\d+(?:\.\d+)*\.?\s+|[IVX]+\.\s+)?(abstract|summary|introduction|background|results?|discussion|"#
        + #"results and discussion|methods?|materials and methods|methods and materials|star\s*methods|star\+methods|online methods|"#
        + #"conclusions?|references|bibliography|acknowledge?ments?|funding|author contributions?|"#
        + #"competing interests?|declaration of interests?|conflicts? of interest|data availability|code availability|"#
        + #"data and code availability|supplementary (?:information|material|figures?|tables?)|supporting information|"#
        + #"appendix|keywords|significance|highlights|limitations|related work|experiments?|"#
        + #"figure legends|figures|tables|abbreviations|ethics statement|resource availability)\s*[:.]?$"#, .caseInsensitive)
    /// A DOI on its own: "https://doi.org/10.1038/…", "doi: 10.…".
    static func isDOILine(_ s: String) -> Bool {
        s.trimmingCharacters(in: .whitespaces).matches(#"^(https?://(dx\.)?doi\.org/|doi:?\s*)10\.\d{4,9}/\S+$"#, .caseInsensitive)
    }
    /// P2E_DEBUG_SYMBOLS: each symbol font glyph (see SymbolFonts) and the characters PDFKit read around it.
    static let debugSymbols = ProcessInfo.processInfo.environment["P2E_DEBUG_SYMBOLS"] != nil
    /// P2E_DEBUG_CHARS=<text>: the characters of lines with that text, and the gaps near them;
    /// P2E_DEBUG_GAPS=<page>: the text the content stream shows on that page (PDFContentScanner)
    static let debugChars = ProcessInfo.processInfo.environment["P2E_DEBUG_CHARS"]
    static let debugGapsPage = ProcessInfo.processInfo.environment["P2E_DEBUG_GAPS"].flatMap { Int($0) }

    /// A list item's bullet at the start of a line ("• ISS elucidates…"), with a space after it: LaTeX's
    /// degree sign (\circ, "◦C") and other marks used as symbols ("•OH") are text, and so is "◦ C".
    static let bulletRE = re(#"^\s*(?:[•●▪‣]|◦(?!\s*[CFK]\b))\s+\S"#)
    static func isBullet(_ l: PLine) -> Bool { bulletRE.test(l.text) }

    /// Paragraphs that start with a bullet (and a space; ◦ not as a degree sign), one after another,
    /// become a list: `<p>• a</p><p>• b</p>` → `<ul><li>a</li><li>b</li></ul>`.
    static func bulletLists(_ html: String) -> String {
        let mark = #"(?:<sup>[•●▪‣]</sup>|[•●▪‣]|(?:<sup>◦</sup>|◦)(?!\s*[CFK]\b))\s+"#
        let item = try! NSRegularExpression(pattern: #"<p>\s*"# + mark + #"(.*?)</p>"#)
        let run = try! NSRegularExpression(pattern: #"(?:<p>\s*"# + mark + #"\S.*?</p>)+"#)
        let ns = html as NSString
        var out = ""
        var last = 0
        for m in run.matches(in: html, range: NSRange(location: 0, length: ns.length)) {
            out += ns.substring(with: NSRange(location: last, length: m.range.location - last))
            let block = ns.substring(with: m.range)
            out += "<ul>" + item.stringByReplacingMatches(in: block, range: NSRange(location: 0, length: (block as NSString).length),
                                                         withTemplate: "<li>$1</li>") + "</ul>"
            last = m.range.location + m.range.length
        }
        return out + ns.substring(from: last)
    }

    /// A figure's, table's or box's number: "2", "S1", a book's chapter-numbered "1.2", "10.14", "S1.2",
    /// "3-4" and an appendix's "A.1". A dot or dash continues the number only into a digit ("Figure 1.
    /// Overview" is figure 1), and the whole number is taken (atomic: "Figure 1.2 depicts" is not
    /// "Figure 1" + ". 2 depicts").
    static let labelNumber = #"(?>S?\d+(?:[.\-–]\d+)*|[A-Z](?:[.\-–]\d+)+)"#

    static let captionRE = re(
        // the label must be followed by punctuation, a capitalised word ("3D" too) or nothing: running text
        // that wraps onto a line starting "Figure S2G). Top genes…" or "Fig. 1a and …" is not a caption
        #"^(?:Supplementary\s+|Extended\s+Data\s+|Supporting\s+)?(Figure|Fig\.?|Scheme|Table|Algorithm|Box)\s*"# + labelNumber
            + #"[A-Za-z]?(?![\w)\];,])\s*(?:[.:|\-–—]|(?-i:[A-Z(\[]|\d[A-Z])|$)"#,
        .caseInsensitive)

    let doc: PDFDocument
    var pagesLines: [[PLine]] = []
    var graphics: [PageGraphics] = []
    var boxes: [CGRect] = []
    var bodySize: CGFloat = 10
    var running = Set<String>()
    private var imageCounter = 0
    private var usedNames = Set<String>()
    /// `name`, or with "-2", "-3"… when an image of that name was made already: two equations on a
    /// page may start on the same line ("p296-eq192"), and an EPUB (a zip) may not hold a file twice.
    func uniqueName(_ name: String) -> String {
        var n = name, k = 1
        while usedNames.contains(n) { k += 1; n = "\(name)-\(k)" }
        usedNames.insert(n)
        return n
    }
    /// Line-end markers in PLine.text: `glue` = the word continues on the next line (PDFKit removed a
    /// hyphenation hyphen); `hardHyphen` = a real hyphen at the end of a line ("sample-|level").
    static let glue = "\u{AD}", hardHyphen = "\u{2011}"
    private var gluedLines: [CGFloat: Int] = [:]   // by font size
    static let spacingAccents: [String: String] = ["¨": "\u{0308}", "´": "\u{0301}", "ˆ": "\u{0302}", "˜": "\u{0303}",
                                                    "ˇ": "\u{030C}", "¸": "\u{0327}", "˚": "\u{030A}"]
    static let primes: Set<String> = ["′", "″", "‴"]
    static let noSpaceBefore: Set<String> = [",", ".", ";", ":", "!", "?", ")", "]", "}", "’", "”", "%"]
    static let noSpaceAfter: Set<String> = ["(", "[", "{", "‘", "“"]
    let log: (String) -> Void
    /// Paper or book (see DocumentClassifier), decided after the lines are read; the work type the
    /// library knows for the item is one of its signals.
    var workType = ""
    var kind: DocumentKind = .paper
    var kindReasons: [String] = []
    /// Book mode: the PDF's outline, the margin and table-of-contents lines left out, the pages of the index.
    var outline: [OutlineEntry] = []
    var dropped = Set<ObjectIdentifier>()
    var tocPages = Set<Int>()
    var indexPages: [Range<Int>] = []
    /// The text columns' left and right ends, by the side of the page (PDFEquations.swift)
    var docEdges: [Int: (lefts: [CGFloat], rights: [CGFloat])] = [:]

    init(doc: PDFDocument, log: @escaping (String) -> Void) {
        self.doc = doc
        self.log = log
    }

    // MARK: - Extraction

    func extract() {
        var sizes: [CGFloat: Int] = [:]
        for pno in 0..<doc.pageCount {
            // (PDFKit's selections and attributed strings of a page are autoreleased: let them go page by page)
            autoreleasepool {
                guard let page = doc.page(at: pno) else { pagesLines.append([]); graphics.append(PageGraphics()); boxes.append(.zero); return }
                let mb = page.bounds(for: .mediaBox)
                boxes.append(mb)
                if Self.isCoverSheet(page) {
                    pagesLines.append([]); graphics.append(PageGraphics()); return
                }
                PDFContentScanner.debugGaps = Self.debugGapsPage == pno + 1
                let g = page.pageRef.map { PDFContentScanner(box: mb).scan($0) } ?? PageGraphics()
                graphics.append(g)
                let lines = extractLines(page, pno: pno, mb: mb, runs: g.runs, gaps: g.gaps, joints: g.joints, ligatures: g.ligatures)
                for l in lines { sizes[l.size, default: 0] += l.text.count }
                pagesLines.append(lines)
            }
            if pno % 5 == 4 { log("Reading page \(pno + 1) of \(doc.pageCount)") }
        }
        if let best = sizes.max(by: { ($0.value, $0.key) < ($1.value, $1.key) }) { bodySize = best.key }
        // When PDFKit itself takes out the hyphenation hyphens (it can tell them apart in this PDF), the
        // hyphens left at line ends are real ones ("sample-|level"): keep them when the lines are joined.
        // Decided per font size: PDFKit may do it for the body text and not for the legends.
        let hyphenEnds = Dictionary(grouping: pagesLines.joined().filter {
            $0.text.range(of: "[A-Za-z]-$", options: .regularExpression) != nil
        }) { ($0.size * 2).rounded() / 2 }
        for (size, ends) in hyphenEnds {
            let glued = gluedLines[size] ?? 0
            if glued >= 3 && glued >= ends.count / 2 {
                for l in ends { l.text = String(l.text.dropLast()) + Self.hardHyphen }
            }
        }
        Self.mathFromComputerModern(pagesLines.joined(), bodySize: bodySize)
        Self.clearMathIfBodyFont(pagesLines.joined(), bodySize: bodySize)
        findRunning()
        // P2E_DEBUG_LINES=12,13: the lines read on those pages (1-based)
        if let pages = ProcessInfo.processInfo.environment["P2E_DEBUG_LINES"] {
            for s in pages.split(separator: ",") {
                guard let k = Int(s), k >= 1, k <= pagesLines.count else { continue }
                for l in pagesLines[k - 1] {
                    print(String(format: "L%-3d (%5.1f,%5.1f)-(%5.1f,%5.1f) sz%4.1f m%.2f b%.2f ", k, l.bbox.x0, l.bbox.y0, l.bbox.x1, l.bbox.y1,
                                 l.size, l.math, l.bold) + l.text + "  ⟦" + l.spans.map { s in
                                    (s.sup ? "^" : s.sub ? "_" : "") + (s.italic ? "/" : "") + (s.bold ? "*" : "") + s.text }.joined(separator: "|") + "⟧")
                }
            }
        }
    }

    private func extractLines(_ page: PDFPage, pno: Int, mb: CGRect, runs: [TextRun], gaps: [CGPoint], joints: [CGPoint],
                              ligatures: [Ligature] = []) -> [PLine] {
        guard let attr = page.attributedString, let sel = page.selection(for: mb) else { return [] }
        let ns = attr.string as NSString
        // bucket text runs by baseline for quick style lookup
        var buckets: [Int: [TextRun]] = [:]
        for r in runs { buckets[Int(r.baseline / 4), default: []].append(r) }
        var gapBuckets: [Int: [CGPoint]] = [:]
        for g in gaps { gapBuckets[Int(g.y / 4), default: []].append(g) }
        var jointBuckets: [Int: [CGPoint]] = [:]
        for j in joints { jointBuckets[Int(j.y / 4), default: []].append(j) }
        /// a point of `buckets` between the middles of two characters, on their baseline. A glyph PDFKit
        /// cannot map (a Greek letter in a symbol font: "hepatitis δ antigen") is left out of its text
        /// but widens the box of the letter before or after it, whose middle then lies past the word
        /// gap: no further in than 0.3 em from the letter's edge.
        func between(_ buckets: [Int: [CGPoint]], _ p: R, _ c: R, size: CGFloat = 0) -> Bool {
            // (a box much taller than its type, a line's with sub- and superscripts in it: the em is the
            // type's, and the point must sit on the baseline of the text there, not the next line's)
            let tall = size > 0 && c.h > 1.6 * max(size, 4)
            let em = tall ? max(size, 4) : max(p.h, c.h, 4)
            let lo = max((p.x0 + p.x1) / 2, p.x1 - 0.3 * em), hi = min((c.x0 + c.x1) / 2, c.x0 + 0.3 * em)
            let base = tall ? style(c, size: size)?.baseline : nil
            let key = Int(c.y1 / 4)
            // the point must sit on this line's baseline (lower part of the glyph box), not the line above
            return ((key - 2)...(key + 2)).contains { k in
                (buckets[k] ?? []).contains { g in
                    guard g.x > lo && g.x < hi else { return false }
                    if let base { return abs(g.y - base) < 0.3 * em }
                    return g.y >= c.y0 + 0.4 * c.h && g.y <= c.y1 + 1.5
                }
            }
        }

        func toTop(_ b: CGRect) -> R {
            R(x0: b.minX - mb.minX, y0: mb.maxY - b.maxY, x1: b.maxX - mb.minX, y1: mb.maxY - b.minY)
        }
        func style(_ cb: R, size: CGFloat) -> TextRun? {
            let cx = cb.cx
            var best: TextRun?
            var bestD = CGFloat.greatestFiniteMagnitude
            // A box much taller than the type (PDFKit gives some lines' characters the whole line's box,
            // a formula's with its fraction or matrix): the run that has a glyph starting where the
            // character starts, anywhere in the box
            if cb.h > 1.6 * max(size, 4) {
                for k in (Int(cb.y0 / 4) - 1)...(Int(cb.y1 / 4) + 1) {
                    for r in buckets[k] ?? [] where cx >= r.x0 - 0.8 && cx <= r.x1 + 0.8 && r.baseline >= cb.y0 && r.baseline <= cb.y1 + 1 {
                        let d = r.starts.map { abs($0 - cb.x0) }.min() ?? 0.5 * max(size, 4)
                        if d < bestD { bestD = d; best = r }
                    }
                }
                return best
            }
            let key = Int(cb.y1 / 4)
            for k in (key - 2)...(key + 2) {
                for r in buckets[k] ?? [] where cx >= r.x0 - 0.8 && cx <= r.x1 + 0.8
                    && r.baseline >= cb.y0 + 0.3 * cb.h && r.baseline <= cb.y1 + 1 {
                    let d = abs(r.baseline - (cb.y1 - 0.22 * max(size, 4)))
                    if d < bestD { bestD = d; best = r }
                }
            }
            return best
        }

        // Whether a letter is one the page does not show (see TextRun.hidden). A hidden copy of the text
        // may lie within a point of the text shown, too close to go by the baseline: the letter goes with
        // the glyph that starts where it starts, and is hidden only when a hidden glyph starts there and
        // none shown does.
        let hasHidden = runs.contains { $0.hidden }
        func isHidden(_ cb: R, size: CGFloat) -> Bool {
            let s = max(size, 4), base = cb.y1 - 0.22 * s
            var shown = CGFloat.greatestFiniteMagnitude, hidden = CGFloat.greatestFiniteMagnitude
            let key = Int(cb.y1 / 4)
            var near: [TextRun] = []
            for k in (key - 2)...(key + 2) {
                for r in buckets[k] ?? [] where abs(r.baseline - base) < 0.6 * s && cb.cx >= r.x0 - 1 && cb.cx <= r.x1 + 1 {
                    near.append(r)
                }
            }
            // a copy in another type size goes by the size first
            let tol = max(0.3, 0.06 * size)
            let sized = near.filter { r in abs(r.size - size) <= tol }
            for r in sized.isEmpty ? near : sized {
                let dx = r.starts.map { abs($0 - cb.x0) }.min() ?? .greatestFiniteMagnitude
                if r.hidden || r.clip.map({ cb.cx < $0.x0 || cb.cx > $0.x1 }) == true { hidden = min(hidden, dx) } else { shown = min(shown, dx) }
            }
            // with no text shown on that baseline, a hidden glyph nearby is enough
            return hidden < 0.3 && hidden + 0.2 < shown || shown == .greatestFiniteMagnitude && hidden < 0.5 * s
        }

        var out: [PLine] = []
        var usedLigatures = Set<Int>()   // each goes into one line (PDFKit may break the line at it)
        var lineSels: [(sel: PDFSelection, range: NSRange)] = []
        for ls in sel.selectionsByLine() {
            let nr = ls.numberOfTextRanges(on: page)
            guard nr > 0 else { continue }
            var range = ls.range(at: 0, on: page)
            for k in 1..<max(1, nr) { range = NSUnionRange(range, ls.range(at: k, on: page)) }
            guard range.location != NSNotFound, range.location + range.length <= ns.length, range.length > 0 else { continue }
            lineSels.append((ls, range))
        }
        let letters = CharacterSet.letters
        func isLetter(_ i: Int) -> Bool {
            guard i >= 0, i < ns.length, let u = Unicode.Scalar(ns.character(at: i)) else { return false }
            return letters.contains(u)
        }
        struct C { var ch: String; var size: CGFloat; var box: R; var run: TextRun?; var added = false; var ligature = false; var symbol = false }
        /// the median gap between the words of a line (0 for one word)
        func typicalWordGap(_ chars: [C]) -> CGFloat {
            let blank = { (c: C) in c.ch.trimmingCharacters(in: .whitespaces).isEmpty }
            var gaps: [CGFloat] = []
            for k in chars.indices.dropFirst().dropLast() where chars[k].ch == " " && !blank(chars[k - 1]) && !blank(chars[k + 1]) {
                gaps.append(chars[k + 1].box.x0 - chars[k - 1].box.x1)
            }
            gaps.sort()
            return gaps.isEmpty ? 0 : gaps[gaps.count / 2]
        }
        /// Puts back the ligatures PDFKit left out of the line (see PageGraphics.ligatures): where a
        /// ligature glyph is drawn on the line's baseline and PDFKit has no letter, its letters go in
        /// place of the space PDFKit put there (or between the letters either side), with a space
        /// before or after only where the glyphs leave room for one ("detoxi cation" →
        /// "detoxification", "to lter" → "to filter").
        func restoreLigatures(_ chars: inout [C]) {
            let blank = { (c: C) in c.ch.trimmingCharacters(in: .whitespaces).isEmpty }
            let shown = chars.filter { !blank($0) && $0.box.w > 0 }
            guard let first = shown.first else { return }
            let lineBox = R.union(shown.map(\.box))
            for (li, l) in ligatures.enumerated().sorted(by: { $0.element.x0 < $1.element.x0 }) where !usedLigatures.contains(li) && !l.symbol {
                let em = max(l.size, 4)
                // on this line: the baseline in the lower part of its letters, within its extent
                guard l.baseline >= lineBox.y0 + 0.4 * first.box.h, l.baseline <= lineBox.y1 + 1.5,
                      l.x0 >= lineBox.x0 - em, l.x1 <= lineBox.x1 + em else { continue }
                let letters = chars.indices.filter { !blank(chars[$0]) && chars[$0].box.w > 0 }
                // PDFKit has its letters there: it mapped the ligature itself (another letter may reach
                // over it, its box widened by the glyph PDFKit left out)
                let inner = 0.3 * (l.x1 - l.x0)
                let own = Set(l.text.map(String.init) + ["\u{FB00}", "\u{FB01}", "\u{FB02}", "\u{FB03}", "\u{FB04}"])
                if letters.contains(where: { own.contains(chars[$0].ch) && chars[$0].box.x0 < l.x1 - inner && chars[$0].box.x1 > l.x0 + inner }) { continue }
                let w = (l.x1 - l.x0) / CGFloat(l.text.count)
                func glyphs(_ y0: CGFloat, _ y1: CGFloat) -> [C] {
                    l.text.enumerated().map { k, ch in
                        let x = l.x0 + w * CGFloat(k)
                        return C(ch: String(ch), size: l.size, box: R(x0: x, y0: y0, x1: x + w, y1: y1), run: nil, ligature: true)
                    }
                }
                // something else in its middle: a symbol the font's wrong ToUnicode map gives for it
                // ("di↵erent") is replaced; a letter means the ligature is not where PDFKit's text is
                let within = letters.filter { chars[$0].box.cx > l.x0 && chars[$0].box.cx < l.x1 }
                if let a = within.first, let b = within.last {
                    // (not a character of Latin-1: a font may name its ";" "fi" and map it right)
                    guard within.allSatisfy({ k in chars[k].ch.unicodeScalars.allSatisfy { $0.value > 0xFF && !CharacterSet.alphanumerics.contains($0) } }) else { continue }
                    chars.replaceSubrange(a...b, with: glyphs(chars[a].box.y0, chars[a].box.y1))
                    usedLigatures.insert(li)
                    continue
                }
                // nothing there: put in only a ligature at a code PDFKit leaves out
                guard l.lowCode else { continue }
                let mid = (l.x0 + l.x1) / 2
                let j = letters.first { chars[$0].box.cx >= mid } ?? chars.count
                let i = letters.last { $0 < j }
                // the letters either side touch the ligature (or there is none on that side)
                guard i != nil || j < chars.count else { continue }
                if let i, l.x0 - chars[i].box.x1 > em { continue }
                if j < chars.count, chars[j].box.x0 - l.x1 > em { continue }
                // and leave room for it (else the glyphs are not where the widths put them)
                if let i, j < chars.count, chars[j].box.x0 - chars[i].box.x1 < 0.8 * (l.x1 - l.x0) { continue }
                let lo = i.map { $0 + 1 } ?? 0
                guard (lo..<j).allSatisfy({ blank(chars[$0]) }) else { continue }
                let near = chars[i ?? j].box
                var ins: [C] = []
                if let i, l.x0 - chars[i].box.x1 > 0.15 * em {
                    ins.append(C(ch: " ", size: l.size, box: R(x0: chars[i].box.x1, y0: near.y0, x1: l.x0, y1: near.y1), run: nil, added: true))
                }
                ins += glyphs(near.y0, near.y1)
                if j < chars.count, chars[j].box.x0 - l.x1 > 0.15 * em {
                    ins.append(C(ch: " ", size: l.size, box: R(x0: l.x1, y0: near.y0, x1: chars[j].box.x0, y1: near.y1), run: nil, added: true))
                }
                chars.replaceSubrange(lo..<j, with: ins)
                usedLigatures.insert(li)
            }
        }
        /// A symbol font's glyph (see SymbolFonts) replaces what PDFKit read for it ("d" for a bullet,
        /// "ª" or "Ó" for ©): the character of its name or code that starts where it starts, on its
        /// baseline (PDFKit widens the letter next to a glyph it leaves out: that letter stays). Where
        /// PDFKit read nothing on any line, the glyph goes in between the letters either side, on their
        /// baseline. Either way it takes the height of the letters next to it: a bullet drawn small and
        /// high is not a superscript.
        let symbolGlyphs = ligatures.indices.filter { ligatures[$0].symbol }
        func reads(_ c: C, _ l: Ligature) -> Bool {
            // (or a box PDFKit widened over the glyph)
            let tol = max(1, 0.2 * max(l.size, 4))
            return l.reads.contains(c.ch) && c.box.w > 0 && onBaseline(c, l)
                && (abs(c.box.x0 - l.x0) <= tol || c.box.x0 <= l.x0 + tol && c.box.x1 >= l.x1 - tol)
        }
        func onBaseline(_ c: C, _ l: Ligature) -> Bool { l.baseline >= c.box.y0 + 0.3 * c.box.h && l.baseline <= c.box.y1 + 1 }
        var symbolRead = Set<Int>()
        func restoreSymbols(_ chars: inout [C]) {
            let blank = { (c: C) in c.ch.trimmingCharacters(in: .whitespaces).isEmpty || c.box.w <= 0 }
            for li in symbolGlyphs where !usedLigatures.contains(li) {
                let l = ligatures[li], em = max(l.size, 4)
                let hits = chars.indices.filter { reads(chars[$0], l) && !chars[$0].symbol }
                if Self.debugSymbols, chars.contains(where: { $0.box.x1 > l.x0 - 15 && $0.box.x0 < l.x1 + 15 && abs($0.box.y1 - l.baseline) < 20 }) {
                    let near = chars.filter { $0.box.x1 > l.x0 - 15 && $0.box.x0 < l.x1 + 15 && abs($0.box.y1 - l.baseline) < 20 }
                    FileHandle.standardError.write("SYM \(l.text) x \(Int(l.x0))-\(Int(l.x1)) base \(Int(l.baseline)) hits \(hits.count) read \(symbolRead.contains(li)) near \(near.map { "\($0.ch)[\(Int($0.box.x0))-\(Int($0.box.x1)),\(Int($0.box.y0))-\(Int($0.box.y1))]" }.joined(separator: " "))\n".data(using: .utf8)!)
                }
                func glyph(_ cs: [C], _ text: String) -> C? {
                    // the height of the nearest letter that is not a symbol (nor what PDFKit read for it)
                    let mid = (l.x0 + l.x1) / 2
                    let near = cs.filter { !blank($0) && !$0.symbol && !reads($0, l) }.min { abs($0.box.cx - mid) < abs($1.box.cx - mid) }
                    guard let c = near ?? hits.first.map({ chars[$0] }) else { return nil }
                    return C(ch: text, size: c.size, box: R(x0: l.x0, y0: c.box.y0, x1: l.x1, y1: c.box.y1), run: nil, symbol: true)
                }
                /// puts `g` in between the letters either side (on its baseline, and near)
                func insert(_ g: C, into cs: inout [C]) -> Bool {
                    let letters = cs.indices.filter { !blank(cs[$0]) }
                    let mid = (l.x0 + l.x1) / 2
                    let j = letters.first { cs[$0].box.x0 >= mid } ?? cs.count
                    let i = letters.last { $0 < j }
                    guard i != nil || j < cs.count else { return false }
                    if let i, !onBaseline(cs[i], l) || l.x0 - cs[i].box.x1 > 3 * em { return false }
                    if j < cs.count, !onBaseline(cs[j], l) || cs[j].box.x0 - l.x1 > 3 * em { return false }
                    let lo = i.map { $0 + 1 } ?? 0
                    guard (lo..<j).allSatisfy({ blank(cs[$0]) }) else { return false }
                    // not into a word whose letters leave no room for it (where PDFKit's letters are
                    // not where the content stream draws them, a glyph would land inside a word)
                    if lo == j, let i, j < cs.count, cs[j].box.x0 - cs[i].box.x1 < 0.5 * (l.x1 - l.x0) { return false }
                    var ins: [C] = []
                    if let i, l.x0 - cs[i].box.x1 > 0.15 * em {
                        ins.append(C(ch: " ", size: g.size, box: R(x0: cs[i].box.x1, y0: g.box.y0, x1: l.x0, y1: g.box.y1), run: nil, added: true))
                    }
                    ins.append(g)
                    if j < cs.count, cs[j].box.x0 - l.x1 > 0.15 * em {
                        ins.append(C(ch: " ", size: g.size, box: R(x0: l.x1, y0: g.box.y0, x1: cs[j].box.x0, y1: g.box.y1), run: nil, added: true))
                    }
                    cs.replaceSubrange(lo..<j, with: ins)
                    return true
                }
                if let h = hits.min(by: { abs(chars[$0].box.x0 - l.x0) < abs(chars[$1].box.x0 - l.x0) }) {
                    guard let g = glyph(chars, l.byRead.map { $0[chars[h].ch] ?? "" } ?? l.text), !g.ch.isEmpty else { continue }
                    usedLigatures.insert(li)
                    // in its place, unless PDFKit put it out of order ("pA + ð" for "(pA + pA)")
                    let prev = chars[..<h].lastIndex { !blank($0) }, next = chars[(h + 1)...].firstIndex { !blank($0) }
                    if (prev.map { chars[$0].box.x0 < l.x0 } ?? true) && (next.map { chars[$0].box.x0 >= l.x0 - 0.5 } ?? true) {
                        chars[h] = g
                        continue
                    }
                    var rest = chars
                    rest.remove(at: h)
                    if insert(g, into: &rest) { chars = rest } else { chars[h] = g }
                    continue
                }
                guard !symbolRead.contains(li), l.byRead == nil, let g = glyph(chars, l.text) else { continue }
                if insert(g, into: &chars) { usedLigatures.insert(li) }
            }
        }
        // the lines' characters first, to tell glyphs drawn twice over (see below)
        var built: [(li: Int, range: NSRange, bbox: R, chars: [C])] = []
        for (li, (ls, range)) in lineSels.enumerated() {
            let bbox = toTop(ls.bounds(for: page))
            if bbox.h > 2.5 * bbox.w && range.length > 2 { continue }   // rotated margin text
            var chars: [C] = []
            var i = range.location
            while i < range.location + range.length {
                let r = ns.rangeOfComposedCharacterSequence(at: i)
                let ch = ns.substring(with: r)
                i = r.location + r.length
                if ch == "\n" || ch == "\r" {
                    // PDFKit removes soft hyphens and moves the line break to the end of the rejoined word
                    // ("ques-|tions require" → "questions\nrequire"): a break inside a line is a word space
                    if let p = chars.last, p.ch != " " { chars.append(C(ch: " ", size: p.size, box: R(x0: p.box.x1, y0: p.box.y0, x1: p.box.x1, y1: p.box.y1), run: nil, added: true)) }
                    continue
                }
                let f = attr.attribute(.font, at: r.location, effectiveRange: nil) as? PFont
                // characterBounds(at:) uses a different index space on some PDFs; a 1-char selection is reliable
                let box = page.selection(for: r).map { toTop($0.bounds(for: page)) } ?? R(x0: 0, y0: 0, x1: 0, y1: 0)
                let size = f?.pointSize ?? 0
                // a bullet with a box across the whole line is a second copy of an item's bullet ("••", drawn
                // twice to look bold) that PDFKit put after the line's last word ("analy•" for "analy-")
                if ch == "•" && box.w > 3 * max(size, 4) { continue }
                chars.append(C(ch: ch, size: size, box: box, run: nil))
            }
            while chars.last?.ch == " " { chars.removeLast() }
            guard !chars.isEmpty else { continue }
            built.append((li, range, bbox, chars))
        }
        // Glyphs drawn twice in the same place (a book's bullets and line-end hyphens overprinted to look
        // bold): PDFKit may gather the copies into lines of their own between the lines of the text
        // ("chro-|- • - • - •|mosomes", "analy•|y• y• y•|ses"). A line of nothing but copies of letters
        // drawn before goes, and in a line a symbol drawn twice.
        // (the same letter, within 0.6 pt: a copy drawn to look bold is set a little to the side)
        func same(_ a: C, _ b: C) -> Bool {
            a.ch == b.ch && abs(a.box.x0 - b.box.x0) < 0.6 && abs(a.box.y0 - b.box.y0) < 0.6 && abs(a.box.w - b.box.w) < 0.6
        }
        var drawn: [String: [(line: Int, c: C)]] = [:]   // by letter: where it is drawn first
        func first(_ c: C) -> Int? { drawn[c.ch]?.first { same($0.c, c) }?.line }
        let shown = { (c: C) in !c.ch.trimmingCharacters(in: .whitespaces).isEmpty && c.box.w > 0 }
        for (n, b) in built.enumerated() {
            for c in b.chars where shown(c) && first(c) == nil { drawn[c.ch, default: []].append((n, c)) }
        }
        for n in built.indices {
            let visible = built[n].chars.filter(shown)
            if !visible.isEmpty && visible.allSatisfy({ first($0).map { $0 < n } == true }) {
                built[n].chars = []
                continue
            }
            // (within a line only symbols: PDFKit's boxes for a word drawn twice are not where its letters are)
            var kept: [C] = []
            for c in built[n].chars {
                if shown(c) && !c.ch.unicodeScalars.contains(where: { CharacterSet.alphanumerics.contains($0) })
                    && kept.contains(where: { same($0, c) }) { continue }
                kept.append(c)
            }
            while kept.last?.ch == " " { kept.removeLast() }
            built[n].chars = kept
        }
        let copies = Set(built.filter { $0.chars.isEmpty }.map(\.li))
        for li in symbolGlyphs where built.contains(where: { b in b.chars.contains { reads($0, ligatures[li]) } }) { symbolRead.insert(li) }
        for (li, range, bbox, lineChars) in built where !lineChars.isEmpty {
            var chars = lineChars
            // text the content stream draws outside a form's box is not shown, though PDFKit reads it
            // (see GState.clip) and mixes it into the lines it overlaps: leave it out, and the spaces
            // that came with it unless there is room for one between the letters that stay
            var endsHidden = false
            if hasHidden {
                let hidden = chars.map { c in
                    !c.ch.trimmingCharacters(in: .whitespaces).isEmpty && c.box.w > 0 && isHidden(c.box, size: c.size)
                }
                // the last line does not run on into hidden letters
                if hidden.first == true, let prev = out.last, prev.text.hasSuffix(Self.glue) {
                    prev.text.removeLast()
                    gluedLines[(prev.size * 2).rounded() / 2, default: 1] -= 1
                }
                // a line of mostly hidden letters is the hidden copy, whose few letters that start where a
                // shown one does are hidden too
                let letters = chars.filter { !$0.ch.trimmingCharacters(in: .whitespaces).isEmpty }.count
                if letters > 0 && hidden.filter({ $0 }).count * 10 >= letters * 6 { continue }
                if hidden.contains(true) {
                    endsHidden = hidden.last == true
                    // the letters that stay are spaced by where they stand: the spaces next to hidden letters
                    // go, and one is put where the letters shown leave room for it
                    var kept: [C] = []
                    var cut = false
                    for (k, c) in chars.enumerated() {
                        let space = c.ch.trimmingCharacters(in: .whitespaces).isEmpty
                        if hidden[k] || space && ((k > 0 && hidden[k - 1]) || (k + 1 < chars.count && hidden[k + 1])) {
                            cut = true
                            continue
                        }
                        if cut, !space, let p = kept.last, p.ch != " ",
                           c.box.x0 - p.box.x1 >= 0.15 * max(c.size, p.size, 4) || c.box.x0 < p.box.x0 {
                            kept.append(C(ch: " ", size: c.size, box: R(x0: p.box.x1, y0: c.box.y0, x1: max(p.box.x1, c.box.x0), y1: c.box.y1),
                                          run: nil, added: true))
                        }
                        if !space { cut = false }
                        kept.append(c)
                    }
                    chars = kept
                    while chars.first?.ch == " " { chars.removeFirst() }
                    while chars.last?.ch == " " { chars.removeLast() }
                    guard !chars.isEmpty else { continue }
                }
            }
            if !symbolGlyphs.isEmpty { restoreSymbols(&chars) }
            if !ligatures.isEmpty { restoreLigatures(&chars) }
            let endsWithLigature = chars.last?.ligature == true
            chars = chars.indices.compactMap { k in
                guard k > 0, k + 1 < chars.count, chars[k].ch == " " else { return chars[k] }
                let p = chars[k - 1], n = chars[k + 1]
                return Self.isSpuriousSpace(chars[k].ch, chars[k].box, added: chars[k].added, prev: (p.ch, p.box, p.size),
                                            next: (n.ch, n.box, n.size), joint: between(jointBuckets, p.box, n.box, size: n.size))
                    ? nil : chars[k]
            }
            var sizeCount: [CGFloat: Int] = [:]
            for c in chars where !c.ch.trimmingCharacters(in: .whitespaces).isEmpty {
                sizeCount[(c.size * 2).rounded() / 2, default: 0] += 1
            }
            guard let dom = sizeCount.max(by: { ($0.value, $0.key) < ($1.value, $1.key) })?.key, dom > 0 else { continue }
            let mains = chars.filter { abs($0.size - dom) < 0.6 && $0.box.h > 0 }
            let mainCenter = mains.isEmpty ? bbox.y0 + bbox.h / 2 : mains.map { ($0.box.y0 + $0.box.y1) / 2 }.reduce(0, +) / CGFloat(mains.count)
            let mainHeight = mains.isEmpty ? bbox.h : mains.map(\.box.h).reduce(0, +) / CGFloat(mains.count)
            var spans: [Span] = []
            var nBold = 0, nMath = 0, nCMR = 0, nMono = 0, n = 0
            var last: TextRun?
            var text = ""
            // Some PDFs place words by position and never encode a space: put one back wherever the
            // content stream jumped right between two characters (see PageGraphics.gaps) — but never
            // before closing or after opening punctuation.
            if let want = Self.debugChars, chars.map(\.ch).joined().contains(want) {
                for c in chars { print(String(format: "CH %@ %.2f-%.2f sz %.1f", c.ch, c.box.x0, c.box.x1, c.size)) }
                for g in gaps where abs(g.y - bbox.y1) < 6 { print(String(format: "GAP %.2f %.1f", g.x, g.y)) }
            }
            var spaced: [C] = []
            for c in chars {
                if let p = spaced.last, !p.ch.trimmingCharacters(in: .whitespaces).isEmpty,
                   !c.ch.trimmingCharacters(in: .whitespaces).isEmpty, p.box.w > 0, c.box.w > 0, c.box.x0 >= p.box.x0,
                   !Self.noSpaceBefore.contains(c.ch), !Self.noSpaceAfter.contains(p.ch) {
                    if between(gapBuckets, p.box, c.box, size: c.size) { spaced.append(C(ch: " ", size: c.size, box: R(x0: p.box.x1, y0: c.box.y0, x1: c.box.x0, y1: c.box.y1), run: nil)) }
                }
                spaced.append(c)
            }
            chars = spaced
            // Accents drawn as glyphs of their own: fold them into their letter. After it ("Bu¨ ttner",
            // drawn after a move back over the letter, or with no width: "Falca˜ o"), drop the space the
            // move leaves behind; before it, set right up to it ("St´ephane", and the "€" Elsevier's
            // accent font has for a dieresis: "J€akel" → "Jäkel").
            if chars.contains(where: { Self.spacingAccents[$0.ch] != nil || $0.ch == "€" }) {
                let letter = { (c: C) in c.ch.count == 1 && c.ch.first!.isLetter }
                var folded: [C] = []
                var k = 0
                while k < chars.count {
                    let c = chars[k]
                    let comb = Self.spacingAccents[c.ch] ?? (c.ch == "€" ? "\u{0308}" : nil)
                    let prev = folded.last.flatMap { letter($0) ? $0 : nil }
                    let next = k + 1 < chars.count && letter(chars[k + 1]) ? chars[k + 1] : nil
                    let em = max(c.size, 4)
                    // over the letter before it (or no width): that letter's
                    let onPrev = prev.map { p in c.box.w <= 0.05 * em || min(p.box.x1, c.box.x1) - max(p.box.x0, c.box.x0) > 0.3 * c.box.w } ?? false
                    // set right up to the next letter, within a word: that one's, if the two make a letter
                    // and the letter before and the accent do not ("St´ephane", "J€akel"; but "Mu¨nster",
                    // and "4˚C" is a degree sign)
                    let makes = { (base: C) in comb.map { ((base.ch == "ı" ? "i" : base.ch) + $0).precomposedStringWithCanonicalMapping.unicodeScalars.count == 1 } ?? false }
                    let onNext = !onPrev && next.map { n in n.box.x0 >= c.box.x0 - 0.1 * em && n.box.x0 <= c.box.x1 + 0.1 * em } == true
                        && prev.map { !makes($0) } == true && next.map(makes) == true
                    if let comb, onNext, let n = next {
                        folded.append(n)
                        folded[folded.count - 1].ch = ((n.ch == "ı" ? "i" : n.ch) + comb).precomposedStringWithCanonicalMapping
                        folded[folded.count - 1].box = c.box.union(n.box)
                        k += 2
                        continue
                    }
                    if let comb, c.ch != "€", let p = prev {
                        folded[folded.count - 1].ch = ((p.ch == "ı" ? "i" : p.ch) + comb).precomposedStringWithCanonicalMapping
                        if k + 2 < chars.count, chars[k + 1].ch == " ", chars[k + 2].ch.first?.isLowercase == true { k += 1 }
                        k += 1
                        continue
                    }
                    folded.append(c)
                    k += 1
                }
                chars = folded
            }
            // A letter and its accent PDFKit made one ("e" + U+0301) may still have the space after it the
            // accent's moves left ("Cance´ rologie", "C ˇ uklina"): it goes where the next letter is set
            // closer than the line's word spaces.
            if chars.contains(where: { $0.ch.unicodeScalars.count > 1 && $0.ch.unicodeScalars.dropFirst().allSatisfy { (0x300...0x36F).contains($0.value) } }) {
                let typical = typicalWordGap(chars)
                var kept: [C] = []
                for (k, c) in chars.enumerated() {
                    if c.ch == " ", !c.added, k > 0, k + 1 < chars.count, let p = kept.last,
                       p.ch.unicodeScalars.count > 1, p.ch.unicodeScalars.dropFirst().allSatisfy({ (0x300...0x36F).contains($0.value) }),
                       chars[k + 1].ch.first?.isLetter == true {
                        let gap = chars[k + 1].box.x0 - p.box.x1, em = max(p.size, 4)
                        if gap < 0.15 * em || gap < 0.7 * typical { continue }
                    }
                    kept.append(c)
                }
                chars = kept
            }
            // ligature characters (U+FB00–FB06, "eﬀicient" where a font's ToUnicode gives them) as their
            // letters, so that search, quotes and reading aloud find the word; each takes its share of the box
            if chars.contains(where: { $0.ch.unicodeScalars.count == 1 && (0xFB00...0xFB06).contains($0.ch.unicodeScalars.first!.value) }) {
                var plain: [C] = []
                for c in chars {
                    guard c.ch.unicodeScalars.count == 1, let u = c.ch.unicodeScalars.first, (0xFB00...0xFB06).contains(u.value) else { plain.append(c); continue }
                    let letters = Array(c.ch.precomposedStringWithCompatibilityMapping)
                    let w = c.box.w / CGFloat(max(1, letters.count))
                    for (k, ch) in letters.enumerated() {
                        var d = c
                        d.ch = String(ch)
                        d.box = R(x0: c.box.x0 + w * CGFloat(k), y0: c.box.y0, x1: c.box.x0 + w * CGFloat(k + 1), y1: c.box.y1)
                        plain.append(d)
                    }
                }
                chars = plain
            }
            // the line's baseline as the content stream sets it: where every character has the full
            // line's box, a smaller one is raised or lowered by its own baseline (TeX's θ₁₀, x²)
            // (and wherever a smaller character has its own run, it is raised or lowered by the run's
            // baseline against the line's: a box's middle says little where PDFKit gives a formula's
            // characters tall boxes, and "citation-like" said every small digit was raised, β₃ as β³)
            var mainBase: CGFloat?
            if chars.contains(where: { $0.size > 0 && $0.size < 0.8 * dom }) {
                let bs = mains.compactMap { style($0.box, size: $0.size)?.baseline }.sorted()
                if !bs.isEmpty { mainBase = bs[bs.count / 2] }
            }
            for idx in chars.indices {
                let c = chars[idx]
                let isSpace = c.ch.trimmingCharacters(in: .whitespaces).isEmpty
                var run = last
                var own: TextRun?
                if !isSpace, c.box.w > 0 || c.box.h > 0 {
                    own = style(c.box, size: c.size)
                    run = own ?? last
                    last = run
                }
                text += c.ch
                let small = c.size > 0 && c.size < 0.8 * dom
                let center = (c.box.y0 + c.box.y1) / 2
                // some PDFs give every character the full line box: then fall back to "small citation-like text"
                let flatBoxes = c.box.h > 0.85 * mainHeight
                let citeLike = c.ch.range(of: #"^[0-9,–\-−*†‡§a-e]$"#, options: .regularExpression) != nil
                var sup = !isSpace && small && (flatBoxes ? citeLike : center < mainCenter - 0.12 * dom)
                var sub = !isSpace && small && !flatBoxes && center > mainCenter + 0.12 * dom
                if !isSpace, small, let mb = mainBase, let b = own?.baseline, abs(own!.size - c.size) < 0.6 || flatBoxes {
                    sup = b < mb - 0.15 * dom
                    sub = b > mb + 0.08 * dom
                }
                // a prime is set raised already: not a script of its own (θ′, f′ᵢ)
                if Self.primes.contains(c.ch) { sup = false; sub = false }
                if !isSpace {
                    n += 1
                    if run?.bold == true { nBold += 1 }
                    if run?.math == true { nMath += 1 }
                    if run?.cmRoman == true { nCMR += 1 }
                    if run?.mono == true { nMono += 1 }
                }
                let sp = Span(text: c.ch, bold: run?.bold ?? false, italic: run?.italic ?? false, sup: sup, sub: sub)
                if let lastSpan = spans.last, (isSpace && !lastSpan.sup && !lastSpan.sub) ||
                    (lastSpan.bold == sp.bold && lastSpan.italic == sp.italic && lastSpan.sup == sp.sup && lastSpan.sub == sp.sub) {
                    spans[spans.count - 1].text += c.ch
                } else {
                    spans.append(sp)
                }
            }
            guard !text.trimmingCharacters(in: .whitespaces).isEmpty else { continue }
            // the word goes on in the next line with nothing between (PDFKit dropped a hyphenation hyphen):
            // mark it so the lines are joined without a space
            let end = range.location + range.length
            // (or the line ends in a ligature PDFKit left out, and broke the line at)
            let next = li + 1 < lineSels.count ? lineSels[li + 1].range.location : -1
            let isSpace = { (i: Int) in i < ns.length && (ns.character(at: i) == 0x20 || ns.character(at: i) == 0x0A) }
            /// the box of the next line's first letter: it goes on right after `c` on the same baseline
            func goesOnAfter(_ c: C, at k: Int) -> Bool {
                guard let nb = page.selection(for: NSRange(location: k, length: 1)).map({ toTop($0.bounds(for: page)) }) else { return false }
                let em = max(c.size, 4), gap = nb.x0 - c.box.x1
                return min(nb.y1, c.box.y1) - max(nb.y0, c.box.y0) > 0.5 * min(nb.h, c.box.h) && gap > -0.3 * em
                    && (gap < 0.15 * em || gap < 0.7 * typicalWordGap(chars))
            }
            let firstNext = isLetter(end) ? end : end + 1
            let onAfterLigature = endsWithLigature && next == end && (isLetter(end) || isSpace(end) && isLetter(end + 1))
                && chars.last.map { goesOnAfter($0, at: firstNext) } == true
            // (or PDFKit broke the line after an accented letter, "Falca˜|o", and the word goes on right
            // after it on the same baseline)
            var onAfterAccent = false
            if !onAfterLigature, next == end || next == end + 1 && isSpace(end), isLetter(next), let c = chars.last,
               c.ch.decomposedStringWithCanonicalMapping.unicodeScalars.count > 1 {
                onAfterAccent = goesOnAfter(c, at: next)
            }
            // (the bullet copy left out above may stand between the word's two halves)
            let lastLetter = end >= 2 && ns.character(at: end - 1) == 0x2022 && chars.last?.ch != "•" ? end - 2 : end - 1
            // (or the word goes on in the line after lines of copies left out above: "inves•|s• s• s•|tigated")
            var after = li + 1
            while copies.contains(after) { after += 1 }
            // (PDFKit took out the hyphen there, and put the bullet copy in its place)
            var wordEnd = end - 1
            while wordEnd > range.location, [0x0A, 0x20, 0x2022].contains(ns.character(at: wordEnd)) { wordEnd -= 1 }
            let bulletCopy = wordEnd + 1 < end && ns.character(at: wordEnd + 1) == 0x2022
            let afterAt = after < lineSels.count ? lineSels[after].range.location : -1
            let overCopies = after > li + 1 && bulletCopy && (afterAt == end || afterAt == end + 1 && isSpace(end))
                && isLetter(wordEnd) && chars.last?.ch.first?.isLetter == true && isLetter(afterAt)
                && ns.substring(with: NSRange(location: afterAt, length: 1)).first?.isLowercase == true
            if li + 1 < lineSels.count, next == end && isLetter(lastLetter) && isLetter(end) || overCopies || onAfterLigature || onAfterAccent, !endsHidden,
               text.last.map({ $0.isLetter }) == true {
                text += Self.glue
                // (only PDFKit's own: they tell whether it takes out hyphenation hyphens)
                if !onAfterLigature && !onAfterAccent { gluedLines[(dom * 2).rounded() / 2, default: 0] += 1 }
            }
            let line = PLine(text: text, bbox: bbox, size: dom, bold: CGFloat(nBold) / CGFloat(max(1, n)),
                             math: CGFloat(nMath) / CGFloat(max(1, n)), spans: spans, page: pno)
            line.cmr = CGFloat(nCMR) / CGFloat(max(1, n))
            line.mono = CGFloat(nMono) / CGFloat(max(1, n))
            line.charBoxes = chars.map(\.box)
            // a display equation's number set apart at the line's end ("= 2(f₃ − yᵢ).      (7.11)"): a line
            // of its own, for the equations to be found by (see PDFEquations.swift)
            if let tag = Self.splitEquationTag(line, chars: chars.map { ($0.ch, $0.box, $0.size) }) {
                out.append(line)
                out.append(tag)
                continue
            }
            out.append(line)
        }
        Self.attachFragments(&out)
        for l in out { l.charBoxes = [] }
        return out
    }

    /// PDFKit may put a space where the text goes on 0.18 pt further (pieces of a Type 3 font placed
    /// one after the other: "foundati on model", "real -world"), and keeps the spaces some publishers
    /// put in URLs with no room, to let them break ("https:// creat iveco mmons"). Such a space is
    /// dropped when the content stream shows the text going on there with no room for a space (`joint`,
    /// see PageGraphics.joints) and PDFKit's own glyph boxes leave no room for one either. Both are asked because neither is enough alone: synthetic italics
    /// (a skewed font) widen PDFKit's boxes until real spaces look like none ("et al"), and tightly set
    /// Helvetica has word spaces of 0.1 em. Spaces put back for a line break stay.
    static func isSpuriousSpace(_ ch: String, _ box: R, added: Bool, prev: (String, R, CGFloat)?, next: (String, R, CGFloat)?,
                                joint: Bool) -> Bool {
        guard ch == " ", !added, joint, let (pc, p, ps) = prev, let (nc, n, ns) = next,
              !pc.trimmingCharacters(in: .whitespaces).isEmpty, !nc.trimmingCharacters(in: .whitespaces).isEmpty,
              p.w > 0, n.w > 0, p.h > 0, n.h > 0, ps > 0, ns > 0 else { return false }
        let em = max(ps, ns, 4)
        let sameLine = min(p.y1, n.y1) - max(p.y0, n.y0) > 0.5 * min(p.h, n.h)
        let gap = n.x0 - p.x1
        return sameLine && n.x0 > p.x0 && gap < 0.1 * em && gap > -0.3 * em && box.w < 0.1 * em
    }

    /// Publisher download sheets ("Use of this article is subject to the Terms of service") carry no content.
    static func isCoverSheet(_ page: PDFPage) -> Bool {
        let t = page.string ?? ""
        return t.count < 3000 && (t.contains("Use of this article is subject to the Terms of service")
            || t.contains("View the article online") && t.contains("Permissions"))
    }

    /// A margin line with its numbers left out ("2 of 11", "3of11", "e70999") and, if it has words, the
    /// punctuation at its ends, for finding running heads and feet.
    func norm(_ s: String) -> String {
        let t = s.lowercased().replacingOccurrences(of: #"\d+\s*of\s*\d+"#, with: "", options: .regularExpression)
            .replacingOccurrences(of: #"\b[es]?\d+\b"#, with: "", options: .regularExpression)
            .replacingOccurrences(of: #"\s+"#, with: " ", options: .regularExpression)
        let words = t.trimmingCharacters(in: CharacterSet(charactersIn: " ,.;:|–-"))
        return words.contains(where: \.isLetter) ? words : t.trimmingCharacters(in: .whitespaces)
    }

    private func findRunning() {
        let n = doc.pageCount
        guard n >= 2 else { return }
        var c: [String: Int] = [:]
        for (pno, lines) in pagesLines.enumerated() {
            let h = boxes[pno].height
            var seen = Set<String>()
            for l in lines where l.bbox.y0 < 0.09 * h || l.bbox.y1 > 0.91 * h { seen.insert(norm(l.text)) }
            for s in seen { c[s, default: 0] += 1 }
        }
        running = Set(c.filter { Double($0.value) >= max(2, 0.3 * Double(n)) }.keys)
    }

    /// A superscript at the end of a line that the PDF gives as a line of its own (a footnote's or a
    /// citation's "2" after "outlier."): put back at the end of its line, as a superscript, rather than
    /// dropped with the page numbers.
    static func attachMarkers(_ lines: [PLine], body: CGFloat) -> [PLine] {
        var gone = Set<ObjectIdentifier>()
        for l in lines {
            let t = l.text.trimmingCharacters(in: .whitespaces)
            guard t.count <= 3, t.matches(#"^(\d{1,3}|[*†‡§¶‖]{1,3}|[a-z])$"#), l.size > 0, l.size < 0.85 * body else { continue }
            guard let host = lines.first(where: { h in
                h !== l && !gone.contains(ObjectIdentifier(h)) && h.text.count > 3 && h.size >= 0.85 * body
                    && l.bbox.x0 >= h.bbox.x1 - 1 && l.bbox.x0 - h.bbox.x1 < 0.6 * h.size
                    && l.bbox.y0 >= h.bbox.y0 - 0.5 * h.size && l.bbox.y1 <= h.bbox.y1 + 1 && (l.bbox.y0 + l.bbox.y1) / 2 < (h.bbox.y0 + h.bbox.y1) / 2
            }) else { continue }
            // (in the spans only: the line's text, which decides where paragraphs end, still ends with its
            // full stop)
            host.spans.append(Span(text: t, sup: true))
            host.bbox = host.bbox.union(l.bbox)
            gone.insert(ObjectIdentifier(l))
        }
        return gone.isEmpty ? lines : lines.filter { !gone.contains(ObjectIdentifier($0)) }
    }

    func isNoise(_ l: PLine) -> Bool {
        if !dropped.isEmpty && dropped.contains(ObjectIdentifier(l)) { return true }
        let t = l.text.trimmingCharacters(in: .whitespaces)
        let h = boxes[l.page].height
        let inMargin = l.bbox.y0 < 0.09 * h || l.bbox.y1 > 0.91 * h
        if inMargin && (running.contains(norm(t)) || t.matches(#"^[\divxlcIVXLC\s\-–/of|]+$"#)) { return true }
        // the first page's fuller version of a running foot: "Chemistry – A European Journal, 2026; 32:e70999 https://doi.org/…"
        if inMargin && l.page == 0, case let nt = norm(t), running.contains(where: { $0.count >= 15 && nt.hasPrefix($0) }) { return true }
        if t.matches(#"^\d{1,4}$"#) && l.bbox.w < 25 { return true }
        if t.matches(#"^(bioRxiv|medRxiv) preprint doi"#, .caseInsensitive) || t.contains("copyright holder for this preprint") { return true }
        if inMargin && t.matches(#"^(downloaded from|arxiv:\d{4}\.\d+)"#, .caseInsensitive) { return true }
        if t.allSatisfy({ $0 == "\u{FFFC}" || $0.isWhitespace }) { return true }   // logo placeholder
        if t.matches(#"^check for updates$"#, .caseInsensitive) { return true }   // Crossmark badge
        if t.matches(#"^\((figure )?legend (continued )?on (the )?(next|previous|following) page\)$"#, .caseInsensitive) { return true }
        // publisher footer: "Cell 190, 1–19 … © 2026 Elsevier Inc." / "All rights are reserved, …"
        if l.bbox.y0 > 0.86 * h && l.size <= bodySize + 0.5 && t.count < 200
            && t.matches(#"©|\bAll rights (are )?reserved\b|^for text and data mining\b"#, .caseInsensitive) { return true }
        // first-page citation footer: "Jiang et al., 2027, Cell 190, 1–19"
        if l.page <= 1 && l.bbox.y0 > 0.86 * h && t.matches(#"^[A-Z][\w'’-]+ et al\., \d{4}, .+\d$"#) { return true }
        return false
    }

    // MARK: - Figure / table regions

    struct Region { var rect: R; var kind: Para.Kind; var image: BookImage?; var alt: String? = nil }

    private struct Cluster { var rect: R; var hasImage: Bool; var n: Int }

    func findRegions(_ pno: Int, _ lines: [PLine]) -> [Region] {
        let mb = boxes[pno]
        let parea = mb.width * mb.height
        let g = graphics[pno]
        var rects: [(img: Bool, r: R)] = []
        for r in g.images {
            if (r.w < 1 && r.h < 1) || r.area > 0.95 * parea { continue }
            // a band behind a line of text ("Published online: …" on a shaded strip) is not a picture
            if r.h < 30 && Self.textCover(r, lines) > 0.5 * r.area { continue }
            rects.append((true, r))
        }
        var thin: [R] = []
        for r in g.drawings {
            if r.w < 2.5 || r.h < 2.5 { thin.append(r) } else if r.area < 0.9 * parea { rects.append((false, r)) }
        }
        rects.sort { ($0.r.y0, $0.r.x0) < ($1.r.y0, $1.r.x0) }
        var clusters: [Cluster] = []
        for (img, r) in rects {
            if let li = clusters.indices.last, clusters[li].rect.intersects(r, pad: 6) {
                clusters[li].rect = clusters[li].rect.union(r); clusters[li].hasImage = clusters[li].hasImage || img; clusters[li].n += 1
                continue
            }
            if let ci = clusters.firstIndex(where: { $0.rect.intersects(r, pad: 6) }) {
                clusters[ci].rect = clusters[ci].rect.union(r); clusters[ci].hasImage = clusters[ci].hasImage || img; clusters[ci].n += 1
            } else {
                clusters.append(Cluster(rect: r, hasImage: img, n: 1))
            }
        }
        mergeClusters(&clusters) { a, b in a.intersects(b, pad: 6) }
        // join nearby pieces of one figure unless body text sits between them
        let prose = lines.filter { $0.text.count > 50 && $0.size >= bodySize - 0.5 }.map(\.bbox)
        mergeClusters(&clusters) { a, b in
            guard a.intersects(b, pad: 22) else { return false }
            let u = a.union(b)
            return !prose.contains { u.contains($0, tol: 0) && !$0.intersects(a) && !$0.intersects(b) }
        }
        // (with the rules that touch them, so that a table's or a frame's piece stands for all of it)
        for ci in clusters.indices {
            for r in thin where clusters[ci].rect.intersects(r, pad: 2) { clusters[ci].rect = clusters[ci].rect.union(r) }
        }
        // the rows and columns of one figure's panels sit further apart, their letters and axis labels in
        // between: join pieces while the joined box holds no running text, caption or heading (only short labels)
        let barrier = lines.filter { l in
            let t = l.text.trimmingCharacters(in: .whitespaces)
            return t.count > 50 || (l.size >= bodySize + 1 && t.matches(#"\p{L}{4}"#)) || Self.captionRE.test(t)
        }.map(\.bbox)
        // (a line of them in the box, if only by a few points)
        func hits(_ u: R, _ p: R) -> Bool {
            let o = u.intersection(p)
            return o.w > 1 && o.h > 0.3 * p.h
        }
        func clean(_ u: R) -> Bool { !barrier.contains { hits(u, $0) } }
        // (and two figures side by side, each over its own caption, stay two)
        let capBoxes = lines.filter { Self.captionRE.test($0.text.trimmingCharacters(in: .whitespaces)) }.map(\.bbox)
        func captionBelow(_ r: R) -> R? {
            capBoxes.filter { $0.y0 >= r.y1 - 2 && $0.x1 > r.x0 && $0.x0 < r.x1 }.min { $0.y0 < $1.y0 }
        }
        func oneFigure(_ a: R, _ b: R) -> Bool {
            if let x = captionBelow(a), let y = captionBelow(b) { return x == y }
            return true
        }
        // (not the masthead's or the foot's logos and lines, nor a lone shape such as a frame's corner: only
        // pictures and pieces of several shapes)
        let mTop = 0.07 * mb.height, mBot = 0.93 * mb.height
        let lone = clusters.filter { !$0.hasImage && $0.n < 2 }.map(\.rect)
        func joins(_ r: R) -> Bool { r.y1 >= mTop && r.y0 <= mBot && r.area >= 100 && !lone.contains(r) }
        mergeClusters(&clusters) { a, b in a.intersects(b, pad: 60) && joins(a) && joins(b) && clean(a.union(b)) && oneFigure(a, b) }
        var regions: [Region] = []
        for c in clusters {
            let r = c.rect
            if r.area < 2500 || r.y1 < mTop || r.y0 > mBot { continue }
            let inside = lines.filter { r.contains($0.bbox) }.reduce(0) { $0 + $1.text.count }
            if !c.hasImage && c.n <= 3 && inside > 250 { continue }   // shaded text box
            if !c.hasImage && c.n <= 2 && r.h < 30 { continue }
            // the journal's logo in the first page's masthead
            if pno == 0 && r.y1 < 0.12 * mb.height && r.h < 0.08 * mb.height { continue }
            // a bar down the page's edge ("Author manuscript", "Europe PMC Funders") is no figure, nor on the first
            // page a frame or band that holds running text rather than a picture (the abstract's box; the bar with
            // the header's rules)
            if r.w < 0.06 * mb.width && r.h > 0.4 * mb.height { continue }
            let running = lines.filter { r.contains($0.bbox) && $0.size >= bodySize - 0.5 && Self.isProse($0.text) }.count
            let pictures = g.images.filter { r.contains($0) }.reduce(0) { $0 + $1.area }
            if pno == 0 && running >= 5 && pictures < 0.5 * r.area { continue }
            regions.append(Region(rect: r, kind: .figure))
        }

        // tables: caption + horizontal rules, or a borderless grid of short cells
        // (and in a book an algorithm's box, between its rules: kept as a picture of it, its text as its alternative)
        let caps = lines.filter { l in
            let t = l.text.trimmingCharacters(in: .whitespaces)
            return t.matches(#"^(Supplementary\s+)?Table\s*"# + Self.labelNumber, .caseInsensitive)
                || kind.isBookLike && t.matches(#"^Algorithm\s*"# + Self.labelNumber + #"\s*[:.]"#)
        }
        // a rule drawn in pieces (one per column of the table) is one rule
        var hrules: [R] = []
        for r in thin.filter({ $0.w > 2.5 && $0.h < 2.5 }).sorted(by: { ($0.y0, $0.x0) < ($1.y0, $1.x0) }) {
            if let m = hrules.last, abs(m.y0 - r.y0) < 1, r.x0 <= m.x1 + 2 { hrules[hrules.count - 1] = m.union(r) } else { hrules.append(r) }
        }
        hrules = hrules.filter { $0.w > 80 }.sorted { $0.y0 < $1.y0 }
        if Self.debugGapsPage == pno + 1 {
            print("REGIONS caps \(caps.map { "\($0.text)@\(Int($0.bbox.y0))-\(Int($0.bbox.y1))" }) hrules \(hrules.map { "\(Int($0.x0))-\(Int($0.x1))@\(Int($0.y0))" }) regions \(regions.map { "\(Int($0.rect.y0))-\(Int($0.rect.y1))" })")
        }
        for cap in caps {
            let algorithm = cap.text.trimmingCharacters(in: .whitespaces).hasPrefix("Algorithm")
            // (algorithm boxes stand one under the other: only a box it is in already, which then starts
            // under its caption)
            if algorithm, let k = regions.firstIndex(where: { $0.rect.contains(cap.bbox, tol: 2) }) {
                if regions[k].rect.y0 < cap.bbox.y1, let under = hrules.first(where: { $0.y0 >= cap.bbox.y1 - 1 && $0.y0 < regions[k].rect.y1 }) {
                    regions[k].rect.y0 = under.y0
                }
                continue
            }
            if !algorithm && regions.contains(where: { cap.bbox.intersects($0.rect, pad: 30) }) { continue }
            // (an algorithm's box: from the rule under its caption)
            let below = hrules.filter { algorithm ? $0.y0 >= cap.bbox.y1 - 1 : $0.y0 > cap.bbox.y0 - 2 }
            let above = hrules.filter { $0.y1 < cap.bbox.y0 + 2 }
            // the table's rules are about as wide as the one next to its caption: a rule much wider (the
            // running head's rule over both columns of the page) or narrower (a table in the other column) is
            // not one of them, unless that leaves the table a single rule
            func sameTable(_ a: R, _ r: R) -> Bool { r.w <= 1.5 * a.w && r.w >= 0.6 * a.w && min(r.x1, a.x1) - max(r.x0, a.x0) > 0.5 * r.w }
            // (and a table ends before another table's or figure's caption)
            func otherCaption(_ y0: CGFloat, _ y1: CGFloat) -> Bool {
                lines.contains { $0 !== cap && $0.bbox.y0 > y0 && $0.bbox.y1 < y1 && Self.captionRE.test($0.text.trimmingCharacters(in: .whitespaces)) }
            }
            func group(_ onlySame: Bool) -> [R]? {
                if let f = below.first, f.y0 - cap.bbox.y1 < 60 {
                    var gg = [f]
                    for r in below.dropFirst() where !onlySame || sameTable(f, r) {
                        if r.y0 - gg.last!.y1 < 250 && !otherCaption(gg.last!.y1, r.y0) { gg.append(r) } else { break }
                    }
                    return gg
                } else if let l = above.last, cap.bbox.y0 - l.y1 < 60 {
                    var gg = [l]
                    for r in above.dropLast().reversed() where !onlySame || sameTable(l, r) {
                        if gg.last!.y0 - r.y1 < 250 && !otherCaption(r.y1, gg.last!.y0) { gg.append(r) } else { break }
                    }
                    return gg
                }
                return nil
            }
            let grp = group(true).flatMap { $0.count >= 2 ? $0 : nil } ?? group(false)
            if let grp, grp.count >= 2 {
                let r = R.union(grp)
                // (an algorithm's text as its picture's alternative)
                let alt = algorithm ? lines.filter { r.contains($0.bbox, tol: 2) }.sorted { ($0.bbox.y0, $0.bbox.x0) < ($1.bbox.y0, $1.bbox.x0) }
                    .map { lineEndsCleaned($0.text).trimmingCharacters(in: .whitespaces) }.joined(separator: " ") : nil
                regions.append(Region(rect: r, kind: algorithm ? .figure : .table, alt: alt))
            } else if algorithm {
                continue
            } else if let tr = textTableRegion(cap, lines, mb) {
                regions.append(Region(rect: tr, kind: .table))
            }
        }
        // expand regions with nearby short labels (axis labels, panel letters); not a caption, nor the line
        // beside one of this region's (its continuation in the other column)
        for gi in regions.indices {
            var r = regions[gi].rect
            for _ in 0..<2 {
                for l in lines {
                    let t = l.text.trimmingCharacters(in: .whitespaces)
                    if capBoxes.contains(where: { min($0.y1, l.bbox.y1) - max($0.y0, l.bbox.y0) > 0.5 * l.bbox.h && $0.x1 > r.x0 && $0.x0 < r.x1 }) {
                        continue
                    }
                    let vo = min(r.y1, l.bbox.y1) - max(r.y0, l.bbox.y0) > 0.6 * l.bbox.h
                    // (a line of running text beside it is no label)
                    if r.intersects(l.bbox, pad: 8) && t.count < 70 && !(t.count > 50 && l.size >= bodySize - 0.5 && Self.isProse(t)) && !r.contains(l.bbox)
                        && (l.size < bodySize - 0.4 || vo || t.count <= 12) {
                        r = r.union(l.bbox)
                    }
                }
            }
            regions[gi].rect = r
        }
        // pieces of a figure that have grown into each other by their labels are one figure
        var merged = true
        while merged {
            merged = false
            outer: for i in regions.indices {
                for j in (i + 1)..<regions.count {
                    let a = regions[i].rect, b = regions[j].rect
                    if regions[i].kind == .figure && regions[j].kind == .figure && a.intersects(b) && clean(a.union(b))
                        && oneFigure(a, b) {
                        regions[i].rect = a.union(b)
                        regions.remove(at: j)
                        merged = true
                        break outer
                    }
                }
            }
        }
        // a piece that has grown inside another (by its labels) is part of it
        let all = regions
        return all.indices.filter { i in
            !all.indices.contains { j in
                j != i && all[j].kind == all[i].kind && all[j].rect.contains(all[i].rect, tol: 2)
                    && (!all[i].rect.contains(all[j].rect, tol: 2) || j < i)
            }
        }.map { all[$0] }
    }

    /// How much of `r` the text lines cover (their overlaps added up).
    static func textCover(_ r: R, _ lines: [PLine]) -> CGFloat {
        lines.reduce(0) { acc, l in
            let o = r.intersection(l.bbox)
            return acc + (o.w > 0 && o.h > 0 ? o.area : 0)
        }
    }

    /// A footnote of the first pages that belongs to the front matter, not the body: correspondence and
    /// e-mail, "A full list of affiliations…", dates, editor, competing interests, ORCID, licence. In
    /// footnote type anywhere on the page, or in body type at its foot if it starts like one; a sentence of
    /// the body that merely gives an address ("Requests to x@y") is in body type and does not start like a
    /// footnote.
    static func frontFootnote(_ text: String, size: CGFloat, body: CGFloat, atFoot: Bool) -> Bool {
        let t = text.trimmingCharacters(in: .whitespacesAndNewlines)
        if t.count > 1200 || t.matches(#"(?i)^(data|code|materials?|software)( and \w+)? availability"#) { return false }
        let starts = t.matches(#"(?i)^[\W\d]{0,3}\s*("#
            + #"corresponding authors?|\*?\s*correspondence\b|(to )?whom (all )?correspondence|address(ed)? (all )?correspondence|"#
            + #"e-?mail( address(es)?)?\s*:|email\b|edited by\b|reviewed by\b|academic editor\b|handling editor\b|"#
            + #"(received|accepted|revised|published( online)?)\s*[:\d]|published online\b|available online\b|first published\b|article history\b|"#
            + #"author contributions?\s*:|competing interests?\s*:|conflicts? of interests?\s*:|the authors? declares?\b|"#
            + #"orcid\b|these authors contributed|present address|current address|deceased\b|a full list of affiliations|"#
            + #"©|copyright\b|\(c\)\s*(19|20)\d\d|this (is an )?open access|open access\b|this article (is a pnas|is an open|is licensed|is distributed|contains supporting)|"#
            + #"https?://doi\.org/|doi:\s*10\."#
            + #")"#)
        let anywhere = t.matches(#"(?i)✉|\ba full list of affiliations\b|\breceived in revised form\b|\bPNAS Direct Submission\b|"#
            + #"\bto whom correspondence\b|\bcorresponding authors?\b|\bcontributed equally\b"#)
        if size <= body - 0.4 { return starts || anywhere || t.matches(#"[\w.+-]+@[\w-]+(\.[\w-]+)+"#) }
        return atFoot && (starts || anywhere)
    }

    private func mergeClusters(_ cs: inout [Cluster], _ shouldMerge: (R, R) -> Bool) {
        var merged = true
        while merged {
            merged = false
            outer: for i in cs.indices {
                for j in (i + 1)..<cs.count where shouldMerge(cs[i].rect, cs[j].rect) {
                    cs[i].rect = cs[i].rect.union(cs[j].rect)
                    cs[i].hasImage = cs[i].hasImage || cs[j].hasImage
                    cs[i].n += cs[j].n
                    cs.remove(at: j)
                    merged = true
                    break outer
                }
            }
        }
    }

    private func textTableRegion(_ cap: PLine, _ lines: [PLine], _ mb: CGRect) -> R? {
        let (x0, x1): (CGFloat, CGFloat) = cap.bbox.w < 0.6 * mb.width ? (cap.bbox.x0 - 5, cap.bbox.x1 + 5) : (0, mb.width)
        var rows: [Int: [PLine]] = [:]
        for l in lines where l !== cap && l.bbox.x0 >= x0 - 40 && l.bbox.x1 <= x1 + 40 {
            rows[Int((l.bbox.y0 / 3).rounded()), default: []].append(l)
        }
        let ys = rows.keys.sorted()
        func runFrom(_ startY: CGFloat, down: Bool) -> R? {
            let seq = down ? ys.filter { CGFloat($0) * 3 >= startY } : ys.filter { CGFloat($0) * 3 <= startY }.reversed()
            var got: [Int] = []
            var last: CGFloat?
            var nMulti = 0
            for y in seq {
                let row = rows[y]!
                let ry = down ? row.map(\.bbox.y0).min()! : row.map(\.bbox.y1).max()!
                if let last, abs(ry - last) > 40 { break }
                let multi = row.count >= 2
                if !multi && row.reduce(0, { $0 + $1.text.count }) > 90 { break }
                got.append(y)
                if multi { nMulti += 1 }
                last = down ? row.map(\.bbox.y1).max()! : row.map(\.bbox.y0).min()!
            }
            if got.count >= 3 && Double(nMulti) >= 0.5 * Double(got.count) {
                return R.union(got.flatMap { rows[$0]! }.map(\.bbox))
            }
            return nil
        }
        let up = cap.bbox.y0 > mb.height * 0.25 ? runFrom(cap.bbox.y0 - 1, down: false) : nil
        let down = runFrom(cap.bbox.y1 + 1, down: true)
        return [up, down].compactMap { $0 }.max { $0.area < $1.area }
    }

    // MARK: - Rendering crops

    func render(_ pno: Int, _ rect: R, photo: Bool, name: String) -> BookImage? {
        guard let page = doc.page(at: pno) else { return nil }
        let mb = boxes[pno]
        var r = R(x0: rect.x0 - 4, y0: rect.y0 - 4, x1: rect.x1 + 4, y1: rect.y1 + 4)
            .intersection(R(x0: 0, y0: 0, x1: mb.width, y1: mb.height))
        guard r.w > 2, r.h > 2 else { return nil }
        let scale = min(200.0 / 72.0, 1600 / max(r.w, r.h))
        let W = Int((r.w * scale).rounded()), H = Int((r.h * scale).rounded())
        guard let cs = CGColorSpace(name: CGColorSpace.sRGB),
              let ctx = CGContext(data: nil, width: W, height: H, bitsPerComponent: 8, bytesPerRow: 0, space: cs,
                                  bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue) else { return nil }
        ctx.setFillColor(CGColor(red: 1, green: 1, blue: 1, alpha: 1))
        ctx.fill(CGRect(x: 0, y: 0, width: W, height: H))
        ctx.scaleBy(x: scale, y: scale)
        ctx.translateBy(x: -r.x0, y: -(mb.height - r.y1))
        page.draw(with: .mediaBox, to: ctx)
        guard let img = ctx.makeImage() else { return nil }
        r = rect
        imageCounter += 1
        let name = uniqueName(name)
        if photo, let d = Images.jpeg(img, quality: 0.85) {
            return BookImage(fileName: "\(name).jpg", data: d, mediaType: "image/jpeg")
        }
        let out = NSMutableData()
        guard let dest = CGImageDestinationCreateWithData(out, "public.png" as CFString, 1, nil) else { return nil }
        CGImageDestinationAddImage(dest, img, nil)
        guard CGImageDestinationFinalize(dest) else { return nil }
        return BookImage(fileName: "\(name).png", data: out as Data, mediaType: "image/png")
    }

    // MARK: - Assembly

    func analyze() -> [Para] {
        extract()
        decideKind()
        var all: [Para] = []
        var sidenotes: [Para] = []
        let pageLines = pagesLines.map { Self.attachMarkers($0, body: bodySize).filter { !isNoise($0) } }
        var pageRegions = pageLines.indices.map { findRegions($0, pageLines[$0]) }
        // logos and banners that sit in the same place on many pages are not figures
        func key(_ r: R) -> String { "\(Int(r.x0 / 6)),\(Int(r.y0 / 6)),\(Int(r.x1 / 6)),\(Int(r.y1 / 6))" }
        var counts: [String: Int] = [:]
        for rs in pageRegions { for k in Set(rs.map { key($0.rect) }) { counts[k, default: 0] += 1 } }
        let limit = max(3, Int(0.3 * Double(pageRegions.count)))
        pageRegions = pageRegions.map { rs in rs.filter { (counts[key($0.rect)] ?? 0) < limit } }
        for pno in pagesLines.indices {
            let lines = pageLines[pno]
            var regions = pageRegions[pno]
            for i in regions.indices {
                let rr = regions[i].rect
                let photo = graphics[pno].images.contains { rr.contains($0, tol: 5) && $0.area > 0.3 * rr.area }
                regions[i].image = autoreleasepool {
                    render(pno, rr, photo: photo, name: "p\(pno + 1)-\(regions[i].kind == .table ? "table" : "figure")\(i + 1)")
                }
            }
            var free = lines.filter { l in
                !regions.contains { g in
                    g.rect.contains(l.bbox, tol: 3) || (l.bbox.area > 0 && g.rect.intersects(l.bbox)
                        && g.rect.intersection(l.bbox).area > 0.6 * l.bbox.area)
                }
            }
            // a book's margin notes: out of the way of the page's paragraphs, placed after classifying
            if kind.isBookLike {
                let (rest, notes) = marginNotes(pno, free)
                free = rest
                sidenotes += notes
            }
            var (rest, blocks) = displayEquations(pno, free, regions)
            // a book's code listings
            if kind.isBookLike {
                let (r2, code) = codeBlocks(pno, rest)
                rest = r2
                blocks += code
            }
            all += paragraphs(pno, rest, regions, equations: blocks)
            if pno % 5 == 4 { log("Laying out page \(pno + 1) of \(doc.pageCount)") }
        }
        // (a margin text on many pages is the book's furniture, a part's name in the margin: not a note)
        var marginPages: [String: Set<Int>] = [:]
        // ("Problem 2.1" and "Problem 2.2" are not the same text)
        let key = { (p: Para) in p.text.lowercased().trimmingCharacters(in: .whitespaces) }
        for n in sidenotes { marginPages[key(n), default: []].insert(n.page) }
        sidenotes.removeAll { (marginPages[key($0)]?.count ?? 0) >= 4 }
        if !sidenotes.isEmpty { log("Margin notes: \(sidenotes.count)") }
        return findFootnotes(Self.placeSidenotes(classify(all), sidenotes))
    }

    func paragraphs(_ pno: Int, _ input: [PLine], _ regions: [Region], equations: [Para] = []) -> [Para] {
        // merge fragments on the same baseline (PDFKit sometimes splits a line)
        // pieces of one line PDFKit gave apart, touching (within half an em), the same type, level by their
        // tops or, for a piece raised by a superscript in it, by their bottoms: one line
        func level(_ a: PLine, _ b: PLine) -> Bool {
            abs(a.bbox.y0 - b.bbox.y0) < 2 || abs(a.bbox.y1 - b.bbox.y1) < 2
                && min(a.bbox.y1, b.bbox.y1) - max(a.bbox.y0, b.bbox.y0) > 0.6 * min(a.bbox.h, b.bbox.h)
        }
        func join(_ m: PLine, _ l: PLine) {
            // (the joined line's share of bold and math type: a run-in head "Forward pass:" and the text
            // after it are one line of mostly regular type, not a bold line that breaks off as a heading)
            let a = CGFloat(m.text.count), b = CGFloat(l.text.count)
            if a + b > 0 {
                m.bold = (m.bold * a + l.bold * b) / (a + b)
                m.math = (m.math * a + l.math * b) / (a + b)
                m.cmr = (m.cmr * a + l.cmr * b) / (a + b)
            }
            if m.text.hasSuffix(Self.glue) {   // the word goes on (PDFKit broke the line at a ligature)
                m.text.removeLast()
                m.text += l.text
                m.spans += l.spans
            } else if let c = l.text.first.map(String.init), Self.noSpaceBefore.contains(c), l.bbox.x0 - m.bbox.x1 < 0.15 * l.size {
                m.text += l.text   // ("described⁶⁸" + ", with")
                m.spans += l.spans
            } else {
                m.text += (m.text.hasSuffix(" ") ? "" : " ") + l.text
                m.spans += [Span(text: " ")] + l.spans
            }
            m.bbox = m.bbox.union(l.bbox)
        }
        var pieces = input.sorted { $0.bbox.x0 < $1.bbox.x0 }
        var k = 0
        while k < pieces.count {
            let l = pieces[k]
            if let m = pieces[..<k].last(where: { m in
                level(m, l) && abs(m.size - l.size) < 0.5 && l.bbox.x0 >= m.bbox.x1 - 1 && l.bbox.x0 - m.bbox.x1 < 0.5 * l.size
            }) {
                join(m, l)
                pieces.remove(at: k)
                continue
            }
            k += 1
        }
        let sorted = pieces.sorted { (($0.bbox.y0).rounded(), $0.bbox.x0) < (($1.bbox.y0).rounded(), $1.bbox.x0) }
        var merged: [PLine] = []
        for l in sorted {
            if let m = merged.last, abs(m.bbox.y0 - l.bbox.y0) < 2, l.bbox.x0 >= m.bbox.x1 - 1, l.bbox.x0 - m.bbox.x1 < 1.2 * l.size {
                join(m, l)
            } else {
                merged.append(l)
            }
        }
        var units = merged.map { Para([$0], page: pno) }
        for g in regions {
            let p = Para([], page: pno, kind: g.kind, region: g.rect)
            p.asset = g.image
            p.alt = g.alt
            units.append(p)
        }
        units += equations
        let (ordered, cols) = order(pno, units)
        units = ordered
        let txt = zip(units, cols).filter { $0.0.kind == .text }.map { ($0.0.lines[0].bbox, $0.1) }
        func rightEdge(_ l: PLine, _ col: (CGFloat, CGFloat)) -> CGFloat {
            // the column's right edge: lines of other columns (full-width legends, summary) do not count
            let sameColumn = txt.filter { $0.1 == col }.map(\.0)
            let ov = sameColumn.filter { min($0.x1, l.bbox.x1) - max($0.x0, l.bbox.x0) > 0.3 * l.bbox.w }.map(\.x1)
            return ov.max() ?? l.bbox.x1
        }
        let abbrev = re(#"(\b(e\.g|i\.e|et al|cf|Fig|Figs|Eq|Eqs|vs|approx|ca|resp|Ref|Refs|No|Dr|Prof|al)\.|\b[A-Z]\.)$"#)
        let W = boxes[pno].width
        var paras: [Para] = []
        var cur: [PLine] = []
        var curCol: (CGFloat, CGFloat) = (0, 0)
        var curBox: Int?
        // where the paragraph's lines in the current column start (after a column continuation)
        var seg = 0
        func flush() {
            if !cur.isEmpty { let p = Para(cur, page: pno); p.boxed = curBox; paras.append(p); cur = [] }
        }
        for (u, col) in zip(units, cols) {
            if u.kind != .text { flush(); paras.append(u); continue }
            if u.boxed != curBox {
                // a box's text never runs on into the page's or the other way round
                flush()
                curBox = u.boxed
            }
            let l = u.lines[0]
            let aCol = curCol
            curCol = col
            guard let a = cur.last else { seg = 0; cur.append(l); continue }
            let left = cur[seg...].map(\.bbox.x0).min()!
            let gap = l.bbox.y0 - a.bbox.y1
            let jumped = l.bbox.y0 < a.bbox.y0 - 2 || abs(l.bbox.x0 - left) > 0.3 * W
            let reA = rightEdge(a, aCol)
            let width = max(1, reA - left)
            let at = a.text.trimmingCharacters(in: .whitespaces)
            let endsSentence = at.hasSuffix(".") || at.hasSuffix(":") || at.hasSuffix("?") || at.hasSuffix("!")
            let shortA = (reA - a.bbox.x1) > 0.18 * width && endsSentence && !abbrev.test(at)
            // (not the hanging indent of a numbered reference: "4. Satija, R., … (2015)." / "Spatial …")
            let numberedItem = cur[0].text.trimmingCharacters(in: .whitespaces).matches(#"^(\[\d+\]|\d+\.)\s"#)
            let indent = l.bbox.x0 - left > 8 && cur[max(seg, 1)...].allSatisfy { abs($0.bbox.x0 - left) < 3 } && endsSentence
                && !abbrev.test(at) && !numberedItem
            // (a drop cap is not a hanging first line)
            let dropCap = cur[0].bbox.h > 2.5 * cur[0].size
            let hanging = cur.count >= 2 && l.bbox.x0 < a.bbox.x0 - 6 && cur[0].bbox.x0 < a.bbox.x0 - 6 && !dropCap
            let sizeChange = abs(l.size - a.size) >= 1.0
            let styleBreak = (a.bold > 0.9) != (l.bold > 0.9) && (at.count < 120 || l.text.count < 120)
            let cap = Self.captionRE.test(l.text.trimmingCharacters(in: .whitespaces))
            // a bullet starts a list item; the item's next lines are indented to its text
            let bullet = Self.isBullet(l), inItem = Self.isBullet(cur[0])
            if jumped || gap > 0.75 * max(l.size, a.size) || shortA || indent || hanging || sizeChange || styleBreak || cap || bullet
                || (cur.count == 1 && l.bbox.x0 > a.bbox.x0 + 8 && gap > 0.3 * l.size && !numberedItem && !inItem) {
                if jumped && !(gap > 0.75 * l.size) && !shortA && !sizeChange && !cap
                    && l.text.trimmingCharacters(in: .whitespaces).first?.isLowercase == true && !Self.isDOILine(l.text) {
                    seg = cur.count
                    cur.append(l); continue   // column continuation
                }
                flush()
                seg = 0
            }
            cur.append(l)
        }
        flush()
        return paras
    }

    /// A block for the reading order: one line or region, or a box of them (`items`: their indices).
    private struct Node { var r: R; var n: Int; var text: Bool; var items: [Int] }

    /// Reading order of a page's lines and blocks, with the column each was read in: the x-interval
    /// between the gutters on either side of it (lines of one column share it).
    ///
    /// Recursive: a gutter splits the page into columns, and blocks across it (titles, full-width
    /// figures and legends) cut it into bands that are ordered the same way. So three columns, a
    /// two-column legend under a full-width figure, or columns of different widths above and below a
    /// figure each come out column by column. A framed or shaded box is read as one block.
    func order(_ pno: Int, _ paras: [Para]) -> ([Para], [(CGFloat, CGFloat)]) {
        let singles = paras.enumerated().map { Node(r: $1.bbox, n: $1.text.count, text: $1.kind == .text, items: [$0]) }
        let panels = self.panels(pno, singles)
        let inPanel = Set(panels.joined())
        var nodes = singles.filter { !inPanel.contains($0.items[0]) }
        for (k, m) in panels.enumerated() {
            for i in m { paras[i].boxed = pno * 64 + k }
            nodes.append(Node(r: R.union(m.map { singles[$0].r }), n: m.reduce(0) { $0 + singles[$1].n }, text: true, items: m))
        }
        nodes.sort { $0.items[0] < $1.items[0] }
        var out: [(Int, (CGFloat, CGFloat))] = []
        orderRec(pno, singles, nodes, depth: 0, inherited: nil, key: (-.infinity, .infinity), &out)
        return (out.map { paras[$0.0] }, out.map(\.1))
    }

    /// Framed or shaded boxes (a drawn rectangle around several lines of text that no line crosses,
    /// holding part of the page's text): the members of each, outermost boxes only.
    private func panels(_ pno: Int, _ nodes: [Node]) -> [[Int]] {
        let W = boxes[pno].width, H = boxes[pno].height
        let allN = nodes.filter(\.text).reduce(0) { $0 + $1.n }
        let rects = graphics[pno].drawings.filter { $0.w >= 0.2 * W && $0.h >= 3 * bodySize && $0.area < 0.9 * W * H }
            .sorted { $0.area > $1.area }
        var out: [[Int]] = []
        for r in rects {
            var members: [Int] = []
            var ok = true
            for (i, n) in nodes.enumerated() {
                if r.contains(n.r, tol: 2) {
                    if !n.text { ok = false; break }   // a figure inside: left to the figure detection
                    members.append(i)
                } else if n.r.area > 0 && r.intersects(n.r) && r.intersection(n.r).area > 0.3 * n.r.area {
                    ok = false; break   // text across the frame: not a box
                }
            }
            let n = members.reduce(0) { $0 + nodes[$1].n }
            if !ok || members.count < 3 || n < 100 || Double(n) > 0.6 * Double(allN) { continue }
            if out.contains(where: { m in members.contains { m.contains($0) } }) { continue }   // inside a box already found
            out.append(members)
        }
        return out
    }

    /// The best column gutter among `nodes`: the x that most often separates two lines side by side on
    /// the same row, decided row by row so that full-width blocks do not hide the columns. Of several
    /// gutters (three columns) the one fewest blocks cross. Also returns how many rows it separates.
    private func findGutter(_ pno: Int, _ nodes: [Node], depth: Int) -> (CGFloat, Int) {
        let W = boxes[pno].width
        let textual = nodes.filter { $0.text && $0.r.w >= 0.15 * W }
        var gaps: [(CGFloat, CGFloat)] = []
        for (ai, a) in textual.enumerated() {
            var rightOf: CGFloat?
            for (bi, b) in textual.enumerated() where bi != ai && b.r.x0 >= a.r.x1 + 3 && b.r.x0 - a.r.x1 < 0.4 * W
                && min(a.r.y1, b.r.y1) - max(a.r.y0, b.r.y0) > 0.5 * min(a.r.h, b.r.h) {
                rightOf = min(rightOf ?? b.r.x0, b.r.x0)
            }
            if let rightOf { gaps.append((a.r.x1, rightOf)) }
        }
        var lo = 0.25 * W, hi = 0.75 * W
        if depth > 0, let x0 = nodes.map(\.r.x0).min(), let x1 = nodes.map(\.r.x1).max() {
            lo = x0 + 0.25 * (x1 - x0); hi = x1 - 0.25 * (x1 - x0)
        }
        var cover: [(x: CGFloat, n: Int)] = []
        var x = lo
        while x <= hi {
            cover.append((x, gaps.filter { $0.0 < x && x < $0.1 }.count))
            x += 1
        }
        guard let maxN = cover.map(\.n).max() else { return (0.5 * W, 0) }
        if maxN == 0 {
            // no two lines side by side (a summary page: the graphical abstract above the Highlights on
            // the left, the authors on the right): the middle of the widest stretch the fewest blocks
            // cross, not the middle of the page, which may cut through the left column's longer lines
            let crossing = cover.map { c in nodes.filter { crosses(pno, $0, c.x) }.count }
            guard let least = crossing.min() else { return (0.5 * W, 0) }
            var run: (lo: Int, hi: Int)?
            var k = 0
            while k < crossing.count {
                guard crossing[k] == least else { k += 1; continue }
                var m = k
                while m + 1 < crossing.count && crossing[m + 1] == least { m += 1 }
                if run.map({ m - k > $0.hi - $0.lo }) ?? true { run = (k, m) }
                k = m + 1
            }
            let (a, b) = run ?? (0, cover.count - 1)
            return ((cover[a].x + cover[b].x) / 2, 0)
        }
        // stretches covered nearly as often as the best one; in each, the middle of its longest most-covered run
        let thr = maxN == 0 ? 0 : max(1, Int((0.7 * Double(maxN)).rounded(.up)))
        var best: (crossing: Int, peak: Int, run: Int, x: CGFloat)?
        var i = 0
        while i < cover.count {
            guard cover[i].n >= thr else { i += 1; continue }
            var j = i
            while j + 1 < cover.count && cover[j + 1].n >= thr { j += 1 }
            let peak = cover[i...j].map(\.n).max() ?? 0
            var run: (lo: Int, hi: Int)?
            var k = i
            while k <= j {
                guard cover[k].n == peak else { k += 1; continue }
                var m = k
                while m < j && cover[m + 1].n == peak { m += 1 }
                if run.map({ m - k > $0.hi - $0.lo }) ?? true { run = (k, m) }
                k = m + 1
            }
            let (a, b) = run ?? (i, j)
            let g = (cover[a].x + cover[b].x) / 2
            let crossing = nodes.filter { crosses(pno, $0, g) }.count
            if best.map({ (crossing, -peak, -(b - a)) < ($0.crossing, -$0.peak, -$0.run) }) ?? true {
                best = (crossing, peak, b - a, g)
            }
            i = j + 1
        }
        return best.map { ($0.x, $0.peak) } ?? (0.5 * W, 0)
    }

    /// A block across the gutter at `g` (text never crosses it unless it spans the columns; a figure's
    /// box, grown by its labels, may stick out of its column a little).
    private func crosses(_ pno: Int, _ n: Node, _ g: CGFloat) -> Bool {
        let W = boxes[pno].width
        let m: CGFloat = n.text ? 3 : 0.05 * W
        return (n.r.x0 < g - m && n.r.x1 > g + m) || (!n.text && n.r.w >= 0.55 * W)
    }

    private func orderRec(_ pno: Int, _ singles: [Node], _ nodes: [Node], depth: Int, inherited: CGFloat?,
                          key: (CGFloat, CGFloat), _ out: inout [(Int, (CGFloat, CGFloat))]) {
        func leaf(_ nodes: [Node], _ out: inout [(Int, (CGFloat, CGFloat))]) {
            let sorted = depth == 0
                ? nodes.sorted { (($0.r.y0 / 4).rounded(), $0.r.x0) < (($1.r.y0 / 4).rounded(), $1.r.x0) }
                : nodes.sorted { $0.r.y0 < $1.r.y0 }
            for n in sorted {
                if n.items.count == 1 { out.append((n.items[0], key)); continue }
                // a box: its own lines in their own order
                orderRec(pno, singles, n.items.map { singles[$0] }, depth: depth + 1, inherited: nil, key: (n.r.x0, n.r.x1), &out)
            }
        }
        if nodes.count <= 1 || depth > 12 { leaf(nodes, &out); return }
        let W = boxes[pno].width
        let found = findGutter(pno, nodes, depth: depth)
        var gutter = found.0
        let maxN = found.1
        let columns: Bool
        if depth == 0 {
            // pages with little text (end matter): fall back on "most text sits in one half or the other"
            let narrow = nodes.filter { $0.text && $0.r.w < 0.55 * W }
            let leftN = narrow.filter { $0.r.x1 <= gutter }.reduce(0) { $0 + $1.n }
            let rightN = narrow.filter { $0.r.x0 >= gutter }.reduce(0) { $0 + $1.n }
            let allN = max(1, nodes.filter(\.text).reduce(0) { $0 + $1.n })
            let halves = Double(leftN + rightN) > 0.5 * Double(allN) && Double(min(leftN, rightN)) > 0.2 * Double(leftN + rightN)
            columns = maxN >= 4 || halves
        } else if maxN >= 4 {
            columns = true
        } else if let g = inherited, !nodes.contains(where: { crosses(pno, $0, g) }) {
            // a band too short to show its columns: the page's
            gutter = g
            columns = nodes.contains { $0.r.cx < g } && nodes.contains { $0.r.cx >= g }
        } else {
            columns = false
        }
        guard columns else { leaf(nodes, &out); return }
        let g = gutter
        let wide = nodes.filter { crosses(pno, $0, g) }.sorted { $0.r.y0 < $1.r.y0 }
        let rest = nodes.filter { !crosses(pno, $0, g) }
        if wide.isEmpty {
            let left = rest.filter { $0.r.cx < g }, right = rest.filter { $0.r.cx >= g }
            if left.isEmpty || right.isEmpty { leaf(left + right, &out); return }
            orderRec(pno, singles, left, depth: depth + 1, inherited: nil, key: (key.0, g), &out)
            orderRec(pno, singles, right, depth: depth + 1, inherited: nil, key: (g, key.1), &out)
            return
        }
        // blocks across the gutter, one after the other, form full-width zones; text beside their lines
        // (the other half of a two-column legend) belongs to the zone
        let tol = 0.8 * bodySize
        func lineLike(_ n: Node) -> Bool { n.text && n.r.h <= 3 * bodySize }
        var zones: [(y0: CGFloat, y1: CGFloat, lines: (CGFloat, CGFloat)?, members: [Node])] = []
        for n in wide {
            let lines: (CGFloat, CGFloat)? = lineLike(n) ? (n.r.y0, n.r.y1) : nil
            if let z = zones.last, n.r.y0 <= z.y1 + tol {
                zones[zones.count - 1].y1 = max(z.y1, n.r.y1)
                if let (a, b) = lines {
                    zones[zones.count - 1].lines = z.lines.map { (min($0.0, a), max($0.1, b)) } ?? (a, b)
                }
                zones[zones.count - 1].members.append(n)
            } else {
                zones.append((n.r.y0, n.r.y1, lines, [n]))
            }
        }
        var bandNodes: [Node] = []
        for n in rest {
            let c = (n.r.y0 + n.r.y1) / 2
            if lineLike(n), let zi = zones.firstIndex(where: { z in z.lines.map { $0.0 < c && c < $0.1 } ?? false }) {
                zones[zi].members.append(n)
            } else {
                bandNodes.append(n)
            }
        }
        if bandNodes.isEmpty && zones.count == 1 { leaf(zones[0].members, &out); return }
        var prev = -CGFloat.infinity
        for z in zones {
            let inBand = { (n: Node) in prev <= n.r.y0 && n.r.y0 < z.y0 }
            let band = bandNodes.filter(inBand)
            bandNodes = bandNodes.filter { !inBand($0) }
            orderRec(pno, singles, band, depth: depth + 1, inherited: g, key: key, &out)
            orderRec(pno, singles, z.members, depth: depth + 1, inherited: nil, key: key, &out)
            prev = z.y0
        }
        orderRec(pno, singles, bandNodes, depth: depth + 1, inherited: g, key: key, &out)
    }

    func classify(_ input: [Para]) -> [Para] {
        let body = bodySize
        // glue wrapped large-font lines (titles, long headings)
        var paras: [Para] = []
        for p in input {
            if let q = paras.last, p.kind == .text, q.kind == .text, p.page == q.page, p.boxed == q.boxed, p.size == q.size,
               p.size >= body + 1, p.bbox.y0 - q.bbox.y1 >= 0, p.bbox.y0 - q.bbox.y1 < 0.9 * p.size,
               q.text.count + p.text.count < 400 {
                q.lines += p.lines
            } else {
                paras.append(p)
            }
        }
        if let big = paras.filter({ $0.page == 0 && $0.kind == .text && $0.boxed == nil }).max(by: { $0.size < $1.size }),
           big.size >= body + 2, big.text.count < 400 {
            big.kind = .title
        }
        // the masthead above the title (the journal's logo, an "ARTICLE | Open Access" banner) is no figure
        if let title = paras.first(where: { $0.page == 0 && $0.kind == .title }) {
            let ty = title.bbox.y0, h = boxes[0].height
            paras.removeAll { $0.page == 0 && $0.kind == .figure && $0.bbox.y1 <= ty + 2 && $0.bbox.h < 0.25 * h }
        }
        var headingSizes = Set<CGFloat>()
        // (a box's own headings and labels stay in the box: plain text)
        for p in paras where p.kind == .text && p.boxed == nil {
            let t = p.text.trimmingCharacters(in: .whitespaces)
            if Self.captionRE.test(t) { p.kind = .caption; continue }
            let n = t.count
            let totalChars = max(1, p.lines.reduce(0) { $0 + $1.text.count })
            let boldish = p.lines.reduce(CGFloat(0)) { $0 + $1.bold * CGFloat($1.text.count) } / CGFloat(totalChars)
            let numbered = t.matches(#"^(\d+(?:\.\d+)*)\.?\s+[A-Z]"#)
            let endsSentence = t.hasSuffix(".") && n > 60
            if n <= 160 && p.lines.count <= 3 && !endsSentence {
                if p.size >= body + 1 || Self.knownSections.test(t) || (boldish > 0.85 && n < 110 && p.size >= body - 0.6)
                    || (numbered && boldish > 0.5) {
                    if p.size < body - 1.5 { continue }
                    p.kind = .heading
                    headingSizes.insert(p.size)
                }
            }
        }
        // runs of 3+ "headings" are usually table cells or figure labels
        var i = 0
        while i < paras.count {
            var j = i
            while j < paras.count && paras[j].kind == .heading { j += 1 }
            if j - i >= 3 {
                for p in paras[i..<j] {
                    let t = p.text.trimmingCharacters(in: .whitespaces)
                    if !(Self.knownSections.test(t) || t.matches(#"^[A-Z]?\d+(\.\d+)*\.?\s+[A-Z][a-z]"#)) { p.kind = .text }
                }
            }
            i = max(j, i + 1)
        }
        for p in paras where p.kind == .heading {
            let t = p.text.trimmingCharacters(in: .whitespaces)
            if t.matches(#"^[\(\[\d·.,\s]+$"#) || t.hasPrefix("(") || t.hasPrefix("[") { p.kind = .text }
        }
        // front matter: page-1 material before the first real section heading (papers only: a book's
        // title page, copyright page and preface stay text)
        if kind == .paper { frontMatter(paras, body: body) }
        let ranks = headingSizes.sorted(by: >)
        let big = ranks.filter { $0 >= body + 1 }
        let titleText = paras.first { $0.kind == .title }?.text ?? ""
        for p in paras where p.kind == .heading {
            let t = p.text.trimmingCharacters(in: .whitespaces)
            if !titleText.isEmpty && PDFBook.similar(t, titleText) { p.kind = .title; continue }
            if Self.knownSections.test(t) && !t.matches(#"^\d+\.\d"#) { p.level = 1; continue }
            if let m = Resolver.firstMatch(#"^(\d+(?:\.\d+)*)\.?\s"#, in: t) {
                p.level = min(3, m.filter { $0 == "." }.count + 1)
            } else {
                p.level = min(3, (big.firstIndex(of: p.size).map { $0 + 1 }) ?? (big.count + 1))
            }
        }
        // display equations: mostly math-font lines; merge stacked pieces
        func mathFrac(_ p: Para) -> (CGFloat, Int) {
            let n = p.lines.reduce(0) { $0 + $1.text.count }
            return (p.lines.reduce(CGFloat(0)) { $0 + $1.math * CGFloat($1.text.count) } / CGFloat(max(1, n)), n)
        }
        var out: [Para] = []
        for p in paras {
            if p.kind == .text, !p.lines.isEmpty, p.boxed == nil {
                let (mf, n) = mathFrac(p)
                // (not a list item, nor sentences about formulas set in math type: words of running text
                // in most of its lines)
                let words = Self.textWords(p.text)
                let talk = Self.isBullet(p.lines[0]) || words >= 4 && CGFloat(words) > 1.5 * CGFloat(p.lines.count)
                if mf > 0.3 && n < 400 && !Self.isProse(p.text) && !talk { p.kind = .equation }
                // a book's display equations: numbered ("y = f[x, ϕ]. (2.2)"), or a short piece in math type
                if kind.isBookLike, p.kind == .text, n < 200, !Self.isProse(p.text),
                   mf > 0.1 && n < 60 && p.size >= body - 0.6 || mf > 0.05 && p.text.matches(#"\((\d{1,3}|[A-Z])\.\d{1,3}[a-z]?\)$"#) {
                    p.kind = .equation
                }
            }
            // stacked pieces (not a piece back up the page, at the top of the next column)
            if let prev = out.last, prev.page == p.page, prev.crop == nil, p.crop == nil,
               (p.kind == .equation || (p.kind == .text && p.boxed == nil)), prev.kind == .equation,
               p.bbox.y0 - prev.bbox.y1 < 1.6 * body, p.bbox.y0 > prev.bbox.y0 - body,
               p.kind == .equation || p.text.count < 30 {
                out[out.count - 1] = Para(prev.lines + p.lines, page: p.page, kind: .equation)
                continue
            }
            if let prev = out.last, prev.page == p.page, p.kind == .equation, p.crop == nil, prev.kind == .text, prev.boxed == nil, prev.text.count < 30,
               p.bbox.y0 - prev.bbox.y1 < body, !prev.text.matches("[a-z]{4}") {
                out[out.count - 1] = Para(prev.lines + p.lines, page: p.page, kind: .equation)
                continue
            }
            out.append(p)
        }
        for p in out where p.kind == .equation {
            if let crop = p.crop {
                if let (img, w) = autoreleasepool(invoking: { renderInk(p.page, crop, blank: p.blank, name: "p\(p.page + 1)-eq\(Int(p.bbox.y0))") }) {
                    p.asset = img
                    p.inkWidth = w
                    continue
                }
            }
            p.asset = autoreleasepool { render(p.page, p.bbox, photo: false, name: "p\(p.page + 1)-eq\(Int(p.bbox.y0))") }
        }
        return out
    }

    /// Papers: page-1 material before the first real section heading is front matter (title page,
    /// authors, affiliations, footnotes); the abstract is found there.
    private func frontMatter(_ paras: [Para], body: CGFloat) {
        var boundary: Int?
        for (k, p) in paras.enumerated() {
            if p.page > 1 { break }
            let t = p.text.trimmingCharacters(in: .whitespaces)
            // a summary page's "Highlights" box is still front matter (Cell: graphical abstract, highlights,
            // in brief — then the article itself with title and authors on the next page)
            if p.kind == .heading && t.matches(#"^(highlights|graphical abstract|in brief)$"#, .caseInsensitive) { continue }
            if p.kind == .heading && (Self.knownSections.test(t) || t.matches(#"^\d+\.?\s+[A-Z]"#)) { boundary = k; break }
        }
        var bnd = boundary ?? (paras.firstIndex { $0.page > 1 || ($0.page == 1 && $0.kind == .heading) } ?? paras.count)
        // a piece without section headings (the first one is "References"): the body starts at its first
        // paragraph in body type, not after a guessed abstract
        if let b = boundary, paras[b].text.trimmingCharacters(in: .whitespaces).matches(
            #"^(references|bibliography|acknowledge?ments?|funding|author contributions?|competing interests?|declaration of interests?|conflicts? of interest|data availability|code availability)\s*[:.]?$"#,
            .caseInsensitive),
           let k = paras[..<b].firstIndex(where: { $0.kind == .text && abs($0.size - body) < 0.6 && $0.text.count >= 150 }) {
            bnd = k
        }
        var abstractStarted = false
        for p in paras[0..<bnd] {
            if [.title, .figure, .table, .caption, .equation].contains(p.kind) || p.boxed != nil { continue }
            let affil = p.text.components(separatedBy: .whitespaces).filter {
                $0.matches(#"(?i)universit|institut|department|centre|center|hospital|laborator|faculty"#)
            }.count
            if abstractStarted && p.kind == .text { continue }
            if p.text.trimmingCharacters(in: .whitespaces).matches(#"^abstract\b"#, .caseInsensitive) || (p.text.count >= 500 && affil < 2) {
                p.kind = .abstract
                abstractStarted = true
            } else if p.kind == .text || p.kind == .heading {
                p.kind = .front
            }
        }
        // affiliation / correspondence footnotes on the first page belong to the front matter, and so does a
        // DOI on a line of its own
        for p in paras where p.page <= 1 && p.kind == .text && p.boxed == nil && Self.isDOILine(p.text) { p.kind = .front }
        for p in paras where p.page == 0 && p.kind == .text && p.boxed == nil {
            let affil = p.text.matches(#"(?i)universit|institut|department|hospital|laborator|faculty"#)
            if affil && (p.text.matches(#"(?i)e-?mail:|correspond"#) || p.text.matches(#"^\d\s?[A-Z]"#)) { p.kind = .front }
            // "A and B contributed equally to this work." at the foot of the first page
            if p.text.count < 300 && p.text.matches(#"(?i)contributed equally|equal(ly)? contribut|these authors contributed"#) { p.kind = .front }
        }
        // the first pages' footnotes: correspondence, e-mail, dates, editor, licence, ORCID
        for p in paras where p.page <= 1 && p.kind == .text && p.boxed == nil {
            if Self.frontFootnote(p.text, size: p.size, body: body, atFoot: p.bbox.y0 > 0.75 * boxes[p.page].height) { p.kind = .front }
        }
    }

    /// Computer Modern roman (CMR10, CMBX10) in a document whose text is set in another font (Latin
    /// Modern, Times) sets only its formulas' digits, brackets and function names ("sin", "exp"): math.
    static func mathFromComputerModern<S: Sequence>(_ lines: S, bodySize: CGFloat) where S.Element == PLine {
        var body = 0, inCM = 0, any = false
        for l in lines {
            if l.cmr > 0 { any = true }
            guard abs(l.size - bodySize) <= 0.5 else { continue }
            body += l.text.count
            inCM += Int((l.cmr * CGFloat(l.text.count)).rounded())
        }
        guard any, body > 0, Double(inCM) < 0.1 * Double(body) else { return }
        for l in lines where l.cmr > 0 { l.math = min(1, l.math + l.cmr) }
    }

    /// A "math" font that sets the running text (STIX-Regular in some Wiley and Springer journals) is
    /// the body font: when a good share of the body-size lines are prose in it, no font in the document
    /// marks a formula (and no paragraph is cropped as one).
    static func clearMathIfBodyFont<S: Sequence>(_ lines: S, bodySize: CGFloat) where S.Element == PLine {
        var body = 0, inMath = 0
        for l in lines where abs(l.size - bodySize) <= 0.5 {
            body += l.text.count
            if l.math >= 0.5 && isProse(l.text) { inMath += l.text.count }
        }
        if body > 0 && Double(inMath) > 0.3 * Double(body) {
            for l in lines { l.math = 0 }
        }
    }

    /// Running text rather than a formula: five or more words of three or more lower-case letters
    /// that make up most of it. Such a paragraph is kept as text (never cropped as an image), whatever
    /// its fonts say.
    static func isProse(_ t: String) -> Bool {
        let ns = t as NSString
        let words = proseWordRE.matches(in: t, range: NSRange(location: 0, length: ns.length))
        let wordChars = words.reduce(0) { $0 + $1.range.length }
        let chars = t.unicodeScalars.filter { !CharacterSet.whitespacesAndNewlines.contains($0) }.count
        return words.count >= 5 && Double(wordChars) >= 0.5 * Double(max(1, chars))
    }
    static let proseWordRE = re(#"\b[a-z]{3,}\b"#)

    /// Paragraph text as XHTML with italics/bold/super- and subscripts.
    func styled(_ p: Para) -> String {
        let totalChars = max(1, p.lines.reduce(0) { $0 + $1.text.count })
        let boldChars = p.lines.flatMap(\.spans).filter(\.bold).reduce(0) { $0 + $1.text.count }
        let paraBold = Double(boldChars) > 0.85 * Double(totalChars)
        var linesHTML: [(plain: String, html: String)] = []
        for l in p.lines {
            var h = ""
            for s in l.spans {
                let core = s.text.trimmingCharacters(in: .whitespaces)
                if core.isEmpty { h += esc(s.text); continue }
                let lead = String(s.text.prefix { $0 == " " }), trail = String(s.text.reversed().prefix { $0 == " " })
                var c = esc(core)
                if let id = s.noteRef {
                    c = "<a epub:type=\"noteref\" role=\"doc-noteref\" href=\"#\(id)\" id=\"fnref-\(id)\" class=\"fnref\">\(c)</a>"
                }
                else if s.sup && core.count < 25 { c = "<sup>\(c)</sup>" }
                else if s.sub && core.count < 25 { c = "<sub>\(c)</sub>" }
                else {
                    if s.bold && !paraBold { c = "<strong>\(c)</strong>" }
                    if s.italic { c = "<em>\(c)</em>" }
                }
                h += lead + c + trail
            }
            linesHTML.append((l.text.trimmingCharacters(in: .whitespaces), h.trimmingCharacters(in: .whitespaces)))
        }
        // join with the same de-hyphenation rules as joinLines
        var out = "", outPlain = ""
        for (plain, html) in linesHTML where !plain.isEmpty {
            if outPlain.isEmpty { out = html; outPlain = plain; continue }
            if outPlain.hasSuffix(Self.glue) {
                out += html; outPlain = String(outPlain.dropLast()) + plain
            } else if outPlain.hasSuffix(Self.hardHyphen) {
                out += html; outPlain = String(outPlain.dropLast()) + "-" + plain
            } else if urlContinues(outPlain, plain) {
                out += html; outPlain += plain
            } else if outPlain.range(of: "[A-Za-z]-$", options: .regularExpression) != nil, plain.first?.isLowercase == true,
               let r = out.range(of: #"-((</[a-z]+>)*)$"#, options: .regularExpression) {
                let tags = String(out[r].dropFirst())
                out.replaceSubrange(r, with: tags)
                out += html; outPlain = String(outPlain.dropLast()) + plain
            } else if (outPlain.hasSuffix("-") || outPlain.hasSuffix("/") || outPlain.hasSuffix("–")) && !outPlain.hasSuffix(" -") {
                out += html; outPlain += plain
            } else {
                out += " " + html; outPlain += " " + plain
            }
        }
        return out
    }

    // MARK: - Metadata

    func metaGuess(_ paras: [Para]) -> PaperMeta {
        var m = PaperMeta()
        let attrs = doc.documentAttributes ?? [:]
        let t = ((attrs[PDFDocumentAttribute.titleAttribute] as? String) ?? "").trimmingCharacters(in: .whitespaces)
        if t.count > 12 && !t.matches(#"(?i)microsoft word|untitled|\.docx?$|\.pdf$|^paper$"#) { m.title = t }
        if m.title.isEmpty, let tp = paras.first(where: { $0.kind == .title }) { m.title = tp.text }
        let a = ((attrs[PDFDocumentAttribute.authorAttribute] as? String) ?? "").trimmingCharacters(in: .whitespaces)
        if !a.isEmpty && !a.matches(#"(?i)administrator|user|owner|^admin"#) {
            m.authors = a.components(separatedBy: CharacterSet(charactersIn: ";,")).flatMap { $0.components(separatedBy: " and ") }
                .map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }
        }
        if let d = attrs[PDFDocumentAttribute.creationDateAttribute] as? Date {
            m.date = String(Calendar(identifier: .gregorian).component(.year, from: d))
        }
        let firstText = paras.prefix(60).filter { $0.page < 2 }.map(\.text).joined(separator: " ")
            + " " + ((attrs[PDFDocumentAttribute.subjectAttribute] as? String) ?? "")
        if let d = Resolver.doi(in: firstText) ?? Self.quickDOI(doc) { m.doi = d }
        return m
    }

    /// Quick DOI sniff from the first pages, before the full analysis.
    static func quickDOI(_ doc: PDFDocument) -> String? {
        var text = ((doc.documentAttributes?[PDFDocumentAttribute.subjectAttribute] as? String) ?? "") + " "
        for i in Array(0..<min(2, doc.pageCount)) + [doc.pageCount - 1] { text += (doc.page(at: i)?.string ?? "") + " " }
        return Resolver.doi(in: text)
    }
}

// MARK: - Book assembly

enum PDFBook {
    /// A PDF's reference list: its entries keep the numbers they are printed with.
    static let refsOL = "<ol class=\"refs\" style=\"list-style-type: none; padding-left: 0\">"

    static func build(_ data: Data, meta extra: PaperMeta, log: @escaping (String) -> Void) async throws -> Book {
        guard let doc = PDFDocument(data: data) else { throw fail("Could not open the PDF.") }
        if doc.isLocked && !doc.unlock(withPassword: "") { throw fail("The PDF is password-protected.") }
        guard doc.pageCount > 0 else { throw fail("The PDF has no pages.") }
        log("Analysing \(doc.pageCount) pages")
        let an = PDFAnalyzer(doc: doc, log: log)
        an.workType = extra.workType.isEmpty ? Paper2Epub.knownType : extra.workType
        let paras = an.analyze()
        let isBook = an.kind.isBookLike
        var meta = an.metaGuess(paras)
        if isBook && meta.authors.isEmpty { meta.authors = titlePageAuthors(paras) }
        let guessed = meta
        if !extra.title.isEmpty || !extra.authors.isEmpty { meta = extra; meta.merge(guessed) }
        if !meta.doi.isEmpty, let cr = await crossref(meta.doi) {
            log("Found metadata for doi:\(meta.doi)")
            var m = cr; m.merge(meta); meta = m
        }

        if ProcessInfo.processInfo.environment["P2E_DEBUG"] != nil {
            for p in paras {
                let b = p.bbox
                print(String(format: "p%-2d %-9@ L%d sz%4.1f bold%.2f (%3.0f,%3.0f)-(%3.0f,%3.0f) ", p.page + 1, "\(p.kind)" as NSString,
                             p.level, p.size, p.lines.first?.bold ?? 0, b.x0, b.y0, b.x1, b.y1) + String(p.text.prefix(70)))
            }
        }
        // merge paragraphs split across columns/pages
        var merged: [Para] = []
        for p in paras {
            // (past figures, legends, front matter, notes and boxes set between the columns)
            if p.kind == .text, p.text.first?.isLowercase == true,
               // (and a piece of a line cropped as a formula, a lone "∗")
               let j = merged.lastIndex(where: { ![.figure, .table, .caption, .front, .footnote, .sidenote, .code].contains($0.kind) && $0.boxed == p.boxed
                   && !($0.kind == .equation && $0.eqLabel == nil && $0.text.trimmingCharacters(in: .whitespaces).count <= 3) }),
               merged[j].kind == .text, merged[(j + 1)...].filter({ $0.boxed == p.boxed && $0.kind != .footnote && $0.kind != .sidenote }).count <= 8 {
                let q = merged[j]
                if !(q.text.hasSuffix(".") || q.text.hasSuffix("?") || q.text.hasSuffix("!") || q.text.hasSuffix(":")) {
                    q.lines += p.lines
                    continue
                }
            }
            merged.append(p)
        }
        // figure legends continue after the caption title in smaller type: fold them into the caption
        // (in a book only what follows right below it: its exercises and notes are set in that type too)
        func adjacent(_ p: Para, _ c: Para) -> Bool {
            guard p.page == c.page, let last = c.lines.last, let first = p.lines.first else { return false }
            return first.bbox.y0 >= last.bbox.y0 && first.bbox.y0 - last.bbox.y1 < 1.2 * max(p.size, 1)
                && first.bbox.x0 < last.bbox.x1 && first.bbox.x1 > last.bbox.x0
        }
        var folded: [Para] = []
        for p in merged {
            let t = p.text.trimmingCharacters(in: .whitespaces)
            let legendLike = t.matches(#"^\([A-Z][A-Za-z]{0,3}\)\s|^See also Figure|^Scale bars?"#)
            if p.kind == .text, p.boxed == nil, p.size < an.bodySize - 0.4,
               // a legend's "(D) …" panels continue after the page's body text when it runs onto the next page
               let j = folded.lastIndex(where: { $0.kind == .caption }), folded.count - j <= 8 || legendLike,
               abs(folded[j].page - p.page) <= 1, abs(p.size - folded[j].size) < 0.8,
               legendLike || j == folded.count - 1, !isBook || adjacent(p, folded[j]) {
                folded[j] = Para(folded[j].lines + p.lines, page: folded[j].page, kind: .caption)
                continue
            }
            folded.append(p)
        }
        merged = folded
        // the reference list: one entry per reference
        let (withRefs, refEntry) = PDFReferences.mark(merged, body: an.bodySize)
        merged = withRefs
        // attach captions to the nearest figure/table on the same page
        var capOf: [Int: Int] = [:]
        for (i, p) in merged.enumerated() where p.kind == .figure || p.kind == .table || p.kind == .code {
            var best: Int?, bd = CGFloat.greatestFiniteMagnitude
            for j in max(0, i - 4)..<min(merged.count, i + 5) {
                let c = merged[j]
                guard c.kind == .caption, c.page == p.page, !capOf.values.contains(j) else { continue }
                // (an algorithm's caption heads its box)
                if c.text.hasPrefix("Algorithm"), c.bbox.y0 > p.bbox.y0 { continue }
                let isTab = c.text.matches(#"^(supplementary\s+)?table"#, .caseInsensitive)
                if (p.kind == .table) != isTab { continue }
                let d = captionGap(c.bbox, p.bbox)
                // (beside it only if no other figure on the page is nearer: not the next column's caption)
                if d < min(abs(c.bbox.y0 - p.bbox.y1), abs(p.bbox.y0 - c.bbox.y1)),
                   merged.contains(where: { $0 !== p && $0.kind == p.kind && $0.page == p.page && captionGap(c.bbox, $0.bbox) < d }) { continue }
                if d < bd && d < 120 { best = j; bd = d }
            }
            if let best { capOf[i] = best }
        }
        for (i, p) in merged.enumerated() where (p.kind == .figure || p.kind == .table) && capOf[i] == nil
            && (p.region.map { $0.area > 40000 } ?? false) {
            var best: Int?
            var bd = Int.max
            for j in max(0, i - 12)..<min(merged.count, i + 13) {
                let c = merged[j]
                guard c.kind == .caption, abs(c.page - p.page) == 1, !capOf.values.contains(j) else { continue }
                let isTab = c.text.matches(#"^(supplementary\s+)?table"#, .caseInsensitive)
                if (p.kind == .table) != isTab { continue }
                if abs(j - i) < bd { best = j; bd = abs(j - i) }
            }
            if let best { capOf[i] = best }
        }
        let usedCaps = Set(capOf.values)
        // a book: chapters from its outline
        let plan = isBook && !an.outline.isEmpty ? plan(merged, outline: an.outline) : nil
        let outlineSections = plan?.entries.contains { $0.level >= 2 } ?? false
        var nextEntry = 0
        var indexOpened = false
        var chapterLabel: String?

        var chapters: [Chapter] = []
        var cur: (title: String, id: String, html: String, headings: [Heading])?
        var images: [BookImage] = []
        var front = ""
        var autoID = 0
        func nextID() -> String { autoID += 1; return "s\(autoID)" }
        func close() {
            if let c = cur { chapters.append(Chapter(title: c.title, html: "<section>\(PDFAnalyzer.bulletLists(c.html))</section>", headings: c.headings)) }
            cur = nil
        }
        func open(_ title: String) {
            close()
            let id = nextID()
            cur = (title, id, "<h1 id=\"\(id)\">\(esc(title))</h1>", [Heading(level: 1, id: id, title: title)])
        }
        func append(_ html: String) {
            if cur == nil { open(isBook ? "Front matter" : "Main text") }
            cur!.html += html
        }
        func addImage(_ img: BookImage?) -> String? {
            guard let img else { return nil }
            images.append(img)
            return "../images/\(img.fileName)"
        }

        let hasAbstractHeading = merged.prefix(30).contains { $0.kind == .heading && $0.text.lowercased().hasPrefix("abstract") }
        let hasIntro = merged.contains { $0.kind == .heading && $0.text.matches("(?i)introduction|background") }
        var abstractDone = false, introDone = false
        var lastKind: Para.Kind?
        let missingTitle = meta.title.isEmpty
        var openBox: Int?
        var inList = false
        // footnotes: each goes to the end of the chapter its marker is in (else the one it was found in)
        var footnotes: [(id: String, label: String, html: String, home: Int)] = []
        var eqIDs = Set<String>()
        for (i, p) in merged.enumerated() {
            if let plan {
                // the outline's entries that start here
                while nextEntry < plan.entries.count && !before(p, plan.entries[nextEntry]) {
                    if inList { append("</ol>"); inList = false }
                    if openBox != nil { append("</div>"); openBox = nil }
                    let e = plan.entries[nextEntry], title = plan.titles[nextEntry]
                    if e.level <= 1 { open(title) } else {
                        let id = nextID()
                        let h = min(e.level, 3)
                        append("<h\(h) id=\"\(id)\">\(esc(title))</h\(h)>")
                        if h == 2 { cur!.headings.append(Heading(level: 2, id: id, title: title)) }
                    }
                    nextEntry += 1
                }
                if plan.consumed.contains(i) { continue }
            }
            // a book's index: one entry (line) per paragraph, in one chapter
            if isBook, an.onIndexPage(p.page), [.text, .heading, .caption, .front, .title].contains(p.kind) {
                if inList { append("</ol>"); inList = false }
                if plan == nil && !indexOpened { open("Index"); indexOpened = true }
                if PDFAnalyzer.isIndexTitle(p.text) { continue }
                for l in p.lines {
                    let t = l.text.replacingOccurrences(of: PDFAnalyzer.glue, with: "").replacingOccurrences(of: PDFAnalyzer.hardHyphen, with: "-")
                        .trimmingCharacters(in: .whitespaces)
                    if !t.isEmpty { append("<p class=\"index\">\(esc(t))</p>") }
                }
                continue
            }
            if p.kind == .footnote, let id = p.noteID {
                footnotes.append((id, p.noteLabel ?? "", an.styled(p), chapters.count))
                continue
            }
            // reference entries: one list item each (numbered as printed)
            if inList && !refEntry[i] { append("</ol>"); inList = false }
            if p.kind == .sidenote {
                append("<aside class=\"sidenote\" role=\"note\"><p>\(an.styled(p))</p></aside>")
                continue
            }
            if refEntry[i] {
                if !inList { append(refsOL); inList = true }
                append("<li>\(an.styled(p))</li>")
                continue
            }
            // a box's paragraphs, kept together
            if p.boxed != openBox {
                if openBox != nil { append("</div>") }
                if p.boxed != nil { append("<div class=\"box\">") }
                openBox = p.boxed
            }
            switch p.kind {
            case .code:
                // a listing: its lines as they are set, under its caption ("Figure 7.8 Sample code…")
                let code = "<pre><code>\(esc(PDFAnalyzer.codeText(p)))</code></pre>"
                if let c = capOf[i] { append("<figure class=\"code\">\(code)<figcaption>\(an.styled(merged[c]))</figcaption></figure>") } else { append(code) }
            case .figure, .table:
                let cap = capOf[i].map { an.styled(merged[$0]) } ?? ""
                guard let src = addImage(p.asset) else { break }
                if p.kind == .table {
                    append("<div class=\"table-wrap\">\(cap.isEmpty ? "" : "<p class=\"caption\">\(cap)</p>")<img src=\"\(src)\" alt=\"Table\"/></div>")
                } else {
                    append("<figure><img src=\"\(src)\" alt=\"\(esc(p.alt ?? "Figure"))\"/>\(cap.isEmpty ? "" : "<figcaption>\(cap)</figcaption>")</figure>")
                }
            case _ where usedCaps.contains(i):
                break
            case .title:
                if missingTitle { meta.title = p.text }
            case .heading where isBook:
                var t = p.text.trimmingCharacters(in: .whitespaces)
                if plan != nil {
                    // the outline gives the chapters: a heading it doesn't list is a sub-heading
                    let section = !outlineSections && t.matches(#"^(\d{1,3}|[A-Z])\.\d{1,3}\.?\s+\S"#)
                    let id = nextID(), h = section ? 2 : 3
                    append("<h\(h) id=\"\(id)\">\(esc(t))</h\(h)>")
                    if section { cur!.headings.append(Heading(level: 2, id: id, title: t)) }
                    break
                }
                // no outline: "Chapter 3" over the chapter's title makes one title, "3 Shallow neural networks"
                if let m = PDFAnalyzer.chapterLabelRE.firstMatch(in: t, range: NSRange(t.startIndex..., in: t)),
                   let r = Range(m.range(at: 1), in: t), i + 1 < merged.count, merged[i + 1].kind == .heading {
                    chapterLabel = String(t[r])
                    break
                }
                if let l = chapterLabel { t = "\(l) \(t)"; chapterLabel = nil; open(t); break }
                if p.level <= 1 { open(t) } else {
                    let id = nextID()
                    let h = min(p.level, 3)
                    append("<h\(h) id=\"\(id)\">\(esc(t))</h\(h)>")
                    if h == 2 { cur!.headings.append(Heading(level: 2, id: id, title: t)) }
                }
            case .heading:
                let t = p.text.trimmingCharacters(in: .whitespaces)
                if Self.knownSections(t) == "abstract" { abstractDone = true }
                if p.level <= 1 { open(t) } else {
                    let id = nextID()
                    let h = min(p.level, 3)
                    append("<h\(h) id=\"\(id)\">\(esc(t))</h\(h)>")
                    if h == 2 { cur!.headings.append(Heading(level: 2, id: id, title: t)) }
                }
            case .front:
                if !meta.title.isEmpty && similar(p.text, meta.title) { break }
                front += "<p>\(an.styled(p))</p>"
            case .abstract:
                if !abstractDone { open("Abstract"); abstractDone = true }
                let s = an.styled(p).replacingOccurrences(of: #"^(<strong>)?Abstract(</strong>)?[.:]?(</strong>)?\s*"#, with: "",
                                                          options: [.regularExpression, .caseInsensitive])
                append("<p>\(s)</p>")
            case .equation:
                if let src = addImage(p.asset) {
                    let wem = (p.inkWidth.map { $0 + 4 } ?? p.bbox.w + 8) / max(an.bodySize, 6) * 0.95
                    // its text as the alternative (search, VoiceOver); its number as text, for references
                    let alt = PDFAnalyzer.equationText(p)
                    let img = "<img class=\"equation\" src=\"\(src)\" alt=\"\(esc(alt.isEmpty ? "equation" : alt))\" style=\"width: \(String(format: "%.1f", wem))em\"/>"
                    if let label = p.eqLabel {
                        let key = label.replacingOccurrences(of: ".", with: "-")
                        var eid = "eq-\(key)", k = 1
                        while eqIDs.contains(eid) { k += 1; eid = "eq-\(key)-\(k)" }
                        eqIDs.insert(eid)
                        append("<div class=\"equation\" id=\"\(eid)\"><span class=\"eqlabel eqno\">(\(esc(label)))</span>\(img)</div>")
                    } else {
                        append("<div class=\"equation\">\(img)</div>")
                    }
                }
            case .caption:
                append("<p class=\"caption\">\(an.styled(p))</p>")
            default:
                if abstractDone && !introDone && lastKind == .abstract && !hasIntro && p.boxed == nil { open("Introduction") }
                introDone = introDone || lastKind == .heading
                append("<p>\(an.styled(p))</p>")
            }
            if p.kind != .front && p.kind != .footnote && p.kind != .sidenote { lastKind = p.kind }
        }
        if inList { cur?.html += "</ol>" }
        if openBox != nil { cur?.html += "</div>" }
        close()
        placeFootnotes(footnotes, in: &chapters)
        _ = hasAbstractHeading
        if isBook {
            // outline entries with nothing under them (a "Contents" entry, its pages left out)
            chapters.removeAll { ch in
                ch.html.range(of: #"<(p|img|li|figure|table|div)\b"#, options: .regularExpression) == nil
            }
        }
        if !front.isEmpty && meta.affiliationsHTML.isEmpty { meta.affiliationsHTML = PDFAnalyzer.bulletLists(front) }
        guard !chapters.isEmpty else { throw fail("Could not find any text in the PDF (is it a scanned image?).") }
        var notes = ["Converted from the PDF without AI: equations and tables are kept as images."]
        if isBook { notes.append("Converted as a \(an.kind.rawValue)\(plan != nil ? ", chapters from the PDF's outline" : ""): \(an.kindReasons.joined(separator: "; ")).") }
        return Book(meta: meta, chapters: chapters, images: images, sourceLabel: "PDF (layout analysis)",
                    notes: notes, kind: an.kind, kindReasons: an.kindReasons)
    }

    /// The notes at the end of the chapters their markers are in, each as an `<aside>` whose label leads
    /// back to the marker; a note whose marker wasn't found goes to the chapter it was found in.
    static func placeFootnotes(_ notes: [(id: String, label: String, html: String, home: Int)], in chapters: inout [Chapter]) {
        guard !notes.isEmpty, !chapters.isEmpty else { return }
        var byChapter: [Int: String] = [:]
        for n in notes {
            let k = chapters.firstIndex { $0.html.contains("href=\"#\(n.id)\"") } ?? min(n.home, chapters.count - 1)
            let label = chapters[k].html.contains("id=\"fnref-\(n.id)\"")
                ? "<a href=\"#fnref-\(n.id)\">\(esc(n.label))</a>" : "<span class=\"fn-label\">\(esc(n.label))</span>"
            byChapter[k, default: ""] += "<aside epub:type=\"footnote\" role=\"doc-footnote\" id=\"\(n.id)\" class=\"footnote\"><p>\(label) \(n.html)</p></aside>"
        }
        for (k, html) in byChapter {
            var h = chapters[k].html
            if h.hasSuffix("</section>") { h.removeLast("</section>".count); h += "<div class=\"footnotes\">\(html)</div></section>" }
            else { h += "<div class=\"footnotes\">\(html)</div>" }
            chapters[k].html = h
        }
    }

    /// The figures in the PDF by their label ("fig 3", "extended data fig 2", "supplementary fig 1"):
    /// to fill in figures another source (an XML version without its images) doesn't have.
    ///
    /// Matched by page, not through caption detection: a figure is the page with a line that starts
    /// its caption ("Fig. 3 | …", "Extended Data Fig. 2 | See next page for caption"), and the
    /// image is all figure areas on that page together (figures are often found in pieces). With
    /// several captions on a page, each area goes to the nearest one.
    static func figuresByLabel(_ data: Data, log: @escaping (String) -> Void) -> [String: BookImage] {
        guard let doc = PDFDocument(data: data), !doc.isLocked || doc.unlock(withPassword: ""), doc.pageCount > 0 else { return [:] }
        let an = PDFAnalyzer(doc: doc, log: log)
        let paras = an.analyze()
        var areas: [Int: [R]] = [:]
        for p in paras where p.kind == .figure { areas[p.page, default: []].append(p.bbox) }
        // caption starts: the label, then a separator (not "Fig. 5a)" in the text)
        let head = try! NSRegularExpression(pattern: #"^(?:Extended\s+Data\s+|Supplementary\s+|Supporting\s+)?(?:Figure|Fig\.?)\s*"# + PDFAnalyzer.labelNumber + #"\s*(?:[|.:—–]|\s-\s)"#,
                                            options: [.caseInsensitive])
        var heads: [Int: [(key: String, y: CGFloat)]] = [:]
        var pageOf: [String: Int] = [:]
        for (pno, lines) in an.pagesLines.enumerated() where areas[pno] != nil {
            for l in lines {
                let t = l.text.trimmingCharacters(in: .whitespaces)
                guard head.firstMatch(in: t, range: NSRange(t.startIndex..., in: t)) != nil, let key = figureKey(t),
                      pageOf[key] == nil else { continue }
                pageOf[key] = pno
                heads[pno, default: []].append((key, (l.bbox.y0 + l.bbox.y1) / 2))
            }
        }
        var out: [String: BookImage] = [:]
        for (pno, hs) in heads {
            var rects: [String: R] = [:]
            for a in areas[pno] ?? [] {
                let cy = (a.y0 + a.y1) / 2
                guard let near = hs.min(by: { abs($0.y - cy) < abs($1.y - cy) }) else { continue }
                rects[near.key] = rects[near.key].map { $0.union(a) } ?? a
            }
            for (key, r) in rects {
                let photo = an.graphics[pno].images.contains { r.contains($0, tol: 5) && $0.area > 0.3 * r.area }
                out[key] = an.render(pno, r, photo: photo, name: "p\(pno + 1)-\(slug(key))")
            }
        }
        return out
    }

    /// How far a caption is from a figure: the vertical gap for one above or below it; for one beside it (a
    /// book's margin caption, level with its figure) the horizontal gap, so that a paragraph below the
    /// figure isn't taken for its caption.
    static func captionGap(_ c: R, _ f: R) -> CGFloat {
        let vertical = min(abs(c.y0 - f.y1), abs(f.y0 - c.y1))
        let overlap = min(c.y1, f.y1) - max(c.y0, f.y0)
        guard overlap > 0.5 * c.h, c.x0 >= f.x1 - 2 || c.x1 <= f.x0 + 2 else { return vertical }
        return min(vertical, max(0, c.x0 - f.x1, f.x0 - c.x1))
    }

    /// "Fig. 3 | Title", "Figure 3.", "Extended Data Fig. 3", "Supplementary Figure S1" → a key that
    /// is the same for the XML's label and the PDF's caption. Nil for anything that isn't a figure.
    static func figureKey(_ text: String) -> String? {
        let t = text.trimmingCharacters(in: .whitespacesAndNewlines)
        let re = try! NSRegularExpression(pattern: #"^(Extended\s+Data\s+|Supplementary\s+|Supporting\s+|Appendix\s+)?(?:Figure|Fig\.?)\s*(S?)(?>(\d+(?:[.\-–]\d+)*|[A-Z](?:[.\-–]\d+)+))"#,
                                          options: [.caseInsensitive])
        guard let m = re.firstMatch(in: t, range: NSRange(t.startIndex..., in: t)) else { return nil }
        func g(_ i: Int) -> String { Range(m.range(at: i), in: t).map { String(t[$0]) } ?? "" }
        var kind = g(1).lowercased().replacingOccurrences(of: "\\s+", with: " ", options: .regularExpression).trimmingCharacters(in: .whitespaces)
        if kind == "supporting" || kind == "appendix" || (!g(2).isEmpty && kind.isEmpty) { kind = "supplementary" }
        // ("Figure 3-4" and "Figure 3.4" are one figure)
        let num = g(3).uppercased().replacingOccurrences(of: "[-–]", with: ".", options: .regularExpression)
        return (kind.isEmpty ? "" : kind + " ") + "fig " + num
    }

    static func knownSections(_ t: String) -> String? {
        t.lowercased().hasPrefix("abstract") ? "abstract" : nil
    }

    static func similar(_ a: String, _ b: String) -> Bool {
        let na = a.lowercased().replacingOccurrences(of: #"\W+"#, with: "", options: .regularExpression)
        let nb = b.lowercased().replacingOccurrences(of: #"\W+"#, with: "", options: .regularExpression)
        return !na.isEmpty && !nb.isEmpty && (na == nb || (na.count > 20 && (na.contains(nb) || nb.contains(na))))
    }

    /// Title, authors, journal and date from Crossref.
    static func crossref(_ doi: String) async -> PaperMeta? {
        guard let enc = doi.addingPercentEncoding(withAllowedCharacters: .urlPathAllowed),
              let url = URL(string: "https://api.crossref.org/works/\(enc)"),
              let j = try? await Net.json(url), let m = j["message"] as? [String: Any] else { return nil }
        var meta = PaperMeta()
        meta.doi = doi
        meta.title = ((m["title"] as? [String])?.first ?? "").replacingOccurrences(of: "<[^>]+>", with: "", options: .regularExpression)
        meta.authors = ((m["author"] as? [[String: Any]]) ?? []).compactMap { a in
            let s = [a["given"] as? String, a["family"] as? String].compactMap { $0 }.joined(separator: " ")
            return s.isEmpty ? a["name"] as? String : s
        }
        meta.journal = (m["container-title"] as? [String])?.first ?? ""
        meta.workType = m["type"] as? String ?? ""
        if let parts = ((m["issued"] as? [String: Any])?["date-parts"] as? [[Int]])?.first, let y = parts.first {
            meta.date = parts.count >= 3 ? String(format: "%04d-%02d-%02d", y, parts[1], parts[2]) : String(y)
        }
        if let lic = (m["license"] as? [[String: Any]])?.compactMap({ $0["URL"] as? String })
            .first(where: { $0.contains("creativecommons") }) { meta.license = lic }
        return meta
    }
}
