import SwiftUI
import PDFKit
import WebKit

// Go to Page. The reading view shows its own panel (a page number and a slider: luntaShowGoTo in
// JumpBack.swift, shared with Android and desktop; also opened by clicking the page number and
// by ⌥⌘G inside the page). The PDF asks for a page number here.

struct GoToPageModifier: ViewModifier {
    /// Changed (incremented) to ask.
    let request: Int
    let pdf: Bool
    let webView: () -> WKWebView?
    let pdfView: () -> PDFView?
    @State private var asking = false
    @State private var text = ""
    @State private var count = 0

    func body(content: Content) -> some View {
        content
            .onChange(of: request) { open() }
            #if os(iOS)
            // ⌥⌘G from a hardware keyboard (iPad)
            .background {
                Button("Go to Page") { open() }.keyboardShortcut("g", modifiers: [.command, .option])
                    .opacity(0).allowsHitTesting(false).accessibilityHidden(true)
            }
            #endif
            .alert("Go to Page", isPresented: $asking) {
                TextField("Page", text: $text)
                    #if os(iOS)
                    .keyboardType(.numberPad)
                    #endif
                Button("Go") { go() }
                Button("Cancel", role: .cancel) {}
            } message: { Text("Page 1–\(count)") }
    }

    private func open() {
        if pdf {
            guard let v = pdfView(), let d = v.document, d.pageCount > 0 else { return }
            count = d.pageCount
            text = String((v.currentPage.map { d.index(for: $0) } ?? 0) + 1)
            asking = true
        } else {
            webView()?.evaluateJavaScript("window.luntaShowGoTo && luntaShowGoTo()")
        }
    }

    private func go() {
        guard let v = pdfView(), let d = v.document, let n = Int(text.trimmingCharacters(in: .whitespaces)) else { return }
        if let p = d.page(at: max(0, min(d.pageCount - 1, n - 1))) { v.go(to: p) }
    }
}

/// The reader's menu item.
struct GoToPageButton: View {
    @Binding var request: Int
    var body: some View {
        Button { request += 1 } label: { Label("Go to Page…", systemImage: "arrow.right.to.line") }
    }
}

/// For the Mac's View menu: Go to Page… in the focused reader window.
struct GoToPageAction { let run: () -> Void }
struct GoToPageKey: FocusedValueKey { typealias Value = GoToPageAction }
extension FocusedValues {
    var goToPage: GoToPageAction? {
        get { self[GoToPageKey.self] }
        set { self[GoToPageKey.self] = newValue }
    }
}

#if os(macOS)
struct GoToPageCommands: Commands {
    @FocusedValue(\.goToPage) private var goTo
    var body: some Commands {
        CommandGroup(after: .toolbar) {
            Button("Go to Page…") { goTo?.run() }
                .keyboardShortcut(goTo != nil ? KeyboardShortcut("g", modifiers: [.command, .option]) : nil)
                .disabled(goTo == nil)
        }
    }
}
#endif
