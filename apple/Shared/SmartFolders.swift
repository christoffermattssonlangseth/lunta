import SwiftUI
import MarginsCore

// Smart folders: saved searches that fill themselves ("unread + #vbt + after 2020",
// "added this month", "no PDF yet"). Kept in <library>/smart-folders.json, so they sync.

struct SmartFolder: Codable, Identifiable, Hashable {
    enum PDF: String, Codable, CaseIterable { case any, has, missing }
    var id = UUID().uuidString
    var name = ""
    var text = ""            // words searched in titles, authors, full text and notes
    var tag = ""
    var folder = ""          // collection id
    var unreadOnly = false
    var starredOnly = false
    var yearFrom = ""
    var yearTo = ""
    var addedDays = 0        // 0 = any time
    var pdf: PDF = .any
    var highlightsOnly = false

    /// Short description of the rules, e.g. "unread · #vbt · 2020–".
    var summary: String {
        var parts: [String] = []
        if !text.isEmpty { parts.append("“\(text)”") }
        if unreadOnly { parts.append("unread") }
        if starredOnly { parts.append("starred") }
        if !tag.isEmpty { parts.append("#\(tag)") }
        if !yearFrom.isEmpty || !yearTo.isEmpty { parts.append("\(yearFrom)–\(yearTo)") }
        if addedDays > 0 { parts.append("added in the last \(addedDays) days") }
        if pdf == .has { parts.append("with PDF") }
        if pdf == .missing { parts.append("no PDF") }
        if highlightsOnly { parts.append("highlighted") }
        return parts.isEmpty ? "all papers" : parts.joined(separator: " · ")
    }

    /// The papers that match, in the given order.
    func papers(_ lib: Library, sort: SortKey) -> [Paper] {
        var list = lib.papers(filter: PaperFilter(collection: folder.isEmpty ? nil : folder, tag: tag.isEmpty ? nil : tag,
                                                  starredOnly: starredOnly, unreadOnly: unreadOnly), sort: sort)
        let words = text.trimmingCharacters(in: .whitespaces)
        if !words.isEmpty {
            let hits = Set(((try? lib.search(query: words, limit: 2000)) ?? []).map(\.paper.id))
            list = list.filter { hits.contains($0.id) }
        }
        if let from = Int(yearFrom) { list = list.filter { (Int($0.meta.year) ?? 0) >= from } }
        if let to = Int(yearTo) { list = list.filter { (Int($0.meta.year) ?? 9999) <= to } }
        if addedDays > 0 {
            let since = Int64((Date().timeIntervalSince1970 - Double(addedDays) * 86400) * 1000)
            list = list.filter { $0.added >= since }
        }
        switch pdf {
        case .any: break
        case .has: list = list.filter(\.hasPdf)
        case .missing: list = list.filter { !$0.hasPdf }
        }
        if highlightsOnly { list = list.filter { $0.annotationCount > 0 } }
        return list
    }
}

extension LibraryModel {
    private var smartURL: URL { libraryURL.appendingPathComponent("smart-folders.json") }

    func loadSmartFolders() {
        CloudLibrary.startDownload(smartURL)
        guard let d = try? Data(contentsOf: smartURL),
              let list = try? JSONDecoder().decode([SmartFolder].self, from: d) else { return }
        if list != smartFolders { smartFolders = list }
    }

    private func saveSmartFolders() {
        let enc = JSONEncoder()
        enc.outputFormatting = [.prettyPrinted, .sortedKeys]
        try? enc.encode(smartFolders).write(to: smartURL, options: .atomic)
    }

    func saveSmartFolder(_ f: SmartFolder) {
        loadSmartFolders()    // pick up edits from the other device first
        if let i = smartFolders.firstIndex(where: { $0.id == f.id }) { smartFolders[i] = f } else { smartFolders.append(f) }
        smartFolders.sort { $0.name.localizedCaseInsensitiveCompare($1.name) == .orderedAscending }
        saveSmartFolders()
        if source == .smart(f.id) { reloadList() }
    }

    func deleteSmartFolder(_ id: String) {
        loadSmartFolders()
        smartFolders.removeAll { $0.id == id }
        saveSmartFolders()
        if source == .smart(id) { source = .all }
    }

    func smartFolder(_ id: String) -> SmartFolder? { smartFolders.first { $0.id == id } }

    static let smartExamples: [SmartFolder] = [
        SmartFolder(name: "Added this month", addedDays: 31),
        SmartFolder(name: "Unread, last 3 years", unreadOnly: true, yearFrom: String(Calendar.current.component(.year, from: Date()) - 2)),
        SmartFolder(name: "No PDF yet", pdf: .missing),
        SmartFolder(name: "Highlighted", highlightsOnly: true),
    ]
}

// MARK: - Editor

struct SmartFolderEditor: View {
    @EnvironmentObject var model: LibraryModel
    @Environment(\.dismiss) private var dismiss
    @State var folder: SmartFolder
    var isNew = false

    var body: some View {
        let count = folder.papers(model.lib, sort: .added).count
        NavigationStack {
            Form {
                Section {
                    TextField("Name", text: $folder.name)
                }
                if isNew {
                    Section("Start from") {
                        ScrollView(.horizontal, showsIndicators: false) {
                            HStack {
                                ForEach(LibraryModel.smartExamples, id: \.name) { ex in
                                    Button(ex.name) { var f = ex; f.id = folder.id; folder = f }
                                        .buttonStyle(.bordered).font(.callout)
                                }
                            }
                        }
                    }
                }
                Section("Papers that match") {
                    TextField("Words (title, authors, full text, notes)", text: $folder.text)
                    Toggle("Unread only", isOn: $folder.unreadOnly)
                    Toggle("Starred only", isOn: $folder.starredOnly)
                    Toggle("With highlights only", isOn: $folder.highlightsOnly)
                    Picker("Tag", selection: $folder.tag) {
                        Text("Any").tag("")
                        ForEach(model.tags, id: \.self) { Text("#\($0)").tag($0) }
                    }
                    Picker("In folder", selection: $folder.folder) {
                        Text("Any").tag("")
                        ForEach(model.foldersFlat, id: \.folder.id) { item in
                            Text(String(repeating: "  ", count: item.depth) + item.folder.name).tag(item.folder.id)
                        }
                    }
                    HStack {
                        Text("Year")
                        Spacer()
                        TextField("from", text: $folder.yearFrom).frame(width: 70).multilineTextAlignment(.trailing)
                        Text("–")
                        TextField("to", text: $folder.yearTo).frame(width: 70)
                    }
                    #if os(iOS)
                    .keyboardType(.numberPad)
                    #endif
                    Picker("Added", selection: $folder.addedDays) {
                        Text("Any time").tag(0)
                        Text("Last 7 days").tag(7)
                        Text("Last 31 days").tag(31)
                        Text("Last 90 days").tag(90)
                        Text("Last year").tag(365)
                    }
                    Picker("PDF", selection: $folder.pdf) {
                        Text("Any").tag(SmartFolder.PDF.any)
                        Text("Has a PDF").tag(SmartFolder.PDF.has)
                        Text("No PDF yet").tag(SmartFolder.PDF.missing)
                    }
                }
                Section {
                    Label("\(count) paper\(count == 1 ? "" : "s") match right now", systemImage: "line.3.horizontal.decrease.circle")
                        .foregroundStyle(.secondary)
                }
            }
            .formStyle(.grouped)
            .navigationTitle(isNew ? "New Smart Folder" : "Edit Smart Folder")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Save") {
                        var f = folder
                        if f.name.trimmingCharacters(in: .whitespaces).isEmpty { f.name = f.summary.capitalizedFirst }
                        model.saveSmartFolder(f)
                        if isNew { model.source = .smart(f.id) }
                        dismiss()
                    }
                }
            }
        }
        #if os(macOS)
        .frame(minWidth: 460, minHeight: 560)
        #endif
    }
}
