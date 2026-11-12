import SwiftUI
import PDFKit
import Combine
import LuntaCore

#if os(macOS)
private typealias PlatformColor = NSColor
#else
private typealias PlatformColor = UIColor
#endif

/// Highlights in the PDF view. They are the same highlights as in the reading view: each is stored
/// with its text (quote, and a little text before and after), so one made in either view shows in
/// both. In the PDF they are drawn on top of the pages and never written into the file.
@MainActor
final class PDFHighlighter: ObservableObject {
    @Published var hasSelection = false
    weak var view: PDFView?
    var onTap: (String) -> Void = { _ in }
    private var observers: [NSObjectProtocol] = []
    private var pageText: [Int: (norm: NSString, map: [Int])] = [:]

    func attach(_ v: PDFView) {
        view = v
        pageText = [:]
        observers.forEach(NotificationCenter.default.removeObserver)
        observers = [
            NotificationCenter.default.addObserver(forName: .PDFViewSelectionChanged, object: v, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated {
                    let s = self?.view?.currentSelection?.string?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
                    self?.hasSelection = !s.isEmpty
                }
            },
            NotificationCenter.default.addObserver(forName: .PDFViewDocumentChanged, object: v, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { self?.pageText = [:] }
            },
        ]
    }

    /// Taps and clicks on a highlight open it.
    func handleTap(at point: CGPoint) -> Bool {
        guard let v = view, let page = v.page(for: point, nearest: false) else { return false }
        let p = v.convert(point, to: page)
        guard let hit = page.annotations.last(where: { ($0.userName ?? "").hasPrefix("lunta:") && $0.bounds.insetBy(dx: -2, dy: -2).contains(p) }),
              let name = hit.userName, v.currentSelection?.string?.isEmpty ?? true else { return false }
        onTap(String(name.dropFirst(6)))
        return true
    }

    // MARK: drawing

    func apply(_ anns: [Annotation]) {
        guard let doc = view?.document else { return }
        for i in 0..<doc.pageCount {
            guard let page = doc.page(at: i) else { continue }
            for a in page.annotations where (a.userName ?? "").hasPrefix("lunta:") { page.removeAnnotation(a) }
        }
        for a in anns {
            guard let (page, range) = locate(a, in: doc), let sel = page.selection(for: range) else { continue }
            let color = PlatformColor(HighlightColor(rawValue: a.color)?.color ?? HighlightColor.yellow.color).withAlphaComponent(0.45)
            for line in sel.selectionsByLine() {
                let b = line.bounds(for: page)
                guard b.width > 0.5, b.height > 0.5 else { continue }
                let h = PDFAnnotation(bounds: b, forType: .highlight, withProperties: nil)
                h.color = color
                h.userName = "lunta:" + a.id
                if !a.note.isEmpty { h.contents = a.note }
                page.addAnnotation(h)
            }
        }
    }

    /// Scrolls to a highlight (from the list of highlights).
    func show(_ a: Annotation) {
        guard let v = view, let doc = v.document, let (page, range) = locate(a, in: doc),
              let sel = page.selection(for: range) else { return }
        v.go(to: sel)
        v.setCurrentSelection(sel, animate: true)
    }

    private func text(_ doc: PDFDocument, _ i: Int) -> (norm: NSString, map: [Int])? {
        if let t = pageText[i] { return t }
        guard let s = doc.page(at: i)?.string else { return nil }
        let t = Self.normalize(s as NSString)
        pageText[i] = t
        return t
    }

    /// The page and the range in its text where a highlight's quote is (the occurrence whose
    /// preceding text matches best).
    private func locate(_ a: Annotation, in doc: PDFDocument) -> (PDFPage, NSRange)? {
        var quote = Self.normalize(a.quote as NSString).norm as String
        quote = quote.trimmingCharacters(in: .whitespaces)
        guard quote.count >= 3 else { return nil }
        let before = String((Self.normalize(a.prefix as NSString).norm as String).suffix(16)).lowercased()
        // a long quote may run onto the next page: look for its start
        let candidates = quote.count > 140 ? [quote, String(quote.prefix(100))] : [quote]
        for q in candidates {
            var best: (Int, NSRange, Int)?   // page, range in normalized text, score
            for i in 0..<doc.pageCount {
                guard let t = text(doc, i) else { continue }
                var from = 0
                while from < t.norm.length {
                    let r = t.norm.range(of: q, options: [.caseInsensitive], range: NSRange(location: from, length: t.norm.length - from))
                    if r.location == NSNotFound { break }
                    var score = 1
                    if !before.isEmpty {
                        let start = max(0, r.location - before.count - 2)
                        let pre = t.norm.substring(with: NSRange(location: start, length: r.location - start)).lowercased()
                        if pre.hasSuffix(before) { score += 2 }
                    }
                    if best == nil || score > best!.2 { best = (i, r, score) }
                    from = r.location + max(1, r.length)
                }
            }
            if let (i, r, _) = best, let t = text(doc, i), let page = doc.page(at: i) {
                let start = t.map[r.location]
                let end = t.map[r.location + r.length - 1] + 1
                return (page, NSRange(location: start, length: end - start))
            }
        }
        return nil
    }

    /// The text with all whitespace left out (the reading view and the PDF space things differently:
    /// "( Figure 4C )" vs "(Figure 4C)"), words broken across lines ("tran-\nscription") joined,
    /// ligatures spelled out and look-alike characters folded (µ/μ, curly quotes, dashes). `map`
    /// gives, for each unit of the result, its index in the original text.
    nonisolated static func normalize(_ s: NSString) -> (norm: NSString, map: [Int]) {
        var out: [unichar] = [], map: [Int] = []
        out.reserveCapacity(s.length); map.reserveCapacity(s.length)
        let ligatures: [unichar: String] = [0xFB00: "ff", 0xFB01: "fi", 0xFB02: "fl", 0xFB03: "ffi", 0xFB04: "ffl"]
        let fold: [unichar: unichar] = [0x00B5: 0x03BC, 0x2018: 39, 0x2019: 39, 0x201C: 34, 0x201D: 34,
                                        0x2010: 45, 0x2011: 45, 0x2012: 45, 0x2013: 45, 0x2014: 45, 0x2212: 45]
        let n = s.length
        var i = 0
        func isSpace(_ c: unichar) -> Bool { c == 32 || c == 9 || c == 10 || c == 13 || c == 0xA0 || c == 0x2028 }
        while i < n {
            let c = s.character(at: i)
            if c == 0x2D || c == 0xAD {   // a hyphen at the end of a line followed by a lowercase letter
                var j = i + 1, newline = false
                while j < n, isSpace(s.character(at: j)) { if s.character(at: j) == 10 || s.character(at: j) == 13 { newline = true }; j += 1 }
                if newline, j < n, let u = UnicodeScalar(s.character(at: j)), CharacterSet.lowercaseLetters.contains(u) {
                    i = j
                    continue
                }
            }
            if isSpace(c) || c == 0x200B {
                i += 1
                continue
            }
            if let lig = ligatures[c] {
                for u in lig.utf16 { out.append(u); map.append(i) }
            } else {
                out.append(fold[c] ?? c); map.append(i)
            }
            i += 1
        }
        return (NSString(characters: out, length: out.count), map)
    }

    // MARK: making highlights

    /// The selected text, with a little text before and after it, as the reading view stores it.
    func selection() -> PageSelection? {
        guard let v = view, let doc = v.document, let sel = v.currentSelection, let page = sel.pages.first,
              let raw = sel.string, !raw.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return nil }
        let pageString = (page.string ?? "") as NSString
        let r = sel.range(at: 0, on: page)
        // stored readably (spaces kept), with words broken across lines joined
        func clean(_ s: String) -> String {
            s.replacingOccurrences(of: #"[-\u00AD][ \t]*[\r\n]+\s*(?=\p{Ll})"#, with: "", options: .regularExpression)
             .replacingOccurrences(of: #"\s+"#, with: " ", options: .regularExpression)
        }
        var prefix = "", suffix = ""
        if r.location != NSNotFound, r.location <= pageString.length {
            let p0 = max(0, r.location - 60)
            prefix = clean(pageString.substring(with: NSRange(location: p0, length: r.location - p0)))
            let e = min(pageString.length, r.location + r.length)
            suffix = clean(pageString.substring(with: NSRange(location: e, length: min(60, pageString.length - e))))
        }
        // roughly where it falls in the paper's text (the reading view uses it to break ties)
        var start = 0
        let pi = doc.index(for: page)
        for i in 0..<pi { start += doc.page(at: i)?.string?.count ?? 0 }
        if r.location != NSNotFound { start += r.location }
        return PageSelection(quote: clean(raw).trimmingCharacters(in: .whitespaces),
                             prefix: String(prefix.suffix(40)), suffix: String(suffix.prefix(40)), start: start)
    }

    func clearSelection() {
        view?.clearSelection()
        hasSelection = false
    }
}

#if os(macOS)
private final class ClickablePDFView: PDFView {
    var onClick: ((CGPoint) -> Bool)?
    override func mouseUp(with event: NSEvent) {
        super.mouseUp(with: event)
        if event.clickCount == 1 { _ = onClick?(convert(event.locationInWindow, from: nil)) }
    }
}

struct HighlightablePDFView: NSViewRepresentable {
    let url: URL
    let highlighter: PDFHighlighter
    func makeNSView(context: Context) -> PDFView {
        let v = ClickablePDFView()
        v.autoScales = true
        v.displayMode = .singlePageContinuous
        v.document = PDFDocument(url: url)
        v.onClick = { [weak highlighter] p in highlighter?.handleTap(at: p) ?? false }
        highlighter.attach(v)
        return v
    }
    func updateNSView(_ v: PDFView, context: Context) {
        if v.document?.documentURL != url { v.document = PDFDocument(url: url) }
    }
}
#else
struct HighlightablePDFView: UIViewRepresentable {
    let url: URL
    let highlighter: PDFHighlighter
    func makeUIView(context: Context) -> PDFView {
        let v = PDFView()
        v.autoScales = true
        v.displayMode = .singlePageContinuous
        v.document = PDFDocument(url: url)
        let tap = UITapGestureRecognizer(target: context.coordinator, action: #selector(Coordinator.tapped(_:)))
        tap.cancelsTouchesInView = false
        tap.delegate = context.coordinator
        v.addGestureRecognizer(tap)
        context.coordinator.highlighter = highlighter
        highlighter.attach(v)
        return v
    }
    func updateUIView(_ v: PDFView, context: Context) {}
    func makeCoordinator() -> Coordinator { Coordinator() }

    final class Coordinator: NSObject, UIGestureRecognizerDelegate {
        weak var highlighter: PDFHighlighter?
        @MainActor @objc func tapped(_ g: UITapGestureRecognizer) {
            guard let v = g.view else { return }
            _ = highlighter?.handleTap(at: g.location(in: v))
        }
        func gestureRecognizer(_ g: UIGestureRecognizer, shouldRecognizeSimultaneouslyWith other: UIGestureRecognizer) -> Bool { true }
    }
}
#endif

/// The PDF with its highlights, a bar for highlighting the selected text, and tapping a highlight
/// to edit it.
struct PDFReader: View {
    @EnvironmentObject var model: LibraryModel
    let url: URL
    let paperID: String
    @ObservedObject var highlighter: PDFHighlighter
    var onEdit: (Annotation) -> Void

    var body: some View {
        HighlightablePDFView(url: url, highlighter: highlighter)
            .overlay(alignment: .bottom) {
                if highlighter.hasSelection {
                    PDFHighlightBar { color, note in highlight(color, note: note) }
                        .padding(.bottom, 18)
                        .transition(.move(edge: .bottom).combined(with: .opacity))
                }
            }
            .animation(.easeOut(duration: 0.15), value: highlighter.hasSelection)
            .onAppear {
                highlighter.onTap = { id in if let a = model.annotations(paperID).first(where: { $0.id == id }) { onEdit(a) } }
                // after the document has loaded its pages
                DispatchQueue.main.async { highlighter.apply(model.annotations(paperID)) }
            }
            .onReceive(model.objectWillChange.debounce(for: .milliseconds(250), scheduler: RunLoop.main)) { _ in
                highlighter.apply(model.annotations(paperID))
            }
    }

    private func highlight(_ color: HighlightColor, note: Bool) {
        guard let sel = highlighter.selection() else { return }
        let a = model.addHighlight(paperID, sel, color: color, location: "pdf")
        highlighter.clearSelection()
        highlighter.apply(model.annotations(paperID))
        if note, let a { onEdit(a) }
    }
}

/// Highlight colours and Note, shown while text in the PDF is selected.
struct PDFHighlightBar: View {
    let action: (HighlightColor, Bool) -> Void
    var body: some View {
        HStack(spacing: 12) {
            ForEach(HighlightColor.allCases) { c in
                Button { action(c, false) } label: {
                    Circle().fill(c.color).frame(width: 24, height: 24)
                        .overlay(Circle().strokeBorder(.black.opacity(0.12)))
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Highlight \(c.name)")
                .help("Highlight (\(c.name.lowercased()))")
            }
            Divider().frame(height: 22)
            Button { action(.yellow, true) } label: { Label("Note", systemImage: "note.text") }
                .buttonStyle(.plain).font(.callout.weight(.medium))
                .help("Highlight and add a note")
        }
        .padding(.horizontal, 16).padding(.vertical, 10)
        .background(.regularMaterial, in: Capsule())
        .shadow(color: .black.opacity(0.15), radius: 8, y: 2)
    }
}
