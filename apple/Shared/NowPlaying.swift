import SwiftUI
import Combine

/// The paper being read aloud, wherever you are: the narrator that plays (or is paused) is held
/// here, so it keeps reading after its reader closes, and the Now Playing bar shows it in the
/// library and in other readers.
@MainActor
final class NowPlaying: ObservableObject {
    static let shared = NowPlaying()

    /// The narrator that is playing or paused; nil once stopped (Narrator.active).
    @Published var narrator: Narrator?
    /// iPhone and iPad: readers on screen. The library's bar steps aside for them (each reader
    /// shows its own player, or the Now Playing bar for another paper).
    @Published private(set) var readersShown = 0

    func readerAppeared() { readersShown += 1 }
    func readerDisappeared() { readersShown = max(0, readersShown - 1) }

    /// The narration of this paper, for its reader opening again: it takes the narration over.
    func narrator(for paperID: String) -> Narrator? {
        guard let n = narrator, !paperID.isEmpty, n.paperID == paperID else { return nil }
        return n
    }

    /// Fires once the narrator has changed and settled (a reader that starts over stops and plays
    /// again at once): a reader whose narration was stopped elsewhere, or replaced by another
    /// paper's, closes its player bar.
    var settled: AnyPublisher<Void, Never> {
        $narrator.map { _ in () }
            .debounce(for: .milliseconds(300), scheduler: RunLoop.main)
            .eraseToAnyPublisher()
    }
}

/// Now Playing: the paper being read aloud, its section, play/pause, previous/next paragraph and
/// stop; the title opens the paper at the paragraph being read. Shown while narration is playing
/// or paused and its reader's own player isn't on screen.
struct NowPlayingBar: View {
    enum Style { case bar, capsule }
    var style: Style = .bar
    /// Hide it for this narrator (the reader that shows its own player for it).
    var except: Narrator?
    @ObservedObject private var now = NowPlaying.shared

    var body: some View {
        VStack(spacing: 0) {
            if let n = now.narrator, n !== except, !n.items.isEmpty {
                NowPlayingContent(narrator: n, style: style)
                    .transition(.move(edge: .bottom).combined(with: .opacity))
            }
        }
        .animation(.easeOut(duration: 0.2), value: now.narrator.map(ObjectIdentifier.init))
    }
}

private struct NowPlayingContent: View {
    @ObservedObject var narrator: Narrator
    let style: NowPlayingBar.Style
    #if os(macOS)
    @Environment(\.openWindow) private var openWindow
    #endif

    var body: some View {
        HStack(spacing: style == .bar ? 12 : 8) {
            Button(action: open) {
                HStack(spacing: 10) {
                    Image(systemName: "waveform")
                        .symbolEffect(.variableColor.iterative, isActive: narrator.playing)
                        .font(.body.weight(.semibold))
                        .foregroundStyle(Color.luntaTeal)
                        .frame(width: 22)
                    VStack(alignment: .leading, spacing: 1) {
                        Text(narrator.title.isEmpty ? "Listening" : narrator.title)
                            .font(.callout.weight(.semibold)).lineLimit(1).truncationMode(.tail)
                        Text(subtitle).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                    }
                    Spacer(minLength: 0)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .help("Open the paper at the paragraph being read")
            .accessibilityLabel("Now playing: \(narrator.title). Open the paper")
            controls
        }
        .padding(.horizontal, style == .bar ? 14 : 16)
        .padding(.vertical, style == .bar ? 6 : 6)
        .frame(maxWidth: style == .capsule ? 480 : .infinity)
        .background {
            if style == .capsule {
                Capsule().fill(.regularMaterial).shadow(color: .black.opacity(0.18), radius: 8, y: 2)
            } else {
                Rectangle().fill(.bar)
            }
        }
        // how far along: a thin line along the top of the bar (the bottom of the capsule)
        .overlay(alignment: style == .bar ? .top : .bottom) {
            GeometryReader { g in
                Rectangle().fill(Color.luntaTeal)
                    .frame(width: g.size.width * min(1, max(0, narrator.progress)), height: 2)
            }
            .frame(height: 2)
            .padding(.horizontal, style == .capsule ? 22 : 0)
            .accessibilityHidden(true)
        }
        .overlay(alignment: .top) { if style == .bar { Divider() } }
    }

    private var subtitle: String {
        let section = narrator.currentSection
        let state = narrator.waiting ? "Loading…" : narrator.playing ? "" : "Paused"
        return [state, section].filter { !$0.isEmpty }.joined(separator: " · ").ifEmpty("Reading aloud")
    }

    private var controls: some View {
        HStack(spacing: 2) {
            button("backward.fill", "Previous paragraph") { narrator.previous() }
            button(narrator.playing ? "pause.fill" : "play.fill", narrator.playing ? "Pause" : "Play", big: true) { narrator.toggle() }
            button("forward.fill", "Next paragraph") { narrator.next() }
            button("xmark", "Stop listening") { narrator.stop() }
        }
        .foregroundStyle(Color.luntaTeal)
    }

    private func button(_ icon: String, _ label: String, big: Bool = false, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Image(systemName: icon)
                .font(big ? .title3 : .callout)
                #if os(iOS)
                .frame(width: 40, height: 44)
                #else
                .frame(width: 28, height: 28)
                #endif
                .contentShape(Rectangle())
        }
        .buttonStyle(.borderless)
        .help(label)
        .accessibilityLabel(label)
    }

    private func open() {
        let id = narrator.paperID
        guard !id.isEmpty else { return }
        #if os(macOS)
        openWindow(value: id)   // brings its reader forward, or opens one that takes the narration over
        #else
        NotificationCenter.default.post(name: .luntaReadPaper, object: id)
        #endif
    }
}

private extension String {
    func ifEmpty(_ s: String) -> String { isEmpty ? s : self }
}
