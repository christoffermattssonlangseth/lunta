import SwiftUI
import UniformTypeIdentifiers
import LuntaCore

// Summarize Highlights: the Summary side of the outline sheet. The on-device model (Apple's, or
// Qwen3 via MLX on 27) condenses the highlights and notes into a few sentences, each citing the
// highlights it is based on; the core (core/src/outline/summary.rs) writes the prompts, cuts long
// sets into parts (combined afterwards), and checks every sentence: one that cites nothing, a
// highlight that wasn't given, or says more than the highlights it cites is left out. Tap a
// citation for its quote and Open in Paper (at the highlight).

/// The highlight to show when its paper's reader is ready (from a summary's Open in Paper).
@MainActor
enum HighlightJump {
    private static var pending: [String: String] = [:]

    /// Show this highlight in its paper: the reader that has it open shows it now, one that opens shows it once loaded.
    static func request(paper: String, annotation: String) {
        pending[paper] = annotation
        NotificationCenter.default.post(name: .luntaShowHighlight, object: paper)
    }

    static func has(_ paper: String) -> Bool { pending[paper] != nil || finds[paper] != nil }
    static func take(_ paper: String) -> String? { pending.removeValue(forKey: paper) }

    private static var finds: [String: String] = [:]

    /// Show this passage in its paper (Check Citations' evidence, lunta://open?id=…&find=…): the
    /// reader finds its words.
    static func request(paper: String, find text: String) {
        finds[paper] = text
        NotificationCenter.default.post(name: .luntaShowHighlight, object: paper)
    }

    static func takeFind(_ paper: String) -> String? { finds.removeValue(forKey: paper) }
}

extension View {
    /// A reader showing the highlight HighlightJump has for its paper: when asked (`pdfOnly` false), and a moment after
    /// opening for the PDF, which has no page-ready callback (`pdfOnly` true).
    func showsPendingHighlight(_ paperID: String, show: @escaping (_ pdfOnly: Bool, _ delay: Duration) -> Void) -> some View {
        onReceive(NotificationCenter.default.publisher(for: .luntaShowHighlight)) { n in
            if n.object as? String == paperID { show(false, .milliseconds(200)) }
        }
        .task {
            try? await Task.sleep(for: .seconds(1.5))
            show(true, .zero)
        }
    }
}

extension Notification.Name {
    /// A highlight waits to be shown (object: its paper's id; HighlightJump.take gives it).
    static let luntaShowHighlight = Notification.Name("luntaShowHighlight")
}

extension LibraryModel {
    /// The summary of these papers' highlights by the on-device model: part by part when they
    /// don't fit its context, then combined; checked by the core. nil when cancelled.
    func makeSummary(_ ids: [String], topic: String, length: SummaryLength, style: SummaryStyle, engine: AskEngine,
                     progress: @escaping @MainActor (String) -> Void) async -> HighlightSummary? {
        let lib = self.lib
        // Apple's on-device model has about 4,000 tokens; Qwen3 4B on phones 4,096
        let max: UInt32 = engine.usesOpenModel ? 45 : 25
        let plan = await Task.detached { lib.summaryPlan(ids: ids, topic: topic, length: length, style: style, maxItems: max, maxParts: 6) }.value
        var answers: [String] = []
        for (k, p) in plan.parts.enumerated() {
            if Task.isCancelled { return nil }
            progress(plan.parts.count > 1 ? "Summarizing part \(k + 1) of \(plan.parts.count)…" : "The on-device model is summarizing…")
            answers.append(await engine.plainAnswer(p.prompt, instructions: p.instructions) ?? "")
        }
        var combined: String?
        if let c = summaryCombinePrompt(plan: plan, answers: answers) {
            if Task.isCancelled { return nil }
            progress("Combining the parts…")
            combined = await engine.plainAnswer(c.prompt, instructions: c.instructions)
        }
        let done = answers
        return await Task.detached { summaryFromAnswers(plan: plan, answers: done, combined: combined) }.value
    }
}

enum SummaryTexts {
    static let label = "Written by the on-device model from your highlights. Check it against the quotes below."
    #if os(iOS)
    static let needs = "Summaries need Apple Intelligence (iOS 26 or newer) or the open model (iOS 27)."
    #else
    static let needs = "Summaries need Apple Intelligence (macOS 26 or newer) or the open model (macOS 27)."
    #endif

    /// Why there is no summary here, from the assistant's availability texts.
    static func unavailable(_ reason: String) -> String {
        if reason == AskEngine.openModelPending {
            return needs + " The open model isn't downloaded yet: download it in the assistant panel (✦). The outline works without a model."
        }
        if reason == AskEngine.needsNewerSystem { return needs + " The outline works without a model." }
        return needs + " " + reason + " The outline works without a model."
    }
}

/// The summary, its citations, the quotes cited.
struct SummaryPane: View {
    @EnvironmentObject var model: LibraryModel
    let summary: HighlightSummary
    /// Open in Paper: the paper at this highlight.
    var open: (SummarySource) -> Void
    @State private var selected: UInt32?

    var body: some View {
        ScrollViewReader { proxy in
            ScrollView {
                VStack(alignment: .leading, spacing: 10) {
                    Text(attributed)
                        .font(.body)
                        .lineSpacing(3)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .environment(\.openURL, OpenURLAction { url in
                            guard url.scheme == "lunta-cite", let n = UInt32(url.host ?? url.path) else { return .systemAction }
                            withAnimation(.easeOut(duration: 0.15)) { selected = n }
                            return .handled
                        })
                    Label(SummaryTexts.label, systemImage: "sparkles")
                        .font(.caption).foregroundStyle(.secondary)
                    if let n = selected, let s = summary.sources.first(where: { $0.number == n }) {
                        card(s)
                    }
                    Divider().padding(.vertical, 4)
                    Text("Highlights cited").font(.headline)
                    ForEach(summary.sources, id: \.number) { s in
                        row(s).id(s.number)
                    }
                }
                .padding(12)
            }
            .onChange(of: selected) { if let n = selected { withAnimation { proxy.scrollTo(n, anchor: .center) } } }
        }
        .background(.quaternary.opacity(0.4), in: RoundedRectangle(cornerRadius: 8))
    }

    /// The sentences with their citation numbers as links.
    private var attributed: AttributedString {
        var out = AttributedString()
        for (i, s) in summary.sentences.enumerated() {
            if summary.bullets {
                if i > 0 { out += AttributedString("\n") }
                out += AttributedString("•  ")
            } else if i > 0 {
                out += AttributedString(" ")
            }
            var body = s.text
            var end = "."
            if let last = body.last, ".!?".contains(last) { end = String(last); body.removeLast() }
            out += AttributedString(body + " [")
            for (k, n) in s.citations.enumerated() {
                if k > 0 { out += AttributedString(", ") }
                var c = AttributedString("\(n)")
                c.link = URL(string: "lunta-cite://\(n)")
                c.foregroundColor = .luntaTeal
                c.font = .body.weight(.semibold)
                out += c
            }
            out += AttributedString("]" + end)
        }
        return out
    }

    private func quote(_ s: SummarySource) -> String {
        if s.quote.isEmpty { return "Note: \(s.note)" }
        return "“\(s.quote)”" + (s.note.isEmpty ? "" : " — Note: \(s.note)")
    }

    private func place(_ s: SummarySource) -> String {
        s.section.isEmpty ? s.label : "\(s.label) · \(s.section)"
    }

    /// The tapped citation's highlight.
    private func card(_ s: SummarySource) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(alignment: .firstTextBaseline) {
                Text("[\(s.number)]").font(.callout.weight(.semibold)).foregroundStyle(Color.luntaTeal)
                Text(place(s)).font(.callout.weight(.semibold))
                Spacer()
                Button { selected = nil } label: { Image(systemName: "xmark.circle.fill").foregroundStyle(.secondary) }
                    .buttonStyle(.plain).accessibilityLabel("Close")
            }
            Text(quote(s)).font(.callout).textSelection(.enabled)
            Button("Open in Paper", systemImage: "arrow.up.forward.app") { open(s) }
                .buttonStyle(.bordered).controlSize(.small)
        }
        .padding(10)
        .background(Color.luntaTeal.opacity(0.08), in: RoundedRectangle(cornerRadius: 8))
    }

    private func row(_ s: SummarySource) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Text("[\(s.number)]").font(.callout.monospacedDigit().weight(.semibold))
                .foregroundStyle(selected == s.number ? Color.luntaCopper : Color.luntaTeal)
            VStack(alignment: .leading, spacing: 2) {
                Text(quote(s)).font(.callout).textSelection(.enabled)
                HStack(spacing: 6) {
                    Text(place(s)).font(.caption).foregroundStyle(.secondary)
                    Button("Open in Paper") { open(s) }.font(.caption).buttonStyle(.borderless)
                }
            }
        }
    }
}
