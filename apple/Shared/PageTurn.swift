import Foundation
import WebKit
#if os(iOS)
import UIKit.UIGestureRecognizerSubclass
#endif

// Page turning: the reading view laid out as pages instead of one long scroll. The page is cut
// into screen-sized CSS columns and moved sideways one screen at a time (two columns side by side
// on a wide window). On the iPhone the web view's own scroll view does the paging, so the page
// follows your finger; tap the left or right edge to turn, the middle to show or hide the bars.
// On the Mac: arrow keys, space, or a swipe / scroll.
//
// Everything that used to scroll (contents, narration, highlights, the saved position) goes
// through scrollIntoView or luntaScrollToOffset, which turn to the right page in this mode.

extension ReaderAssets {
    static let pagedJS = #"""
    (function(){
      var on = false, top = 24, bottom = 34, native = false, W = 0, Hh = 0, anchor = null, settle = null, busy = false, checks = [], heldPage = -1, target = 0;
      var style = document.createElement('style'); document.documentElement.appendChild(style);
      var label = document.createElement('div'); label.id = 'm-page-no';
      // the page number opens Go to Page (JumpBack.swift)
      label.title = 'Go to Page';
      label.addEventListener('click', function(e){ e.preventDefault(); e.stopPropagation(); if (window.luntaShowGoTo) window.luntaShowGoTo(); });
      function post(m){ try { window.webkit.messageHandlers.page.postMessage(m); } catch (x) {} }

      window.luntaPaged = function(){ return on; };
      // the exact viewport size: with larger text it isn't a whole number of pixels, and
      // innerWidth's rounding would add up to a visible shift after a few pages
      function vw(){ return window.visualViewport ? window.visualViewport.width * window.visualViewport.scale : window.innerWidth; }
      function vh(){ return window.visualViewport ? window.visualViewport.height * window.visualViewport.scale : window.innerHeight; }
      var pad = 20;
      // where the first line of the page starts (to read the exact position)
      window.luntaPageOrigin = function(){ return on ? { x: pad + 3, y: top + 6 } : null; };
      function cols(){ return vw() >= 860 ? 2 : 1; }
      function page(){ return W ? Math.round(window.scrollX / W) : 0; }
      function pages(){ return W ? Math.max(1, Math.round(document.documentElement.scrollWidth / W)) : 1; }

      function layout(){
        if (!on) { style.textContent = ''; document.documentElement.classList.remove('m-paged'); label.remove(); return; }
        W = vw(); Hh = vh();
        var H = Hh, n = cols();
        var minPad = n > 1 ? 44 : (W < 500 ? 20 : 36);
        var colW = Math.min(W / n - 2 * minPad, 640);
        pad = (W / n - colW) / 2;
        var inner = H - top - bottom;
        style.textContent =
          // iPhone: the scroll view pages natively, so the root must stay scrollable (iOS stops
          // scrolling an axis the root hides); Mac: scripts turn the pages, nothing scrolls by hand
          'html.m-paged { height: ' + H + 'px; scroll-behavior: auto !important; max-width: none !important;' +
          (native ? ' overflow: visible !important; }' : ' overflow: hidden !important; }') +
          'html.m-paged body { margin: 0 !important; max-width: none !important; width: auto !important; height: ' + H + 'px; box-sizing: border-box;' +
          ' padding: ' + top + 'px ' + pad + 'px ' + bottom + 'px !important; overflow: visible !important;' +
          ' column-width: ' + colW + 'px; column-gap: ' + (2 * pad) + 'px; column-fill: auto; }' +
          'html.m-paged img, html.m-paged svg, html.m-paged video { max-height: ' + (inner - 10) + 'px; width: auto; object-fit: contain; }' +
          'html.m-paged figure, html.m-paged .m-scroll, html.m-paged pre, html.m-paged math[display=block] { break-inside: avoid; }' +
          'html.m-paged .m-scroll { max-height: ' + inner + 'px; overflow: auto; }' +
          'html.m-paged h1, html.m-paged h2, html.m-paged h3, html.m-paged h4 { break-after: avoid; }' +
          'html.m-paged h1 { margin-top: 0; break-before: column; }' +
          'html.m-paged .titlepage h1, html.m-paged .titlepage + * h1:first-child { break-before: auto; }' +
          '#m-page-no { position: fixed; left: 50%; transform: translateX(-50%); bottom: ' + Math.max(2, bottom / 2 - 12) + 'px; text-align: center;' +
          ' padding: 4px 12px; white-space: nowrap; cursor: pointer; -webkit-user-select: none; user-select: none; -webkit-tap-highlight-color: transparent;' +
          ' font: 12px -apple-system, sans-serif; color: rgba(127,127,127,.85); z-index: 10; }';
        document.documentElement.classList.add('m-paged');
        if (!label.parentNode) document.documentElement.appendChild(label);
        if (window.luntaFixOverflow) window.luntaFixOverflow();
      }

      function show(){
        if (!on) return;
        var p = page(), t = pages();
        label.textContent = cols() > 1 ? (2 * p + 1) + '–' + Math.min(2 * p + 2, 2 * t) + ' of ' + (2 * t)
                                       : (p + 1) + ' of ' + t;
      }

      // (`keep`: a turn while a selection is dragged over the page's edge, which keeps the selection)
      window.luntaGoToPage = function(n, smooth, keep){
        if (!on) return;
        n = Math.max(0, Math.min(pages() - 1, n));
        target = n;
        // iPhone: the scroll view moves itself (WebKit can override a script's scroll after a text-size change)
        if (native) post({ page: n, smooth: !!smooth, keep: !!keep });
        else window.scrollTo({ left: n * W, top: 0, behavior: smooth ? 'smooth' : 'instant' });
        setTimeout(show, smooth ? 380 : 30);
      };
      // the page showing and the number of pages, as the label counts them (two columns: two pages)
      window.luntaPageInfo = function(){ return on ? { page: page(), pages: pages(), cols: cols() } : null; };
      window.luntaTurn = function(d){ window.luntaGoToPage(page() + d, true); };

      // the page holding a rectangle (from getBoundingClientRect or a range)
      window.luntaShowRect = function(r, smooth){
        if (!on || !r) return false;
        var x = r.left + window.scrollX + 1;
        window.luntaGoToPage(Math.floor(x / W), smooth);
        return true;
      };

      // contents, narration and highlights call scrollIntoView: turn to the element's page instead
      var plain = Element.prototype.scrollIntoView;
      Element.prototype.scrollIntoView = function(o){
        if (!on) return plain.apply(this, arguments);
        var rs = this.getClientRects();
        window.luntaShowRect(rs.length ? rs[0] : this.getBoundingClientRect(), !!(o && o.behavior === 'smooth'));
      };

      function remember(){ if (window.luntaReadingOffset) anchor = window.luntaReadingOffset(); }
      // the saved place being restored: lay out at it (not at the page that was showing) and keep
      // it while you stay on its page
      window.luntaKeepPlace = function(off){ anchor = off; heldPage = target; };
      window.luntaPageAnchor = function(){ return on && Math.abs(vw() - W) < 0.01 ? anchor : null; };
      function restore(){
        if (!(anchor != null && window.luntaScrollToOffset && window.luntaScrollToOffset(anchor))) window.luntaGoToPage(page(), false);
      }
      // while you stay on the page a relayout put you on, keep the place you were at (re-reading it
      // would give that page's first word, and the place would creep back with every change)
      function relayout(){ busy = true; layout(); restore(); busy = false; heldPage = target; show(); }

      // for the app after a text-size change: lay out again if the size changed, and say which page
      // holds the place being read (the app then moves the scroll view there)
      window.luntaTargetPage = function(){
        if (!on) return -1;
        if (Math.abs(vw() - W) > 0.01 || Math.abs(vh() - Hh) > 0.01) relayout();
        else restore();
        return target;
      };

      // A text-size change reaches the page a moment after the app asks for new pages, and the
      // resize event doesn't always follow: keep checking the size for a second and lay the
      // pages out again (at the remembered place) whenever it no longer matches.
      function watchSize(){
        checks.forEach(clearTimeout);
        checks = [50, 150, 350, 700, 1200].map(function(ms){
          return setTimeout(function(){ if (on && (Math.abs(vw() - W) > 0.01 || Math.abs(vh() - Hh) > 0.01)) relayout(); }, ms);
        });
      }

      window.luntaSetPaged = function(want, t, b, isNative){
        var changed = want !== on || t !== top || b !== bottom;
        if (!changed) { watchSize(); return; }
        // the place is remembered as you turn; only take it fresh when switching modes
        // (mid-resize the page may already be laid out for another size)
        if (want !== on || anchor == null) remember();
        on = want; top = t; bottom = b; native = !!isNative;
        relayout();
        watchSize();
      };

      // iPhone text size: the font is scaled (the page isn't zoomed, which iOS would magnify past
      // the screen's edge); the place being read is kept in both layouts
      window.luntaSetTextScale = function(x){
        var root = document.documentElement;
        if (root.style.getPropertyValue('--m-scale') === String(x)) return;
        if (on) {
          root.style.setProperty('--m-scale', String(x));
          relayout(); watchSize();
          setTimeout(function(){ relayout(); }, 400);     // again once images have settled
        } else {
          var off = window.luntaReadingOffset ? window.luntaReadingOffset() : null;
          root.style.setProperty('--m-scale', String(x));
          if (off != null && off > 0 && window.luntaScrollToOffset) window.luntaScrollToOffset(off);
          if (window.luntaFixOverflow) window.luntaFixOverflow();
        }
      };

      var lastShownPage = -1;
      window.addEventListener('scroll', function(){
        if (!on || busy) return;
        clearTimeout(settle);
        settle = setTimeout(function(){
          if (Math.abs(vw() - W) < 0.01 && page() !== heldPage) { heldPage = -1; remember(); }
          // a different page: a selection left on the previous one (and its Copy/Highlight menu) goes
          // (not the page a selection was just dragged onto, below)
          if (page() !== lastShownPage) {
            lastShownPage = page();
            var sl = window.getSelection(); if (sl && !sl.isCollapsed && Date.now() - edgeAt > 1500) sl.removeAllRanges();
          }
          // Mac: never left between two pages (a selection dragged past the edge scrolls the page)
          if (!native && W && Math.abs(window.scrollX - page() * W) > 2) window.luntaGoToPage(page(), false);
          show();
        }, 150);
      });
      var resizing = null;
      window.addEventListener('resize', function(){
        if (!on) return;
        clearTimeout(resizing);
        resizing = setTimeout(function(){ relayout(); watchSize(); }, 120);
      });
      if (window.visualViewport) window.visualViewport.addEventListener('resize', function(){ if (on) watchSize(); });
      window.addEventListener('load', function(){ if (on) { show(); } });

      // WebKit never turns the page while text is selected. iPhone: dragging a selection handle near
      // the screen's edge makes WebKit scroll the page sideways (its selection autoscroll); the app
      // holds the page still while text is selected (PageMessageHandler.held) and lines it up again
      // after. (A handle resting at the edge turns a whole page instead: luntaSelectionEdgeTurn.)
      // Mac: a selection dragged past the edge can shift the page; it is put back once settled.
      var selOn = false, selAt = 0;
      document.addEventListener('selectionchange', function(){
        var s = window.getSelection(), has = !!(s && !s.isCollapsed);
        if (has || selOn) selAt = Date.now();
        trackEnds(s, has);
        if (has === selOn) return;
        selOn = has;
        if (native) post({ selecting: has });
      });

      // A selection dragged over the page's edge (iPhone, iPad) goes on onto the next page. The app
      // watches the finger while a selection handle is dragged (SelectionDragWatcher below) and,
      // when it has rested for half a second, asks luntaSelectionEdgeTurn with the screen edge it
      // rests at (1 right, -1 left, 0 neither). The page turns when the finger is at an edge, or
      // when the end being dragged is on the page's last line (its start: the first line); the
      // other end stays where it is. One turn per rest: the finger has to move again for the next.
      // WebKit keeps the selection over the turn; if it ever drops it, it is made again from the
      // end that stayed to the first (last) word of the new page.
      var ends = null, moving = 1, edgeAt = 0;
      function trackEnds(s, has){
        if (!has || !s.rangeCount) { if (Date.now() - edgeAt > 1500) ends = null; return; }
        var r = s.getRangeAt(0);
        var e = { sc: r.startContainer, so: r.startOffset, ec: r.endContainer, eo: r.endOffset };
        if (ends) {
          var sameStart = e.sc === ends.sc && e.so === ends.so, sameEnd = e.ec === ends.ec && e.eo === ends.eo;
          if (sameEnd && !sameStart) moving = -1; else if (sameStart && !sameEnd) moving = 1;
        } else moving = 1;
        ends = e;
      }
      // which way to turn for the moving end's line box `g` (viewport coordinates; see the call):
      // the end at the end of the page's last line (or already past the page), the start at the
      // beginning of its first line. Anywhere else on those lines it stays (a word being picked there).
      window.luntaEdgeDirection = function(g){
        var colW = g.W / g.cols, p = Math.round(g.scrollX / g.W);
        var col = Math.floor((g.left + g.scrollX + 1) / colW), first = p * g.cols, last = first + g.cols - 1;
        var lh = Math.max(12, g.height || 0), x0 = (col - first) * colW;   // the column, on the screen
        if (g.end > 0) {
          if (col > last) return 1;                                         // already past this page
          if (col === last && g.bottom >= g.H - g.bottom0 - 1.6 * lh && g.right >= x0 + colW - g.pad - 1.5 * lh) return 1;
        } else {
          if (col < first) return -1;
          if (col === first && g.top <= g.top0 + lh && g.left <= x0 + g.pad + 1.5 * lh) return -1;
        }
        return 0;
      };
      function lineBox(r, end){
        var rs = r.getClientRects();
        for (var i = 0; i < rs.length; i++) {
          var x = rs[end > 0 ? rs.length - 1 - i : i];
          if (x.width > 0 || x.height > 0) return x;
        }
        return null;
      }
      // a text position at a point of the viewport (the new page's first or last word)
      function wordAt(x, y, end){
        var c = document.caretRangeFromPoint && document.caretRangeFromPoint(x, y);
        if (!c || c.startContainer.nodeType !== 3) return null;
        var t = c.startContainer.nodeValue, o = c.startOffset;
        if (end > 0) { while (o < t.length && /\S/.test(t[o])) o++; } else { while (o > 0 && /\S/.test(t[o - 1])) o--; }
        return [c.startContainer, o];
      }
      window.luntaSelectionEdgeTurn = function(zone){
        if (!on || !W || Date.now() - edgeAt < 900) return 0;
        var s = window.getSelection(); if (!s || !s.rangeCount || s.isCollapsed) return 0;
        var r = s.getRangeAt(0), box = lineBox(r, moving);
        var dir = zone || (box ? window.luntaEdgeDirection({ left: box.left, right: box.right, top: box.top, bottom: box.bottom, height: box.height,
                                   scrollX: window.scrollX, W: W, H: Hh, cols: cols(), pad: pad, top0: top, bottom0: bottom, end: moving }) : 0);
        var p = page();
        if (!dir || p + dir < 0 || p + dir > pages() - 1) return 0;
        var stay = moving > 0 ? [r.startContainer, r.startOffset] : [r.endContainer, r.endOffset];
        edgeAt = Date.now();
        window.luntaGoToPage(p + dir, true, true);
        setTimeout(function(){
          var s2 = window.getSelection();
          if (s2 && s2.rangeCount && !s2.isCollapsed) return;   // kept, as it should be
          var to = dir > 0 ? wordAt(pad + 3, top + 6, 1) : wordAt(W - pad - 3, Hh - bottom - 8, -1);
          if (!to) return;
          edgeAt = Date.now();
          try { s2.setBaseAndExtent(stay[0], stay[1], to[0], to[1]); } catch (x) {}
        }, 500);
        return dir;
      };

      // taps: edges turn the page, the middle shows or hides the bars (iPhone)
      document.addEventListener('click', function(e){
        if (!on || !native || e.defaultPrevented) return;
        if (document.body.classList.contains('listening')) return;
        if (e.target.closest && e.target.closest('a, img, mark, button, input, .m-scroll, svg')) return;
        var s = window.getSelection(); if (s && !s.isCollapsed) return;
        // the tap that only dismissed a selection (or ended a handle drag) doesn't turn
        if (Date.now() - selAt < 600) return;
        var x = e.clientX / window.innerWidth;
        if (x < 0.22) window.luntaTurn(-1);
        else if (x > 0.78) window.luntaTurn(1);
        else post('chrome');
      });

      // keys and trackpad (Mac)
      document.addEventListener('keydown', function(e){
        if (!on || e.metaKey || e.ctrlKey || e.altKey) return;
        var k = e.key;
        if (k === 'ArrowRight' || k === 'PageDown' || (k === ' ' && !e.shiftKey) || k === 'ArrowDown') { window.luntaTurn(1); e.preventDefault(); }
        else if (k === 'ArrowLeft' || k === 'PageUp' || (k === ' ' && e.shiftKey) || k === 'ArrowUp') { window.luntaTurn(-1); e.preventDefault(); }
        else if (k === 'Home') { window.luntaGoToPage(0, true); e.preventDefault(); }
        else if (k === 'End') { window.luntaGoToPage(pages() - 1, true); e.preventDefault(); }
      });
      var acc = 0, quietAt = 0, lockedUntil = 0, wheelTimer = null;
      window.addEventListener('wheel', function(e){
        if (!on || native) return;
        var box = e.target.closest && e.target.closest('.m-scroll');
        if (box && (box.scrollWidth > box.clientWidth + 1 || box.scrollHeight > box.clientHeight + 1)) return;
        e.preventDefault();
        var now = Date.now();
        clearTimeout(wheelTimer);
        wheelTimer = setTimeout(function(){ acc = 0; }, 180);
        if (now < lockedUntil) return;      // the rest of one swipe's momentum
        acc += Math.abs(e.deltaX) > Math.abs(e.deltaY) ? e.deltaX : e.deltaY;
        if (Math.abs(acc) > 40) {
          window.luntaTurn(acc > 0 ? 1 : -1);
          acc = 0; lockedUntil = now + 450;
        }
      }, { passive: false });

      post('hello');
      if (window.__luntaPagedStart) { var s0 = window.__luntaPagedStart; window.luntaSetPaged(true, s0[0], s0[1], s0[2]); }
    })();
    """#
}

/// What the page should look like; sent to the page whenever it changes or the page loads.
struct PagedState: Equatable {
    var on: Bool
    var top: Double
    var bottom: Double
    var native: Bool

    var call: String { "luntaSetPaged(\(on), \(Int(top)), \(Int(bottom)), \(native))" }
    var start: String { on ? "window.__luntaPagedStart = [\(Int(top)), \(Int(bottom)), \(native)];" : "" }
}

/// Receives "page" messages: the page loaded ("hello") or the middle was tapped ("chrome").
final class PageMessageHandler: NSObject, WKScriptMessageHandler {
    var state: PagedState?
    var onChrome: () -> Void = {}
    #if os(iOS)
    /// Text is selected in the page (the page is then held still, see `moved`).
    fileprivate var selecting = false
    /// Where the page is held while text is selected.
    fileprivate var held: CGFloat?
    fileprivate weak var watched: UIScrollView?
    fileprivate weak var webView: WKWebView?
    fileprivate var sync: DispatchWorkItem?
    fileprivate var offsetWatch: NSKeyValueObservation?
    /// A page turn with a selection being dragged over the edge is under way (its animation isn't undone).
    fileprivate var turningUntil = Date.distantPast
    fileprivate var dragWatcher: SelectionDragWatcher?
    #endif
    func userContentController(_ c: WKUserContentController, didReceive m: WKScriptMessage) {
        #if os(iOS)
        if let d = m.body as? [String: Any], let n = d["page"] as? Int, let v = m.webView {
            Self.show(page: n, in: v, animated: d["smooth"] as? Bool ?? false, keep: d["keep"] as? Bool ?? false)
            return
        }
        if let d = m.body as? [String: Any], let on = d["selecting"] as? Bool, let v = m.webView {
            selectionChanged(on, in: v)
            return
        }
        if m.body as? String == "hello", let v = m.webView { selecting = false; held = nil; watch(v) }
        #endif
        guard let s = m.body as? String else { return }
        if s == "hello", let state, let v = m.webView { v.evaluateJavaScript(state.call) }
        if s == "chrome" { Task { @MainActor in self.onChrome() } }
    }
}

#if os(iOS)
extension PageMessageHandler {
    /// Moves the scroll view to a page (a page is exactly the view's width).
    /// Right after the pages are laid out again (the page opening, a new text size) the scroll
    /// view's content size can lag behind for a moment: a page past its end waits for it
    /// (up to `wait` more tries) rather than landing on the last page of the old layout.
    /// `keep`: the turn of a selection dragged over the page's edge, animated like any turn (the
    /// selection is held on the new page).
    @MainActor static func show(page n: Int, in v: WKWebView, animated: Bool, keep: Bool = false, wait: Int = 20) {
        let sv = v.scrollView
        let w = sv.bounds.width
        guard w > 0 else { return }
        let maxX = max(0, sv.contentSize.width - w)
        let lastPage = (maxX / w).rounded(.up)
        if CGFloat(n) > lastPage, wait > 0 {
            latest[ObjectIdentifier(v)] = n
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.1) {
                // (unless another page was asked for meanwhile)
                guard latest[ObjectIdentifier(v)] == n else { return }
                show(page: n, in: v, animated: animated, keep: keep, wait: wait - 1)
            }
            return
        }
        latest[ObjectIdentifier(v)] = n
        let x = max(0, min(CGFloat(n) * w, lastPage * w))
        // a turn while text is selected (narration, contents): that page is the one to hold
        let h = handlers.object(forKey: v)
        h?.held = x
        let animate = animated && (h?.selecting != true || keep)
        if keep, animate { h?.turningUntil = Date().addingTimeInterval(0.5) }
        sv.setContentOffset(CGPoint(x: x, y: 0), animated: animate)
    }
    @MainActor private static var latest: [ObjectIdentifier: Int] = [:]
    @MainActor private static let handlers = NSMapTable<WKWebView, PageMessageHandler>.weakToWeakObjects()

    // Selecting text must never move the page. Dragging a selection handle near the screen's edge
    // makes WebKit scroll the page towards it (its selection autoscroll), a little at a time, which
    // left the page shifted part-way to the next one. While text is selected the scroll view is held
    // where it was; only a finger swiping the page (or a turn the app asks for) moves it then.

    /// Follows the page's scroll view (once per web view).
    @MainActor private func watch(_ v: WKWebView) {
        Self.handlers.setObject(self, forKey: v)
        webView = v
        if dragWatcher?.view !== v {
            let w = SelectionDragWatcher(handler: self)
            v.addGestureRecognizer(w)
            dragWatcher = w
        }
        guard watched !== v.scrollView else { return }
        watched = v.scrollView
        offsetWatch = v.scrollView.observe(\.contentOffset, options: [.new]) { [weak self] sv, _ in
            MainActor.assumeIsolated { self?.moved(sv) }
        }
    }

    @MainActor private func moved(_ sv: UIScrollView) {
        guard selecting, state?.on == true, Date() >= turningUntil else { return }
        let x = sv.contentOffset.x
        if sv.isDragging || sv.isDecelerating {
            // a swipe turns the page as usual, and the selection (with its Copy/Highlight menu) is
            // dropped at once rather than left floating over the next page
            if held != nil, let v = webView { v.evaluateJavaScript("window.getSelection().removeAllRanges()") }
            held = x
            return
        }
        guard let held else { held = x; return }
        guard abs(x - held) > 0.5 else { return }
        // WebKit's autoscroll: undo it at once (nothing shows of it), and tell the page too, which
        // would otherwise go on thinking it had scrolled (and place the selection by that)
        sv.contentOffset.x = held
        // (again a moment later: WebKit can deliver a last step of its scroll after the first)
        sync?.cancel()
        let now = DispatchWorkItem { [weak self] in self?.syncPage() }
        let later = DispatchWorkItem { [weak self] in self?.syncPage() }
        sync = later
        DispatchQueue.main.async(execute: now)
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.3, execute: later)
    }

    // MARK: a selection dragged over the page's edge (pagedJS: luntaSelectionEdgeTurn)

    /// A selection handle can be dragged now: text is selected, in pages, and the page isn't being swiped.
    @MainActor fileprivate var canEdgeTurn: Bool {
        guard selecting, state?.on == true, let sv = webView?.scrollView else { return false }
        return !sv.isDragging && !sv.isDecelerating
    }

    /// The handle is being dragged (true) or was let go: highlight mode waits for that.
    @MainActor fileprivate func dragChanged(_ on: Bool) {
        webView?.evaluateJavaScript("window.luntaDragging = \(on)")
    }

    /// The finger has rested at `zone` (1 the right edge, -1 the left, 0 elsewhere): the page
    /// decides whether to turn.
    @MainActor fileprivate func edgeRest(zone: Int) {
        guard canEdgeTurn else { return }
        webView?.evaluateJavaScript("window.luntaSelectionEdgeTurn ? luntaSelectionEdgeTurn(\(zone)) : 0")
    }

    @MainActor private func syncPage() {
        guard selecting, let h = held, let v = webView else { return }
        v.evaluateJavaScript("if (Math.abs(window.scrollX - \(h)) > 0.5) window.scrollTo(\(h), 0)")
    }

    @MainActor private func selectionChanged(_ on: Bool, in v: WKWebView) {
        watch(v)
        let sv = v.scrollView
        selecting = on
        guard state?.on == true else { return }
        let w = sv.bounds.width
        guard w > 0, !sv.isDragging, !sv.isDecelerating else { held = nil; return }
        let x = sv.contentOffset.x
        if on {
            // the page being read (a long press needs a still finger, so the page is at rest)
            held = x
        } else {
            held = nil
            // whatever moved it meanwhile, the page ends up whole
            let page = (x / w).rounded()
            if abs(x - page * w) > 1 { Self.show(page: Int(page), in: v, animated: true) }
        }
    }
}

/// Watches a finger dragging a selection handle in pages, without taking part in any gesture
/// (WebKit's handles, swipes and taps work as before): where it is, and when it has rested for
/// 0.7 s (then, before the next rest counts, 1.5 s must pass and the finger must move well away; at the
/// screen's side it must also leave the side first, so a finger held there turns one page).
final class SelectionDragWatcher: UIGestureRecognizer, UIGestureRecognizerDelegate {
    private weak var handler: PageMessageHandler?
    private var start: CGPoint?
    private var restPoint: CGPoint?
    private var dragging = false
    private var rest: DispatchWorkItem?
    /// Where and when the last rest asked for a turn, and whether the finger has left the screen's
    /// side since: a finger kept at the side (it is never quite still) must not flip page after page.
    private var lastAsk: (point: CGPoint, at: Date)?
    private var leftSide = true
    /// How close to the screen's side the finger counts as at its edge (points).
    static let edge: CGFloat = 24
    /// How long the finger rests before a turn, and the least time between two turns.
    static let hold: TimeInterval = 0.7
    static let between: TimeInterval = 1.5
    /// How far the finger must move after a turn (points), or leave the side, before the next rest counts.
    static let moveAgain: CGFloat = 44

    init(handler: PageMessageHandler) {
        self.handler = handler
        super.init(target: nil, action: nil)
        cancelsTouchesInView = false
        delaysTouchesBegan = false
        delaysTouchesEnded = false
        delegate = self
    }

    func gestureRecognizer(_ g: UIGestureRecognizer, shouldRecognizeSimultaneouslyWith other: UIGestureRecognizer) -> Bool { true }

    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent) {
        guard (event.allTouches?.count ?? touches.count) == 1, let t = touches.first, t.type == .direct else { finish(); return }
        start = t.location(in: view)
    }

    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent) {
        guard let t = touches.first, let v = view, let s = start, let h = handler else { return }
        let p = t.location(in: v)
        guard h.canEdgeTurn else { return }
        if !dragging {
            guard hypot(p.x - s.x, p.y - s.y) > 8 else { return }
            dragging = true
            h.dragChanged(true)
        }
        // still resting (a finger is never quite still)
        if let r = restPoint, hypot(p.x - r.x, p.y - r.y) < 6 { return }
        restPoint = p
        rest?.cancel()
        let w = v.bounds.width
        let zone = p.x > w - Self.edge ? 1 : (p.x < Self.edge ? -1 : 0)
        // well away from both sides again: the next rest at a side may turn
        if p.x < w - 3 * Self.edge && p.x > 3 * Self.edge { leftSide = true }
        let item = DispatchWorkItem { [weak self] in
            MainActor.assumeIsolated {
                guard let self, self.dragging else { return }
                if let last = self.lastAsk {
                    if Date().timeIntervalSince(last.at) < Self.between { return }
                    if hypot(p.x - last.point.x, p.y - last.point.y) < Self.moveAgain { return }
                }
                if zone != 0 && !self.leftSide { return }
                self.lastAsk = (p, Date())
                if zone != 0 { self.leftSide = false }
                self.handler?.edgeRest(zone: zone)
            }
        }
        rest = item
        DispatchQueue.main.asyncAfter(deadline: .now() + Self.hold, execute: item)
    }

    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent) { finish() }
    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent) { finish() }
    override func reset() { super.reset(); finish(recognizerState: false) }

    private func finish(recognizerState: Bool = true) {
        rest?.cancel(); rest = nil
        if dragging { handler?.dragChanged(false) }
        dragging = false; start = nil; restPoint = nil; lastAsk = nil; leftSide = true
        // never recognized: it only watches
        if recognizerState, state == .possible { state = .failed }
    }
}
#endif

extension WKWebViewConfiguration {
    /// Adds page turning (off until `state.on`); returns the handler to update later.
    func addPaging(_ state: PagedState) -> PageMessageHandler {
        userContentController.addUserScript(WKUserScript(source: state.start + ReaderAssets.pagedJS, injectionTime: .atDocumentEnd, forMainFrameOnly: true))
        let h = PageMessageHandler()
        h.state = state
        userContentController.add(h, name: "page")
        return h
    }
}
