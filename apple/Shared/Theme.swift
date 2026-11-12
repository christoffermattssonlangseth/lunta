import SwiftUI

/// Colours from the Lunta logo.
extension Color {
    static let luntaTeal = Color(red: 0.165, green: 0.392, blue: 0.471)     // #2a6478
    static let luntaCopper = Color(red: 0.761, green: 0.439, blue: 0.239)   // #c2703d
    static let luntaHighlight = Color(red: 0.949, green: 0.706, blue: 0.353) // #f2b45a
}

/// Small rounded badge: "Reading view", "PDF", "3 notes", journal names.
struct Pill: View {
    let text: String
    var icon: String?
    var color: Color = .luntaTeal

    var body: some View {
        HStack(spacing: 3) {
            if let icon { Image(systemName: icon).imageScale(.small) }
            Text(text)
        }
        .font(.caption2.weight(.semibold))
        .padding(.horizontal, 6).padding(.vertical, 2)
        .foregroundStyle(color)
        .background(Capsule().fill(color.opacity(0.13)))
    }
}

/// Rounded section used in the detail pane.
struct Card<Content: View>: View {
    let title: String
    @ViewBuilder var content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(title.uppercased()).font(.caption.weight(.semibold)).foregroundStyle(.secondary).tracking(0.6)
            content
        }
        .padding(14)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: 12, style: .continuous).fill(.background.secondary))
    }
}
