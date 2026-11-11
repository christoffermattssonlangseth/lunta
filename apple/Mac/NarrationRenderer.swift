import Foundation
import WebKit
import MarginsCore

/// Renders natural-sounding narration with Kokoro (neural TTS) locally on the Mac and saves it
/// next to the paper (narration.m4a + narration.json), so it syncs to the iPhone through iCloud.
@MainActor
final class NarrationRenderer: ObservableObject {
    static let shared = NarrationRenderer()

    /// Kokoro voices (a… = American English, b… = British English).
    static let voices: [(id: String, name: String)] = [
        ("af_heart", "Heart · US female"), ("af_bella", "Bella · US female"), ("af_nicole", "Nicole · US female"),
        ("am_michael", "Michael · US male"), ("am_fenrir", "Fenrir · US male"),
        ("bf_emma", "Emma · UK female"), ("bm_george", "George · UK male"), ("bm_fable", "Fable · UK male"),
    ]

    @Published var voice = UserDefaults.standard.string(forKey: "neuralVoice") ?? "af_heart" {
        didSet { UserDefaults.standard.set(voice, forKey: "neuralVoice") }
    }
    @Published private(set) var rendering: Set<String> = []
    weak var model: LibraryModel?
    private var queue: [String] = []
    private var running = false

    static var supportDir: URL { CloudLibrary.localURL.deletingLastPathComponent().appendingPathComponent("tts", isDirectory: true) }
    static var python: URL { supportDir.appendingPathComponent(".venv/bin/python") }

    func enqueue(_ ids: [String]) {
        for id in ids where !queue.contains(id) && !rendering.contains(id) { queue.append(id) }
        rendering.formUnion(ids)
        guard !running else { return }
        running = true
        Task {
            while !queue.isEmpty {
                let id = queue.removeFirst()
                do {
                    try await render(id)
                } catch {
                    model?.errorMessage = "Natural narration failed: \(error.localizedDescription)"
                }
                rendering.remove(id)
            }
            running = false
            model?.progress = nil
        }
    }

    // MARK: - Setup (one time)

    /// Installs the speech engine (mlx-audio + Kokoro) into Margins' own folder with uv.
    func ensureEngine() async throws {
        if FileManager.default.fileExists(atPath: Self.python.path),
           (try? await run(Self.python.path, ["-c", "import mlx_audio, misaki, en_core_web_sm"])) != nil { return }
        guard let uv = ["~/.local/bin/uv", "/opt/homebrew/bin/uv", "/usr/local/bin/uv"]
            .map({ NSString(string: $0).expandingTildeInPath }).first(where: FileManager.default.isExecutableFile) else {
            throw fail("The natural voice needs “uv” (a Python installer). Install it with: brew install uv")
        }
        guard ["/opt/homebrew/lib/libespeak-ng.dylib", "/usr/local/lib/libespeak-ng.dylib"].contains(where: FileManager.default.fileExists) else {
            throw fail("The natural voice needs espeak-ng. Install it with: brew install espeak-ng")
        }
        model?.status = "Setting up the natural voice (one time, about 1 GB)…"
        try FileManager.default.createDirectory(at: Self.supportDir, withIntermediateDirectories: true)
        let venv = Self.supportDir.appendingPathComponent(".venv").path
        _ = try await run(uv, ["venv", "--python", "3.12", venv])
        // pip: misaki/spaCy use it to fetch their English model; install that model up front too
        _ = try await run(uv, ["pip", "install", "--python", Self.python.path, "pip", "mlx-audio", "misaki[en]"])
        _ = try await run(Self.python.path, ["-m", "spacy", "download", "en_core_web_sm"])
    }

    // MARK: - Rendering

    private func render(_ id: String) async throws {
        guard let model, let paper = model.paper(id) else { return }
        try await ensureEngine()
        guard paper.hasEpub, await model.ensureLocal(id, .epub) != nil else { throw fail("this paper has no reading view yet") }
        let lib = model.lib
        let book = try await Task.detached { try lib.openEpub(id: id) }.value
        let items = await PageTextExtractor.items(of: URL(fileURLWithPath: book.combined), readAccess: URL(fileURLWithPath: book.dir))
        guard !items.isEmpty else { throw fail("no text found to read") }
        let segments = items.map { ["id": $0.id, "kind": $0.kind, "text": Narrator.spoken($0)] }.filter { !($0["text"] ?? "").isEmpty }

        let work = FileManager.default.temporaryDirectory.appendingPathComponent("margins-tts-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: work, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: work) }
        let input = work.appendingPathComponent("segments.json")
        try JSONSerialization.data(withJSONObject: segments).write(to: input)
        guard let script = Bundle.main.url(forResource: "margins_tts", withExtension: "py") else { throw fail("narration script missing from the app") }

        let title = paper.meta.title.prefix(50)
        model.status = "Natural voice: starting “\(title)”…"
        _ = try await run(Self.python.path, [script.path, "--voice", voice, "--out", work.path], stdin: input) { line in
            let p = line.split(separator: " ")
            if p.count >= 3, p[0] == "PROGRESS", let i = Double(p[1]), let n = Double(p[2]) {
                Task { @MainActor in
                    model.status = "Natural voice: “\(title)” \(Int(i)) of \(Int(n)) paragraphs"
                    model.progress = i / n
                }
            }
        }
        let wav = work.appendingPathComponent("narration.wav")
        let m4a = work.appendingPathComponent("narration.m4a")
        _ = try await run("/usr/bin/afconvert", ["-f", "m4af", "-d", "aac", "-b", "48000", wav.path, m4a.path])
        let audio = try Data(contentsOf: m4a)
        let timings = try Data(contentsOf: work.appendingPathComponent("narration.json"))
        // audio first: the timing map marks the narration as ready
        try await Task.detached {
            try lib.attachFile(id: id, kind: .audio, data: audio)
            try lib.attachFile(id: id, kind: .audioIndex, data: timings)
        }.value
        model.reload()
        model.status = "Natural voice ready for “\(title)”"
    }

    /// Runs a program; `onLine` receives stdout lines as they arrive. Throws with stderr on failure.
    @discardableResult
    private func run(_ exe: String, _ args: [String], stdin: URL? = nil, onLine: (@Sendable (String) -> Void)? = nil) async throws -> String {
        try await withCheckedThrowingContinuation { cont in
            let p = Process()
            p.executableURL = URL(fileURLWithPath: exe)
            p.arguments = args
            var env = ProcessInfo.processInfo.environment
            env["PATH"] = "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin"
            env["TOKENIZERS_PARALLELISM"] = "false"
            p.environment = env
            if let stdin { p.standardInput = try? FileHandle(forReadingFrom: stdin) }
            let out = Pipe(), err = Pipe()
            p.standardOutput = out
            p.standardError = err
            let collected = LineCollector(onLine: onLine)
            out.fileHandleForReading.readabilityHandler = { h in collected.feed(h.availableData) }
            p.terminationHandler = { proc in
                out.fileHandleForReading.readabilityHandler = nil
                collected.feed(out.fileHandleForReading.readDataToEndOfFile())
                let errText = String(decoding: err.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
                if proc.terminationStatus == 0 {
                    cont.resume(returning: collected.text)
                } else {
                    // skip download progress bars; keep the last real error lines
                    let lines = errText.split(whereSeparator: { $0 == "\n" || $0 == "\r" })
                        .filter { !$0.contains("it/s]") && !$0.contains("Fetching") && !$0.trimmingCharacters(in: .whitespaces).isEmpty }
                    let tail = lines.suffix(3).joined(separator: " ")
                    cont.resume(throwing: fail("\(URL(fileURLWithPath: exe).lastPathComponent) failed: \(tail)"))
                }
            }
            do { try p.run() } catch { cont.resume(throwing: error) }
        }
    }
}

/// Collects a process's stdout and hands complete lines to a callback.
private final class LineCollector: @unchecked Sendable {
    private let lock = NSLock()
    private var buffer = Data()
    private(set) var text = ""
    let onLine: (@Sendable (String) -> Void)?
    init(onLine: (@Sendable (String) -> Void)?) { self.onLine = onLine }
    func feed(_ d: Data) {
        guard !d.isEmpty else { return }
        lock.lock(); defer { lock.unlock() }
        buffer.append(d)
        while let nl = buffer.firstIndex(of: 0x0A) {
            let line = String(decoding: buffer[buffer.startIndex..<nl], as: UTF8.self)
            buffer.removeSubrange(buffer.startIndex...nl)
            text += line + "\n"
            onLine?(line)
        }
    }
}

private struct RenderError: LocalizedError {
    let message: String
    var errorDescription: String? { message }
}

private func fail(_ m: String) -> Error { RenderError(message: m) }

/// Loads a reading-view document in an invisible web view and runs the narration extractor,
/// so rendered narration uses exactly the same paragraphs (and ids) as the reader.
@MainActor
final class PageTextExtractor: NSObject, WKNavigationDelegate {
    private var cont: CheckedContinuation<Void, Never>?
    private var webView: WKWebView?

    static func items(of url: URL, readAccess: URL) async -> [NarrationItem] {
        let x = PageTextExtractor()
        return await x.run(url, readAccess)
    }

    private func run(_ url: URL, _ readAccess: URL) async -> [NarrationItem] {
        let cfg = WKWebViewConfiguration()
        cfg.userContentController.addUserScript(WKUserScript(source: ReaderAssets.narrationJS, injectionTime: .atDocumentEnd, forMainFrameOnly: true))
        let v = WKWebView(frame: CGRect(x: 0, y: 0, width: 800, height: 1000), configuration: cfg)
        v.navigationDelegate = self
        webView = v
        await withCheckedContinuation { c in
            cont = c
            v.loadFileURL(url, allowingReadAccessTo: readAccess)
        }
        guard let json = try? await v.evaluateJavaScript("marginsNarration(false)") as? String,
              let data = json.data(using: .utf8) else { return [] }
        return (try? JSONDecoder().decode([NarrationItem].self, from: data)) ?? []
    }

    nonisolated func webView(_ w: WKWebView, didFinish n: WKNavigation!) {
        Task { @MainActor in self.cont?.resume(); self.cont = nil }
    }

    nonisolated func webView(_ w: WKWebView, didFail n: WKNavigation!, withError e: Error) {
        Task { @MainActor in self.cont?.resume(); self.cont = nil }
    }
}
