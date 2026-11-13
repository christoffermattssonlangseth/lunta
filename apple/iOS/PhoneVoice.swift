import Foundation
import FluidAudio

/// Natural voice on the iPhone: Kokoro-82M running on the Neural Engine (FluidAudio / Core ML).
/// Paragraphs are made as they're needed, like the live voice on the Mac. The model downloads once.
@MainActor
final class PhoneVoice: ObservableObject {
    static let shared = PhoneVoice()

    enum State: Equatable { case idle, preparing, ready, failed(String) }
    @Published private(set) var state: State = .idle
    @Published var voice = UserDefaults.standard.string(forKey: "neuralVoice") ?? "af_heart" {
        didSet { UserDefaults.standard.set(voice, forKey: "neuralVoice") }
    }

    /// Same Kokoro voices as the Mac.
    static let voices: [(id: String, name: String)] = [
        ("af_heart", "Heart · US female"), ("af_bella", "Bella · US female"), ("af_nicole", "Nicole · US female"),
        ("am_michael", "Michael · US male"), ("am_fenrir", "Fenrir · US male"),
        ("bf_emma", "Emma · UK female"), ("bm_george", "George · UK male"), ("bm_fable", "Fable · UK male"),
    ]

    private let engine = PhoneVoiceEngine()
    private var preparing: Task<Bool, Never>?

    /// Was the model downloaded before (so preparing is quick)?
    var downloaded: Bool { UserDefaults.standard.bool(forKey: "phoneVoiceReady") }

    /// Downloads (first time) and loads the model.
    func prepare() async -> Bool {
        if state == .ready { return true }
        if let preparing { return await preparing.value }
        state = .preparing
        let v = voice
        let t = Task<Bool, Never> {
            do {
                try await engine.load(voice: v)
                UserDefaults.standard.set(true, forKey: "phoneVoiceReady")
                state = .ready
                return true
            } catch {
                NSLog("Lunta: natural voice failed to load: %@", String(describing: error))
                state = .failed(error.readable)
                return false
            }
        }
        preparing = t
        let ok = await t.value
        preparing = nil
        return ok
    }

    /// Audio for one paragraph, or nil if it can't be made.
    func clip(_ text: String, kind: String) async -> URL? {
        guard await prepare() else { return nil }
        return try? await engine.clip(text, kind: kind, voice: voice)
    }
}

/// Owns the Core ML model; renders one paragraph at a time and caches the results.
actor PhoneVoiceEngine {
    private var manager: KokoroAneManager?
    private var voices: Set<String> = []
    private var inflight: [String: Task<URL, Error>] = [:]
    private let dir: URL = {
        let d = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0].appendingPathComponent("narration-live", isDirectory: true)
        try? FileManager.default.createDirectory(at: d, withIntermediateDirectories: true)
        return d
    }()

    func load(voice: String) async throws {
        if manager != nil { return }
        let m = KokoroAneManager(defaultVoice: voice)
        try await m.initialize(preloadVoices: [voice])
        manager = m
        voices = [voice]
    }

    func clip(_ text: String, kind: String, voice: String) async throws -> URL {
        let key = Self.key(voice + "|" + kind + "|" + text)
        let url = dir.appendingPathComponent(key + ".wav")
        if FileManager.default.fileExists(atPath: url.path) { return url }
        if let t = inflight[key] { return try await t.value }
        guard let manager else { throw CocoaError(.featureUnsupported) }
        if !voices.contains(voice) {
            try await manager.initialize(preloadVoices: [voice])   // fetches the new voice pack
            voices.insert(voice)
        }
        let t = Task<URL, Error> {
            let r = try await manager.synthesizeDetailed(text: text, voice: voice)
            let (before, after): (Double, Double) = kind == "heading" ? (0.55, 0.45) : kind == "caption" ? (0.2, 0.4) : (0, 0.35)
            try Self.writeWAV(r.samples, rate: r.sampleRate, padBefore: before, padAfter: after, to: url)
            return url
        }
        inflight[key] = t
        defer { inflight[key] = nil }
        return try await t.value
    }

    private static func writeWAV(_ samples: [Float], rate: Int, padBefore: Double, padAfter: Double, to url: URL) throws {
        let pre = Int(padBefore * Double(rate)), post = Int(padAfter * Double(rate))
        let n = pre + samples.count + post
        var d = Data(capacity: 44 + n * 2)
        func u32(_ v: UInt32) { withUnsafeBytes(of: v.littleEndian) { d.append(contentsOf: $0) } }
        func u16(_ v: UInt16) { withUnsafeBytes(of: v.littleEndian) { d.append(contentsOf: $0) } }
        d.append(contentsOf: Array("RIFF".utf8)); u32(UInt32(36 + n * 2)); d.append(contentsOf: Array("WAVE".utf8))
        d.append(contentsOf: Array("fmt ".utf8)); u32(16); u16(1); u16(1); u32(UInt32(rate)); u32(UInt32(rate * 2)); u16(2); u16(16)
        d.append(contentsOf: Array("data".utf8)); u32(UInt32(n * 2))
        d.append(Data(count: pre * 2))
        var pcm = [Int16](repeating: 0, count: samples.count)
        for i in 0..<samples.count { pcm[i] = Int16(max(-1, min(1, samples[i])) * 32767) }
        pcm.withUnsafeBytes { d.append(contentsOf: $0) }
        d.append(Data(count: post * 2))
        let tmp = url.appendingPathExtension("part")
        try d.write(to: tmp)
        _ = try? FileManager.default.removeItem(at: url)
        try FileManager.default.moveItem(at: tmp, to: url)
    }

    private static func key(_ s: String) -> String {
        var h: UInt64 = 0xcbf29ce484222325
        for b in s.utf8 { h = (h ^ UInt64(b)) &* 0x100000001b3 }
        return String(h, radix: 36)
    }
}
