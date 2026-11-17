import Foundation

/// Hand-off between the Safari extension on iPhone and iPad and the Lunta app. There is no Lunta
/// server on the device (as there is on the Mac, 127.0.0.1:47480), so background.js sends the
/// same requests as native messages to SafariWebExtensionHandler, which only writes files (an
/// extension has little memory): each request becomes a folder in the App Group container, and
/// the app does the work (adds the paper, attaches the PDF, makes the reading view) when it
/// becomes active or is opened with lunta://safari.
///
///   <group>/SafariInbox/index.json              the library as the extension needs it for "status"
///                                               (written by the app when the library changes)
///   <group>/SafariInbox/.staging/<token>/       a request being sent, in pieces
///   <group>/SafariInbox/<ms>-<token>/           a request ready for the app:
///       request.json   {kind: save|pdf|fulltext, meta?, paperId?, name?, url?, doi?, title?,
///                       html?, liveHtml?, pdf?, images?: [{urls, file}], imagesSkipped?, dryRun?}
///       the files it names (page.html, live.html, paper.pdf, img-0 …)
///
/// Messages (background.js → handler), all answered with a JSON object ({error} on failure):
///   {op: "ping"}                                  → {platform: "ios"}
///   {op: "status", doi, title}                    → {inLibrary, id?, title?, hasPdf, hasReadingView,
///                                                     pending, pendingSave, pendingPdf, pendingFullText}
///   {op: "begin"}                                 → {token}
///   {op: "append", token, file, base64 | text}    → {ok}   (pieces of ≤ ~1 MB, appended in order)
///   {op: "commit", token, request}                → status + {queued: true, message}
///   {op: "abort", token}                          → {ok}
/// Foundation only: compiled into the extension (LuntaSafari) and into both apps.
enum SafariInbox {
    static let group = "group.com.cmlangseth.margins"

    /// The folder in the App Group container (nil if the group isn't available).
    static var groupRoot: URL? {
        FileManager.default.containerURL(forSecurityApplicationGroupIdentifier: group)?
            .appendingPathComponent("SafariInbox", isDirectory: true)
    }

    /// A request may not grow beyond this (the PDF, the page and its figures together).
    static let maxRequestBytes = 160 * 1024 * 1024

    // MARK: - Extension side

    static func handle(_ m: [String: Any], root: URL) -> [String: Any] {
        do {
            switch m["op"] as? String ?? "" {
            case "ping":
                return ["platform": "ios"]
            case "status":
                return status(doi: m["doi"] as? String ?? "", title: m["title"] as? String ?? "", root: root)
            case "begin":
                cleanStaging(root: root, olderThan: 6 * 3600)
                let token = UUID().uuidString
                _ = try staging(root, token)
                return ["token": token]
            case "append":
                let dir = try stagingDir(m, root)
                guard let name = m["file"] as? String, validName(name) else { throw Failure("bad file name") }
                let data: Data
                if let b = m["base64"] as? String {
                    guard let d = Data(base64Encoded: b) else { throw Failure("bad base64") }
                    data = d
                } else if let t = m["text"] as? String {
                    data = Data(t.utf8)
                } else { throw Failure("nothing to append") }
                guard size(of: dir) + data.count <= maxRequestBytes else {
                    try? FileManager.default.removeItem(at: dir)
                    throw Failure("This is too large to send to Lunta from Safari.")
                }
                let file = dir.appendingPathComponent(name)
                if !FileManager.default.fileExists(atPath: file.path) { FileManager.default.createFile(atPath: file.path, contents: nil) }
                let h = try FileHandle(forWritingTo: file)
                defer { try? h.close() }
                try h.seekToEnd()
                try h.write(contentsOf: data)
                return ["ok": true]
            case "commit":
                let dir = try stagingDir(m, root)
                guard let req = m["request"] as? [String: Any], let kind = req["kind"] as? String,
                      ["save", "pdf", "findpdf", "fulltext"].contains(kind) else { throw Failure("bad request") }
                for f in files(in: req) {
                    guard validName(f), FileManager.default.fileExists(atPath: dir.appendingPathComponent(f).path) else {
                        throw Failure("missing part: \(f)")
                    }
                }
                try JSONSerialization.data(withJSONObject: req).write(to: dir.appendingPathComponent("request.json"))
                let ready = root.appendingPathComponent("\(Int64(Date().timeIntervalSince1970 * 1000))-\(dir.lastPathComponent)", isDirectory: true)
                try FileManager.default.moveItem(at: dir, to: ready)
                let (doi, title) = identity(req)
                var out = status(doi: doi, title: title, root: root)
                out["queued"] = true
                out["outcome"] = "queued"
                out["message"] = queuedMessage(kind, req)
                return out
            case "abort":
                if let dir = try? stagingDir(m, root) { try? FileManager.default.removeItem(at: dir) }
                return ["ok": true]
            default:
                throw Failure("unknown request")
            }
        } catch let f as Failure {
            return ["error": f.message]
        } catch {
            return ["error": error.localizedDescription]
        }
    }

    static func queuedMessage(_ kind: String, _ req: [String: Any]) -> String {
        switch kind {
        case "pdf": return "PDF sent to Lunta. Open Lunta to finish: it saves the PDF there."
        case "findpdf": return "Sent to Lunta. Open Lunta to finish: it looks for a free PDF there."
        case "fulltext":
            let n = (req["images"] as? [Any])?.count ?? 0
            return "Sent the page to Lunta (\(n == 1 ? "1 figure" : "\(n) figures")). Open Lunta to finish: it makes the reading view there (and keeps the current one if a highlight wouldn't be found in this page)."
        default: return "Sent to Lunta. Open Lunta to finish: it adds the paper there."
        }
    }

    // MARK: - Status (from the app's index and the requests still waiting)

    struct Entry: Codable, Equatable {
        var id: String
        var doi: String
        var title: String
        var pdf: Bool
        var epub: Bool
    }

    static func norm(_ s: String) -> String { s.lowercased().filter { $0.isLetter || $0.isNumber } }

    static func status(doi: String, title: String, root: URL) -> [String: Any] {
        let d = doi.lowercased(), t = norm(title)
        var out: [String: Any] = ["inLibrary": false]
        if let data = try? Data(contentsOf: root.appendingPathComponent("index.json")),
           let entries = try? JSONDecoder().decode([Entry].self, from: data) {
            let hit = (d.isEmpty ? nil : entries.first { $0.doi.lowercased() == d })
                ?? (t.count > 12 ? entries.first { norm($0.title) == t } : nil)
            if let p = hit {
                out = ["inLibrary": true, "id": p.id, "title": p.title, "hasPdf": p.pdf, "hasReadingView": p.epub]
            }
        }
        var save = false, pdf = false, page = false
        for (_, req) in pending(root: root) {
            let (rd, rt) = identity(req)
            guard (!d.isEmpty && rd.lowercased() == d) || (t.count > 12 && norm(rt) == t) else { continue }
            switch req["kind"] as? String {
            case "save": save = true
            case "pdf", "findpdf": pdf = true
            case "fulltext": page = true
            default: break
            }
        }
        out["pendingSave"] = save
        out["pendingPdf"] = pdf
        out["pendingFullText"] = page
        out["pending"] = save || pdf || page
        return out
    }

    /// The paper a request is about: its DOI and title.
    static func identity(_ req: [String: Any]) -> (String, String) {
        let meta = req["meta"] as? [String: Any] ?? [:]
        let doi = req["doi"] as? String ?? meta["doi"] as? String ?? ""
        let title = req["title"] as? String ?? meta["title"] as? String ?? ""
        return (doi, title)
    }

    // MARK: - App side

    /// Requests waiting for the app, oldest first.
    static func pending(root: URL) -> [(URL, [String: Any])] {
        let fm = FileManager.default
        guard let dirs = try? fm.contentsOfDirectory(at: root, includingPropertiesForKeys: nil, options: [.skipsHiddenFiles]) else { return [] }
        return dirs.sorted { $0.lastPathComponent < $1.lastPathComponent }.compactMap { dir in
            guard let d = try? Data(contentsOf: dir.appendingPathComponent("request.json")),
                  let req = try? JSONSerialization.jsonObject(with: d) as? [String: Any] else { return nil }
            return (dir, req)
        }
    }

    /// A file a request names, inside its folder (nil for a name that isn't one of ours).
    static func file(_ name: Any?, in dir: URL) -> URL? {
        guard let name = name as? String, validName(name) else { return nil }
        let u = dir.appendingPathComponent(name)
        return FileManager.default.fileExists(atPath: u.path) ? u : nil
    }

    static func writeIndex(_ entries: [Entry], root: URL) throws {
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        try JSONEncoder().encode(entries).write(to: root.appendingPathComponent("index.json"), options: .atomic)
    }

    /// Requests that were never finished (Safari closed mid-way) are dropped after a while.
    static func cleanStaging(root: URL, olderThan age: TimeInterval) {
        let fm = FileManager.default
        let dir = root.appendingPathComponent(".staging", isDirectory: true)
        guard let items = try? fm.contentsOfDirectory(at: dir, includingPropertiesForKeys: nil) else { return }
        let now = Date().timeIntervalSince1970
        for u in items {
            // the token's folder is made at "begin": its name carries no time, so look at its marker
            let marker = u.appendingPathComponent(".begun")
            let t = (try? String(contentsOf: marker, encoding: .utf8)).flatMap(Double.init) ?? 0
            if now - t > age { try? fm.removeItem(at: u) }
        }
    }

    // MARK: - Helpers

    struct Failure: Error { let message: String; init(_ m: String) { message = m } }

    private static func staging(_ root: URL, _ token: String) throws -> URL {
        let dir = root.appendingPathComponent(".staging", isDirectory: true).appendingPathComponent(token, isDirectory: true)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        try String(Date().timeIntervalSince1970).write(to: dir.appendingPathComponent(".begun"), atomically: true, encoding: .utf8)
        return dir
    }

    private static func stagingDir(_ m: [String: Any], _ root: URL) throws -> URL {
        guard let t = m["token"] as? String, UUID(uuidString: t) != nil else { throw Failure("bad token") }
        let dir = root.appendingPathComponent(".staging", isDirectory: true).appendingPathComponent(t, isDirectory: true)
        guard FileManager.default.fileExists(atPath: dir.path) else { throw Failure("This request has expired. Try again.") }
        return dir
    }

    static func validName(_ s: String) -> Bool {
        !s.isEmpty && s.count <= 64 && !s.hasPrefix(".") && s != "request.json"
            && s.unicodeScalars.allSatisfy { CharacterSet.alphanumerics.contains($0) && $0.isASCII || "._-".unicodeScalars.contains($0) }
    }

    private static func files(in req: [String: Any]) -> [String] {
        var out = [req["html"], req["liveHtml"], req["pdf"]].compactMap { $0 as? String }
        for i in (req["images"] as? [[String: Any]]) ?? [] { if let f = i["file"] as? String { out.append(f) } }
        return out
    }

    private static func size(of dir: URL) -> Int {
        let fm = FileManager.default
        return ((try? fm.contentsOfDirectory(at: dir, includingPropertiesForKeys: [.fileSizeKey])) ?? [])
            .reduce(0) { $0 + ((try? $1.resourceValues(forKeys: [.fileSizeKey]).fileSize) ?? 0) }
    }
}
