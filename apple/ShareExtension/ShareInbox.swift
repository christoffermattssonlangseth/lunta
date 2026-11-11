import Foundation

/// Hand-off between the share extension and the app: the extension drops PDFs and links into a
/// folder in the shared App Group container; the app adds them to the library when it next opens.
enum ShareInbox {
    static let group = "group.com.cmlangseth.margins"

    static var folder: URL? {
        FileManager.default.containerURL(forSecurityApplicationGroupIdentifier: group)?
            .appendingPathComponent("Inbox", isDirectory: true)
    }

    struct Item {
        enum Kind { case pdf(URL), link(String) }
        let kind: Kind
        let file: URL
    }

    @discardableResult
    static func put(pdf: URL) throws -> URL {
        guard let dir = folder else { throw CocoaError(.fileNoSuchFile) }
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let name = pdf.deletingPathExtension().lastPathComponent
        let dest = dir.appendingPathComponent("\(Int(Date().timeIntervalSince1970 * 1000))-\(UUID().uuidString.prefix(6))-\(name).pdf")
        try FileManager.default.copyItem(at: pdf, to: dest)
        return dest
    }

    static func put(pdfData: Data, name: String) throws {
        guard let dir = folder else { throw CocoaError(.fileNoSuchFile) }
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let dest = dir.appendingPathComponent("\(Int(Date().timeIntervalSince1970 * 1000))-\(UUID().uuidString.prefix(6))-\(name).pdf")
        try pdfData.write(to: dest)
    }

    static func put(link: String) throws {
        guard let dir = folder else { throw CocoaError(.fileNoSuchFile) }
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let dest = dir.appendingPathComponent("\(Int(Date().timeIntervalSince1970 * 1000))-\(UUID().uuidString.prefix(6)).link")
        try link.write(to: dest, atomically: true, encoding: .utf8)
    }

    /// Everything waiting, oldest first.
    static func pending() -> [Item] {
        guard let dir = folder,
              let files = try? FileManager.default.contentsOfDirectory(at: dir, includingPropertiesForKeys: nil) else { return [] }
        return files.sorted { $0.lastPathComponent < $1.lastPathComponent }.compactMap { f in
            switch f.pathExtension.lowercased() {
            case "pdf": return Item(kind: .pdf(f), file: f)
            case "link":
                guard let s = try? String(contentsOf: f, encoding: .utf8) else { return nil }
                return Item(kind: .link(s), file: f)
            default: return nil
            }
        }
    }

    static func remove(_ item: Item) { try? FileManager.default.removeItem(at: item.file) }
}
