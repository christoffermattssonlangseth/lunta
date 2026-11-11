import SwiftUI
import MarginsCore

// Both MarginsCore and Paper2EpubKit define a PaperMeta; in the app it means the library's.
typealias PaperMeta = MarginsCore.PaperMeta

@main
struct MarginsiOSApp: App {
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
            .tint(.marginsTeal)
        }
        .onChange(of: phase) { _, p in if p == .active { holder.model?.refresh() } }
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
            NSLog("Margins: resolving iCloud container")
            let (url, cloud) = await Task.detached { CloudLibrary.resolve() }.value
            NSLog("Margins: library at %@ (iCloud: %d) after %.1fs", url.path, cloud ? 1 : 0, Date().timeIntervalSince(t0))
            if cloud {
                await CloudLibrary.prepareLogs(url) { text in Task { @MainActor in self.progress = text } }
                NSLog("Margins: change logs ready after %.1fs", Date().timeIntervalSince(t0))
            }
            progress = "Opening library…"
            do {
                let m = try await Task.detached { try LibraryModel.openLibrary(root: url) }.value
                NSLog("Margins: core opened after %.1fs", Date().timeIntervalSince(t0))
                model = LibraryModel(lib: m, root: url, inCloud: cloud)
                NSLog("Margins: ready after %.1fs, %d papers", Date().timeIntervalSince(t0), model?.counts.all ?? -1)
            } catch {
                NSLog("Margins: open failed: %@", error.localizedDescription)
                self.error = error.localizedDescription
            }
        }
    }
}
