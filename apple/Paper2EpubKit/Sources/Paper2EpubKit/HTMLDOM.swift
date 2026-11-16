import Foundation

/// A forgiving HTML parser for publisher pages (which `XMLParser` rejects: unclosed `<p>` and
/// `<li>`, unquoted attributes, `<br>`, bare `&`). Builds the same `XNode` tree the XML
/// converters use. Not a full HTML5 tree builder; it handles what article pages need:
/// void elements, raw-text elements (script, style), implied end tags for p / li / dt / dd /
/// tr / td / th / option, end tags without a matching start (ignored) and entities.
/// MathML (`<math>` subtrees) is flagged `isMath`, as the XML parser does.
///
/// Rust port (desktop / Android): core/src/convert would use html5ever (or `scraper`) instead.
enum HTMLDOM {
    static let void: Set<String> = ["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param",
                                    "source", "track", "wbr", "keygen", "command"]
    static let rawText: Set<String> = ["script", "style", "textarea", "title", "noscript", "template", "iframe", "xmp"]
    /// Opening one of these closes an open `<p>` (as in HTML).
    static let closesP: Set<String> = ["address", "article", "aside", "blockquote", "details", "div", "dl", "fieldset",
                                       "figcaption", "figure", "footer", "form", "h1", "h2", "h3", "h4", "h5", "h6",
                                       "header", "hr", "main", "nav", "ol", "p", "pre", "section", "table", "ul", "menu"]

    /// Parses a whole page (or fragment); the root is a synthetic `#document` element.
    static func parse(_ html: String) -> XNode {
        let root = XNode(name: "#document")
        var stack: [XNode] = [root]
        let s = Array(html.utf16)
        let n = s.count
        var i = 0
        var textStart = 0

        func flushText(_ end: Int) {
            guard end > textStart else { return }
            let raw = String(utf16CodeUnits: Array(s[textStart..<end]), count: end - textStart)
            addText(decode(raw))
        }
        func addText(_ t: String) {
            guard !t.isEmpty, let top = stack.last else { return }
            if let last = top.children.last, last.isText { last.text += t; return }
            let x = XNode(name: "#text", text: t, isMath: top.isMath)
            x.parent = top
            top.children.append(x)
        }
        func inStack(_ name: String, stopAt: Set<String> = []) -> Int? {
            var k = stack.count - 1
            while k > 0 {
                if stack[k].name == name { return k }
                if stopAt.contains(stack[k].name) { return nil }
                k -= 1
            }
            return nil
        }
        func popTo(_ k: Int) { stack.removeSubrange(k...) }
        func open(_ name: String, _ attrs: [(String, String)], selfClosing: Bool) {
            let parentMath = stack.last?.isMath ?? false
            let isMath = parentMath || name == "math"
            let isForeign = isMath || name == "svg" || stack.contains { $0.name == "svg" }
            if !isForeign {
                // implied end tags
                if closesP.contains(name), let k = inStack("p", stopAt: ["div", "section", "article", "td", "th", "li", "blockquote", "figure", "table", "button"]) { popTo(k) }
                switch name {
                case "li":
                    if let k = inStack("li", stopAt: ["ul", "ol", "menu"]) { popTo(k) }
                case "dt", "dd":
                    if let k = inStack("dd", stopAt: ["dl"]) ?? inStack("dt", stopAt: ["dl"]) { popTo(k) }
                case "tr":
                    if let k = inStack("tr", stopAt: ["table", "thead", "tbody", "tfoot"]) { popTo(k) }
                case "td", "th":
                    if let k = inStack("td", stopAt: ["tr", "table"]) ?? inStack("th", stopAt: ["tr", "table"]) { popTo(k) }
                case "thead", "tbody", "tfoot":
                    if let k = inStack("thead", stopAt: ["table"]) ?? inStack("tbody", stopAt: ["table"]) ?? inStack("tfoot", stopAt: ["table"]) { popTo(k) }
                case "option":
                    if let k = inStack("option", stopAt: ["select", "datalist"]) { popTo(k) }
                default: break
                }
            }
            var dict: [String: String] = [:]
            var order: [String] = []
            for (k, v) in attrs {
                var key = k
                if let c = key.lastIndex(of: ":") {
                    if key.hasPrefix("xmlns") { continue }
                    if isMath { continue }
                    key = String(key[key.index(after: c)...])
                }
                if key == "xmlns" { continue }
                if dict[key] == nil { order.append(key) }
                dict[key] = v
            }
            let x = XNode(name: name, attrs: dict, attrOrder: order, isMath: isMath)
            let top = stack.last!
            x.parent = top
            top.children.append(x)
            if !(void.contains(name) && !isForeign) && !(selfClosing && isForeign) && !(selfClosing && void.contains(name)) {
                stack.append(x)
            }
        }
        func close(_ name: String) {
            if name == "p", inStack("p") == nil { return }   // a stray </p>
            if let k = inStack(name) { popTo(k) }
        }

        while i < n {
            guard s[i] == 0x3C /* < */ else { i += 1; continue }
            // comment
            if i + 3 < n, s[i + 1] == 0x21, s[i + 2] == 0x2D, s[i + 3] == 0x2D {
                flushText(i)
                var j = i + 4
                while j + 2 < n, !(s[j] == 0x2D && s[j + 1] == 0x2D && s[j + 2] == 0x3E) { j += 1 }
                i = min(n, j + 3); textStart = i
                continue
            }
            // CDATA
            if i + 8 < n, String(utf16CodeUnits: Array(s[i..<(i + 9)]), count: 9) == "<![CDATA[" {
                flushText(i)
                var j = i + 9
                while j + 2 < n, !(s[j] == 0x5D && s[j + 1] == 0x5D && s[j + 2] == 0x3E) { j += 1 }
                addText(String(utf16CodeUnits: Array(s[(i + 9)..<min(j, n)]), count: min(j, n) - (i + 9)))
                i = min(n, j + 3); textStart = i
                continue
            }
            // doctype / processing instruction
            if i + 1 < n, s[i + 1] == 0x21 || s[i + 1] == 0x3F {
                flushText(i)
                var j = i + 2
                while j < n, s[j] != 0x3E { j += 1 }
                i = min(n, j + 1); textStart = i
                continue
            }
            let isEnd = i + 1 < n && s[i + 1] == 0x2F
            let nameStart = i + (isEnd ? 2 : 1)
            guard nameStart < n, isLetter(s[nameStart]) else { i += 1; continue }   // a literal "<"
            flushText(i)
            var j = nameStart
            while j < n, !isSpace(s[j]), s[j] != 0x3E, s[j] != 0x2F { j += 1 }
            let name = String(utf16CodeUnits: Array(s[nameStart..<j]), count: j - nameStart).lowercased()
            // attributes
            var attrs: [(String, String)] = []
            var selfClosing = false
            while j < n {
                while j < n, isSpace(s[j]) { j += 1 }
                guard j < n else { break }
                if s[j] == 0x3E { j += 1; break }
                if s[j] == 0x2F { selfClosing = true; j += 1; continue }
                selfClosing = false
                let as0 = j
                while j < n, !isSpace(s[j]), s[j] != 0x3E, s[j] != 0x3D, !(s[j] == 0x2F && j + 1 < n && s[j + 1] == 0x3E) { j += 1 }
                if j == as0 { j += 1; continue }
                let an = String(utf16CodeUnits: Array(s[as0..<j]), count: j - as0).lowercased()
                while j < n, isSpace(s[j]) { j += 1 }
                var value = ""
                if j < n, s[j] == 0x3D {
                    j += 1
                    while j < n, isSpace(s[j]) { j += 1 }
                    if j < n, s[j] == 0x22 || s[j] == 0x27 {
                        let q = s[j]; j += 1
                        let vs = j
                        while j < n, s[j] != q { j += 1 }
                        value = String(utf16CodeUnits: Array(s[vs..<min(j, n)]), count: min(j, n) - vs)
                        j += 1
                    } else {
                        let vs = j
                        while j < n, !isSpace(s[j]), s[j] != 0x3E { j += 1 }
                        value = String(utf16CodeUnits: Array(s[vs..<j]), count: j - vs)
                    }
                }
                if !isEnd { attrs.append((an, decode(value))) }
            }
            i = j; textStart = i
            if isEnd { close(name); continue }
            open(name, attrs, selfClosing: selfClosing)
            // raw text: everything up to the matching end tag
            if rawText.contains(name), !selfClosing {
                let endTag = Array("</\(name)".utf16)
                var k = i
                while k < n {
                    if s[k] == 0x3C, k + endTag.count <= n {
                        var match = true
                        for (o, c) in endTag.enumerated() where lower(s[k + o]) != c { match = false; break }
                        if match { break }
                    }
                    k += 1
                }
                let raw = String(utf16CodeUnits: Array(s[i..<k]), count: k - i)
                if let top = stack.last, top.name == name {
                    let t = XNode(name: "#text", text: raw)
                    t.parent = top
                    top.children.append(t)
                    stack.removeLast()
                }
                var e = k
                while e < n, s[e] != 0x3E { e += 1 }
                i = min(n, e + 1); textStart = i
            }
        }
        flushText(n)
        return root
    }

    @inline(__always) static func isSpace(_ c: UInt16) -> Bool { c == 0x20 || c == 0x09 || c == 0x0A || c == 0x0D || c == 0x0C }
    @inline(__always) static func isLetter(_ c: UInt16) -> Bool { (c >= 0x41 && c <= 0x5A) || (c >= 0x61 && c <= 0x7A) }
    @inline(__always) static func lower(_ c: UInt16) -> UInt16 { (c >= 0x41 && c <= 0x5A) ? c + 32 : c }

    static let extraEntities: [String: Int] = [
        "apos": 39, "quot": 34, "amp": 38, "lt": 60, "gt": 62, "shy": 173, "laquo": 171, "raquo": 187, "sbquo": 8218,
        "bdquo": 8222, "lsaquo": 8249, "rsaquo": 8250, "euro": 8364, "pound": 163, "yen": 165, "cent": 162, "frac12": 189,
        "frac14": 188, "frac34": 190, "sup1": 185, "sup2": 178, "sup3": 179, "ordf": 170, "ordm": 186, "not": 172,
        "acute": 180, "cedil": 184, "uml": 168, "macr": 175, "iexcl": 161, "iquest": 191, "agrave": 224, "egrave": 232,
        "igrave": 236, "ograve": 242, "ugrave": 249, "acirc": 226, "ecirc": 234, "icirc": 238, "ocirc": 244, "ucirc": 251,
        "atilde": 227, "otilde": 245, "euml": 235, "iuml": 239, "yuml": 255, "Eacute": 201, "Ccedil": 199, "Oslash": 216,
        "AElig": 198, "oelig": 339, "OElig": 338, "scaron": 353, "Scaron": 352, "zcaron": 382, "Zcaron": 381,
        "isin": 8712, "notin": 8713, "sum": 8721, "prod": 8719, "radic": 8730, "prop": 8733, "part": 8706, "nabla": 8711,
        "forall": 8704, "exist": 8707, "empty": 8709, "cap": 8745, "cup": 8746, "sub": 8834, "sup": 8835, "and": 8743,
        "or": 8744, "equiv": 8801, "cong": 8773, "lowast": 8727, "sdot": 8901, "larr": 8592, "rArr": 8658, "lArr": 8656,
        "hArr": 8660, "varepsilon": 949, "thetasym": 977, "sigmaf": 962, "upsih": 978, "piv": 982, "Alpha": 913,
        "Beta": 914, "Epsilon": 917, "Zeta": 918, "Eta": 919, "Iota": 921, "Kappa": 922, "Mu": 924, "Nu": 925,
        "Omicron": 927, "omicron": 959, "Rho": 929, "Tau": 932, "Upsilon": 933, "Chi": 935, "zwsp": 8203, "lrm": 8206,
        "rlm": 8207, "NewLine": 10, "Tab": 9, "colon": 58, "comma": 44, "period": 46, "lpar": 40, "rpar": 41,
        "lsqb": 91, "rsqb": 93, "lbrack": 91, "rbrack": 93, "sol": 47, "bsol": 92, "ast": 42, "num": 35, "percnt": 37,
        "plus": 43, "equals": 61, "excl": 33, "quest": 63, "commat": 64, "dollar": 36, "lowbar": 95, "verbar": 124,
        "vert": 124, "lcub": 123, "rcub": 125, "Hat": 94, "grave": 96, "half": 189, "hyphen": 8208, "dash": 8208,
    ]

    /// Decodes character references (named, decimal, hex).
    static func decode(_ s: String) -> String {
        guard s.contains("&") else { return s }
        var out = ""
        out.reserveCapacity(s.utf8.count)
        var rest = Substring(s)
        while let a = rest.firstIndex(of: "&") {
            out += rest[..<a]
            let after = rest[rest.index(after: a)...]
            var end = after.startIndex
            var len = 0
            while end < after.endIndex, len < 32, after[end].isLetter || after[end].isNumber || after[end] == "#" { end = after.index(after: end); len += 1 }
            let name = String(after[..<end])
            var consumed = end
            if end < after.endIndex, after[end] == ";" { consumed = after.index(after: end) }
            var scalar: Unicode.Scalar?
            if name.hasPrefix("#x") || name.hasPrefix("#X") {
                scalar = UInt32(name.dropFirst(2), radix: 16).flatMap(Unicode.Scalar.init)
            } else if name.hasPrefix("#") {
                scalar = UInt32(name.dropFirst()).flatMap(Unicode.Scalar.init)
            } else if let v = XML.namedEntities[name] ?? extraEntities[name] {
                scalar = Unicode.Scalar(UInt32(v))
            }
            if let scalar, !name.isEmpty {
                out.unicodeScalars.append(scalar)
                rest = after[consumed...]
            } else {
                out += "&"
                rest = after
            }
        }
        out += rest
        return out
    }
}

extension XNode {
    var classes: [String] { (attr("class") ?? "").split(whereSeparator: { $0.isWhitespace }).map(String.init) }

    /// Class and id words split on `-` and `_` ("c-article-share-box" → c, article, share, box), lower-cased.
    var nameTokens: Set<String> {
        var out = Set<String>()
        for c in classes + [attr("id") ?? ""] {
            for t in c.lowercased().split(whereSeparator: { $0 == "-" || $0 == "_" }) { out.insert(String(t)) }
        }
        return out
    }

    /// Pre-order elements (not including self) for which `pred` holds.
    func all(where pred: (XNode) -> Bool) -> [XNode] {
        var out: [XNode] = []
        func rec(_ x: XNode) {
            for c in x.children where !c.isText {
                if pred(c) { out.append(c) }
                rec(c)
            }
        }
        rec(self)
        return out
    }

    func ancestors() -> [XNode] {
        var out: [XNode] = []
        var p = parent
        while let x = p { out.append(x); p = x.parent }
        return out
    }
}
