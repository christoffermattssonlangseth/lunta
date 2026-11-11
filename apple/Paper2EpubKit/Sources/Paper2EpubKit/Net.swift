import Foundation

enum Net {
    static let userAgent = "Paper2Epub/0.1 (iOS)"

    static let session: URLSession = {
        let c = URLSessionConfiguration.default
        c.timeoutIntervalForRequest = 25
        c.timeoutIntervalForResource = 300
        c.httpAdditionalHeaders = ["User-Agent": userAgent]
        c.httpMaximumConnectionsPerHost = 6
        return URLSession(configuration: c)
    }()

    struct Response {
        var data: Data
        var status: Int
        var url: URL
        var contentType: String
    }

    /// Skips hosts that are down: after 3 failures in a row, requests fail fast for 5 minutes.
    actor Breaker {
        static let shared = Breaker()
        private var failures: [String: Int] = [:]
        private var openUntil: [String: Date] = [:]
        func check(_ host: String) throws {
            if let until = openUntil[host], until > Date() {
                throw fail("\(host) is not responding; skipping it for a few minutes")
            }
        }
        func success(_ host: String) { failures[host] = 0; openUntil[host] = nil }
        func failure(_ host: String) {
            failures[host, default: 0] += 1
            if failures[host]! >= 3 { openUntil[host] = Date().addingTimeInterval(300); failures[host] = 0 }
        }
    }

    /// GET a URL. Throws for transport errors; returns non-2xx responses so callers can decide.
    static func fetch(_ url: URL, accept: String? = nil, timeout: TimeInterval = 25) async throws -> Response {
        let host = url.host ?? ""
        try await Breaker.shared.check(host)
        var req = URLRequest(url: url, timeoutInterval: min(timeout, 45))
        if let accept { req.setValue(accept, forHTTPHeaderField: "Accept") }
        do {
            let (data, resp) = try await session.data(for: req)
            let http = resp as? HTTPURLResponse
            if let code = http?.statusCode, code >= 500 { await Breaker.shared.failure(host) } else { await Breaker.shared.success(host) }
            return Response(data: data, status: http?.statusCode ?? 0, url: http?.url ?? url,
                            contentType: http?.value(forHTTPHeaderField: "Content-Type") ?? "")
        } catch {
            if let e = error as? URLError, [.timedOut, .cannotConnectToHost, .networkConnectionLost, .cannotFindHost].contains(e.code) {
                await Breaker.shared.failure(host)
            }
            throw error
        }
    }

    /// GET a URL and require a 2xx status.
    static func get(_ url: URL, accept: String? = nil, timeout: TimeInterval = 60) async throws -> Data {
        let r = try await fetch(url, accept: accept, timeout: timeout)
        guard (200..<300).contains(r.status) else {
            throw fail("HTTP \(r.status) from \(url.host ?? url.absoluteString)")
        }
        return r.data
    }

    static func json(_ url: URL) async throws -> [String: Any] {
        let d = try await get(url, accept: "application/json", timeout: 30)
        guard let o = try JSONSerialization.jsonObject(with: d) as? [String: Any] else {
            throw fail("Unexpected JSON from \(url.host ?? "")")
        }
        return o
    }

    /// Retries transient failures (5xx, timeouts) a couple of times; Europe PMC has bad minutes.
    static func withRetry<T>(attempts: Int = 2, _ op: () async throws -> T) async throws -> T {
        var lastError: Error?
        for i in 0..<attempts {
            do { return try await op() } catch {
                lastError = error
                let msg = "\(error)"
                // timeouts are not retried: a slow service would just stall the queue
                let transient = msg.contains("HTTP 5") || (error as? URLError)?.code == .networkConnectionLost
                if !transient || i == attempts - 1 { break }
                try await Task.sleep(nanoseconds: UInt64(1_500_000_000 * (i + 1)))
            }
        }
        throw lastError!
    }
}

enum Sniff {
    static func kind(_ d: Data) -> String {
        let head = d.prefix(5)
        if head.starts(with: Array("%PDF-".utf8)) { return "pdf" }
        if d.starts(with: [0x1f, 0x8b]) { return "gzip" }
        if d.starts(with: [0x50, 0x4b, 0x03, 0x04]) { return "zip" }
        let s = String(decoding: d.prefix(3000), as: UTF8.self).lowercased()
        if s.contains("<article") || s.contains("jats") || s.contains("<!doctype article") { return "jats" }
        if s.contains("<html") || s.contains("<!doctype html") { return "html" }
        return "unknown"
    }
}
