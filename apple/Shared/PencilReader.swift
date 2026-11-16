#if os(iOS)
import UIKit
import WebKit
import SwiftUI

/// Apple Pencil in the reading view (iPad). Fingers are not affected.
///  - A stroke along the text highlights it in the highlight-mode colour: from the word under the
///    stroke's start to the word under its end, coloured at once and saved like a highlight made
///    from the selection menu. The Pencil doesn't scroll or turn pages (fingers do).
///  - A long press with the Pencil on a paragraph, or a double-tap on the Pencil (where it hovers,
///    or where it last touched), adds a note to that paragraph and opens the note editor, where
///    Scribble turns handwriting into text.
@MainActor
final class PencilReader: NSObject, UIGestureRecognizerDelegate, UIPencilInteractionDelegate {
    private weak var webView: ReaderWebView?
    private let trail = CAShapeLayer()
    private var points: [CGPoint] = []
    /// Where the Pencil last was (hovering or touching), for a double-tap.
    private var lastPencil: CGPoint?
    /// A long press made a note: the rest of that touch draws nothing.
    private var pressed = false

    init(_ v: ReaderWebView) {
        webView = v
        super.init()
        let pencil = [NSNumber(value: UITouch.TouchType.pencil.rawValue)]
        let stroke = UIPanGestureRecognizer(target: self, action: #selector(stroked(_:)))
        stroke.allowedTouchTypes = pencil
        stroke.maximumNumberOfTouches = 1
        stroke.delegate = self
        v.addGestureRecognizer(stroke)
        let press = UILongPressGestureRecognizer(target: self, action: #selector(longPressed(_:)))
        press.allowedTouchTypes = pencil
        press.minimumPressDuration = 0.45
        press.allowableMovement = 8
        press.delegate = self
        v.addGestureRecognizer(press)
        let hover = UIHoverGestureRecognizer(target: self, action: #selector(hovered(_:)))
        v.addGestureRecognizer(hover)
        let interaction = UIPencilInteraction(delegate: self)
        v.addInteraction(interaction)
        // the Pencil marks text instead of scrolling or turning pages
        v.scrollView.panGestureRecognizer.allowedTouchTypes = [UITouch.TouchType.direct, .indirect, .indirectPointer]
            .map { NSNumber(value: $0.rawValue) }

        trail.fillColor = nil
        trail.lineCap = .round
        trail.lineJoin = .round
        trail.lineWidth = 16
        trail.zPosition = 1000
        v.layer.addSublayer(trail)
        #if DEBUG
        pencilTest()
        #endif
    }

    private var color: HighlightColor {
        HighlightColor(rawValue: UserDefaults.standard.string(forKey: "highlightModeColor") ?? "") ?? .yellow
    }

    func gestureRecognizer(_ g: UIGestureRecognizer, shouldRecognizeSimultaneouslyWith other: UIGestureRecognizer) -> Bool {
        // the long press and the stroke watch the same touch; WebKit's own gestures stay out
        other.view === g.view && other.delegate === self
    }

    // MARK: strokes

    @objc private func stroked(_ g: UIPanGestureRecognizer) {
        guard let v = webView else { return }
        let p = g.location(in: v)
        lastPencil = p
        switch g.state {
        case .began:
            guard !pressed else { return }
            let t = g.translation(in: v)
            points = [CGPoint(x: p.x - t.x, y: p.y - t.y), p]
            trail.strokeColor = UIColor(color.color).withAlphaComponent(0.45).cgColor
            trail.opacity = 1
            drawTrail()
        case .changed:
            guard !pressed, !points.isEmpty else { return }
            points.append(p)
            drawTrail()
        case .ended:
            guard !pressed, let first = points.first else { endStroke(); return }
            highlight(from: first, to: p)
            endStroke()
        default:
            endStroke()
        }
    }

    private func drawTrail() {
        let path = UIBezierPath()
        guard let first = points.first else { trail.path = nil; return }
        path.move(to: first)
        points.dropFirst().forEach { path.addLine(to: $0) }
        CATransaction.begin(); CATransaction.setDisableActions(true)
        trail.frame = webView?.bounds ?? .zero
        trail.path = path.cgPath
        CATransaction.commit()
    }

    private func endStroke() {
        points = []
        CATransaction.begin()
        CATransaction.setCompletionBlock { [weak self] in
            guard let self, self.points.isEmpty else { return }
            self.trail.path = nil
        }
        trail.opacity = 0
        CATransaction.commit()
    }

    /// The page's coordinates (CSS pixels from the top left of what the page shows) of a point in
    /// the web view: the scroll view's insets (under the bars) aren't part of the page's viewport.
    private func client(_ p: CGPoint) -> CGPoint {
        guard let sv = webView?.scrollView else { return p }
        return CGPoint(x: p.x - sv.adjustedContentInset.left, y: p.y - sv.adjustedContentInset.top)
    }

    /// Highlights the text between two points of the web view (in the highlight-mode colour).
    func highlight(from a: CGPoint, to b: CGPoint) {
        guard let v = webView else { return }
        let ca = client(a), cb = client(b)
        let js = "luntaPencilHighlight(\(ca.x), \(ca.y), \(cb.x), \(cb.y), '\(color.rawValue)')"
        let c = color
        Task {
            guard let json = try? await v.evaluateJavaScript(js) as? String, let d = json.data(using: .utf8),
                  let sel = try? JSONDecoder().decode(PageSelection.self, from: d) else { return }
            save(.highlight(c), sel)
        }
    }

    private func save(_ action: ReaderMenuAction, _ sel: PageSelection) {
        guard let controller = webView?.controller, let onMenu = controller.onMenu else { return }
        controller.pencilSelection = (sel, Date())
        onMenu(action)
    }

    // MARK: notes

    @objc private func longPressed(_ g: UILongPressGestureRecognizer) {
        guard let v = webView else { return }
        switch g.state {
        case .began:
            pressed = true
            points = []
            trail.path = nil
            UIImpactFeedbackGenerator(style: .medium).impactOccurred()
            note(at: g.location(in: v))
        case .ended, .cancelled, .failed:
            pressed = false
        default: break
        }
    }

    @objc private func hovered(_ g: UIHoverGestureRecognizer) {
        if let v = webView, g.state == .began || g.state == .changed { lastPencil = g.location(in: v) }
    }

    func pencilInteraction(_ interaction: UIPencilInteraction, didReceiveTap tap: UIPencilInteraction.Tap) {
        // the user's double-tap setting: "Off" means nothing happens
        guard UIPencilInteraction.preferredTapAction != .ignore, let v = webView else { return }
        let p = tap.hoverPose?.location ?? lastPencil ?? CGPoint(x: v.bounds.midX, y: v.bounds.midY)
        note(at: p)
    }

    /// A note on the paragraph at a point of the web view, opened in the note editor.
    func note(at p: CGPoint) {
        guard let v = webView else { return }
        let c = client(p)
        Task {
            _ = try? await v.evaluateJavaScript("window.getSelection().removeAllRanges()")
            guard let json = try? await v.evaluateJavaScript("luntaParagraphAt(\(c.x), \(c.y))") as? String,
                  let d = json.data(using: .utf8), let sel = try? JSONDecoder().decode(PageSelection.self, from: d) else { return }
            AnnotationEditor.focusNext = Date()
            save(.note, sel)
        }
    }
}

#if DEBUG
extension PencilReader {
    /// Simulator test (no Pencil there): -pencilTest takes the 3rd to 9th word of a paragraph on
    /// screen, strokes from the middle of one to the middle of the other (the trail is drawn as a
    /// stroke would) and checks the quote and offsets the page gives, then that the highlight was
    /// saved (drawn again with its real id). -pencilNoteTest then makes a note on that paragraph.
    func pencilTest() {
        let args = ProcessInfo.processInfo.arguments
        guard args.contains("-pencilTest") || args.contains("-pencilNoteTest") else { return }
        Task { @MainActor in
            func log(_ s: String) { print("PENCIL " + s); fflush(stdout) }
            try? await Task.sleep(for: .seconds(12))   // loaded, and past the restored place
            guard let v = webView else { return }
            _ = try? await v.evaluateJavaScript("window.luntaHold && luntaHold()")
            // a paragraph in view with enough words; the client rects of its 3rd and 9th words
            let probe = #"""
            (function(){
              var ps = document.querySelectorAll('p');
              for (var i = 0; i < ps.length; i++) {
                var p = ps[i], r = p.getBoundingClientRect();
                if (p.textContent.length < 300 || r.left < 0 || r.right > window.innerWidth) continue;
                var w = document.createTreeWalker(p, NodeFilter.SHOW_TEXT), n, words = [];
                while ((n = w.nextNode()) && words.length < 12) {
                  var re = /[A-Za-z]{3,}/g, m;
                  while ((m = re.exec(n.nodeValue)) && words.length < 12) words.push([n, m.index, m.index + m[0].length]);
                }
                if (words.length < 10) continue;
                function rect(k){ var g = document.createRange(); g.setStart(words[k][0], words[k][1]); g.setEnd(words[k][0], words[k][2]); return g.getClientRects()[0]; }
                var a = rect(2), b = rect(8);
                if (a.top < 100 || b.bottom > window.innerHeight - 60) { if (window.__pt) continue; window.__pt = 1; p.scrollIntoView({ block: 'center', behavior: 'instant' }); return 'scrolled'; }
                var g = document.createRange(); g.setStart(words[2][0], words[2][1]); g.setEnd(words[8][0], words[8][2]);
                return JSON.stringify({ x1: a.left + a.width / 2, y1: a.top + a.height / 2, x2: b.left + b.width / 2, y2: b.top + b.height / 2, expect: g.toString() });
              }
              return '';
            })()
            """#
            var pj0 = try? await v.evaluateJavaScript(probe) as? String
            if pj0 == "scrolled" { try? await Task.sleep(for: .seconds(1.5)); pj0 = try? await v.evaluateJavaScript(probe) as? String }
            guard let pj = pj0, let pd = pj.data(using: .utf8),
                  let probe = try? JSONSerialization.jsonObject(with: pd) as? [String: Any],
                  let x1 = probe["x1"] as? Double, let y1 = probe["y1"] as? Double,
                  let x2 = probe["x2"] as? Double, let y2 = probe["y2"] as? Double, let expect = probe["expect"] as? String else {
                log("no paragraph found"); return
            }
            let inset = v.scrollView.adjustedContentInset
            // client → web view points (the inverse of `client`)
            let a = CGPoint(x: x1 + inset.left, y: y1 + inset.top), b = CGPoint(x: x2 + inset.left, y: y2 + inset.top)
            log("inset top=\(inset.top) left=\(inset.left); stroke \(a) → \(b); expect “\(expect)”")
            // the bare selection first (no colouring)
            let check = """
            (function(){ var j = luntaSelectionBetween(\(x1), \(y1), \(x2), \(y2)); if (!j) return 'empty';
              var o = JSON.parse(j), t = '', w = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT), n;
              while ((n = w.nextNode())) { var p = n.parentNode.nodeName.toLowerCase(); if (p !== 'script' && p !== 'style') t += n.nodeValue; }
              return JSON.stringify({ quote: o.quote, start: o.start, atStart: t.slice(o.start, o.start + o.quote.length) === o.quote,
                prefixOK: t.slice(o.start - o.prefix.length, o.start) === o.prefix, suffixOK: t.slice(o.start + o.quote.length, o.start + o.quote.length + o.suffix.length) === o.suffix }); })()
            """
            log("scripts: " + ((try? await v.evaluateJavaScript("typeof luntaSelectionBetween + ' ' + typeof luntaApplyAnnotations + ' ' + typeof luntaParagraphAt")) as? String ?? "-"))
            let res: String
            do { res = try await v.evaluateJavaScript(check) as? String ?? "-" } catch { res = "error \(error)" }
            log("selectionBetween: \(res)")
            if let d = res.data(using: .utf8), let o = try? JSONSerialization.jsonObject(with: d) as? [String: Any] {
                log("quote matches expected: \((o["quote"] as? String) == expect)")
            }
            if args.contains("-pencilTest") {
                points = [a, CGPoint(x: (a.x + b.x) / 2, y: (a.y + b.y) / 2), b]
                trail.strokeColor = UIColor(color.color).withAlphaComponent(0.45).cgColor
                trail.opacity = 1
                drawTrail()
                highlight(from: a, to: b)
                try? await Task.sleep(for: .milliseconds(150))
                let pending = (try? await v.evaluateJavaScript("Array.prototype.map.call(document.querySelectorAll('mark[data-ann=\"__pending\"]'), function(m){ return m.textContent; }).join('')")) as? String ?? "-"
                log("coloured at once: “\(pending)”")
                try? await Task.sleep(for: .seconds(2))
                let saved = (try? await v.evaluateJavaScript("""
                    (function(){ var by = {}; document.querySelectorAll('mark[data-ann]').forEach(function(m){ var id = m.getAttribute('data-ann'); by[id] = (by[id] || '') + m.textContent; });
                      var out = []; for (var id in by) if (by[id] === \(Self.jsString(expect))) out.push(id + ' ' + (document.querySelector('mark[data-ann="' + id + '"]').className)); return out.join(', '); })()
                    """)) as? String ?? "-"
                log("saved and drawn: \(saved.isEmpty ? "NO" : saved)")
                if !args.contains("-pencilKeepTrail") { endStroke() }
            }
            if args.contains("-pencilNoteTest") {
                try? await Task.sleep(for: .seconds(1))
                note(at: CGPoint(x: (a.x + b.x) / 2, y: (a.y + b.y) / 2))
                let pj2 = (try? await v.evaluateJavaScript("luntaParagraphAt(\(x1), \(y1))")) as? String ?? "-"
                log("paragraph note anchor: \(pj2)")
            }
        }
    }

    private static func jsString(_ s: String) -> String {
        let d = (try? JSONSerialization.data(withJSONObject: [s])) ?? Data("[\"\"]".utf8)
        return String(String(data: d, encoding: .utf8)!.dropFirst().dropLast())
    }
}
#endif
#endif
