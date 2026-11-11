import Foundation

/// A small DOM built on Foundation's `XMLParser` (iOS has no `XMLDocument`).
///
/// Element names and attribute names are stored without namespace prefixes
/// (`xlink:href` becomes `href`). Elements inside a MathML `<math>` subtree are
/// flagged with `isMath` so they can be serialised back out verbatim.
public final class XNode {
    public let name: String            // local name; "#text" for text nodes
    public var attrs: [String: String]
    public var attrOrder: [String]
    public var children: [XNode] = []
    public var text: String            // text nodes only
    public let isMath: Bool
    public weak var parent: XNode?

    init(name: String, attrs: [String: String] = [:], attrOrder: [String] = [], text: String = "", isMath: Bool = false) {
        self.name = name
        self.attrs = attrs
        self.attrOrder = attrOrder
        self.text = text
        self.isMath = isMath
    }

    public var isText: Bool { name == "#text" }
    public var elements: [XNode] { children.filter { !$0.isText } }

    public func attr(_ n: String) -> String? { attrs[n] }

    public func child(_ n: String) -> XNode? { children.first { $0.name == n } }
    public func children(_ n: String) -> [XNode] { children.filter { $0.name == n } }

    /// Pre-order descendants with the given name (not including self).
    public func descendants(_ n: String) -> [XNode] {
        var out: [XNode] = []
        func rec(_ x: XNode) {
            for c in x.children where !c.isText {
                if c.name == n { out.append(c) }
                rec(c)
            }
        }
        rec(self)
        return out
    }

    public func firstDescendant(_ n: String) -> XNode? {
        for c in children where !c.isText {
            if c.name == n { return c }
            if let d = c.firstDescendant(n) { return d }
        }
        return nil
    }

    /// Follows a simple slash path of child names, e.g. "caption/title".
    public func path(_ p: String) -> XNode? {
        var cur: XNode? = self
        for part in p.split(separator: "/") { cur = cur?.child(String(part)) }
        return cur
    }

    /// All text in this subtree, concatenated.
    public var allText: String {
        if isText { return text }
        return children.map(\.allText).joined()
    }

    /// Text with whitespace collapsed.
    public var plain: String {
        allText.split(whereSeparator: { $0.isWhitespace }).joined(separator: " ")
    }

    public func childText(_ n: String) -> String? { child(n)?.allText }
}

public enum XMLError: Error, LocalizedError {
    case parse(String)
    public var errorDescription: String? {
        switch self { case .parse(let m): return "XML parse error: \(m)" }
    }
}

public enum XML {
    /// Parses XML data into a DOM and returns the root element.
    public static func parse(_ data: Data) throws -> XNode {
        let prepared = prepare(data)
        let p = XMLParser(data: prepared)
        p.shouldProcessNamespaces = false
        p.shouldResolveExternalEntities = false
        let b = Builder()
        p.delegate = b
        if !p.parse() || b.root == nil {
            throw XMLError.parse(p.parserError?.localizedDescription ?? "unknown error")
        }
        return b.root!
    }

    /// Removes the DOCTYPE (we never load DTDs) and turns HTML-style named
    /// entities into numeric references, which `XMLParser` would otherwise reject.
    static func prepare(_ data: Data) -> Data {
        guard var s = String(data: data, encoding: .utf8) ?? String(data: data, encoding: .isoLatin1) else { return data }
        if let r = s.range(of: #"<!DOCTYPE[^>\[]*(\[[\s\S]*?\])?\s*>"#, options: .regularExpression) {
            s.removeSubrange(r)
        }
        if s.range(of: #"&[A-Za-z][A-Za-z0-9]*;"#, options: .regularExpression) != nil {
            let re = try! NSRegularExpression(pattern: #"&([A-Za-z][A-Za-z0-9]*);"#)
            let ns = s as NSString
            var out = ""
            var last = 0
            for m in re.matches(in: s, range: NSRange(location: 0, length: ns.length)) {
                out += ns.substring(with: NSRange(location: last, length: m.range.location - last))
                let name = ns.substring(with: m.range(at: 1))
                if ["amp", "lt", "gt", "quot", "apos"].contains(name) {
                    out += "&\(name);"
                } else if let cp = namedEntities[name] {
                    out += "&#\(cp);"
                } else {
                    out += "&amp;\(name);"
                }
                last = m.range.location + m.range.length
            }
            out += ns.substring(from: last)
            s = out
        }
        return Data(s.utf8)
    }

    static let namedEntities: [String: Int] = [
        "nbsp": 160, "ensp": 8194, "emsp": 8195, "thinsp": 8201, "hairsp": 8202, "zwnj": 8204, "zwj": 8205,
        "ndash": 8211, "mdash": 8212, "hellip": 8230, "lsquo": 8216, "rsquo": 8217, "ldquo": 8220, "rdquo": 8221,
        "bull": 8226, "middot": 183, "times": 215, "divide": 247, "minus": 8722, "plusmn": 177, "deg": 176,
        "micro": 181, "prime": 8242, "Prime": 8243, "le": 8804, "ge": 8805, "ne": 8800, "asymp": 8776,
        "infin": 8734, "rarr": 8594, "larr": 8592, "harr": 8596, "uarr": 8593, "darr": 8595, "sim": 8764,
        "copy": 169, "reg": 174, "trade": 8482, "sect": 167, "para": 182, "dagger": 8224, "Dagger": 8225,
        "alpha": 945, "beta": 946, "gamma": 947, "delta": 948, "epsilon": 949, "zeta": 950, "eta": 951,
        "theta": 952, "iota": 953, "kappa": 954, "lambda": 955, "mu": 956, "nu": 957, "xi": 958, "pi": 960,
        "rho": 961, "sigma": 963, "tau": 964, "upsilon": 965, "phi": 966, "chi": 967, "psi": 968, "omega": 969,
        "Gamma": 915, "Delta": 916, "Theta": 920, "Lambda": 923, "Xi": 926, "Pi": 928, "Sigma": 931,
        "Phi": 934, "Psi": 936, "Omega": 937, "auml": 228, "ouml": 246, "uuml": 252, "Auml": 196, "Ouml": 214,
        "Uuml": 220, "aring": 229, "Aring": 197, "eacute": 233, "egrave": 232, "aacute": 225, "iacute": 237,
        "oacute": 243, "uacute": 250, "ccedil": 231, "ntilde": 241, "szlig": 223, "oslash": 248, "aelig": 230,
    ]

    private final class Builder: NSObject, XMLParserDelegate {
        var root: XNode?
        var stack: [XNode] = []

        func parser(_ parser: XMLParser, didStartElement elementName: String, namespaceURI: String?,
                    qualifiedName qName: String?, attributes attributeDict: [String: String] = [:]) {
            let local = Self.local(elementName)
            let parentMath = stack.last?.isMath ?? false
            let isMath = parentMath || local == "math"
            var attrs: [String: String] = [:]
            var order: [String] = []
            for (k, v) in attributeDict.sorted(by: { $0.key < $1.key }) {
                if k.hasPrefix("xmlns") { continue }
                if isMath && k.contains(":") { continue }   // drop namespaced attrs inside MathML
                let lk = Self.local(k)
                attrs[lk] = v
                order.append(lk)
            }
            let n = XNode(name: local, attrs: attrs, attrOrder: order, isMath: isMath)
            if let top = stack.last {
                n.parent = top
                top.children.append(n)
            } else {
                root = n
            }
            stack.append(n)
        }

        func parser(_ parser: XMLParser, didEndElement elementName: String, namespaceURI: String?, qualifiedName qName: String?) {
            _ = stack.popLast()
        }

        func parser(_ parser: XMLParser, foundCharacters string: String) { addText(string) }

        func parser(_ parser: XMLParser, foundCDATA CDATABlock: Data) {
            addText(String(data: CDATABlock, encoding: .utf8) ?? "")
        }

        private func addText(_ s: String) {
            guard let top = stack.last else { return }
            if let last = top.children.last, last.isText {
                last.text += s
            } else {
                let t = XNode(name: "#text", text: s, isMath: top.isMath)
                t.parent = top
                top.children.append(t)
            }
        }

        static func local(_ n: String) -> String {
            if let i = n.lastIndex(of: ":") { return String(n[n.index(after: i)...]) }
            return n
        }
    }
}

// MARK: - Escaping

@inline(__always)
func esc(_ s: String) -> String {
    var out = ""
    out.reserveCapacity(s.utf8.count)
    for ch in s.unicodeScalars {
        switch ch {
        case "&": out += "&amp;"
        case "<": out += "&lt;"
        case ">": out += "&gt;"
        case "\"": out += "&quot;"
        default:
            // drop characters that are illegal in XML 1.0
            let v = ch.value
            if v == 0x9 || v == 0xA || v == 0xD || (v >= 0x20 && v <= 0xD7FF) || (v >= 0xE000 && v <= 0xFFFD) || v >= 0x10000 {
                out.unicodeScalars.append(ch)
            }
        }
    }
    return out
}
