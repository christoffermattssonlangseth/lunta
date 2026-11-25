import SwiftUI
import WebKit
import FoundationModels
import LuntaCore
import os

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
        // before macOS/iOS 26 there is no FoundationModels, so no model at all (and no card)
        guard #available(macOS 26.0, iOS 26.0, *) else { return nil }
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

    @available(macOS 26.0, iOS 26.0, *)
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

/// Timing of the summaries, for Instruments (os_signpost, subsystem com.cmlangseth.lunta,
/// category Summary) and the log: waiting for a still page, the source, fitting the prompt, the
/// model's session, the first words and the whole summary.
enum SummaryTrace {
    static let log = Logger(subsystem: "com.cmlangseth.lunta", category: "Summary")
    static let signposter = OSSignposter(logger: log)
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

    #if os(iOS)
    static let noModel = "Can't write a new summary on this iPhone: summaries need Apple Intelligence (iOS 26 or newer), or the open model (iOS 27)."
    #else
    static let noModel = "Can't write a new summary on this Mac: summaries need Apple Intelligence (macOS 26 or newer), or the open model (macOS 27)."
    #endif

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
        guard !Task.isCancelled, let engine = SummaryModel.current, #available(macOS 26.0, iOS 26.0, *),
              force || SummaryBudget.allowsAutomatic() else {
            if !force { hide() }
            // Regenerate with no model here (a summary that synced from another device): say why
            else if !Task.isCancelled, let s = shown, s.state == "done" { show("done", s.text, note: Self.noModel) }
            return
        }
        // The model works the chip hard: it starts once the paper has settled and the page has been
        // still for a moment (not while it opens, its figures load or the pages are being turned)
        if !force {
            let w = SummaryTrace.signposter.beginInterval("wait for a still page")
            let ok = await waitUntilStill(controller)
            SummaryTrace.signposter.endInterval("wait for a still page", w)
            guard ok, !Task.isCancelled else { return }
        }
        show("loading", "Summarizing…")
        let started = Date()
        let sp = SummaryTrace.signposter
        let whole = sp.beginInterval("summary", "\(id, privacy: .public) \(engine.rawValue, privacy: .public)")
        defer { sp.endInterval("summary", whole) }
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
            sp.emitEvent("prompt ready", "\(Date().timeIntervalSince(started), format: .fixed(precision: 2)) s")
            let sessionSP = sp.beginInterval("session")
            let session = await engine.session()
            sp.endInterval("session", sessionSP)
            var first = true, updates = 0
            let text = try await SummaryWriter.write(session, prompt: prompt) { [weak self] t in
                if first {
                    first = false
                    sp.emitEvent("first words", "\(Date().timeIntervalSince(started), format: .fixed(precision: 2)) s")
                }
                updates += 1
                self?.show("writing", t)
            }
            guard let text, !Task.isCancelled else { if !Task.isCancelled { hide() }; return }
            let s = PaperSummary(text: text, model: engine.rawValue, created: Date(), source: hash)
            SummaryStore.write(s, id, model: model)
            show("done", text, note: SummaryModel.note(engine.rawValue))
            SummaryTrace.log.info("summary of \(id, privacy: .public) by \(engine.rawValue, privacy: .public) in \(Date().timeIntervalSince(started), format: .fixed(precision: 1)) s, \(updates) partial texts, \(self.pushes) drawn")
        } catch {
            // refused (guardrails), too long, or the model went away: no card, nothing kept
            if !(error is CancellationError) { NSLog("Lunta: no summary for %@: %@", id, String(describing: error)) }
            if !Task.isCancelled { hide() }
        }
    }

    /// Waits until the page has been still (no finger on it, not scrolled or turned) for
    /// SummaryBudget.stillBeforeStart ms; false when the reader went away meanwhile.
    private func waitUntilStill(_ controller: ReaderController) async -> Bool {
        // the paper's opening work first (place, highlights, figures)
        try? await Task.sleep(for: .milliseconds(SummaryBudget.stillBeforeStart))
        while !Task.isCancelled {
            guard let v = controller.webView else { return false }
            let still = (try? await v.evaluateJavaScript("window.luntaSummaryStill ? luntaSummaryStill() : 1e9")) as? Double ?? 1e9
            if still >= Double(SummaryBudget.stillBeforeStart) { return true }
            try? await Task.sleep(for: .milliseconds(max(250, SummaryBudget.stillBeforeStart - Int(still))))
        }
        return false
    }

    private var throttle = SummaryThrottle(perSecond: SummaryBudget.updatesPerSecond)
    /// Updates sent to the page (for the log).
    private var pushes = 0

    private func show(_ state: String, _ text: String, note: String = "") {
        guard Self.enabled else { return }
        // while it's being written: at most a few updates a second (each one lays the page out again;
        // the page also holds them while it is touched or moving, SummaryCard.swift)
        shown = (state, text, note)
        guard throttle.allows(partial: state == "writing") else { return }
        pushes += 1
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
