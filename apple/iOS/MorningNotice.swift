import SwiftUI
import BackgroundTasks
import UserNotifications
import LuntaCore

/// The morning notification on the iPhone. The Mac usually picks the day's papers (at the morning
/// time set in New for You) and writes them to foryou.json in iCloud; iOS wakes Lunta in the
/// background a little after that, and this reads the file and announces the feed with the first
/// sentence of its summary. Tapping the notification opens New for You.
///
/// iOS decides when background refreshes run (more reliably for apps used every day), so the
/// notification can come somewhat after the chosen time.
@MainActor
final class MorningNotice: NSObject, ObservableObject, UNUserNotificationCenterDelegate {
    static let shared = MorningNotice()
    static let taskID = "com.cmlangseth.margins.ios.foryou"

    /// Set when the notification was tapped; LibraryView opens New for You and clears it.
    @Published var openForYou = false

    func install() {
        UNUserNotificationCenter.current().delegate = self
    }

    /// Asks iOS to wake Lunta after the next morning time (plus a little, so the Mac has picked
    /// the papers and written the summary).
    nonisolated static func schedule(after now: Date = Date()) {
        let morning = (stateNow()?.nextMorning(after: now)) ?? Calendar.current.date(bySettingHour: 7, minute: 0, second: 0, of: now.addingTimeInterval(86400))!
        let req = BGAppRefreshTaskRequest(identifier: taskID)
        req.earliestBeginDate = morning.addingTimeInterval(20 * 60)
        BGTaskScheduler.shared.cancel(taskRequestWithIdentifier: taskID)
        do { try BGTaskScheduler.shared.submit(req) } catch { NSLog("Lunta: morning refresh not scheduled: %@", "\(error)") }
    }

    /// The background refresh: fresh foryou.json from iCloud, then the notification if there's a
    /// new feed. Always schedules the next one.
    nonisolated static func refresh() async {
        defer { schedule() }
        let (root, cloud) = CloudLibrary.resolve()
        let url = root.appendingPathComponent("foryou.json")
        if cloud {
            CloudLibrary.startDownload(url)
            _ = await CloudLibrary.download(url, timeout: 20)
        }
        guard let s = read(url), let n = WatchCenter.morningNotice(s) else { return }
        WatchCenter.post(n, feed: s.lastRun)
    }

    nonisolated private static func stateNow() -> ForYouState? {
        read(CloudLibrary.resolve().url.appendingPathComponent("foryou.json"))
    }

    nonisolated private static func read(_ url: URL) -> ForYouState? {
        guard let d = try? Data(contentsOf: url) else { return nil }
        return try? JSONDecoder.iso.decode(ForYouState.self, from: d)
    }

    // MARK: UNUserNotificationCenterDelegate

    nonisolated func userNotificationCenter(_ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse) async {
        guard response.notification.request.content.userInfo["open"] as? String == "foryou" else { return }
        await MainActor.run { self.openForYou = true }
    }

    nonisolated func userNotificationCenter(_ center: UNUserNotificationCenter, willPresent notification: UNNotification) async -> UNNotificationPresentationOptions {
        [.list]   // Lunta is open: no banner, it's in Notification Center
    }
}
