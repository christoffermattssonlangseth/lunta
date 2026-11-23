import Foundation
import CoreGraphics
import PDFKit

// Display equations found line by line, before the page's paragraphs are made (PDFAnalyzer.paragraphs
// would otherwise run a formula's rows into the prose around them): a line ending in an equation's
// number ("(7.9)", "(C.8)", "(3)") set apart at the column's right edge, with the formula's lines
// beside it and the rows, fractions and limits above and below it; and, in a book, lines of math
// centred in the column. Each becomes one equation, cropped as an image to its ink without its
// number; the number stays text, after it, for cross-references ("equation 7.9") to find.

extension PDFAnalyzer {
    /// An equation's number as set at the right of a display: "(7.9)", "(C.8)", "(12)", "(4.6a)", "(S3)".
    static let eqTagRE = try! NSRegularExpression(pattern: #"^\(((?:[A-Z]?\d{1,3}(?:\.\d{1,3}){0,2}|[A-Z]\.\d{1,3})[a-z]?)\)$"#)

    static func isEquationTag(_ t: String) -> Bool {
        eqTagRE.test(t.trimmingCharacters(in: .whitespaces))
    }

    /// A line whose text ends in an equation number set apart from the rest by a wide gap (1.2 em or
    /// more): the line without it, and the number as a line of its own. Nil when there is none (or
    /// the line is nothing but the number).
    static func splitEquationTag(_ line: PLine, chars: [(ch: String, box: R, size: CGFloat)]) -> PLine? {
        let t = line.text
        guard t.hasSuffix(")"), t.count >= 4 else { return nil }
        // the number's characters, from the end
        var k = chars.count - 1
        var tag = ""
        while k >= 0 {
            tag = chars[k].ch + tag
            if chars[k].ch == "(" { break }
            if tag.count > 12 { return nil }
            k -= 1
        }
        guard k > 0, isEquationTag(tag) else { return nil }
        guard let p = chars[..<k].lastIndex(where: { !$0.ch.trimmingCharacters(in: .whitespaces).isEmpty }) else { return nil }
        let a = chars[p].box, b = chars[k].box
        let tagSize = chars[k...].map(\.size).max() ?? line.size
        guard a.w > 0, b.w > 0, b.x0 - a.x1 >= 0.8 * max(tagSize, line.size, 4) else { return nil }
        let tagBox = R.union(chars[k...].map(\.box).filter { $0.w > 0 })
        let headLen = chars[...p].reduce(0) { $0 + $1.ch.count }
        let boxes = line.charBoxes
        let tagLine = PLine(text: tag, bbox: R(x0: tagBox.x0, y0: line.bbox.y0, x1: tagBox.x1, y1: line.bbox.y1),
                            size: tagSize, bold: 0, math: 0, spans: [Span(text: tag)], page: line.page)
        tagLine.tagOf = line
        line.text = String(t.prefix(headLen))
        line.spans = cutSpans(line.spans, keeping: headLen)
        if boxes.count == t.count || boxes.count == t.count - 1 { line.charBoxes = Array(boxes.prefix(headLen)); tagLine.charBoxes = Array(boxes.suffix(tag.count)) }
        let headBox = R.union(chars[...p].map(\.box).filter { $0.w > 0 || $0.h > 0 })
        line.bbox = R(x0: min(line.bbox.x0, headBox.x0), y0: line.bbox.y0, x1: headBox.x1, y1: line.bbox.y1)
        return tagLine
    }

    /// Pieces of a line PDFKit gives as lines of their own, inside another line: a sub- or superscript
    /// ("x∗", "θ₁₀", a citation's "¹²"), a prime, an accent over a letter ("ŷ"), the limits of a sum in
    /// the text. Each goes back into the line it is in, at its place, raised or lowered (an accent onto
    /// its letter). Only pieces of up to four characters, in smaller type (or marks), that lie within
    /// the line's width and height.
    static let marks = Set("∗*′″‴†‡").union(spacingAccents.keys.compactMap(\.first)).union(["¯", "˙", "→"])
    static func attachFragments(_ lines: inout [PLine]) {
        var gone = Set<ObjectIdentifier>()
        for f in lines {
            let t = f.text.trimmingCharacters(in: .whitespaces)
            guard !t.isEmpty, t.count <= 4, !isEquationTag(t), f.size > 0, f.bbox.w < 3 * max(f.size, 4) else { continue }
            let mark = t.allSatisfy { marks.contains($0) }
            let fc = (f.bbox.y0 + f.bbox.y1) / 2
            var best: PLine?, bestD = CGFloat.greatestFiniteMagnitude
            for h in lines where h !== f && !gone.contains(ObjectIdentifier(h)) && h.text.count >= t.count + 3
                && (h.charBoxes.count == h.text.count || h.text.hasSuffix(glue) && h.charBoxes.count == h.text.count - 1) {
                guard mark || f.size < 0.9 * h.size,
                      f.bbox.x0 >= h.bbox.x0 - 0.5, f.bbox.x0 < h.bbox.x1 + 0.3 * h.size, f.bbox.x1 <= h.bbox.x1 + 1.5 * h.size else { continue }
                // its middle within the line's height and a little more
                guard fc >= h.bbox.y0 - 0.4 * h.size, fc <= h.bbox.y1 + 0.4 * h.size else { continue }
                // (not into a line it would sit on top of: a piece of the line above or below)
                let hc = (h.bbox.y0 + h.bbox.y1) / 2
                let d = abs(fc - hc)
                if d < bestD { bestD = d; best = h }
            }
            guard let h = best else { continue }
            // the line's own middle: its characters in its type size
            let own = h.charBoxes.filter { $0.h > 0 && $0.h < 2 * h.size }
            let mid = own.isEmpty ? (h.bbox.y0 + h.bbox.y1) / 2 : own.map { ($0.y0 + $0.y1) / 2 }.sorted()[own.count / 2]
            let up = fc < mid
            let chars = Array(h.text)
            // where it goes: after the characters that start before it
            let k = h.charBoxes.firstIndex { $0.w > 0 && ($0.x0 + $0.x1) / 2 > f.bbox.x0 } ?? chars.count
            // (a script follows something: a piece before the line's first letter is not one, an entry's
            // "1." beside a heading)
            guard k > 0 else { continue }
            if up, t.count == 1, let comb = spacingAccents[t] ?? (t == "¯" ? "\u{0304}" : t == "˙" ? "\u{0307}" : nil),
               let j = h.charBoxes.firstIndex(where: { $0.x0 <= f.bbox.cx + 0.5 && $0.x1 >= f.bbox.cx - 0.5 && $0.w > 0 }),
               chars[j].isLetter {
                // an accent over a letter: that letter's
                let folded = (String(chars[j]) + comb).precomposedStringWithCanonicalMapping
                h.text = String(chars[..<j]) + folded + String(chars[(j + 1)...])
                var pos = 0
                for si in h.spans.indices {
                    let n = h.spans[si].text.count
                    if j < pos + n {
                        var sc = Array(h.spans[si].text)
                        sc[j - pos] = Character(folded)
                        h.spans[si].text = String(sc)
                        break
                    }
                    pos += n
                }
                gone.insert(ObjectIdentifier(f))
                continue
            }
            // (the line's text may end in the marker for a word that goes on: the spans do not)
            let spanLen = h.spans.reduce(0) { $0 + $1.text.count }
            guard k <= spanLen else { continue }
            var ins = f.spans.filter { !$0.text.trimmingCharacters(in: .whitespaces).isEmpty }
            for i in ins.indices {
                ins[i].text = ins[i].text.trimmingCharacters(in: .whitespaces)
                ins[i].sup = up && !mark || t.allSatisfy { "∗*†‡".contains($0) }
                ins[i].sub = !ins[i].sup && !t.allSatisfy { "′″‴".contains($0) }
            }
            h.text = String(chars[..<k]) + t + String(chars[k...])
            var newSpans: [Span] = []
            var pos = 0, done = false
            for s in h.spans {
                let n = s.text.count
                if !done && k <= pos + n {
                    let cut = k - pos
                    var a = s, b = s
                    a.text = String(s.text.prefix(cut)); b.text = String(s.text.dropFirst(cut))
                    if !a.text.isEmpty { newSpans.append(a) }
                    newSpans += ins
                    if !b.text.isEmpty { newSpans.append(b) }
                    done = true
                } else {
                    newSpans.append(s)
                }
                pos += n
            }
            if !done { newSpans += ins }
            h.spans = newSpans
            h.charBoxes.insert(contentsOf: Array(repeating: f.bbox, count: t.count), at: k)
            h.bbox = h.bbox.union(f.bbox)
            gone.insert(ObjectIdentifier(f))
        }
        if !gone.isEmpty { lines.removeAll { gone.contains(ObjectIdentifier($0)) } }
    }

    /// The spans' first `n` characters.
    static func cutSpans(_ spans: [Span], keeping n: Int) -> [Span] {
        var out: [Span] = []
        var left = n
        for s in spans where left > 0 {
            if s.text.count <= left { out.append(s); left -= s.text.count } else {
                var c = s
                c.text = String(s.text.prefix(left))
                out.append(c)
                left = 0
            }
        }
        return out
    }

    /// Where the page's text column(s) start and end: the left and right ends shared by its lines of
    /// running text, with those of the pages on the same side of the book (a page of formulas has
    /// few of its own).
    func columnEdges(_ pno: Int, _ lines: [PLine]) -> (lefts: [CGFloat], rights: [CGFloat]) {
        func edges(_ ls: [PLine]) -> (lefts: [CGFloat], rights: [CGFloat]) {
            var l: [Int: Int] = [:], r: [Int: Int] = [:]
            var n = 0
            for x in ls where abs(x.size - bodySize) <= 1.6 && x.text.count > 40 && Self.isProse(x.text) {
                l[Int(x.bbox.x0.rounded()), default: 0] += 1
                r[Int(x.bbox.x1.rounded()), default: 0] += 1
                n += 1
            }
            let need = max(3, n / 12)
            // (neighbouring values counted together: justified lines end within a point)
            func peaks(_ h: [Int: Int]) -> [CGFloat] {
                var out: [CGFloat] = []
                for (x, _) in h.sorted(by: { $0.value > $1.value }) {
                    let c = (x - 1...x + 1).reduce(0) { $0 + (h[$1] ?? 0) }
                    if c >= need && !out.contains(where: { abs($0 - CGFloat(x)) <= 2 }) { out.append(CGFloat(x)) }
                }
                return out
            }
            return (peaks(l), peaks(r))
        }
        if docEdges.isEmpty {
            // the document's, by the side of the page
            for side in 0..<2 {
                let ls = pagesLines.indices.filter { $0 % 2 == side }.flatMap { pagesLines[$0] }
                docEdges[side] = edges(ls)
            }
        }
        let own = edges(lines)
        let doc = docEdges[pno % 2] ?? ([], [])
        return (own.lefts + doc.lefts.filter { d in !own.lefts.contains { abs($0 - d) <= 2 } },
                own.rights + doc.rights.filter { d in !own.rights.contains { abs($0 - d) <= 2 } })
    }

    /// Words of running text in `t`: three or more lower-case letters, not a function's name ("log", "exp", "sig").
    static func textWords(_ t: String) -> Int {
        let ns = t as NSString
        return proseWordRE.matches(in: t, range: NSRange(location: 0, length: ns.length))
            .filter { !mathWords.contains(ns.substring(with: $0.range)) }.count
    }
    static let mathWords: Set<String> = ["log", "exp", "sin", "cos", "tan", "tanh", "sinh", "cosh", "max", "min", "sup", "inf", "sig", "lim",
                                         "det", "arg", "argmax", "argmin", "softmax", "softplus", "relu", "var", "cov", "diag", "mod", "sgn",
                                         "sign", "erf", "std", "mean", "trace", "rank", "dim", "ker", "atan", "arcsin", "arccos", "arctan"]

    static let mathChars = CharacterSet(charactersIn: "=<>≤≥≈≡∝∼≃+−×·∑∏∫∂√∞∇∈∉⊂⊆∪∩→←↔⇒⇔∀∃^_|‖⟨⟩⌈⌉⌊⌋±∓∘⊙⊗⊕")
        .union(CharacterSet(charactersIn: UnicodeScalar(0xE000)!...UnicodeScalar(0xF8FF)!))
        .union(CharacterSet(charactersIn: UnicodeScalar(0x0391)!...UnicodeScalar(0x03C9)!))

    /// The page's display equations: their lines out of `lines`, each equation a paragraph of kind
    /// `.equation` with its number (`eqLabel`), the area to crop its ink from (`crop`) and its text.
    func displayEquations(_ pno: Int, _ lines: [PLine], _ regions: [Region]) -> ([PLine], [Para]) {
        let body = bodySize
        let (lefts, rights) = columnEdges(pno, lines)
        guard !rights.isEmpty else { return (lines, []) }
        let id = { (l: PLine) in ObjectIdentifier(l) }
        let tags = lines.filter { l in
            Self.isEquationTag(l.text) && l.size >= body - 2.5 && l.size <= body + 1.5
                && rights.contains { abs($0 - l.bbox.x1) <= 0.6 * l.size + 1 }
        }.sorted { $0.bbox.y0 < $1.bbox.y0 }
        let tagIDs = Set(tags.map(id))
        if Self.debugEquations {
            print("EQ p\(pno + 1) rights \(rights.map { Int($0) }) lefts \(lefts.map { Int($0) }) tags \(tags.map(\.text)) candidates \(lines.filter { Self.isEquationTag($0.text) }.map { "\($0.text)@\(Int($0.bbox.x1))" })")
        }
        var taken = Set<ObjectIdentifier>()
        var out: [Para] = []

        func vOverlap(_ a: R, _ b: R) -> CGFloat { min(a.y1, b.y1) - max(a.y0, b.y0) }
        /// the left edge of the column that `x` is in: the first left edge after the column before it
        /// (not a paragraph's indent)
        func columnLeft(_ x: CGFloat) -> CGFloat {
            let prevRight = rights.filter { $0 < x }.max() ?? -.infinity
            return lefts.filter { $0 > prevRight && $0 <= x + 3 * body }.min() ?? 0
        }
        /// a line that may be part of a formula: not running text, a caption or a bold heading; set in
        /// from the column's left edge or in math type
        func mathLike(_ l: PLine, colLeft: CGFloat) -> Bool {
            let t = l.text.trimmingCharacters(in: .whitespaces)
            if t.isEmpty || Self.isProse(t) || Self.captionRE.test(t) { return false }
            if l.bold > 0.6 && l.math < 0.2 && t.count > 3 { return false }   // a heading, a run-in head
            let words = Self.textWords(t)
            let em = max(l.size, body)
            let indented = l.bbox.x0 > colLeft + 1.0 * em
            if !indented && l.math < 0.4 { return false }
            if words >= 3 && l.math < 0.3 { return false }
            // (running text about the formulas, in math type: "where ζ = e…, and", "• IB(0) = 0. • IB(R) = …",
            // "Q(t) := … is the Gaussian complementary CDF and …")
            if words >= 4 || words >= 2 && !indented || Self.bulletRE.test(t) { return false }
            if t.hasSuffix(":") && words >= 2 { return false }
            let mathy = l.math >= 0.3 || t.unicodeScalars.contains { Self.mathChars.contains($0) }
            return mathy || t.count <= 6
        }
        /// math in it: a math font, or a symbol of one
        func evidence(_ ls: [PLine]) -> Bool {
            ls.contains { l in l.math >= 0.25 || l.text.unicodeScalars.contains { Self.mathChars.contains($0) } }
        }
        /// the lines next to `block` (within `gap`) that go on with the formula, until none does
        func grow(_ block: inout [PLine], colLeft: CGFloat, colRight: CGFloat, gap: CGFloat, _ mine: (PLine) -> Bool = { _ in true }) {
            var box = R.union(block.map(\.bbox))
            var added = true
            while added && block.count < 40 {
                added = false
                for l in lines where !taken.contains(id(l)) && !tagIDs.contains(id(l)) && !block.contains(where: { $0 === l }) {
                    guard l.bbox.x0 >= colLeft - 1.5 * body, l.bbox.x1 <= colRight + 1,
                          l.bbox.y0 < box.y1 + gap, l.bbox.y1 > box.y0 - gap, mathLike(l, colLeft: colLeft), mine(l) else { continue }
                    block.append(l)
                    box = box.union(l.bbox)
                    added = true
                }
            }
        }
        func makeEquation(_ block: [PLine], label: String?, colLeft: CGFloat, colRight: CGFloat, blank: R? = nil) {
            for l in block { taken.insert(id(l)) }
            let box = R.union(block.map(\.bbox))
            // the crop: the column beside the number (or the whole column), up and down to the lines and
            // figures around it, at most an em; its ink is taken from there (fraction rules, big
            // operators and brackets the text has no characters for)
            // (the lines of the equations before and after it too)
            // (a line PDFKit gives a box much taller than its type, a matrix's, says little about where it is)
            let others = lines.filter { l in
                !block.contains { $0 === l } && !tagIDs.contains(id(l)) && l.bbox.x1 > colLeft && l.bbox.x0 < colRight
                    && (l.bbox.h <= 1.8 * max(l.size, 4) || !taken.contains(id(l)) && !mathLike(l, colLeft: colLeft))
            }
                .map(\.bbox) + regions.map(\.rect)
            let em = body
            // (a line beside it, its box made tall by the formula's, is above or below it by its middle)
            let near = others.filter { $0.x1 > box.x0 - em && $0.x0 < box.x1 + em }
            let cy = (box.y0 + box.y1) / 2
            let above = near.filter { $0.y1 <= box.y0 + 1 || ($0.y0 + $0.y1) / 2 < cy && $0.y0 < box.y0 }.map(\.y1).max() ?? -.infinity
            let below = near.filter { $0.y0 >= box.y1 - 1 || ($0.y0 + $0.y1) / 2 > cy && $0.y1 > box.y1 }.map(\.y0).min() ?? .infinity
            let crop = R(x0: max(colLeft - 0.5 * em, box.x0 - 2 * em), y0: max(box.y0 - 1.2 * em, above + 0.5),
                         x1: min(colRight, box.x1 + 2 * em), y1: min(box.y1 + 1.2 * em, below - 0.5))
            let p = Para(block.sorted { ($0.bbox.y0, $0.bbox.x0) < ($1.bbox.y0, $1.bbox.x0) }, page: pno, kind: .equation, region: box)
            p.eqLabel = label
            p.crop = crop
            p.blank = blank
            out.append(p)
        }

        // numbered equations
        // (a number's band: an em around its middle; PDFKit may give it the height of a matrix beside it)
        func band(_ t: PLine) -> R {
            let cy = (t.bbox.y0 + t.bbox.y1) / 2, h = 0.6 * max(t.size, 4)
            return R(x0: t.bbox.x0, y0: max(t.bbox.y0, cy - h), x1: t.bbox.x1, y1: min(t.bbox.y1, cy + h))
        }
        for tag in tags where !taken.contains(id(tag)) {
            let em = max(tag.size, body)
            let colLeft = columnLeft(tag.bbox.x0 - 3 * em)
            var colRight = tag.bbox.x0 - 0.5
            let b = band(tag)
            let inColumn = { (l: PLine) in l.bbox.x0 >= colLeft - 1.5 * em && l.bbox.x1 <= colRight + 0.5 }
            let beside = lines.filter { l in
                !taken.contains(id(l)) && !tagIDs.contains(id(l)) && inColumn(l)
                    && vOverlap(l.bbox, b) > 0.3 * min(max(l.bbox.h, 1), max(b.h, 1))
            }
            // (running text beside it: a number in the text, not an equation's; a line of text whose box
            // is tall with a formula in it counts only if it is level with the number)
            if beside.contains(where: { l in
                l.text.count > 30 && Self.isProse(l.text) && (l.bbox.y0 + l.bbox.y1) / 2 >= b.y0 && (l.bbox.y0 + l.bbox.y1) / 2 <= b.y1
            }) { debugEq(pno, tag, "prose beside"); continue }
            var block = beside.filter { mathLike($0, colLeft: colLeft) }
            // a number set below its equation's last line (it did not fit beside it)
            if block.isEmpty {
                block = lines.filter { l in
                    !taken.contains(id(l)) && !tagIDs.contains(id(l)) && l.bbox.x0 >= colLeft - 1.5 * em && l.bbox.x1 <= tag.bbox.x1 + 0.5
                        && l.bbox.y0 < tag.bbox.y0 - 1 && l.bbox.y1 >= tag.bbox.y0 - 0.9 * em && mathLike(l, colLeft: colLeft)
                }
                if !block.isEmpty { colRight = max(colRight, block.map(\.bbox.x1).max()! + 0.5) }
            }
            guard !block.isEmpty else { debugEq(pno, tag, "nothing beside"); continue }
            // (the lines nearer another number in the column are that equation's)
            let others = tags.filter { $0 !== tag && abs($0.bbox.x1 - tag.bbox.x1) < 3 * em }.map(band)
            let cyTag = (b.y0 + b.y1) / 2
            // (rows of the formula not level with its number may reach past it, to the column's edge)
            let fullRight = max(colRight, rights.filter { abs($0 - tag.bbox.x1) <= 0.6 * tag.size + 1 }.max() ?? colRight)
            grow(&block, colLeft: colLeft, colRight: fullRight, gap: 0.6 * em) { l in
                // (not the text level with the number, past it: a line PDFKit gives a tall box may be)
                if l.bbox.x1 > colRight + 0.5 && l.bbox.h <= 1.8 * max(l.size, 4) && vOverlap(l.bbox, tag.bbox) > 0.5 * tag.bbox.h { return false }
                let cy = (l.bbox.y0 + l.bbox.y1) / 2
                return !others.contains { abs(($0.y0 + $0.y1) / 2 - cy) < abs(cyTag - cy) }
            }
            guard evidence(block) else { debugEq(pno, tag, "no math: " + block.map { "\($0.text)[\($0.math)]" }.joined(separator: " / ")); continue }
            taken.insert(id(tag))
            makeEquation(block, label: Self.eqTagRE.firstMatch(in: tag.text.trimmingCharacters(in: .whitespaces),
                                                               range: NSRange(location: 0, length: (tag.text.trimmingCharacters(in: .whitespaces) as NSString).length))
                .map { (tag.text.trimmingCharacters(in: .whitespaces) as NSString).substring(with: $0.range(at: 1)) },
                         colLeft: colLeft, colRight: max(colRight, block.map(\.bbox.x1).max()! + 0.5),
                         blank: R(x0: tag.bbox.x0 - 1, y0: tag.bbox.y0 - 1, x1: tag.bbox.x1 + 2, y1: tag.bbox.y1 + 1))
        }
        // a book's unnumbered displays: math centred in the column, set apart from the running text
        if kind.isBookLike {
            for l in lines.sorted(by: { $0.bbox.y0 < $1.bbox.y0 }) where !taken.contains(id(l)) && !tagIDs.contains(id(l)) {
                let em = max(l.size, body)
                guard l.size >= body - 1.1, l.math >= 0.45, l.text.trimmingCharacters(in: .whitespaces).count >= 3,
                      case let colLeft = columnLeft(l.bbox.x0), colLeft > 0,
                      let colRight = rights.filter({ $0 >= l.bbox.x1 - 1 }).min(),
                      l.bbox.x0 >= colLeft + 2 * em, mathLike(l, colLeft: colLeft) else { continue }
                let mid = (colLeft + colRight) / 2, cx = (l.bbox.x0 + l.bbox.x1) / 2
                guard abs(cx - mid) <= 0.2 * (colRight - colLeft) else { continue }
                // (not a piece of a line of text beside it, nor a line of a paragraph set in from the
                // column: a caption's, a quotation's)
                if lines.contains(where: { n in
                    n !== l && vOverlap(n.bbox, l.bbox) > 0.5 * min(n.bbox.h, l.bbox.h)
                        && max(n.bbox.x0 - l.bbox.x1, l.bbox.x0 - n.bbox.x1) < 1.5 * em && !mathLike(n, colLeft: colLeft)
                }) { continue }
                if lines.contains(where: { n in
                    n !== l && abs(n.bbox.x0 - l.bbox.x0) < 2 && n.text.count > 30 && Self.isProse(n.text)
                        && max(n.bbox.y0 - l.bbox.y1, l.bbox.y0 - n.bbox.y1) < 0.5 * em
                }) { continue }
                var block = [l]
                grow(&block, colLeft: colLeft, colRight: colRight, gap: 0.6 * em)
                let n = block.reduce(0) { $0 + $1.text.count }
                let mf = block.reduce(CGFloat(0)) { $0 + $1.math * CGFloat($1.text.count) } / CGFloat(max(1, n))
                guard mf >= 0.4 else { continue }
                makeEquation(block, label: nil, colLeft: colLeft, colRight: colRight)
            }
        }
        // a number split off its line (see splitEquationTag) that is no equation's goes back on it
        var rest = lines.filter { !taken.contains(id($0)) }
        let back = rest.filter { $0.tagOf != nil }
        for t in back {
            guard let h = t.tagOf, rest.contains(where: { $0 === h }) else { continue }
            h.text += " " + t.text
            h.spans += [Span(text: " ")] + t.spans
            h.bbox = h.bbox.union(t.bbox)
            rest.removeAll { $0 === t }
        }
        for l in lines { l.tagOf = nil }
        guard !out.isEmpty else { return (rest, []) }
        return (rest, out)
    }

    static let debugEquations = ProcessInfo.processInfo.environment["P2E_DEBUG_EQ"] != nil
    func debugEq(_ pno: Int, _ tag: PLine, _ why: String) {
        if Self.debugEquations { print("EQ p\(pno + 1) \(tag.text): \(why)") }
    }

    /// An equation's ink within `crop`, cropped close (the margin's and the number's left out by the
    /// crop), and its width in points.
    func renderInk(_ pno: Int, _ crop: R, blank: R? = nil, name: String) -> (BookImage, CGFloat)? {
        guard let page = doc.page(at: pno) else { return nil }
        let mb = boxes[pno]
        let r = crop.intersection(R(x0: 0, y0: 0, x1: mb.width, y1: mb.height))
        guard r.w > 2, r.h > 2 else { return nil }
        let scale = min(200.0 / 72.0, 1600 / max(r.w, r.h))
        let W = Int((r.w * scale).rounded()), H = Int((r.h * scale).rounded())
        guard W > 0, H > 0, let cs = CGColorSpace(name: CGColorSpace.sRGB),
              let ctx = CGContext(data: nil, width: W, height: H, bitsPerComponent: 8, bytesPerRow: W * 4, space: cs,
                                  bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue) else { return nil }
        ctx.setFillColor(CGColor(red: 1, green: 1, blue: 1, alpha: 1))
        ctx.fill(CGRect(x: 0, y: 0, width: W, height: H))
        ctx.saveGState()
        ctx.scaleBy(x: scale, y: scale)
        ctx.translateBy(x: -r.x0, y: -(mb.height - r.y1))
        page.draw(with: .mediaBox, to: ctx)
        // (the equation's number, where the crop reaches past it)
        if let b = blank {
            ctx.setFillColor(CGColor(red: 1, green: 1, blue: 1, alpha: 1))
            ctx.fill(CGRect(x: b.x0, y: mb.height - b.y1, width: b.w, height: b.h))
        }
        ctx.restoreGState()
        guard let data = ctx.data else { return nil }
        let px = data.bindMemory(to: UInt8.self, capacity: W * H * 4)
        func dark(_ x: Int, _ y: Int) -> Bool {
            let o = px + (y * W + x) * 4
            return Int(o[0]) + Int(o[1]) + Int(o[2]) < 3 * 200
        }
        let inkRow = (0..<H).map { y in (0..<W).contains { dark($0, y) } }
        // ink cut by the crop's top or bottom edge, apart from the rest by a white band: the bottom of the
        // formula above (a matrix's brackets), the top of the one below
        var top = 0, bottom = H - 1
        let gap = max(2, Int((0.15 * bodySize * scale).rounded()))
        func band(from start: Int, step: Int) -> Int? {
            // the ink touching the edge, then at least `gap` white rows
            var y = start
            while y >= 0 && y < H && inkRow[y] { y += step }
            var w = 0
            while y >= 0 && y < H && !inkRow[y] { w += 1; y += step }
            return w >= gap && y >= 0 && y < H ? y : nil
        }
        if inkRow.first == true, let y = band(from: 0, step: 1), inkRow[y...].filter({ $0 }).count >= 2 * inkRow[..<y].filter({ $0 }).count { top = y }
        if inkRow.last == true, let y = band(from: H - 1, step: -1), y > top, inkRow[...y].filter({ $0 }).count >= 2 * inkRow[(y + 1)...].filter({ $0 }).count { bottom = y }
        var minX = W, minY = H, maxX = -1, maxY = -1
        for y in top...bottom where inkRow[y] {
            for x in 0..<W where dark(x, y) {
                if x < minX { minX = x }
                if x > maxX { maxX = x }
                if y < minY { minY = y }
                if y > maxY { maxY = y }
            }
        }
        // (nothing, or a sliver: the crop missed the formula; the caller crops its box instead)
        guard maxX >= minX, maxY >= minY, CGFloat(maxY - minY) >= 0.5 * bodySize * scale, let full = ctx.makeImage() else { return nil }
        let pad = Int((2 * scale).rounded())
        let box = CGRect(x: max(0, minX - pad), y: max(0, minY - pad), width: min(W, maxX + pad + 1) - max(0, minX - pad),
                         height: min(H, maxY + pad + 1) - max(0, minY - pad))
        guard let img = full.cropping(to: box) else { return nil }
        let out = NSMutableData()
        guard let dest = CGImageDestinationCreateWithData(out, "public.png" as CFString, 1, nil) else { return nil }
        CGImageDestinationAddImage(dest, img, nil)
        guard CGImageDestinationFinalize(dest) else { return nil }
        return (BookImage(fileName: "\(uniqueName(name)).png", data: out as Data, mediaType: "image/png"), box.width / scale)
    }

    /// An equation's text, for its image's text alternative: its lines top to bottom, left to right.
    static func equationText(_ p: Para) -> String {
        let ls = p.lines.sorted { a, b in
            abs(a.bbox.y0 - b.bbox.y0) < 0.4 * max(a.size, 4) ? a.bbox.x0 < b.bbox.x0 : a.bbox.y0 < b.bbox.y0
        }
        return ls.map { lineEndsCleaned($0.text).trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }.joined(separator: " ")
            .unicodeScalars.filter { !(0xE000...0xF8FF).contains($0.value) }.map(String.init).joined()
            .replacingOccurrences(of: #"\s+"#, with: " ", options: .regularExpression)
    }
}
