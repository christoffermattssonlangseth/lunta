import Foundation

/// Minimal ZIP writer (stored + deflate) — enough for EPUB, no third-party dependency.
struct ZipWriter {
    private struct Entry {
        var name: [UInt8]
        var method: UInt16
        var crc: UInt32
        var size: UInt32
        var payload: Data
        var offset: UInt32 = 0
    }

    private var entries: [Entry] = []

    /// Adds a file. `compress: false` stores it (required for the EPUB `mimetype` entry).
    mutating func add(_ name: String, _ data: Data, compress: Bool = true) {
        let crc = CRC32.checksum(data)
        var payload = data
        var method: UInt16 = 0
        if compress, data.count > 64, let d = try? (data as NSData).compressed(using: .zlib) as Data, d.count < data.count {
            payload = d          // NSData's .zlib is raw DEFLATE, which is what ZIP method 8 expects
            method = 8
        }
        entries.append(Entry(name: Array(name.utf8), method: method, crc: crc, size: UInt32(data.count), payload: payload))
    }

    func finish() -> Data {
        var out = Data()
        var entries = self.entries
        let (time, date) = Self.dosNow()
        for i in entries.indices {
            entries[i].offset = UInt32(out.count)
            let e = entries[i]
            out.u32(0x04034b50); out.u16(20); out.u16(0x0800); out.u16(e.method); out.u16(time); out.u16(date)
            out.u32(e.crc); out.u32(UInt32(e.payload.count)); out.u32(e.size)
            out.u16(UInt16(e.name.count)); out.u16(0)
            out.append(contentsOf: e.name)
            out.append(e.payload)
        }
        let cdStart = out.count
        for e in entries {
            out.u32(0x02014b50); out.u16(20); out.u16(20); out.u16(0x0800); out.u16(e.method); out.u16(time); out.u16(date)
            out.u32(e.crc); out.u32(UInt32(e.payload.count)); out.u32(e.size)
            out.u16(UInt16(e.name.count)); out.u16(0); out.u16(0); out.u16(0); out.u16(0); out.u32(0)
            out.u32(e.offset)
            out.append(contentsOf: e.name)
        }
        let cdSize = out.count - cdStart
        out.u32(0x06054b50); out.u16(0); out.u16(0)
        out.u16(UInt16(entries.count)); out.u16(UInt16(entries.count))
        out.u32(UInt32(cdSize)); out.u32(UInt32(cdStart)); out.u16(0)
        return out
    }

    private static func dosNow() -> (UInt16, UInt16) {
        let c = Calendar(identifier: .gregorian).dateComponents(in: .current, from: Date())
        let time = UInt16((c.hour ?? 0) << 11 | (c.minute ?? 0) << 5 | (c.second ?? 0) / 2)
        let date = UInt16(((c.year ?? 1980) - 1980) << 9 | (c.month ?? 1) << 5 | (c.day ?? 1))
        return (time, date)
    }
}

private extension Data {
    mutating func u16(_ v: UInt16) { append(UInt8(v & 0xff)); append(UInt8(v >> 8)) }
    mutating func u32(_ v: UInt32) { for i in 0..<4 { append(UInt8((v >> (8 * UInt32(i))) & 0xff)) } }
}

enum CRC32 {
    static let table: [UInt32] = (0..<256).map { i -> UInt32 in
        var c = UInt32(i)
        for _ in 0..<8 { c = (c & 1) != 0 ? 0xEDB88320 ^ (c >> 1) : c >> 1 }
        return c
    }

    static func checksum(_ data: Data) -> UInt32 {
        var c: UInt32 = 0xffffffff
        data.withUnsafeBytes { (buf: UnsafeRawBufferPointer) in
            for b in buf { c = table[Int((c ^ UInt32(b)) & 0xff)] ^ (c >> 8) }
        }
        return c ^ 0xffffffff
    }
}

/// Minimal ZIP reader (stored + deflate, ZIP64): unpacks an archive into a folder, e.g. a Zotero
/// export zipped and sent to an iPad. No encryption; entries with ".." or absolute paths, and
/// macOS's "__MACOSX" resource forks, are left out.
public enum ZipReader {
    public enum Failure: LocalizedError {
        case notAZip, damaged(String), encrypted(String), unsupported(String, UInt16)
        public var errorDescription: String? {
            switch self {
            case .notAZip: return "The file is not a ZIP archive."
            case .damaged(let n): return "The ZIP archive is damaged" + (n.isEmpty ? "." : " (at “\(n)”).")
            case .encrypted(let n): return "“\(n)” in the ZIP archive is encrypted."
            case .unsupported(let n, let m): return "“\(n)” in the ZIP archive is compressed in a way Lunta can't read (method \(m))."
            }
        }
    }

    /// Unpacks `zip` into `dir` (made if needed). Returns the files written.
    @discardableResult
    public static func extract(_ zip: URL, to dir: URL) throws -> [URL] {
        let data = try Data(contentsOf: zip, options: .alwaysMapped)
        return try extract(data, to: dir)
    }

    @discardableResult
    public static func extract(_ data: Data, to dir: URL) throws -> [URL] {
        let fm = FileManager.default
        try fm.createDirectory(at: dir, withIntermediateDirectories: true)
        var out: [URL] = []
        for e in try entries(data) {
            let parts = e.name.split(separator: "/", omittingEmptySubsequences: true).map(String.init)
            guard !e.name.hasSuffix("/"), !parts.isEmpty, !e.name.hasPrefix("/"), !e.name.hasPrefix("\\"),
                  !parts.contains(where: { $0 == ".." || $0 == "." }), parts.first != "__MACOSX",
                  parts.last?.hasPrefix("._") == false else { continue }
            if e.flags & 1 != 0 { throw Failure.encrypted(e.name) }
            let start = Int(e.localOffset)
            guard start + 30 <= data.count, data.u32(at: start) == 0x04034b50 else { throw Failure.damaged(e.name) }
            let body = start + 30 + Int(data.u16(at: start + 26)) + Int(data.u16(at: start + 28))
            guard body + Int(e.compressedSize) <= data.count else { throw Failure.damaged(e.name) }
            let payload = data.subdata(in: data.startIndex + body ..< data.startIndex + body + Int(e.compressedSize))
            let content: Data
            switch e.method {
            case 0: content = payload
            case 8:
                if payload.isEmpty { content = Data() } else {
                    guard let d = try? (payload as NSData).decompressed(using: .zlib) as Data else { throw Failure.damaged(e.name) }
                    content = d
                }
            default: throw Failure.unsupported(e.name, e.method)
            }
            let url = parts.reduce(dir) { $0.appendingPathComponent($1) }
            try fm.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
            try content.write(to: url)
            out.append(url)
        }
        return out
    }

    struct Entry {
        var name: String
        var method: UInt16
        var flags: UInt16
        var compressedSize: UInt64
        var size: UInt64
        var localOffset: UInt64
    }

    /// The central directory's entries.
    static func entries(_ data: Data) throws -> [Entry] {
        // the end-of-central-directory record: in the last 64 KB + 22 bytes
        guard data.count >= 22 else { throw Failure.notAZip }
        var eocd = -1
        var i = data.count - 22
        let stop = max(0, data.count - 22 - 65_535)
        while i >= stop { if data.u32(at: i) == 0x06054b50 { eocd = i; break }; i -= 1 }
        guard eocd >= 0 else { throw Failure.notAZip }
        var count = UInt64(data.u16(at: eocd + 10))
        var cdOffset = UInt64(data.u32(at: eocd + 16))
        // ZIP64: the locator just before the record points at the ZIP64 record with the real numbers
        if eocd >= 20, data.u32(at: eocd - 20) == 0x07064b50 {
            let z = Int(data.u64(at: eocd - 12))
            if z >= 0, z + 56 <= data.count, data.u32(at: z) == 0x06064b50 {
                count = data.u64(at: z + 32)
                cdOffset = data.u64(at: z + 48)
            }
        }
        var out: [Entry] = []
        var p = Int(cdOffset)
        for _ in 0..<count {
            guard p >= 0, p + 46 <= data.count, data.u32(at: p) == 0x02014b50 else { throw Failure.damaged("") }
            let flags = data.u16(at: p + 8), method = data.u16(at: p + 10)
            var csize = UInt64(data.u32(at: p + 20)), size = UInt64(data.u32(at: p + 24))
            let nameLen = Int(data.u16(at: p + 28)), extraLen = Int(data.u16(at: p + 30)), commentLen = Int(data.u16(at: p + 32))
            var offset = UInt64(data.u32(at: p + 42))
            guard p + 46 + nameLen + extraLen <= data.count else { throw Failure.damaged("") }
            let nameBytes = data.subdata(in: data.startIndex + p + 46 ..< data.startIndex + p + 46 + nameLen)
            let name = String(data: nameBytes, encoding: .utf8) ?? String(decoding: nameBytes, as: UTF8.self)
            // ZIP64 extra field: the sizes and offset that didn't fit, in this order
            var x = p + 46 + nameLen
            let xEnd = x + extraLen
            while x + 4 <= xEnd {
                let id = data.u16(at: x), len = Int(data.u16(at: x + 2))
                if id == 1 {
                    var f = x + 4
                    if size == 0xFFFF_FFFF, f + 8 <= x + 4 + len { size = data.u64(at: f); f += 8 }
                    if csize == 0xFFFF_FFFF, f + 8 <= x + 4 + len { csize = data.u64(at: f); f += 8 }
                    if offset == 0xFFFF_FFFF, f + 8 <= x + 4 + len { offset = data.u64(at: f) }
                }
                x += 4 + len
            }
            out.append(Entry(name: name, method: method, flags: flags, compressedSize: csize, size: size, localOffset: offset))
            p += 46 + nameLen + extraLen + commentLen
        }
        return out
    }
}

private extension Data {
    func u16(at i: Int) -> UInt16 {
        let b = startIndex + i
        return UInt16(self[b]) | UInt16(self[b + 1]) << 8
    }
    func u32(at i: Int) -> UInt32 {
        let b = startIndex + i
        return (0..<4).reduce(UInt32(0)) { $0 | UInt32(self[b + $1]) << (8 * UInt32($1)) }
    }
    func u64(at i: Int) -> UInt64 { UInt64(u32(at: i)) | UInt64(u32(at: i + 4)) << 32 }
}
