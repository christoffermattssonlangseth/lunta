import Foundation

/// The text a reading view shows, as Lunta's reader sees it, and finding highlights in it.
///
/// Lunta's reader shows an EPUB as one continuous page (core/src/epub.rs `combine`: every
/// section's <body> after the cover, each preceded by an empty `section-start` div) and anchors a
/// highlight by its quote, the text just before and after it, and its offset into the text of that
/// page's <body> (Shared/Highlights.swift `locate`, in JavaScript). `HighlightLocator` is that
/// `locate` in Swift, over the same UTF-16 text, so the app can tell before replacing a reading
/// view whether every highlight will still be found in the new one.
///
/// Rust parity: core/src/highlight.rs has `dom_text` (same as `ReaderText.domText`); its `locate`
/// is a different (quote-by-agent) search, so this exact/squash/dense/prefix-suffix ladder would
/// need porting there for desktop / Android.
public enum ReaderText {
    /// Text of the <body> of an XHTML document: every text node in order (script and style left
    /// out), entities decoded — what the reader's `textNodes()` concatenates.
    public static func domText(_ xhtml: String) -> String {
        var body = Substring(xhtml)
        if let b = xhtml.range(of: "<body"), let gt = xhtml[b.upperBound...].firstIndex(of: ">") {
            body = xhtml[xhtml.index(after: gt)...]
        }
        if let e = body.range(of: "</body>", options: .backwards) { body = body[..<e.lowerBound] }
        return textOf(body)
    }

    /// Text of an XHTML fragment (tags dropped, script/style skipped, entities decoded). Works on
    /// unicode scalars: as Characters, a combining mark right after ">" would merge with it.
    static func textOf(_ str: Substring) -> String {
        let s = Array(str.unicodeScalars)
        let n = s.count
        var out = String.UnicodeScalarView()
        func starts(_ i: Int, _ p: String) -> Bool {
            var k = i
            for c in p.unicodeScalars { guard k < n, s[k] == c else { return false }; k += 1 }
            return true
        }
        func find(_ p: String, from: Int, caseInsensitive: Bool = false) -> Int? {
            let pat = Array(p.unicodeScalars)
            var i = from
            while i + pat.count <= n {
                var ok = true
                for (o, c) in pat.enumerated() {
                    let x = s[i + o]
                    if caseInsensitive ? (x.properties.lowercaseMapping != c.properties.lowercaseMapping) : (x != c) { ok = false; break }
                }
                if ok { return i }
                i += 1
            }
            return nil
        }
        var i = 0
        while i < n {
            if s[i] != "<" {
                var j = i
                while j < n, s[j] != "<" { j += 1 }
                var chunk = String.UnicodeScalarView(); chunk.append(contentsOf: s[i..<j])
                out.append(contentsOf: HTMLDOM.decode(String(chunk)).unicodeScalars)
                i = j
                continue
            }
            if starts(i, "<!--") {
                i = find("-->", from: i + 4).map { $0 + 3 } ?? n
            } else if starts(i, "<![CDATA[") {
                let e = find("]]>", from: i + 9) ?? n
                out.append(contentsOf: s[(i + 9)..<e])
                i = min(n, e + 3)
            } else if starts(i, "<?") || starts(i, "<!") {
                var j = i
                while j < n, s[j] != ">" { j += 1 }
                i = min(n, j + 1)
            } else {
                var k = i + 1
                var q: Unicode.Scalar?
                while k < n {
                    let c = s[k]
                    if let qq = q { if c == qq { q = nil } } else if c == "\"" || c == "'" { q = c } else if c == ">" { break }
                    k += 1
                }
                let tagEnd = min(n, k + 1)
                var name = ""
                var j = i + 1
                while j < tagEnd, s[j].properties.isAlphabetic || ("0"..."9").contains(s[j]) || s[j] == ":" || s[j] == "-" { name.unicodeScalars.append(s[j]); j += 1 }
                name = name.lowercased()
                let selfClosing = tagEnd >= 2 && s[tagEnd - 2] == "/"
                i = tagEnd
                if (name == "script" || name == "style"), !selfClosing {
                    if let c = find("</\(name)", from: i, caseInsensitive: true) {
                        var e = c
                        while e < n, s[e] != ">" { e += 1 }
                        i = min(n, e + 1)
                    } else { i = n }
                }
            }
        }
        return String(out)
    }

    /// The body text of the continuous page the reader builds from these EPUB sections (title page
    /// first, cover left out), as core/src/epub.rs `combine` lays it out.
    static func combined(_ sectionBodies: [String]) -> String {
        var t = "\n"
        for b in sectionBodies { t += "\n" + textOf(Substring(b)) + "\n" }
        return t
    }
}

/// A highlight's anchor: what `locate` needs.
public struct HighlightAnchor: Sendable {
    public var quote: String
    public var prefix: String
    public var suffix: String
    public var start: Int
    public init(quote: String, prefix: String, suffix: String, start: Int) {
        self.quote = quote; self.prefix = prefix; self.suffix = suffix; self.start = start
    }
}

/// Swift port of the reader's `locate` (apple/Shared/Highlights.swift): exact quote (best by
/// prefix / suffix / closeness to `start`), then ignoring whitespace runs, then "dense" (case,
/// dashes, quote marks, µ folded; whitespace and quote marks left out), then the longest
/// surviving start and end of the passage close together. Offsets are UTF-16, like JavaScript's.
public final class HighlightLocator {
    private let text: [UInt16]
    private lazy var flat: (chars: [UInt16], map: [Int]) = {
        var map: [Int] = [], out: [UInt16] = []
        map.reserveCapacity(text.count); out.reserveCapacity(text.count)
        for (k, c0) in text.enumerated() {
            var c = c0
            if Self.isSpace(c) { if let l = out.last, l == 0x20 { continue }; c = 0x20 }
            map.append(k); out.append(c)
        }
        return (out, map)
    }()
    private lazy var dense: (chars: [UInt16], map: [Int]) = {
        var map: [Int] = [], out: [UInt16] = []
        for (k, c) in text.enumerated() where !Self.skip(c) { map.append(k); out.append(Self.fold(c)) }
        return (out, map)
    }()

    public init(text: String) { self.text = Array(text.utf16) }

    /// The UTF-16 range the highlight would cover, or nil if the reader wouldn't find it.
    public func locate(_ a: HighlightAnchor) -> Range<Int>? {
        let quote = Array(a.quote.utf16)
        if a.quote.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { return nil }
        let prefix = Array(a.prefix.utf16), suffix = Array(a.suffix.utf16)
        var best = -1
        var bestScore = -Double.infinity
        var i = Self.indexOf(text, quote, from: 0)
        while i >= 0 {
            var score = 0.0
            if !prefix.isEmpty, Array(text[max(0, i - prefix.count)..<i]) == prefix { score += 2 }
            if !suffix.isEmpty {
                let s = i + quote.count, e = min(text.count, s + suffix.count)
                if Array(text[s..<e]) == suffix { score += 2 }
            }
            score -= min(1, Double(abs(i - a.start)) / 100000)
            if score > bestScore { bestScore = score; best = i }
            i = Self.indexOf(text, quote, from: i + 1)
        }
        if best >= 0 { return best..<(best + quote.count) }

        // whitespace may differ between devices: match ignoring it
        var q: [UInt16] = []
        for c in quote {
            if Self.isSpace(c) { if let l = q.last, l == 0x20 { continue }; q.append(0x20) } else { q.append(c) }
        }
        while let f = q.first, f == 0x20 { q.removeFirst() }
        while let l = q.last, l == 0x20 { q.removeLast() }
        if q.count < 4 { return nil }
        let j = Self.indexOf(flat.chars, q, from: 0)
        if j >= 0 { return flat.map[j]..<(flat.map[j + q.count - 1] + 1) }

        // dense: spacing, dashes, quote marks and case left out / folded
        var dq = quote.filter { !Self.skip($0) }.map(Self.fold)
        if dq.count < 4 { return nil }
        let dn = dense.chars, dmap = dense.map
        var d = Self.indexOf(dn, dq, from: 0)
        let whole = dq
        if d < 0 && dq.count > 140 { dq = Array(dq.prefix(100)); d = Self.indexOf(dn, dq, from: 0) }
        if d < 0 {
            dq = whole
            var pre = -1, preLen = 0, suf = -1, sufLen = 0
            var L = min(dq.count - 1, 120)
            while L >= 30 { let x = Self.indexOf(dn, Array(dq.prefix(L)), from: 0); if x >= 0 { pre = x; preLen = L; break }; L -= 5 }
            if pre < 0 { return nil }
            var L2 = min(dq.count - 1, 120)
            while L2 >= 30 { let y = Self.indexOf(dn, Array(dq.suffix(L2)), from: pre + preLen); if y >= 0 { suf = y; sufLen = L2; break }; L2 -= 5 }
            if suf < 0 || suf + sufLen - pre > dq.count * 2 { return nil }
            return dmap[pre]..<(dmap[suf + sufLen - 1] + 1)
        }
        return dmap[d]..<(dmap[d + dq.count - 1] + 1)
    }

    /// JavaScript `/\s/` (ECMAScript white space and line terminators).
    static func isSpace(_ c: UInt16) -> Bool {
        switch c {
        case 0x09...0x0D, 0x20, 0xA0, 0x1680, 0x2000...0x200A, 0x2028, 0x2029, 0x202F, 0x205F, 0x3000, 0xFEFF: return true
        default: return false
        }
    }

    /// The reader's `skip`: whitespace and quote marks.
    static func skip(_ c: UInt16) -> Bool {
        if isSpace(c) { return true }
        switch c {
        case 0x27, 0x22, 0x2018, 0x2019, 0x201A, 0x201B, 0x201C, 0x201D, 0x201E, 0x201F, 0xB4, 0x60, 0xAB, 0xBB: return true
        default: return false
        }
    }

    /// The reader's `foldc`.
    static func fold(_ c: UInt16) -> UInt16 {
        if c == 0xB5 { return 0x3BC }
        if c == 0x2018 || c == 0x2019 { return 0x27 }
        if c == 0x201C || c == 0x201D { return 0x22 }
        if (0x2010...0x2014).contains(c) || c == 0x2212 { return 0x2D }
        if c < 0x80 { return (c >= 0x41 && c <= 0x5A) ? c + 32 : c }
        if (0xD800...0xDFFF).contains(c) { return c }
        guard let u = Unicode.Scalar(c) else { return c }
        let l = Array(String(Character(u)).lowercased().utf16)
        return l.count == 1 ? l[0] : c
    }

    /// `indexOf` on UTF-16 arrays (first-unit scan, then compare).
    static func indexOf(_ hay: [UInt16], _ needle: [UInt16], from: Int) -> Int {
        let n = needle.count, h = hay.count
        guard n > 0 else { return from <= h ? from : -1 }
        guard h >= n, from <= h - n else { return -1 }
        let first = needle[0]
        return hay.withUnsafeBufferPointer { hp in
            needle.withUnsafeBufferPointer { np in
                var i = max(0, from)
                let last = h - n
                while i <= last {
                    if hp[i] == first {
                        var k = 1
                        while k < n && hp[i + k] == np[k] { k += 1 }
                        if k == n { return i }
                    }
                    i += 1
                }
                return -1
            }
        }
    }
}
