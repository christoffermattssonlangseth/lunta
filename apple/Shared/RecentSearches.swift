import SwiftUI

/// The library searches made on this device, newest first, shown as suggestions when the search
/// field is empty. A search is remembered once it has found something and the text has stayed the
/// same for a moment (so "s", "sc", "scs"… while typing aren't kept); one that extends or shortens
/// the last remembered search replaces it.
@MainActor
final class RecentSearches: ObservableObject {
    static let shared = RecentSearches()
    private static let key = "luntaRecentSearches"
    private static let limit = 8

    @Published private(set) var list: [String] = UserDefaults.standard.stringArray(forKey: key) ?? []
    private var pending: Task<Void, Never>?
    private var lastAdded: (text: String, at: Date)?

    /// Called whenever a library search has its results.
    func searched(_ query: String, found: Int) {
        pending?.cancel()
        let q = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard q.count >= 2, found > 0 else { return }
        pending = Task { [weak self] in
            try? await Task.sleep(for: .seconds(1.5))
            guard !Task.isCancelled else { return }
            self?.remember(q)
        }
    }

    /// Remembers it now (Return in the search field, or a paper opened from the results).
    func remember(_ query: String) {
        let q = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard q.count >= 2 else { return }
        var l = list.filter { $0.caseInsensitiveCompare(q) != .orderedSame }
        // refining the search just made: one entry, not one per step
        if let last = lastAdded, Date().timeIntervalSince(last.at) < 120, let first = l.first, first == last.text,
           q.lowercased().hasPrefix(first.lowercased()) || first.lowercased().hasPrefix(q.lowercased()) {
            l.removeFirst()
        }
        l.insert(q, at: 0)
        list = Array(l.prefix(Self.limit))
        lastAdded = (q, Date())
        UserDefaults.standard.set(list, forKey: Self.key)
    }

    func remove(_ q: String) {
        list.removeAll { $0 == q }
        UserDefaults.standard.set(list, forKey: Self.key)
    }

    func clear() {
        list = []
        lastAdded = nil
        UserDefaults.standard.removeObject(forKey: Self.key)
    }
}

/// The recent searches under an empty library search field (Mac and iPhone/iPad).
struct RecentSearchSuggestions: ViewModifier {
    @EnvironmentObject var model: LibraryModel
    @ObservedObject private var recent = RecentSearches.shared

    func body(content: Content) -> some View {
        content.searchSuggestions {
            if model.query.isEmpty, model.source != .discover, !recent.list.isEmpty {
                Section("Recent Searches") {
                    ForEach(recent.list, id: \.self) { q in
                        Label(q, systemImage: "clock.arrow.circlepath").searchCompletion(q)
                    }
                }
                Button("Clear Recent Searches", role: .destructive) { recent.clear() }
            }
        }
    }
}
