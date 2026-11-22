import SwiftUI
import ImageIO
import LuntaCore

// Figure marks: a region of a figure's image marked in the figure viewer (a rectangle or an
// ellipse: "panel c", a cluster in a UMAP), with a colour and an optional note. A mark is an
// annotation of the paper (core/src/figmark.rs: kind .figureMark, location "figure", its region
// in the image's own 0–1 coordinates), so it syncs, undoes and lists as highlights do. The reading
// view never looks for it in the text (showAnnotations leaves marks out); the figure's card shows a
// badge for its marks, the viewer draws them over the image (ZoomableImage), and the Highlights &
// Notes list shows each with a crop of its region ("Figure 3 · region — note"); tapping it opens the
// viewer at the mark.

/// A mark as the viewer draws it: where on the image (0–1 from the top left), how and in what colour.
struct RegionMark: Equatable, Identifiable {
    let id: String
    var rect: CGRect
    var ellipse = false
    var color: HighlightColor = .yellow
    var note = ""
    /// The figure's image it is on.
    var image = 0

    init(_ a: Annotation, image: Int) {
        id = a.id
        let r = figureRegion(annotation: a)
        rect = CGRect(x: r?.x ?? 0, y: r?.y ?? 0, width: r?.w ?? 0, height: r?.h ?? 0)
        ellipse = r?.shape == "ellipse"
        color = HighlightColor(rawValue: a.color) ?? .yellow
        note = a.note
        self.image = image
    }
}

extension Annotation {
    var isFigureMark: Bool { kind == .figureMark || location == "figure" }
}

/// The colour new marks get (outside highlight mode, which marks in its own colour).
enum FigureMarkColor {
    static let key = "figureMarkColor"
    static var current: HighlightColor { HighlightColor(rawValue: UserDefaults.standard.string(forKey: key) ?? "") ?? .yellow }
}

extension LibraryModel {
    /// Saves a new mark on a figure; undo takes it away again.
    @discardableResult
    func addFigureMark(_ paper: String, region: FigureRegion, color: HighlightColor, note: String = "", caption: String,
                       undo: UndoManager? = nil) -> Annotation? {
        let a = newFigureMark(paperId: paper, region: region, color: color.rawValue, note: note, caption: caption)
        do {
            let saved = try lib.saveAnnotation(a: a)
            reload()
            undo?.registerUndo(withTarget: self) { m in MainActor.assumeIsolated { m.deleteAnnotation(saved, undo: undo) } }
            undo?.setActionName("Figure Mark")
            return saved
        } catch { errorMessage = error.readable; return nil }
    }
}

extension ReaderController {
    /// The figures as the core places marks on them: label and image files, in the page's order.
    nonisolated static func figureImages(_ figs: [ReaderFigure]) -> [FigureImages] {
        figs.map { FigureImages(label: $0.label, images: $0.images.map { $0.isFileURL ? $0.path : $0.absoluteString }) }
    }

    /// The paper's marks placed on these figures (the page's, in its order): mark id -> (figure, image).
    func placedMarks(_ all: [Annotation], on figs: [ReaderFigure]) -> [String: (figure: Int, image: Int)] {
        let marks = all.filter(\.isFigureMark)
        guard !marks.isEmpty, !figs.isEmpty else { return [:] }
        var out: [String: (figure: Int, image: Int)] = [:]
        for p in placeFigureMarks(marks: marks, figures: Self.figureImages(figs)) {
            out[p.annotationId] = (Int(p.figure), Int(p.image))
        }
        return out
    }

    /// The marks on figure `index` of the page (`figs`: all its figures), as the viewer draws them.
    func regionMarks(figure index: Int, figs: [ReaderFigure]) -> [RegionMark] {
        guard let model else { return [] }
        let all = model.annotations(paperID)
        let placed = placedMarks(all, on: figs)
        return all.compactMap { a in
            guard let p = placed[a.id], p.figure == index else { return nil }
            return RegionMark(a, image: p.image)
        }
    }

    /// Badges on the figures' cards for the regions marked on them (called with each redraw of the highlights).
    func showFigureMarkBadges(_ all: [Annotation]) {
        let placed = placedMarks(all, on: figures)
        var by: [String: [String]] = [:]
        for a in all where a.isFigureMark {
            if let p = placed[a.id] { by[String(p.figure), default: []].append(a.color) }
        }
        guard let data = try? JSONSerialization.data(withJSONObject: by), let json = String(data: data, encoding: .utf8) else { return }
        let arg = json.replacingOccurrences(of: "\\", with: "\\\\").replacingOccurrences(of: "'", with: "\\'")
        webView?.evaluateJavaScript("window.luntaFigures && luntaFigures.setRegions && luntaFigures.setRegions('\(arg)')")
    }

    /// Opens the figure viewer at a mark (from the Highlights & Notes list, a summary's citation).
    /// False when the mark's figure isn't in the page shown.
    @discardableResult
    func showFigureMark(_ id: String) -> Bool {
        guard let model, let a = model.annotations(paperID).first(where: { $0.id == id }), a.isFigureMark else { return false }
        guard let p = placedMarks([a], on: figures)[id] else {
            // a book read chapter by chapter: the figure may be in another part (opened, then the mark)
            if let ch = chapters, let r = figureRegion(annotation: a) {
                let k = ch.part(forOffset: Int(r.start))
                if k != ch.current { ch.go(k, then: .annotation(id)); return true }
            }
            return false
        }
        Task { @MainActor in
            guard var f = await figure(at: p.figure) ?? figures[safe: p.figure] else { return }
            if f.set.isEmpty { f.set = figures }
            f.start = p.image
            f.focus = id
            if f.set.indices.contains(p.figure) { f.set[p.figure].focus = id; f.set[p.figure].start = p.image }
            onFigure?(f)
        }
        return true
    }
}

extension Array {
    subscript(safe i: Int) -> Element? { indices.contains(i) ? self[i] : nil }
}

/// A figure's image as a region of it: the crop the Highlights & Notes list and the mark's editor show.
func regionCrop(_ url: URL, _ r: CGRect, maxPixels: Int = 900) -> CGImage? {
    guard let src = CGImageSourceCreateWithURL(url as CFURL, nil),
          let full = CGImageSourceCreateImageAtIndex(src, 0, nil) else { return nil }
    let w = CGFloat(full.width), h = CGFloat(full.height)
    // a little around the region, so it shows where it is
    let pad = 0.04
    let box = CGRect(x: (r.minX - pad) * w, y: (r.minY - pad) * h, width: (r.width + 2 * pad) * w, height: (r.height + 2 * pad) * h)
        .intersection(CGRect(x: 0, y: 0, width: w, height: h)).integral
    guard box.width >= 2, box.height >= 2, let crop = full.cropping(to: box) else { return nil }
    let longest = max(crop.width, crop.height)
    guard longest > maxPixels else { return crop }
    let s = CGFloat(maxPixels) / CGFloat(longest)
    let tw = max(1, Int(CGFloat(crop.width) * s)), th = max(1, Int(CGFloat(crop.height) * s))
    guard let ctx = CGContext(data: nil, width: tw, height: th, bitsPerComponent: 8, bytesPerRow: 0,
                              space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { return crop }
    ctx.interpolationQuality = .high
    ctx.draw(crop, in: CGRect(x: 0, y: 0, width: tw, height: th))
    return ctx.makeImage() ?? crop
}

/// The region of a mark, cropped from its image: given the image, or found in the paper's reading view.
struct MarkThumbnail: View {
    @EnvironmentObject var model: LibraryModel
    let annotation: Annotation
    var url: URL? = nil
    var size: CGFloat = 54
    @State private var image: CGImage?
    @State private var looked = false

    var body: some View {
        ZStack {
            if let image {
                Image(decorative: image, scale: 1).resizable().scaledToFit()
            } else {
                Image(systemName: "rectangle.dashed").font(.title3).foregroundStyle(.secondary)
                    .opacity(looked ? 1 : 0.4)
            }
        }
        .frame(width: size, height: size)
        .background(Color.white)
        .clipShape(RoundedRectangle(cornerRadius: 6))
        .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder((HighlightColor(rawValue: annotation.color) ?? .yellow).color, lineWidth: 2))
        .task(id: "\(annotation.id)|\(annotation.position)") {
            let lib = model.lib, a = annotation, given = url
            image = await Task.detached(priority: .userInitiated) { () -> CGImage? in
                guard let r = figureRegion(annotation: a) else { return nil }
                let file = given ?? lib.figureMarkImage(paper: a.paperId, id: a.id).map { URL(fileURLWithPath: $0) }
                guard let file else { return nil }
                return regionCrop(file, CGRect(x: r.x, y: r.y, width: r.w, height: r.h), maxPixels: 300)
            }.value
            looked = true
        }
        .accessibilityHidden(true)
    }
}

/// "Figure 3 · region": a mark's line in lists.
func figureMarkTitle(_ a: Annotation) -> String {
    let label = figureRegion(annotation: a)?.figure ?? ""
    return "\(label.isEmpty ? "Figure" : label) · region"
}

// MARK: - Editing one mark

/// A mark's editor: its region (cropped), colour, shape and note; Delete Mark.
struct FigureMarkEditor: View {
    @EnvironmentObject var model: LibraryModel
    @Environment(\.dismiss) private var dismiss
    @Environment(\.highlightUndo) private var undo
    @State var annotation: Annotation
    /// The image it is on (the viewer has it at hand), else found in the reading view.
    var imageURL: URL? = nil
    var onDone: () -> Void = {}
    @State private var ellipse = false
    @FocusState private var noteFocused: Bool

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    HStack {
                        Spacer()
                        MarkThumbnail(annotation: annotation, url: imageURL, size: 150)
                        Spacer()
                    }
                    if !annotation.suffix.isEmpty {
                        Text(annotation.suffix).font(.footnote).foregroundStyle(.secondary).lineLimit(4)
                    }
                }
                Section("Color") {
                    HStack(spacing: 14) {
                        ForEach(HighlightColor.allCases) { c in
                            Button { annotation.color = c.rawValue } label: {
                                Circle().fill(c.color).frame(width: 28, height: 28)
                                    .overlay(Circle().stroke(Color.primary, lineWidth: annotation.color == c.rawValue ? 2.5 : 0))
                            }
                            .buttonStyle(.plain)
                            .accessibilityLabel(c.name)
                        }
                    }
                    .padding(.vertical, 4)
                }
                Section("Shape") {
                    Picker("Shape", selection: $ellipse) {
                        Label("Rectangle", systemImage: "rectangle").tag(false)
                        Label("Ellipse", systemImage: "oval").tag(true)
                    }
                    .pickerStyle(.segmented)
                    .labelsHidden()
                }
                Section("Note") {
                    TextField("What is here (panel c, a cluster…)", text: $annotation.note, axis: .vertical)
                        .focused($noteFocused)
                        #if os(iOS)
                        .lineLimit(UIDevice.current.userInterfaceIdiom == .pad ? 4...12 : 3...8)
                        #else
                        .lineLimit(3...10)
                        #endif
                }
                Section {
                    Button("Delete Mark", role: .destructive) {
                        model.deleteAnnotation(annotation, undo: undo); onDone(); dismiss()
                    }
                }
            }
            .formStyle(.grouped)
            .onAppear { ellipse = figureRegion(annotation: annotation)?.shape == "ellipse" }
            .navigationTitle(figureMarkTitle(annotation))
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Save") { save(); onDone(); dismiss() }
                }
            }
        }
        #if os(macOS)
        .frame(minWidth: 400, minHeight: 520)
        #endif
    }

    /// Saved as it is now; undo puts back the mark as it was.
    private func save() {
        var a = annotation
        if var r = figureRegion(annotation: a) {
            r.shape = ellipse ? "ellipse" : "rect"
            a = setFigureRegion(annotation: a, region: r)
        }
        a.note = a.note.trimmingCharacters(in: .whitespacesAndNewlines)
        let before = model.annotations(a.paperId).first { $0.id == a.id }
        model.saveAnnotation(a)
        UserDefaults.standard.set(a.color, forKey: FigureMarkColor.key)
        if let before, let undo { registerEditUndo(before: before, after: a, undo: undo) }
    }

    private func registerEditUndo(before: Annotation, after: Annotation, undo: UndoManager) {
        let m = model
        undo.registerUndo(withTarget: m) { m in MainActor.assumeIsolated {
            m.saveAnnotation(before)
            undo.registerUndo(withTarget: m) { m in MainActor.assumeIsolated { m.saveAnnotation(after) } }
            undo.setActionName("Figure Mark")
        } }
        undo.setActionName("Figure Mark")
    }
}

// MARK: - Drawing marks on the image

/// What the viewer's image does with marks: those to draw, whether new ones can be drawn and how.
struct MarkTool {
    var regions: [RegionMark] = []
    /// Marks can be drawn (a figure of the reader's page, with the library at hand).
    var enabled = false
    /// The Mark tool or highlight mode is on: one finger (a drag on the Mac) draws a mark at once.
    /// Otherwise: a long press, then drag (iPhone, iPad), ⌥-drag (Mac); the Apple Pencil always draws.
    var direct = false
    var color: HighlightColor = .yellow
    /// A mark to show: zoomed to and flashed.
    var focus = ""
    /// A region drawn (0–1 of the image, from its top left).
    var onDraw: (CGRect) -> Void = { _ in }
    /// A mark tapped or clicked.
    var onTap: (String) -> Void = { _ in }
}

/// A region from a drag in a view of `size`: 0–1 of the image, from its top left.
func normalizedRegion(_ a: CGPoint, _ b: CGPoint, in size: CGSize) -> CGRect {
    guard size.width > 0, size.height > 0 else { return .zero }
    let x0 = min(max(min(a.x, b.x), 0), size.width), x1 = min(max(max(a.x, b.x), 0), size.width)
    let y0 = min(max(min(a.y, b.y), 0), size.height), y1 = min(max(max(a.y, b.y), 0), size.height)
    return CGRect(x: x0 / size.width, y: y0 / size.height, width: (x1 - x0) / size.width, height: (y1 - y0) / size.height)
}

/// The topmost (smallest) mark at a point of a view of `size`, with a little slack around thin ones.
func markAt(_ p: CGPoint, in size: CGSize, _ marks: [RegionMark], slack: CGFloat = 6) -> String? {
    guard size.width > 0, size.height > 0 else { return nil }
    let hits = marks.filter { m in
        let r = CGRect(x: m.rect.minX * size.width, y: m.rect.minY * size.height, width: m.rect.width * size.width, height: m.rect.height * size.height)
        return r.insetBy(dx: -slack, dy: -slack).contains(p)
    }
    return hits.min { $0.rect.width * $0.rect.height < $1.rect.width * $1.rect.height }?.id
}

/// Draws marks (and the one being drawn) over the image, in the image view's own coordinates
/// (flipped: top left 0, 0), so they zoom and scroll with it.
final class MarkPainter {
    var marks: [RegionMark] = []
    var live: CGRect?
    var liveColor: HighlightColor = .yellow
    var flash = ""
    /// The zoom: lines stay as thick on screen at any zoom.
    var scale: CGFloat = 1

    func draw(in ctx: CGContext, size: CGSize) {
        let line = 2 / max(scale, 0.01)
        for m in marks {
            let r = CGRect(x: m.rect.minX * size.width, y: m.rect.minY * size.height, width: m.rect.width * size.width, height: m.rect.height * size.height)
            let path = m.ellipse ? CGPath(ellipseIn: r, transform: nil) : CGPath(roundedRect: r, cornerWidth: min(3 / scale, r.width / 2), cornerHeight: min(3 / scale, r.height / 2), transform: nil)
            let c = cg(m.color.color)
            ctx.addPath(path)
            ctx.setFillColor(c.copy(alpha: flash == m.id ? 0.32 : 0.16) ?? c)
            ctx.fillPath()
            ctx.addPath(path)
            ctx.setStrokeColor(c.copy(alpha: 0.95) ?? c)
            ctx.setLineWidth(flash == m.id ? line * 2.2 : line)
            ctx.strokePath()
            if !m.note.isEmpty {   // a note: a small dot at the top right corner, as a note's underline in the text
                let d = 7 / max(scale, 0.01)
                ctx.setFillColor(cg(Color(red: 0.78, green: 0.47, blue: 0.16)))
                ctx.fillEllipse(in: CGRect(x: r.maxX - d * 0.6, y: r.minY - d * 0.4, width: d, height: d))
            }
        }
        if let r = live {
            let c = cg(liveColor.color)
            ctx.setFillColor(c.copy(alpha: 0.14) ?? c)
            ctx.fill(r)
            ctx.setStrokeColor(c)
            ctx.setLineWidth(line)
            ctx.setLineDash(phase: 0, lengths: [5 / scale, 3 / scale])
            ctx.stroke(r)
            ctx.setLineDash(phase: 0, lengths: [])
        }
    }

    private func cg(_ c: Color) -> CGColor {
        #if os(iOS)
        UIColor(c).cgColor
        #else
        NSColor(c).cgColor
        #endif
    }
}

#if os(iOS)
/// The marks over the image (a subview of the zoomed image view).
final class MarkOverlayView: UIView {
    let painter = MarkPainter()
    override init(frame: CGRect) {
        super.init(frame: frame)
        isOpaque = false
        backgroundColor = .clear
        contentMode = .redraw
        isUserInteractionEnabled = false   // the image view's recognizers take the touches
    }
    required init?(coder: NSCoder) { fatalError() }
    override func draw(_ rect: CGRect) {
        guard let ctx = UIGraphicsGetCurrentContext() else { return }
        painter.draw(in: ctx, size: bounds.size)
    }
}
#else
/// The marks over the image (a subview of the image view, flipped), and the mouse for drawing them.
final class MarkOverlayView: NSView {
    let painter = MarkPainter()
    var tool = MarkTool()
    private var start: CGPoint?

    override var isFlipped: Bool { true }

    override func draw(_ dirtyRect: NSRect) {
        guard let ctx = NSGraphicsContext.current?.cgContext else { return }
        painter.scale = enclosingScrollView?.magnification ?? 1
        painter.draw(in: ctx, size: bounds.size)
    }

    /// Takes the mouse to draw (the Mark tool, highlight mode, ⌥ held) or on a mark (to open it).
    override func hitTest(_ point: NSPoint) -> NSView? {
        guard tool.enabled, let sup = superview else { return nil }
        let p = convert(point, from: sup)
        guard bounds.contains(p) else { return nil }
        if tool.direct || NSEvent.modifierFlags.contains(.option) { return self }
        return markAt(p, in: bounds.size, tool.regions, slack: 3) != nil ? self : nil
    }

    override func resetCursorRects() {
        if tool.enabled && tool.direct { addCursorRect(bounds, cursor: .crosshair) }
    }

    override func mouseDown(with e: NSEvent) {
        start = convert(e.locationInWindow, from: nil)
    }

    override func mouseDragged(with e: NSEvent) {
        guard let s = start, tool.direct || e.modifierFlags.contains(.option) else { return }
        let p = convert(e.locationInWindow, from: nil)
        painter.live = CGRect(x: min(s.x, p.x), y: min(s.y, p.y), width: abs(p.x - s.x), height: abs(p.y - s.y)).intersection(bounds)
        painter.liveColor = tool.color
        needsDisplay = true
        autoscroll(with: e)
    }

    override func mouseUp(with e: NSEvent) {
        defer { start = nil; painter.live = nil; needsDisplay = true }
        guard let s = start else { return }
        let p = convert(e.locationInWindow, from: nil)
        let zoom = enclosingScrollView?.magnification ?? 1
        if painter.live != nil, abs(p.x - s.x) * zoom >= 8, abs(p.y - s.y) * zoom >= 8 {
            tool.onDraw(normalizedRegion(s, p, in: bounds.size))
        } else if e.clickCount <= 1, let id = markAt(p, in: bounds.size, tool.regions, slack: 3) {
            tool.onTap(id)
        }
    }
}
#endif
