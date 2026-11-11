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
                    ProgressView("Opening library…")
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

    init() {
        Task {
            let (url, cloud) = await Task.detached { CloudLibrary.resolve() }.value
            do { model = try LibraryModel(root: url, inCloud: cloud) } catch { self.error = error.localizedDescription }
        }
    }
}
