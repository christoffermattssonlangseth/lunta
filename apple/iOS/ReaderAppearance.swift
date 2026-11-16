import SwiftUI

/// "Text & Theme": everything about how the paper looks, in one small sheet with live changes
/// (instead of submenus in the reader's ⋯ menu, where each change took three taps).
struct ReaderAppearanceSheet: View {
    @Binding var fontScale: Double
    @Binding var paged: Bool
    @Binding var themeRaw: String
    @AppStorage(FigureSetting.key) private var cards = true
    @ObservedObject private var voice = PhoneVoice.shared
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            Form {
                Section("Text Size") {
                    HStack(spacing: 16) {
                        Button { fontScale = max(0.7, ((fontScale - 0.1) * 10).rounded() / 10) } label: {
                            Image(systemName: "textformat.size.smaller").frame(maxWidth: .infinity, minHeight: 36)
                        }
                        .disabled(fontScale <= 0.7)
                        Text("\(Int((fontScale * 100).rounded()))%").monospacedDigit().foregroundStyle(.secondary)
                        Button { fontScale = min(2.2, ((fontScale + 0.1) * 10).rounded() / 10) } label: {
                            Image(systemName: "textformat.size.larger").frame(maxWidth: .infinity, minHeight: 36)
                        }
                        .disabled(fontScale >= 2.2)
                    }
                    .buttonStyle(.borderless)
                    .font(.title3)
                }
                Section("Theme") {
                    HStack(spacing: 10) {
                        ForEach(ReaderTheme.allCases) { t in
                            Button { themeRaw = t.rawValue } label: {
                                VStack(spacing: 4) {
                                    ThemeSwatch(theme: t)
                                        .overlay(RoundedRectangle(cornerRadius: 5)
                                            .strokeBorder(themeRaw == t.rawValue ? Color.accentColor : .clear, lineWidth: 2.5))
                                    Text(t.name).font(.caption2).foregroundStyle(themeRaw == t.rawValue ? .primary : .secondary)
                                }
                                .frame(maxWidth: .infinity)
                            }
                            .buttonStyle(.plain)
                            .accessibilityAddTraits(themeRaw == t.rawValue ? .isSelected : [])
                        }
                    }
                }
                Section {
                    Picker("Layout", selection: $paged) {
                        Text("Scroll").tag(false)
                        Text("Turn Pages").tag(true)
                    }
                    .pickerStyle(.segmented)
                    Picker("Figures", selection: $cards) {
                        Text("Figures as Cards").tag(true)
                        Text("Figures in Full").tag(false)
                    }
                    .pickerStyle(.segmented)
                }
                Section("Listening") {
                    Picker("Natural Voice", selection: Binding(get: { voice.voice }, set: { voice.voice = $0 })) {
                        ForEach(PhoneVoice.voices, id: \.id) { Text($0.name).tag($0.id) }
                    }
                }
            }
            .navigationTitle("Text & Theme")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } } }
        }
    }
}
