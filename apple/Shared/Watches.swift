import SwiftUI
import UserNotifications
import MarginsCore

// Watches: follow an author, a topic, or the papers citing one of yours. Margins checks OpenAlex
// when it opens and twice a day while it runs, and lists what's new under "New for You".
// The watches (and what you've dismissed) live in <library>/watches.json, so they sync; each
// device keeps its own list of found papers in its cache.

struct Watch: Codable, Identifiable, Hashable {
    enum Kind: String, Codable, CaseIterable { case topic, author, citing }
    var id = UUID().uuidString
    var kind: Kind
    var value: String          // words, an author's name, or a DOI
    var label: String          // shown in the app
    var created = Date()
    var lastChecked: Date?
    var seen: [String] = []    // keys of papers already shown or dismissed

    var icon: String {
        switch kind {
        case .topic: return "text.magnifyingglass"
        case .author: return "person"
        case .citing: return "quote.bubble"
        }
    }

    var coreKind: WatchKind {
        switch kind {
        case .topic: return .topic
        case .author: return .author
        case .citing: return .citing
        }
    }

    var describe: String {
        switch kind {
        case .topic: return "New papers on “\(label)”"
        case .author: return "New papers by \(label)"
        case .citing: return "Papers citing “\(label)”"
        }
    }
}

/// A paper a watch found.
struct WatchFind: Identifiable {
    let watch: String
    let hit: DiscoverHit
    var id: String { watch + "|" + LibraryModel.hitKey(hit) }
}

@MainActor
final class WatchCenter: ObservableObject {
    static let shared = WatchCenter()
    @Published var watches: [Watch] = []
    @Published var finds: [WatchFind] = []
    @Published var checking = false
    weak var model: LibraryModel?
    private var timer: Timer?

    private var fileURL: URL? { model?.libraryURL.appendingPathComponent("watches.json") }

    func start(_ model: LibraryModel) {
        self.model = model
        load()
        Task { await check() }
        timer?.invalidate()
        timer = Timer.scheduledTimer(withTimeInterval: 12 * 3600, repeats: true) { _ in
            Task { @MainActor in await WatchCenter.shared.check() }
        }
    }

    func load() {
        guard let url = fileURL else { return }
        CloudLibrary.startDownload(url)
        if let d = try? Data(contentsOf: url), let list = try? JSONDecoder.iso.decode([Watch].self, from: d), list != watches {
            watches = list
        }
    }

    private func save() {
        guard let url = fileURL, let d = try? JSONEncoder.iso.encode(watches) else { return }
        try? d.write(to: url, options: .atomic)
    }

    private func update(_ id: String, _ change: (inout Watch) -> Void) {
        load()
        guard let i = watches.firstIndex(where: { $0.id == id }) else { return }
        change(&watches[i])
        save()
    }

    func add(_ w: Watch) {
        load()
        guard !watches.contains(where: { $0.kind == w.kind && $0.value.caseInsensitiveCompare(w.value) == .orderedSame }) else { return }
        watches.append(w)
        save()
        requestNotifications()
        Task { await check(only: w.id) }
    }

    func remove(_ id: String) {
        load()
        watches.removeAll { $0.id == id }
        finds.removeAll { $0.watch == id }
        save()
    }

    /// Hides a found paper here and on the other devices.
    func dismiss(_ f: WatchFind) {
        finds.removeAll { $0.id == f.id }
        update(f.watch) { $0.seen.append(LibraryModel.hitKey(f.hit)) }
    }

    func dismissAll(of watch: String) {
        let keys = finds.filter { $0.watch == watch }.map { LibraryModel.hitKey($0.hit) }
        finds.removeAll { $0.watch == watch }
        update(watch) { $0.seen.append(contentsOf: keys) }
    }

    var newCount: Int { finds.filter { model?.libraryID(of: $0.hit) == nil && $0.hit.inLibrary == nil }.count }

    /// Looks for new papers for every watch (or one).
    func check(only: String? = nil) async {
        guard let model, !checking else { return }
        load()
        checking = true
        defer { checking = false }
        let lib = model.lib
        var fresh = 0
        let fmt = DateFormatter()
        fmt.dateFormat = "yyyy-MM-dd"
        for w in watches where only == nil || w.id == only {
            // look back 90 days the first time, then a week before the last check (late indexing)
            let since = (w.lastChecked?.addingTimeInterval(-7 * 86400)) ?? Date().addingTimeInterval(-90 * 86400)
            let s = fmt.string(from: since)
            let hits = await Task.detached { (try? lib.watchNew(kind: w.coreKind, value: w.value, since: s, limit: 20)) ?? [] }.value
            let seen = Set(w.seen)
            let known = Set(finds.filter { $0.watch == w.id }.map { LibraryModel.hitKey($0.hit) })
            let new = hits.filter { $0.inLibrary == nil && !seen.contains(LibraryModel.hitKey($0)) && !known.contains(LibraryModel.hitKey($0)) }
            finds.append(contentsOf: new.map { WatchFind(watch: w.id, hit: $0) })
            fresh += new.count
            update(w.id) { $0.lastChecked = Date() }
        }
        finds.sort { $0.hit.meta.date > $1.hit.meta.date }
        if fresh > 0, only == nil { notify(fresh) }
    }

    private func requestNotifications() {
        UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .badge, .sound]) { _, _ in }
    }

    private func notify(_ n: Int) {
        let c = UNMutableNotificationContent()
        c.title = "Margins"
        c.body = "\(n) new paper\(n == 1 ? "" : "s") for your watches."
        UNUserNotificationCenter.current().add(UNNotificationRequest(identifier: "watches", content: c, trigger: nil))
    }
}

extension JSONEncoder {
    static var iso: JSONEncoder { let e = JSONEncoder(); e.dateEncodingStrategy = .iso8601; e.outputFormatting = [.prettyPrinted]; return e }
}
extension JSONDecoder {
    static var iso: JSONDecoder { let d = JSONDecoder(); d.dateDecodingStrategy = .iso8601; return d }
}

extension LibraryModel {
    nonisolated static func hitKey(_ h: DiscoverHit) -> String {
        h.meta.doi.isEmpty ? titleKey(h.meta.title) : doiKey(h.meta.doi)
    }

    /// Adds a paper found by a watch and remembers it on the watch's find.
    func addFind(_ f: WatchFind) async -> String? {
        let id = await addDiscovered(f.hit)
        if let id, let i = WatchCenter.shared.finds.firstIndex(where: { $0.id == f.id }) {
            WatchCenter.shared.finds[i] = WatchFind(watch: f.watch, hit: { var h = f.hit; h.inLibrary = id; return h }())
        }
        return id
    }
}

// MARK: - Views

/// "New for You": what the watches found, grouped by watch.
struct WatchInbox: View {
    @EnvironmentObject var model: LibraryModel
    @ObservedObject var center = WatchCenter.shared
    @Binding var selection: String?
    @State private var addingWatch = false

    var body: some View {
        List(selection: $selection) {
            Section {
                HStack(spacing: 10) {
                    Image(systemName: "bell.badge").font(.title2).foregroundStyle(Color.marginsTeal)
                    VStack(alignment: .leading, spacing: 2) {
                        Text("New for You").font(.headline)
                        Text(center.watches.isEmpty
                             ? "Follow authors, topics or the papers citing yours, and new work shows up here."
                             : "Checked on OpenAlex when Margins opens and twice a day.")
                            .font(.caption).foregroundStyle(.secondary)
                    }
                    Spacer()
                    if center.checking { ProgressView().controlSize(.small) }
                    else {
                        Button { Task { await center.check() } } label: { Image(systemName: "arrow.clockwise") }
                            .buttonStyle(.borderless).help("Check now")
                    }
                    Button { addingWatch = true } label: { Label("Watch…", systemImage: "plus") }
                        .buttonStyle(.bordered)
                }
            }
            ForEach(center.watches) { w in
                let items = center.finds.filter { $0.watch == w.id }
                Section {
                    if items.isEmpty {
                        Text(w.lastChecked == nil ? "Checking…" : "Nothing new.").font(.callout).foregroundStyle(.secondary)
                    }
                    ForEach(items) { f in
                        row(f).tag(f.id)
                    }
                } header: {
                    HStack {
                        Label(w.describe, systemImage: w.icon)
                        Spacer()
                        Menu {
                            if !items.isEmpty { Button("Dismiss All") { center.dismissAll(of: w.id) } }
                            Button("Stop Watching", role: .destructive) { center.remove(w.id) }
                        } label: { Image(systemName: "ellipsis.circle") }
                        .menuStyle(.borderlessButton).fixedSize()
                    }
                }
            }
        }
        .sheet(isPresented: $addingWatch) { NewWatchView().environmentObject(model) }
    }

    private func row(_ f: WatchFind) -> some View {
        HStack(alignment: .top) {
            #if os(iOS)
            NavigationLink { DiscoverDetail(hit: f.hit) } label: { DiscoverRow(hit: f.hit) }
            #else
            DiscoverRow(hit: f.hit)
            #endif
        }
        .swipeActions { Button("Dismiss", role: .destructive) { center.dismiss(f) } }
        .contextMenu { Button("Dismiss") { center.dismiss(f) } }
    }
}

struct NewWatchView: View {
    @EnvironmentObject var model: LibraryModel
    @Environment(\.dismiss) private var dismiss
    @State var kind: Watch.Kind = .topic
    @State var value = ""

    var body: some View {
        NavigationStack {
            Form {
                Picker("Watch", selection: $kind) {
                    Text("A topic").tag(Watch.Kind.topic)
                    Text("An author").tag(Watch.Kind.author)
                    Text("Papers citing…").tag(Watch.Kind.citing)
                }
                .pickerStyle(.segmented)
                TextField(kind == .topic ? "Words, e.g. velocity-based training" : kind == .author ? "Author's name" : "DOI of the paper", text: $value)
                    .autocorrectionDisabled()
                if kind == .citing {
                    Text("Tip: right-click a paper in your library › Watch for Papers Citing This.").font(.caption).foregroundStyle(.secondary)
                }
            }
            .formStyle(.grouped)
            .navigationTitle("New Watch")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Watch") {
                        let v = value.trimmingCharacters(in: .whitespacesAndNewlines)
                        WatchCenter.shared.add(Watch(kind: kind, value: v, label: v))
                        dismiss()
                    }
                    .disabled(value.trimmingCharacters(in: .whitespaces).isEmpty)
                }
            }
        }
        #if os(macOS)
        .frame(minWidth: 420, minHeight: 240)
        #endif
    }
}

/// Context-menu items to start watching from a paper.
struct WatchMenu: View {
    let paper: Paper
    var body: some View {
        if !paper.meta.doi.isEmpty {
            Button("Watch for Papers Citing This") {
                WatchCenter.shared.add(Watch(kind: .citing, value: paper.meta.doi, label: String(paper.meta.title.prefix(70))))
            }
        }
        if let a = paper.meta.authors.first {
            Button("Follow \(a)") { WatchCenter.shared.add(Watch(kind: .author, value: a, label: a)) }
        }
        if let last = paper.meta.authors.last, paper.meta.authors.count > 1 {
            Button("Follow \(last)") { WatchCenter.shared.add(Watch(kind: .author, value: last, label: last)) }
        }
    }
}
