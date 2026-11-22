import SwiftUI
#if os(macOS)
import AppKit
#endif

/// Lunta's own light/dark choice (Settings › Appearance): follow the system, or always light or dark,
/// whatever the rest of the device uses. The reader's "Automatic" theme follows this too.
enum AppAppearance: String, CaseIterable, Identifiable {
    case system, light, dark
    static let key = "appAppearance"
    var id: String { rawValue }
    var name: String {
        switch self { case .system: return "System"; case .light: return "Light"; case .dark: return "Dark" }
    }
    var scheme: ColorScheme? {
        switch self { case .system: return nil; case .light: return .light; case .dark: return .dark }
    }
    static var current: AppAppearance { AppAppearance(rawValue: UserDefaults.standard.string(forKey: key) ?? "") ?? .system }

    #if os(macOS)
    /// Applies the choice to every window of the app.
    @MainActor static func apply(_ a: AppAppearance = current) {
        switch a {
        case .system: NSApp.appearance = nil
        case .light: NSApp.appearance = NSAppearance(named: .aqua)
        case .dark: NSApp.appearance = NSAppearance(named: .darkAqua)
        }
    }
    #endif
}

/// The appearance picker, used in the Mac's Settings window and the iPhone/iPad library.
struct AppearancePicker: View {
    @AppStorage(AppAppearance.key) private var raw = AppAppearance.system.rawValue

    var body: some View {
        Picker("Appearance", selection: $raw) {
            ForEach(AppAppearance.allCases) { a in Text(a.name).tag(a.rawValue) }
        }
        #if os(macOS)
        .pickerStyle(.radioGroup)
        .onChange(of: raw) { _, v in AppAppearance.apply(AppAppearance(rawValue: v) ?? .system) }
        #endif
    }
}

#if os(macOS)
/// Lunta › Settings… (⌘,)
struct AppSettingsView: View {
    var body: some View {
        Form {
            LibraryStorageSettingRow()
            Divider().padding(.vertical, 6)
            AppearancePicker()
            Text("System follows your Mac's light or dark mode. The reader's own themes (Paper, Quiet, Black…) are in the reader toolbar.")
                .font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            Divider().padding(.vertical, 6)
            SummarySettingToggle()   // Summary.swift
            Text(SummarySettingToggle.explanation)
                .font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            Divider().padding(.vertical, 6)
            BibtexAuthorsSetting()   // Mac/BibFiles.swift
            Text(BibtexAuthorsSetting.explanation)
                .font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            Divider().padding(.vertical, 6)
            UpdateSettings()   // Mac/Updates.swift
        }
        .padding(24)
        .frame(width: 420)
    }
}

/// Settings: where the library is, and the way to Library Storage (moving it).
struct LibraryStorageSettingRow: View {
    @Environment(\.openWindow) private var openWindow
    var body: some View {
        LabeledContent("Library") {
            HStack {
                Text(LibraryStorage.stored == .folder
                     ? "Folder “\(ChosenFolder.storedPath.map { ($0 as NSString).lastPathComponent } ?? "")”"
                     : LibraryStorage.stored?.name ?? "Automatic (iCloud Drive when available)")
                Spacer()
                Button("Change…") { openWindow(id: "storage") }
            }
        }
        if LibraryStorage.stored == .googleDrive {
            DriveOnDemandSetting()
        }
    }
}

/// Settings: "Download papers when you open them" for a library in Google Drive (also in Library Storage).
struct DriveOnDemandSetting: View {
    @AppStorage(DriveLibrarySync.onDemandKey) private var onDemand = DriveLibrarySync.onDemandDefault
    var body: some View {
        Toggle("Download papers when you open them", isOn: $onDemand)
            .onChange(of: onDemand) { _, _ in DriveLibrarySync.shared.onDemandChanged() }
        Text(DriveDownloadsView.explanation).font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
    }
}
#endif
