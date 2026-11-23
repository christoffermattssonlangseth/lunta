import Foundation

// The citation add-ins' HTTP API, served by the Mac app on https://localhost:47481 (CitationServer
// owns the socket and TLS; everything that decides what to answer is here, so tests can drive it
// with a scratch library). Not generated: kept next to the bindings so the package's tests reach it.
//
//   GET  /api/status                         → {ok, papers, api}
//   GET  /api/search?q=…[&limit=n]           → [item]   (item: id, title, authors, families, year, journal, doi, meta)
//   POST /api/bibliography {style, ids, papers?}
//        → {entries: [item + entry]}; an id the library lacks is formatted from `papers[id]` (the
//          details the document keeps) as {fromDocument: true}, or found again by DOI or title
//          ({libraryId}), or else {missing: true}
//   GET  /api/styles?q=…                     → journal styles in the CSL catalogue [{id, title, format}]
//   POST /api/render {style, citations, papers?, options?}
//        → a journal style: {title, citations: [{text, superscript}], bibliography: [{id, text}],
//          items: [item], missing: [id]}; `options` (api 4), one per citation (null: none):
//          {id: {locator, label, prefix, suffix, mode}} (mode "", "suppress" or "narrative")
//   Resolve Citations (api 3; core/src/placeholders):
//   POST /api/placeholders {texts: [text], latex?}
//        → {placeholders: [[{start, end, text, kind, cites: [cite]}]]} (UTF-16 offsets; cite: text,
//          surnames, etAl, year, hints, key)
//   POST /api/suggest {cite} | {query}, external? (default true)
//        → {candidates: [item + {source, confidence, exact, inLibrary, citeKey}], exact, note}; a
//          candidate from outside the library has id "" and its details in meta
//   POST /api/add {meta} → item + {added}: a paper the writer confirmed, added (or the library's)
//   Converting citations from Zotero, EndNote and Mendeley (api 4):
//   POST /api/match {items: [meta]} → {matches: [{id, how} | null]}: the library's paper for each
//        (by DOI, PubMed id, title, or first author, year and nearly the same title); nothing added
//   Check Citations (api 5; core/src/claims): does the cited paper say what the sentence says?
//   POST /api/claims {text, start, end, others?, before?} | {claim, context?}, ids, papers?, judge?
//        → {claim, sentence, context, judged, note, privacy, results: [{id, libraryId?, title, label,
//          source, verdict, verdictLabel, byModel, reason, note, evidence: [{quote, section, link}],
//          passages: [{text, section, score, link}]}]}; the on-device model (`judge`) when there is
//          one, else the passages with their word overlap. Nothing leaves the Mac.
//   POST /api/open {id, find?} → {ok}: Lunta shows the paper, found at those words (the evidence)
//
// Who may ask: pages from this server, the hosted Word panel, and the Google Docs sidebar (served
// from *.googleusercontent.com, so it must also present the pairing key). Requests a web page
// makes from anywhere else are refused, also the ones a browser sends without an Origin. The
// browser extension asks through its own endpoint (ExtensionServer, /cite/… → `answerTrusted`).

/// One HTTP request, as read off the connection.
public struct CitationRequest: Sendable {
    public var method = ""
    public var path = "/"
    public var query: [String: String] = [:]
    /// Lower-cased names.
    public var headers: [String: String] = [:]
    public var body = Data()

    public init(method: String = "GET", path: String = "/", query: [String: String] = [:], headers: [String: String] = [:], body: Data = Data()) {
        self.method = method
        self.path = path
        self.query = query
        self.headers = headers
        self.body = body
    }
}

public struct CitationResponse: Sendable {
    public var status: Int
    public var body: Data
    public var contentType: String
    public var headers: [(String, String)] = []

    public init(status: Int, body: Data, contentType: String, headers: [(String, String)] = []) {
        self.status = status
        self.body = body
        self.contentType = contentType
        self.headers = headers
    }

    public static func json(_ status: Int, _ obj: Any) -> CitationResponse {
        CitationResponse(status: status, body: (try? JSONSerialization.data(withJSONObject: obj)) ?? Data("{}".utf8), contentType: "application/json")
    }

    public static func error(_ status: Int, _ message: String) -> CitationResponse {
        json(status, ["error": message])
    }

    /// The response as bytes on the wire (the connection is closed after it).
    public func encoded() -> Data {
        let reason = [200: "OK", 204: "No Content", 400: "Bad Request", 403: "Forbidden", 404: "Not Found", 413: "Payload Too Large",
                      421: "Misdirected Request", 502: "Bad Gateway", 503: "Service Unavailable"][status] ?? "Error"
        var h = "HTTP/1.1 \(status) \(reason)\r\nContent-Type: \(contentType)\r\nContent-Length: \(body.count)\r\n"
        h += "Connection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n"
        for (k, v) in headers { h += "\(k): \(v)\r\n" }
        h += "\r\n"
        var out = Data(h.utf8)
        out.append(body)
        return out
    }
}

public enum CitationHTTP {
    public enum Parsed: Sendable {
        /// More bytes are needed.
        case incomplete
        /// Not HTTP, or too large: answer 400 / 413 and close.
        case bad(Int)
        case request(CitationRequest)
    }

    public static let maxHeader = 64 << 10
    public static let maxBody = 8 << 20

    public static func parse(_ d: Data) -> Parsed {
        guard let end = d.range(of: Data("\r\n\r\n".utf8)) else {
            return d.count > maxHeader ? .bad(400) : .incomplete
        }
        var lines = String(decoding: d[d.startIndex..<end.lowerBound], as: UTF8.self).components(separatedBy: "\r\n")
        let first = lines.removeFirst().split(separator: " ")
        guard first.count >= 2, first[1].hasPrefix("/") else { return .bad(400) }
        var r = CitationRequest()
        r.method = String(first[0]).uppercased()
        let comps = URLComponents(string: "https://x\(first[1])")
        r.path = comps?.path ?? "/"
        for q in comps?.queryItems ?? [] { r.query[q.name] = q.value ?? "" }
        for l in lines {
            if let i = l.firstIndex(of: ":") {
                r.headers[l[..<i].trimmingCharacters(in: .whitespaces).lowercased()] = l[l.index(after: i)...].trimmingCharacters(in: .whitespaces)
            }
        }
        if r.headers["transfer-encoding"] != nil { return .bad(400) }   // fetch() sends Content-Length
        guard let len = Int(r.headers["content-length"] ?? "0"), len >= 0 else { return .bad(400) }
        if len > maxBody { return .bad(413) }
        let start = end.upperBound
        guard d.endIndex - start >= len else { return .incomplete }
        r.body = d.subdata(in: start..<(start + len))
        return .request(r)
    }
}

/// Answers the add-ins. Thread-safe (the core library locks itself); `handle` can block for
/// seconds the first time a journal style is fetched, so call it off the main thread.
public final class CitationAPI: @unchecked Sendable {
    public static let port: UInt16 = 47481
    /// Bumped when the API grows; the add-ins read it from /api/status.
    public static let version = 5
    /// Where the hosted Word panel lives (see .github/workflows/pages.yml).
    public static let hostedPanelOrigin = "https://getlunta.app"
    /// Where it was before the site moved (panels Word cached from there still ask).
    public static let oldHostedPanelOrigin = "https://christoffermattssonlangseth.github.io"
    static func isHostedPanel(_ origin: String) -> Bool { origin == hostedPanelOrigin || origin == oldHostedPanelOrigin }
    static let ownOrigins: Set<String> = ["https://localhost:\(port)", "https://127.0.0.1:\(port)"]

    private let library: Library?
    private let cacheDir: String
    private let token: String
    /// Called (off the main thread) with the id of a paper /api/add put in the library, so the app
    /// can show it and look for its PDF.
    public var paperAdded: (@Sendable (String) -> Void)?
    /// Check Citations: the on-device model's answer to (instructions, prompt), blocking; nil
    /// without a model (then the passages are shown for the writer to judge). Never a cloud model.
    public var judge: (@Sendable (String, String) -> String?)?
    /// Check Citations' "open the paper at this passage": (paper id, words to find, maybe "").
    public var openPaper: (@Sendable (String, String) -> Void)?

    /// `cacheDir`: where journal styles are kept. `token`: the Google Docs pairing key.
    public init(library: Library?, cacheDir: String, token: String) {
        self.library = library
        self.cacheDir = cacheDir
        self.token = token
    }

    // MARK: - Who may ask

    public static func isGoogleSidebar(_ origin: String) -> Bool {
        guard origin.hasPrefix("https://"), let host = URL(string: origin)?.host?.lowercased() else { return false }
        return host.hasSuffix(".googleusercontent.com")
    }

    /// Origins that are answered (CORS) at all; the Google sidebar still needs the key.
    public static func isKnownOrigin(_ origin: String) -> Bool {
        ownOrigins.contains(origin) || isHostedPanel(origin) || isGoogleSidebar(origin)
    }

    public func corsHeaders(_ r: CitationRequest) -> [(String, String)] {
        guard let origin = r.headers["origin"], Self.isKnownOrigin(origin), !Self.ownOrigins.contains(origin) else { return [] }
        return [("Access-Control-Allow-Origin", origin), ("Vary", "Origin"),
                ("Access-Control-Allow-Methods", "GET, POST, OPTIONS"),
                ("Access-Control-Allow-Headers", "Content-Type, X-Margins-Token, X-Lunta-Token"),
                ("Access-Control-Allow-Private-Network", "true"), ("Access-Control-Max-Age", "600")]
    }

    /// nil when the request may go on; otherwise the refusal to send.
    public func refusal(_ r: CitationRequest) -> CitationResponse? {
        // a page on another name that resolves to this Mac (DNS rebinding) never gets here with
        // a valid certificate, but don't rely on that alone
        if let host = r.headers["host"]?.lowercased(),
           !["localhost:\(Self.port)", "127.0.0.1:\(Self.port)", "localhost", "127.0.0.1"].contains(host) {
            return .error(421, "Wrong host.")
        }
        guard r.path.hasPrefix("/api/") else { return nil }   // the add-in files are public
        if r.method == "OPTIONS" { return nil }
        if let origin = r.headers["origin"] {
            if Self.ownOrigins.contains(origin) || Self.isHostedPanel(origin) { return nil }
            if Self.isGoogleSidebar(origin) {
                let sent = r.headers["x-lunta-token"] ?? r.headers["x-margins-token"] ?? ""
                if !token.isEmpty, Self.same(sent, token) { return nil }
                return .error(403, sent.isEmpty
                    ? "Not paired. Enter the pairing key shown in Lunta › Citations in Word & Google Docs…"
                    : "Wrong pairing key. Enter the key shown in Lunta › Citations in Word & Google Docs… (each Mac has its own).")
            }
            return .error(403, "This page may not use your Lunta library.")
        }
        // no Origin: the add-in's own GETs, or another app on this Mac (curl); a browser asking
        // for another site (<script src>, no-cors fetch) says so in Sec-Fetch-Site
        if let site = r.headers["sec-fetch-site"], site != "same-origin", site != "none" {
            return .error(403, "This page may not use your Lunta library.")
        }
        return nil
    }

    /// Compares without stopping at the first difference.
    static func same(_ a: String, _ b: String) -> Bool {
        let x = Array(a.utf8), y = Array(b.utf8)
        guard x.count == y.count else { return false }
        var d: UInt8 = 0
        for i in 0..<x.count { d |= x[i] ^ y[i] }
        return d == 0
    }

    // MARK: - Answers

    /// The whole answer to an /api/ request: refusal, preflight or the API, with CORS headers.
    public func respond(_ r: CitationRequest, paperCount: Int) -> CitationResponse {
        var out: CitationResponse
        if let no = refusal(r) {
            out = no
        } else if r.method == "OPTIONS" {
            out = CitationResponse(status: 204, body: Data(), contentType: "text/plain")
        } else {
            out = handle(r, paperCount: paperCount)
        }
        out.headers += corsHeaders(r)
        return out
    }

    /// The API's answer for a caller that has already decided who may ask: the browser extension's
    /// endpoint (127.0.0.1:47480, extension requests only) forwards its `/cite/…` here as `/api/…`,
    /// so the extension needs neither the certificate nor the pairing key. No CORS headers.
    public func answerTrusted(_ r: CitationRequest, paperCount: Int) -> CitationResponse {
        handle(r, paperCount: paperCount)
    }

    func handle(_ r: CitationRequest, paperCount: Int) -> CitationResponse {
        guard let lib = library else { return .error(503, "Lunta's library isn't open yet. Try again in a moment.") }
        switch (r.method, r.path) {
        case ("GET", "/api/status"):
            return .json(200, ["ok": true, "papers": paperCount, "api": Self.version])
        case ("GET", "/api/search"):
            let q = (r.query["q"] ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
            let limit = min(max(Int(r.query["limit"] ?? "") ?? 30, 1), 50)
            if q.isEmpty { return .json(200, [Any]()) }
            var papers: [Paper] = []
            if let p = lib.findByDoi(doi: q) { papers.append(p) }   // a pasted DOI
            for h in (try? lib.search(query: q, limit: UInt32(limit))) ?? [] where !papers.contains(where: { $0.id == h.paper.id }) {
                papers.append(h.paper)
            }
            return .json(200, papers.prefix(limit).map { Self.item($0) })
        case ("POST", "/api/bibliography"):
            guard let j = Self.object(r.body), let ids = j["ids"] as? [String], ids.count <= 10_000 else {
                return .error(400, "Expected {style, ids}.")
            }
            let key = ((j["style"] as? String) ?? "apa").trimmingCharacters(in: .whitespaces).lowercased()
            guard let style = citationStyles().first(where: { $0.key == key })?.style else {
                return .error(400, "Unknown citation style “\(key)”.")
            }
            let given = (j["papers"] as? [String: Any]) ?? [:]
            var entries: [[String: Any]] = []
            for id in Self.unique(ids) {
                if let p = lib.paper(id: id) {
                    var d = Self.item(p)
                    d["entry"] = lib.citation(id: id, style: style) ?? ""
                    entries.append(d)
                } else if let m = (given[id] as? [String: Any]).flatMap(Self.meta) {
                    if let p = lib.findCited(doi: m.doi, title: m.title) {
                        var d = Self.item(p)
                        d["id"] = id
                        d["libraryId"] = p.id
                        d["entry"] = lib.citation(id: p.id, style: style) ?? ""
                        entries.append(d)
                    } else {
                        var d = Self.item(id: id, meta: m)
                        d["fromDocument"] = true
                        d["entry"] = formatCitation(meta: m, style: style)
                        entries.append(d)
                    }
                } else {
                    entries.append(["id": id, "missing": true, "entry": ""])
                }
            }
            return .json(200, ["entries": entries])
        case ("GET", "/api/styles"):
            let q = r.query["q"] ?? ""
            do {
                let hits = try cslSearch(cacheDir: cacheDir, query: q, limit: 40)
                return .json(200, hits.map { ["id": $0.id, "title": $0.title, "format": $0.format] })
            } catch {
                return .error(502, "Couldn’t load the list of journal styles (\(Self.describe(error))). Check the internet connection; styles used before still work.")
            }
        case ("POST", "/api/render"):
            guard let j = Self.object(r.body), let style = j["style"] as? String, let cites = j["citations"] as? [[String]] else {
                return .error(400, "Expected {style, citations}.")
            }
            let given = (j["papers"] as? [String: Any]) ?? [:]
            var swap: [String: String] = [:]          // document id → the library's id
            var extra: [CitedPaper] = [], missing: [String] = [], items: [[String: Any]] = []
            for id in Self.unique(cites.flatMap { $0 }) {
                if let p = lib.paper(id: id) {
                    items.append(Self.item(p))
                } else if let m = (given[id] as? [String: Any]).flatMap(Self.meta) {
                    if let p = lib.findCited(doi: m.doi, title: m.title) {
                        swap[id] = p.id
                        var d = Self.item(p)
                        d["id"] = id
                        d["libraryId"] = p.id
                        items.append(d)
                    } else {
                        extra.append(CitedPaper(id: id, meta: m))
                        var d = Self.item(id: id, meta: m)
                        d["fromDocument"] = true
                        items.append(d)
                    }
                } else {
                    missing.append(id)
                }
            }
            let options = (j["options"] as? [Any]) ?? []
            let mapped = cites.enumerated().map { (n, list) in
                list.map { id in Self.citeItem(id: swap[id] ?? id, options: (options.indices.contains(n) ? options[n] as? [String: Any] : nil)?[id] as? [String: Any]) }
            }
            let back = Dictionary(swap.map { ($1, $0) }, uniquingKeysWith: { a, _ in a })
            do {
                let out = try lib.citeCslItems(cacheDir: cacheDir, styleId: style, citations: mapped, extra: extra)
                return .json(200, [
                    "title": out.styleTitle,
                    "citations": out.citations.map { ["text": $0.text, "superscript": $0.superscript] },
                    "bibliography": out.bibliography.map { ["id": back[$0.id] ?? $0.id, "text": $0.text] },
                    "items": items,
                    "missing": missing,
                ])
            } catch {
                return .error(502, "Couldn’t format in this journal style (\(Self.describe(error))). The first time a style is used it is downloaded: check the internet connection.")
            }
        case ("POST", "/api/placeholders"):
            guard let j = Self.object(r.body), let texts = j["texts"] as? [String], texts.count <= 100_000 else {
                return .error(400, "Expected {texts}.")
            }
            let latex = j["latex"] as? Bool ?? false
            let all = texts.map { t in (latex ? findTexPlaceholders(text: t) : findPlaceholders(text: t)).map(Self.placeholderJSON) }
            return .json(200, ["placeholders": all])
        case ("POST", "/api/suggest"):
            guard let j = Self.object(r.body) else { return .error(400, "Expected {cite} or {query}.") }
            let external = j["external"] as? Bool ?? true
            let found: CitationSuggestions
            if let c = j["cite"] as? [String: Any] {
                found = lib.suggestCitation(cite: Self.cite(c), external: external)
            } else if let q = j["query"] as? String {
                found = lib.searchCitation(query: String(q.prefix(500)), external: external)
            } else {
                return .error(400, "Expected {cite} or {query}.")
            }
            return .json(200, ["candidates": found.candidates.map(Self.candidateJSON), "exact": found.exact, "note": found.note])
        case ("POST", "/api/add"):
            guard let m = (Self.object(r.body)?["meta"] as? [String: Any]).flatMap(Self.meta) else {
                return .error(400, "Expected {meta}.")
            }
            let had = lib.findCited(doi: m.doi, title: m.title) != nil
            do {
                let p = try lib.addFoundPaper(meta: m)
                if !had { paperAdded?(p.id) }
                var d = Self.item(p)
                d["added"] = !had
                return .json(200, d)
            } catch {
                return .error(502, "Couldn’t add the paper (\(Self.describe(error))).")
            }
        case ("POST", "/api/match"):
            guard let list = Self.object(r.body)?["items"] as? [Any], list.count <= 5000 else {
                return .error(400, "Expected {items}.")
            }
            let metas = list.map { ($0 as? [String: Any]).flatMap(Self.meta) }
            let found = lib.matchCitedPapers(metas: metas.map { $0 ?? Self.emptyMeta })
            let matches: [Any] = zip(metas, found).map { m, f in
                m == nil || f.paperId.isEmpty ? NSNull() as Any : ["id": f.paperId, "how": f.how] as Any
            }
            return .json(200, ["matches": matches])
        case ("POST", "/api/claims"):
            let out = lib.claimsRequest(body: String(decoding: r.body, as: UTF8.self), judge: ModelJudge(judge))
            return CitationResponse(status: Int(out.status), body: Data(out.body.utf8), contentType: "application/json")
        case ("POST", "/api/open"):
            guard let id = Self.object(r.body)?["id"] as? String, lib.paper(id: id) != nil else {
                return .error(404, "That paper isn’t in your Lunta library.")
            }
            openPaper?(id, String(((Self.object(r.body)?["find"] as? String) ?? "").prefix(200)))
            return .json(200, ["ok": true])
        default:
            return .error(404, "Not found.")
        }
    }

    /// The app's model for the core's Check Citations.
    final class ModelJudge: ClaimJudge, @unchecked Sendable {
        let ask: (@Sendable (String, String) -> String?)?
        init(_ ask: (@Sendable (String, String) -> String?)?) { self.ask = ask }
        func answer(instructions: String, prompt: String, maxTokens: UInt32) -> String? { ask?(instructions, prompt) }
    }

    static let emptyMeta = PaperMeta(title: "", authors: [], date: "", journal: "", volume: "", issue: "", pages: "", publisher: "", doi: "",
                                     pmid: "", pmcid: "", arxiv: "", url: "", abstractText: "", keywords: [], license: "")

    /// One cited paper with the options a document keeps for it (`/api/render`'s `options`).
    static func citeItem(id: String, options o: [String: Any]?) -> CslCiteItem {
        let s = { (k: String, n: Int) in String(((o?[k] as? String) ?? "").prefix(n)) }
        let mode = s("mode", 20)
        return CslCiteItem(id: id, locator: s("locator", 200), label: s("label", 40), prefix: s("prefix", 500), suffix: s("suffix", 500),
                           mode: ["suppress", "narrative"].contains(mode) ? mode : "")
    }

    // MARK: - Resolve Citations

    static func placeholderJSON(_ p: Placeholder) -> [String: Any] {
        let kind: String
        switch p.kind {
        case .parentheses: kind = "parentheses"
        case .brackets: kind = "brackets"
        case .braces: kind = "braces"
        case .latex: kind = "latex"
        }
        return ["start": p.start, "end": p.end, "text": p.text, "kind": kind, "cites": p.cites.map {
            ["text": $0.text, "surnames": $0.surnames, "etAl": $0.etAl, "year": $0.year, "hints": $0.hints, "key": $0.key] as [String: Any]
        }]
    }

    /// A placeholder's cite as the panel sends it back.
    static func cite(_ c: [String: Any]) -> PlaceholderCite {
        let st = { (k: String) in String(((c[k] as? String) ?? "").prefix(500)) }
        let list = { (k: String) in ((c[k] as? [Any]) ?? []).compactMap { $0 as? String }.prefix(20).map { String($0.prefix(200)) } }
        return PlaceholderCite(text: st("text"), surnames: Array(list("surnames")), etAl: c["etAl"] as? Bool ?? false, year: st("year"),
                               hints: Array(list("hints")), key: st("key"))
    }

    static func candidateJSON(_ c: CitationCandidate) -> [String: Any] {
        var d = item(id: c.paperId, meta: c.meta)
        d["source"] = c.source
        d["confidence"] = (c.confidence * 100).rounded() / 100
        d["exact"] = c.exact
        d["inLibrary"] = !c.paperId.isEmpty
        d["citeKey"] = c.citeKey
        return d
    }

    // MARK: - Papers as JSON

    static func object(_ body: Data) -> [String: Any]? {
        (try? JSONSerialization.jsonObject(with: body)) as? [String: Any]
    }

    static func unique(_ ids: [String]) -> [String] {
        var seen = Set<String>()
        return ids.map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty && seen.insert($0).inserted }
    }

    /// What the add-ins get for a paper. `meta` is what a document keeps of the papers it cites
    /// (long author lists shortened to the first 20 and the last, as no style shows more).
    public static func item(_ p: Paper) -> [String: Any] {
        item(id: p.id, meta: p.meta)
    }

    static func item(id: String, meta m: PaperMeta) -> [String: Any] {
        let authors = shortList(m.authors)
        return ["id": id, "title": m.title, "authors": authors, "families": authors.map { familyName(name: $0) },
                "year": String(m.date.prefix(4)), "journal": m.journal, "doi": m.doi, "meta": metaJSON(m)]
    }

    /// The first 20 authors and the last: all any style shows (a consortium paper has thousands).
    static func shortList(_ a: [String]) -> [String] {
        a.count > 21 ? Array(a.prefix(20)) + [a.last!] : a
    }

    static func metaJSON(_ m: PaperMeta) -> [String: Any] {
        let cap = { (s: String) in String(s.prefix(2000)) }
        var d: [String: Any] = ["title": cap(m.title), "authors": shortList(m.authors).map(cap)]
        for (k, v) in [("date", m.date), ("journal", m.journal), ("volume", m.volume), ("issue", m.issue), ("pages", m.pages),
                       ("publisher", m.publisher), ("doi", m.doi), ("pmid", m.pmid), ("pmcid", m.pmcid), ("arxiv", m.arxiv), ("url", m.url)]
        where !v.isEmpty {
            d[k] = cap(v)
        }
        return d
    }

    /// A paper's details as a document sent them (nil without a title, authors, DOI or PubMed id to
    /// find it by: ReadCube Papers' citations may have only a DOI).
    static func meta(_ d: [String: Any]) -> PaperMeta? {
        let s = { (k: String) -> String in
            if let v = d[k] as? String { return String(v.prefix(2000)) }
            if let v = d[k] as? NSNumber { return v.stringValue }
            return ""
        }
        let authors = ((d["authors"] as? [Any]) ?? []).compactMap { $0 as? String }.prefix(200).map { String($0.prefix(500)) }
        let m = PaperMeta(title: s("title"), authors: Array(authors), date: s("date").isEmpty ? s("year") : s("date"), journal: s("journal"),
                          volume: s("volume"), issue: s("issue"), pages: s("pages"), publisher: s("publisher"), doi: s("doi"),
                          pmid: s("pmid"), pmcid: s("pmcid"), arxiv: s("arxiv"), url: s("url"), abstractText: "", keywords: [], license: "")
        return m.title.isEmpty && m.authors.isEmpty && m.doi.isEmpty && m.pmid.isEmpty ? nil : m
    }

    static func describe(_ e: Error) -> String {
        if let c = e as? CoreError {
            switch c {
            case .Io(let m), .Db(let m), .Network(let m), .NotFound(let m), .Invalid(let m): return m
            }
        }
        return e.localizedDescription
    }
}
