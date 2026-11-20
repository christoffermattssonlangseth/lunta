import SwiftUI
import UniformTypeIdentifiers
import LuntaCore

// Outline from Highlights: the highlights and notes in a folder, a tag, the papers listed (a
// search) or a selection, as a Markdown outline (core/src/outline.rs). Every point is a
// highlight word for word or the researcher's own note; the on-device model, when there is one,
// only groups the points into themes and names them (its one-line summaries are marked). Copy
// as Markdown or rich text, or save a .md file.

/// The papers an outline is made of, and what to call them ("Thesis", "3 papers").
struct OutlineRequest: Identifiable {
    let id = UUID()
    let ids: [String]
    let scope: String
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
    @StateObject private var engine = AskEngine(scope: .paper("__outline__"))
    let request: OutlineRequest
    @State private var topic = ""
    @State private var useModel = true
    @State private var outline: Outline?
    @State private var working = false
    @State private var saving = false
    @State private var copied: String?

    private var modelAvailable: Bool { engine.unavailableReason == nil }

    var body: some View {
        NavigationStack {
            VStack(alignment: .leading, spacing: 12) {
                Text("Highlights and notes in \(request.scope) (\(request.ids.count) paper\(request.ids.count == 1 ? "" : "s")).")
                    .foregroundStyle(.secondary)
                HStack {
                    TextField("Topic (optional)", text: $topic)
                        .textFieldStyle(.roundedBorder)
                        .onSubmit { make() }
                    Button(working ? "Making…" : outline == nil ? "Make Outline" : "Make Again") { make() }
                        .buttonStyle(.borderedProminent).tint(.luntaTeal).disabled(working)
                }
                if modelAvailable {
                    Toggle("Group into themes with the on-device model", isOn: $useModel)
                } else {
                    Text("Grouped by paper and section (no on-device model here). With a topic, only the highlights that mention it.")
                        .font(.caption).foregroundStyle(.secondary)
                }
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
                    HStack {
                        Button("Copy Markdown") { copyMarkdown(o) }
                        Button("Copy as Rich Text") { copyRich(o) }
                        Button("Save as .md…") { saving = true }
                        if let copied { Text(copied).font(.caption).foregroundStyle(.secondary) }
                        Spacer()
                    }
                    .buttonStyle(.bordered)
                } else if working {
                    ProgressView(modelAvailable && useModel ? "Grouping the highlights…" : "Gathering the highlights…")
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                } else {
                    Spacer()
                }
            }
            .padding()
            .navigationTitle("Outline from Highlights")
            .toolbar { ToolbarItem(placement: .cancellationAction) { Button("Done") { dismiss() } } }
            .fileExporter(isPresented: $saving, document: MarkdownFile(text: outline?.markdown ?? ""),
                          contentType: UTType(filenameExtension: "md") ?? .plainText,
                          defaultFilename: fileName) { _ in }
        }
        #if os(macOS)
        .frame(minWidth: 680, minHeight: 560)
        #endif
        .task { make() }
    }

    private var fileName: String {
        let base = (topic.trimmingCharacters(in: .whitespaces).isEmpty ? "Outline \(request.scope)" : "Outline \(topic)")
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

    private func copyMarkdown(_ o: Outline) {
        #if os(macOS)
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(o.markdown, forType: .string)
        #else
        UIPasteboard.general.string = o.markdown
        #endif
        copied = "Copied."
    }

    /// HTML and the Markdown as plain text: Pages, Word, Google Docs and Mail take the HTML.
    private func copyRich(_ o: Outline) {
        #if os(macOS)
        let pb = NSPasteboard.general
        pb.clearContents()
        pb.declareTypes([.html, .string], owner: nil)
        pb.setString(o.html, forType: .html)
        pb.setString(o.markdown, forType: .string)
        #else
        UIPasteboard.general.items = [[UTType.html.identifier: o.html, UTType.utf8PlainText.identifier: o.markdown]]
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
