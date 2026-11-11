import SwiftUI
import MarginsCore

/// The Ask panel: a conversation with Apple's model about one paper or the whole library.
struct AssistantPanel: View {
    @EnvironmentObject var model: LibraryModel
    @ObservedObject var engine: AskEngine
    /// A cited passage (reading-view element id) was tapped.
    var onPassage: (String) -> Void = { _ in }
    /// A cited library paper was tapped.
    var onPaper: (String) -> Void = { _ in }
    @State private var draft = ""
    @FocusState private var focused: Bool

    var body: some View {
        VStack(spacing: 0) {
            header
            Divider()
            if engine.unavailableReason == AskEngine.openModelPending {
                if #available(iOS 27.0, macOS 27.0, *) { OpenModelDownload(engine: engine) }
            } else if let why = engine.unavailableReason {
                ContentUnavailableView("Assistant unavailable", systemImage: "apple.intelligence", description: Text(why))
                    .frame(maxHeight: .infinity)
                    .task {
                        // check again every few seconds; the panel switches on when the model is ready
                        while engine.unavailableReason != nil, !Task.isCancelled {
                            try? await Task.sleep(for: .seconds(4))
                            engine.objectWillChange.send()
                        }
                    }
            } else {
                conversation
                Divider()
                input
            }
        }
        .onAppear { engine.model = model }
    }

    private var header: some View {
        HStack(spacing: 8) {
            Image(systemName: "sparkles").foregroundStyle(Color.marginsTeal)
            VStack(alignment: .leading, spacing: 1) {
                Text(engine.scope == .library ? "Ask your library" : "Ask this paper").font(.headline)
                Text(engine.modelLabel).font(.caption2).foregroundStyle(.secondary)
            }
            Spacer()
            Menu {
                Picker("Model", selection: $engine.choice) {
                    ForEach(AskModelChoice.allCases.filter { $0 != .privateCloud || AskEngine.cloudEntitled }) { Text($0.title).tag($0) }
                }
                Divider()
                Button("New Conversation", systemImage: "square.and.pencil") { engine.reset() }
                    .disabled(engine.messages.isEmpty)
            } label: { Image(systemName: "ellipsis.circle") }
            .menuStyle(.borderlessButton)
            .fixedSize()
        }
        .padding(.horizontal, 14).padding(.vertical, 10)
    }

    private var conversation: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 12) {
                    if engine.messages.isEmpty { suggestions }
                    ForEach(engine.messages) { m in
                        MessageView(message: m, sources: engine.sources, onSource: open).id(m.id)
                    }
                    ForEach(engine.proposals) { p in ProposalCard(proposal: p, engine: engine) }
                    if engine.busy {
                        HStack(spacing: 6) {
                            ProgressView().controlSize(.small)
                            Text("Thinking…").font(.caption).foregroundStyle(.secondary)
                        }
                        .id("busy")
                    }
                }
                .padding(14)
            }
            .onChange(of: engine.messages.last?.text) { _, _ in
                if let last = engine.messages.last { proxy.scrollTo(last.id, anchor: .bottom) }
            }
            .onChange(of: engine.messages.count) { _, _ in
                if let last = engine.messages.last { withAnimation { proxy.scrollTo(last.id, anchor: .bottom) } }
            }
        }
    }

    @ViewBuilder private var suggestions: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(engine.scope == .library
                 ? "Ask about the papers in your library, find related work, or have it organise papers for you."
                 : "Ask anything about this paper. Answers cite the passages they come from: tap a number to jump there.")
                .font(.callout).foregroundStyle(.secondary)
                .padding(.bottom, 4)
            if engine.scope == .library {
                chip("Which papers have I not read about…", fill: "Which unread papers do I have about ")
                chip("Find recent papers beyond my library on…", fill: "Find new papers on ")
                chip("Tag my papers about…", fill: "Find my papers about  and propose tagging them")
                chip("What were my last 10 papers about?") { engine.ask("Summarize the topics of the 10 papers I added most recently.") }
                if let (name, ids) = currentGroup {
                    chip("Summarize “\(name)”") { engine.overview(ids, name: name) }
                    chip("What's missing in “\(name)”?") { engine.gaps(ids, name: name) }
                    if ids.count >= 2 && ids.count <= 8 {
                        chip("Compare the papers in “\(name)”") { engine.compare(ids) }
                    }
                }
            } else {
                chip("Summarize this paper") { engine.summarize() }
                chip("Explain the text I've selected") { engine.explainSelection() }
                chip("What are the key findings?") { engine.ask("What are the key findings, with the main numbers?") }
                chip("How did they do it?") { engine.ask("Describe the data and methods: cohort or samples, measurements, and statistical analysis.") }
                chip("What are the limitations?") { engine.ask("What limitations do the authors acknowledge, and what others are apparent?") }
            }
        }
    }

    /// The folder or tag being shown in the library, with its papers.
    private var currentGroup: (String, [String])? {
        switch model.source {
        case .collection(let id): return (model.collections.first { $0.id == id }?.name ?? "Folder", model.papers.map(\.id))
        case .tag(let t): return ("#\(t)", model.papers.map(\.id))
        default: return nil
        }
    }

    private func chip(_ title: String, fill: String? = nil, action: (() -> Void)? = nil) -> some View {
        Button {
            if let action { action() } else if let fill { draft = fill; focused = true }
        } label: {
            Text(title).font(.callout).padding(.horizontal, 10).padding(.vertical, 6)
                .background(Color.marginsTeal.opacity(0.10), in: Capsule())
        }
        .buttonStyle(.plain)
    }

    private var input: some View {
        HStack(alignment: .bottom, spacing: 8) {
            TextField(engine.scope == .library ? "Ask your library…" : "Ask about this paper…", text: $draft, axis: .vertical)
                .textFieldStyle(.plain)
                .lineLimit(1...6)
                .focused($focused)
                .onSubmit(send)
            if engine.busy {
                Button { engine.stop() } label: { Image(systemName: "stop.circle.fill").font(.title2) }
                    .buttonStyle(.plain).foregroundStyle(.secondary)
            } else {
                Button(action: send) { Image(systemName: "arrow.up.circle.fill").font(.title2) }
                    .buttonStyle(.plain).foregroundStyle(Color.marginsTeal)
                    .disabled(draft.trimmingCharacters(in: .whitespaces).isEmpty)
            }
        }
        .padding(12)
    }

    private func send() {
        let q = draft
        draft = ""
        engine.ask(q)
    }

    private func open(_ s: AskSource) {
        switch s.kind {
        case .passage(let id): onPassage(id)
        case .paper(let id): onPaper(id)
        case .external: break
        }
    }
}

private struct MessageView: View {
    let message: AskMessage
    let sources: [Int: AskSource]
    let onSource: (AskSource) -> Void

    var body: some View {
        switch message.role {
        case .user:
            HStack {
                Spacer(minLength: 30)
                Text(message.text)
                    .padding(.horizontal, 12).padding(.vertical, 8)
                    .background(Color.marginsTeal.opacity(0.14), in: RoundedRectangle(cornerRadius: 12))
                    .textSelection(.enabled)
            }
        case .activity:
            Label(message.text, systemImage: message.text.hasPrefix("Searching beyond") ? "globe.europe.africa" : "magnifyingglass")
                .font(.caption).foregroundStyle(.secondary)
        case .notice:
            Label(message.text, systemImage: "info.circle").font(.caption).foregroundStyle(Color.marginsCopper)
        case .assistant:
            VStack(alignment: .leading, spacing: 8) {
                Text(rendered)
                    .textSelection(.enabled)
                    .environment(\.openURL, OpenURLAction { url in
                        if url.scheme == "margins-src", let n = Int(url.host ?? ""), let s = sources[n] { onSource(s) }
                        return .handled
                    })
                let cited = citedSources
                if !cited.isEmpty {
                    FlowChips(sources: cited, onSource: onSource)
                }
            }
        }
    }

    private var numbers: [Int] {
        let re = /\[(\d{1,3})\]/
        var seen: [Int] = []
        for m in message.text.matches(of: re) {
            if let n = Int(m.1), !seen.contains(n) { seen.append(n) }
        }
        return seen
    }

    private var citedSources: [AskSource] { numbers.compactMap { sources[$0] } }

    /// Markdown with [n] turned into tappable links.
    private var rendered: AttributedString {
        let linked = message.text.replacing(/\[(\d{1,3})\]/) { m in
            sources[Int(m.1) ?? 0] != nil ? "[\\[\(m.1)\\]](margins-src://\(m.1))" : "[\(m.1)]"
        }
        let opts = AttributedString.MarkdownParsingOptions(interpretedSyntax: .inlineOnlyPreservingWhitespace)
        var a = (try? AttributedString(markdown: linked, options: opts)) ?? AttributedString(message.text)
        for run in a.runs where run.link != nil {
            a[run.range].foregroundColor = .marginsTeal
            a[run.range].font = .body.weight(.semibold)
        }
        return a
    }
}

private struct FlowChips: View {
    let sources: [AskSource]
    let onSource: (AskSource) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            ForEach(sources) { s in
                Button { onSource(s) } label: {
                    HStack(alignment: .firstTextBaseline, spacing: 6) {
                        Text("\(s.id)").font(.caption2.weight(.bold)).foregroundStyle(.white)
                            .frame(minWidth: 16).padding(.vertical, 1)
                            .background(tint(s), in: RoundedRectangle(cornerRadius: 4))
                        Image(systemName: icon(s)).font(.caption2).foregroundStyle(tint(s))
                        Text(s.label).font(.caption).lineLimit(1).foregroundStyle(.secondary)
                        if case .external = s.kind {
                            Text("beyond your library").font(.caption2.weight(.medium)).foregroundStyle(Color.marginsCopper)
                        }
                    }
                }
                .buttonStyle(.plain)
            }
        }
    }

    private func icon(_ s: AskSource) -> String {
        switch s.kind {
        case .passage: return "text.quote"
        case .paper: return "doc.text"
        case .external: return "globe.europe.africa"
        }
    }

    private func tint(_ s: AskSource) -> Color {
        if case .external = s.kind { return .marginsCopper }
        return .marginsTeal
    }
}

private struct ProposalCard: View {
    let proposal: ProposedChange
    @ObservedObject var engine: AskEngine

    var body: some View {
        HStack(spacing: 10) {
            Image(systemName: "wand.and.stars").foregroundStyle(Color.marginsCopper)
            Text(proposal.summary).font(.callout)
            Spacer(minLength: 4)
            switch proposal.state {
            case .pending:
                Button("Dismiss") { engine.dismiss(proposal) }.buttonStyle(.borderless)
                Button("Apply") { Task { await engine.apply(proposal) } }
                    .buttonStyle(.borderedProminent).tint(.marginsTeal).controlSize(.small)
            case .applied:
                Label("Done", systemImage: "checkmark.circle.fill").font(.caption).foregroundStyle(Color.marginsTeal)
            case .dismissed:
                Text("Dismissed").font(.caption).foregroundStyle(.secondary)
            case .failed:
                Text("Failed").font(.caption).foregroundStyle(.red)
            }
        }
        .padding(10)
        .background(Color.marginsCopper.opacity(0.08), in: RoundedRectangle(cornerRadius: 10))
    }
}

/// Offers the one-time download of the open model where Apple Intelligence isn't available.
@available(iOS 27.0, macOS 27.0, *)
private struct OpenModelDownload: View {
    @ObservedObject var engine: AskEngine
    @ObservedObject private var open = OpenModel.shared

    var body: some View {
        VStack(spacing: 14) {
            Image(systemName: "cpu").font(.system(size: 40)).foregroundStyle(Color.marginsTeal)
            Text("Ask with an open model").font(.title3.weight(.semibold))
            Text("Apple Intelligence isn't available here, so Ask can use \(OpenModel.displayName), a free open model that runs entirely on this device: private, offline, no account. It's a one-time download of \(OpenModel.downloadSize); Wi‑Fi recommended.")
                .font(.callout).foregroundStyle(.secondary).multilineTextAlignment(.center)
            switch open.state {
            case .notDownloaded:
                Button { Task { await open.prepare(); engine.objectWillChange.send() } } label: {
                    Label("Download \(OpenModel.displayName)", systemImage: "arrow.down.circle.fill").padding(.horizontal, 6)
                }
                .buttonStyle(.borderedProminent).tint(.marginsTeal)
            case .downloading(let f):
                ProgressView(value: f) { Text("Downloading… \(Int(f * 100))%").font(.caption) }.frame(maxWidth: 260)
            case .loading:
                ProgressView("Loading the model…")
            case .ready:
                ProgressView().task { engine.objectWillChange.send() }
            case .failed(let m):
                Text(m).font(.caption).foregroundStyle(.red).multilineTextAlignment(.center)
                Button("Try Again") { Task { await open.prepare(); engine.objectWillChange.send() } }
            }
        }
        .padding(28)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}
