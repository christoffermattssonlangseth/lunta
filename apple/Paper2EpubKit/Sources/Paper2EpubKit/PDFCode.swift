import Foundation
import CoreGraphics

// Code listings in a book: lines set in a monospaced font (LMMono, Courier, Inconsolata…), one after
// another in a column, become one block, shown as <pre><code> with the lines' breaks and indentation,
// instead of paragraphs of prose (a training script spliced into the middle of a sentence).

extension PDFAnalyzer {
    /// The page's code listings: their lines out of `lines`, each listing a paragraph of kind `.code`.
    func codeBlocks(_ pno: Int, _ lines: [PLine]) -> ([PLine], [Para]) {
        let code = lines.filter { $0.mono >= 0.8 && !$0.text.trimmingCharacters(in: .whitespaces).isEmpty }
            .sorted { ($0.bbox.y0, $0.bbox.x0) < ($1.bbox.y0, $1.bbox.x0) }
        guard code.count >= 3 else { return (lines, []) }
        let other = lines.filter { $0.mono < 0.8 && !$0.text.trimmingCharacters(in: .whitespaces).isEmpty }
        var blocks: [[PLine]] = []
        for l in code {
            // (the next line of the listing: close below the last, starting at or right of its left edge,
            // with no line of other type between: a web address in a reference list's entries is no listing)
            if var b = blocks.last, let last = b.last, l.bbox.y0 - last.bbox.y1 < 2.2 * max(l.size, 4),
               l.bbox.y0 >= last.bbox.y0 - 1, abs(l.bbox.x0 - b.map(\.bbox.x0).min()!) < 0.5 * boxes[pno].width,
               !other.contains(where: { o in o.bbox.y0 > last.bbox.y0 + 1 && o.bbox.y0 < l.bbox.y0 - 1 && o.bbox.x1 > l.bbox.x0 - 20 && o.bbox.x0 < l.bbox.x1 + 20 }) {
                b.append(l)
                blocks[blocks.count - 1] = b
            } else {
                blocks.append([l])
            }
        }
        var taken = Set<ObjectIdentifier>()
        var out: [Para] = []
        let url = try! NSRegularExpression(pattern: #"https?\s*:|www\.|\.(com|org|net|edu)\b|^\s*/|\w/\S*/"#)
        for b in blocks where b.count >= 3 && b.filter({ url.test($0.text) }).count * 3 < b.count {
            for l in b { taken.insert(ObjectIdentifier(l)) }
            out.append(Para(b, page: pno, kind: .code))
        }
        guard !out.isEmpty else { return (lines, []) }
        return (lines.filter { !taken.contains(ObjectIdentifier($0)) }, out)
    }

    /// A listing's text: its lines, each indented by how far right of the listing's left edge it starts
    /// (in the font's character widths), blank lines where the listing leaves a gap.
    static func codeText(_ p: Para) -> String {
        let ls = p.lines.sorted { $0.bbox.y0 < $1.bbox.y0 }
        guard let left = ls.map(\.bbox.x0).min() else { return "" }
        var out: [String] = []
        var prev: PLine?
        // (a character's width: the lines' own, a monospaced font's are all the same; 0.525 em for Latin Modern Mono)
        let ws = ls.filter { $0.text.count >= 8 }.map { $0.bbox.w / CGFloat($0.text.count) }.sorted()
        for l in ls {
            let cw = ws.isEmpty ? 0.525 * max(l.size, 4) : ws[ws.count / 2]
            if let prev, l.bbox.y0 - prev.bbox.y1 > 0.8 * l.size { out.append("") }
            let indent = max(0, Int(((l.bbox.x0 - left) / cw).rounded()))
            out.append(String(repeating: " ", count: indent) + lineEndsCleaned(l.text).trimmingCharacters(in: .whitespaces))
            prev = l
        }
        return out.joined(separator: "\n")
    }
}
