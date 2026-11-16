import Foundation
import WebKit

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
          '#m-page-no { position: fixed; left: 0; right: 0; bottom: ' + Math.max(6, bottom / 2 - 8) + 'px; text-align: center;' +
          ' font: 12px -apple-system, sans-serif; color: rgba(127,127,127,.85); pointer-events: none; z-index: 10; }';
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

      window.luntaGoToPage = function(n, smooth){
        if (!on) return;
        n = Math.max(0, Math.min(pages() - 1, n));
        target = n;
        // iPhone: the scroll view moves itself (WebKit can override a script's scroll after a text-size change)
        if (native) post({ page: n, smooth: !!smooth });
        else window.scrollTo({ left: n * W, top: 0, behavior: smooth ? 'smooth' : 'instant' });
        setTimeout(show, smooth ? 380 : 30);
      };
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
          if (page() !== lastShownPage) {
            lastShownPage = page();
            var sl = window.getSelection(); if (sl && !sl.isCollapsed) sl.removeAllRanges();
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

      // Selecting text never turns the page. iPhone: dragging a selection handle near the screen's
      // edge makes WebKit scroll the page sideways (its selection autoscroll); the app holds the
      // page still while text is selected (PageMessageHandler.held) and lines it up again after.
      // Mac: a selection dragged past the edge can shift the page; it is put back once settled.
      var selOn = false, selAt = 0;
      document.addEventListener('selectionchange', function(){
        var s = window.getSelection(), has = !!(s && !s.isCollapsed);
        if (has || selOn) selAt = Date.now();
        if (has === selOn) return;
        selOn = has;
        if (native) post({ selecting: has });
      });

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
    #endif
    func userContentController(_ c: WKUserContentController, didReceive m: WKScriptMessage) {
        #if os(iOS)
        if let d = m.body as? [String: Any], let n = d["page"] as? Int, let v = m.webView {
            Self.show(page: n, in: v, animated: d["smooth"] as? Bool ?? false)
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
    @MainActor static func show(page n: Int, in v: WKWebView, animated: Bool, wait: Int = 20) {
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
                show(page: n, in: v, animated: animated, wait: wait - 1)
            }
            return
        }
        latest[ObjectIdentifier(v)] = n
        let x = max(0, min(CGFloat(n) * w, lastPage * w))
        // a turn while text is selected (narration, contents): that page is the one to hold
        let h = handlers.object(forKey: v)
        h?.held = x
        sv.setContentOffset(CGPoint(x: x, y: 0), animated: animated && h?.selecting != true)
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
        guard watched !== v.scrollView else { return }
        watched = v.scrollView
        offsetWatch = v.scrollView.observe(\.contentOffset, options: [.new]) { [weak self] sv, _ in
            MainActor.assumeIsolated { self?.moved(sv) }
        }
    }

    @MainActor private func moved(_ sv: UIScrollView) {
        guard selecting, state?.on == true else { return }
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
