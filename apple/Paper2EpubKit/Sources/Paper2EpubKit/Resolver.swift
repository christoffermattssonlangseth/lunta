import Foundation

/// Where figure images for a JATS document can be found.
enum AssetLocator {
    /// PMC open-data bucket on AWS: one folder per article version with the XML and every figure.
    case s3(base: URL, files: [String])
    /// URL templates; `{href}` and `{stem}` are replaced with the graphic reference.
    case patterns([String])
    case none
}

enum Source {
    case jats(Data, assets: AssetLocator, meta: PaperMeta, label: String)
    case pdf(Data, meta: PaperMeta, label: String)
}

typealias Candidate = (label: String, load: () async throws -> Source)

/// Turns user input (link, DOI, PMCID, PubMed ID, file) into candidate sources, best first.
enum Resolver {
    static let doiRE = try! NSRegularExpression(pattern: #"(10\.\d{4,9}/[^\s"<>?#]+)"#, options: [.caseInsensitive])
    static let arxivRE = try! NSRegularExpression(
        pattern: #"^(?:arxiv:)?(\d{4}\.\d{4,5}(?:v\d+)?|[a-z\-]+(?:\.[A-Z]{2})?/\d{7}(?:v\d+)?)$"#, options: [.caseInsensitive])

    static func candidates(for input: String, log: @escaping (String) -> Void) throws -> [Candidate] {
        var s = input.trimmingCharacters(in: .whitespacesAndNewlines)
        if s.isEmpty { throw fail("Paste a link, DOI or PMCID first.") }
        if s.lowercased().hasPrefix("doi:") { s = String(s.dropFirst(4)).trimmingCharacters(in: .whitespaces) }

        // arXiv: links, IDs and arXiv DOIs
        if let aid = arxivID(in: s) {
            return [("arXiv \(aid)", { throw fail("arXiv papers are not supported yet (coming in the next version).") })]
        }

        // PMCID, bare or inside a PMC / Europe PMC link
        if let pmc = firstMatch(#"\b(PMC\d{4,})\b"#, in: s, options: [.caseInsensitive])?.uppercased() {
            return [("PMC \(pmc)", { try await pmcSource(pmc, log: log) })]
        }

        // PubMed ID links
        if let pmid = firstMatch(#"(?:pubmed\.ncbi\.nlm\.nih\.gov/|europepmc\.org/(?:abstract|article)/MED/)(\d+)"#, in: s) {
            return [("PubMed \(pmid)", { try await europePMCByQuery("EXT_ID:\(pmid) AND SRC:MED", what: "PMID \(pmid)", log: log) })]
        }

        // DOI: doi.org links, bare DOIs, or a DOI embedded in a publisher URL
        if let doi = doi(in: s) {
            return doiCandidates(doi, log: log)
        }

        if s.hasPrefix("http://") || s.hasPrefix("https://"), let url = URL(string: s) {
            return [("web page", { try await fromWebPage(url, log: log) })]
        }
        if s.allSatisfy(\.isNumber), s.count >= 5 {   // bare PubMed ID
            return [("PubMed \(s)", { try await europePMCByQuery("EXT_ID:\(s) AND SRC:MED", what: "PMID \(s)", log: log) })]
        }
        throw fail("Don't know how to read “\(input)”. Try a DOI, a link to the article, or a PMCID.")
    }

    /// Candidates for a file handed to the app (share sheet / Files).
    static func candidates(forFile url: URL, log: @escaping (String) -> Void) throws -> [Candidate] {
        let data = try Data(contentsOf: url)
        switch Sniff.kind(data) {
        case "pdf":
            return [("PDF \(url.lastPathComponent)", { .pdf(data, meta: PaperMeta(), label: "PDF file") })]
        case "jats":
            return [("JATS \(url.lastPathComponent)", { .jats(data, assets: .none, meta: PaperMeta(), label: "JATS file") })]
        default:
            throw fail("\(url.lastPathComponent) is not a PDF or JATS XML file.")
        }
    }

    // MARK: - Input parsing

    static func arxivID(in s: String) -> String? {
        var t = s
        if let m = firstMatch(#"arxiv\.org/(?:abs|pdf|html)/([^\s?#]+?)(?:\.pdf)?/?$"#, in: s, options: [.caseInsensitive]) { t = m }
        if let r = t.range(of: #"(?i)^10\.48550/arxiv\."#, options: .regularExpression) { t.removeSubrange(r) }
        if let r = t.range(of: #"(?i)^https?://doi\.org/10\.48550/arxiv\."#, options: .regularExpression) { t.removeSubrange(r) }
        let ns = t as NSString
        if let m = arxivRE.firstMatch(in: t, range: NSRange(location: 0, length: ns.length)) {
            return ns.substring(with: m.range(at: 1))
        }
        return nil
    }

    static func doi(in s: String) -> String? {
        let decoded = s.removingPercentEncoding ?? s
        let ns = decoded as NSString
        guard let m = doiRE.firstMatch(in: decoded, range: NSRange(location: 0, length: ns.length)) else { return nil }
        var d = ns.substring(with: m.range(at: 1))
        // strip publisher URL suffixes that follow the DOI
        for suffix in ["/full", "/abstract", "/pdf", "/epdf", "/pdfdirect", "/fulltext", ".full.pdf", ".full", ".pdf", ".abstract", "/"] {
            if d.lowercased().hasSuffix(suffix) { d = String(d.dropLast(suffix.count)) }
        }
        d = d.trimmingCharacters(in: CharacterSet(charactersIn: ".,;)"))
        // bioRxiv / medRxiv URLs carry a version: 10.1101/2024.01.02.573123v2 -> 10.1101/2024.01.02.573123
        if d.hasPrefix("10.1101/"), let r = d.range(of: #"v\d+$"#, options: .regularExpression) { d.removeSubrange(r) }
        return d
    }

    static func firstMatch(_ pattern: String, in s: String, options: NSRegularExpression.Options = []) -> String? {
        guard let re = try? NSRegularExpression(pattern: pattern, options: options) else { return nil }
        let ns = s as NSString
        guard let m = re.firstMatch(in: s, range: NSRange(location: 0, length: ns.length)) else { return nil }
        return ns.substring(with: m.range(at: m.numberOfRanges > 1 ? 1 : 0))
    }

    // MARK: - DOI

    static func doiCandidates(_ doi: String, log: @escaping (String) -> Void) -> [Candidate] {
        var out: [Candidate] = []
        if doi.lowercased().hasPrefix("10.48550/arxiv.") {
            return [("arXiv", { throw fail("arXiv papers are not supported yet (coming in the next version).") })]
        }
        out.append(("Europe PMC full text for \(doi)", {
            try await europePMCByQuery("DOI:\"\(doi)\"", what: doi, log: log)
        }))
        if doi.lowercased().hasPrefix("10.7554/elife.") {
            let num = doi.split(separator: ".").last.map { String($0.filter(\.isNumber)) } ?? ""
            out.append(("eLife XML \(num)", { try await eLifeSource(num, log: log) }))
        }
        return out
    }

    // MARK: - Europe PMC / PMC

    static func europePMCByQuery(_ query: String, what: String, log: @escaping (String) -> Void) async throws -> Source {
        var comps = URLComponents(string: "https://www.ebi.ac.uk/europepmc/webservices/rest/search")!
        comps.queryItems = [.init(name: "query", value: query), .init(name: "resultType", value: "core"),
                            .init(name: "format", value: "json")]
        log("Looking up \(what) in Europe PMC")
        let r = try await Net.withRetry { try await Net.json(comps.url!) }
        let results = ((r["resultList"] as? [String: Any])?["result"] as? [[String: Any]]) ?? []
        guard let hit = results.first else { throw fail("Europe PMC does not know \(what)") }
        let meta = epmcMeta(hit)
        if let pmcid = hit["pmcid"] as? String {
            var src = try await pmcSource(pmcid, log: log)
            if case .jats(let d, let a, var m, let l) = src { m.merge(meta); src = .jats(d, assets: a, meta: m, label: l) }
            return src
        }
        if hit["source"] as? String == "PPR", let id = hit["id"] as? String {   // preprint with full text
            let xml = try await europePMCFullText(id, log: log)
            return .jats(xml, assets: .none, meta: meta, label: "Europe PMC preprint XML")
        }
        let journal = meta.journal.isEmpty ? "the publisher" : meta.journal
        throw fail("“\(meta.title.isEmpty ? what : meta.title)” isn't in PubMed Central, so there is no XML version to convert. "
                   + "It may still be free on \(journal)'s site as a PDF. PDF conversion isn't in the app yet.")
    }

    static func epmcMeta(_ r: [String: Any]) -> PaperMeta {
        var m = PaperMeta()
        m.title = ((r["title"] as? String) ?? "").trimmingCharacters(in: CharacterSet(charactersIn: ". "))
        let authors = ((r["authorList"] as? [String: Any])?["author"] as? [[String: Any]]) ?? []
        m.authors = authors.compactMap { $0["fullName"] as? String }.filter { !$0.isEmpty }
        m.date = (r["firstPublicationDate"] as? String) ?? ""
        m.doi = (r["doi"] as? String) ?? ""
        m.journal = (((r["journalInfo"] as? [String: Any])?["journal"] as? [String: Any])?["title"] as? String) ?? ""
        m.abstract = ((r["abstractText"] as? String) ?? "").replacingOccurrences(of: "<[^>]+>", with: "", options: .regularExpression)
        return m
    }

    static func europePMCFullText(_ id: String, log: @escaping (String) -> Void) async throws -> Data {
        log("Downloading full-text XML for \(id) from Europe PMC")
        let url = URL(string: "https://www.ebi.ac.uk/europepmc/webservices/rest/\(id)/fullTextXML")!
        let xml = try await Net.withRetry { try await Net.get(url, timeout: 90) }
        guard Sniff.kind(xml) == "jats" else { throw fail("Europe PMC returned no XML for \(id)") }
        return xml
    }

    static let s3Base = URL(string: "https://pmc-oa-opendata.s3.amazonaws.com/")!

    /// Prefers the PMC open-data bucket (XML + figures side by side); falls back to Europe PMC's XML.
    static func pmcSource(_ pmcid: String, log: @escaping (String) -> Void) async throws -> Source {
        log("Checking the PMC open-access archive for \(pmcid)")
        if let (folder, files) = try? await s3Listing(pmcid),
           let xmlName = files.first(where: { $0.hasSuffix(".xml") }) {
            let base = s3Base.appendingPathComponent(folder)
            if let xml = try? await Net.withRetry({ try await Net.get(base.appendingPathComponent(xmlName), timeout: 90) }),
               Sniff.kind(xml) == "jats" {
                return .jats(xml, assets: .s3(base: base, files: files), meta: PaperMeta(), label: "PMC open-access XML")
            }
        }
        let xml = try await europePMCFullText(pmcid, log: log)
        return .jats(xml, assets: .none, meta: PaperMeta(), label: "Europe PMC XML")
    }

    /// Lists the newest version folder for a PMCID in the PMC open-data bucket.
    static func s3Listing(_ pmcid: String) async throws -> (String, [String]) {
        var comps = URLComponents(url: s3Base, resolvingAgainstBaseURL: false)!
        comps.queryItems = [.init(name: "list-type", value: "2"), .init(name: "prefix", value: "\(pmcid)."),
                            .init(name: "max-keys", value: "1000")]
        let d = try await Net.get(comps.url!, timeout: 30)
        let body = String(decoding: d, as: UTF8.self)
        let re = try NSRegularExpression(pattern: "<Key>([^<]+)</Key>")
        let ns = body as NSString
        var byVersion: [Int: [String]] = [:]
        for m in re.matches(in: body, range: NSRange(location: 0, length: ns.length)) {
            let key = ns.substring(with: m.range(at: 1))
            let parts = key.split(separator: "/", maxSplits: 1)
            guard parts.count == 2, let dot = parts[0].lastIndex(of: "."),
                  let v = Int(parts[0][parts[0].index(after: dot)...]) else { continue }
            byVersion[v, default: []].append(String(parts[1]))
        }
        guard let v = byVersion.keys.max() else { throw fail("not in PMC open-access archive") }
        return ("\(pmcid).\(v)", byVersion[v]!)
    }

    // MARK: - eLife

    static func eLifeSource(_ num: String, log: @escaping (String) -> Void) async throws -> Source {
        let n = String(repeating: "0", count: max(0, 5 - num.count)) + num
        log("Downloading eLife XML \(n)")
        for v in [3, 2, 1] {
            let url = URL(string: "https://raw.githubusercontent.com/elifesciences/elife-article-xml/master/articles/elife-\(n)-v\(v).xml")!
            if let d = try? await Net.get(url, timeout: 90), Sniff.kind(d) == "jats" {
                return .jats(d, assets: .patterns([
                    "https://iiif.elifesciences.org/lax:\(n)%2F{href}/full/1500,/0/default.jpg",
                    "https://iiif.elifesciences.org/lax/\(n)%2F{href}/full/1500,/0/default.jpg",
                    "https://cdn.elifesciences.org/articles/\(n)/{stem}.jpg",
                ]), meta: PaperMeta(), label: "eLife XML")
            }
        }
        throw fail("eLife XML not found for \(n)")
    }

    // MARK: - Publisher pages

    /// Reads a publisher page's `citation_*` meta tags to find the DOI (or a PDF link).
    static func fromWebPage(_ url: URL, log: @escaping (String) -> Void) async throws -> Source {
        log("Reading \(url.host ?? "the page")")
        let r: Net.Response
        do { r = try await Net.fetch(url, accept: "text/html,application/xhtml+xml,application/pdf;q=0.9,*/*;q=0.8") }
        catch { throw fail("Could not open \(url.host ?? "the link"): \(error.localizedDescription)") }
        let kind = Sniff.kind(r.data)
        if kind == "pdf", (200..<300).contains(r.status) {
            return .pdf(r.data, meta: PaperMeta(url: url.absoluteString), label: "PDF from link")
        }
        if kind == "jats" { return .jats(r.data, assets: .none, meta: PaperMeta(), label: "XML from link") }
        guard (200..<300).contains(r.status) else {
            throw fail("\(url.host ?? "The site") refused the request (HTTP \(r.status)). Try the paper's DOI, or open the PDF in Safari and share it to Paper2Epub.")
        }
        let html = String(decoding: r.data.prefix(1_500_000), as: UTF8.self)
        let tags = metaTags(html)
        var doi = tags["citation_doi"] ?? tags["prism.doi"] ?? tags["dc.identifier"] ?? tags["bepress_citation_doi"]
        if doi == nil, let canon = Resolver.firstMatch(#"<link[^>]+rel="canonical"[^>]+href="([^"]+)""#, in: html) { doi = Resolver.doi(in: canon) }
        if let d = doi.flatMap({ Resolver.doi(in: $0) }) {
            log("Found DOI \(d)")
            var lastError: Error = fail("No open full text found for \(d)")
            for c in doiCandidates(d, log: log) {
                do { return try await c.load() } catch { lastError = error; log("  \(c.label): \(error.localizedDescription)") }
            }
            if let pdf = tags["citation_pdf_url"] { log("The page links a PDF: \(pdf)") }
            throw lastError
        }
        throw fail("Couldn't find a DOI on that page. Try the DOI directly, or share the PDF.")
    }

    /// `<meta name=… content=…>` pairs, lower-cased names, first occurrence wins.
    static func metaTags(_ html: String) -> [String: String] {
        var out: [String: String] = [:]
        let tagRE = try! NSRegularExpression(pattern: #"<meta\s[^>]*>"#, options: [.caseInsensitive])
        let attrRE = try! NSRegularExpression(pattern: #"([a-zA-Z:-]+)\s*=\s*("([^"]*)"|'([^']*)')"#)
        let ns = html as NSString
        for m in tagRE.matches(in: html, range: NSRange(location: 0, length: ns.length)) {
            let tag = ns.substring(with: m.range)
            let tns = tag as NSString
            var a: [String: String] = [:]
            for am in attrRE.matches(in: tag, range: NSRange(location: 0, length: tns.length)) {
                let k = tns.substring(with: am.range(at: 1)).lowercased()
                let v = am.range(at: 3).location != NSNotFound ? tns.substring(with: am.range(at: 3)) : tns.substring(with: am.range(at: 4))
                a[k] = v
            }
            if let n = (a["name"] ?? a["property"])?.lowercased(), let c = a["content"], out[n] == nil {
                out[n] = c.replacingOccurrences(of: "&amp;", with: "&")
            }
        }
        return out
    }
}

extension PaperMeta {
    init(url: String) { self.init(); self.url = url }
}
