import SwiftUI
import LuntaCore

// Both LuntaCore and Paper2EpubKit define a PaperMeta; in the app it means the library's.
typealias PaperMeta = LuntaCore.PaperMeta

@main
struct LuntaiOSApp: App {
    @StateObject private var holder = ModelHolder()
    @Environment(\.scenePhase) private var phase

    @AppStorage(AppAppearance.key) private var appearance = AppAppearance.system.rawValue

    var body: some Scene {
        WindowGroup {
            Group {
                if let model = holder.model {
                    AdaptiveLibrary().environmentObject(model)
                        .alert(FoundInICloud.title, isPresented: $holder.foundInICloud) {
                            Button(FoundInICloud.switchButton) { holder.showStorage = true }
                            Button("OK", role: .cancel) {}
                        } message: { Text(FoundInICloud.message) }
                        .sheet(isPresented: $holder.showStorage) { LibraryStorageSheet().environmentObject(model) }
                } else if let ask = holder.asking {
                    StorageChoiceView(recommended: ask.recommended, iCloudAvailable: ask.iCloudAvailable) { holder.choose($0) }
                } else if let error = holder.error {
                    ContentUnavailableView("Can't open the library", systemImage: "exclamationmark.triangle", description: Text(error))
                } else {
                    VStack(spacing: 14) {
                        ProgressView()
                        Text(holder.progress).font(.callout).foregroundStyle(.secondary).multilineTextAlignment(.center)
                    }
                    .padding()
                }
            }
            .tint(.luntaTeal)
            .preferredColorScheme((AppAppearance(rawValue: appearance) ?? .system).scheme)
            // lunta://safari (the Safari extension's "Open Lunta"), lunta://open?id=…: do what
            // Safari sent (also done whenever the app becomes active)
            .onOpenURL { _ in if let m = holder.model { Task { await m.importShared() } } }
        }
        .onChange(of: phase) { _, p in
            if p == .active, let m = holder.model {
                m.refresh(); m.freshenReadingViews(); Task { await m.importShared() }
                DriveLibrarySync.shared.cameToFront()   // a library in Google Drive: sync unless it just did
            }
            if p == .background { MorningNotice.schedule() }
        }
        .backgroundTask(.appRefresh(MorningNotice.taskID)) { await MorningNotice.refresh() }
    }

    init() {
        MorningNotice.shared.install()
    }
}

/// The first-launch question's settings (StorageChoiceView).
struct StorageAsk: Equatable {
    var recommended: LibraryStorage
    var iCloudAvailable: Bool
}

/// Library Storage (where the library lives, moving it), from the library's list or the
/// found-in-iCloud alert.
struct LibraryStorageSheet: View {
    @Environment(\.dismiss) private var dismiss
    @ObservedObject private var mover = LibraryMover.shared
    var body: some View {
        NavigationStack {
            LibraryStorageView()
                .navigationTitle("Library Storage")
                .navigationBarTitleDisplayMode(.inline)
                .toolbar { ToolbarItem(placement: .confirmationAction) { Button("Done") { mover.clear(); dismiss() }.disabled(mover.working) } }
        }
        .interactiveDismissDisabled(mover.working)
    }
}

@MainActor
final class ModelHolder: ObservableObject {
    @Published var model: LibraryModel?
    @Published var error: String?
    @Published var progress = "Opening library…"
    /// No library on this device yet: where should it live? (StorageDecision.swift)
    @Published var asking: StorageAsk?
    /// A new device found the library in iCloud and opened it: say so once.
    @Published var foundInICloud = false
    @Published var showStorage = false

    init() {
        LibraryMover.shared.reopen = { [weak self] _ in Task { await self?.reopen() } }
        Task {
            let (decision, iCloud) = await LibraryLocation.startupDecision { line in Task { @MainActor in self.progress = line } }
            switch decision {
            case .ask(let recommended):
                asking = StorageAsk(recommended: recommended, iCloudAvailable: iCloud)
                return   // choose(_:) opens the library
            case .openFoundInICloud:
                LibraryStorage.stored = .iCloud
                foundInICloud = true
            case .open, .lookInICloud:
                break
            }
            await open()
        }
    }

    /// The first-launch answer (for Google Drive, after signing in).
    func choose(_ s: LibraryStorage) {
        LibraryStorage.stored = s
        asking = nil
        Task { await open() }
    }

    /// The library moved (Library Storage): open it in its new place. The old model stays on
    /// screen until the new one is ready.
    func reopen() async {
        DriveLibrarySync.shared.stop()
        let (url, cloud) = await Task.detached { CloudLibrary.resolve() }.value
        if cloud { await CloudLibrary.prepareLogs(url) { _ in } }
        do {
            let lib = try await Task.detached { try LibraryModel.openLibrary(root: url) }.value
            let m = LibraryModel(lib: lib, root: url, inCloud: cloud)
            model = m
            started(m)
        } catch {
            LibraryMover.shared.failedToReopen(error.readable)
        }
    }

    /// What runs once a library is open.
    private func started(_ model: LibraryModel) {
        // the citation card in a reader: add the cited paper (or say it's already there)
        ExternalLinks.onLunta = { url, web in Task { @MainActor in web.citationDone(await model.paperForCitation(url).label) } }
        Task { await model.importShared() }
        WatchCenter.shared.start(model)
        model.freshenReadingViews()
        if LibraryStorage.stored == .googleDrive { DriveLibrarySync.shared.start(model) }
    }

    private func open() async {
        do {
            let t0 = Date()
            NSLog("Lunta: resolving iCloud container")
            let (url, cloud) = await Task.detached { CloudLibrary.resolve() }.value
            NSLog("Lunta: library at %@ (iCloud: %d) after %.1fs", url.path, cloud ? 1 : 0, Date().timeIntervalSince(t0))
            if cloud {
                await CloudLibrary.prepareLogs(url) { text in Task { @MainActor in self.progress = text } }
                NSLog("Lunta: change logs ready after %.1fs", Date().timeIntervalSince(t0))
            }
            progress = "Opening library…"
            do {
                let m = try await Task.detached { try LibraryModel.openLibrary(root: url) }.value
                NSLog("Lunta: core opened after %.1fs", Date().timeIntervalSince(t0))
                model = LibraryModel(lib: m, root: url, inCloud: cloud)
                NSLog("Lunta: ready after %.1fs, %d papers", Date().timeIntervalSince(t0), model?.counts.all ?? -1)
                if let model { started(model) }
                if ProcessInfo.processInfo.arguments.contains("-testDownload"), let model {
                    // diagnostics: fetch a few reading views from iCloud and report how long it took
                    for p in model.papers.filter(\.hasEpub).prefix(4) {
                        let t = Date()
                        let before = model.needsDownload(p.id, .epub)
                        let url = await model.ensureLocal(p.id, .epub)
                        print("download test: needed=\(before) ok=\(url != nil) \(String(format: "%.1f", Date().timeIntervalSince(t)))s \(p.meta.title.prefix(40))")
                    }
                    print("download test: done"); fflush(stdout)
                }
                if ProcessInfo.processInfo.arguments.contains("-testVoice") {
                    var t = Date()
                    let ok = await PhoneVoice.shared.prepare()
                    print("voice test: prepared=\(ok) \(String(format: "%.1f", Date().timeIntervalSince(t)))s"); fflush(stdout)
                    for text in ["Methods", "CSF lactate was higher in RRMS patients compared to controls. A negative correlation was found between CSF lactate and the progression index, and higher lactate levels were associated with greater disability over the follow-up period."] {
                        t = Date()
                        let u = await PhoneVoice.shared.clip(text, kind: "text")
                        let size = (try? FileManager.default.attributesOfItem(atPath: u?.path ?? "")[.size] as? Int) ?? 0
                        print("voice test: clip ok=\(u != nil) \(size / 48000)s audio in \(String(format: "%.2f", Date().timeIntervalSince(t)))s"); fflush(stdout)
                    }
                }
            } catch {
                NSLog("Lunta: open failed: %@", error.readable)
                self.error = error.readable
            }
        }
    }
}

extension LibraryModel {
    /// Adds what was shared to Lunta from other apps (PDFs and links waiting in the share inbox).
    /// Also does what was sent from Safari (SafariInbox: save, PDF, "Use this page's full text").
    func importShared() async {
        guard !importingShared else { return }
        importingShared = true
        defer { importingShared = false }
        let safari = await importSafari()
        if safari.show { errorMessage = safari.lines.joined(separator: "\n\n") }
        let items = ShareInbox.pending()
        guard !items.isEmpty else { return }
        var pdfs: [(URL, ShareInbox.Item)] = []
        for item in items {
            switch item.kind {
            case .pdf(let u): pdfs.append((u, item))
            case .link(let s):
                await add(s)
                ShareInbox.remove(item)
            }
        }
        if !pdfs.isEmpty {
            // copy out of the inbox first: adding may take a while and the inbox file is removed after
            let tmp = FileManager.default.temporaryDirectory
            var urls: [URL] = []
            for (u, item) in pdfs {
                let name = u.lastPathComponent.split(separator: "-", maxSplits: 2).last.map(String.init) ?? u.lastPathComponent
                let dest = tmp.appendingPathComponent(name)
                try? FileManager.default.removeItem(at: dest)
                if (try? FileManager.default.copyItem(at: u, to: dest)) != nil { urls.append(dest) }
                ShareInbox.remove(item)
            }
            _ = await addPDFs(urls)
        }
    }
}
