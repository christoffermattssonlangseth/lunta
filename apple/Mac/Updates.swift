import SwiftUI
import Sparkle

/// Updates for Lunta for Mac from GitHub Releases, through Sparkle: the feed (SUFeedURL in
/// Info.plist) is appcast.xml on the latest release, made and signed by scripts/release-mac.sh.
/// Checks once a day by default (SUEnableAutomaticChecks); the switch is in Settings. Sparkle
/// remembers the switch and the last check in the app's defaults. Lunta for Mac isn't sandboxed,
/// so Sparkle needs none of its XPC services.
///
/// Debug builds don't check by themselves (a development build would offer to replace itself
/// with the release); Check for Updates… still works there.
@MainActor
final class AppUpdater: ObservableObject {
    static let shared = AppUpdater()

    let controller: SPUStandardUpdaterController
    @Published var canCheck = false
    @Published var automaticallyChecks: Bool {
        didSet { if updater.automaticallyChecksForUpdates != automaticallyChecks { updater.automaticallyChecksForUpdates = automaticallyChecks } }
    }
    private var observers: [NSKeyValueObservation] = []
    private var started: Bool

    var updater: SPUUpdater { controller.updater }

    private init() {
        #if DEBUG
        let start = false
        #else
        let start = true
        #endif
        started = start
        controller = SPUStandardUpdaterController(startingUpdater: start, updaterDelegate: nil, userDriverDelegate: nil)
        automaticallyChecks = controller.updater.automaticallyChecksForUpdates
        canCheck = controller.updater.canCheckForUpdates || !start
        observers = [
            controller.updater.observe(\.canCheckForUpdates, options: [.new]) { [weak self] u, _ in
                let can = u.canCheckForUpdates
                Task { @MainActor in self?.canCheck = can }
            },
            controller.updater.observe(\.automaticallyChecksForUpdates, options: [.new]) { [weak self] u, _ in
                let on = u.automaticallyChecksForUpdates
                Task { @MainActor in if self?.automaticallyChecks != on { self?.automaticallyChecks = on } }
            },
        ]
    }

    func checkForUpdates() {
        if !started { started = true; controller.startUpdater() }
        controller.checkForUpdates(nil)
    }
}

/// Lunta › Check for Updates…
struct CheckForUpdatesButton: View {
    @ObservedObject private var updates = AppUpdater.shared

    var body: some View {
        Button("Check for Updates…") { updates.checkForUpdates() }
            .disabled(!updates.canCheck)
    }
}

/// The switch in Settings.
struct UpdateSettings: View {
    @ObservedObject private var updates = AppUpdater.shared

    var body: some View {
        Toggle("Check for updates automatically", isOn: $updates.automaticallyChecks)
        Text("Once a day Lunta asks GitHub, where it is published, whether there is a newer version, and offers to install it. Nothing about you or your library is sent.")
            .font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
        Button("Check Now") { updates.checkForUpdates() }
            .disabled(!updates.canCheck)
    }
}
