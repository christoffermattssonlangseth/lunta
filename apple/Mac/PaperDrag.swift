import SwiftUI
import AppKit

/// Dragging papers out of the paper table, done by AppKit.
///
/// SwiftUI's `TableRow.itemProvider` / `.draggable` start no drag at all in the library window on
/// macOS 26, and neither does the table's own NSTableView dragging (it asks for the rows' pasteboard
/// writers but never lifts them). So the table gets a subclass, made at run time, whose `mouseDown:`
/// starts the drag itself once a plain press on a row moves a few points; anything else (a click,
/// ⌘/⇧-click, double-click) goes to the table untouched. The dragged text is the paper ids, one per
/// line: the whole selection when the pressed row is selected, else that one paper. The sidebar's
/// drop targets (`SidebarDrop`) read it.
struct PaperDragSource: NSViewRepresentable {
    /// The paper id for a table row.
    let paperID: (Int) -> String?

    func makeNSView(context: Context) -> Anchor {
        let v = Anchor()
        v.paperID = paperID
        return v
    }
    func updateNSView(_ v: Anchor, context: Context) { v.paperID = paperID }

    /// Sits behind the table; finds the table under the first mouse-down and gives it the subclass.
    final class Anchor: NSView, NSDraggingSource {
        var paperID: (Int) -> String? = { _ in nil }
        private var monitor: Any?
        private var dragged: [String] = []
        /// A plain press on a row that may turn into a drag.
        private var pending: (table: NSTableView, row: Int, down: NSEvent)?
        /// A drag is under way: the table's (late) mouse-down must not start tracking.
        private(set) var dragging = false
        /// The press that started the last drag, so its late delivery to the table is ignored.
        private var dragPress: TimeInterval = -1

        override func hitTest(_ point: NSPoint) -> NSView? { nil }

        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            if let monitor { NSEvent.removeMonitor(monitor); self.monitor = nil }
            guard window != nil else { return }
            // the drag starts here, in the app's event stream, not in the table's mouseDown: SwiftUI's
            // gesture recognizers on the table (double-click to open) hold the press back from the
            // table until they give up, which made every drag start late
            monitor = NSEvent.addLocalMonitorForEvents(matching: [.leftMouseDown, .leftMouseDragged, .leftMouseUp]) { [weak self] e in
                guard let self else { return e }
                switch e.type {
                case .leftMouseDown: self.press(e)
                case .leftMouseDragged: if self.drag(e) { return nil }
                default: self.pending = nil
                }
                return e
            }
        }

        deinit { if let monitor { NSEvent.removeMonitor(monitor) } }

        private func press(_ e: NSEvent) {
            pending = nil
            guard let win = window, e.window === win else { return }
            guard bounds.contains(convert(e.locationInWindow, from: nil)), let content = win.contentView,
                  let hit = content.hitTest(content.convert(e.locationInWindow, from: nil)) else { return }
            var v: NSView? = hit
            while let cur = v, !(cur is NSTableView) { v = cur.superview }
            guard let table = v as? NSTableView else { return }
            objc_setAssociatedObject(table, &anchorKey, self, .OBJC_ASSOCIATION_ASSIGN)
            PaperDragSource.install(on: table)
            guard e.clickCount == 1,
                  e.modifierFlags.intersection([.command, .shift, .control, .option]).isEmpty else { return }
            let row = table.row(at: table.convert(e.locationInWindow, from: nil))
            if row >= 0, paperID(row) != nil { pending = (table, row, e) }
        }

        /// A dragged event seen before the table tracks the press: starts the drag once it has moved.
        private func drag(_ e: NSEvent) -> Bool {
            guard let p = pending else { return false }
            let a = p.down.locationInWindow, b = e.locationInWindow
            guard hypot(b.x - a.x, b.y - a.y) >= 4 else { return false }
            pending = nil
            beginDrag(p.table, row: p.row, event: e, press: p.down)
            return true
        }

        /// Called from the table's `mouseDown:`; returns false to let the table handle the press.
        func handle(_ table: NSTableView, _ down: NSEvent) -> Bool {
            if dragging || down.timestamp == dragPress { return true }
            pending = nil
            guard down.clickCount == 1,
                  down.modifierFlags.intersection([.command, .shift, .control, .option]).isEmpty else { return false }
            let row = table.row(at: table.convert(down.locationInWindow, from: nil))
            guard row >= 0, paperID(row) != nil, let win = table.window else { return false }

            // wait: a drag (moved a few points) or a click (released first)
            let start = down.locationInWindow
            while let e = win.nextEvent(matching: [.leftMouseDragged, .leftMouseUp]) {
                if e.type == .leftMouseUp {
                    // a click: hand the press and the release to the table
                    NSApp.postEvent(e, atStart: true)
                    return false
                }
                let p = e.locationInWindow
                if hypot(p.x - start.x, p.y - start.y) >= 4 {
                    beginDrag(table, row: row, event: e, press: down)
                    return true
                }
            }
            return false
        }

        /// `event` is the mouse-dragged event that crossed the threshold: starting the session from the
        /// press instead makes AppKit wait most of a second before the drag image appears.
        private func beginDrag(_ table: NSTableView, row: Int, event: NSEvent, press: NSEvent) {
            // the whole selection when the pressed row is part of it, else just that row. The selection
            // is left alone (as AppKit tables do): changing it would redraw the detail pane before the
            // drag could start.
            let rows = table.selectedRowIndexes.contains(row) ? table.selectedRowIndexes : IndexSet(integer: row)
            let ids = rows.compactMap { paperID($0) }
            guard !ids.isEmpty else { return }

            dragged = ids
            let item = NSDraggingItem(pasteboardWriter: ids.joined(separator: "\n") as NSString)
            let rect = table.rect(ofRow: row)
            item.setDraggingFrame(rect, contents: snapshot(table, row, rect, count: ids.count))
            dragging = true
            dragPress = press.timestamp
            let session = table.beginDraggingSession(with: [item], event: event, source: self)
            session.animatesToStartingPositionsOnCancelOrFail = false
        }


        /// The pressed row as an image, with a count badge when several papers are dragged.
        private func snapshot(_ table: NSTableView, _ row: Int, _ rect: NSRect, count: Int) -> NSImage {
            let image = NSImage(size: rect.size)
            // just the row's view, not the table (which would lay out and draw far more)
            let source: NSView = table.rowView(atRow: row, makeIfNecessary: false) ?? table
            let area = source === table ? rect : source.bounds
            if let rep = source.bitmapImageRepForCachingDisplay(in: area) {
                source.cacheDisplay(in: area, to: rep)
                image.addRepresentation(rep)
            }
            guard count > 1 else { return image }
            let out = NSImage(size: rect.size, flipped: false) { r in
                image.draw(in: r, from: .zero, operation: .sourceOver, fraction: 0.85)
                let text = "\(count)" as NSString
                let attrs: [NSAttributedString.Key: Any] = [.font: NSFont.boldSystemFont(ofSize: 12), .foregroundColor: NSColor.white]
                let size = text.size(withAttributes: attrs)
                let badge = NSRect(x: 8, y: r.midY - 10, width: max(20, size.width + 12), height: 20)
                NSColor.systemRed.setFill()
                NSBezierPath(roundedRect: badge, xRadius: 10, yRadius: 10).fill()
                text.draw(at: NSPoint(x: badge.midX - size.width / 2, y: badge.midY - size.height / 2), withAttributes: attrs)
                return true
            }
            return out
        }

        func draggingSession(_ session: NSDraggingSession, movedTo p: NSPoint) {
            SidebarDrop.hover(p)
        }

        func draggingSession(_ session: NSDraggingSession, endedAt p: NSPoint, operation: NSDragOperation) {
            let ids = dragged
            dragged = []
            dragging = false
            // the sidebar resolves its own drops (see SidebarDrop); anything else was handled elsewhere
            SidebarDrop.hover(nil)
            guard operation.isEmpty, !ids.isEmpty else { return }
            MainActor.assumeIsolated { _ = SidebarDrop.drop([ids.joined(separator: "\n")], at: p) }
        }

        func draggingSession(_ session: NSDraggingSession, sourceOperationMaskFor context: NSDraggingContext) -> NSDragOperation {
            context == .withinApplication ? [.copy, .generic] : []
        }
    }

    /// Gives the table a subclass (made at run time) whose `mouseDown:` asks the anchor first.
    static func install(on table: NSTableView) {
        let base: AnyClass = object_getClass(table)!
        if NSStringFromClass(base).hasPrefix("LuntaDrag_") { return }
        let name = "LuntaDrag_" + NSStringFromClass(base)
        var sub: AnyClass? = NSClassFromString(name)
        if sub == nil, let made = objc_allocateClassPair(base, name, 0) {
            typealias DownIMP = @convention(c) (AnyObject, Selector, NSEvent) -> Void
            let sel = #selector(NSResponder.mouseDown(with:))
            let sup = unsafeBitCast(class_getMethodImplementation(base, sel), to: DownIMP.self)
            let down: @convention(block) (NSTableView, NSEvent) -> Void = { table, e in
                let anchor = objc_getAssociatedObject(table, &anchorKey) as? Anchor
                if anchor?.handle(table, e) == true { return }
                sup(table, sel, e)
            }
            class_addMethod(made, sel, imp_implementationWithBlock(down), "v@:@")
            objc_registerClassPair(made)
            sub = made
        }
        if let sub { object_setClass(table, sub) }
    }
}

private var anchorKey: UInt8 = 0
