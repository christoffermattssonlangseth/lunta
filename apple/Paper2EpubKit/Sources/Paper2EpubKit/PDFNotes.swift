import Foundation
import CoreGraphics

// Footnotes and margin notes of a PDF (core/src/convert/pdf/notes.rs is the same in Rust).
//
// A footnote is a block at the foot of a page, below the last line of body type in its column and set
// apart from it, in smaller type, that starts with a marker ("²", "*", "†", "a") which the page's
// running text also has: a superscript (or a symbol) right after a word or a punctuation mark. The
// note leaves the running text for the end of its chapter (`<aside epub:type="footnote">`) and the
// marker in the text becomes a link to it (`<a epub:type="noteref">`) that keeps its character. A
// block at the next page's foot without a marker continues the last note of the page before.
//
// A superscript number in a paper that cites by number is a citation unless such a block exists on
// its page; with several candidates for the same number the note is still moved but nothing is
// linked. The reference list's pages, the contents and the index are left alone, and so are the
// first pages' footnotes the front matter already took (correspondence, affiliations, "equal
// contribution").
//
// A margin note (books only) is a block of small type outside the text column ("Notebook 1.1",
// "Problems 2.1–2.2", "Appendix C.5.2"): taken out before the page's paragraphs are made (it would
// split them), it follows the paragraph level with it as `<aside class="sidenote">`.

extension PDFAnalyzer {
    /// A footnote's marker at the start of its first line: a superscript "2", "*", "†", "a", or a plain
    /// "2"/"†" before its text ("2 This …", "2This …", "†These …"). Not "2. Smith …" or "[2] …".
    static func noteMarker(_ l: PLine) -> String? {
        guard let k = l.spans.firstIndex(where: { !$0.text.trimmingCharacters(in: .whitespaces).isEmpty }) else { return nil }
        let s = l.spans[k], t = s.text.trimmingCharacters(in: .whitespaces)
        if s.sup {
            guard t.matches(#"^(\d{1,3}|[*†‡§¶‖#]{1,3}|[a-z])$"#),
                  l.spans[(k + 1)...].contains(where: { $0.text.rangeOfCharacter(from: .letters) != nil }) else { return nil }
            return t
        }
        let lt = l.text.trimmingCharacters(in: .whitespaces)
        if let m = Resolver.firstMatch(#"^([*†‡§¶‖]{1,3})\s*\p{L}"#, in: lt) { return m }
        if let m = Resolver.firstMatch(#"^(\d{1,3})(?:\s+\p{Lu}|\p{Lu}\p{Ll})"#, in: lt) { return m }
        return nil
    }

    /// The line without its leading marker (spans and text).
    static func stripMarker(_ l: PLine, _ m: String) {
        if let k = l.spans.firstIndex(where: { !$0.text.trimmingCharacters(in: .whitespaces).isEmpty }) {
            if l.spans[k].sup {
                l.spans.removeSubrange(0...k)
            } else {
                var t = Substring(l.spans[k].text.drop { $0 == " " })
                if t.hasPrefix(m) { t = t.dropFirst(m.count) }
                l.spans[k].text = String(t)
                l.spans.removeSubrange(0..<k)
            }
            if let f = l.spans.first { l.spans[0].text = String(f.text.drop { $0 == " " }) }
        }
        var t = Substring(l.text.drop { $0 == " " })
        if t.hasPrefix(m) { t = t.dropFirst(m.count) }
        l.text = String(t.drop { $0 == " " })
    }

    /// A place in the running text where a note's marker is: a span of `line`, or (for a symbol set in
    /// the text's own type) the character at `at` within it.
    struct NoteRef { let line: PLine; let span: Int; let at: Int?; let score: Int }

    /// Where `marker` stands in these paragraphs' text as a note's marker, with how much it looks like
    /// one: after a word of three or more letters or a punctuation mark rather than a variable or a
    /// digit ("x²", "(a+b)²" in a formula), not run into the next word.
    static func noteRefs(_ marker: String, in paras: [Para]) -> [NoteRef] {
        let symbol = marker.first.map { noteSymbols.contains($0) } ?? false
        var out: [NoteRef] = []
        for p in paras {
            for l in p.lines {
                for (k, s) in l.spans.enumerated() {
                    let before = l.spans[..<k].map(\.text).joined()
                    let after = l.spans[(k + 1)...].map(\.text).joined()
                    if s.sup, s.noteRef == nil, s.text.trimmingCharacters(in: .whitespaces) == marker, !s.text.hasPrefix(" ") {
                        out.append(NoteRef(line: l, span: k, at: nil, score: refScore(before, after, italic: s.italic, line: l)))
                    } else if symbol, !s.sup, s.noteRef == nil {
                        // "word†" in the text's own type
                        let chars = Array(s.text)
                        for (i, c) in chars.enumerated() where String(c) == marker {
                            let prev = i > 0 ? String(chars[..<i]) : before
                            guard let pc = prev.last, pc.isLetter || pc.isNumber || ".,;:)”’]".contains(pc) else { continue }
                            out.append(NoteRef(line: l, span: k, at: i, score: refScore(prev, String(chars[(i + 1)...]) + after, italic: false, line: l)))
                        }
                    }
                }
            }
        }
        return out
    }

    static func refScore(_ before: String, _ after: String, italic: Bool, line: PLine) -> Int {
        var score = 0
        if let pc = before.last {
            if ".,;:!?)”’\"]".contains(pc) { score += 2 }
            else if pc.isLetter {
                let run = before.reversed().prefix { $0.isLetter }.count
                score += run >= 3 ? 1 : -2
            } else if pc.isNumber { score -= 2 }
            else if pc == " " { score -= 1 }
        }
        if let nc = after.first, nc.isLetter || nc.isNumber { score -= 2 }
        if italic { score -= 1 }
        if line.math > 0.5 { score -= 1 }
        return score
    }

    static let noteSymbols: Set<Character> = ["*", "†", "‡", "§", "¶", "‖", "#"]

    /// The id of a note: "fn-p12-2" (page 12, marker 2), "fn-p3-s1" for the first symbol.
    static func noteID(page: Int, marker: String, taken: inout Set<String>) -> String {
        let key: String
        if marker.matches(#"^[0-9a-z]+$"#) { key = marker }
        else {
            let order: [Character] = ["*", "†", "‡", "§", "¶", "‖", "#"]
            let i = marker.first.flatMap { order.firstIndex(of: $0) } ?? 0
            key = "s\(i + 1)\(marker.count > 1 ? "x\(marker.count)" : "")"
        }
        var id = "fn-p\(page + 1)-\(key)", n = 2
        while taken.contains(id) { id = "fn-p\(page + 1)-\(key)-\(n)"; n += 1 }
        taken.insert(id)
        return id
    }

    /// Links the marker in the text to note `id` (a plain symbol becomes a span of its own).
    static func link(_ r: NoteRef, to id: String) {
        let l = r.line
        if let at = r.at {
            let s = l.spans[r.span], chars = Array(s.text)
            var a = s; a.text = String(chars[..<at])
            var m = s; m.text = String(chars[at]); m.noteRef = id
            var b = s; b.text = String(chars[(at + 1)...])
            l.spans.replaceSubrange(r.span...r.span, with: [a, m, b].filter { !$0.text.isEmpty })
        } else {
            l.spans[r.span].noteRef = id
        }
    }

    // MARK: - footnotes

    func findFootnotes(_ input: [Para]) -> [Para] {
        let body = bodySize
        // the reference list on: its entries are not notes
        let refsAt = input.firstIndex { ($0.kind == .heading || ($0.kind == .text && $0.lines.count <= 2)) && PDFReferences.isHeading($0.text) } ?? input.count
        // a paper that cites by superscript numbers
        var numericSups = 0
        for p in input where p.kind == .text || p.kind == .abstract {
            for l in p.lines { numericSups += l.spans.filter { $0.sup && $0.text.trimmingCharacters(in: .whitespaces).matches(#"^\d{1,3}([,–\-]\d{1,3})*$"#) }.count }
        }
        let citesByNumber = kind == .paper && numericSups >= 8
        var byPage: [Int: [Int]] = [:]
        for (i, p) in input.enumerated() where i < refsAt { byPage[p.page, default: []].append(i) }
        func small(_ p: Para) -> Bool { p.kind == .text && p.boxed == nil && p.size > 0 && p.size <= body - 0.6 && !p.lines.isEmpty }
        func blocks(_ p: Para) -> Bool {
            switch p.kind {
            case .text: return !small(p)
            case .heading, .abstract, .equation, .figure, .table, .caption, .title: return true
            default: return false
            }
        }
        func overlapX(_ a: R, _ b: R) -> Bool { min(a.x1, b.x1) - max(a.x0, b.x0) > 2 }
        var replace: [Int: [Para]] = [:]     // a paragraph's place → what goes there instead
        var ids = Set<String>()
        var lastNote: Para?, lastNotePage = -10
        var found = 0, linked = 0, continued = 0
        for pno in byPage.keys.sorted() {
            if tocPages.contains(pno) || onIndexPage(pno) { continue }
            let idx = byPage[pno]!
            let H = boxes[pno].height
            // the foot of the page: small type with nothing of the body below it in its column, set apart
            // from the text above it (or stacked under another note)
            var foot: [Int] = []
            for i in idx.sorted(by: { input[$0].bbox.y0 < input[$1].bbox.y0 }) where small(input[i]) && input[i].bbox.y0 > 0.4 * H {
                let r = input[i].bbox
                let col = idx.filter { $0 != i && blocks(input[$0]) && overlapX(input[$0].bbox, r) }
                if col.contains(where: { input[$0].bbox.y1 > r.y0 + 1 }) { continue }
                let aboveBody = col.map { input[$0].bbox.y1 }.max()
                let aboveFoot = foot.filter { overlapX(input[$0].bbox, r) }.map { input[$0].bbox.y1 }.max()
                if let f = aboveFoot, f >= (aboveBody ?? -1) { foot.append(i); continue }
                if let a = aboveBody, r.y0 - a >= 0.4 * body { foot.append(i) }
            }
            guard !foot.isEmpty, foot.map({ input[$0].bbox.y1 }).max()! > 0.75 * H else { continue }
            let footSet = Set(foot)
            let text = idx.filter { !footSet.contains($0) && [.text, .abstract, .heading, .caption].contains(input[$0].kind) }.map { input[$0] }
            func refs(_ m: String) -> [NoteRef] { Self.noteRefs(m, in: text) }
            // the foot's blocks, cut where a line starts with a marker the text has
            var chunks: [(at: Int, lines: [PLine])] = []
            for i in foot.sorted() {
                var cur: [PLine] = []
                for (k, l) in input[i].lines.enumerated() {
                    if k > 0, let m = Self.noteMarker(l), !refs(m).isEmpty, !cur.isEmpty { chunks.append((i, cur)); cur = [] }
                    cur.append(l)
                }
                if !cur.isEmpty { chunks.append((i, cur)) }
            }
            let markers = chunks.map { Self.noteMarker($0.lines[0]) }
            let anyMatched = markers.contains { $0.map { !refs($0).isEmpty } ?? false }
            guard anyMatched || (lastNotePage == pno - 1 && lastNote != nil) else { continue }
            var notes: [Para] = []
            var out: [Int: [Para]] = [:]
            var unmarked: [(at: Int, para: Para, marked: Bool)] = []
            for (c, m) in zip(chunks, markers) {
                let p = Para(c.lines, page: pno)
                guard let m, anyMatched else { unmarked.append((c.at, p, m != nil)); continue }
                let rs = refs(m)
                // (a block that reads like a reference, when the text doesn't have its marker or the paper
                // cites by number: the reference list explains the number better; left alone)
                if (rs.isEmpty || citesByNumber) && !Self.isProse(p.text) && PDFReferences.score(p.text) >= 3 { unmarked.append((c.at, p, true)); continue }
                p.kind = .footnote
                p.noteLabel = m
                p.noteID = Self.noteID(page: pno, marker: m, taken: &ids)
                Self.stripMarker(p.lines[0], m)
                // the marker in the text: the likeliest place; in a paper citing by number, only if no
                // other superscript with that number could be it
                let top = rs.map(\.score).max() ?? -1
                if top >= 0, let best = rs.first(where: { $0.score == top }),
                   !(citesByNumber && m.first?.isNumber == true && rs.filter({ $0.score >= 0 }).count > 1) {
                    Self.link(best, to: p.noteID!)
                    linked += 1
                }
                notes.append(p)
                out[c.at, default: []].append(p)
                found += 1
            }
            // blocks without a marker: the note before goes on (beside it or under it), or the last note of
            // the page before does (the first block of this page's foot, before any note of its own)
            for u in unmarked {
                let r = u.para.bbox
                if let n = notes.last(where: { $0.bbox.y0 <= r.y0 + u.para.size && overlapX($0.bbox, r) || min($0.bbox.y1, r.y1) - max($0.bbox.y0, r.y0) > 0.5 * u.para.size }),
                   r.y0 - n.bbox.y1 < 1.5 * u.para.size {
                    n.lines = (n.lines + u.para.lines).sorted { a, b in
                        abs(a.bbox.y0 - b.bbox.y0) < 0.6 * a.size ? a.bbox.x0 < b.bbox.x0 : a.bbox.y0 < b.bbox.y0
                    }
                    continue
                }
                if !u.marked, let last = lastNote, lastNotePage == pno - 1, !notes.contains(where: { $0.bbox.y0 < r.y0 }),
                   abs(last.size - u.para.size) < 0.5,
                   !last.text.trimmingCharacters(in: .whitespaces).matches(#"[.!?)”"]$"#) || u.para.text.first?.isLowercase == true {
                    last.lines += u.para.lines
                    continued += 1
                    lastNotePage = pno
                    continue
                }
                out[u.at, default: []].append(u.para)
            }
            for i in foot { replace[i] = (out[i] ?? []).sorted { $0.lines[0].bbox.y0 < $1.lines[0].bbox.y0 } }
            // (the page's lowest note is the one a block on the next page may continue)
            if let n = notes.max(by: { $0.bbox.y1 < $1.bbox.y1 }) { lastNote = n; lastNotePage = pno }
        }
        guard found > 0 else { return input }
        log("Footnotes: \(found) found, \(linked) linked to their marker\(continued > 0 ? ", \(continued) continued on the next page" : "")")
        var result: [Para] = []
        for (i, p) in input.enumerated() {
            if let r = replace[i] { result += r } else { result.append(p) }
        }
        return result
    }

    // MARK: - margin notes

    /// A book page's margin notes: blocks of small type outside the text column (the column taken
    /// from the page's wide lines). Returns the page's other lines and the notes.
    func marginNotes(_ pno: Int, _ lines: [PLine]) -> ([PLine], [Para]) {
        let body = bodySize, W = boxes[pno].width
        // (the column's lines: wide ones, in body type or the smaller type of a chapter's notes)
        let bodyLines = lines.filter { $0.size > body - 1.6 && $0.bbox.w > 0.4 * W }
        guard bodyLines.count >= 4 else { return (lines, []) }
        let x0s = bodyLines.map(\.bbox.x0).sorted(), x1s = bodyLines.map(\.bbox.x1).sorted()
        let colX0 = x0s[x0s.count / 5], colX1 = x1s[x1s.count * 4 / 5]
        // a margin line the PDF ran into a mark of the formula beside it ("Argmin function ˆ"): the
        // note without the mark (the formula's crop keeps it)
        for l in lines where l.size > 0 && l.size <= body - 1 && l.bbox.x0 < colX0 - 10 && l.bbox.x1 > colX0 + 1 {
            guard let r = l.text.range(of: #"\s+[^\p{Lu}\p{Ll}\p{Lo}\p{N}\s]{1,3}$"#, options: .regularExpression),
                  l.text[..<r.lowerBound].contains(where: \.isLetter) else { continue }
            let tail = l.text[r].trimmingCharacters(in: .whitespaces)
            l.text = String(l.text[..<r.lowerBound])
            if let k = l.spans.lastIndex(where: { $0.text.contains(tail) }),
               let rr = l.spans[k].text.range(of: tail, options: .backwards) {
                l.spans[k].text.removeSubrange(rr)
                l.spans[k].text = String(l.spans[k].text.reversed().drop { $0 == " " }.reversed())
                if l.spans[k].text.isEmpty { l.spans.remove(at: k) }
            }
            l.bbox.x1 = min(l.bbox.x1, colX0 - 2)
        }
        func outside(_ l: PLine) -> Int {   // -1 left, 1 right, 0 in the column
            guard l.size > 0, l.size <= body - 1, l.bbox.w < 0.3 * W else { return 0 }
            if l.bbox.x0 >= colX1 - 1 { return 1 }
            if l.bbox.x1 <= colX0 + 1 { return -1 }
            return 0
        }
        var blocks: [(side: Int, lines: [PLine])] = []
        for l in lines.sorted(by: { $0.bbox.y0 < $1.bbox.y0 }) {
            let side = outside(l)
            guard side != 0 else { continue }
            if let k = blocks.lastIndex(where: { $0.side == side }), let last = blocks[k].lines.last,
               l.bbox.y0 - last.bbox.y1 < 1.0 * l.size, min(l.bbox.x1, last.bbox.x1) > max(l.bbox.x0, last.bbox.x0) - 4 {
                blocks[k].lines.append(l)
            } else {
                blocks.append((side, [l]))
            }
        }
        var taken = Set<ObjectIdentifier>()
        var notes: [Para] = []
        for b in blocks {
            let t = b.lines.map(\.text).joined(separator: " ").trimmingCharacters(in: .whitespaces)
            // (a caption set in the margin is a caption; a page number or a lone letter is nothing)
            if Self.captionRE.test(t) || b.lines.count > 10 || t.count > 500 || t.count < 3 || t.matches(#"^[\d\s.,–-]+$"#) { continue }
            for l in b.lines { taken.insert(ObjectIdentifier(l)) }
            let p = Para(b.lines, page: pno, kind: .sidenote)
            notes.append(p)
        }
        return (lines.filter { !taken.contains(ObjectIdentifier($0)) }, notes)
    }

    /// Puts each margin note after the paragraph level with it (its first line within the paragraph's
    /// height, else the nearest one above it on the page, else the page's first).
    static func placeSidenotes(_ paras: [Para], _ notes: [Para]) -> [Para] {
        guard !notes.isEmpty else { return paras }
        var after: [ObjectIdentifier: [Para]] = [:]
        var first: [Int: [Para]] = [:]
        for n in notes {
            let y = n.bbox.y0
            let onPage = paras.filter { $0.page == n.page && [.text, .heading, .abstract, .equation, .caption, .figure, .table].contains($0.kind) }
            let level = onPage.first { $0.bbox.y0 - 2 <= y && y <= $0.bbox.y1 + 2 && $0.kind != .figure && $0.kind != .table }
            let above = onPage.filter { $0.bbox.y1 <= y + 2 }.max { $0.bbox.y1 < $1.bbox.y1 }
            if let t = level ?? above { after[ObjectIdentifier(t), default: []].append(n) } else { first[n.page, default: []].append(n) }
        }
        var out: [Para] = []
        var seen = Set<Int>()
        for p in paras {
            if !seen.contains(p.page) { seen.insert(p.page); out += first[p.page] ?? [] }
            out.append(p)
            out += after[ObjectIdentifier(p)] ?? []
        }
        for (pg, ns) in first where !seen.contains(pg) { out += ns }
        return out
    }
}
