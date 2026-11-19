import XCTest
import Security
@testable import LuntaCore

/// The citation add-ins' API (what the Mac app's https://localhost:47481 answers), against a
/// scratch library in a temporary folder.
final class CitationAPITests: XCTestCase {
    var tmp: URL!
    var lib: Library!
    var api: CitationAPI!
    var cache: URL!

    override func setUpWithError() throws {
        tmp = FileManager.default.temporaryDirectory.appendingPathComponent("lunta-citations-\(UUID())")
        cache = tmp.appendingPathComponent("cache")
        try FileManager.default.createDirectory(at: cache.appendingPathComponent("csl"), withIntermediateDirectories: true)
        lib = try Library.open(root: tmp.appendingPathComponent("lib").path, cacheDir: tmp.appendingPathComponent("index").path, deviceId: "test-mac")
        api = CitationAPI(library: lib, cacheDir: cache.path, token: "secret123456")
    }

    override func tearDownWithError() throws {
        try? FileManager.default.removeItem(at: tmp)
    }

    func meta(_ title: String, _ authors: [String], _ date: String, doi: String = "") -> PaperMeta {
        PaperMeta(title: title, authors: authors, date: date, journal: "Cell", volume: "1", issue: "", pages: "1-2", publisher: "",
                  doi: doi, pmid: "", pmcid: "", arxiv: "", url: "", abstractText: "", keywords: [], license: "")
    }

    func request(_ method: String, _ path: String, json: Any? = nil, headers: [String: String] = [:]) -> CitationRequest {
        let comps = URLComponents(string: "https://x\(path)")!
        var q: [String: String] = [:]
        for i in comps.queryItems ?? [] { q[i.name] = i.value ?? "" }
        var h = ["host": "localhost:47481"]
        for (k, v) in headers { h[k.lowercased()] = v }
        return CitationRequest(method: method, path: comps.path, query: q, headers: h,
                               body: json.map { try! JSONSerialization.data(withJSONObject: $0) } ?? Data())
    }

    func send(_ r: CitationRequest) -> (Int, Any?, [String: String]) {
        let out = api.respond(r, paperCount: 7)
        var h: [String: String] = [:]
        for (k, v) in out.headers { h[k] = v }
        return (out.status, try? JSONSerialization.jsonObject(with: out.body), h)
    }

    // MARK: - who may ask

    func testOnlyTheAddInsMayAsk() {
        // the Word panel served by Lunta: no Origin on GET, its own on POST
        XCTAssertEqual(send(request("GET", "/api/status", headers: ["Sec-Fetch-Site": "same-origin"])).0, 200)
        XCTAssertEqual(send(request("GET", "/api/status", headers: ["Origin": "https://localhost:47481"])).0, 200)
        // the hosted Word panel
        let hosted = send(request("GET", "/api/status", headers: ["Origin": CitationAPI.hostedPanelOrigin]))
        XCTAssertEqual(hosted.0, 200)
        XCTAssertEqual(hosted.2["Access-Control-Allow-Origin"], CitationAPI.hostedPanelOrigin)
        // the Google Docs sidebar: only with the pairing key
        let g = "https://n-abc123-0lu-script.googleusercontent.com"
        let noKey = send(request("GET", "/api/search?q=x", headers: ["Origin": g]))
        XCTAssertEqual(noKey.0, 403)
        XCTAssertEqual(noKey.2["Access-Control-Allow-Origin"], g, "the sidebar must be able to read why")
        XCTAssertTrue(((noKey.1 as? [String: Any])?["error"] as? String ?? "").contains("pairing key"))
        XCTAssertEqual(send(request("GET", "/api/search?q=x", headers: ["Origin": g, "X-Margins-Token": "wrong"])).0, 403)
        XCTAssertEqual(send(request("GET", "/api/search?q=x", headers: ["Origin": g, "X-Margins-Token": "secret123456"])).0, 200)
        XCTAssertEqual(send(request("GET", "/api/search?q=x", headers: ["Origin": g, "X-Lunta-Token": "secret123456"])).0, 200)
        // any other web page: refused, and no CORS headers to read the answer with
        for origin in ["https://evil.example", "null", "https://googleusercontent.com.evil.example", "http://n-1.googleusercontent.com",
                       "https://localhost:47481.evil.example", "https://christoffermattssonlangseth.github.io.evil.example"] {
            let r = send(request("POST", "/api/bibliography", json: ["style": "apa", "ids": []], headers: ["Origin": origin]))
            XCTAssertEqual(r.0, 403, origin)
            XCTAssertNil(r.2["Access-Control-Allow-Origin"], origin)
        }
        // a page elsewhere using <script src> or a no-cors fetch (no Origin header)
        XCTAssertEqual(send(request("GET", "/api/search?q=a", headers: ["Sec-Fetch-Site": "cross-site"])).0, 403)
        XCTAssertEqual(send(request("GET", "/api/search?q=a", headers: ["Sec-Fetch-Site": "same-site"])).0, 403)
        // another name for this Mac (DNS rebinding)
        var rebound = request("GET", "/api/status")
        rebound.headers["host"] = "evil.example:47481"
        XCTAssertEqual(send(rebound).0, 421)
        XCTAssertEqual(send(request("GET", "/api/status")).0, 200, "curl and other local tools")
        // preflight: allowed origins only
        let pre = send(request("OPTIONS", "/api/bibliography", headers: ["Origin": g]))
        XCTAssertEqual(pre.0, 204)
        XCTAssertEqual(pre.2["Access-Control-Allow-Headers"]?.contains("X-Margins-Token"), true)
        XCTAssertEqual(pre.2["Access-Control-Allow-Private-Network"], "true")
        XCTAssertNil(send(request("OPTIONS", "/api/bibliography", headers: ["Origin": "https://evil.example"])).2["Access-Control-Allow-Origin"])
    }

    func testNoLibraryYet() {
        let none = CitationAPI(library: nil, cacheDir: cache.path, token: "k")
        XCTAssertEqual(none.respond(request("GET", "/api/status"), paperCount: 0).status, 503)
    }

    // MARK: - answers

    func testStatusAndSearch() throws {
        let p = try lib.addPaper(meta: meta("Signals <10 Hz & the \"quiet\" cortex", ["Ada Lee", "Ludwig van Beethoven"], "2020", doi: "10.1000/xyz"),
                                 pdf: nil, epub: nil, fulltext: nil)
        let (code, body, _) = send(request("GET", "/api/status"))
        XCTAssertEqual(code, 200)
        XCTAssertEqual((body as? [String: Any])?["api"] as? Int, CitationAPI.version)
        let hits = try XCTUnwrap(send(request("GET", "/api/search?q=quiet%20cortex")).1 as? [[String: Any]])
        XCTAssertEqual(hits.first?["id"] as? String, p.id)
        XCTAssertEqual(hits.first?["families"] as? [String], ["Lee", "van Beethoven"])
        XCTAssertEqual(hits.first?["title"] as? String, "Signals <10 Hz & the \"quiet\" cortex")
        XCTAssertEqual((hits.first?["meta"] as? [String: Any])?["doi"] as? String, "10.1000/xyz")
        // a pasted DOI, and searches with characters the full-text search treats specially
        XCTAssertEqual((send(request("GET", "/api/search?q=https%3A%2F%2Fdoi.org%2F10.1000%2FXYZ")).1 as? [[String: Any]])?.first?["id"] as? String, p.id)
        for q in ["\"", "AND OR (", "*", "-", "NEAR(", "'", "  "] {
            let r = send(request("GET", "/api/search?q=" + q.addingPercentEncoding(withAllowedCharacters: .alphanumerics)!))
            XCTAssertEqual(r.0, 200, q)
            XCTAssertNotNil(r.1 as? [Any], q)
        }
    }

    func testResolveCitations() throws {
        let p = try lib.addPaper(meta: meta("Alpha cells in the islet", ["Ada Lee", "Per Wu", "Kim Ng"], "2020", doi: "10.1234/alpha"),
                                 pdf: nil, epub: nil, fulltext: nil)
        let added = Locked<[String]>([])
        api.paperAdded = { id in added.mutate { $0.append(id) } }
        let found = try XCTUnwrap(send(request("POST", "/api/placeholders", json: ["texts": ["Shown (Lee et al., 2020; Smith 2019) (n = 2020)."]])).1 as? [String: Any])
        let ph = try XCTUnwrap((found["placeholders"] as? [[[String: Any]]])?.first?.first)
        XCTAssertEqual(ph["start"] as? Int, 6)
        XCTAssertEqual(ph["text"] as? String, "(Lee et al., 2020; Smith 2019)")
        let cites = try XCTUnwrap(ph["cites"] as? [[String: Any]])
        XCTAssertEqual(cites.count, 2)
        XCTAssertEqual(cites[0]["etAl"] as? Bool, true)
        let s = try XCTUnwrap(send(request("POST", "/api/suggest", json: ["cite": cites[0], "external": false])).1 as? [String: Any])
        XCTAssertEqual(s["exact"] as? Bool, true)
        XCTAssertEqual((s["candidates"] as? [[String: Any]])?.first?["id"] as? String, p.id)
        // the library's own paper isn't added again; one without a DOI is added from its details
        let same = try XCTUnwrap(send(request("POST", "/api/add", json: ["meta": ["title": "Alpha cells in the islet", "doi": "10.1234/ALPHA"]])).1 as? [String: Any])
        XCTAssertEqual(same["id"] as? String, p.id)
        XCTAssertEqual(same["added"] as? Bool, false)
        let new = try XCTUnwrap(send(request("POST", "/api/add", json: ["meta": ["title": "Beta waves", "authors": ["Kim Ng"], "year": "2021"]])).1 as? [String: Any])
        XCTAssertEqual(new["added"] as? Bool, true)
        XCTAssertNotNil(lib.paper(id: new["id"] as? String ?? ""))
        XCTAssertEqual(added.value, [new["id"] as? String ?? ""])
        XCTAssertEqual(send(request("POST", "/api/suggest", json: [:])).0, 400)
    }

    func testBibliography() throws {
        let a = try lib.addPaper(meta: meta("Alpha study of cells and more", ["Jane Smith"], "2020", doi: "10.1/a"), pdf: nil, epub: nil, fulltext: nil)
        let res = try XCTUnwrap(send(request("POST", "/api/bibliography", json: ["style": "apa", "ids": [a.id, a.id, "gone"]])).1 as? [String: Any])
        let entries = try XCTUnwrap(res["entries"] as? [[String: Any]])
        XCTAssertEqual(entries.count, 2, "each id once")
        XCTAssertEqual(entries[0]["id"] as? String, a.id)
        XCTAssertTrue((entries[0]["entry"] as? String ?? "").hasPrefix("Smith, J. (2020). Alpha study"))
        XCTAssertEqual(entries[1]["missing"] as? Bool, true)
        // a style the core doesn't know isn't quietly APA
        XCTAssertEqual(send(request("POST", "/api/bibliography", json: ["style": "csl:nature", "ids": [a.id]])).0, 400)
        XCTAssertEqual(send(request("POST", "/api/bibliography", json: ["style": "apa"])).0, 400)
        XCTAssertEqual(send(request("POST", "/api/bibliography", json: ["ids": [a.id]])).0, 200, "older add-ins: APA by default")
        var bad = request("POST", "/api/bibliography")
        bad.body = Data("{not json".utf8)
        XCTAssertEqual(send(bad).0, 400)
    }

    func testPapersTheLibraryLacks() throws {
        let kept = try lib.addPaper(meta: meta("Kept paper about oligodendrocytes", ["Ada Lee"], "2021", doi: "10.1/kept"), pdf: nil, epub: nil, fulltext: nil)
        let papers: [String: Any] = [
            // merged into `kept` (the old id): found again by DOI
            "old-id": ["title": "Kept paper about oligodendrocytes", "authors": ["Ada Lee"], "date": "2021", "doi": "https://doi.org/10.1/KEPT"],
            // deleted, or only in a co-author's library: formatted from what the document keeps
            "theirs": ["title": "Deleted & <gone>", "authors": ["Per Wu"], "date": "2019", "journal": "Neuron"],
            // too little to go on
            "empty": ["journal": "X"],
        ]
        let res = try XCTUnwrap(send(request("POST", "/api/bibliography", json: ["style": "apa", "ids": ["old-id", "theirs", "empty", "none"], "papers": papers])).1 as? [String: Any])
        let e = try XCTUnwrap(res["entries"] as? [[String: Any]])
        XCTAssertEqual(e[0]["id"] as? String, "old-id")
        XCTAssertEqual(e[0]["libraryId"] as? String, kept.id)
        XCTAssertEqual(e[1]["fromDocument"] as? Bool, true)
        XCTAssertEqual(e[1]["entry"] as? String, "Wu, P. (2019). Deleted & <gone>. *Neuron*.")
        XCTAssertEqual(e[1]["families"] as? [String], ["Wu"])
        XCTAssertEqual(e[2]["missing"] as? Bool, true)
        XCTAssertEqual(e[3]["missing"] as? Bool, true)
    }

    func testJournalStyleOffline() throws {
        try Self.testStyle.write(to: cache.appendingPathComponent("csl/test-author-date.csl"), atomically: true, encoding: .utf8)
        let a = try lib.addPaper(meta: meta("Alpha", ["Jane Smith"], "2020"), pdf: nil, epub: nil, fulltext: nil)
        let body: [String: Any] = ["style": "test-author-date", "citations": [[a.id], ["theirs"], ["none"]],
                                   "papers": ["theirs": ["title": "Beta", "authors": ["Per Wu"], "date": "2019"]]]
        let res = try XCTUnwrap(send(request("POST", "/api/render", json: body)).1 as? [String: Any])
        XCTAssertEqual(res["title"] as? String, "Test Author Date")
        let cites = try XCTUnwrap(res["citations"] as? [[String: Any]])
        XCTAssertEqual(cites.map { $0["text"] as? String }, ["(Smith, 2020)", "(Wu, 2019)", ""])
        XCTAssertEqual(res["missing"] as? [String], ["none"])
        XCTAssertEqual((res["items"] as? [[String: Any]])?.count, 2)
        // a style that can't be had (offline, or no such style): a message, not a crash
        let r = send(request("POST", "/api/render", json: ["style": "no-such-style-\(UUID().uuidString.prefix(6).lowercased())", "citations": [[a.id]]]))
        XCTAssertEqual(r.0, 502)
        XCTAssertNotNil((r.1 as? [String: Any])?["error"] as? String)
        XCTAssertEqual(send(request("POST", "/api/render", json: ["style": "../../etc/passwd", "citations": [[a.id]]])).0, 502)
    }

    func testSearchStaysFastInALargeLibrary() throws {
        for i in 0..<1200 {
            _ = try lib.addPaper(meta: meta("Paper \(i) on single-cell spatial transcriptomics of the brain", ["Author \(i)", "Ada Lee"], "20\(10 + i % 15)",
                                            doi: "10.5555/\(i)"), pdf: nil, epub: nil, fulltext: nil)
        }
        let start = Date()
        for q in ["spatial", "Lee", "brain transcriptomics", "Paper 1111"] {
            let hits = send(request("GET", "/api/search?q=" + q.addingPercentEncoding(withAllowedCharacters: .alphanumerics)!)).1 as? [[String: Any]]
            XCTAssertFalse(hits?.isEmpty ?? true, q)
        }
        XCTAssertLessThan(Date().timeIntervalSince(start), 2, "four searches in a library of 1200 papers")
        // a thesis: 300 references formatted in one request
        let ids = try lib.search(query: "spatial", limit: 300).map(\.paper.id)
        let t = Date()
        let res = send(request("POST", "/api/bibliography", json: ["style": "vancouver", "ids": ids]))
        XCTAssertEqual(((res.1 as? [String: Any])?["entries"] as? [Any])?.count, ids.count)
        XCTAssertLessThan(Date().timeIntervalSince(t), 2)
    }

    // MARK: - HTTP

    func testParsing() {
        let head = "POST /api/bibliography?x=1 HTTP/1.1\r\nHost: localhost:47481\r\nContent-Type: application/json\r\nContent-Length: 10\r\nX-Margins-Token:  k \r\n\r\n"
        guard case .incomplete = CitationHTTP.parse(Data(head.utf8)) else { return XCTFail("body still to come") }
        guard case .incomplete = CitationHTTP.parse(Data((head + "{\"ids\"").utf8)) else { return XCTFail("half the body") }
        guard case .request(let r) = CitationHTTP.parse(Data((head + "{\"ids\":[]}").utf8)) else { return XCTFail("complete") }
        XCTAssertEqual(r.method, "POST")
        XCTAssertEqual(r.path, "/api/bibliography")
        XCTAssertEqual(r.query["x"], "1")
        XCTAssertEqual(r.headers["x-margins-token"], "k")
        XCTAssertEqual(String(decoding: r.body, as: UTF8.self), "{\"ids\":[]}")
        guard case .bad(400) = CitationHTTP.parse(Data("garbage\r\n\r\n".utf8)) else { return XCTFail("not HTTP") }
        guard case .bad(413) = CitationHTTP.parse(Data("POST /api/render HTTP/1.1\r\nContent-Length: 999999999\r\n\r\n".utf8)) else { return XCTFail("too big") }
        guard case .bad(400) = CitationHTTP.parse(Data("POST / HTTP/1.1\r\nContent-Length: -1\r\n\r\n".utf8)) else { return XCTFail("negative") }
        guard case .bad(400) = CitationHTTP.parse(Data(repeating: 65, count: CitationHTTP.maxHeader + 1)) else { return XCTFail("endless header") }
        let wire = String(decoding: CitationResponse.json(200, ["ok": true]).encoded(), as: UTF8.self)
        XCTAssertTrue(wire.hasPrefix("HTTP/1.1 200 OK\r\n"))
        XCTAssertTrue(wire.contains("Content-Length: 11\r\n"))
        XCTAssertTrue(wire.hasSuffix("\r\n\r\n{\"ok\":true}"))
    }

    // MARK: - certificates

    func testCertificatesAreMadeAndRenewedWithoutTrustingAgain() throws {
        let dir = tmp.appendingPathComponent("tls")
        XCTAssertEqual(CitationCertificates.ensure(in: dir), .newAuthority)
        XCTAssertEqual(CitationCertificates.ensure(in: dir), .current)
        XCTAssertNotNil(CitationCertificates.identity(in: dir))
        let ca = try XCTUnwrap(CitationCertificates.certificate(dir.appendingPathComponent("ca.pem")))
        // valid for localhost and 127.0.0.1 with the authority trusted (name constraints and all)
        XCTAssertTrue(CitationCertificates.evaluate(in: dir, anchors: [ca]))
        XCTAssertTrue(CitationCertificates.evaluate(in: dir, anchors: [ca], host: "127.0.0.1"))
        XCTAssertFalse(CitationCertificates.evaluate(in: dir, anchors: [ca], host: "example.com"))
        let firstPem = try Data(contentsOf: dir.appendingPathComponent("server.pem"))
        XCTAssertGreaterThan(try XCTUnwrap(CitationCertificates.expiry(of: dir.appendingPathComponent("server.pem"))).timeIntervalSinceNow, 800 * 86400)
        // two years on (its last month): a new localhost certificate from the same, still trusted, authority
        let later = Date().addingTimeInterval(800 * 86400)
        XCTAssertEqual(CitationCertificates.ensure(in: dir, now: later), .renewed)
        XCTAssertNotEqual(try Data(contentsOf: dir.appendingPathComponent("server.pem")), firstPem)
        XCTAssertTrue(CitationCertificates.evaluate(in: dir, anchors: [ca]))
        XCTAssertNotNil(CitationCertificates.identity(in: dir))
        // keys readable by this user only
        let mode = try FileManager.default.attributesOfItem(atPath: dir.appendingPathComponent("ca.key").path)[.posixPermissions] as? Int
        XCTAssertEqual(mode, 0o600)
        // an authority without its key (made by earlier versions): kept while the certificate is
        // good, replaced when it runs out (and then trusted again)
        try FileManager.default.removeItem(at: dir.appendingPathComponent("ca.key"))
        XCTAssertEqual(CitationCertificates.ensure(in: dir), .current)
        XCTAssertEqual(CitationCertificates.ensure(in: dir, now: later), .newAuthority)
        XCTAssertFalse(CitationCertificates.evaluate(in: dir, anchors: [ca]), "the old authority no longer vouches for it")
    }

    static let testStyle = """
    <?xml version="1.0" encoding="utf-8"?>
    <style xmlns="http://purl.org/net/xbiblio/csl" class="in-text" version="1.0" default-locale="en-US">
      <info><title>Test Author Date</title><id>http://www.zotero.org/styles/test-author-date</id><updated>2024-01-01T00:00:00+00:00</updated></info>
      <macro name="author"><names variable="author"><name form="short" and="symbol" et-al-min="3" et-al-use-first="1"/></names></macro>
      <macro name="year"><date variable="issued"><date-part name="year"/></date></macro>
      <citation><layout prefix="(" suffix=")" delimiter="; "><text macro="author"/><text prefix=", " macro="year"/></layout></citation>
      <bibliography><layout suffix="."><names variable="author"><name name-as-sort-order="all" initialize-with=". "/></names><text prefix=" (" suffix=")" macro="year"/><text prefix=". " variable="title"/></layout></bibliography>
    </style>
    """
}

/// A value several threads may change.
final class Locked<T>: @unchecked Sendable {
    private var v: T
    private let lock = NSLock()
    init(_ v: T) { self.v = v }
    var value: T { lock.lock(); defer { lock.unlock() }; return v }
    func mutate(_ f: (inout T) -> Void) { lock.lock(); f(&v); lock.unlock() }
}
