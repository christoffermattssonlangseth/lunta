import SwiftUI

/// The reading view's colours. Light and Black force the light or dark appearance (all the
/// reader's dark styling follows it); Paper and Quiet add their own colours on top (see the
/// `html[data-theme]` rules in `ReaderAssets.css`). Automatic follows the system. PDFs are not themed.
enum ReaderTheme: String, CaseIterable, Identifiable {
    case automatic, light, paper, quiet, black
    var id: String { rawValue }

    var name: String {
        switch self {
        case .automatic: return "Automatic"
        case .light: return "Light"
        case .paper: return "Paper"
        case .quiet: return "Quiet"
        case .black: return "Black"
        }
    }

    /// The appearance the reader is shown in (nil: the system's).
    var scheme: ColorScheme? {
        switch self {
        case .automatic: return nil
        case .light, .paper: return .light
        case .quiet, .black: return .dark
        }
    }

    /// The page and text colours, for swatches and the space around the page.
    var page: Color? {
        switch self {
        case .automatic: return nil
        case .light: return .white
        case .paper: return Color(red: 0.961, green: 0.925, blue: 0.851)
        case .quiet: return Color(red: 0.165, green: 0.161, blue: 0.153)
        case .black: return .black
        }
    }
    var text: Color {
        switch self {
        case .automatic, .light: return Color(red: 0.11, green: 0.11, blue: 0.12)
        case .paper: return Color(red: 0.23, green: 0.19, blue: 0.15)
        case .quiet: return Color(red: 0.79, green: 0.77, blue: 0.73)
        case .black: return Color(red: 0.85, green: 0.84, blue: 0.81)
        }
    }

    /// Sets the theme in the page (`<html data-theme>`); run at load and whenever it changes.
    var js: String { "document.documentElement.setAttribute('data-theme', '\(rawValue)');" }

    static var stored: ReaderTheme { ReaderTheme(rawValue: UserDefaults.standard.string(forKey: "readerTheme") ?? "") ?? .automatic }
}

/// A theme choice: "Aa" in the theme's colours.
struct ThemeSwatch: View {
    let theme: ReaderTheme
    var body: some View {
        Text("Aa").font(.system(size: 13, weight: .semibold, design: .serif))
            .foregroundStyle(theme.text)
            .frame(width: 30, height: 22)
            .background(theme.page ?? Color(white: 0.5), in: RoundedRectangle(cornerRadius: 5))
            .overlay(RoundedRectangle(cornerRadius: 5).strokeBorder(.secondary.opacity(0.4)))
    }
}
