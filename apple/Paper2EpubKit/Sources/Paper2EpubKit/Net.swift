import Foundation

enum Net {
    static let userAgent = "Paper2Epub/0.1 (iOS)"

    static let session: URLSession = {
        let c = URLSessionConfiguration.default
        c.timeoutIntervalForRequest = 60
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

    /// GET a URL. Throws for transport errors; returns non-2xx responses so callers can decide.
    static func fetch(_ url: URL, accept: String? = nil, timeout: TimeInterval = 60) async throws -> Response {
        var req = URLRequest(url: url, timeoutInterval: timeout)
        if let accept { req.setValue(accept, forHTTPHeaderField: "Accept") }
        let (data, resp) = try await session.data(for: req)
        let http = resp as? HTTPURLResponse
        return Response(data: data, status: http?.statusCode ?? 0, url: http?.url ?? url,
                        contentType: http?.value(forHTTPHeaderField: "Content-Type") ?? "")
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
    static func withRetry<T>(attempts: Int = 3, _ op: () async throws -> T) async throws -> T {
        var lastError: Error?
        for i in 0..<attempts {
            do { return try await op() } catch {
                lastError = error
                let msg = "\(error)"
                let transient = msg.contains("HTTP 5") || (error as? URLError)?.code == .timedOut
                    || (error as? URLError)?.code == .networkConnectionLost
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
