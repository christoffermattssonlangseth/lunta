import Foundation
import AVFoundation
import SwiftUI
#if os(iOS)
import MediaPlayer
#endif

/// One spoken unit: a heading, paragraph, list item or caption from the reading view.
struct NarrationItem: Decodable, Equatable {
    let id: String
    let text: String
    let kind: String   // "heading" | "text" | "caption"
}

/// Reads a paper aloud with the system's on-device voices (no network), paragraph by paragraph.
@MainActor
final class Narrator: NSObject, ObservableObject {
    @Published private(set) var items: [NarrationItem] = []
    @Published private(set) var index = 0
    @Published private(set) var playing = false
    @Published var speed: Double = UserDefaults.standard.object(forKey: "narrationSpeed") as? Double ?? 1.0 {
        didSet { UserDefaults.standard.set(speed, forKey: "narrationSpeed"); restartCurrent() }
    }
    @Published var voiceID: String = UserDefaults.standard.string(forKey: "narrationVoice") ?? Narrator.bestVoice()?.identifier ?? "" {
        didSet { UserDefaults.standard.set(voiceID, forKey: "narrationVoice"); restartCurrent() }
    }

    /// Called with the element id of the paragraph being read (to highlight and scroll to it).
    var onHighlight: ((String) -> Void)?
    var title = ""

    private let synth = AVSpeechSynthesizer()
    private var current: AVSpeechUtterance?

    override init() {
        super.init()
        synth.delegate = self
        #if os(iOS)
        setUpRemoteCommands()
        #endif
    }

    // MARK: Voices

    /// English voices, best quality first (Premium > Enhanced > default).
    static var voices: [AVSpeechSynthesisVoice] {
        AVSpeechSynthesisVoice.speechVoices()
            .filter { $0.language.hasPrefix("en") }
            .sorted { ($0.quality.rawValue, $0.language == "en-US" ? 1 : 0) > ($1.quality.rawValue, $1.language == "en-US" ? 1 : 0) }
    }

    static func bestVoice() -> AVSpeechSynthesisVoice? { voices.first }

    static func label(_ v: AVSpeechSynthesisVoice) -> String {
        let q: String
        switch v.quality {
        case .premium: q = " (Premium)"
        case .enhanced: q = " (Enhanced)"
        default: q = ""
        }
        return "\(v.name)\(q) · \(v.language)"
    }

    var hasGoodVoice: Bool { Self.voices.contains { $0.quality != .default } }

    // MARK: Control

    func load(_ items: [NarrationItem], title: String) {
        stop()
        self.items = items
        self.title = title
        index = 0
    }

    func play(from i: Int? = nil) {
        guard !items.isEmpty else { return }
        if let i { index = max(0, min(i, items.count - 1)) }
        playing = true
        activateAudio()
        speakCurrent()
    }

    func play(fromID id: String) {
        if let i = items.firstIndex(where: { $0.id == id }) { play(from: i) }
    }

    func toggle() {
        if playing {
            pause()
        } else if synth.isPaused {
            playing = true
            synth.continueSpeaking()
            updateNowPlaying()
        } else {
            play()
        }
    }

    func pause() {
        playing = false
        synth.pauseSpeaking(at: .word)
        updateNowPlaying()
    }

    func stop() {
        playing = false
        current = nil
        synth.stopSpeaking(at: .immediate)
    }

    func next() { if index + 1 < items.count { play(from: index + 1) } }
    func previous() { play(from: max(0, index - 1)) }

    /// Jumps to the next heading (next section).
    func nextSection() {
        if let i = items[(index + 1)...].firstIndex(where: { $0.kind == "heading" }) { play(from: i) }
    }

    var currentSection: String {
        guard !items.isEmpty else { return "" }
        return items[...min(index, items.count - 1)].last { $0.kind == "heading" }?.text ?? ""
    }

    var progress: Double { items.isEmpty ? 0 : Double(index) / Double(items.count) }

    // MARK: Speaking

    private func restartCurrent() {
        guard playing else { return }
        speakCurrent()
    }

    private func speakCurrent() {
        guard index < items.count else { playing = false; return }
        let item = items[index]
        let u = AVSpeechUtterance(string: Self.spoken(item))
        u.voice = AVSpeechSynthesisVoice(identifier: voiceID) ?? Self.bestVoice()
        // AVSpeechUtterance rates are not linear; this maps 0.8×–2× onto a comfortable range
        let base = Double(AVSpeechUtteranceDefaultSpeechRate)
        u.rate = Float(min(Double(AVSpeechUtteranceMaximumSpeechRate), max(Double(AVSpeechUtteranceMinimumSpeechRate), base + (speed - 1) * 0.22)))
        u.preUtteranceDelay = item.kind == "heading" ? 0.45 : 0.05
        u.postUtteranceDelay = item.kind == "heading" ? 0.35 : 0.12
        current = u
        synth.stopSpeaking(at: .immediate)
        synth.speak(u)
        onHighlight?(item.id)
        updateNowPlaying()
    }

    fileprivate func finished(_ u: AVSpeechUtterance) {
        guard u === current, playing else { return }
        if index + 1 < items.count {
            index += 1
            speakCurrent()
        } else {
            playing = false
            updateNowPlaying()
        }
    }

    /// Makes text read well aloud: drops citation markers, URLs and DOIs, expands abbreviations.
    static func spoken(_ item: NarrationItem) -> String {
        var t = item.text
        let rules: [(String, String)] = [
            (#"\[\s*\d+(\s*[-–,]\s*\d+)*\s*\]"#, ""),                    // [12], [1–3], [4, 7]
            (#"https?://\S+"#, ""),
            (#"\bdoi:\s*\S+"#, ""),
            (#"\[equation\]"#, "equation"),
            (#"\bFigs?\.\s*"#, "Figure "),
            (#"\bEqs?\.\s*"#, "Equation "),
            (#"\bRefs?\.\s*"#, "reference "),
            (#"\be\.g\.,?"#, "for example"),
            (#"\bi\.e\.,?"#, "that is"),
            (#"\bet al\."#, "et al"),
            (#"\bvs\.?\b"#, "versus"),
            (#"\bapprox\.?"#, "approximately"),
            (#"\bresp\."#, "respectively"),
            (#"±"#, " plus or minus "),
            (#"×"#, " times "),
            (#"≤"#, " less than or equal to "),
            (#"≥"#, " greater than or equal to "),
            (#"<"#, " less than "),
            (#">"#, " greater than "),
            (#"~\s*(?=\d)"#, "about "),
            (#"(\d)\s*μm\b"#, "$1 micrometers"),
            (#"(\d)\s*nm\b"#, "$1 nanometers"),
            (#"(\d)\s*mm\b"#, "$1 millimeters"),
            (#"(\d)\s*%"#, "$1 percent"),
            (#"\bP\s*[<=]\s*"#, "P "),
            (#"\(\s*\)"#, ""),
            (#"\s{2,}"#, " "),
        ]
        for (p, r) in rules {
            t = t.replacingOccurrences(of: p, with: r, options: .regularExpression)
        }
        if item.kind == "caption" { t = "Figure caption. " + t }
        return t.trimmingCharacters(in: .whitespaces)
    }

    // MARK: Audio session, lock screen

    private func activateAudio() {
        #if os(iOS)
        try? AVAudioSession.sharedInstance().setCategory(.playback, mode: .spokenAudio)
        try? AVAudioSession.sharedInstance().setActive(true)
        #endif
    }

    private func updateNowPlaying() {
        #if os(iOS)
        var info: [String: Any] = [
            MPMediaItemPropertyTitle: title,
            MPMediaItemPropertyArtist: currentSection.isEmpty ? "Margins" : currentSection,
            MPNowPlayingInfoPropertyPlaybackRate: playing ? 1.0 : 0.0,
        ]
        info[MPNowPlayingInfoPropertyPlaybackProgress] = progress
        MPNowPlayingInfoCenter.default().nowPlayingInfo = items.isEmpty ? nil : info
        #endif
    }

    #if os(iOS)
    private func setUpRemoteCommands() {
        let c = MPRemoteCommandCenter.shared()
        c.playCommand.addTarget { [weak self] _ in Task { @MainActor in self?.toggle() }; return .success }
        c.pauseCommand.addTarget { [weak self] _ in Task { @MainActor in self?.pause() }; return .success }
        c.togglePlayPauseCommand.addTarget { [weak self] _ in Task { @MainActor in self?.toggle() }; return .success }
        c.nextTrackCommand.addTarget { [weak self] _ in Task { @MainActor in self?.next() }; return .success }
        c.previousTrackCommand.addTarget { [weak self] _ in Task { @MainActor in self?.previous() }; return .success }
    }
    #endif
}

extension Narrator: AVSpeechSynthesizerDelegate {
    nonisolated func speechSynthesizer(_ s: AVSpeechSynthesizer, didFinish u: AVSpeechUtterance) {
        Task { @MainActor in self.finished(u) }
    }
}

/// Player bar shown at the bottom of the reader while listening.
struct NarrationBar: View {
    @ObservedObject var narrator: Narrator
    var close: () -> Void

    var body: some View {
        VStack(spacing: 6) {
            ProgressView(value: narrator.progress).tint(.marginsTeal)
            HStack(spacing: 14) {
                VStack(alignment: .leading, spacing: 1) {
                    Text(narrator.currentSection.isEmpty ? "Listening" : narrator.currentSection)
                        .font(.subheadline.weight(.semibold)).lineLimit(1)
                    Text(narrator.title).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                }
                Spacer(minLength: 6)
                Button { narrator.previous() } label: { Image(systemName: "backward.fill") }
                Button { narrator.toggle() } label: {
                    Image(systemName: narrator.playing ? "pause.circle.fill" : "play.circle.fill").font(.system(size: 34))
                }
                Button { narrator.next() } label: { Image(systemName: "forward.fill") }
                Button { narrator.nextSection() } label: { Image(systemName: "forward.end.fill") }.help("Next section")
                Menu {
                    Picker("Speed", selection: $narrator.speed) {
                        ForEach([0.8, 1.0, 1.15, 1.3, 1.5, 1.75, 2.0], id: \.self) { Text(String(format: "%.2g×", $0)).tag($0) }
                    }
                    Picker("Voice", selection: $narrator.voiceID) {
                        ForEach(Narrator.voices.prefix(25), id: \.identifier) { Text(Narrator.label($0)).tag($0.identifier) }
                    }
                } label: {
                    Text(String(format: "%.2g×", narrator.speed)).font(.callout.monospacedDigit().weight(.semibold))
                }
                .fixedSize()
                Button(action: close) { Image(systemName: "xmark") }.help("Stop listening")
            }
            .buttonStyle(.borderless)
            .foregroundStyle(Color.marginsTeal)
            if !narrator.hasGoodVoice {
                Text("Tip: download a Premium voice for much more natural speech (Settings › Accessibility › Spoken Content › Voices).")
                    .font(.caption2).foregroundStyle(.secondary).frame(maxWidth: .infinity, alignment: .leading)
            }
        }
        .padding(.horizontal, 14).padding(.vertical, 10)
        .background(.regularMaterial)
    }
}
