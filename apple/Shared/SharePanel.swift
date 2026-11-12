import SwiftUI
import LuntaCore
#if os(macOS)
import AppKit
#else
import UIKit
#endif

/// Copies text to the clipboard on either platform.
func copyText(_ s: String) {
    #if os(macOS)
    NSPasteboard.general.clearContents()
    NSPasteboard.general.setString(s, forType: .string)
    #else
    UIPasteboard.general.string = s
    #endif
}

/// Share papers: their PDFs and reading views (with readable file names), citations, links and bibliographies.
struct SharePanel: View {
    @EnvironmentObject var model: LibraryModel
    @Environment(\.dismiss) private var dismiss
    let ids: [String]
    @State private var pdfs: [URL] = []
    @State private var epubs: [URL] = []
    @State private var bib: URL?
    @State private var preparing = true
    @State private var copied: String?

    private var papers: [Paper] { ids.compactMap { model.paper($0) } }
    private var one: Paper? { papers.count == 1 ? papers.first : nil }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack {
                Text(one.map { $0.meta.title } ?? "\(papers.count) papers").font(.headline).lineLimit(2)
                Spacer()
                #if os(iOS)
                Button("Done") { dismiss() }
                #endif
            }
            group("Files") {
                if preparing {
                    HStack(spacing: 8) { ProgressView().controlSize(.small); Text("Getting the files ready…").foregroundStyle(.secondary) }
                } else {
                    if !pdfs.isEmpty {
                        ShareLink(items: pdfs) { row(pdfs.count == 1 ? "PDF" : "\(pdfs.count) PDFs", "doc.richtext", pdfs.count == 1 ? pdfs[0].lastPathComponent : nil) }
                    }
                    if !epubs.isEmpty {
                        ShareLink(items: epubs) { row(epubs.count == 1 ? "Reading view (EPUB)" : "\(epubs.count) reading views (EPUB)", "book", "Opens in Apple Books and other e-readers") }
                    }
                    if pdfs.isEmpty && epubs.isEmpty { Text("No PDF or reading view yet.").foregroundStyle(.secondary) }
                }
            }
            group("Citation") {
                if let one {
                    ShareLink(item: model.citation(one.id, .apa)) { row("Share citation (APA)", "quote.opening", nil) }
                    HStack(spacing: 6) {
                        copyButton("APA") { model.citation(one.id, .apa) }
                        copyButton("Vancouver") { model.citation(one.id, .vancouver) }
                        copyButton("Nature") { model.citation(one.id, .nature) }
                    }
                }
                HStack(spacing: 6) {
                    copyButton("BibTeX") { model.bibtex(ids) }
                    copyButton("RIS") { model.ris(ids) }
                    copyButton("Markdown") { markdown }
                    if papers.count > 1 { copyButton("APA list") { papers.map { model.citation($0.id, .apa) }.joined(separator: "\n\n") } }
                }
                if let bib {
                    ShareLink(item: bib) { row("Bibliography file (.bib)", "doc.text", bib.lastPathComponent) }
                }
            }
            let links = papers.compactMap(link)
            if !links.isEmpty {
                group("Link") {
                    ShareLink(items: links) { row(links.count == 1 ? "Share link" : "Share \(links.count) links", "link", links.count == 1 ? links[0].absoluteString : nil) }
                    copyButton(links.count == 1 ? "Copy link" : "Copy links") { links.map(\.absoluteString).joined(separator: "\n") }
                }
            }
            if let copied {
                Label("Copied \(copied)", systemImage: "checkmark.circle.fill").font(.caption).foregroundStyle(Color.luntaTeal)
            }
        }
        .padding(18)
        #if os(macOS)
        .frame(width: 400)
        #endif
        .buttonStyle(.plain)
        .task { await prepare() }
    }

    private func group<C: View>(_ title: String, @ViewBuilder _ content: () -> C) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(title.uppercased()).font(.caption2.weight(.semibold)).foregroundStyle(.secondary)
            content()
        }
    }

    private func row(_ title: String, _ icon: String, _ detail: String?) -> some View {
        HStack(spacing: 10) {
            Image(systemName: icon).frame(width: 22).foregroundStyle(Color.luntaTeal)
            VStack(alignment: .leading, spacing: 1) {
                Text(title).foregroundStyle(.primary)
                if let detail { Text(detail).font(.caption).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle) }
            }
            Spacer()
            Image(systemName: "square.and.arrow.up").foregroundStyle(.secondary)
        }
        .padding(.vertical, 6).padding(.horizontal, 10)
        .background(.quaternary.opacity(0.5), in: RoundedRectangle(cornerRadius: 8))
        .contentShape(Rectangle())
    }

    private func copyButton(_ title: String, _ text: @escaping () -> String) -> some View {
        Button {
            copyText(text())
            copied = title
        } label: {
            Text(title).font(.callout).padding(.horizontal, 10).padding(.vertical, 5)
                .background(Color.luntaTeal.opacity(0.12), in: Capsule())
                .foregroundStyle(Color.luntaTeal)
        }
    }

    private func link(_ p: Paper) -> URL? {
        if !p.meta.doi.isEmpty { return URL(string: "https://doi.org/\(p.meta.doi)") }
        return URL(string: p.meta.url).flatMap { $0.scheme?.hasPrefix("http") == true ? $0 : nil }
    }

    private var markdown: String {
        papers.map { p in
            let first = p.meta.authors.first.map { $0.split(separator: " ").last.map(String.init) ?? $0 } ?? "Unknown"
            let who = p.meta.authors.count > 2 ? "\(first) et al." : p.meta.authors.count == 2 ? "\(first) & \(p.meta.authors[1].split(separator: " ").last.map(String.init) ?? "")" : first
            let label = "\(who), \(p.meta.year)"
            if let l = link(p) { return "[\(label)](\(l.absoluteString)) — \(p.meta.title)" }
            return "\(label) — \(p.meta.title)"
        }.joined(separator: "\n")
    }

    /// Copies files under readable names ("Albanese 2016 - Cerebrospinal fluid lactate….pdf"); APFS makes these copies instant.
    private func prepare() async {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("Lunta Share", isDirectory: true)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        var p: [URL] = [], e: [URL] = []
        for paper in papers {
            let name = Self.fileName(paper)
            if paper.hasPdf, let src = await model.ensureLocal(paper.id, .pdf) { if let u = Self.copy(src, to: dir, name: name + ".pdf") { p.append(u) } }
            if paper.hasEpub, let src = await model.ensureLocal(paper.id, .epub) { if let u = Self.copy(src, to: dir, name: name + ".epub") { e.append(u) } }
        }
        let b = dir.appendingPathComponent(papers.count == 1 ? Self.fileName(papers[0]) + ".bib" : "Lunta bibliography (\(papers.count) papers).bib")
        try? model.bibtex(ids).write(to: b, atomically: true, encoding: .utf8)
        pdfs = p
        epubs = e
        bib = b
        preparing = false
    }

    static func fileName(_ p: Paper) -> String {
        let surname = p.meta.authors.first.map { a -> String in
            if a.contains(",") { return String(a.split(separator: ",")[0]) }
            return a.split(separator: " ").last.map(String.init) ?? a
        } ?? ""
        let head = [surname, p.meta.year].filter { !$0.isEmpty }.joined(separator: " ")
        var title = p.meta.title
        if title.count > 70 { title = String(title.prefix(70)).trimmingCharacters(in: .whitespaces) + "…" }
        let raw = head.isEmpty ? title : "\(head) - \(title)"
        let bad = CharacterSet(charactersIn: "/\\:?%*|\"<>").union(.newlines).union(.controlCharacters)
        return raw.components(separatedBy: bad).joined(separator: " ").trimmingCharacters(in: .whitespaces)
    }

    private static func copy(_ src: URL, to dir: URL, name: String) -> URL? {
        let dest = dir.appendingPathComponent(name)
        try? FileManager.default.removeItem(at: dest)
        do { try FileManager.default.copyItem(at: src, to: dest); return dest } catch { return nil }
    }
}
