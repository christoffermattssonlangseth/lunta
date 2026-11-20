import SwiftUI
import PDFKit
import PencilKit
import Combine

// Pencil drawings on the PDF's pages (iPad: drawn with the Apple Pencil in Markup mode; the Mac
// shows them, read-only). They are never written into the PDF: each page's drawing is its own
// small file in the paper's folder, papers/<id>/ink-page-<n>.drawing (n counted from 1; the
// PencilKit data format), so two devices drawing on different pages never touch the same file,
// and iCloud (and the Drive bridge, which syncs every papers/<id>/<file>) carries them like the
// paper's other files. Strokes are kept in page space: points of the page as shown (cropped,
// rotated), origin top left, so they land in the same place at any zoom and on any screen.

#if os(macOS)
typealias InkPlatformView = NSView
#else
typealias InkPlatformView = UIView
#endif

/// Reading and writing the drawing files.
enum InkFiles {
    static func name(page: Int) -> String { "ink-page-\(page + 1).drawing" }

    /// The page index of a drawing file, or of its iCloud placeholder (".ink-page-3.drawing.icloud").
    static func page(of fileName: String) -> (index: Int, placeholder: Bool)? {
        var n = fileName
        let placeholder = n.hasPrefix(".") && n.hasSuffix(".icloud")
        if placeholder { n = String(n.dropFirst().dropLast(".icloud".count)) }
        guard n.hasPrefix("ink-page-"), n.hasSuffix(".drawing"),
              let k = Int(n.dropFirst("ink-page-".count).dropLast(".drawing".count)), k >= 1 else { return nil }
        return (k - 1, placeholder)
    }

    static func modified(_ url: URL) -> Date? {
        var u = URL(fileURLWithPath: url.path)
        u.removeAllCachedResourceValues()
        return try? u.resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate
    }

    /// A coordinated read (iCloud may be writing a newer version at the same moment).
    nonisolated static func read(_ url: URL) -> PKDrawing? {
        var err: NSError?
        var out: PKDrawing?
        NSFileCoordinator().coordinate(readingItemAt: url, options: [], error: &err) { u in
            if let d = try? Data(contentsOf: u) { out = try? PKDrawing(data: d) }
        }
        return out
    }

    /// A coordinated write; an empty drawing removes the file.
    nonisolated static func write(_ drawing: PKDrawing, to url: URL) {
        var err: NSError?
        if drawing.strokes.isEmpty {
            guard FileManager.default.fileExists(atPath: url.path) else { return }
            NSFileCoordinator().coordinate(writingItemAt: url, options: .forDeleting, error: &err) { u in
                try? FileManager.default.removeItem(at: u)
            }
        } else {
            let data = drawing.dataRepresentation()
            NSFileCoordinator().coordinate(writingItemAt: url, options: .forReplacing, error: &err) { u in
                try? data.write(to: u, options: .atomic)
            }
        }
        if let err { NSLog("Lunta: couldn't save drawing %@: %@", url.lastPathComponent, err.localizedDescription) }
    }

    /// Two devices drew on the same page while apart: iCloud keeps one file and the other as a
    /// conflict version. Their strokes are merged into the file (a stroke both have counts once)
    /// and the conflict is marked resolved. True when the file changed.
    nonisolated static func resolveConflicts(_ url: URL) -> Bool {
        guard let others = NSFileVersion.unresolvedConflictVersionsOfItem(at: url), !others.isEmpty else { return false }
        var merged = read(url) ?? PKDrawing()
        func key(_ s: PKStroke) -> String {
            "\(s.path.creationDate.timeIntervalSinceReferenceDate)|\(s.path.count)|\(s.ink.inkType.rawValue)|\(Int(s.renderBounds.midX))|\(Int(s.renderBounds.midY))"
        }
        var have = Set(merged.strokes.map(key))
        for v in others {
            if let data = try? Data(contentsOf: v.url), let d = try? PKDrawing(data: data) {
                for st in d.strokes where !have.contains(key(st)) { merged.strokes.append(st); have.insert(key(st)) }
            }
        }
        write(merged, to: url)
        for v in others { v.isResolved = true }
        try? NSFileVersion.removeOtherVersionsOfItem(at: url)
        return true
    }

    /// The size of a page as shown (crop box, rotation applied): the space the strokes are kept in.
    static func pageSize(_ page: PDFPage) -> CGSize {
        let b = page.bounds(for: .cropBox).size
        return page.rotation % 180 == 0 ? b : CGSize(width: b.height, height: b.width)
    }
}

/// The drawings of the PDF shown: loads them, gives each page an overlay (a canvas to draw on, on
/// iPad; a picture of the drawing on the Mac), saves changes a moment after the last stroke and
/// picks up drawings changed on another device.
@MainActor
final class PDFInk: NSObject, ObservableObject, PDFPageOverlayViewProvider {
    /// Markup mode (iPad): the Pencil draws on the pages and the tool picker shows.
    @Published var markup = false { didSet { if markup != oldValue { applyMarkup() } } }

    /// The paper's folder (where the PDF is).
    private(set) var folder: URL?
    weak var pdfView: PDFView?
    /// Page index → its drawing in page space, as last loaded or drawn.
    private var drawings: [Int: PKDrawing] = [:]
    /// Page index → the modification date of its file as last read or written here.
    private var stamps: [Int: Date] = [:]
    private var pending: [Int: DispatchWorkItem] = [:]
    private var overlays: [Int: InkOverlay] = [:]
    private var poll: Timer?
    private nonisolated static let writer = DispatchQueue(label: "lunta.ink", qos: .utility)
    #if os(iOS)
    private let toolPicker = PKToolPicker()
    private var responder: InkResponder?
    private var background: NSObjectProtocol?
    #endif

    /// Called before the PDF view gets its document (overlays are asked for as pages appear).
    func attach(_ v: PDFView, folder: URL) {
        #if os(iOS)
        guard UIDevice.current.userInterfaceIdiom == .pad else { return }   // iPhone: no Pencil, PDF as before
        #endif
        if v !== pdfView {
            pdfView = v
            overlays = [:]
            #if os(iOS)
            toolPicker.colorUserInterfaceStyle = .light   // the pages are white
            responder?.removeFromSuperview()
            let r = InkResponder(frame: .zero)
            v.addSubview(r)
            responder = r
            #endif
        }
        v.pageOverlayViewProvider = self
        if folder != self.folder {
            flush()
            self.folder = folder
            drawings = [:]; stamps = [:]
            loadAll()
        }
        if poll == nil {
            poll = Timer.scheduledTimer(withTimeInterval: 3, repeats: true) { [weak self] _ in
                MainActor.assumeIsolated { self?.checkForChanges() }
            }
        }
        #if os(iOS)
        if background == nil {
            background = NotificationCenter.default.addObserver(forName: UIApplication.willResignActiveNotification, object: nil, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { self?.flush() }
            }
        }
        #endif
        #if DEBUG
        inkTest()
        #endif
    }

    /// The PDF view went away (back to the reading view, the reader closed).
    func detach() {
        markup = false
        flush()
        poll?.invalidate(); poll = nil
    }

    private func url(_ page: Int) -> URL? { folder?.appendingPathComponent(InkFiles.name(page: page)) }

    // MARK: loading and watching

    /// Every page's drawing that is on this device; the others are asked for from iCloud.
    private func loadAll() {
        guard let folder else { return }
        Task.detached(priority: .userInitiated) { [weak self] in
            let names = (try? FileManager.default.contentsOfDirectory(atPath: folder.path)) ?? []
            var found: [Int: (PKDrawing, Date?)] = [:]
            for n in names {
                guard let (i, placeholder) = InkFiles.page(of: n) else { continue }
                let u = folder.appendingPathComponent(InkFiles.name(page: i))
                if placeholder || !CloudLibrary.isDownloaded(u) { CloudLibrary.startDownload(u); continue }
                if CloudLibrary.isStale(u) { CloudLibrary.startDownload(u) }
                _ = InkFiles.resolveConflicts(u)
                if let d = InkFiles.read(u) { found[i] = (d, InkFiles.modified(u)) }
            }
            await self?.loaded(found, removed: [], from: folder)
        }
    }

    /// Drawings written by another device (iCloud, the Drive bridge): reloads pages whose file
    /// changed and that aren't being drawn on here.
    private func checkForChanges() {
        guard let folder else { return }
        let known = stamps
        let busy = Set(pending.keys)
        Task.detached(priority: .utility) { [weak self] in
            let names = (try? FileManager.default.contentsOfDirectory(atPath: folder.path)) ?? []
            var seen = Set<Int>()
            var changed: [Int: (PKDrawing, Date?)] = [:]
            for n in names {
                guard let (i, placeholder) = InkFiles.page(of: n) else { continue }
                seen.insert(i)
                let u = folder.appendingPathComponent(InkFiles.name(page: i))
                if placeholder || !CloudLibrary.isDownloaded(u) || CloudLibrary.isStale(u) { CloudLibrary.startDownload(u); continue }
                guard !busy.contains(i) else { continue }
                _ = InkFiles.resolveConflicts(u)
                guard let at = InkFiles.modified(u), at != known[i] else { continue }
                if let d = InkFiles.read(u) { changed[i] = (d, at) }
            }
            let removed = Set(known.keys).subtracting(seen).subtracting(busy)
            guard !changed.isEmpty || !removed.isEmpty else { return }
            await self?.loaded(changed, removed: removed, from: folder)
        }
    }

    /// Shows drawings read from the files (except on pages being drawn on here).
    private func loaded(_ found: [Int: (PKDrawing, Date?)], removed: Set<Int>, from folder: URL) {
        guard self.folder == folder else { return }
        for (i, (d, at)) in found where pending[i] == nil {
            drawings[i] = d; stamps[i] = at
            overlays[i]?.show(d)
        }
        for i in removed where pending[i] == nil {
            drawings[i] = nil; stamps[i] = nil
            overlays[i]?.show(PKDrawing())
        }
    }

    // MARK: saving

    /// A page's drawing changed here: saved once the strokes pause.
    fileprivate func changed(_ page: Int, _ d: PKDrawing) {
        drawings[page] = d
        pending[page]?.cancel()
        let work = DispatchWorkItem { [weak self] in self?.save(page) }
        pending[page] = work
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.8, execute: work)
    }

    private func save(_ page: Int) {
        pending[page] = nil
        guard let u = url(page) else { return }
        let d = drawings[page] ?? PKDrawing()
        Self.writer.async { [weak self] in
            InkFiles.write(d, to: u)
            let at = InkFiles.modified(u)
            DispatchQueue.main.async { self?.stamps[page] = at }
        }
    }

    /// Saves what's waiting now (leaving the PDF, the app going to the background).
    func flush() {
        for (i, w) in pending { w.cancel(); save(i) }
        Self.writer.sync {}
    }

    // MARK: PDFPageOverlayViewProvider

    nonisolated func pdfView(_ view: PDFView, overlayViewFor page: PDFPage) -> InkPlatformView? {
        MainActor.assumeIsolated {
            guard folder != nil, let doc = view.document else { return nil }
            let i = doc.index(for: page)
            let o = InkOverlay(page: i, size: InkFiles.pageSize(page), owner: self)
            o.show(drawings[i] ?? PKDrawing())
            #if os(iOS)
            prepare(o)
            #endif
            overlays[i] = o
            return o
        }
    }

    nonisolated func pdfView(_ pdfView: PDFView, willEndDisplayingOverlayView overlayView: InkPlatformView, for page: PDFPage) {
        MainActor.assumeIsolated {
            guard let o = overlayView as? InkOverlay else { return }
            if overlays[o.page] === o { overlays[o.page] = nil }
            #if os(iOS)
            toolPicker.removeObserver(o)
            #endif
        }
    }

    // MARK: markup (iPad)

    #if os(iOS)
    /// The tool chosen in the picker (PKToolPickerItem.tool is iOS 26; before it, each kind of item has its own).
    private var pickedTool: PKTool? {
        let item = toolPicker.selectedToolItem
        if #available(iOS 26.0, *) { return item.tool }
        if let i = item as? PKToolPickerInkingItem { return i.inkingTool }
        if let e = item as? PKToolPickerEraserItem { return e.eraserTool }
        if let l = item as? PKToolPickerLassoItem { return l.lassoTool }
        return nil
    }

    private func prepare(_ o: InkOverlay) {
        toolPicker.addObserver(o)
        toolPicker.setVisible(markup, forFirstResponder: o)
        if let t = pickedTool { o.tool = t }
        o.isUserInteractionEnabled = markup
    }

    private func applyMarkup() {
        pdfView?.isInMarkupMode = markup
        for o in overlays.values {
            o.isUserInteractionEnabled = markup
            toolPicker.setVisible(markup, forFirstResponder: o)
            if markup, let t = pickedTool { o.tool = t }
        }
        guard let r = responder else { return }
        toolPicker.setVisible(markup, forFirstResponder: r)
        if markup {
            r.becomeFirstResponder()
        } else {
            // whichever of them has the tool picker
            r.resignFirstResponder()
            overlays.values.forEach { _ = $0.resignFirstResponder() }
            flush()
        }
    }
    #else
    private func applyMarkup() {}
    #endif
}

#if os(iOS)
/// Holds the tool picker on screen in Markup mode (it shows while its responder is first responder).
private final class InkResponder: UIView {
    override var canBecomeFirstResponder: Bool { true }
}

/// A page's canvas: drawn on with the Pencil (fingers keep scrolling and zooming the PDF).
final class InkOverlay: PKCanvasView, PKCanvasViewDelegate {
    let page: Int
    /// The page's size in page space.
    let pageSize: CGSize
    private weak var owner: PDFInk?
    /// The drawing in page space.
    private var pageDrawing = PKDrawing()
    private var shownWidth: CGFloat = 0
    private var applying = false

    init(page: Int, size: CGSize, owner: PDFInk) {
        self.page = page
        pageSize = size
        self.owner = owner
        super.init(frame: .zero)
        backgroundColor = .clear
        isOpaque = false
        drawingPolicy = .pencilOnly
        isScrollEnabled = false
        overrideUserInterfaceStyle = .light   // ink colours as on paper (the page is white)
        isUserInteractionEnabled = false
        delegate = self
    }
    required init?(coder: NSCoder) { fatalError() }

    /// Canvas points per page point.
    private var scale: CGFloat { pageSize.width > 0 && bounds.width > 0 ? bounds.width / pageSize.width : 1 }

    func show(_ d: PKDrawing) {
        pageDrawing = d
        shownWidth = bounds.width
        applying = true
        drawing = d.transformed(using: CGAffineTransform(scaleX: scale, y: scale))
        applying = false
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        // the page was laid out at a new size (zoom, rotation): the strokes follow it
        if bounds.width > 0, abs(bounds.width - shownWidth) > 0.5 { show(pageDrawing) }
    }

    func canvasViewDrawingDidChange(_ canvas: PKCanvasView) {
        guard !applying, bounds.width > 0 else { return }
        let s = scale
        pageDrawing = drawing.transformed(using: CGAffineTransform(scaleX: 1 / s, y: 1 / s))
        owner?.changed(page, pageDrawing)
    }
}

/// Markup on/off for the PDF view (shown on iPad only; iPhone has no Pencil).
struct PDFMarkupButton: View {
    @ObservedObject var ink: PDFInk
    var body: some View {
        Button { ink.markup.toggle() } label: {
            Image(systemName: "pencil.tip.crop.circle")
                .symbolVariant(ink.markup ? .fill : .none)
                .foregroundStyle(ink.markup ? Color.white : Color.accentColor)
                .padding(4)
                .background(ink.markup ? Color.accentColor : .clear, in: Circle())
        }
        .accessibilityLabel(ink.markup ? "Markup on" : "Markup off")
        .help("Draw on the pages with Apple Pencil")
    }
}
#else
/// A page's drawing over the Mac's PDF view (read-only: clicks go through to the page).
final class InkOverlay: NSView {
    let page: Int
    let pageSize: CGSize
    private(set) var drawing = PKDrawing()

    init(page: Int, size: CGSize, owner: PDFInk) {
        self.page = page
        pageSize = size
        super.init(frame: .zero)
    }
    required init?(coder: NSCoder) { fatalError() }

    override var isFlipped: Bool { true }
    override func hitTest(_ point: NSPoint) -> NSView? { nil }

    func show(_ d: PKDrawing) {
        drawing = d
        needsDisplay = true
    }

    override func draw(_ dirtyRect: NSRect) {
        guard !drawing.strokes.isEmpty, pageSize.width > 0, bounds.width > 0 else { return }
        let s = bounds.width / pageSize.width
        let r = drawing.bounds
        let scale = s * (window?.backingScaleFactor ?? 2)
        // ink colours as drawn on the iPad's white page, also in dark mode
        var image: NSImage?
        (NSAppearance(named: .aqua) ?? NSAppearance.currentDrawing()).performAsCurrentDrawingAppearance {
            image = drawing.image(from: r, scale: scale)
        }
        image?.draw(in: CGRect(x: r.minX * s, y: r.minY * s, width: r.width * s, height: r.height * s),
                    from: .zero, operation: .sourceOver, fraction: 1, respectFlipped: true, hints: nil)
    }
}
#endif

#if DEBUG
extension PDFInk {
    /// Screenshots: -inkDemo <json> draws strokes on a page, [{"page":0,"ink":"pen|marker|pencil",
    /// "color":"#rrggbb","alpha":1,"width":4,"points":[[x,y],…]}, …] (page points, top left origin).
    func inkDemo(_ args: [String]) {
        guard let i = args.firstIndex(of: "-inkDemo"), args.indices.contains(i + 1),
              let data = args[i + 1].data(using: .utf8),
              let list = try? JSONSerialization.jsonObject(with: data) as? [[String: Any]] else { return }
        Task { @MainActor in
            try? await Task.sleep(for: .seconds(3))
            var pages: [Int: [PKStroke]] = [:]
            for s in list {
                let page = s["page"] as? Int ?? 0
                let w = s["width"] as? Double ?? 4
                let hex = (s["color"] as? String ?? "#ff0000").dropFirst()
                let v = Int(hex, radix: 16) ?? 0xff0000
                #if os(macOS)
                let color = NSColor(red: CGFloat((v >> 16) & 255) / 255, green: CGFloat((v >> 8) & 255) / 255, blue: CGFloat(v & 255) / 255, alpha: s["alpha"] as? Double ?? 1)
                #else
                let color = UIColor(red: CGFloat((v >> 16) & 255) / 255, green: CGFloat((v >> 8) & 255) / 255, blue: CGFloat(v & 255) / 255, alpha: s["alpha"] as? Double ?? 1)
                #endif
                let type: PKInk.InkType = s["ink"] as? String == "marker" ? .marker : s["ink"] as? String == "pencil" ? .pencil : .pen
                let pts = (s["points"] as? [[Double]] ?? []).enumerated().map { k, p in
                    PKStrokePoint(location: CGPoint(x: p[0], y: p[1]), timeOffset: Double(k) * 0.02, size: CGSize(width: w, height: w),
                                  opacity: 1, force: 1, azimuth: 0, altitude: .pi / 2)
                }
                pages[page, default: []].append(PKStroke(ink: PKInk(type, color: color), path: PKStrokePath(controlPoints: pts, creationDate: Date())))
            }
            for (page, strokes) in pages {
                let d = PKDrawing(strokes: strokes)
                changed(page, d)
                overlays[page]?.show(d)
            }
        }
    }

    /// Simulator test of saving and loading (the simulator has no Pencil): -inkTest puts a
    /// drawing on page 1, saves it, forgets it, reads the file back and checks that it is the same;
    /// -inkMarkup turns Markup on (for a screenshot of the tool picker).
    func inkTest() {
        let args = ProcessInfo.processInfo.arguments
        inkDemo(args)
        if args.contains("-inkMarkup") {
            DispatchQueue.main.asyncAfter(deadline: .now() + 3) { self.markup = true }
        }
        guard args.contains("-inkTest"), let u = url(0) else { return }
        Task { @MainActor in
            func log(_ s: String) { print("INK " + s); fflush(stdout) }
            try? await Task.sleep(for: .seconds(3))
            let pts = (0..<20).map { k in
                PKStrokePoint(location: CGPoint(x: 72 + Double(k) * 12, y: 120 + sin(Double(k) / 3) * 18), timeOffset: Double(k) * 0.02,
                              size: CGSize(width: 4, height: 4), opacity: 1, force: 1, azimuth: 0, altitude: .pi / 2)
            }
            let stroke = PKStroke(ink: PKInk(.pen, color: .systemRed), path: PKStrokePath(controlPoints: pts, creationDate: Date()))
            let marker = PKStroke(ink: PKInk(.marker, color: .systemYellow),
                                  path: PKStrokePath(controlPoints: [CGPoint(x: 72, y: 200), CGPoint(x: 400, y: 200)].enumerated().map { k, p in
                                      PKStrokePoint(location: p, timeOffset: Double(k), size: CGSize(width: 14, height: 14), opacity: 1, force: 1, azimuth: 0, altitude: .pi / 2)
                                  }, creationDate: Date()))
            let d = PKDrawing(strokes: [stroke, marker])
            let before = drawings[0]?.strokes.count ?? 0
            log("page 1 had \(before) strokes; overlay shown: \(overlays[0] != nil); page size \(pdfView?.document?.page(at: 0).map { InkFiles.pageSize($0) } ?? .zero) overlay bounds \(overlays[0]?.bounds.size ?? .zero)")
            changed(0, d)
            if let o = overlays[0] { o.show(d) }
            try? await Task.sleep(for: .seconds(2))
            let exists = FileManager.default.fileExists(atPath: u.path)
            log("saved \(u.lastPathComponent) exists=\(exists) size=\((try? Data(contentsOf: u).count) ?? -1)")
            drawings = [:]; stamps = [:]
            let back = InkFiles.read(u)
            log("read back strokes=\(back?.strokes.count ?? -1) bounds equal=\(back.map { abs($0.bounds.width - d.bounds.width) < 0.5 && abs($0.bounds.minY - d.bounds.minY) < 0.5 } ?? false)")
            // a change "from another device": a third stroke written to the file directly
            if let back {
                var more = back
                more.strokes.append(PKStroke(ink: PKInk(.pen, color: .systemBlue), path: PKStrokePath(controlPoints: [CGPoint(x: 72, y: 260), CGPoint(x: 300, y: 300)].enumerated().map { k, p in
                    PKStrokePoint(location: p, timeOffset: Double(k), size: CGSize(width: 3, height: 3), opacity: 1, force: 1, azimuth: 0, altitude: .pi / 2)
                }, creationDate: Date())))
                try? await Task.sleep(for: .seconds(1.2))
                InkFiles.write(more, to: u)
                try? await Task.sleep(for: .seconds(4))
                log("after outside change: page 1 strokes=\(drawings[0]?.strokes.count ?? -1) (expect 3); overlay strokes=\(overlays[0]?.drawing.strokes.count ?? -1)")
            }
            if args.contains("-inkClear") {
                changed(0, PKDrawing())
                try? await Task.sleep(for: .seconds(2))
                log("cleared: file exists=\(FileManager.default.fileExists(atPath: u.path)) (expect false)")
            }
        }
    }
}
#endif
