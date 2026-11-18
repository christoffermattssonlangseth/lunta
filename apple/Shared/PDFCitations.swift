import SwiftUI
import PDFKit
import LuntaCore
#if os(macOS)
import AppKit
#else
import UIKit
#endif

/// Citations in the PDF view, like the citation card in the reading view (CitationPopover.swift):
/// clicking a citation shows the reference in a card next to it instead of jumping to the reference
/// list, with its DOI (printed in it, or found by Crossref), Add to Lunta / Open in Lunta, Copy and
/// Go to Reference.
///
/// A citation is a link into the reference list (the reference is the entry at the link's
/// destination), or, in PDFs without links, "[12]", a superscript number or "(Smith et al., 2020)"
/// under the click (the entry with that number, or that first author and year, after the
/// reference list's heading). The core finds the entry in the page's lines (`pdfReferenceAt`,
/// `pdfReferenceForCitation`: core/src/refcite.rs); a link that goes elsewhere (a section, a
/// figure) jumps as before.
@MainActor
final class PDFCitations: ObservableObject {
    struct Card: Identifiable {
        let id = UUID()
        /// the reference as printed
        let text: String
        /// the citation, in the PDF view's coordinates (y down), for the card's arrow
        let anchor: CGRect
        /// where the reference is, for Go to Reference
        let destination: PDFDestination?
    }

    @Published var card: Card?
    weak var view: PDFView?
    private weak var document: PDFDocument?
    private var pageLines: [Int: [TextLine]] = [:]
    private var refsStart: Int?

    func attach(_ v: PDFView) { view = v }

    // MARK: the page's lines

    /// A page's lines with where they are (from the top left of its media box), for the core.
    private func lines(_ page: PDFPage) -> [TextLine] {
        guard let doc = page.document else { return [] }
        if doc !== document { document = doc; pageLines = [:]; refsStart = nil }
        let i = doc.index(for: page)
        if let l = pageLines[i] { return l }
        let box = page.bounds(for: .mediaBox)
        let out: [TextLine] = (page.selection(for: box)?.selectionsByLine() ?? []).compactMap { s in
            guard let t = s.string?.trimmingCharacters(in: .whitespacesAndNewlines), !t.isEmpty else { return nil }
            let r = s.bounds(for: page)
            return TextLine(page: UInt32(i), text: t, left: Double(r.minX - box.minX), top: Double(box.maxY - r.maxY),
                            right: Double(r.maxX - box.minX), bottom: Double(box.maxY - r.minY))
        }
        pageLines[i] = out
        return out
    }

    /// The first page of the reference list: the last "References" / "Bibliography" heading's page
    /// (else the second half of the PDF).
    private func referencesStart(_ doc: PDFDocument) -> Int {
        if doc === document, let s = refsStart { return s }
        var start = doc.pageCount / 2
        let heading = /^\s*([0-9IVX]+\.?\s+)?(references|bibliography|literature cited|references and notes|works cited)\s*:?\s*$/.ignoresCase()
        search: for word in ["References", "Bibliography", "Literature Cited"] {
            for m in doc.findString(word, withOptions: .caseInsensitive).reversed() {
                guard let page = m.pages.first else { continue }
                let b = m.bounds(for: page)
                let line = page.selectionForLine(at: CGPoint(x: b.midX, y: b.midY))?.string ?? ""
                if line.wholeMatch(of: heading) != nil { start = doc.index(for: page); break search }
            }
        }
        document = doc
        refsStart = start
        return start
    }

    /// From page coordinates to the view's, y down (what SwiftUI anchors the card to).
    private func viewRect(_ r: CGRect, on page: PDFPage) -> CGRect {
        guard let v = view else { return .zero }
        var out = v.convert(r, from: page)
        #if os(macOS)
        if !v.isFlipped { out.origin.y = v.bounds.height - out.maxY }
        #endif
        return out
    }

    // MARK: clicks

    /// A link: when it goes into the reference list, the card for that entry. Returns whether it
    /// did (else the link jumps as usual).
    func showLink(_ dest: PDFDestination, at rect: CGRect, on page: PDFPage) -> Bool {
        guard let target = dest.page, let doc = target.document else { return false }
        let p = dest.point
        let none = kPDFDestinationUnspecifiedValue
        guard p.y != none else { return false }
        let box = target.bounds(for: .mediaBox)
        let i = doc.index(for: target)
        var ls = lines(target)
        if i + 1 < doc.pageCount, let next = doc.page(at: i + 1) { ls += lines(next) }
        let x: Double? = p.x == none ? nil : Double(p.x - box.minX)
        guard let text = pdfReferenceAt(lines: ls, page: UInt32(i), x: x, y: Double(box.maxY - p.y)) else { return false }
        card = Card(text: text, anchor: viewRect(rect, on: page), destination: dest)
        return true
    }

    /// A click on text (view coordinates): "[12]", a superscript number or "(Smith et al., 2020)"
    /// there shows the card for that reference. Returns whether it did.
    func showCitation(at point: CGPoint) -> Bool {
        guard let v = view, v.currentSelection?.string?.isEmpty ?? true,
              let page = v.page(for: point, nearest: false), let doc = page.document, let all = page.string else { return false }
        let p = v.convert(point, to: page)
        // a link there is the link's business
        if page.link(at: p) != nil { return false }
        let ci = page.characterIndex(at: p)
        guard ci >= 0, let line = page.selectionForLine(at: p) else { return false }
        let range = line.range(at: 0, on: page)
        let ns = all as NSString
        guard range.location != NSNotFound, range.length > 0, NSMaxRange(range) <= ns.length,
              ci >= range.location, ci < NSMaxRange(range) else { return false }
        let text = ns.substring(with: range)
        let offset = UInt32(ci - range.location)
        guard pdfIsCitationAt(text: text, offset: offset) else { return false }
        let start = referencesStart(doc)
        var ls: [TextLine] = []
        for k in start..<doc.pageCount { if let pg = doc.page(at: k) { ls += lines(pg) } }
        guard let ref = pdfReferenceForCitation(lines: ls, text: text, offset: offset) else { return false }
        let word = page.selectionForWord(at: p)?.bounds(for: page) ?? CGRect(origin: p, size: .zero)
        card = Card(text: ref, anchor: viewRect(word, on: page), destination: destination(of: ref, from: start, in: doc))
        return true
    }

    /// Where an entry is: its first words, found in the reference pages.
    private func destination(of ref: String, from start: Int, in doc: PDFDocument) -> PDFDestination? {
        let words = ref.split(whereSeparator: \.isWhitespace).prefix(4).joined(separator: " ")
        guard words.count >= 6 else { return nil }
        let hit = doc.findString(words, withOptions: []).first { m in m.pages.first.map { doc.index(for: $0) >= start } ?? false }
        guard let hit, let page = hit.pages.first else { return nil }
        let b = hit.bounds(for: page)
        return PDFDestination(page: page, at: CGPoint(x: b.minX, y: b.maxY + 4))
    }

    /// Go to Reference: the PDF jumps there (Back in the PDF view returns).
    func goToReference(_ c: Card) {
        card = nil
        guard let v = view, let d = c.destination else { return }
        #if os(iOS)
        (v as? CitingPDFView)?.goDirectly(to: d)
        #else
        v.go(to: d)
        #endif
    }
}

extension PDFPage {
    /// The link at a point on the page (under a highlight drawn over it, too).
    func link(at p: CGPoint) -> PDFAnnotation? {
        annotations.last { $0.type == "Link" && $0.bounds.contains(p) }
    }
}

#if os(iOS)
/// The iPhone and iPad PDF view: a tapped link into the reference list shows the citation card
/// instead of jumping (PDFKit follows links itself; this sees the jump it asks for).
final class CitingPDFView: PDFView {
    /// the last touch and when, from the tap recogniser (a jump right after it is a tapped link)
    var lastTouch: (point: CGPoint, time: Date)?
    var onLink: ((PDFDestination, CGPoint) -> Bool)?

    override func go(to destination: PDFDestination) {
        if let t = lastTouch, Date().timeIntervalSince(t.time) < 1, let onLink {
            lastTouch = nil
            if onLink(destination, t.point) { return }
        }
        super.go(to: destination)
    }

    func goDirectly(to destination: PDFDestination) {
        lastTouch = nil
        super.go(to: destination)
    }
}
#endif

/// The card: the reference, its DOI, and what can be done with it.
struct PDFCitationCard: View {
    @EnvironmentObject var model: LibraryModel
    let card: PDFCitations.Card
    var goTo: () -> Void
    var open: (String) -> Void
    @State private var ids: ReferenceIds?
    @State private var looking = true
    @State private var inLibrary: String?
    @State private var adding = false
    @State private var note: String?
    @State private var copied = false
    @Environment(\.openURL) private var openURL

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(card.text)
                .font(.callout)
                .textSelection(.enabled)
                .lineLimit(14)
                .fixedSize(horizontal: false, vertical: true)
            status
            HStack(spacing: 8) {
                if let id = inLibrary {
                    Button("Open in Lunta") { open(id) }.buttonStyle(.borderedProminent)
                } else if let ids {
                    Button(adding ? "Adding…" : "Add to Lunta") { add(ids) }
                        .buttonStyle(.borderedProminent).disabled(adding)
                }
                if let url = paperURL { Button("Open Paper") { openURL(url) } }
                if !looking, ids == nil, let url = scholarURL { Button("Search") { openURL(url) } }
                Button(copied ? "Copied" : "Copy") { copy() }
                if card.destination != nil { Button("Go to Reference") { goTo() } }
            }
            .controlSize(.small)
        }
        .padding(14)
        .frame(width: 380)
        .task { await lookUp() }
    }

    @ViewBuilder private var status: some View {
        if looking {
            HStack(spacing: 6) { ProgressView().controlSize(.mini); Text("Looking up the DOI…") }
                .font(.caption).foregroundStyle(.secondary)
        } else if let note {
            Text(note).font(.caption).foregroundStyle(.secondary)
        } else if let ids, !ids.doi.isEmpty {
            Text("DOI \(ids.doi)").font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
        } else if let ids, !ids.arxiv.isEmpty {
            Text("arXiv \(ids.arxiv)").font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
        } else {
            Text("No DOI found for this reference.").font(.caption).foregroundStyle(.secondary)
        }
    }

    private var paperURL: URL? {
        guard let ids else { return nil }
        if !ids.doi.isEmpty { return URL(string: "https://doi.org/\(ids.doi)") }
        if !ids.arxiv.isEmpty { return URL(string: "https://arxiv.org/abs/\(ids.arxiv)") }
        return nil
    }

    private var scholarURL: URL? {
        var c = URLComponents(string: "https://scholar.google.com/scholar")
        c?.queryItems = [URLQueryItem(name: "q", value: String(card.text.prefix(200)))]
        return c?.url
    }

    private func lookUp() async {
        let found = await LibraryModel.lookUpReference(card.text)
        ids = found
        if let found { inLibrary = model.paperInLibrary(doi: found.doi, arxiv: found.arxiv) }
        looking = false
    }

    private func add(_ ids: ReferenceIds) {
        adding = true
        Task {
            let r = await model.paperFor(doi: ids.doi, arxiv: ids.arxiv)
            adding = false
            inLibrary = r.id
            note = r.label
        }
    }

    private func copy() {
        #if os(macOS)
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(card.text, forType: .string)
        #else
        UIPasteboard.general.string = card.text
        #endif
        copied = true
    }
}
