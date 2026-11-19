import SwiftUI
import WebKit
import FoundationModels
import LuntaCore

// The Summary card at the top of the reading view: 3–4 sentences on what the paper did and found,
// written on the device (Apple Intelligence, or the open model where that's what Ask uses) as soon
// as the paper is opened, shown as it is written, and kept with the paper (summary.json in its
// folder, which syncs with the library like its other files) so it is there at once next time and
// on the other devices. A remade reading view gives a different source, so its summary is made
// again. The page's side is in SummaryCard.swift, the prompt in SummaryWriter.swift.

/// A summary as it is kept in the paper's folder (summary.json).
struct PaperSummary: Codable, Equatable {
    var text: String
    /// "apple-on-device" or "qwen3-4b"
    var model: String
    var created: Date
    /// SummaryWriter.sourceHash of what it was made from
    var source: String
}

/// Which model writes summaries here: Apple's on-device model when it's ready; the open model when
/// the user chose it for Ask, or Apple Intelligence isn't available, and it is already downloaded
/// (a summary never starts a 2 GB download); otherwise none, and no card is shown.
enum SummaryModel: String {
    case apple = "apple-on-device", open = "qwen3-4b"

    @MainActor static var current: SummaryModel? {
        let choice = AskModelChoice(rawValue: UserDefaults.standard.string(forKey: "askModel") ?? "") ?? .automatic
        var open = false
        if #available(macOS 27.0, iOS 27.0, *) { open = UserDefaults.standard.bool(forKey: "openModelDownloaded") }
        if choice == .openModel, open { return .open }
        if SystemLanguageModel.default.isAvailable { return .apple }
        return open ? .open : nil
    }

    /// Under the summary: who wrote it (summaries can be wrong).
    static func note(_ id: String) -> String {
        // summaries sync: one may come from the Android or Windows/Linux app's open model
        let who: String
        switch id {
        case SummaryModel.apple.rawValue: who = "Apple Intelligence"
        case SummaryModel.open.rawValue: who = "the open model (\(OpenModelName.name))"
        case "qwen3-1.7b": who = "the open model (Qwen3 1.7B)"
        default: who = "an open model (\(id))"
        }
        return "Made on device by \(who). It can be wrong."
    }

    @MainActor func session() async -> LanguageModelSession {
        if self == .open, #available(macOS 27.0, iOS 27.0, *) {
            await OpenModel.shared.prepare()
            return LanguageModelSession(model: OpenModel.shared.model, instructions: SummaryWriter.instructions)
        }
        return LanguageModelSession(model: SummaryWriter.appleModel, instructions: SummaryWriter.instructions)
    }
}

/// The open model's name without touching OpenModel (whose first use loads it).
private enum OpenModelName { static let name = "Qwen3 4B" }

/// summary.json in the paper's folder.
enum SummaryStore {
    static let fileName = "summary.json"

    @MainActor static func url(_ id: String, model: LibraryModel) -> URL {
        URL(fileURLWithPath: model.lib.expectedFilePath(id: id, kind: .epub)).deletingLastPathComponent().appendingPathComponent(fileName)
    }

    /// The saved summary (fetched from iCloud first if it's only there; given up after a few seconds).
    @MainActor static func read(_ id: String, model: LibraryModel) async -> PaperSummary? {
        let u = url(id, model: model)
        let placeholder = u.deletingLastPathComponent().appendingPathComponent(".\(fileName).icloud")
        let fm = FileManager.default
        guard fm.fileExists(atPath: u.path) || fm.fileExists(atPath: placeholder.path) else { return nil }
        if model.inCloud, !(await CloudLibrary.download(u, timeout: 6)) { return nil }
        return await Task.detached(priority: .utility) {
            guard let d = try? Data(contentsOf: u) else { return nil }
            let dec = JSONDecoder(); dec.dateDecodingStrategy = .iso8601
            return try? dec.decode(PaperSummary.self, from: d)
        }.value
    }

    @MainActor static func write(_ s: PaperSummary, _ id: String, model: LibraryModel) {
        let u = url(id, model: model)
        let enc = JSONEncoder(); enc.dateEncodingStrategy = .iso8601; enc.outputFormatting = [.prettyPrinted, .sortedKeys]
        guard let d = try? enc.encode(s) else { return }
        Task.detached(priority: .utility) {
            try? FileManager.default.createDirectory(at: u.deletingLastPathComponent(), withIntermediateDirectories: true)
            try? d.write(to: u, options: .atomic)
        }
    }

    @MainActor static func remove(_ id: String, model: LibraryModel) {
        try? FileManager.default.removeItem(at: url(id, model: model))
    }
}

/// Makes and shows one reader's summary. pageReady() when the page has loaded (and again after it
/// reloads); stop() when the paper is closed.
@MainActor
final class SummaryMaker: ObservableObject {
    /// Settings › "Show summaries in the reader"
    static let key = "readerSummaries"
    static var enabled: Bool { UserDefaults.standard.object(forKey: key) as? Bool ?? true }

    private var job: Task<Void, Never>?
    private var jobPaper: String?
    private var paperID = ""
    private weak var model: LibraryModel?
    private weak var controller: ReaderController?
    /// What the card shows (put back when the page reloads).
    private var shown: (state: String, text: String, note: String)?

    func pageReady(_ id: String, model: LibraryModel, controller: ReaderController) {
        if paperID != id { stop(); shown = nil }
        paperID = id
        self.model = model
        self.controller = controller
        controller.onSummary = { [weak self] action in self?.menu(action) }
        guard Self.enabled else { hide(); return }
        if let s = shown { show(s.state, s.text, note: s.note) }
        if job == nil { start(force: false) }
    }

    func stop() {
        job?.cancel()
        job = nil
    }

    /// The setting changed while the reader is open.
    func setEnabled(_ on: Bool) {
        if on { if let model, let controller, !paperID.isEmpty { pageReady(paperID, model: model, controller: controller) } }
        else { stop(); hide() }
    }

    private func menu(_ action: String) {
        switch action {
        case "regenerate":
            stop()
            start(force: true)
        case "hide":
            UserDefaults.standard.set(false, forKey: Self.key)
            setEnabled(false)
        default: break
        }
    }

    private func start(force: Bool) {
        let id = paperID
        job = Task(priority: .utility) { [weak self] in
            await self?.run(id, force: force)
            if self?.paperID == id { self?.job = nil }
        }
    }

    private func run(_ id: String, force: Bool) async {
        guard let model, let controller, let paper = model.paper(id) else { return }
        let title = paper.meta.title, abstract = paper.meta.abstractText
        // what the summary is made from, and the key it's kept under
        guard let full = await controller.summarySource(words: SummaryWriter.wordBudget, abstract: abstract),
              full.split(separator: " ").count >= 80 else { hide(); return }
        let hash = SummaryWriter.sourceHash(full)
        if !force, let saved = await SummaryStore.read(id, model: model), saved.source == hash, !saved.text.isEmpty {
            guard !Task.isCancelled else { return }
            show("done", saved.text, note: SummaryModel.note(saved.model))
            return
        }
        guard !Task.isCancelled, let engine = SummaryModel.current else { if !force { hide() }; return }
        show("loading", "Summarizing…")
        let started = Date()
        do {
            let prompt: String?
            if engine == .apple {
                let limit = SystemLanguageModel.default.contextSize - SummaryWriter.answerTokens
                prompt = await SummaryWriter.fit(limit: limit, source: { [weak controller] w in
                    if w == SummaryWriter.wordBudget { return SummaryWriter.prompt(title: title, source: full) }
                    return await controller?.summarySource(words: w, abstract: abstract).map { SummaryWriter.prompt(title: title, source: $0) }
                }, tokens: { await SummaryWriter.appleTokens($0) })
            } else {
                // Qwen3 holds far more; "/no_think" skips its thinking trace
                prompt = SummaryWriter.prompt(title: title, source: full) + " /no_think"
            }
            guard let prompt, !Task.isCancelled else { hide(); return }
            let session = await engine.session()
            let text = try await SummaryWriter.write(session, prompt: prompt) { [weak self] t in
                self?.show("writing", t)
            }
            guard let text, !Task.isCancelled else { if !Task.isCancelled { hide() }; return }
            let s = PaperSummary(text: text, model: engine.rawValue, created: Date(), source: hash)
            SummaryStore.write(s, id, model: model)
            show("done", text, note: SummaryModel.note(engine.rawValue))
            NSLog("Lunta: summary of %@ in %.1f s", id, Date().timeIntervalSince(started))
        } catch {
            // refused (guardrails), too long, or the model went away: no card, nothing kept
            if !(error is CancellationError) { NSLog("Lunta: no summary for %@: %@", id, String(describing: error)) }
            if !Task.isCancelled { hide() }
        }
    }

    private var lastPush = Date.distantPast

    private func show(_ state: String, _ text: String, note: String = "") {
        guard Self.enabled else { return }
        // while it's being written: at most a few updates a second (each one lays the page out again)
        if state == "writing", Date().timeIntervalSince(lastPush) < 0.2 { shown = (state, text, note); return }
        lastPush = Date()
        shown = (state, text, note)
        let obj: [String: String] = ["state": state, "text": text, "note": note]
        guard let d = try? JSONSerialization.data(withJSONObject: obj), let json = String(data: d, encoding: .utf8) else { return }
        controller?.webView?.evaluateJavaScript("window.luntaSummaryShow && luntaSummaryShow(\(json))")
    }

    private func hide() {
        shown = nil
        controller?.webView?.evaluateJavaScript("window.luntaSummaryHide && luntaSummaryHide()")
    }
}

extension ReaderController {
    /// The text a summary is made from (luntaSummarySource), within so many words.
    func summarySource(words: Int, abstract: String) async -> String? {
        guard let v = webView,
              let d = try? JSONSerialization.data(withJSONObject: [abstract]),
              let arg = String(data: d, encoding: .utf8),
              let json = try? await v.evaluateJavaScript("window.luntaSummarySource ? luntaSummarySource(\(words), \(arg)[0]) : ''") as? String,
              let data = json.data(using: .utf8),
              let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let text = obj["text"] as? String, !text.isEmpty else { return nil }
        return text
    }
}

/// Receives the card's menu ("regenerate", "hide") from the page.
final class SummaryMessageHandler: NSObject, WKScriptMessageHandler {
    weak var controller: ReaderController?
    func userContentController(_ c: WKUserContentController, didReceive m: WKScriptMessage) {
        guard let action = m.body as? String else { return }
        Task { @MainActor in self.controller?.onSummary?(action) }
    }
}

extension WKWebViewConfiguration {
    /// Adds the Summary card's script and its message handler.
    func addSummary(_ controller: ReaderController) {
        userContentController.addUserScript(WKUserScript(source: ReaderAssets.summaryJS, injectionTime: .atDocumentEnd, forMainFrameOnly: true))
        let h = SummaryMessageHandler()
        h.controller = controller
        userContentController.add(h, name: "summary")
    }
}

/// Settings: "Show summaries in the reader".
struct SummarySettingToggle: View {
    @AppStorage(SummaryMaker.key) private var on = true
    static let explanation = "A few sentences on what the paper did and found, at the top of the reading view. Written on this device by Apple Intelligence and kept with the paper. Summaries can be wrong."

    var body: some View { Toggle("Show summaries in the reader", isOn: $on) }
}
