import SwiftUI
import UniformTypeIdentifiers
import LuntaCore

// Outline from Highlights: the highlights and notes in a folder, a tag, the papers listed (a
// search) or a selection, as a Markdown outline (core/src/outline.rs). Every point is a
// highlight word for word or the researcher's own note; the on-device model, when there is one,
// only groups the points into themes and names them (its one-line summaries are marked). Copy
// as Markdown or rich text, or save a .md file. The sheet's Summary side (SummaryView.swift) has
// the model condense the same highlights, every sentence citing those it is based on.

/// The papers an outline is made of, and what to call them ("Thesis", "3 papers").
struct OutlineRequest: Identifiable {
    let id = UUID()
    let ids: [String]
    let scope: String
    /// Shown over this paper's reader (iPhone, iPad): Open in Paper goes back to it.
    var inReader = false
}

@MainActor
final class OutlineUI: ObservableObject {
    static let shared = OutlineUI()
    @Published var request: OutlineRequest?

    /// Opens the outline sheet for these papers.
    func open(_ ids: [String], scope: String) {
        request = OutlineRequest(ids: ids, scope: scope)
    }
}

extension LibraryModel {
    /// A folder's papers (with its subfolders'), for an outline.
    func outlineIDs(folder: String) -> [String] {
        var ids: [String] = []
        var todo = [folder]
        while let f = todo.popLast() {
            ids += lib.papers(filter: PaperFilter(collection: f, tag: nil, starredOnly: false, unreadOnly: false), sort: .firstAuthor).map(\.id)
            todo += collections.filter { $0.parent == f }.map(\.id)
        }
        var seen = Set<String>()
        return ids.filter { seen.insert($0).inserted }
    }

    func outlineIDs(tag: String) -> [String] {
        lib.papers(filter: PaperFilter(collection: nil, tag: tag, starredOnly: false, unreadOnly: false), sort: .firstAuthor).map(\.id)
    }

    /// The outline, grouped by the on-device model when `engine` is given and can answer.
    func makeOutline(_ ids: [String], topic: String, engine: AskEngine?) async -> Outline {
        let lib = self.lib
        if let engine, engine.unavailableReason == nil {
            // Apple's on-device model has a small context: fewer highlights for it
            let max: UInt32 = engine.usesOpenModel ? 100 : 30
            let p = await Task.detached { lib.outlinePrompt(ids: ids, topic: topic, maxItems: max) }.value
            if !p.ids.isEmpty, let answer = await engine.plainAnswer(p.prompt, instructions: p.instructions) {
                return await Task.detached { lib.outlineFromHighlights(ids: ids, topic: topic, prompt: p, answer: answer) }.value
            }
        }
        return await Task.detached { lib.outlineFromHighlights(ids: ids, topic: topic, prompt: nil, answer: nil) }.value
    }
}

/// A .md file to save.
struct MarkdownFile: FileDocument {
    static var readableContentTypes: [UTType] { [UTType(filenameExtension: "md") ?? .plainText] }
    var text: String
    init(text: String) { self.text = text }
    init(configuration: ReadConfiguration) throws {
        text = String(data: configuration.file.regularFileContents ?? Data(), encoding: .utf8) ?? ""
    }
    func fileWrapper(configuration: WriteConfiguration) throws -> FileWrapper {
        FileWrapper(regularFileWithContents: Data(text.utf8))
    }
}

struct OutlineView: View {
    @EnvironmentObject var model: LibraryModel
    @Environment(\.dismiss) private var dismiss
    #if os(macOS)
    @Environment(\.openWindow) private var openWindow
    #endif
    @StateObject private var engine = AskEngine(scope: .paper("__outline__"))
    let request: OutlineRequest
    @State private var topic = ""
    @State private var useModel = true
    @State private var outline: Outline?
    @State private var working = false
    @State private var saving = false
    @State private var copied: String?
    @State private var mode: Mode = .outline
    @State private var summary: HighlightSummary?
    @State private var summarizing: Task<Void, Never>?
    @State private var progress = ""
    /// Why the summary isn't shown (nothing in it held up), shown over the outline.
    @State private var summaryFailed: String?
    @AppStorage("summaryLonger") private var longer = false
    @AppStorage("summaryBullets") private var bullets = false

    enum Mode: String, CaseIterable { case outline = "Outline", summary = "Summary" }

    private var modelAvailable: Bool { engine.unavailableReason == nil }

    var body: some View {
        NavigationStack {
            VStack(alignment: .leading, spacing: 12) {
                Text("Highlights and notes in \(request.scope) (\(request.ids.count) paper\(request.ids.count == 1 ? "" : "s")).")
                    .foregroundStyle(.secondary)
                Picker("Show", selection: $mode) {
                    ForEach(Mode.allCases, id: \.self) { Text($0.rawValue).tag($0) }
                }
                .pickerStyle(.segmented).labelsHidden()
                HStack {
                    TextField("Topic (optional)", text: $topic)
                        .textFieldStyle(.roundedBorder)
                        .onSubmit { mode == .outline ? make() : summarize() }
                    if mode == .outline {
                        Button(working ? "Making…" : outline == nil ? "Make Outline" : "Make Again") { make() }
                            .buttonStyle(.borderedProminent).tint(.luntaTeal).disabled(working)
                    } else if modelAvailable {
                        Button(summarizing != nil ? "Summarizing…" : summary == nil ? "Summarize" : "Regenerate") { summarize() }
                            .buttonStyle(.borderedProminent).tint(.luntaTeal).disabled(summarizing != nil)
                    }
                }
                if mode == .outline { outlineSide } else { summarySide }
            }
            .padding()
            .navigationTitle(mode == .outline ? "Outline from Highlights" : "Summarize Highlights")
            .toolbar { ToolbarItem(placement: .cancellationAction) { Button("Done") { summarizing?.cancel(); dismiss() } } }
            .fileExporter(isPresented: $saving, document: MarkdownFile(text: exportMarkdown),
                          contentType: UTType(filenameExtension: "md") ?? .plainText,
                          defaultFilename: fileName) { _ in }
        }
        #if os(macOS)
        .frame(minWidth: 680, minHeight: 560)
        #endif
        .task { make() }
        .onChange(of: mode) { copied = nil }
    }

    @ViewBuilder private var outlineSide: some View {
        if modelAvailable {
            Toggle("Group into themes with the on-device model", isOn: $useModel)
        } else {
            Text("Grouped by paper and section (no on-device model here). With a topic, only the highlights that mention it.")
                .font(.caption).foregroundStyle(.secondary)
        }
        if let summaryFailed { Text(summaryFailed).font(.callout).foregroundStyle(Color.luntaCopper) }
        if let o = outline {
            if !o.note.isEmpty { Text(o.note).font(.callout).foregroundStyle(Color.luntaCopper) }
            ScrollView {
                Text(o.markdown)
                    .font(.system(.callout, design: .monospaced))
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(10)
            }
            .background(.quaternary.opacity(0.4), in: RoundedRectangle(cornerRadius: 8))
            Text(o.byModel
                 ? "Themes grouped and named by the on-device model; every point is a highlight word for word or your note. Its summaries are marked."
                 : "Every point is a highlight word for word or your note, cited with a link to the paper and its citation key.")
                .font(.caption).foregroundStyle(.secondary)
            actions(markdown: o.markdown, html: o.html, title: o.title)
        } else if working {
            ProgressView(modelAvailable && useModel ? "Grouping the highlights…" : "Gathering the highlights…")
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        } else {
            Spacer()
        }
    }

    @ViewBuilder private var summarySide: some View {
        if !modelAvailable {
            Text(SummaryTexts.unavailable(engine.unavailableReason ?? ""))
                .font(.callout).foregroundStyle(.secondary)
                .frame(maxWidth: .infinity, alignment: .leading)
            Spacer()
        } else {
            ViewThatFits(in: .horizontal) {
                HStack(spacing: 12) { lengthPicker; stylePicker; Spacer() }
                VStack(alignment: .leading, spacing: 8) { lengthPicker; stylePicker }
            }
            .disabled(summarizing != nil)
            if let s = summary, s.ok {
                if !s.note.isEmpty { Text(s.note).font(.callout).foregroundStyle(Color.luntaCopper) }
                SummaryPane(summary: s, open: openInPaper).environmentObject(model)
                actions(markdown: s.markdown, html: s.html, title: s.title)
            } else if summarizing != nil {
                ProgressView(progress.isEmpty ? "The on-device model is summarizing…" : progress)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                Text("The on-device model condenses these highlights and notes into a few sentences, each citing the highlights it is based on. Sentences that cite nothing, or say more than the highlights they cite, are left out.")
                    .font(.callout).foregroundStyle(.secondary)
                Spacer()
            }
        }
    }

    private var lengthPicker: some View {
        Picker("Length", selection: $longer) {
            Text("Short").tag(false)
            Text("Longer").tag(true)
        }
        .pickerStyle(.segmented).fixedSize()
    }

    private var stylePicker: some View {
        Picker("Style", selection: $bullets) {
            Text("Prose").tag(false)
            Text("Bullet Points").tag(true)
        }
        .pickerStyle(.segmented).fixedSize()
    }

    private func actions(markdown: String, html: String, title: String) -> some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack {
                Button("Copy Markdown") { copyMarkdown(markdown) }
                Button("Copy as Rich Text") { copyRich(markdown: markdown, html: html) }
                ShareLink("Share", item: markdown, subject: Text(title))
                Button("Save as .md…") { saving = true }
                if let copied { Text(copied).font(.caption).foregroundStyle(.secondary) }
            }
            .buttonStyle(.bordered)
        }
    }

    private var exportMarkdown: String {
        mode == .summary && summary?.ok == true ? summary?.markdown ?? "" : outline?.markdown ?? ""
    }

    private var fileName: String {
        let what = mode == .summary && summary?.ok == true ? "Summary" : "Outline"
        let base = (topic.trimmingCharacters(in: .whitespaces).isEmpty ? "\(what) \(request.scope)" : "\(what) \(topic)")
        return String(base.map { "/:\\".contains($0) ? "-" : $0 }.prefix(80)) + ".md"
    }

    private func make() {
        guard !working else { return }
        working = true
        copied = nil
        Task {
            outline = await model.makeOutline(request.ids, topic: topic.trimmingCharacters(in: .whitespacesAndNewlines),
                                              engine: modelAvailable && useModel ? engine : nil)
            working = false
        }
    }

    private func summarize() {
        guard summarizing == nil, modelAvailable else { return }
        copied = nil
        summary = nil
        summaryFailed = nil
        progress = ""
        let t = topic.trimmingCharacters(in: .whitespacesAndNewlines)
        summarizing = Task {
            let s = await model.makeSummary(request.ids, topic: t, length: longer ? .longer : .short, style: bullets ? .bullets : .prose,
                                            engine: engine) { progress = $0 }
            summarizing = nil
            guard let s, !Task.isCancelled else { return }
            if s.ok {
                summary = s
            } else {
                // nothing held up: the outline instead (made with the same topic)
                summaryFailed = s.note
                mode = .outline
                if outline == nil || (outline?.title != (t.isEmpty ? "Highlights" : t)) { make() }
            }
        }
    }

    /// The paper at the highlight: in its reader (a new one, or the one this sheet is over).
    private func openInPaper(_ s: SummarySource) {
        #if os(macOS)
        HighlightJump.request(paper: s.paperId, annotation: s.annotationId)
        openWindow(value: s.paperId)
        #else
        let inReader = request.inReader && request.ids == [s.paperId]
        summarizing?.cancel()
        dismiss()
        Task { @MainActor in
            try? await Task.sleep(for: .milliseconds(400))
            if !inReader { NotificationCenter.default.post(name: .luntaReadPaper, object: s.paperId) }
            HighlightJump.request(paper: s.paperId, annotation: s.annotationId)
        }
        #endif
    }

    private func copyMarkdown(_ markdown: String) {
        #if os(macOS)
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(markdown, forType: .string)
        #else
        UIPasteboard.general.string = markdown
        #endif
        copied = "Copied."
    }

    /// HTML and the Markdown as plain text: Pages, Word, Google Docs and Mail take the HTML.
    private func copyRich(markdown: String, html: String) {
        #if os(macOS)
        let pb = NSPasteboard.general
        pb.clearContents()
        pb.declareTypes([.html, .string], owner: nil)
        pb.setString(html, forType: .html)
        pb.setString(markdown, forType: .string)
        #else
        UIPasteboard.general.items = [[UTType.html.identifier: html, UTType.utf8PlainText.identifier: markdown]]
        #endif
        copied = "Copied as rich text."
    }
}

/// The outline sheet, on the library window (Mac) and the library (iPhone, iPad).
struct OutlineSheet: ViewModifier {
    @EnvironmentObject var model: LibraryModel
    @ObservedObject private var ui = OutlineUI.shared

    func body(content: Content) -> some View {
        content.sheet(item: $ui.request) { r in OutlineView(request: r).environmentObject(model) }
    }
}

extension View {
    func outlineSheet() -> some View { modifier(OutlineSheet()) }
}

extension LibraryModel {
    /// What the list shows, to name an outline of it: “Thesis”, or the papers found for “glia”.
    var outlineScope: String {
        let q = query.trimmingCharacters(in: .whitespaces)
        return q.isEmpty ? "“\(sourceTitle)”" : "the papers found for “\(q)”"
    }

    /// An outline of the papers the list shows (a folder, a tag, a search).
    func outlineOfList() {
        OutlineUI.shared.open(papers.map(\.id), scope: outlineScope)
    }
}

#if os(iOS)
/// The list's ⋯ menu (iPhone, iPad): Outline from Highlights of the papers shown.
struct ListMoreMenu: View {
    @EnvironmentObject var model: LibraryModel
    var body: some View {
        Menu {
            Button("Outline from Highlights…", systemImage: "list.bullet.indent") { model.outlineOfList() }
                .disabled(model.papers.isEmpty)
        } label: { Label("More", systemImage: "ellipsis.circle") }
    }
}
#endif
