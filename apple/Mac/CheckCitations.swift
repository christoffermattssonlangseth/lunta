import AppKit
import SwiftUI
import UniformTypeIdentifiers
import LuntaCore

/// Check Citations: does each cited paper plausibly say what the sentence citing it says? The core
/// (core/src/claims) finds each citation's sentence, the cited paper's passages that best match it
/// (BM25 over its full text, else its abstract) and, with an on-device model, checks the model's
/// verdict: its quotes must be word for word in the paper. Lunta proposes, the writer decides.
/// Everything stays on this Mac: the model is Apple's on-device model or the open model, never
/// Private Cloud Compute.
@MainActor
enum ClaimModel {
    private static var engine: AskEngine?

    /// The on-device model's answer (nil without one).
    static func answer(_ prompt: String, instructions: String) async -> String? {
        let e = engine ?? AskEngine(scope: .library)
        engine = e
        return await e.plainAnswer(prompt, instructions: instructions, onDeviceOnly: true)
    }

    /// Whether a model on this Mac is ready.
    static var ready: Bool {
        let e = engine ?? AskEngine(scope: .library)
        engine = e
        return e.onDeviceReady
    }

    /// For the citation API (called off the main thread): asks the model on the main actor and waits.
    nonisolated static func blocking() -> @Sendable (String, String) -> String? {
        { instructions, prompt in
            final class Box: @unchecked Sendable { var value: String? }
            let box = Box(), done = DispatchSemaphore(value: 0)
            Task { @MainActor in
                box.value = await answer(prompt, instructions: instructions)
                done.signal()
            }
            return done.wait(timeout: .now() + 120) == .timedOut ? nil : box.value
        }
    }
}

/// The model for the core, from Swift.
final class ClaimModelJudge: ClaimJudge, @unchecked Sendable {
    let ask = ClaimModel.blocking()
    func answer(instructions: String, prompt: String, maxTokens: UInt32) -> String? { ask(instructions, prompt) }
}

/// What the writer said about a citation.
enum ClaimDecision: String { case open, looksRight, dismissed }

struct TexClaimItem: Identifiable {
    let id: Int
    let tex: TexClaim
    /// The library's paper for each key ("" when none).
    var paperIds: [String]
    var report: ClaimReport?
    var checking = false
    var decision = ClaimDecision.open
}

/// "File › Check Citations in a .tex File…".
@MainActor
func checkTexCitations(model: LibraryModel?) {
    guard let model else { return }
    let p = NSOpenPanel()
    p.title = "Choose the .tex file"
    p.message = "Lunta checks each \\cite against the cited paper’s text in your library: does the paper plausibly support the sentence? Nothing is changed or sent anywhere."
    p.allowedContentTypes = [UTType(filenameExtension: "tex")].compactMap { $0 }
    guard p.runModal() == .OK, let file = p.url else { return }
    guard let text = try? String(contentsOf: file, encoding: .utf8) else {
        claimsAlert("Couldn’t read “\(file.lastPathComponent)”", "Lunta reads .tex files written in UTF-8.")
        return
    }
    let found = findTexClaims(text: text)
    if found.isEmpty {
        claimsAlert("No citations in “\(file.lastPathComponent)”", "Lunta looks for \\cite{…}, \\citep{…}, \\citet{…}, \\parencite{…}, \\autocite{…} and the like, outside comments.")
        return
    }
    // the .bib files it names, next to it (keys the library doesn't have are found by their DOI or title)
    let dir = file.deletingLastPathComponent()
    let bib = findTexBibFiles(text: text).compactMap { try? String(contentsOf: dir.appendingPathComponent($0), encoding: .utf8) }.joined(separator: "\n")
    TexClaimsWindow.show(TexClaimsModel(file: file, claims: found, bib: bib, library: model))
}

@MainActor
private func claimsAlert(_ title: String, _ text: String) {
    let a = NSAlert()
    a.messageText = title
    a.informativeText = text
    a.runModal()
}

@MainActor
final class TexClaimsModel: ObservableObject {
    let file: URL
    let library: LibraryModel
    @Published var items: [TexClaimItem]
    @Published var progress: String?
    @Published var judged = false
    @Published var onlyOpen = false
    private var task: Task<Void, Never>?

    init(file: URL, claims: [TexClaim], bib: String, library: LibraryModel) {
        self.file = file
        self.library = library
        let keys = Array(Set(claims.flatMap(\.keys)))
        let ids = library.lib.citeKeyPapers(keys: keys, bib: bib)
        let byKey = Dictionary(zip(keys, ids), uniquingKeysWith: { a, _ in a })
        items = claims.enumerated().map { i, c in TexClaimItem(id: i, tex: c, paperIds: c.keys.map { byKey[$0] ?? "" }) }
    }

    /// Checks the citations one by one (the model takes seconds each).
    func run() {
        task?.cancel()
        let lib = library.lib
        let useModel = ClaimModel.ready
        task = Task {
            for i in items.indices {
                if Task.isCancelled { break }
                progress = "Checking citation \(i + 1) of \(items.count)…"
                items[i].checking = true
                let c = items[i].tex, ids = items[i].paperIds.filter { !$0.isEmpty }
                let report: ClaimReport
                if ids.isEmpty {
                    report = ClaimReport(claim: c.claim, context: c.previous, judged: false, note: "", results: [])
                } else {
                    let judge: ClaimJudge = useModel ? ClaimModelJudge() : NoModelJudge()
                    report = await Task.detached { lib.checkClaim(claim: c.claim, context: c.previous, ids: ids, judge: judge) }.value
                }
                items[i].report = report
                items[i].checking = false
                judged = judged || report.judged
            }
            progress = nil
        }
    }

    func stop() { task?.cancel(); progress = nil }

    var summary: String {
        let results = items.compactMap(\.report).flatMap(\.results)
        let checked = items.filter { $0.report != nil }.count
        var parts: [String] = []
        for (v, name) in [(ClaimVerdict.supported, "supported"), (.partly, "partly supported"), (.contradicted, "contradicted"),
                          (.notFound, judged ? "not found" : "no matching passage"), (.possible, "possible support"), (.noText, "no text to check")] {
            let n = results.filter { $0.verdict == v }.count
            if n > 0 { parts.append("\(n) \(name)") }
        }
        let unknown = items.filter { $0.paperIds.contains("") }.count
        if unknown > 0 { parts.append("\(unknown) with keys not in your library") }
        let decided = items.filter { $0.decision == .looksRight }.count
        if decided > 0 { parts.append("\(decided) marked Looks Right") }
        return "\(checked == 1 ? "1 citation" : "\(checked) citations") checked" + (parts.isEmpty ? "" : ": " + parts.joined(separator: ", "))
    }
}

/// No model: the passages are shown for the writer to judge.
final class NoModelJudge: ClaimJudge, @unchecked Sendable {
    func answer(instructions: String, prompt: String, maxTokens: UInt32) -> String? { nil }
}

@MainActor
enum TexClaimsWindow {
    private static var windows: [NSWindow] = []

    static func show(_ m: TexClaimsModel) {
        let w = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 720, height: 720), styleMask: [.titled, .closable, .resizable, .miniaturizable],
                         backing: .buffered, defer: false)
        w.title = "Check Citations: \(m.file.lastPathComponent)"
        w.isReleasedWhenClosed = false
        w.contentView = NSHostingView(rootView: TexClaimsView(m: m, close: { [weak w] in w?.close() }).environmentObject(m.library))
        w.center()
        windows.append(w)
        NotificationCenter.default.addObserver(forName: NSWindow.willCloseNotification, object: w, queue: .main) { n in
            MainActor.assumeIsolated {
                m.stop()
                windows.removeAll { $0 === n.object as? NSWindow }
            }
        }
        w.makeKeyAndOrderFront(nil)
        m.run()
    }
}

struct TexClaimsView: View {
    @ObservedObject var m: TexClaimsModel
    let close: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("Lunta proposes, you decide: for each citation, the passages of the cited paper that best match the sentence, and with an on-device model a suggested verdict whose quotes are checked word for word against the paper. It can be wrong; read the passages. Checked on this Mac: nothing is sent anywhere.")
                .font(.callout).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            HStack {
                if let p = m.progress {
                    ProgressView().controlSize(.small)
                    Text(p).font(.callout)
                    Button("Stop") { m.stop() }
                } else {
                    Text(m.summary).font(.callout.weight(.medium))
                }
                Spacer()
                Toggle("Only those to look at", isOn: $m.onlyOpen).toggleStyle(.checkbox)
            }
            if m.progress == nil && !m.judged {
                Text("No on-device model here (Apple Intelligence, or the open model on macOS 27): the passages that share the most words with each sentence are shown, with their word overlap, for you to judge.")
                    .font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            }
            List {
                ForEach(m.items.filter { !m.onlyOpen || ($0.decision == .open && $0.needsLook) }) { it in
                    ClaimRow(item: it, decide: { d in if let i = m.items.firstIndex(where: { $0.id == it.id }) { m.items[i].decision = d } })
                        .padding(.vertical, 4)
                }
            }
            HStack {
                Spacer()
                Button("Check Again") { m.run() }.disabled(m.progress != nil)
                Button("Done") { close() }.keyboardShortcut(.defaultAction)
            }
        }
        .padding(16)
        .frame(minWidth: 600, minHeight: 500)
    }
}

extension TexClaimItem {
    /// Not (yet) found to be supported: worth a look.
    var needsLook: Bool {
        guard let r = report else { return true }
        return r.results.isEmpty || r.results.contains { $0.verdict != .supported }
    }
}

struct ClaimRow: View {
    let item: TexClaimItem
    let decide: (ClaimDecision) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(alignment: .firstTextBaseline) {
                Text("Line \(item.tex.line)").font(.caption.monospacedDigit()).foregroundStyle(.secondary)
                Text(item.tex.command).font(.system(.caption, design: .monospaced)).foregroundStyle(.teal).lineLimit(1)
                Spacer()
                switch item.decision {
                case .open:
                    Button("Looks Right") { decide(.looksRight) }.controlSize(.small).disabled(item.report == nil)
                    Button("Dismiss") { decide(.dismissed) }.controlSize(.small)
                case .looksRight:
                    Label("Looks right", systemImage: "checkmark.circle").font(.caption).foregroundStyle(.green)
                    Button("Undo") { decide(.open) }.buttonStyle(.link).controlSize(.small)
                case .dismissed:
                    Text("Dismissed").font(.caption).foregroundStyle(.secondary)
                    Button("Undo") { decide(.open) }.buttonStyle(.link).controlSize(.small)
                }
            }
            Text(item.tex.claim).textSelection(.enabled).fixedSize(horizontal: false, vertical: true)
            if item.decision != .dismissed {
                if item.checking {
                    ProgressView().controlSize(.small)
                } else if let r = item.report {
                    ForEach(Array(item.tex.keys.enumerated()), id: \.offset) { k, key in
                        let id = item.paperIds.indices.contains(k) ? item.paperIds[k] : ""
                        if id.isEmpty {
                            Text("\\cite{\(key)}: not in your Lunta library (nor found by the .bib’s DOI or title), so it can’t be checked.")
                                .font(.caption).foregroundStyle(.secondary)
                        } else if let res = r.results.first(where: { $0.paperId == id }) {
                            ClaimResultView(result: res)
                        }
                    }
                }
            }
        }
    }
}

/// One cited paper's result: the verdict, the model's reason, the quotes, the passages.
struct ClaimResultView: View {
    let result: ClaimResult
    @State private var passagesShown = false

    static func color(_ v: ClaimVerdict) -> Color {
        switch v {
        case .supported: return .green
        case .partly: return .orange
        case .contradicted: return .red
        case .notFound: return .gray
        case .possible: return .blue
        case .noText: return .secondary
        }
    }

    static func label(_ r: ClaimResult) -> String {
        switch r.verdict {
        case .supported: return "Supported"
        case .partly: return "Partly supported"
        case .notFound: return r.byModel ? "Not found in this paper" : "No matching passage"
        case .contradicted: return "Contradicted"
        case .possible: return "Possible support"
        case .noText: return "No text to check"
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(alignment: .firstTextBaseline, spacing: 6) {
                Text(Self.label(result)).font(.caption.weight(.semibold)).foregroundStyle(.white)
                    .padding(.horizontal, 6).padding(.vertical, 1)
                    .background(Capsule().fill(Self.color(result.verdict)))
                Text(result.label.isEmpty ? "Paper" : result.label).font(.caption.weight(.medium))
                Text(result.title).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                if result.byModel { Text("· suggested by the on-device model").font(.caption2).foregroundStyle(.secondary) }
            }
            if !result.reason.isEmpty { Text(result.reason).font(.caption).italic().fixedSize(horizontal: false, vertical: true) }
            ForEach(result.evidence, id: \.quote) { e in
                Button { open(e.link) } label: {
                    Text("“\(e.quote)”" + (e.section.isEmpty ? "" : " — \(e.section)")).font(.caption).multilineTextAlignment(.leading)
                        .fixedSize(horizontal: false, vertical: true)
                }
                .buttonStyle(.link)
                .help("Open the paper at this passage")
            }
            if !result.note.isEmpty { Text(result.note).font(.caption2).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true) }
            if !result.passages.isEmpty {
                DisclosureGroup(isExpanded: $passagesShown) {
                    ForEach(result.passages, id: \.start) { p in
                        VStack(alignment: .leading, spacing: 2) {
                            Text("\(p.section.isEmpty ? "Passage" : p.section) · \(Int((p.score * 100).rounded())) % word overlap").font(.caption2).foregroundStyle(.secondary)
                            Button { open(p.link) } label: {
                                Text(p.text).font(.caption).multilineTextAlignment(.leading).fixedSize(horizontal: false, vertical: true)
                            }
                            .buttonStyle(.plain)
                            .help("Open the paper at this passage")
                        }
                        .padding(.vertical, 2)
                    }
                } label: {
                    Text(result.passages.count == 1 ? "The best-matching passage" : "The \(result.passages.count) best-matching passages").font(.caption)
                }
                .onAppear { passagesShown = !result.byModel }
            }
        }
        .padding(.leading, 8)
    }

    private func open(_ link: String) {
        if let u = URL(string: link) { NSWorkspace.shared.open(u) }
    }
}
