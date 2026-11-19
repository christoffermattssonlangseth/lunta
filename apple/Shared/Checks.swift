import SwiftUI
import LuntaCore
import Paper2EpubKit
import PDFKit
import UniformTypeIdentifiers

// Checks against the published record (core/src/checks), the apps' part:
//
// - Check a Reference List: paste a list, or choose a .bib / .ris / text file (Mac: File › Check a
//   Reference List…; iPhone and iPad: + › Check a Reference List), a folder's papers (its menu:
//   Check References), or a paper's own reference list (its menu: Check This Paper's References:
//   the reading view's list, else the PDF's). A sheet shows the report; Share, Save as Markdown
//   or CSV.
// - Retractions, corrections and published versions of preprints: checked weekly in the
//   background and on demand (a paper's menu, the library's File menu). A badge in the list, a
//   banner in the reader ("This paper was retracted on … · See Notice"; "Published in … · Switch
//   to the Published Version…"), and the smart folders "Retracted or corrected" and "Published
//   since you added them".
//
// Lunta proposes, the researcher decides: nothing in the library changes until a choice is made
// (switching to the published version asks first, and can be undone).

/// A checked reference list, shown in a sheet.
struct RefCheckResult: Identifiable {
    let id = UUID()
    /// What was checked: "refs.bib", "“Thesis” (folder)", "the references of “…”".
    let title: String
    let report: RefCheckReport
}

/// What a check of the library found, shown in a sheet.
struct UpdatesResult: Identifiable {
    let id = UUID()
    let summary: UpdatesSummary
}

/// The menus' way to the checks (the input sheet), without needing the library model.
@MainActor
final class ChecksUI: ObservableObject {
    static let shared = ChecksUI()
    @Published var askingForList = false
}

extension LibraryModel {
    nonisolated static var checksCacheDir: String { cacheURL.path }

    // MARK: Check a reference list

    func checkReferences(text: String, title: String) async {
        let n = countReferences(text: text)
        guard n > 0 else { errorMessage = "No references found in \(title)."; return }
        status = "Checking \(n) reference\(n == 1 ? "" : "s")…"
        let listener = BibImportProgress { [weak self] m in Task { @MainActor in self?.status = m } }
        let lib = self.lib
        let report = await Task.detached {
            lib.markReferencesInLibrary(report: LuntaCore.checkReferences(text: text, cacheDir: Self.checksCacheDir, progress: listener))
        }.value
        status = nil
        refCheck = RefCheckResult(title: title, report: report)
    }

    func checkReferences(file: URL) async {
        do {
            let text = try await Task.detached { try ImportSource.coordinated(file) { try String(contentsOf: $0, encoding: .utf8) } }.value
            await checkReferences(text: text, title: file.lastPathComponent)
        } catch {
            errorMessage = "Couldn't read “\(file.lastPathComponent)”: \(error.readable)"
        }
    }

    /// A folder's papers (nil: the whole library).
    func checkFolderReferences(_ collection: String?, name: String) async {
        status = "Checking the references in “\(name)”…"
        let listener = BibImportProgress { [weak self] m in Task { @MainActor in self?.status = m } }
        let lib = self.lib
        let report = await Task.detached { lib.checkFolderReferences(collection: collection, progress: listener) }.value
        status = nil
        refCheck = RefCheckResult(title: "“\(name)”", report: report)
    }

    /// A paper's own reference list: its reading view's, else its PDF's.
    func checkPaperReferences(_ id: String) async {
        guard let p = lib.paper(id: id) else { return }
        let title = "the references of “\(p.meta.title.prefix(60))\(p.meta.title.count > 60 ? "…" : "")”"
        status = "Reading the reference list…"
        if p.hasEpub { _ = await ensureLocal(id, .epub) }
        let lib = self.lib
        var entries = await Task.detached { lib.paperReferenceList(id: id) }.value
        if entries.isEmpty, p.hasPdf, let pdf = await ensureLocal(id, .pdf) {
            entries = await Task.detached { Self.pdfReferences(pdf) }.value
        }
        guard !entries.isEmpty else {
            status = nil
            errorMessage = "No reference list was found in this paper (its reading view or PDF)."
            return
        }
        status = "Checking \(entries.count) references…"
        let listener = BibImportProgress { [weak self] m in Task { @MainActor in self?.status = m } }
        let report = await Task.detached {
            lib.markReferencesInLibrary(report: checkReferenceEntries(entries: entries, cacheDir: Self.checksCacheDir, progress: listener))
        }.value
        status = nil
        refCheck = RefCheckResult(title: title, report: report)
    }

    /// The PDF's reference list (PDFKit's lines, the core's reading of them).
    nonisolated static func pdfReferences(_ url: URL) -> [String] {
        guard let doc = PDFDocument(url: url) else { return [] }
        var lines: [TextLine] = []
        for i in 0..<doc.pageCount {
            guard let page = doc.page(at: i) else { continue }
            let box = page.bounds(for: .mediaBox)
            for s in page.selection(for: box)?.selectionsByLine() ?? [] {
                guard let t = s.string?.trimmingCharacters(in: .whitespacesAndNewlines), !t.isEmpty else { continue }
                let r = s.bounds(for: page)
                lines.append(TextLine(page: UInt32(i), text: t, left: Double(r.minX - box.minX), top: Double(box.maxY - r.maxY),
                                      right: Double(r.maxX - box.minX), bottom: Double(box.maxY - r.minY)))
            }
        }
        return pdfReferenceList(lines: lines)
    }

    // MARK: Retractions, corrections, published versions

    /// Checks these papers (empty: the library) now, and shows what was found.
    func checkUpdates(_ ids: [String]) async {
        status = ids.count == 1 ? "Checking for retractions, corrections and a published version…" : "Checking for retractions, corrections and published versions…"
        let listener = BibImportProgress { [weak self] m in Task { @MainActor in self?.status = m } }
        let lib = self.lib
        do {
            let s = try await Task.detached { try lib.checkPaperUpdates(ids: ids, progress: listener) }.value
            status = nil
            reload()
            updatesReport = UpdatesResult(summary: s)
        } catch {
            status = nil
            errorMessage = "Couldn't check: \(error.readable)"
        }
    }

    /// The weekly check, in the background: now (if due) and every few hours while the app runs.
    func scheduleUpdateChecks() {
        guard Self.testLibrary == nil else { return }
        func run() {
            let lib = self.lib
            Task.detached(priority: .background) {
                guard let s = try? lib.checkPaperUpdatesIfDue(progress: nil), !s.changed.isEmpty else { return }
                await MainActor.run {
                    self.reload()
                    let retracted = s.changed.filter { s.retracted.contains($0) }.count
                    let published = s.changed.filter { s.published.contains($0) }.count
                    var parts: [String] = []
                    if retracted > 0 { parts.append("\(retracted) paper\(retracted == 1 ? " was" : "s were") retracted") }
                    if published > 0 { parts.append("\(published) preprint\(published == 1 ? " has" : "s have") been published") }
                    if !parts.isEmpty { self.status = parts.joined(separator: "; ").capitalizedFirst + " (see the smart folders)." }
                    Task { try? await Task.sleep(nanoseconds: 8_000_000_000); if self.status?.hasSuffix("(see the smart folders).") == true { self.status = nil } }
                }
            }
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + 60) { run() }
        Timer.scheduledTimer(withTimeInterval: 6 * 3600, repeats: true) { _ in Task { @MainActor in run() } }
    }

    /// "Switch to the Published Version" (after the reader confirmed it).
    func usePublishedVersion(_ id: String) async -> Bool {
        status = "Switching to the published version…"
        let lib = self.lib
        do {
            _ = try await Task.detached { try lib.usePublishedVersion(id: id) }.value
            status = nil
            reload()
            return true
        } catch {
            status = nil
            errorMessage = "Couldn't switch: \(error.readable)"
            return false
        }
    }

    func undoPublishedVersion(_ id: String) {
        do { _ = try lib.undoPublishedVersion(id: id) } catch { errorMessage = "Couldn't undo: \(error.readable)" }
        reload()
    }

    func linkPublishedVersion(_ id: String) { try? lib.linkPublishedVersion(id: id); reload() }
    func keepPreprint(_ id: String) { try? lib.keepPreprint(id: id); reload() }

    /// After switching: the published version's reading view and PDF, when it is open access.
    /// The reading view is replaced only if every highlight is found in the new text (else the
    /// new one is kept as a backup and the highlights' paper stays as it was); the PDF only if it
    /// has no highlights (they are placed on its pages). Backups go where "Use this page's full
    /// text" puts them.
    func fetchPublishedFullText(_ id: String) async -> String {
        guard let p = lib.paper(id: id), !p.meta.doi.isEmpty else { return "No DOI to look up." }
        let tmp = FileManager.default.temporaryDirectory.appendingPathComponent("lunta-published-\(UUID())")
        defer { try? FileManager.default.removeItem(at: tmp); status = nil }
        status = "Looking for the published version's full text…"
        let meta = p.meta
        // a legal open-access PDF of the published (or accepted) version
        let candidates = await Task.detached { findPdfCandidates(meta: meta) }.value.filter { $0.version != "preprint" }
        var pdf: Data?
        var pdfLabel = ""
        for c in candidates {
            guard let url = URL(string: c.url),
                  let got = try? await URLSession.shared.data(from: url), got.0.starts(with: Array("%PDF".utf8)) else { continue }
            pdf = got.0
            pdfLabel = c.label
            break
        }
        // its reading view: the open XML version, else the PDF
        var result = try? await Paper2Epub.convert(meta.doi, into: tmp.appendingPathComponent("xml"))
        if result == nil, let pdf {
            let f = tmp.appendingPathComponent("paper.pdf")
            try? FileManager.default.createDirectory(at: tmp, withIntermediateDirectories: true)
            try? pdf.write(to: f)
            result = try? await Paper2Epub.$knownDOI.withValue(meta.doi) { try await Paper2Epub.convert(file: f, into: tmp.appendingPathComponent("pdf")) }
        }
        guard result != nil || pdf != nil else { return "The published version isn't open access: the preprint's reading view and PDF stay." }
        let lib = self.lib
        let notes = lib.annotations(paper: id)
        var said: [String] = []
        let stamp = Self.backupStamp()
        let folder = Self.backupsURL.appendingPathComponent("\(id)-\(stamp)-preprint", isDirectory: true)
        try? FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)

        if let r = result, let epub = try? Data(contentsOf: r.file) {
            let anchored = notes.filter { $0.location == "epub" && !$0.quote.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }
            let readerText = r.readerText
            let unmatched = await Task.detached {
                let loc = HighlightLocator(text: readerText)
                return anchored.filter { loc.locate(HighlightAnchor(quote: $0.quote, prefix: $0.prefix, suffix: $0.suffix, start: Int($0.position) ?? 0)) == nil }
            }.value
            if p.hasEpub, !unmatched.isEmpty {
                try? epub.write(to: folder.appendingPathComponent("published-book.epub"))
                said.append("Kept the preprint's reading view: \(unmatched.count) of \(anchored.count) highlight\(anchored.count == 1 ? "" : "s") wouldn't be found in the published text (the published reading view is saved in \(Self.backupPlace(folder))).")
            } else {
                if p.hasEpub, let old = await ensureLocal(id, .epub), let d = try? Data(contentsOf: old) { try? d.write(to: folder.appendingPathComponent("book.epub")) }
                let text = Data(r.fulltext.utf8)
                try? await Task.detached { try lib.attachFile(id: id, kind: .epub, data: epub); try lib.attachFile(id: id, kind: .fulltext, data: text) }.value
                said.append(anchored.isEmpty ? "Reading view made from the published version." : "Reading view made from the published version; all \(anchored.count) highlight\(anchored.count == 1 ? "" : "s") found in it.")
            }
        }
        if let pdf {
            let pdfNotes = notes.filter { $0.location.hasPrefix("pdf") }
            if p.hasPdf, !pdfNotes.isEmpty {
                try? pdf.write(to: folder.appendingPathComponent("published.pdf"))
                said.append("Kept the preprint's PDF, which has highlights; the published PDF (\(pdfLabel)) is saved in \(Self.backupPlace(folder)).")
            } else {
                if p.hasPdf, let old = await ensureLocal(id, .pdf), let d = try? Data(contentsOf: old) { try? d.write(to: folder.appendingPathComponent("paper.pdf")) }
                try? await Task.detached { try lib.attachFile(id: id, kind: .pdf, data: pdf) }.value
                said.append("PDF of the published version attached (\(pdfLabel)).")
            }
        }
        if (try? FileManager.default.contentsOfDirectory(atPath: folder.path))?.isEmpty ?? true {
            try? FileManager.default.removeItem(at: folder)
        } else if !said.contains(where: { $0.contains(folder.lastPathComponent) }) {
            said.append("The preprint's files are kept in \(Self.backupPlace(folder)).")
        }
        reload()
        return said.joined(separator: " ")
    }
}

// MARK: - Badges and the banner

extension PaperUpdates {
    var level: NoticeLevel { updatesLevel(updates: self) }
    var mainNotice: EditorialNotice? {
        let rank: (EditorialNotice) -> Int = { n in
            switch updatesLevel(updates: PaperUpdates(notices: [n], published: nil, declinedDoi: "", linked: nil, preprint: nil, switchedAt: 0)) {
            case .retracted: 3
            case .concern: 2
            case .corrected: 1
            case .none: 0
            }
        }
        return notices.filter { rank($0) > 0 }.max { (rank($0), $0.date) < (rank($1), $1.date) }
    }
    /// For the smart folders.
    var flagged: Bool { level != .none }
    var publishedSinceAdded: Bool { published != nil || linked != nil || preprint != nil }
}

/// The list's mark for a paper the checks flagged.
struct UpdatesPill: View {
    let updates: PaperUpdates?
    var body: some View {
        if let u = updates {
            switch u.level {
            case .retracted: Pill(text: "Retracted", icon: "exclamationmark.octagon.fill", color: .red).help(updatesHeadline(updates: u))
            case .concern: Pill(text: "Concern", icon: "exclamationmark.triangle.fill", color: .orange).help(updatesHeadline(updates: u))
            case .corrected: Pill(text: "Corrected", icon: "pencil.circle", color: .orange).help(updatesHeadline(updates: u))
            case .none:
                if let v = u.published { Pill(text: "Published", icon: "arrow.up.forward.circle", color: .luntaTeal).help("Published in \(v.journal.isEmpty ? "a journal" : v.journal)") }
            }
        }
    }
}

/// The reader's banner: a retraction, correction or expression of concern, or a preprint's
/// published version with what can be done about it.
struct UpdatesBanner: View {
    @EnvironmentObject var model: LibraryModel
    @Environment(\.openURL) private var openURL
    let paperID: String
    @State private var hidden = false
    @State private var confirming = false
    @State private var offerFullText = false
    @State private var message: String?

    var body: some View {
        if !hidden, let u = model.paperUpdates[paperID], let content = content(u) {
            HStack(alignment: .firstTextBaseline, spacing: 10) {
                content
                Spacer(minLength: 0)
                Button { hidden = true } label: { Image(systemName: "xmark") }
                    .buttonStyle(.borderless).help("Hide")
            }
            .font(.callout)
            .padding(.horizontal, 14).padding(.vertical, 8)
            .background(tint(u).opacity(0.12))
            .overlay(alignment: .bottom) { Divider() }
            .confirmationDialog("Switch to the published version?", isPresented: $confirming, titleVisibility: .visible) {
                Button("Switch to the Published Version") {
                    Task {
                        if await model.usePublishedVersion(paperID) { offerFullText = true }
                    }
                }
                Button("Cancel", role: .cancel) {}
            } message: {
                Text("The paper's details become the published version's (\(u.published?.journal ?? "")). Highlights, notes, tags and the citation key stay. You can undo this.")
            }
            .alert("Get the published version's full text?", isPresented: $offerFullText) {
                Button("Get Full Text") { Task { message = await model.fetchPublishedFullText(paperID) } }
                Button("Not Now", role: .cancel) {}
            } message: {
                Text("If the published version is open access, Lunta makes its reading view and attaches its PDF. The preprint's reading view is replaced only if every highlight is found in the new text; a PDF with highlights is kept. The preprint's files are backed up.")
            }
            .alert("Published version", isPresented: Binding(get: { message != nil }, set: { if !$0 { message = nil } })) {
                Button("OK") { message = nil }
            } message: { Text(message ?? "") }
        }
    }

    private func tint(_ u: PaperUpdates) -> Color {
        switch u.level {
        case .retracted: .red
        case .concern, .corrected: .orange
        case .none: .luntaTeal
        }
    }

    private func content(_ u: PaperUpdates) -> AnyView? {
        if u.level != .none {
            let n = u.mainNotice
            return AnyView(HStack(alignment: .firstTextBaseline, spacing: 8) {
                Image(systemName: u.level == .retracted ? "exclamationmark.octagon.fill" : "exclamationmark.triangle.fill").foregroundStyle(tint(u))
                Text(updatesHeadline(updates: u)).fontWeight(.semibold)
                if let n, !n.doi.isEmpty, let url = URL(string: "https://doi.org/\(n.doi)") {
                    Button("See Notice") { openURL(url) }.buttonStyle(.borderless)
                }
            })
        }
        if let v = u.published {
            return AnyView(ViewThatFits(in: .horizontal) {
                HStack(alignment: .firstTextBaseline, spacing: 8) { published(v); actions(v) }
                VStack(alignment: .leading, spacing: 6) { published(v); HStack(spacing: 12) { actions(v) } }
            })
        }
        if let v = u.linked {
            return AnyView(HStack(spacing: 8) {
                Image(systemName: "link").foregroundStyle(Color.luntaTeal)
                Text("Published version: \(v.journal.isEmpty ? v.doi : v.journal)\(v.date.isEmpty ? "" : " (\(v.date.prefix(4)))")")
                if let url = URL(string: "https://doi.org/\(v.doi)") { Button("Open") { openURL(url) }.buttonStyle(.borderless) }
                Button("Switch…") { confirming = true }.buttonStyle(.borderless)
            })
        }
        if let pre = u.preprint, u.switchedAt > 0, Double(u.switchedAt) / 1000 > Date().timeIntervalSince1970 - 7 * 86400 {
            return AnyView(HStack(spacing: 8) {
                Image(systemName: "checkmark.circle").foregroundStyle(Color.luntaTeal)
                Text("Switched from the preprint (\(pre.journal.isEmpty ? pre.doi : pre.journal)).")
                Button("Undo") { model.undoPublishedVersion(paperID) }.buttonStyle(.borderless)
            })
        }
        return nil
    }

    private func published(_ v: PublishedVersion) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Image(systemName: "arrow.up.forward.circle.fill").foregroundStyle(Color.luntaTeal)
            Text("Published in \(v.journal.isEmpty ? "a journal" : v.journal)\(v.date.isEmpty ? "" : " (\(v.date.prefix(4)))").").fontWeight(.semibold)
                .help(v.title.isEmpty ? v.doi : "“\(v.title)”, \(v.doi) (found by \(v.foundBy))")
        }
    }

    @ViewBuilder private func actions(_ v: PublishedVersion) -> some View {
        Button("Switch to the Published Version…") { confirming = true }.buttonStyle(.borderless)
        Button("Link Both") { model.linkPublishedVersion(paperID) }.buttonStyle(.borderless)
            .help("Keep the preprint and show its published version with it")
        Button("Keep the Preprint") { model.keepPreprint(paperID) }.buttonStyle(.borderless)
        if let url = URL(string: "https://doi.org/\(v.doi)") { Button("Open") { openURL(url) }.buttonStyle(.borderless) }
    }
}

// MARK: - Sheets

/// The checks' sheets, on the library window (Mac) and the library (iPhone, iPad).
struct ReferenceChecks: ViewModifier {
    @EnvironmentObject var model: LibraryModel
    @ObservedObject private var ui = ChecksUI.shared

    func body(content: Content) -> some View {
        content
            .sheet(isPresented: $ui.askingForList) { CheckReferencesSheet() }
            .sheet(item: $model.refCheck) { r in RefCheckReportView(result: r) }
            .sheet(item: $model.updatesReport) { r in UpdatesReportView(result: r) }
            .onAppear { if !model.updateChecksScheduled { model.updateChecksScheduled = true; model.scheduleUpdateChecks() } }
            .onReceive(NotificationCenter.default.publisher(for: .luntaCheckLibrary)) { _ in Task { await model.checkUpdates([]) } }
    }
}

extension Notification.Name {
    /// Check the library for retractions and published versions now (iPhone and iPad's + menu).
    static let luntaCheckLibrary = Notification.Name("luntaCheckLibrary")
}

extension View {
    func referenceChecks() -> some View { modifier(ReferenceChecks()) }
}

/// Paste a reference list or choose a file.
struct CheckReferencesSheet: View {
    @EnvironmentObject var model: LibraryModel
    @Environment(\.dismiss) private var dismiss
    @State private var text = ""
    @State private var choosing = false

    var body: some View {
        let n = text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? 0 : Int(countReferences(text: text))
        NavigationStack {
            VStack(alignment: .leading, spacing: 10) {
                Text("Paste a reference list (numbered or author–year, from a manuscript or one you're reviewing), BibTeX or RIS, or choose a file. Each reference is looked up in Crossref, DataCite, OpenAlex and PubMed and its details compared with the record. Nothing is added to your library.")
                    .font(.callout).foregroundStyle(.secondary)
                TextEditor(text: $text)
                    .font(.callout.monospaced())
                    .frame(minHeight: 220)
                    .overlay(RoundedRectangle(cornerRadius: 6).stroke(.quaternary))
                HStack {
                    Button("Choose File…") { choosing = true }
                    Spacer()
                    Text(n == 0 ? "" : "\(n) reference\(n == 1 ? "" : "s")").foregroundStyle(.secondary)
                }
            }
            .padding()
            .navigationTitle("Check a Reference List")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Check") {
                        let t = text
                        dismiss()
                        Task { await model.checkReferences(text: t, title: "the pasted list") }
                    }
                    .disabled(n == 0)
                }
            }
            .fileImporter(isPresented: $choosing, allowedContentTypes: [UTType(filenameExtension: "bib"), UTType(filenameExtension: "ris"), .plainText].compactMap { $0 }) { r in
                guard case .success(let url) = r else { return }
                dismiss()
                Task {
                    let ok = url.startAccessingSecurityScopedResource()
                    defer { if ok { url.stopAccessingSecurityScopedResource() } }
                    await model.checkReferences(file: url)
                }
            }
        }
        #if os(macOS)
        .frame(minWidth: 560, minHeight: 420)
        #endif
    }
}

/// A plain-text file to save (the report as Markdown or CSV).
struct TextFileDocument: FileDocument {
    static var readableContentTypes: [UTType] { [.plainText] }
    var text: String
    init(_ text: String) { self.text = text }
    init(configuration: ReadConfiguration) throws { text = String(decoding: configuration.file.regularFileContents ?? Data(), as: UTF8.self) }
    func fileWrapper(configuration: WriteConfiguration) throws -> FileWrapper { FileWrapper(regularFileWithContents: Data(text.utf8)) }
}

/// The report: totals, then the references by result, each with what was found.
struct RefCheckReportView: View {
    @EnvironmentObject var model: LibraryModel
    @Environment(\.dismiss) private var dismiss
    @Environment(\.openURL) private var openURL
    let result: RefCheckResult
    @State private var saving: (doc: TextFileDocument, type: UTType, name: String)?

    private var r: RefCheckReport { result.report }

    var body: some View {
        NavigationStack {
            List {
                Section {
                    Text(r.summary).font(.callout)
                } footer: {
                    Text("Looked up by DOI (Crossref, DataCite) and by the details (Crossref, OpenAlex, PubMed). A reference that could not be found may still exist (books, reports, theses and very new papers are often missing): check it by hand.")
                }
                section("Retracted or with notices", r.items.filter { updatesLevel(updates: PaperUpdates(notices: $0.notices, published: nil, declinedDoi: "", linked: nil, preprint: nil, switchedAt: 0)) != .none }, icon: "exclamationmark.octagon.fill", color: .red)
                section("DOI points to a different work", r.items.filter { $0.verdict == .wrongDoi }, icon: "arrow.triangle.branch", color: .red)
                section("Could not be found in Crossref, OpenAlex or PubMed", r.items.filter { $0.verdict == .notFound }, icon: "questionmark.circle", color: .orange)
                section("Minor differences", r.items.filter { $0.verdict == .minorDifferences }, icon: "exclamationmark.circle", color: .orange)
                section("Couldn't be checked", r.items.filter { $0.verdict == .failed }, icon: "wifi.exclamationmark", color: .secondary)
                section("Verified", r.items.filter { $0.verdict == .verified }, icon: "checkmark.seal.fill", color: .green)
                section("Not looked up (not papers)", r.items.filter { $0.verdict == .notChecked }, icon: "minus.circle", color: .secondary)
            }
            .navigationTitle("Checked \(result.title)")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } }
                ToolbarItem(placement: .automatic) {
                    Menu {
                        ShareLink(item: r.markdown, preview: SharePreview("Reference check")) { Label("Share Report", systemImage: "square.and.arrow.up") }
                        Button("Save as Markdown…", systemImage: "doc.text") { saving = (TextFileDocument(r.markdown), UTType(filenameExtension: "md") ?? .plainText, "Reference check.md") }
                        Button("Save as CSV…", systemImage: "tablecells") { saving = (TextFileDocument(r.csv), .commaSeparatedText, "Reference check.csv") }
                    } label: { Label("Export", systemImage: "square.and.arrow.up") }
                }
            }
            .fileExporter(isPresented: Binding(get: { saving != nil }, set: { if !$0 { saving = nil } }),
                          document: saving?.doc, contentType: saving?.type ?? .plainText, defaultFilename: saving?.name) { _ in saving = nil }
        }
        #if os(macOS)
        .frame(minWidth: 680, minHeight: 560)
        #endif
    }

    @ViewBuilder
    private func section(_ title: String, _ items: [RefCheckItem], icon: String, color: Color) -> some View {
        if !items.isEmpty {
            Section {
                ForEach(items, id: \.number) { i in row(i, icon: icon, color: color) }
            } header: {
                Label("\(title) (\(items.count))", systemImage: icon).foregroundStyle(color)
            }
        }
    }

    private func row(_ i: RefCheckItem, icon: String, color: Color) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(alignment: .firstTextBaseline, spacing: 6) {
                Text(i.key == String(i.number) || i.key.isEmpty ? "[\(i.number)]" : "[\(i.number)] \(i.key)").font(.callout.monospaced()).foregroundStyle(.secondary)
                if i.inLibrary != nil { Pill(text: "In library", icon: "books.vertical", color: .luntaTeal) }
            }
            Text(i.reference).font(.callout).lineLimit(4).textSelection(.enabled)
            let notice = updatesHeadline(updates: PaperUpdates(notices: i.notices, published: nil, declinedDoi: "", linked: nil, preprint: nil, switchedAt: 0))
            if !notice.isEmpty {
                Text(notice.replacingOccurrences(of: "This paper", with: "The work")).font(.callout.weight(.semibold)).foregroundStyle(.red)
            }
            if !i.note.isEmpty { Text(i.note).font(.callout).foregroundStyle(.secondary).textSelection(.enabled) }
            ForEach(Array(i.differences.enumerated()), id: \.offset) { _, d in
                Text("\(d.field): \(d.given.isEmpty ? "not in the reference" : "“\(d.given)”") → “\(d.found)” in \(i.source)")
                    .font(.callout).foregroundStyle(.secondary)
            }
            if let f = i.found, i.verdict != .verified {
                Text("Found: \(f.authorLine)\(f.year.isEmpty ? "" : " (\(f.year))"). \(f.title). \(f.journal)")
                    .font(.caption).foregroundStyle(.secondary).lineLimit(3)
            }
            HStack(spacing: 14) {
                if let url = URL(string: i.link), !i.link.isEmpty {
                    Button(i.source.isEmpty ? "Open Record" : "Open in \(i.source == "Crossref" || i.source == "DataCite" || i.source == "doi.org" ? "doi.org" : i.source)") { openURL(url) }
                }
                ForEach(i.notices.filter { !$0.doi.isEmpty }, id: \.doi) { n in
                    if let url = URL(string: "https://doi.org/\(n.doi)") { Button("\(n.label) Notice") { openURL(url) } }
                }
                if !i.suggestedDoi.isEmpty, let url = URL(string: "https://doi.org/\(i.suggestedDoi)") {
                    Button("Open \(i.suggestedDoi)") { openURL(url) }
                }
            }
            .buttonStyle(.borderless).font(.caption)
        }
        .padding(.vertical, 2)
    }
}

/// What a check of the library found: flagged papers and published preprints, with the choices.
struct UpdatesReportView: View {
    @EnvironmentObject var model: LibraryModel
    @Environment(\.dismiss) private var dismiss
    @Environment(\.openURL) private var openURL
    let result: UpdatesResult
    @State private var switching: String?
    @State private var message: String?

    private var s: UpdatesSummary { result.summary }

    var body: some View {
        NavigationStack {
            List {
                Section { Text(s.text).font(.callout) }
                papers("Retracted", s.retracted, color: .red)
                papers("Expression of concern", s.concern, color: .orange)
                papers("Corrected", s.corrected, color: .orange)
                let published = s.published.filter { model.paperUpdates[$0]?.published != nil }
                if !published.isEmpty {
                    Section("Preprints that have been published (\(published.count))") {
                        ForEach(published, id: \.self) { id in
                            if let p = model.paper(id), let v = model.paperUpdates[id]?.published {
                                VStack(alignment: .leading, spacing: 4) {
                                    Text(p.meta.title).font(.callout.weight(.medium)).lineLimit(2)
                                    Text("Published in \(v.journal.isEmpty ? "a journal" : v.journal)\(v.date.isEmpty ? "" : " (\(v.date.prefix(10)))")\(v.title.isEmpty ? "" : ": “\(v.title)”") · \(v.doi) · found by \(v.foundBy)")
                                        .font(.caption).foregroundStyle(.secondary)
                                    HStack(spacing: 14) {
                                        Button("Switch to the Published Version…") { switching = id }
                                        Button("Link Both") { model.linkPublishedVersion(id) }
                                        Button("Keep the Preprint") { model.keepPreprint(id) }
                                        if let url = URL(string: "https://doi.org/\(v.doi)") { Button("Open") { openURL(url) } }
                                    }
                                    .buttonStyle(.borderless).font(.caption)
                                }
                            }
                        }
                    }
                }
            }
            .navigationTitle("Retractions and Published Versions")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar { ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } } }
            .confirmationDialog("Switch to the published version?", isPresented: Binding(get: { switching != nil }, set: { if !$0 { switching = nil } }), titleVisibility: .visible) {
                Button("Switch") {
                    guard let id = switching else { return }
                    Task {
                        if await model.usePublishedVersion(id) { message = await model.fetchPublishedFullText(id) }
                    }
                }
                Button("Cancel", role: .cancel) { switching = nil }
            } message: {
                Text("The paper's details become the published version's; highlights, notes, tags and the citation key stay, and the published reading view and PDF are added when open access (a reading view only if every highlight is found in it). Undo from the reader's banner.")
            }
            .alert("Published version", isPresented: Binding(get: { message != nil }, set: { if !$0 { message = nil } })) {
                Button("OK") { message = nil }
            } message: { Text(message ?? "") }
        }
        #if os(macOS)
        .frame(minWidth: 620, minHeight: 460)
        #endif
    }

    @ViewBuilder
    private func papers(_ title: String, _ ids: [String], color: Color) -> some View {
        if !ids.isEmpty {
            Section {
                ForEach(ids, id: \.self) { id in
                    if let p = model.paper(id), let u = model.paperUpdates[id] {
                        VStack(alignment: .leading, spacing: 4) {
                            Text(p.meta.title).font(.callout.weight(.medium)).lineLimit(2)
                            Text(updatesHeadline(updates: u)).font(.caption).foregroundStyle(color)
                            if let n = u.mainNotice, !n.doi.isEmpty, let url = URL(string: "https://doi.org/\(n.doi)") {
                                Button("See Notice (\(n.source))") { openURL(url) }.buttonStyle(.borderless).font(.caption)
                            }
                        }
                    }
                }
            } header: { Text("\(title) (\(ids.count))").foregroundStyle(color) }
        }
    }
}

/// The + menu's and File menu's entries.
struct CheckMenuItems: View {
    var check: (() -> Void)? = nil
    var body: some View {
        Button("Check a Reference List…", systemImage: "checkmark.seal") { ChecksUI.shared.askingForList = true }
        if let check {
            Button("Check Library for Retractions and Published Versions", systemImage: "exclamationmark.shield") { check() }
        }
    }
}
