import SwiftUI

extension Notification.Name {
    /// A paper's reading view was made or remade on this device (object: the paper's id): a reader
    /// that has the paper open shows the new one.
    static let luntaReadingViewMade = Notification.Name("luntaReadingViewMade")
}

/// Papers whose reading views are to be made again, waiting for Remake or Cancel.
struct RemakeRequest: Identifiable {
    let id = UUID()
    let ids: [String]
    /// How many of them have a reading view now (the others get their first).
    let existing: Int
}

/// "Make Reading View" and "Remake Reading View…": making one where there is none goes ahead;
/// replacing one asks first (`RemakeReadingViewAlert`). The library's menus post their request
/// here; a reader or a paper's info page keeps its own.
@MainActor
final class RemakeReadingView: ObservableObject {
    static let shared = RemakeReadingView()
    @Published var request: RemakeRequest?

    /// The menu item's title: "Make Reading View(s)" when none of them has one, else
    /// "Remake Reading View(s)…" (it asks first).
    static func title(_ ids: [String], model: LibraryModel) -> String {
        let noun = ids.count == 1 ? "Reading View" : "Reading Views"
        return existing(ids, model: model) > 0 ? "Remake \(noun)…" : "Make \(noun)"
    }

    static func existing(_ ids: [String], model: LibraryModel) -> Int {
        ids.filter { model.paper($0)?.hasEpub == true }.count
    }

    /// From the library: makes them at once (progress in the status bar), or asks first when
    /// any of them has a reading view.
    func makeOrAsk(_ ids: [String], model: LibraryModel) {
        guard !ids.isEmpty else { return }
        let n = Self.existing(ids, model: model)
        if n == 0 { Task { await model.makeReadingViews(ids) } } else { request = RemakeRequest(ids: ids, existing: n) }
    }

    static func alertTitle(_ r: RemakeRequest) -> String {
        r.ids.count == 1 ? "Remake the reading view?" : "Remake the reading views?"
    }

    /// Highlights are found again by their quoted words and the words around them
    /// (`quote`/`prefix`/`suffix`, docs/development.md); those that aren't stay listed with their
    /// quote and note ("Not found in this reading view"). A conversion that fails replaces nothing.
    static func alertMessage(_ r: RemakeRequest) -> String {
        let sources = "the best source it can find now (the publisher’s text, arXiv or the PDF)"
        let highlights = "Your highlights and notes are kept and found again in the new text by their words; any that can’t be found stay in Highlights & Notes, marked as not found."
        if r.ids.count == 1 {
            return "Lunta makes the reading view again from \(sources). \(highlights) If no new reading view can be made, the current one stays."
        }
        let which = r.existing == r.ids.count ? "\(r.ids.count) papers"
            : "\(r.ids.count) papers (\(r.existing) of them have one now)"
        return "Lunta makes the reading views of the \(which) again from \(sources). \(highlights) A paper whose reading view can’t be made again keeps its current one."
    }
}

/// The "Remake the reading view?" alert: Remake / Cancel.
struct RemakeReadingViewAlert: ViewModifier {
    @Binding var request: RemakeRequest?
    let remake: ([String]) -> Void

    func body(content: Content) -> some View {
        content.alert(request.map(RemakeReadingView.alertTitle) ?? "",
                      isPresented: Binding(get: { request != nil }, set: { if !$0 { request = nil } }),
                      presenting: request) { r in
            Button("Remake") { request = nil; remake(r.ids) }
            Button("Cancel", role: .cancel) { request = nil }
        } message: { r in
            Text(RemakeReadingView.alertMessage(r))
        }
    }
}

/// The library's alert (its menus post to `RemakeReadingView.shared`).
struct LibraryRemakeAlert: ViewModifier {
    @EnvironmentObject var model: LibraryModel
    @ObservedObject private var ui = RemakeReadingView.shared

    func body(content: Content) -> some View {
        content.modifier(RemakeReadingViewAlert(request: $ui.request) { ids in
            Task { await model.makeReadingViews(ids) }
        })
    }
}

/// In a reader: the alert, a capsule at the top while the reading view is being made (then how it
/// went), and the new reading view when it's there (made here, from the library or Paper Info).
struct RemakeProgress: ViewModifier {
    let paperID: String
    @Binding var request: RemakeRequest?
    let text: String?
    let busy: Bool
    let remake: () -> Void
    let made: () -> Void

    func body(content: Content) -> some View {
        content
            .overlay(alignment: .top) {
                if let text {
                    HStack(spacing: 8) {
                        if busy { ProgressView().controlSize(.small) }
                        Text(text).font(.callout)
                    }
                    .padding(.horizontal, 14).padding(.vertical, 8)
                    .background(.regularMaterial, in: Capsule())
                    .padding(.horizontal, 12).padding(.top, 10)
                    .transition(.move(edge: .top).combined(with: .opacity))
                }
            }
            .animation(.easeOut(duration: 0.2), value: text)
            .modifier(RemakeReadingViewAlert(request: $request) { _ in remake() })
            .onReceive(NotificationCenter.default.publisher(for: .luntaReadingViewMade)) { n in
                if n.object as? String == paperID { made() }
            }
    }
}
