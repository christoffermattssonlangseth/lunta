import SwiftUI
import WebKit

// Figures in the reading view. Mentions of a figure in the text ("Figure 2", "Fig. 2B",
// "Figures 3A and 3C", "Extended Data Fig. 4", "Figure S1") and the papers' own links to
// figures are made tappable when the paper has that figure; tapping one (or a figure's image)
// opens the figure on its own: the image large and zoomable, its label and full caption below.
// Closing it leaves the reading view exactly where it was (nothing in the page moves).
//
// Done in the page as it is shown, so it works for every reading view already made.

/// A figure to show: its images (`start` the one tapped), label ("Figure 2"), title and caption.
struct ReaderFigure: Identifiable, Equatable {
    let id = UUID()
    var images: [URL]
    var start = 0
    var label = ""
    var title = ""
    var caption = ""
    /// The panel the text pointed to ("B" in "Fig. 2B"), if any.
    var panel = ""
}

/// Receives "fig" messages (a tapped figure mention or image) from the page.
final class FigureMessageHandler: NSObject, WKScriptMessageHandler {
    weak var controller: ReaderController?
    func userContentController(_ c: WKUserContentController, didReceive m: WKScriptMessage) {
        guard let d = m.body as? [String: Any] else { return }
        let srcs = (d["srcs"] as? [String]) ?? [d["src"] as? String].compactMap { $0 }
        let images = srcs.compactMap(URL.init(string:))
        func s(_ k: String) -> String { (d[k] as? String ?? "").trimmingCharacters(in: .whitespacesAndNewlines) }
        let f = ReaderFigure(images: images, start: min(max(0, d["start"] as? Int ?? 0), max(0, images.count - 1)),
                             label: s("label"), title: s("title"), caption: s("caption"), panel: s("panel"))
        guard !f.images.isEmpty || !f.caption.isEmpty || !f.title.isEmpty else { return }
        Task { @MainActor in self.controller?.onFigure?(f) }
    }
}

extension ReaderAssets {
    /// Links figure mentions to their figures and sends taps on them (and on images) to the app.
    /// Part of `styleScript`, so its click handler runs before the highlight and page-turn ones.
    static let figureJS = #"""
    (function(){
      if (!document.body || window.luntaFigures) return;
      var st = document.createElement('style');
      st.textContent = '.m-figref { color: #2a6478; cursor: pointer; text-decoration: underline; text-decoration-color: rgba(42,100,120,.35);' +
        ' text-decoration-thickness: 1px; text-underline-offset: 2px; -webkit-tap-highlight-color: rgba(42,100,120,.18); border-radius: 2px; }' +
        '@media (prefers-color-scheme: dark) { .m-figref { color: #9ec9d8; text-decoration-color: rgba(158,201,216,.35); -webkit-tap-highlight-color: rgba(158,201,216,.2); } }';
      document.documentElement.appendChild(st);

      var PRE = '(Extended\\s+Data\\s+|Supplementa(?:ry|l)\\s+|Suppl?\\.\\s*)?';
      var LABEL = new RegExp('^\\s*' + PRE + '(?:Figure|Fig)s?\\.?\\s*(S?\\d+)', 'i');
      var ANY_LABEL = new RegExp('^\\s*' + PRE + '(?:Figure|Fig|Table)s?\\.?\\s*S?\\d+[A-Za-z]?(?![A-Za-z0-9])', 'i');
      var MENTION = new RegExp('(?<![A-Za-z])' + PRE + '(Figures|Figs\\.?|Figure|Fig\\.?)\\s*(S?\\d+)(?:([A-Za-z])(?![A-Za-z]))?(?![0-9])', 'gi');
      var MORE = /^(\s*(?:,|;|and|&|or|to|–|—|-)\s*)(S?\d+)(?:([A-Za-z])(?![A-Za-z]))?(?![0-9])/i;
      var CUT = /^[\s|.:—–-]+/;
      function norm(s){ return (s || '').replace(/\s+/g, ' ').trim(); }
      // "Extended Data Fig. 4" -> E4, "Figure S1" and "Supplementary Figure 1" -> S1, "Fig. 2" -> 2
      function key(pre, num){
        num = num.toUpperCase(); pre = (pre || '').toLowerCase();
        if (pre.indexOf('ext') === 0) return 'E' + num;
        if (pre && num.charAt(0) !== 'S') return 'S' + num;
        return num;
      }
      // the outermost figure holding an element (sub-figures belong to their figure)
      function outer(el){
        var f = el && el.closest ? el.closest('figure, .fig') : null;
        while (f && f.parentNode && f.parentNode.closest && f.parentNode.closest('figure, .fig')) f = f.parentNode.closest('figure, .fig');
        return f;
      }
      function captionOf(box){
        for (var c = box.firstElementChild; c; c = c.nextElementSibling) if (/^figcaption$/i.test(c.nodeName) || c.classList.contains('caption')) return c;
        return box.querySelector('figcaption, .caption');
      }

      // every figure in the book, and the label each is known by
      var figs = [], byKey = {};
      document.querySelectorAll('figure, .fig').forEach(function(f){
        if (outer(f) !== f) return;
        var i = figs.length; figs.push(f); f.setAttribute('data-m-fig', i);
        var cap = captionOf(f), img = f.querySelector('img');
        var m = LABEL.exec(norm(cap ? cap.textContent : '')) || (img ? LABEL.exec(norm(img.getAttribute('alt'))) : null);
        if (m) { var k = key(m[1], m[2]); if (!(k in byKey)) byKey[k] = i; }
      });

      // label, title and caption text of a figure (or table)
      function parts(box){
        var cap = captionOf(box), img = box.querySelector('img');
        if (cap) {   // reference numbers ("variability<sup>14</sup>") would run into the words
          cap = cap.cloneNode(true);
          cap.querySelectorAll('sup').forEach(function(x){ if (x.querySelector('a') || x.closest('a')) x.remove(); });
          cap.querySelectorAll('a[href^="#"]').forEach(function(x){ if (x.closest('sup')) x.remove(); });
        }
        if (!cap) return { label: '', title: '', caption: img ? norm(img.getAttribute('alt')) : '' };
        var strongs = [];
        for (var n = cap.firstChild; n && strongs.length < 2; n = n.nextSibling) {
          if (n.nodeType === 3 && !n.nodeValue.trim()) continue;
          if (n.nodeType === 1 && (/^(strong|b)$/i.test(n.nodeName) || n.classList.contains('ltx_tag'))) { strongs.push(norm(n.textContent)); continue; }
          break;
        }
        var full = norm(cap.textContent), m = ANY_LABEL.exec(full);
        var label = m ? m[0].trim() : '', rest = (m ? full.slice(m[0].length) : full).replace(CUT, '');
        var title = '';
        if (strongs.length) {
          var s0 = strongs[0];
          if (label && s0.indexOf(label) === 0) s0 = s0.slice(label.length).replace(CUT, '');
          else if (label) s0 = '';
          title = s0 || (strongs.length > 1 ? strongs[1] : '');
          if (title.length < 4 || rest.indexOf(title) !== 0) title = '';
          if (title) rest = rest.slice(title.length).trim();
        }
        if (!label && img) { var a = ANY_LABEL.exec(norm(img.getAttribute('alt'))); if (a) label = a[0].trim(); }
        return { label: label, title: title, caption: rest };
      }

      // mentions in the text
      var SKIP = 'a, script, style, .m-figref, .references, ol.refs, .refs, nav, math, h1, h2, h3, h4, title, button';
      if (figs.length) {
        var w = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT, null), t, list = [];
        while ((t = w.nextNode())) if (/fig/i.test(t.nodeValue)) list.push(t);
        list.forEach(function(n){
          var p = n.parentNode; if (!p || !p.closest || p.closest(SKIP)) return;
          var own = outer(p), ownIdx = own ? own.getAttribute('data-m-fig') : null;
          var s = n.nodeValue, out = [], m;
          MENTION.lastIndex = 0;
          while ((m = MENTION.exec(s))) {
            var refs = [{ at: m.index, end: m.index + m[0].length, num: m[3], panel: m[4] }];
            if (/^fig(ure)?s/i.test(m[2])) {
              var pos = refs[0].end, mm;
              while ((mm = MORE.exec(s.slice(pos)))) { refs.push({ at: pos + mm[1].length, end: pos + mm[0].length, num: mm[2], panel: mm[3] }); pos += mm[0].length; }
              MENTION.lastIndex = pos;
            }
            for (var r = 0; r < refs.length; r++) {
              var i = byKey[key(m[1], refs[r].num)];
              if (i == null || String(i) === ownIdx) continue;
              out.push({ at: refs[r].at, end: refs[r].end, fig: i, panel: refs[r].panel || '' });
            }
          }
          if (!out.length) return;
          var frag = document.createDocumentFragment(), cur = 0;
          out.forEach(function(o){
            if (o.at > cur) frag.appendChild(document.createTextNode(s.slice(cur, o.at)));
            var sp = document.createElement('span');
            sp.className = 'm-figref'; sp.setAttribute('data-fig-ref', o.fig); if (o.panel) sp.setAttribute('data-panel', o.panel);
            sp.textContent = s.slice(o.at, o.end);
            frag.appendChild(sp); cur = o.end;
          });
          if (cur < s.length) frag.appendChild(document.createTextNode(s.slice(cur)));
          p.replaceChild(frag, n);
        });
        // the paper's own links to figures (JATS cross-references, arXiv): open the figure instead of jumping to it
        document.querySelectorAll('a[href^="#"]').forEach(function(a){
          var id = a.getAttribute('href').slice(1); try { id = decodeURIComponent(id); } catch (x) {}
          var f = outer(id ? document.getElementById(id) : null);
          if (!f || !f.hasAttribute('data-m-fig') || outer(a) === f) return;
          a.classList.add('m-figref'); a.setAttribute('data-fig-ref', f.getAttribute('data-m-fig'));
          var pm = /\d\s*([A-Za-z]{1,2})\s*$/.exec(a.textContent) || /^\s*([A-Za-z])\s*$/.exec(a.textContent);
          if (pm) a.setAttribute('data-panel', pm[1]);
        });
      }

      function post(box, panel, img){
        var imgs = box ? Array.prototype.slice.call(box.querySelectorAll('img')) : (img ? [img] : []);
        var p = box ? parts(box) : { label: '', title: '', caption: img ? norm(img.getAttribute('alt')) : '' };
        try {
          window.webkit.messageHandlers.fig.postMessage({ srcs: imgs.map(function(x){ return x.src; }), start: Math.max(0, imgs.indexOf(img)),
            label: p.label, title: p.title, caption: p.caption, panel: (panel || '').toUpperCase() });
          return true;
        } catch (x) { return false; }
      }
      window.luntaFigures = { count: figs.length, open: function(i, panel){ return figs[i] ? post(figs[i], panel, null) : false; } };

      // taps on a mention or an image open the figure (not a page turn, highlight or link)
      document.addEventListener('click', function(e){
        var t = e.target; if (!t || !t.closest) return;
        var ref = t.closest('.m-figref'), img = ref ? null : t.closest('img');
        if (!ref && !img) return;
        if (ref) {
          var sel = window.getSelection(); if (sel && !sel.isCollapsed) return;
          var f = figs[+ref.getAttribute('data-fig-ref')]; if (!f || !post(f, ref.getAttribute('data-panel'), null)) return;
        } else if (!post(img.closest('figure, .fig, .table-wrap') && (outer(img) || img.closest('.table-wrap')), '', img)) return;
        e.preventDefault(); e.stopImmediatePropagation();
      }, true);
    })();
    """#
}

/// A figure on its own: the image large (pinch, scroll or double-tap to zoom), its label and
/// full caption below. Black in dark mode.
struct FigureViewer: View {
    let figure: ReaderFigure
    @Environment(\.dismiss) private var dismiss
    @Environment(\.colorScheme) private var scheme
    @State private var index: Int
    @State private var showCaption = true
    @State private var captionHeight: CGFloat = 0

    init(figure: ReaderFigure) {
        self.figure = figure
        _index = State(initialValue: figure.start)
    }

    private var hasCaption: Bool { !figure.caption.isEmpty || !figure.title.isEmpty }
    private var background: Color { scheme == .dark ? .black : Color(red: 1, green: 0.992, blue: 0.973) }   // the reader's paper

    var body: some View {
        GeometryReader { geo in
            VStack(spacing: 0) {
                header
                if figure.images.indices.contains(index) {
                    ZoomableImage(url: figure.images[index], dark: scheme == .dark)
                        .id(figure.images[index])
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                }
                if showCaption, hasCaption {
                    Divider()
                    ScrollView {
                        captionText
                            .frame(maxWidth: 760, alignment: .leading)
                            .frame(maxWidth: .infinity, alignment: .center)
                            .padding(.horizontal, 18).padding(.vertical, 14)
                            .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { captionHeight = $0 }
                    }
                    .scrollBounceBehavior(.basedOnSize)
                    // as tall as the caption, up to 40% of the screen (all of it when there's no image)
                    .frame(height: figure.images.isEmpty ? nil : min(max(captionHeight, 60), geo.size.height * 0.4))
                    .frame(maxHeight: figure.images.isEmpty ? .infinity : nil, alignment: .top)
                }
            }
        }
        .background(background.ignoresSafeArea())
        #if DEBUG
        .task {   // simulator test: -figClose closes the figure again after a while
            guard ProcessInfo.processInfo.arguments.contains("-figClose") else { return }
            try? await Task.sleep(for: .seconds(6)); dismiss()
        }
        #endif
        #if os(macOS)
        .frame(minWidth: 760, idealWidth: 1100, minHeight: 600, idealHeight: 860)
        #endif
    }

    private var header: some View {
        HStack(spacing: 14) {
            VStack(alignment: .leading, spacing: 1) {
                Text(figure.label.isEmpty ? "Figure" : figure.label).font(.headline)
                if !figure.panel.isEmpty { Text("Panel \(figure.panel)").font(.caption).foregroundStyle(.secondary) }
            }
            Spacer()
            if figure.images.count > 1 {
                Button { index = max(0, index - 1) } label: { Image(systemName: "chevron.left") }.disabled(index == 0)
                Text("\(index + 1) of \(figure.images.count)").font(.caption).monospacedDigit().foregroundStyle(.secondary)
                Button { index = min(figure.images.count - 1, index + 1) } label: { Image(systemName: "chevron.right") }
                    .disabled(index == figure.images.count - 1)
            }
            if hasCaption, !figure.images.isEmpty {
                Button { withAnimation(.easeInOut(duration: 0.2)) { showCaption.toggle() } } label: {
                    Image(systemName: showCaption ? "text.below.photo.fill" : "text.below.photo")
                }
                .help(showCaption ? "Hide the caption" : "Show the caption")
            }
            Button { dismiss() } label: { Image(systemName: "xmark.circle.fill").symbolRenderingMode(.hierarchical) }
                .keyboardShortcut(.cancelAction)
                .help("Back to the text")
        }
        .font(.title2)
        .buttonStyle(.plain)
        .foregroundStyle(Color.primary.opacity(0.85))
        .padding(.horizontal, 16).padding(.vertical, 10)
    }

    private var captionText: some View {
        var t = AttributedString()
        if !figure.title.isEmpty {
            var a = AttributedString(figure.title)
            a.font = .body.weight(.semibold)
            t += a
            if !figure.caption.isEmpty { t += AttributedString(" ") }
        }
        t += AttributedString(figure.caption)
        return Text(t)
            .font(.callout)
            .lineSpacing(3)
            .foregroundStyle(scheme == .dark ? Color(red: 0.847, green: 0.835, blue: 0.808) : Color.primary)   // the reader's text
            .textSelection(.enabled)
    }
}

#if DEBUG
extension ReaderController {
    /// Simulator test (-figTest): logs how many figures and mentions the page linked, scrolls to
    /// the first mention and, with -figOpen, opens it as a tap would.
    func figureTest() async {
        let args = ProcessInfo.processInfo.arguments
        guard args.contains("-figTest") else { return }
        for _ in 0..<40 {
            try? await Task.sleep(for: .milliseconds(500))
            if (try? await webView?.evaluateJavaScript("!!window.luntaFigures && !!window.luntaFixOverflow")) as? Bool == true { break }
        }
        print("FIG overflowJS:", (try? await webView?.evaluateJavaScript("typeof window.luntaFixOverflow")) as? String ?? "-")
        let js = """
        (function(){ var r = document.querySelectorAll('.m-figref'), a = []; for (var i = 0; i < r.length && i < 40; i++) a.push(r[i].textContent + '→' + r[i].getAttribute('data-fig-ref') + (r[i].getAttribute('data-panel') || ''));
          return JSON.stringify({ figures: window.luntaFigures ? window.luntaFigures.count : -1, mentions: r.length, sample: a }); })()
        """
        print("FIG", (try? await webView?.evaluateJavaScript(js)) as? String ?? "-"); fflush(stdout)
        if let i = args.firstIndex(of: "-figScroll"), args.indices.contains(i + 1), let n = Int(args[i + 1]) {
            _ = try? await webView?.evaluateJavaScript("var r = document.querySelectorAll('.m-figref')[\(n)]; if (r) r.scrollIntoView({block: 'center'});")
            try? await Task.sleep(for: .seconds(7))   // past the restored place's hold
            if args.contains("-figOpen") {
                func place() async -> String {
                    let o = (try? await webView?.evaluateJavaScript("luntaPlace()")) as? Int ?? -1
                    #if os(iOS)
                    let p = webView?.scrollView.contentOffset ?? .zero
                    return "place=\(o) x=\(Int(p.x)) y=\(Int(p.y))"
                    #else
                    return "place=\(o)"
                    #endif
                }
                print("FIG before open:", await place()); fflush(stdout)
                _ = try? await webView?.evaluateJavaScript("document.querySelectorAll('.m-figref')[\(n)].click()")
                for i in 1...12 { try? await Task.sleep(for: .seconds(1)); print("FIG t\(i):", await place()); fflush(stdout) }
            }
        }
    }
}
#endif

#if os(iOS)
import UIKit

struct ZoomableImage: UIViewRepresentable {
    let url: URL
    var dark = false

    final class ZoomScrollView: UIScrollView {
        var onLayout: (() -> Void)?
        override func layoutSubviews() { super.layoutSubviews(); onLayout?() }
    }

    func makeUIView(context: Context) -> UIScrollView {
        let sv = ZoomScrollView()
        sv.delegate = context.coordinator
        sv.maximumZoomScale = 8
        sv.minimumZoomScale = 1
        sv.showsVerticalScrollIndicator = false
        sv.showsHorizontalScrollIndicator = false
        sv.backgroundColor = .clear
        sv.contentInsetAdjustmentBehavior = .never
        let iv = UIImageView(image: UIImage(contentsOfFile: url.path))
        iv.contentMode = .scaleAspectFit
        iv.backgroundColor = .white          // figures with a transparent background
        iv.alpha = dark ? 0.92 : 1
        sv.addSubview(iv)
        context.coordinator.imageView = iv
        sv.onLayout = { [weak sv, weak coordinator = context.coordinator] in if let sv { coordinator?.layout(sv) } }
        let tap = UITapGestureRecognizer(target: context.coordinator, action: #selector(Coordinator.doubleTap(_:)))
        tap.numberOfTapsRequired = 2
        sv.addGestureRecognizer(tap)
        return sv
    }

    func updateUIView(_ sv: UIScrollView, context: Context) {
        context.coordinator.imageView?.alpha = dark ? 0.92 : 1
    }

    func makeCoordinator() -> Coordinator { Coordinator() }

    final class Coordinator: NSObject, UIScrollViewDelegate {
        var imageView: UIImageView?
        private var laidOutFor: CGSize = .zero

        /// Fits the image to the view (again whenever the view's size changes, e.g. the caption hides).
        func layout(_ sv: UIScrollView) {
            guard let iv = imageView, let img = iv.image, sv.bounds.size != laidOutFor, sv.bounds.width > 0, sv.bounds.height > 0 else { return }
            laidOutFor = sv.bounds.size
            sv.zoomScale = 1
            let s = min(sv.bounds.width / img.size.width, sv.bounds.height / img.size.height)
            iv.frame = CGRect(x: 0, y: 0, width: img.size.width * s, height: img.size.height * s)
            sv.contentSize = iv.frame.size
            center(sv)
        }

        func viewForZooming(in sv: UIScrollView) -> UIView? { imageView }
        func scrollViewDidZoom(_ sv: UIScrollView) { center(sv) }

        private func center(_ sv: UIScrollView) {
            guard let iv = imageView else { return }
            let dx = max(0, (sv.bounds.width - iv.frame.width) / 2)
            let dy = max(0, (sv.bounds.height - iv.frame.height) / 2)
            sv.contentInset = UIEdgeInsets(top: dy, left: dx, bottom: dy, right: dx)
        }

        @objc func doubleTap(_ g: UITapGestureRecognizer) {
            guard let sv = g.view as? UIScrollView, let iv = imageView else { return }
            if sv.zoomScale > 1.01 {
                sv.setZoomScale(1, animated: true)
            } else {
                let p = g.location(in: iv)
                let size = CGSize(width: sv.bounds.width / 3, height: sv.bounds.height / 3)
                sv.zoom(to: CGRect(x: p.x - size.width / 2, y: p.y - size.height / 2, width: size.width, height: size.height), animated: true)
            }
        }
    }
}
#else
import AppKit

struct ZoomableImage: NSViewRepresentable {
    let url: URL
    var dark = false

    /// Keeps the image in the middle while it's smaller than the view.
    final class CenteringClipView: NSClipView {
        override func constrainBoundsRect(_ proposed: NSRect) -> NSRect {
            var r = super.constrainBoundsRect(proposed)
            guard let doc = documentView else { return r }
            if doc.frame.width < r.width { r.origin.x = (doc.frame.width - r.width) / 2 }
            if doc.frame.height < r.height { r.origin.y = (doc.frame.height - r.height) / 2 }
            return r
        }
    }

    final class FitScrollView: NSScrollView {
        var onTile: (() -> Void)?
        override func tile() { super.tile(); onTile?() }
    }

    func makeNSView(context: Context) -> NSScrollView {
        let sv = FitScrollView()
        sv.contentView = CenteringClipView()
        sv.allowsMagnification = true
        sv.maxMagnification = 8
        sv.minMagnification = 0.5
        sv.hasVerticalScroller = true
        sv.hasHorizontalScroller = true
        sv.autohidesScrollers = true
        sv.drawsBackground = false
        let iv = NSImageView()
        iv.image = NSImage(contentsOf: url)
        iv.imageScaling = .scaleProportionallyUpOrDown
        iv.wantsLayer = true
        iv.layer?.backgroundColor = NSColor.white.cgColor
        iv.alphaValue = dark ? 0.92 : 1
        sv.documentView = iv
        let click = NSClickGestureRecognizer(target: context.coordinator, action: #selector(Coordinator.doubleClick(_:)))
        click.numberOfClicksRequired = 2
        sv.addGestureRecognizer(click)
        context.coordinator.scrollView = sv
        sv.onTile = { [weak coordinator = context.coordinator] in coordinator?.fit() }
        return sv
    }

    func updateNSView(_ sv: NSScrollView, context: Context) {
        sv.documentView?.alphaValue = dark ? 0.92 : 1
        DispatchQueue.main.async { context.coordinator.fit() }
    }

    func makeCoordinator() -> Coordinator { Coordinator() }

    final class Coordinator: NSObject {
        weak var scrollView: NSScrollView?
        private var fittedFor: CGSize = .zero

        /// Fits the image to the view (again whenever the window is resized), unless zoomed in.
        func fit() {
            guard let sv = scrollView, let iv = sv.documentView as? NSImageView, let img = iv.image else { return }
            let box = sv.frame.size
            guard box.width > 0, box.height > 0, box != fittedFor, sv.magnification <= 1.01 else { return }
            fittedFor = box
            let s = min(box.width / img.size.width, box.height / img.size.height)
            iv.frame = CGRect(x: 0, y: 0, width: img.size.width * s, height: img.size.height * s)
            sv.magnification = 1
        }

        @objc func doubleClick(_ g: NSClickGestureRecognizer) {
            guard let sv = scrollView else { return }
            let p = g.location(in: sv.documentView)
            sv.setMagnification(sv.magnification > 1.01 ? 1 : 3, centeredAt: p)
            if sv.magnification <= 1.01 { fittedFor = .zero; fit() }
        }
    }
}
#endif
