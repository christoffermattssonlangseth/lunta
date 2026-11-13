import SwiftUI
import LuntaCore

// Both LuntaCore and Paper2EpubKit define a PaperMeta; in the app it means the library's.
typealias PaperMeta = LuntaCore.PaperMeta

@main
struct LuntaiOSApp: App {
    @StateObject private var holder = ModelHolder()
    @Environment(\.scenePhase) private var phase

    var body: some Scene {
        WindowGroup {
            Group {
                if let model = holder.model {
                    LibraryView().environmentObject(model)
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
        }
        .onChange(of: phase) { _, p in
            if p == .active, let m = holder.model { m.refresh(); Task { await m.importShared() } }
        }
    }
}

@MainActor
final class ModelHolder: ObservableObject {
    @Published var model: LibraryModel?
    @Published var error: String?
    @Published var progress = "Opening library…"

    init() {
        Task {
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
                if let model { Task { await model.importShared() }; WatchCenter.shared.start(model) }
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
    func importShared() async {
        let items = ShareInbox.pending()
        guard !items.isEmpty, !importingShared else { return }
        importingShared = true
        defer { importingShared = false }
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
