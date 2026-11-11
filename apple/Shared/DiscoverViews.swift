import SwiftUI
import MarginsCore

extension DiscoverHit: Identifiable {
    public var id: String { meta.doi.isEmpty ? meta.title : meta.doi }
}

/// Banner that makes it obvious the results come from outside the library.
struct BeyondBanner: View {
    var body: some View {
        HStack(spacing: 10) {
            Image(systemName: "globe.europe.africa.fill").font(.title2).foregroundStyle(Color.marginsTeal)
            VStack(alignment: .leading, spacing: 2) {
                Text("Beyond your library").font(.headline)
                Text("Searching 250 million papers on OpenAlex. Add any of them to Margins.")
                    .font(.caption).foregroundStyle(.secondary)
            }
            Spacer(minLength: 0)
        }
        .padding(12)
        .background(RoundedRectangle(cornerRadius: 12, style: .continuous).fill(Color.marginsTeal.opacity(0.10)))
        .overlay(RoundedRectangle(cornerRadius: 12, style: .continuous).strokeBorder(Color.marginsTeal.opacity(0.25), style: StrokeStyle(lineWidth: 1, dash: [4, 3])))
    }
}

struct DiscoverRow: View {
    @EnvironmentObject var model: LibraryModel
    let hit: DiscoverHit
    @State private var adding = false

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            VStack(alignment: .leading, spacing: 4) {
                Text(hit.meta.title).font(.body.weight(.medium)).lineLimit(3)
                Text([hit.meta.authorLine, hit.meta.journal, hit.meta.year].filter { !$0.isEmpty }.joined(separator: " · "))
                    .font(.caption).foregroundStyle(.secondary).lineLimit(1)
                HStack(spacing: 4) {
                    if hit.openAccess { Pill(text: "Open access", icon: "lock.open", color: .green) }
                    if hit.citedBy > 0 { Pill(text: "Cited \(hit.citedBy)", color: .secondary) }
                }
            }
            Spacer(minLength: 6)
            if hit.inLibrary != nil {
                Label("In library", systemImage: "checkmark.circle.fill")
                    .labelStyle(.iconOnly).foregroundStyle(Color.marginsTeal).font(.title3)
                    .help("Already in your library")
            } else if adding {
                ProgressView().controlSize(.small)
            } else {
                Button {
                    adding = true
                    Task { await model.addDiscovered(hit); adding = false }
                } label: { Label("Add", systemImage: "plus.circle.fill").labelStyle(.iconOnly).font(.title3) }
                .buttonStyle(.borderless).foregroundStyle(Color.marginsTeal)
                .help("Add to your library")
            }
        }
        .padding(.vertical, 4)
    }
}

struct DiscoverDetail: View {
    @EnvironmentObject var model: LibraryModel
    let hit: DiscoverHit
    @State private var adding = false

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 14) {
                HStack(spacing: 6) {
                    Pill(text: "Not in your library", icon: "globe", color: .marginsTeal)
                    if hit.openAccess { Pill(text: "Open access", icon: "lock.open", color: .green) }
                }
                Text(hit.meta.title).font(.title2.weight(.bold)).textSelection(.enabled)
                if !hit.meta.authors.isEmpty {
                    Text(hit.meta.authors.prefix(12).joined(separator: ", ") + (hit.meta.authors.count > 12 ? " et al." : ""))
                        .font(.callout).foregroundStyle(.secondary)
                }
                Text([hit.meta.journal, hit.meta.year, hit.citedBy > 0 ? "cited \(hit.citedBy)×" : ""].filter { !$0.isEmpty }.joined(separator: " · "))
                    .font(.caption).foregroundStyle(.secondary)
                if hit.inLibrary == nil {
                    Button {
                        adding = true
                        Task { await model.addDiscovered(hit); adding = false }
                    } label: { Label(adding ? "Adding…" : "Add to Library", systemImage: "plus") }
                    .buttonStyle(.borderedProminent).tint(.marginsTeal).disabled(adding)
                } else {
                    Label("In your library", systemImage: "checkmark.circle.fill").foregroundStyle(Color.marginsTeal)
                }
                if !hit.meta.abstractText.isEmpty {
                    Card(title: "Abstract") { Text(hit.meta.abstractText).lineSpacing(3).textSelection(.enabled) }
                }
                if !hit.meta.doi.isEmpty, let u = URL(string: "https://doi.org/\(hit.meta.doi)") {
                    Link("doi:\(hit.meta.doi)", destination: u).font(.callout).tint(.marginsTeal)
                }
            }
            .padding(22)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}
