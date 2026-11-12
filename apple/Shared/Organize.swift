import SwiftUI
import LuntaCore

// Folders (nested collections) and tags: model helpers plus the views both apps share.

/// A folder and its subfolders, for outline lists.
struct FolderNode: Identifiable, Hashable {
    let folder: PaperCollection
    var children: [FolderNode]?
    var id: String { folder.id }
}

extension LibraryModel {
    /// Folders directly inside `parent` (nil = top level). Folders whose parent is gone count as top level.
    func folders(in parent: String?) -> [PaperCollection] {
        let ids = Set(collections.map(\.id))
        return collections.filter { c in
            guard let p = c.parent, ids.contains(p) else { return parent == nil }
            return p == parent
        }
    }

    var folderTree: [FolderNode] {
        func nodes(_ parent: String?) -> [FolderNode] {
            folders(in: parent).map { c in
                let kids = nodes(c.id)
                return FolderNode(folder: c, children: kids.isEmpty ? nil : kids)
            }
        }
        return nodes(nil)
    }

    /// Every folder in outline order with its depth, for flat menus and pickers.
    var foldersFlat: [(folder: PaperCollection, depth: Int)] {
        var out: [(PaperCollection, Int)] = []
        func walk(_ parent: String?, _ d: Int) {
            for c in folders(in: parent) { out.append((c, d)); walk(c.id, d + 1) }
        }
        walk(nil, 0)
        return out
    }

    /// "Thesis › Chapter 1"
    func folderPath(_ id: String) -> String {
        var names: [String] = [], cur: String? = id, seen = Set<String>()
        while let c = cur, !seen.contains(c), let f = collections.first(where: { $0.id == c }) {
            names.insert(f.name, at: 0); seen.insert(c); cur = f.parent
        }
        return names.joined(separator: " › ")
    }

    /// The folder and everything inside it.
    func folderAndDescendants(_ id: String) -> [String] {
        [id] + folders(in: id).flatMap { folderAndDescendants($0.id) }
    }

    @discardableResult
    func newFolder(_ name: String, parent: String? = nil, with ids: [String] = []) -> String? {
        let n = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !n.isEmpty else { return nil }
        do {
            let c = try lib.createCollection(name: n, parent: parent)
            for id in ids { try lib.setMembership(collection: c.id, paper: id, present: true) }
            reload()
            return c.id
        } catch { errorMessage = error.localizedDescription; return nil }
    }

    func renameFolder(_ id: String, _ name: String) {
        let n = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !n.isEmpty else { return }
        do { try lib.renameCollection(id: id, name: n) } catch { errorMessage = error.localizedDescription }
        reload()
    }

    func moveFolder(_ id: String, into parent: String?) {
        do { try lib.moveCollection(id: id, parent: parent) } catch { errorMessage = error.localizedDescription }
        reload()
    }

    /// Deletes the folder and its subfolders. Papers stay in the library.
    func deleteFolder(_ id: String) {
        for f in folderAndDescendants(id).reversed() { try? lib.deleteCollection(id: f) }
        if case .collection(let s) = source, !collections.contains(where: { $0.id == s }) || folderAndDescendants(id).contains(s) {
            source = .all
        }
        reload()
    }

    func setTag(_ ids: [String], _ tag: String, _ on: Bool) {
        let t = tag.trimmingCharacters(in: .whitespacesAndNewlines).trimmingCharacters(in: CharacterSet(charactersIn: "#"))
        guard !t.isEmpty else { return }
        for id in ids { try? lib.setTag(id: id, tag: t, present: on) }
        reload()
    }

    private func idsTagged(_ tag: String) -> [String] {
        lib.papers(filter: PaperFilter(collection: nil, tag: tag, starredOnly: false, unreadOnly: false), sort: .added).map(\.id)
    }

    func renameTag(_ old: String, _ new: String) {
        let n = new.trimmingCharacters(in: .whitespacesAndNewlines).trimmingCharacters(in: CharacterSet(charactersIn: "#"))
        guard !n.isEmpty, n != old else { return }
        for id in idsTagged(old) {
            try? lib.setTag(id: id, tag: n, present: true)
            try? lib.setTag(id: id, tag: old, present: false)
        }
        if source == .tag(old) { source = .tag(n) }
        reload()
    }

    func deleteTag(_ tag: String) {
        for id in idsTagged(tag) { try? lib.setTag(id: id, tag: tag, present: false) }
        if source == .tag(tag) { source = .all }
        reload()
    }

    /// Papers dragged onto a folder or tag arrive as their ids; folders as "folder:<id>".
    static let folderDragPrefix = "folder:"
}

// MARK: - Asking for a name

struct NameRequest: Identifiable {
    let id = UUID()
    let title: String
    var message: String? = nil
    var initial = ""
    var action: String = "Save"
    let done: (String) -> Void
}

private struct NamePromptModifier: ViewModifier {
    @Binding var request: NameRequest?
    @State private var text = ""

    func body(content: Content) -> some View {
        content
            .alert(request?.title ?? "", isPresented: Binding(get: { request != nil }, set: { if !$0 { request = nil } }), presenting: request) { r in
                TextField("Name", text: $text)
                Button(r.action) { r.done(text); request = nil }
                Button("Cancel", role: .cancel) { request = nil }
            } message: { r in
                if let m = r.message { Text(m) }
            }
            .onChange(of: request?.id) { text = request?.initial ?? "" }
    }
}

extension View {
    func namePrompt(_ request: Binding<NameRequest?>) -> some View { modifier(NamePromptModifier(request: request)) }
}

// MARK: - Menus (context menus on papers)

/// "Add to Folder" with subfolders as submenus.
struct FolderMenu: View {
    @EnvironmentObject var model: LibraryModel
    let ids: [String]
    var newFolder: (() -> Void)?

    var body: some View {
        Menu("Add to Folder") {
            ForEach(model.folderTree) { FolderMenuItem(node: $0, ids: ids) }
            if let newFolder {
                if !model.collections.isEmpty { Divider() }
                Button("New Folder…", action: newFolder)
            }
        }
    }
}

private struct FolderMenuItem: View {
    @EnvironmentObject var model: LibraryModel
    let node: FolderNode
    let ids: [String]

    var body: some View {
        if let kids = node.children {
            Menu(node.folder.name) {
                Button("Add to “\(node.folder.name)”") { model.setMembership(ids, collection: node.id, true) }
                Divider()
                ForEach(kids) { FolderMenuItem(node: $0, ids: ids) }
            }
        } else {
            Button(node.folder.name) { model.setMembership(ids, collection: node.id, true) }
        }
    }
}

/// Toggle tags on the selected papers.
struct TagMenu: View {
    @EnvironmentObject var model: LibraryModel
    let ids: [String]
    var newTag: (() -> Void)?

    var body: some View {
        Menu("Tags") {
            ForEach(model.tags, id: \.self) { t in
                let on = ids.allSatisfy { model.paper($0)?.tags.contains(t) == true }
                Toggle(t, isOn: Binding(get: { on }, set: { model.setTag(ids, t, $0) }))
            }
            if let newTag {
                if !model.tags.isEmpty { Divider() }
                Button("New Tag…", action: newTag)
            }
        }
    }
}

// MARK: - Organize sheet: folders and tags for one or more papers

struct OrganizeView: View {
    @EnvironmentObject var model: LibraryModel
    @Environment(\.dismiss) private var dismiss
    let ids: [String]
    @State private var newTag = ""
    @State private var prompt: NameRequest?

    private var papers: [Paper] { ids.compactMap { model.paper($0) } }

    var body: some View {
        NavigationStack {
            Form {
                if papers.count == 1, let p = papers.first {
                    Section { Text(p.meta.title.isEmpty ? "Untitled" : p.meta.title).font(.headline).lineLimit(3) }
                } else {
                    Section { Text("\(papers.count) papers").font(.headline) }
                }
                Section {
                    if model.collections.isEmpty {
                        Text("No folders yet.").foregroundStyle(.secondary)
                    }
                    ForEach(model.foldersFlat, id: \.folder.id) { item in
                        folderRow(item.folder, depth: item.depth)
                    }
                    Button { prompt = NameRequest(title: "New Folder", action: "Create") { model.newFolder($0, with: ids) } } label: {
                        Label("New Folder", systemImage: "folder.badge.plus")
                    }
                } header: { Text("Folders") }
                Section {
                    let all = Set(model.tags)
                    if !all.isEmpty {
                        TagChips(tags: model.tags) { t in
                            ids.allSatisfy { model.paper($0)?.tags.contains(t) == true }
                        } toggle: { t, on in model.setTag(ids, t, on) }
                    }
                    HStack {
                        Image(systemName: "tag").foregroundStyle(.secondary)
                        TextField("New tag", text: $newTag)
                            #if os(iOS)
                            .textInputAutocapitalization(.never)
                            #endif
                            .autocorrectionDisabled()
                            .onSubmit(addTag)
                        Button("Add", action: addTag).disabled(newTag.trimmingCharacters(in: .whitespaces).isEmpty)
                    }
                } header: { Text("Tags") }
            }
            .formStyle(.grouped)
            .navigationTitle("Folders & Tags")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar { ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } } }
            .namePrompt($prompt)
        }
        #if os(macOS)
        .frame(minWidth: 420, minHeight: 480)
        #endif
    }

    private func folderRow(_ f: PaperCollection, depth: Int) -> some View {
        let inAll = ids.allSatisfy { model.paper($0)?.collections.contains(f.id) == true }
        let inSome = !inAll && ids.contains { model.paper($0)?.collections.contains(f.id) == true }
        return Button { model.setMembership(ids, collection: f.id, !inAll) } label: {
            HStack {
                Image(systemName: depth > 0 ? "folder" : "folder.fill").foregroundStyle(Color.luntaTeal)
                Text(f.name).foregroundStyle(.primary)
                Spacer()
                if inAll { Image(systemName: "checkmark").foregroundStyle(Color.luntaTeal) }
                else if inSome { Image(systemName: "minus").foregroundStyle(.secondary) }
            }
            .padding(.leading, CGFloat(depth) * 18)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }

    private func addTag() {
        let t = newTag
        newTag = ""
        model.setTag(ids, t, true)
    }
}

/// Tappable tag chips that wrap onto new lines.
struct TagChips: View {
    let tags: [String]
    let isOn: (String) -> Bool
    let toggle: (String, Bool) -> Void

    var body: some View {
        WrapLayout(spacing: 6) {
            ForEach(tags, id: \.self) { t in
                let on = isOn(t)
                Button { toggle(t, !on) } label: {
                    Label(t, systemImage: on ? "checkmark" : "tag")
                        .font(.callout)
                        .padding(.horizontal, 10).padding(.vertical, 5)
                        .background(Capsule().fill(on ? Color.luntaTeal.opacity(0.2) : Color.secondary.opacity(0.12)))
                        .foregroundStyle(on ? Color.luntaTeal : .primary)
                }
                .buttonStyle(.plain)
            }
        }
        .padding(.vertical, 4)
    }
}

/// Lays children out left to right, wrapping like text.
struct WrapLayout: Layout {
    var spacing: CGFloat = 6

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let rows = arrange(proposal.width ?? .infinity, subviews)
        return CGSize(width: proposal.width ?? rows.map(\.width).max() ?? 0, height: rows.last.map { $0.y + $0.height } ?? 0)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        for row in arrange(bounds.width, subviews) {
            var x = bounds.minX
            for i in row.items {
                let s = subviews[i].sizeThatFits(.unspecified)
                subviews[i].place(at: CGPoint(x: x, y: bounds.minY + row.y), proposal: ProposedViewSize(s))
                x += s.width + spacing
            }
        }
    }

    private struct Row { var items: [Int] = []; var y: CGFloat = 0; var width: CGFloat = 0; var height: CGFloat = 0 }

    private func arrange(_ maxWidth: CGFloat, _ subviews: Subviews) -> [Row] {
        var rows: [Row] = [Row()]
        for (i, v) in subviews.enumerated() {
            let s = v.sizeThatFits(.unspecified)
            if !rows[rows.count - 1].items.isEmpty, rows[rows.count - 1].width + spacing + s.width > maxWidth {
                let last = rows[rows.count - 1]
                rows.append(Row(y: last.y + last.height + spacing))
            }
            var r = rows[rows.count - 1]
            r.width += (r.items.isEmpty ? 0 : spacing) + s.width
            r.height = max(r.height, s.height)
            r.items.append(i)
            rows[rows.count - 1] = r
        }
        return rows
    }
}

// MARK: - Managing folders and tags (iPhone; the Mac does this in the sidebar)

struct ManageFoldersView: View {
    @EnvironmentObject var model: LibraryModel
    @Environment(\.dismiss) private var dismiss
    @State private var prompt: NameRequest?
    @State private var moving: PaperCollection?
    @State private var deleting: PaperCollection?

    var body: some View {
        NavigationStack {
            List {
                Section {
                    ForEach(model.foldersFlat, id: \.folder.id) { item in
                        let f = item.folder
                        HStack {
                            Image(systemName: "folder").foregroundStyle(Color.luntaTeal)
                            Text(f.name)
                            Spacer()
                            Text("\(f.paperCount)").foregroundStyle(.secondary).monospacedDigit()
                            Menu {
                                Button("Show Papers", systemImage: "doc.text") { model.source = .collection(f.id); dismiss() }
                                Button("New Subfolder…", systemImage: "folder.badge.plus") {
                                    prompt = NameRequest(title: "New Folder inside “\(f.name)”", action: "Create") { model.newFolder($0, parent: f.id) }
                                }
                                Button("Rename…", systemImage: "pencil") {
                                    prompt = NameRequest(title: "Rename Folder", initial: f.name) { model.renameFolder(f.id, $0) }
                                }
                                Button("Move…", systemImage: "arrow.turn.down.right") { moving = f }
                                Button("Delete", systemImage: "trash", role: .destructive) { deleting = f }
                            } label: { Image(systemName: "ellipsis.circle").foregroundStyle(.secondary) }
                        }
                        .padding(.leading, CGFloat(item.depth) * 20)
                    }
                    Button { prompt = NameRequest(title: "New Folder", action: "Create") { model.newFolder($0) } } label: {
                        Label("New Folder", systemImage: "folder.badge.plus")
                    }
                } header: { Text("Folders") }
                Section {
                    if model.tags.isEmpty { Text("No tags yet. Add them from a paper.").foregroundStyle(.secondary) }
                    ForEach(model.tags, id: \.self) { t in
                        HStack {
                            Image(systemName: "tag").foregroundStyle(.secondary)
                            Text(t)
                            Spacer()
                            Menu {
                                Button("Show Papers", systemImage: "doc.text") { model.source = .tag(t); dismiss() }
                                Button("Rename…", systemImage: "pencil") {
                                    prompt = NameRequest(title: "Rename Tag", initial: t) { model.renameTag(t, $0) }
                                }
                                Button("Remove from All Papers", systemImage: "trash", role: .destructive) { model.deleteTag(t) }
                            } label: { Image(systemName: "ellipsis.circle").foregroundStyle(.secondary) }
                        }
                    }
                } header: { Text("Tags") }
            }
            .navigationTitle("Folders & Tags")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar { ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } } }
            .namePrompt($prompt)
            .confirmationDialog("Delete “\(deleting?.name ?? "")”?", isPresented: Binding(get: { deleting != nil }, set: { if !$0 { deleting = nil } }), titleVisibility: .visible) {
                Button("Delete Folder", role: .destructive) { if let f = deleting { model.deleteFolder(f.id) }; deleting = nil }
            } message: { Text("Its subfolders go too. The papers stay in your library.") }
            .sheet(item: $moving) { f in MoveFolderView(folder: f).environmentObject(model) }
        }
    }
}

/// Pick where a folder goes.
struct MoveFolderView: View {
    @EnvironmentObject var model: LibraryModel
    @Environment(\.dismiss) private var dismiss
    let folder: PaperCollection

    var body: some View {
        NavigationStack {
            List {
                Button { model.moveFolder(folder.id, into: nil); dismiss() } label: {
                    Label("Top Level", systemImage: "tray.full")
                }
                let blocked = Set(model.folderAndDescendants(folder.id))
                ForEach(model.foldersFlat.filter { !blocked.contains($0.folder.id) }, id: \.folder.id) { item in
                    Button { model.moveFolder(folder.id, into: item.folder.id); dismiss() } label: {
                        Label(item.folder.name, systemImage: "folder").padding(.leading, CGFloat(item.depth) * 20)
                    }
                }
            }
            .navigationTitle("Move “\(folder.name)”")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar { ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } } }
        }
    }
}

/// The folders a paper is in, with a button to change them.
struct PaperFolders: View {
    @EnvironmentObject var model: LibraryModel
    let paper: Paper
    @State private var organizing = false

    var body: some View {
        let ids = Set(paper.collections)
        let mine = model.foldersFlat.map(\.folder).filter { ids.contains($0.id) }
        WrapLayout(spacing: 6) {
            ForEach(mine, id: \.id) { f in
                Button { model.source = .collection(f.id) } label: {
                    Label(model.folderPath(f.id), systemImage: "folder")
                        .font(.callout)
                        .padding(.horizontal, 9).padding(.vertical, 4)
                        .background(Capsule().fill(Color.luntaTeal.opacity(0.14)))
                        .foregroundStyle(Color.luntaTeal)
                }
                .buttonStyle(.plain)
                .help("Show this folder")
            }
            Button { organizing = true } label: {
                #if os(iOS)
                Label("Folders & Tags…", systemImage: "folder.badge.plus").font(.callout)
                #else
                Label(mine.isEmpty ? "Add to Folder…" : "Change…", systemImage: "folder.badge.plus").font(.callout)
                #endif
            }
            .buttonStyle(.borderless)
        }
        .sheet(isPresented: $organizing) { OrganizeView(ids: [paper.id]).environmentObject(model) }
    }
}
