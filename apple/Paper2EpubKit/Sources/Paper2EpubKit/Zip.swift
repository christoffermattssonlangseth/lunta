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
