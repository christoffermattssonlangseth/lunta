import SwiftUI
import WebKit
import ImageIO

// Figures in the reading view. Mentions of a figure in the text ("Figure 2", "Fig. 2B",
// "Figures 3A and 3C", "Extended Data Fig. 4", "Figure S1") and the papers' own links to
// figures are made tappable when the paper has that figure; tapping one (or a figure's image)
// opens the figure on its own: the image large and zoomable, its label and full caption below.
// Closing it leaves the reading view exactly where it was (nothing in the page moves).
//
// In the text, each figure is shown as a compact card (thumbnail, label, title) unless the reader
// setting asks for figures in full; the toolbar's Figures button shows them all as a grid.
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
    /// Highlights in the caption (their text and color).
    var marks: [FigureMark] = []
    /// Its place among the paper's figures (`set`), to go from one to the next in the viewer.
    var index = 0
    var set: [ReaderFigure] = []

    /// One figure as the page describes it.
    init(_ d: [String: Any]) {
        let srcs = (d["srcs"] as? [String]) ?? [d["src"] as? String].compactMap { $0 }
        images = srcs.compactMap(URL.init(string:))
        func s(_ k: String) -> String { (d[k] as? String ?? "").trimmingCharacters(in: .whitespacesAndNewlines) }
        label = s("label"); title = s("title"); caption = s("caption"); panel = s("panel")
        start = min(max(0, d["start"] as? Int ?? 0), max(0, images.count - 1))
        index = d["index"] as? Int ?? 0
        marks = (d["marks"] as? [[String: Any]] ?? []).compactMap { m in
            (m["t"] as? String).map { FigureMark(text: $0, color: m["c"] as? String ?? "yellow") }
        }
    }

    /// A message from the page: one figure, or figure `index` of `all` (then `set` holds them all).
    static func from(_ d: [String: Any]) -> ReaderFigure? {
        if let all = d["all"] as? [[String: Any]], let i = d["index"] as? Int, all.indices.contains(i) {
            let set = all.map(ReaderFigure.init)
            var f = set[i]
            f.set = set
            f.start = min(max(0, d["start"] as? Int ?? 0), max(0, f.images.count - 1))
            f.panel = (d["panel"] as? String ?? "").trimmingCharacters(in: .whitespaces)
            return f
        }
        let f = ReaderFigure(d)
        return f.images.isEmpty && f.caption.isEmpty && f.title.isEmpty ? nil : f
    }

    static func == (a: ReaderFigure, b: ReaderFigure) -> Bool { a.id == b.id }
}

struct FigureMark: Equatable {
    let text: String
    let color: String
}

/// Receives "fig" messages from the page: a tapped figure mention, card or image, or (as the page
/// loads) the list of its figures.
final class FigureMessageHandler: NSObject, WKScriptMessageHandler {
    weak var controller: ReaderController?
    func userContentController(_ c: WKUserContentController, didReceive m: WKScriptMessage) {
        guard let d = m.body as? [String: Any] else { return }
        if let list = d["list"] as? [[String: Any]] {
            let figs = list.map(ReaderFigure.init)
            Task { @MainActor in self.controller?.figures = figs }
            return
        }
        guard let f = ReaderFigure.from(d) else { return }
        Task { @MainActor in self.controller?.onFigure?(f) }
    }
}

extension ReaderController {
    /// Figure `i` as the page shows it now (with the highlights in its caption), and all the others.
    func figure(at i: Int) async -> ReaderFigure? {
        guard let json = (try? await webView?.evaluateJavaScript("window.luntaFigures ? luntaFigures.data(\(i)) : ''")) as? String,
              let data = json.data(using: .utf8),
              let d = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] else { return nil }
        return ReaderFigure.from(d)
    }

    func setFigureCards(_ on: Bool) {
        webView?.evaluateJavaScript("window.luntaFigureCards && luntaFigureCards(\(on))")
    }
}

/// Figures in the reading text: as cards (tap to open) or in full.
enum FigureSetting {
    static let key = "readerFigureCards"
    static var cards: Bool { UserDefaults.standard.object(forKey: key) as? Bool ?? true }
}

extension ReaderAssets {
    /// The figure setting, for the page to start with.
    static var figureSettingJS: String { "window.__luntaFigureCards = \(FigureSetting.cards);" }

    /// Links figure mentions to their figures and sends taps on them (and on images) to the app.
    /// Part of `styleScript`, so its click handler runs before the highlight and page-turn ones.
    static let figureJS = #"""
    (function(){
      if (!document.body || window.luntaFigures) return;
      var st = document.createElement('style');
      st.textContent = '.m-figref { color: #2a6478; cursor: pointer; text-decoration: underline; text-decoration-color: rgba(42,100,120,.35);' +
        ' text-decoration-thickness: 1px; text-underline-offset: 2px; -webkit-tap-highlight-color: rgba(42,100,120,.18); border-radius: 2px; }' +
        '@media (prefers-color-scheme: dark) { .m-figref { color: #9ec9d8; text-decoration-color: rgba(158,201,216,.35); -webkit-tap-highlight-color: rgba(158,201,216,.2); } }';
      // figures as cards: one row (thumbnail, label, title) instead of the image and the caption. The
      // figure's own content stays in the page, only collapsed (the caption's text at size 0), so the
      // page text, the character offsets of highlights and reading places, is unchanged. The card's
      // words are drawn from attributes (CSS content), so they add no text to the page.
      st.textContent +=
        'html.m-cards figure.m-carded { margin: 1em 0 !important; padding: 0 !important; border: 0 !important; background: none !important; text-align: left !important; break-inside: avoid; }' +
        'html.m-cards figure.m-carded > :not(.m-figcard) { height: 0 !important; min-height: 0 !important; max-height: 0 !important; overflow: hidden !important;' +
        ' margin: 0 !important; padding: 0 !important; border: 0 !important; font-size: 0 !important; line-height: 0 !important; }' +
        'html.m-cards figure.m-carded > :not(.m-figcard) * { font-size: 0 !important; line-height: 0 !important; }' +
        'html.m-cards figure.m-carded img, html.m-cards figure.m-carded svg { display: none !important; }' +
        'html:not(.m-cards) .m-figcard { display: none !important; }' +
        '.m-figcard { display: flex; align-items: center; gap: 12px; padding: 8px 10px 8px 8px; border: 1px solid rgba(127,127,127,.28); border-radius: 10px;' +
        ' background: rgba(127,127,127,.06); cursor: pointer; -webkit-user-select: none; user-select: none; break-inside: avoid; -webkit-touch-callout: none;' +
        ' font-family: -apple-system, "Helvetica Neue", sans-serif; -webkit-tap-highlight-color: rgba(42,100,120,.15); }' +
        '.m-fc-thumb { flex: 0 0 60px; width: 60px; height: 60px; border-radius: 6px; background: #fff center / contain no-repeat; border: 1px solid rgba(127,127,127,.18); box-sizing: border-box; }' +
        '.m-fc-none { background: rgba(127,127,127,.12); }' +
        '.m-fc-text { flex: 1 1 auto; min-width: 0; }' +
        '.m-fc-label { font-size: .8rem; font-weight: 600; line-height: 1.3; color: #2a6478; }' +
        '.m-fc-title { margin-top: 2px; font-size: .8rem; line-height: 1.35; color: rgba(29,29,31,.72); display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; }' +
        '.m-fc-label::before, .m-fc-title::before { content: attr(data-t); }' +
        '.m-figcard::after { content: "›"; flex: 0 0 auto; font-size: 1.4rem; line-height: 1; color: rgba(127,127,127,.6); padding-left: 2px; }' +
        '@media (prefers-color-scheme: dark) { .m-figcard { background: #0f0f0f; border-color: #2b2b2b; } .m-fc-label { color: #9ec9d8; }' +
        ' .m-fc-title { color: rgba(216,213,206,.68); } .m-fc-thumb { opacity: .9; border-color: #333; } }';
      document.documentElement.appendChild(st);

      var PRE = '(Extended\\s+Data\\s+|Supplementa(?:ry|l)\\s+|Suppl?\\.\\s*)?';
      // a figure's number: "2", "S1", a book's chapter-numbered "1.2", "10.14", "S1.2", "3-4" and an appendix's
      // "A.1" (a dot only when a digit follows it: "Figure 1. Overview" is figure 1)
      var NUM = '(?:S?\\d+(?:[.\\-–]\\d+)*|[A-Z](?:[.\\-–]\\d+)+)(?![.\\-–]\\d)';
      // in the text a dash between numbers is a range ("Figs. 3–6") unless the book has a figure of that
      // number ("Figure 3-4", see joined()): mentions read dotted numbers only
      var REF = '(?:S?\\d+(?:\\.\\d+)*|[A-Z](?:[.\\-–]\\d+)+)(?!\\.\\d)';
      var LABEL = new RegExp('^\\s*' + PRE + '(?:Figure|Fig)s?\\.?\\s*(' + NUM + ')', 'i');
      var ANY_LABEL = new RegExp('^\\s*' + PRE + '(?:Figure|Fig|Table)s?\\.?\\s*' + NUM + '[A-Za-z]?(?![A-Za-z0-9])', 'i');
      var MENTION = new RegExp('(?<![A-Za-z])' + PRE + '(Figures|Figs\\.?|Figure|Fig\\.?)\\s*(' + REF + ')(?:([A-Za-z])(?![A-Za-z]))?(?![0-9])', 'gi');
      var MORE = new RegExp('^(\\s*(?:,|;|and|&|or|to|–|—|-)\\s*)(' + REF + ')(?:([A-Za-z])(?![A-Za-z]))?(?![0-9])', 'i');
      var HYPH = /^[-–](\d+)/;
      var CUT = /^[\s|.:—–-]+/;
      function norm(s){ return (s || '').replace(/\s+/g, ' ').trim(); }
      // "Extended Data Fig. 4" -> E4, "Figure S1" and "Supplementary Figure 1" -> S1, "Fig. 2" -> 2,
      // "Figure 1.2" and "Figure 1-2" -> 1.2
      function key(pre, num){
        num = num.toUpperCase().replace(/[-–]/g, '.'); pre = (pre || '').toLowerCase();
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

      // tables stay in the text as they are (LaTeXML puts them in <figure class="ltx_table">)
      function isTable(f){
        if (f.classList.contains('ltx_table')) return true;
        if (f.querySelector('table') && !f.querySelector('img')) return true;
        var cap = captionOf(f);
        return !!cap && /^\s*(Supplementa(ry|l)\s+)?Table\s/i.test(cap.textContent || '');
      }

      // every figure in the book, and the label each is known by
      var figs = [], byKey = {};
      document.querySelectorAll('figure, .fig').forEach(function(f){
        if (outer(f) !== f || isTable(f)) return;
        var i = figs.length; figs.push(f); f.setAttribute('data-m-fig', i);
        var cap = captionOf(f), img = f.querySelector('img');
        var m = LABEL.exec(norm(cap ? cap.textContent : '')) || (img ? LABEL.exec(norm(img.getAttribute('alt'))) : null);
        if (m) { var k = key(m[1], m[2]); if (!(k in byKey)) byKey[k] = i; }
      });

      // label, title and caption text of a figure (or table)
      function parts(box){
        var cap = captionOf(box), img = box.querySelector('img');
        var marks = [];
        if (cap) cap.querySelectorAll('mark.m-hl').forEach(function(x){
          var c = /m-(yellow|green|blue|pink)/.exec(x.className);
          var t = norm(x.textContent); if (t) marks.push({ t: t, c: c ? c[1] : 'yellow' });
        });
        if (cap) {   // reference numbers ("variability<sup>14</sup>") would run into the words
          cap = cap.cloneNode(true);
          cap.querySelectorAll('sup').forEach(function(x){ if (x.querySelector('a') || x.closest('a')) x.remove(); });
          cap.querySelectorAll('a[href^="#"]').forEach(function(x){ if (x.closest('sup')) x.remove(); });
        }
        if (!cap) return { label: fallback(box, ''), title: '', caption: img ? norm(img.getAttribute('alt')) : '', marks: [] };
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
        return { label: label.replace(/[\s|.:—–-]+$/, '') || fallback(box, full), title: title, caption: rest, marks: marks };
      }
      // a figure without a label: a graphical abstract, or just "Figure"
      function fallback(box, text){
        var h = box.previousElementSibling, near = (h && /^h[1-6]$/i.test(h.nodeName) ? h.textContent : '') + ' ' + text + ' ' + (box.id || '');
        var img = box.querySelector('img'); if (img) near += ' ' + (img.getAttribute('alt') || '') + ' ' + (img.getAttribute('src') || '');
        return /graphical[\s_-]*abstract|\bga\d*\b/i.test(near) ? 'Graphical abstract' : 'Figure';
      }

      // a book's hyphenated number ("Figure 3-4"), when the book has that figure; otherwise the dash is a
      // range ("Figs. 3–6") and the number ends before it
      function joined(pre, s, r){
        var h;
        while (!r.panel && (h = HYPH.exec(s.slice(r.end))) && (key(pre, r.num + '.' + h[1]) in byKey)) { r.num += '.' + h[1]; r.end += h[0].length; }
        return r;
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
            var refs = [joined(m[1], s, { at: m.index, end: m.index + m[0].length, num: m[3], panel: m[4] })];
            MENTION.lastIndex = refs[0].end;
            if (/^fig(ure)?s/i.test(m[2])) {
              var pos = refs[0].end, mm;
              while ((mm = MORE.exec(s.slice(pos)))) {
                var rr = joined(m[1], s, { at: pos + mm[1].length, end: pos + mm[0].length, num: mm[2], panel: mm[3] });
                refs.push(rr); pos = rr.end;
              }
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

      // what the app needs to show figure i (its images, label, title, caption, highlights in the caption)
      function data(i){
        var f = figs[i], p = parts(f);
        var srcs = Array.prototype.slice.call(f.querySelectorAll('img')).map(function(x){ return x.src; });
        return { index: i, srcs: srcs, label: p.label, title: p.title, caption: p.caption, marks: p.marks };
      }
      function all(){ return figs.map(function(_, i){ return data(i); }); }
      function send(m){ try { window.webkit.messageHandlers.fig.postMessage(m); return true; } catch (x) { return false; } }
      // figure i, with all the others (to go from one to the next in the viewer)
      function openFig(i, panel, img){
        var start = img ? Math.max(0, Array.prototype.slice.call(figs[i].querySelectorAll('img')).indexOf(img)) : 0;
        return send({ all: all(), index: i, start: start, panel: (panel || '').toUpperCase() });
      }
      window.luntaFigures = {
        count: figs.length,
        open: function(i, panel){ return figs[i] ? openFig(i, panel, null) : false; },
        data: function(i){ return JSON.stringify({ all: all(), index: i }); }
      };

      // the cards
      document.documentElement.classList.toggle('m-cards', window.__luntaFigureCards !== false);
      figs.forEach(function(f, i){
        var d = data(i), card = document.createElement('div');
        card.className = 'm-figcard'; card.setAttribute('data-fig-card', i);
        var th = document.createElement('div');
        th.className = 'm-fc-thumb' + (d.srcs.length ? '' : ' m-fc-none');
        if (d.srcs.length) th.style.backgroundImage = 'url(' + JSON.stringify(d.srcs[0]) + ')';
        var tx = document.createElement('div'); tx.className = 'm-fc-text';
        var lb = document.createElement('div'); lb.className = 'm-fc-label'; lb.setAttribute('data-t', d.label);
        tx.appendChild(lb);
        var title = d.title || d.caption.split(/(?<=[a-z0-9)\]][.;:])\s/)[0];
        if (title.length > 220) title = title.slice(0, 220) + '…';
        if (title) { var tt = document.createElement('div'); tt.className = 'm-fc-title'; tt.setAttribute('data-t', title); tx.appendChild(tt); }
        card.appendChild(th); card.appendChild(tx);
        f.insertBefore(card, f.firstChild);
        f.classList.add('m-carded');
      });
      // the app's setting: cards or figures in full, keeping the place being read
      window.luntaFigureCards = function(on){
        var root = document.documentElement; on = !!on;
        window.__luntaFigureCards = on;
        if (root.classList.contains('m-cards') === on) return;
        var paged = window.luntaPaged && window.luntaPaged();
        var off = !paged && window.luntaReadingOffset ? window.luntaReadingOffset() : null;
        var at = paged && window.luntaPlace ? window.luntaPlace() : null;
        root.classList.toggle('m-cards', on);
        if (window.luntaFixOverflow) window.luntaFixOverflow();
        if (paged) {   // to the page holding the place, and keep that place (not the page's first word)
          if (window.luntaTargetPage) window.luntaTargetPage();
          if (at != null && window.luntaKeepPlace) window.luntaKeepPlace(at);
        }
        else if (off != null && off > 0 && window.luntaScrollToOffset) window.luntaScrollToOffset(off);
      };
      // anything in a collapsed figure (a highlight or the narration in its caption) is shown by its card
      // (after the other scripts, so page turning's own version is the one wrapped)
      setTimeout(function(){
        var prev = Element.prototype.scrollIntoView;
        Element.prototype.scrollIntoView = function(o){
          var f = document.documentElement.classList.contains('m-cards') && this.closest ? this.closest('figure.m-carded') : null;
          var card = f ? f.querySelector('.m-figcard') : null;
          return prev.call(card && card !== this ? card : this, o);
        };
      }, 0);
      if (figs.length) send({ list: all() });

      // taps on a mention, a card or an image open the figure (not a page turn, highlight or link)
      document.addEventListener('click', function(e){
        var t = e.target; if (!t || !t.closest) return;
        var ref = t.closest('.m-figref'), card = ref ? null : t.closest('.m-figcard'), img = ref || card ? null : t.closest('img');
        if (!ref && !card && !img) return;
        var sel = window.getSelection();
        if (ref) {
          if (sel && !sel.isCollapsed) return;
          var i = +ref.getAttribute('data-fig-ref'); if (!figs[i] || !openFig(i, ref.getAttribute('data-panel'), null)) return;
        } else if (card) {
          if (!openFig(+card.getAttribute('data-fig-card'), '', null)) return;
        } else {
          var f = outer(img), k = f ? figs.indexOf(f) : -1;
          if (k >= 0) { if (!openFig(k, '', img)) return; }
          else {
            var box = img.closest('.table-wrap'), p = box ? parts(box) : null;
            if (!send({ srcs: [img.src], label: p ? p.label : '', title: p ? p.title : '', caption: p ? p.caption : norm(img.getAttribute('alt')) })) return;
          }
        }
        e.preventDefault(); e.stopImmediatePropagation();
      }, true);
    })();
    """#
}

/// A figure on its own: the image large (pinch, scroll or double-tap to zoom), its label and full
/// caption below; swipe (iPhone) or use the arrow keys (Mac) for the paper's other figures.
/// Black in dark mode.
struct FigureViewer: View {
    let figure: ReaderFigure
    @Environment(\.dismiss) private var dismiss
    @Environment(\.colorScheme) private var scheme
    @State private var current: Int
    @State private var showCaption = true
    /// Shown as a card over the text (`figureOverlay`): closing goes through this instead of dismiss.
    var onClose: (() -> Void)? = nil

    init(figure: ReaderFigure, onClose: (() -> Void)? = nil) {
        self.figure = figure
        self.onClose = onClose
        _current = State(initialValue: figure.set.isEmpty ? 0 : figure.index)
    }

    private func close() { if let onClose { onClose() } else { dismiss() } }

    private var figures: [ReaderFigure] { figure.set.isEmpty ? [figure] : figure.set }
    private var shown: ReaderFigure { figures[min(current, figures.count - 1)] }
    private var background: Color { scheme == .dark ? .black : Color(red: 1, green: 0.992, blue: 0.973) }   // the reader's paper

    var body: some View {
        VStack(spacing: 0) {
            header
            pages
        }
        .background(background.ignoresSafeArea())
        #if DEBUG
        .task {   // simulator test: -figNext goes to the next figure, -figClose closes the viewer
            let args = ProcessInfo.processInfo.arguments
            if args.contains("-figNext") { try? await Task.sleep(for: .seconds(3)); withAnimation { current = min(figures.count - 1, current + 1) } }
            guard args.contains("-figClose") else { return }
            try? await Task.sleep(for: .seconds(4)); close()
        }
        #endif
        #if os(macOS)
        .frame(minWidth: onClose == nil ? 760 : nil, idealWidth: onClose == nil ? 1100 : nil,
               minHeight: onClose == nil ? 600 : nil, idealHeight: onClose == nil ? 860 : nil)
        #endif
    }

    @ViewBuilder private var pages: some View {
        #if os(iOS)
        TabView(selection: $current) {
            ForEach(figures.indices, id: \.self) { i in
                FigurePage(figure: figures[i], start: figures[i].id == figure.id ? figure.start : 0, showCaption: showCaption, dark: scheme == .dark)
                    .tag(i)
            }
        }
        .tabViewStyle(.page(indexDisplayMode: .never))
        #else
        FigurePage(figure: shown, start: shown.id == figure.id ? figure.start : 0, showCaption: showCaption, dark: scheme == .dark)
            .id(current)
        #endif
    }

    private var header: some View {
        HStack(spacing: 14) {
            VStack(alignment: .leading, spacing: 1) {
                Text(shown.label.isEmpty ? "Figure" : shown.label).font(.headline).lineLimit(1)
                let panel = shown.id == figure.id ? figure.panel : ""
                if !panel.isEmpty || figures.count > 1 {
                    Text([panel.isEmpty ? nil : "Panel \(panel)", figures.count > 1 ? "\(current + 1) of \(figures.count)" : nil]
                        .compactMap { $0 }.joined(separator: " · "))
                        .font(.caption).monospacedDigit().foregroundStyle(.secondary)
                }
            }
            Spacer()
            #if os(macOS)
            if figures.count > 1 {
                Button { current = max(0, current - 1) } label: { Image(systemName: "chevron.left") }
                    .disabled(current == 0).keyboardShortcut(.leftArrow, modifiers: []).help("Previous figure (←)")
                Button { current = min(figures.count - 1, current + 1) } label: { Image(systemName: "chevron.right") }
                    .disabled(current == figures.count - 1).keyboardShortcut(.rightArrow, modifiers: []).help("Next figure (→)")
            }
            #endif
            if figures.contains(where: { !$0.caption.isEmpty || !$0.title.isEmpty }) {
                Button { withAnimation(.easeInOut(duration: 0.2)) { showCaption.toggle() } } label: {
                    Image(systemName: showCaption ? "text.below.photo.fill" : "text.below.photo")
                }
                .help(showCaption ? "Hide the caption" : "Show the caption")
            }
            Button { close() } label: { Image(systemName: "xmark.circle.fill").symbolRenderingMode(.hierarchical) }
                .keyboardShortcut(.cancelAction)
                .help("Back to the text")
        }
        .font(.title2)
        .buttonStyle(.plain)
        .foregroundStyle(Color.primary.opacity(0.85))
        .padding(.horizontal, 16).padding(.vertical, 10)
    }
}

/// One figure in the viewer: its image(s) and caption.
private struct FigurePage: View {
    let figure: ReaderFigure
    let showCaption: Bool
    let dark: Bool
    @State private var image: Int
    @State private var captionHeight: CGFloat = 0

    init(figure: ReaderFigure, start: Int, showCaption: Bool, dark: Bool) {
        self.figure = figure; self.showCaption = showCaption; self.dark = dark
        _image = State(initialValue: start)
    }

    private var hasCaption: Bool { !figure.caption.isEmpty || !figure.title.isEmpty }

    var body: some View {
        GeometryReader { geo in
            VStack(spacing: 0) {
                if figure.images.indices.contains(image) {
                    ZoomableImage(url: figure.images[image], dark: dark)
                        .id(figure.images[image])
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                        .overlay(alignment: .bottom) {
                            if figure.images.count > 1 {   // a figure in several images
                                HStack(spacing: 14) {
                                    Button { image = max(0, image - 1) } label: { Image(systemName: "chevron.left") }.disabled(image == 0)
                                    Text("Image \(image + 1) of \(figure.images.count)").font(.caption).monospacedDigit()
                                    Button { image = min(figure.images.count - 1, image + 1) } label: { Image(systemName: "chevron.right") }
                                        .disabled(image == figure.images.count - 1)
                                }
                                .buttonStyle(.plain)
                                .padding(.horizontal, 12).padding(.vertical, 6)
                                .background(.regularMaterial, in: Capsule())
                                .padding(8)
                            }
                        }
                } else if figure.images.isEmpty && !hasCaption {
                    Spacer()
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
        // the reader's highlights in the caption
        for m in figure.marks where m.text.count > 1 {
            let color = (HighlightColor(rawValue: m.color) ?? .yellow).color.opacity(dark ? 0.35 : 0.45)
            var from = t.startIndex
            while from < t.endIndex, let r = t[from...].range(of: m.text) {
                t[r].backgroundColor = color
                from = r.upperBound
            }
        }
        return Text(t)
            .font(.callout)
            .lineSpacing(3)
            .foregroundStyle(dark ? Color(red: 0.847, green: 0.835, blue: 0.808) : Color.primary)   // the reader's text
            .textSelection(.enabled)
    }
}

/// All the paper's figures, as a grid of thumbnails; tap one to open it.
struct FigureGrid: View {
    @ObservedObject var controller: ReaderController
    @Environment(\.dismiss) private var dismiss
    @State private var viewing: ReaderFigure?

    var body: some View {
        NavigationStack {
            ScrollView {
                LazyVGrid(columns: [GridItem(.adaptive(minimum: 150), spacing: 14)], spacing: 18) {
                    ForEach(Array(controller.figures.enumerated()), id: \.offset) { i, f in
                        Button { open(i) } label: { cell(f) }
                            .buttonStyle(.plain)
                    }
                }
                .padding()
            }
            .navigationTitle("Figures")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } }
                #if os(macOS)
                ToolbarItem(placement: .automatic) { FigureSettingPicker() }   // (on the iPhone it's in the reader's menu)
                #endif
            }
        }
        #if os(iOS)
        .fullScreenCover(item: $viewing) { FigureViewer(figure: $0) }
        #else
        .sheet(item: $viewing) { FigureViewer(figure: $0) }
        .frame(minWidth: 620, idealWidth: 760, minHeight: 480, idealHeight: 640)
        #endif
        #if DEBUG
        .task {   // simulator test: -figGridOpen <n> opens figure n from the grid
            let args = ProcessInfo.processInfo.arguments
            guard let k = args.firstIndex(of: "-figGridOpen"), args.indices.contains(k + 1), let n = Int(args[k + 1]) else { return }
            try? await Task.sleep(for: .seconds(3)); open(n)
        }
        #endif
    }

    private func cell(_ f: ReaderFigure) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            FigureThumbnail(url: f.images.first)
                .frame(height: 130).frame(maxWidth: .infinity)
                .background(Color.white)
                .clipShape(RoundedRectangle(cornerRadius: 8))
                .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(Color.secondary.opacity(0.25)))
            Text(f.label.isEmpty ? "Figure" : f.label).font(.subheadline.weight(.semibold)).lineLimit(1)
            if !f.title.isEmpty || !f.caption.isEmpty {
                Text(f.title.isEmpty ? f.caption : f.title).font(.caption).foregroundStyle(.secondary).lineLimit(2)
            }
        }
        .contentShape(Rectangle())
    }

    private func open(_ i: Int) {
        Task {
            if let f = await controller.figure(at: i) { viewing = f; return }
            guard controller.figures.indices.contains(i) else { return }
            var f = controller.figures[i]
            f.set = controller.figures
            viewing = f
        }
    }
}

/// A small copy of a figure image, made off the main thread.
private struct FigureThumbnail: View {
    let url: URL?
    @State private var image: CGImage?

    var body: some View {
        ZStack {
            if let image { Image(decorative: image, scale: 1).resizable().scaledToFit().padding(4) }
            else if url == nil { Image(systemName: "photo").font(.title).foregroundStyle(.gray) }
        }
        .task(id: url) {
            guard let url else { return }
            image = await Task.detached(priority: .userInitiated) { () -> CGImage? in
                guard let src = CGImageSourceCreateWithURL(url as CFURL, nil) else { return nil }
                let opts: [CFString: Any] = [kCGImageSourceCreateThumbnailFromImageAlways: true,
                                             kCGImageSourceThumbnailMaxPixelSize: 480,
                                             kCGImageSourceCreateThumbnailWithTransform: true]
                return CGImageSourceCreateThumbnailAtIndex(src, 0, opts as CFDictionary)
            }.value
        }
    }
}

/// The reader setting: figures in the text as cards (default) or in full.
struct FigureSettingPicker: View {
    @AppStorage(FigureSetting.key) private var cards = true

    var body: some View {
        Picker(selection: $cards) {
            Label("As Cards", systemImage: "rectangle.grid.1x2").tag(true)
            Label("In Full", systemImage: "photo").tag(false)
        } label: {
            Label("Figures in the Text", systemImage: "photo.on.rectangle")
        }
        .pickerStyle(.menu)
        .help("Show figures in the text as cards or in full")
    }
}

/// The reader's figure support: the grid of figures, and the page following the figure setting.
private struct ReaderFigures: ViewModifier {
    let controller: ReaderController
    @Binding var browsing: Bool
    @AppStorage(FigureSetting.key) private var cards = true

    func body(content: Content) -> some View {
        content
            .sheet(isPresented: $browsing) { FigureGrid(controller: controller) }
            .onChange(of: cards) { _, on in controller.setFigureCards(on) }
            #if DEBUG
            .task {   // simulator test: -figGrid opens the grid of figures
                guard ProcessInfo.processInfo.arguments.contains("-figGrid") else { return }
                try? await Task.sleep(for: .seconds(9)); browsing = true
            }
            #endif
    }
}

extension View {
    /// Adds the figure grid (shown while `browsing`) and applies the figure setting to the page.
    func readerFigures(_ controller: ReaderController, browsing: Binding<Bool>) -> some View {
        modifier(ReaderFigures(controller: controller, browsing: browsing))
    }
}

/// The toolbar button for the figure grid (only when the paper has figures).
struct FiguresButton: View {
    @ObservedObject var controller: ReaderController
    let action: () -> Void
    var body: some View {
        if !controller.figures.isEmpty {
            Button(action: action) { Image(systemName: "photo.on.rectangle.angled") }
                .help("Figures (\(controller.figures.count))")
                .accessibilityLabel("Figures")
        }
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
          return JSON.stringify({ figures: window.luntaFigures ? window.luntaFigures.count : -1, cards: document.querySelectorAll('.m-figcard').length, cardsOn: document.documentElement.classList.contains('m-cards'), labels: Array.prototype.map.call(document.querySelectorAll('.m-fc-label'), function(x){ return x.getAttribute('data-t'); }), mentions: r.length, sample: a.slice(0, 8) }); })()
        """
        print("FIG", (try? await webView?.evaluateJavaScript(js)) as? String ?? "-"); fflush(stdout)
        if let i = args.firstIndex(of: "-figScroll"), args.indices.contains(i + 1), let n = Int(args[i + 1]) {
            let sel = args.contains("-figCard") ? ".m-figcard" : ".m-figref"   // -figCard: the n-th card instead of mention
            try? await Task.sleep(for: .seconds(6))   // past the restored place's hold
            _ = try? await webView?.evaluateJavaScript("window.luntaHold && luntaHold(); var r = document.querySelectorAll('\(sel)')[\(n)]; if (r) r.scrollIntoView({block: 'center'});")
            try? await Task.sleep(for: .seconds(2))
            print("FIG scrolled"); fflush(stdout)
            try? await Task.sleep(for: .seconds(2))
            if args.contains("-figToggle") {   // the setting changed while reading: the place stays
                for on in [true, false, true] {
                    let before = (try? await webView?.evaluateJavaScript("luntaPlace()")) as? Int ?? -1
                    setFigureCards(on)
                    try? await Task.sleep(for: .seconds(2))
                    let after = (try? await webView?.evaluateJavaScript("luntaPlace()")) as? Int ?? -1
                    print("FIG toggle cards=\(on): place \(before) -> \(after)"); fflush(stdout)
                }
            }
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
                _ = try? await webView?.evaluateJavaScript("document.querySelectorAll('\(sel)')[\(n)].click()")
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

/// The figure viewer as a card over the reading text: the text stays visible (dimmed) around it,
/// and tapping or clicking it closes the figure, as does the close button or Esc.
private struct FigureOverlay: ViewModifier {
    @Binding var figure: ReaderFigure?

    func body(content: Content) -> some View {
        content
            .overlay {
                if let f = figure {
                    ZStack {
                        Color.black.opacity(0.42)
                            .ignoresSafeArea()
                            .contentShape(Rectangle())
                            .onTapGesture { figure = nil }
                            .accessibilityLabel("Back to the text")
                            .accessibilityAddTraits(.isButton)
                        FigureViewer(figure: f, onClose: { figure = nil })
                            .clipShape(RoundedRectangle(cornerRadius: 16, style: .continuous))
                            .shadow(color: .black.opacity(0.35), radius: 24, y: 8)
                            #if os(iOS)
                            .padding(.horizontal, 8)
                            .padding(.top, 44)       // a strip of text stays visible above …
                            .padding(.bottom, 56)    // … and below: tap it to go back
                            #else
                            .padding(36)
                            #endif
                    }
                    .transition(.opacity.combined(with: .scale(scale: 0.97)))
                }
            }
            .animation(.easeOut(duration: 0.18), value: figure?.id)
    }
}

extension View {
    /// Shows `figure` as a card over this view (see `FigureOverlay`).
    func figureOverlay(_ figure: Binding<ReaderFigure?>) -> some View { modifier(FigureOverlay(figure: figure)) }
}
