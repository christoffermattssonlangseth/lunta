import Foundation
import CoreGraphics

/// The reference list of a PDF: found from its heading, or, without one, from the entries
/// themselves, and cut into one entry per reference. The Rust converter does the same in
/// `core/src/convert/pdf/refs.rs`; the two must agree.
///
/// A heading the layout analysis missed is found when it is followed by entries: "References" in
/// the reference type (smaller than the body, so not taken for a heading), on the first line of
/// the list's first paragraph ("References 1. Hanahan D. …"), set off by an ornament ("❚
/// REFERENCES"), numbered ("6 References") or in another language ("Literatur", "Références").
/// Without any reference heading in the paper, a run of at least six paragraphs that each start
/// like a reference ("1.", "[1]", "Smith JA,", "Smith, J. A.,", "J. A. Smith,") and look like one
/// (a year, volume and pages, a DOI or web address, "et al.", initials), numbered in order or
/// sorted by author, after the first third of the text and with almost no running text in body
/// type after it, is a reference list.
///
/// Entries: in a numbered list each paragraph that starts with a number is an entry, a line that
/// starts with the next number (or a number at the hanging indent) starts the next one, and
/// anything else continues the current one (an entry broken across columns or pages); in an
/// author–year list each paragraph is an entry unless it plainly continues the one before, and a
/// paragraph holding several entries is cut where a line starts at the hanging indent or after a
/// short line that ends an entry.
enum PDFReferences {
    private static func rx(_ p: String) -> NSRegularExpression { try! NSRegularExpression(pattern: p) }
    private static func has(_ r: NSRegularExpression, _ s: String) -> Bool { r.test(s) }
    private static func n(_ r: NSRegularExpression, _ s: String) -> Int {
        r.numberOfMatches(in: s, range: NSRange(location: 0, length: (s as NSString).length))
    }

    static let headingRE = rx(
        #"(?i)^(references?(\s+and\s+notes)?|references\s+cited|cited\s+references|reference\s+list|list\s+of\s+references|"#
        + #"bibliography|literature\s+cited|cited\s+literature|works\s+cited|referenzen|literatur|literaturverzeichnis|"#
        + #"références(\s+bibliographiques)?|referencias(\s+bibliográficas)?|bibliografía|bibliografia|"#
        + #"referências(\s+bibliográficas)?|riferimenti(\s+bibliografici)?|literatuur|referenser|litteratur|referanser)\s*[:.]?$"#)
    private static let ornamentRE = rx(#"^[^\p{L}\p{N}]+"#)
    private static let numberRE = rx(#"^(\d+(\.\d+)*\.?|[IVX]+\.)\s+"#)
    private static let spacedRE = rx(#"^(\p{L} )+\p{L}$"#)
    static let labelRE = rx(#"^\s*(?:\[(\d{1,4})\]|\((\d{1,4})\)\s|(\d{1,4})[.)](?:\s|$|\p{Lu}\p{Ll})|(\d{1,3})\s+\p{Lu}[\p{L}.])"#)
    static let authorRE = rx(
        #"^\s*(?:(?:van|von|de|der|den|di|da|du|del|la|le|dos|das|ten|ter|el|al)\s+)*\p{Lu}[\p{L}'’\-]+(?:[\s\-]\p{Lu}[\p{L}'’\-]+)?"#
        + #"(?:,\s*(?:\p{Lu}\.\s?-?){1,3}|,?\s+\p{Lu}{1,3}(?:[,.:]|\s+(?:and|&)\s)|,?\s+(?:\p{Lu}\.\s?-?){1,3}[,\s])"#
        + #"|^\s*(?:\p{Lu}\.\s?-?){1,3}\s*\p{Lu}[\p{L}'’\-]+(?:\s\p{Lu}[\p{L}'’\-]+)?,"#)
    private static let yearRE = rx(#"\b(1[89]|20)\d{2}[a-z]?\b"#)
    private static let yearPlaceRE = rx(#"\((1[89]|20)\d{2}[a-z]?\)|\b(1[89]|20)\d{2}[a-z]?\s*[;:]\s*\d|\b(1[89]|20)\d{2}[a-z]?\s*,\s*\d{1,4}\s*[,:(]"#)
    private static let volPagesRE = rx(#"\b\d{1,4}\s*(\([\dS\s–\-]+\))?\s*[:,]\s*[A-Za-z]?\d{1,6}\s*[–\-]\s*[A-Za-z]?\d{1,6}\b|\b\d{1,4}\s*\(\d{1,4}\)\s*:|\b(1[89]|20)\d{2},\s*\d{1,4},\s*[A-Za-z]?\d{1,6}\b"#)
    private static let linkRE = rx(#"(?i)\bdoi\b|https?://|www\.|\bpmid\b|\b10\.\d{4,9}/"#)
    private static let etAlRE = rx(#"\bet\s+al\b"#)
    private static let initialsRE = rx(#"\b\p{Lu}\.\s?(\p{Lu}\.\s?)*"#)
    private static let initials2RE = rx(#"\b\p{Lu}[\p{Ll}'’\-]+\s\p{Lu}{1,3}[,.]"#)
    private static let wordRE = rx(#"\p{L}+"#)
    private static let commonRE = rx(#"(?i)\b(we|our|was|were|is|are|this|these|that|which|have|has|had|been|showed|shown|found|suggest|suggests|however|thus|therefore|could|would|may|might|should|also|than|there)\b"#)
    private static let continuesRE = rx(#"^\s*([\p{Ll}\d(,;:.–\-&]|and\s)"#)
    private static let endsRE = rx(#"[.)\]>]\s*$"#)
    private static let lineEndsRE = rx(#"[.)]$"#)

    /// A reference-list heading: "References", "REFERENCES", "6 References", "❚ REFERENCES",
    /// "R E F E R E N C E S", "Literature Cited", "References and Notes", "Literatur", "Références".
    static func isHeading(_ t: String) -> Bool { has(headingRE, headingText(t)) }

    /// A heading's words without an ornament or a number in front and without letter spacing.
    static func headingText(_ t: String) -> String {
        func strip(_ r: NSRegularExpression, _ s: String) -> String {
            r.stringByReplacingMatches(in: s, range: NSRange(location: 0, length: (s as NSString).length), withTemplate: "")
        }
        let s = strip(numberRE, strip(ornamentRE, t.trimmingCharacters(in: .whitespacesAndNewlines)))
            .trimmingCharacters(in: .whitespacesAndNewlines)
        return has(spacedRE, s) ? s.replacingOccurrences(of: " ", with: "") : s
    }

    /// The number an entry starts with: "12.", "12.Smith", "[12]", "(12)", "12)", "12 Smith" or "12 J. Smith".
    static func label(_ t: String) -> Int? {
        let ns = t as NSString
        guard let m = labelRE.firstMatch(in: t, range: NSRange(location: 0, length: ns.length)) else { return nil }
        for i in 1...4 where m.range(at: i).location != NSNotFound { return Int(ns.substring(with: m.range(at: i))) }
        return nil
    }

    /// Starts with an author: "Smith JA,", "Smith, J. A.,", "Smith J.A.", "van der Berg, A.", "J. A. Smith,".
    static func authorStart(_ t: String) -> Bool { has(authorRE, t) }

    /// Starts like a reference: a number or an author.
    static func entryStart(_ t: String) -> Bool { label(t) != nil || authorStart(t) }

    /// How much a piece of text looks like a reference: a year, a year in brackets or before volume
    /// and pages, volume and pages, a DOI or web address, "et al.", several initials; less for
    /// running text.
    static func score(_ t: String) -> Int {
        var s = 0
        if has(yearRE, t) { s += 1 }
        if has(yearPlaceRE, t) { s += 1 }
        if has(volPagesRE, t) { s += 1 }
        if has(linkRE, t) { s += 1 }
        if has(etAlRE, t) { s += 1 }
        if n(initialsRE, t) + n(initials2RE, t) >= 2 { s += 1 }
        if prose(t) { s -= 2 }
        return s
    }

    /// Running text: many words of a sentence ("we", "was", "were", "these", "however") or very long.
    static func prose(_ t: String) -> Bool {
        let words = n(wordRE, t), common = n(commonRE, t)
        return t.count > 1500 || (words >= 20 && Double(common) > 0.08 * Double(words))
    }

    private static func strong(_ t: String) -> Bool { entryStart(t) && score(t) >= 3 }

    /// Continues the entry before it: starts in lower case, with a digit or punctuation.
    private static func continues(_ t: String) -> Bool { has(continuesRE, t) && label(t) == nil }

    private static func textPara(_ p: Para) -> Bool { p.kind == .text && p.boxed == nil && !p.lines.isEmpty }
    private static func neutral(_ p: Para) -> Bool { [.figure, .table, .caption, .equation].contains(p.kind) }

    private static func headingPara(_ lines: [PLine], page: Int) -> Para {
        let ls = lines.map { l -> PLine in
            let t = headingText(l.text)
            return PLine(text: t, bbox: l.bbox, size: l.size, bold: 1, math: 0, spans: [Span(text: t, bold: true)], page: l.page)
        }
        let h = Para(ls, page: page, kind: .heading)
        h.level = 1
        return h
    }

    /// The analysed paragraphs with the reference lists marked: their headings are level-1
    /// headings, and each entry is one paragraph, flagged in the second array.
    static func mark(_ input: [Para], body: CGFloat) -> ([Para], [Bool]) {
        // 1. headings: known ones, and "References" in small type or on the first line of the list
        var out: [Para] = []
        var anyHeading = false
        for (i, p) in input.enumerated() {
            let text = p.text
            func nextEntry() -> Bool {
                guard let q = input[(i + 1)...].first(where: { !neutral($0) }) else { return false }
                return q.kind == .text && q.boxed == nil && entryStart(q.text)
            }
            if p.kind == .heading && isHeading(text) {
                out.append(headingPara(p.lines, page: p.page)); anyHeading = true; continue
            }
            if (p.kind == .text || p.kind == .heading) && p.boxed == nil && p.lines.count <= 2 && isHeading(text) && nextEntry() {
                out.append(headingPara(p.lines, page: p.page)); anyHeading = true; continue
            }
            if p.kind == .text && p.boxed == nil && p.lines.count >= 2 && isHeading(p.lines[0].text) && entryStart(p.lines[1].text) {
                out.append(headingPara([p.lines[0]], page: p.page))
                let rest = Para(Array(p.lines.dropFirst()), page: p.lines[1].page, kind: .text)
                out.append(rest)
                anyHeading = true
                continue
            }
            out.append(p)
        }
        var paras = out
        // 2. a list without a heading
        if !anyHeading { paras = findRuns(paras, body: body) }
        // 3. the entries of each list
        var result: [Para] = [], flags: [Bool] = []
        var i = 0
        while i < paras.count {
            let p = paras[i]
            result.append(p); flags.append(false)
            i += 1
            guard p.kind == .heading && p.level <= 1 && isHeading(p.text) else { continue }
            var section: [Para] = []
            while i < paras.count {
                let q = paras[i]
                let t = q.text
                let entryLike = entryStart(t) && score(t) >= 2
                if q.kind == .heading && q.level <= 1 && !entryLike { break }
                if q.kind == .heading && q.boxed == nil && entryLike { q.kind = .text; q.level = 0 }
                section.append(q)
                i += 1
            }
            for (q, f) in entries(section) { result.append(q); flags.append(f) }
        }
        return (result, flags)
    }

    /// Inserts a "References" heading before each run of paragraphs that is a reference list.
    private static func findRuns(_ paras: [Para], body: CGFloat) -> [Para] {
        let total = paras.filter(textPara).reduce(0) { $0 + $1.text.count }
        var runs: [(start: Int, last: Int)] = []
        var before = 0
        var i = 0
        while i < paras.count {
            let p = paras[i]
            guard textPara(p) && strong(p.text) else {
                if textPara(p) { before += p.text.count }
                i += 1
                continue
            }
            // the run: entries, pieces that continue one, figures and captions in between
            var j = i, nStrong = 0, nCont = 0, authors = 0, last = i
            var labels: [Int] = [], firsts: [Character] = []
            while j < paras.count {
                let q = paras[j]
                if neutral(q) { j += 1; continue }
                if !textPara(q) { break }
                let t = q.text
                if strong(t) {
                    nStrong += 1
                    if let l = label(t) { labels.append(l) }
                    else if authorStart(t) {
                        authors += 1
                        if let c = t.trimmingCharacters(in: .whitespaces).lowercased().first { firsts.append(c) }
                    }
                } else if (entryStart(t) && score(t) >= 2) || (!entryStart(t) && (continues(t) || score(t) >= 2) && !prose(t)) {
                    nCont += 1
                } else {
                    break
                }
                last = j
                j += 1
            }
            func inOrder(_ v: [Int]) -> Bool {
                v.count >= 6 && Double(zip(v, v.dropFirst()).filter { $1 > $0 && $1 <= $0 + 5 }.count) >= 0.8 * Double(v.count - 1)
            }
            func sorted(_ v: [Character]) -> Bool {
                Double(zip(v, v.dropFirst()).filter { $1 >= $0 }.count) >= 0.7 * Double(max(v.count, 2) - 1)
            }
            let okList = labels.count >= nStrong / 2 ? inOrder(labels) : (authors >= 8 && sorted(firsts))
            // running text after the list, up to the next section (notes in small type do not count)
            var after = 0
            for q in paras[(last + 1)...] {
                if q.kind == .heading && q.level <= 1 { break }
                if textPara(q) && q.text.count >= 100 && q.size > body - 1 { after += 1 }
            }
            if nStrong >= 6 && Double(nStrong) >= 0.6 * Double(nStrong + nCont) && okList && paras[i].page >= 1
                && Double(before) >= 0.3 * Double(total) && after <= 2 {
                // (a list broken by an entry that does not look like one goes on)
                if let r = runs.last, i <= r.last + 2 { runs[runs.count - 1].last = last } else { runs.append((i, last)) }
            }
            for q in paras[i...last] where textPara(q) { before += q.text.count }
            i = last + 1
        }
        guard !runs.isEmpty else { return paras }
        var out: [Para] = []
        for (k, p) in paras.enumerated() {
            if runs.contains(where: { $0.start == k }) {
                let l = p.lines[0]
                let r = R(x0: l.bbox.x0, y0: l.bbox.y0, x1: l.bbox.x0 + 5 * l.size, y1: l.bbox.y1)
                out.append(headingPara([PLine(text: "References", bbox: r, size: l.size, bold: 1, math: 0, spans: [], page: l.page)], page: p.page))
            }
            out.append(p)
        }
        return out
    }

    /// One paragraph per entry, flagged; other paragraphs of the section (notes after the list, a
    /// publisher's box) stay as they are.
    private static let bareLabelRE = rx(#"^\s*(\[\d{1,4}\]|\d{1,4}[.)]?)\s*$"#)

    private static func entries(_ input: [Para]) -> [(Para, Bool)] {
        // a number set apart from its entry ("1." on a line of its own) goes with the entry
        var section = input
        var k = 0
        while k + 1 < section.count {
            if textPara(section[k]) && textPara(section[k + 1]) && has(bareLabelRE, section[k].text) {
                let next = section.remove(at: k + 1)
                section[k].lines += next.lines
            }
            k += 1
        }
        let texts = section.filter(textPara).map(\.text)
        let numbered = !texts.isEmpty && texts.filter { label($0) != nil }.count * 2 >= texts.count
        // whether an entry with a number starts later in the section
        var startsLater = Array(repeating: false, count: section.count)
        var seen = false
        for k in stride(from: section.count - 1, through: 0, by: -1) {
            startsLater[k] = seen
            if textPara(section[k]) && label(section[k].text) != nil { seen = true }
        }
        var out: [(Para, Bool)] = []
        var cur: Int?
        // the index in `out` of the entry being built, while no other paragraph came after it
        var open: Int?
        for (k, p) in section.enumerated() {
            if !textPara(p) {
                if !neutral(p) { open = nil }
                out.append((p, false))
                continue
            }
            let t = p.text
            // at the hanging indent of the entry before it, in the same column
            let indented = open.map { o -> Bool in
                let a = out[o].0.lines[0], b = p.lines[0]
                return a.page == b.page && b.bbox.x0 > a.bbox.x0 + 3 && b.bbox.x0 < a.bbox.x0 + 40
            } ?? false
            let starts = numbered ? label(t) != nil : (entryStart(t) || score(t) >= 2) && !(open != nil && (continues(t) || indented))
            if !starts {
                let prevEnds = open.map { has(endsRE, out[$0].0.text) } ?? true
                let goesOn = open != nil && (numbered ? (startsLater[k] || continues(t) || score(t) >= 1) : (continues(t) || indented || !prevEnds))
                if goesOn {
                    var units = split(p.lines, numbered: numbered, cur: &cur)
                    if let o = open, !units.isEmpty { out[o].0.lines += units.removeFirst() }
                    for u in units {
                        out.append((Para(u, page: u[0].page, kind: .text), true))
                        open = out.count - 1
                    }
                    continue
                }
                open = nil
                out.append((p, false))
                continue
            }
            if numbered { cur = label(t) }
            for u in split(p.lines, numbered: numbered, cur: &cur) {
                out.append((Para(u, page: u[0].page, kind: .text), true))
            }
            open = out.count - 1
        }
        return out
    }

    /// A paragraph's lines cut into entries: at the next number (or a number at the hanging
    /// indent) in a numbered list, else at the hanging indent, or after a short line that ends an
    /// entry when the next starts with an author.
    private static func split(_ lines: [PLine], numbered: Bool, cur: inout Int?) -> [[PLine]] {
        // per line, the column it is in (the lines of the paragraph starting within 40 pt of it): its
        // left and right edge, and whether lines in it are indented (a hanging indent)
        let cols: [(left: CGFloat, right: CGFloat, hanging: Bool)] = lines.map { l in
            let same = lines.filter { abs($0.bbox.x0 - l.bbox.x0) < 40 }
            let left = same.map(\.bbox.x0).min() ?? l.bbox.x0
            let right = same.map(\.bbox.x1).max() ?? l.bbox.x1
            return (left, right, same.contains { $0.bbox.x0 > left + 3 })
        }
        var units: [[PLine]] = []
        for (k, l) in lines.enumerated() {
            let t = l.text.trimmingCharacters(in: .whitespaces)
            var new = false
            if k > 0 {
                let c = cols[k]
                if numbered {
                    if let n = label(t), let cu = cur {
                        new = n == cu + 1 || (c.hanging && abs(l.bbox.x0 - c.left) < 1.5 && n > cu && n <= cu + 20)
                    }
                } else if let prev = units.last?.last {
                    let p = cols[k - 1]
                    new = (c.hanging && abs(l.bbox.x0 - c.left) < 1.5 && entryStart(t))
                        || (!c.hanging && has(lineEndsRE, prev.text.trimmingCharacters(in: .whitespaces))
                            && p.right - prev.bbox.x1 > 0.15 * max(p.right - p.left, 1) && authorStart(t))
                }
            }
            if new {
                if numbered { cur = label(t) }
                units.append([l])
            } else if units.isEmpty {
                units.append([l])
            } else {
                units[units.count - 1].append(l)
            }
        }
        return units
    }
}
