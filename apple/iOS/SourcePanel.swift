import SwiftUI
import MarginsCore

/// The iPhone's side panel: what the library shows (all, starred, unread, folders, smart folders,
/// tags, Discover, New for You), sorting, and the library tools. Opens by swiping in from the
/// left edge of the library, or with the button at the top left.
struct SourcePanel: View {
    @EnvironmentObject var model: LibraryModel
    @ObservedObject private var center = WatchCenter.shared
    let close: () -> Void
    @State private var managing = false
    @State private var cleaning = false
    @State private var newSmart = false
    @State private var editingSmart: SmartFolder?
    @State private var prompt: NameRequest?

    var body: some View {
        List {
            Section {
                row(.all, "All Papers", "books.vertical", count: model.counts.all)
                row(.starred, "Starred", "star", count: model.counts.starred)
                row(.unread, "Unread", "smallcircle.filled.circle", count: model.counts.unread)
            }
            Section {
                row(.discover, "Discover New Papers", "globe.europe.africa")
                row(.alerts, "New for You", "bell.badge", count: center.newCount, highlight: true)
            }
            Section("Folders") {
                ForEach(model.foldersFlat, id: \.folder.id) { item in
                    row(.collection(item.folder.id), item.folder.name, item.depth == 0 ? "folder" : "folder.fill",
                        indent: item.depth)
                }
                Button {
                    prompt = NameRequest(title: "New Folder", action: "Create") { _ = model.newFolder($0) }
                } label: { Label("New Folder", systemImage: "folder.badge.plus").foregroundStyle(.secondary) }
            }
            Section("Smart Folders") {
                ForEach(model.smartFolders) { f in
                    row(.smart(f.id), f.name, "gearshape.2")
                        .contextMenu {
                            Button("Edit…", systemImage: "slider.horizontal.3") { editingSmart = f }
                            Button("Delete", systemImage: "trash", role: .destructive) { model.deleteSmartFolder(f.id) }
                        }
                }
                Button { newSmart = true } label: {
                    Label("New Smart Folder", systemImage: "plus").foregroundStyle(.secondary)
                }
            }
            if !model.tags.isEmpty {
                Section("Tags") {
                    ForEach(model.tags, id: \.self) { t in row(.tag(t), t, "tag") }
                }
            }
            Section {
                Picker(selection: $model.sort) {
                    Text("Recently added").tag(SortKey.added)
                    Text("Year").tag(SortKey.year)
                    Text("Title").tag(SortKey.title)
                    Text("First author").tag(SortKey.firstAuthor)
                } label: { Label("Sort", systemImage: "arrow.up.arrow.down") }
                .pickerStyle(.menu)
                Button { managing = true } label: { Label("Manage Folders & Tags…", systemImage: "folder.badge.gearshape") }
                Button { cleaning = true } label: { Label("Clean Up Library…", systemImage: "wand.and.sparkles") }
            }
        }
        .listStyle(.insetGrouped)
        .tint(Color.marginsTeal)
        .safeAreaInset(edge: .top, spacing: 0) {
            HStack {
                Text("Margins").font(.title2.weight(.bold))
                Spacer()
                Button { close() } label: { Image(systemName: "xmark.circle.fill").font(.title2).symbolRenderingMode(.hierarchical) }
                    .foregroundStyle(.secondary)
            }
            .padding(.horizontal, 20).padding(.top, 8).padding(.bottom, 4)
            .background(Color(uiColor: .systemGroupedBackground))
        }
        .namePrompt($prompt)
        .sheet(isPresented: $managing) { ManageFoldersView().environmentObject(model) }
        .sheet(isPresented: $newSmart) { SmartFolderEditor(folder: SmartFolder(), isNew: true).environmentObject(model) }
        .sheet(item: $editingSmart) { f in SmartFolderEditor(folder: f).environmentObject(model) }
        .sheet(isPresented: $cleaning) {
            NavigationStack {
                CleanupView().toolbar { ToolbarItem(placement: .confirmationAction) { Button("Done") { cleaning = false } } }
            }
            .environmentObject(model)
        }
    }

    private func row(_ item: SidebarItem, _ title: String, _ icon: String, count: Int? = nil,
                     indent: Int = 0, highlight: Bool = false) -> some View {
        let selected = model.source == item
        return Button {
            model.source = item
            close()
        } label: {
            HStack {
                Label(title, systemImage: icon)
                    .padding(.leading, CGFloat(indent) * 16)
                    .foregroundStyle(selected ? Color.marginsTeal : .primary)
                    .fontWeight(selected ? .semibold : .regular)
                Spacer()
                if let count, count > 0 {
                    Text("\(count)")
                        .font(.footnote.monospacedDigit())
                        .foregroundStyle(highlight ? .white : .secondary)
                        .padding(.horizontal, highlight ? 7 : 0).padding(.vertical, highlight ? 2 : 0)
                        .background { if highlight { Capsule().fill(Color.marginsTeal) } }
                }
            }
        }
        .listRowBackground(selected ? Color.marginsTeal.opacity(0.12) : nil)
    }
}

/// Wraps the library in a side panel that slides in from the left: swipe in from the left edge
/// (only on the library itself, not inside a paper, where that swipe goes back) or call `open`.
struct SidePanelHost<Content: View>: View {
    @Binding var shown: Bool
    var edgeEnabled = true
    @ViewBuilder let content: Content
    let panel: () -> SourcePanel
    @State private var drag: CGFloat = 0

    var body: some View {
        GeometryReader { geo in
            let w = min(340, geo.size.width * 0.84)
            let revealed = shown ? max(0, min(w, w + drag)) : max(0, min(w, drag))
            ZStack(alignment: .leading) {
                content
                if edgeEnabled && !shown {
                    // the catch area for the edge swipe, below the navigation bar
                    Color.clear
                        .frame(width: 18)
                        .contentShape(Rectangle())
                        .padding(.top, 120)
                        .gesture(DragGesture(minimumDistance: 6, coordinateSpace: .global)
                            .onChanged { drag = max(0, $0.translation.width) }
                            .onEnded { g in finish(open: g.translation.width > w * 0.3 || g.predictedEndTranslation.width > w * 0.6) })
                }
                if revealed > 0 {
                    Color.black.opacity(0.3 * Double(revealed / w))
                        .ignoresSafeArea()
                        .onTapGesture { finish(open: false) }
                        .gesture(closeDrag(w))
                    panel()
                        .frame(width: w)
                        .background(Color(uiColor: .systemGroupedBackground))
                        .clipShape(UnevenRoundedRectangle(bottomTrailingRadius: 22, topTrailingRadius: 22))
                        .shadow(color: .black.opacity(0.18), radius: 16, x: 4)
                        .offset(x: revealed - w)
                        .simultaneousGesture(closeDrag(w))   // the list still scrolls
                        .ignoresSafeArea(edges: .bottom)
                }
            }
        }
    }

    private func closeDrag(_ w: CGFloat) -> some Gesture {
        DragGesture(minimumDistance: 12, coordinateSpace: .global)
            .onChanged { g in
                // only sideways drags close it; up and down scroll the list
                guard abs(g.translation.width) > abs(g.translation.height) else { return }
                drag = min(0, g.translation.width)
            }
            .onEnded { g in
                let sideways = abs(g.translation.width) > abs(g.translation.height)
                finish(open: !(sideways && (g.translation.width < -w * 0.3 || g.predictedEndTranslation.width < -w * 0.6)))
            }
    }

    private func finish(open: Bool) {
        withAnimation(.spring(response: 0.32, dampingFraction: 0.9)) {
            shown = open
            drag = 0
        }
    }
}
