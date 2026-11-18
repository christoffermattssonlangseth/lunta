import SwiftUI
import SafariServices

/// Safari never turns an app's extension on by itself (the user ticks it in Safari › Settings ›
/// Extensions), and it can be off again after Lunta is updated. Lunta checks when it starts and
/// when it comes to the front, and offers to open Safari's settings at the Lunta extension.
@MainActor
final class SafariExtensionState: ObservableObject {
    static let shared = SafariExtensionState()
    static let identifier = "com.cmlangseth.margins.safari"

    /// nil until checked; false when the extension is off (and Safari is installed)
    @Published private(set) var enabled: Bool?
    @AppStorage("safariExtensionPromptOff") var promptOff = false
    @Published var dismissedThisRun = false

    var showPrompt: Bool { enabled == false && !promptOff && !dismissedThisRun }

    func check() {
        SFSafariExtensionManager.getStateOfSafariExtension(withIdentifier: Self.identifier) { state, error in
            let on = error == nil ? (state?.isEnabled ?? false) : nil
            Task { @MainActor in
                // an error (no Safari, the extension not registered yet): say nothing
                self.enabled = on
            }
        }
    }

    func openSafariSettings() {
        SFSafariApplication.showPreferencesForExtension(withIdentifier: Self.identifier) { _ in
            // when the user comes back to Lunta, it checks again
        }
    }
}

/// A slim bar under the paper list: "The Lunta extension is off in Safari  [Turn On in Safari…] ✕".
struct SafariExtensionBar: View {
    @ObservedObject private var safari = SafariExtensionState.shared

    var body: some View {
        if safari.showPrompt {
            HStack(spacing: 10) {
                Image(systemName: "safari").foregroundStyle(Color.luntaTeal)
                Text("The Lunta extension is off in Safari. Turn it on to save papers and PDFs from any journal page.")
                    .font(.callout).lineLimit(2)
                Spacer(minLength: 8)
                Button("Turn On in Safari…") { safari.openSafariSettings() }
                    .buttonStyle(.borderedProminent).tint(.luntaTeal).controlSize(.small)
                Menu {
                    Button("Not Now") { safari.dismissedThisRun = true }
                    Button("Don’t Show Again") { safari.promptOff = true }
                } label: { Image(systemName: "xmark") }
                .menuStyle(.borderlessButton).menuIndicator(.hidden).fixedSize()
                .help("Hide")
            }
            .padding(.horizontal, 12).padding(.vertical, 7)
            .background(Color.luntaTeal.opacity(0.10))
            .overlay(alignment: .top) { Divider() }
        }
    }
}

extension View {
    /// Checks the Safari extension's state at start and whenever Lunta becomes active.
    func checksSafariExtension() -> some View {
        self.task { SafariExtensionState.shared.check() }
            .onReceive(NotificationCenter.default.publisher(for: NSApplication.didBecomeActiveNotification)) { _ in
                SafariExtensionState.shared.check()
            }
    }
}
