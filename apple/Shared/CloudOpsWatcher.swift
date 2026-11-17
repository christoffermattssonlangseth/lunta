import Foundation

/// Hears about changes to the library's change logs (ops/*.jsonl) in iCloud as they happen, from
/// any device, and fetches them at once. iOS doesn't push a new version of a file it already has
/// promptly (a paper added on the Mac could take many minutes to appear on the iPhone); asking
/// iCloud for the new version as soon as it announces one brings it over in seconds.
@MainActor
final class CloudOpsWatcher {
    private let query = NSMetadataQuery()
    private var observers: [NSObjectProtocol] = []
    private var pending: DispatchWorkItem?
    private var lastVersions: [String: Date] = [:]
    private let onChange: () -> Void

    init(onChange: @escaping () -> Void) {
        self.onChange = onChange
        query.searchScopes = [NSMetadataQueryUbiquitousDocumentsScope]
        query.predicate = NSPredicate(format: "%K LIKE '*.jsonl'", NSMetadataItemFSNameKey)
        query.notificationBatchingInterval = 0.5
        let nc = NotificationCenter.default
        for name in [Notification.Name.NSMetadataQueryDidFinishGathering, .NSMetadataQueryDidUpdate] {
            observers.append(nc.addObserver(forName: name, object: query, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { self?.look() }
            })
        }
        query.start()
    }

    deinit {
        query.stop()
        observers.forEach(NotificationCenter.default.removeObserver)
    }

    private func look() {
        query.disableUpdates()
        defer { query.enableUpdates() }
        var changed = false
        for case let item as NSMetadataItem in query.results {
            guard let path = item.value(forAttribute: NSMetadataItemPathKey) as? String, path.contains("/ops/") else { continue }
            let status = item.value(forAttribute: NSMetadataUbiquitousItemDownloadingStatusKey) as? String
            if status != NSMetadataUbiquitousItemDownloadingStatusCurrent {
                // a newer version (or a new log) is in iCloud: fetch it now
                CloudLibrary.startDownload(URL(fileURLWithPath: path))
                continue
            }
            let date = item.value(forAttribute: NSMetadataItemFSContentChangeDateKey) as? Date ?? .distantPast
            if lastVersions[path] != date { lastVersions[path] = date; changed = true }
        }
        guard changed else { return }
        // read the logs once the batch has settled
        pending?.cancel()
        let work = DispatchWorkItem { [weak self] in self?.onChange() }
        pending = work
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.3, execute: work)
    }
}
