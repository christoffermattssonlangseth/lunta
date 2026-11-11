import Foundation
import WebKit

// Page turning: the reading view laid out as pages instead of one long scroll. The page is cut
// into screen-sized CSS columns and moved sideways one screen at a time (two columns side by side
// on a wide window). On the iPhone the web view's own scroll view does the paging, so the page
// follows your finger; tap the left or right edge to turn, the middle to show or hide the bars.
// On the Mac: arrow keys, space, or a swipe / scroll.
//
// Everything that used to scroll (contents, narration, highlights, the saved position) goes
// through scrollIntoView or marginsScrollToOffset, which turn to the right page in this mode.

extension ReaderAssets {
    static let pagedJS = #"""
    (function(){
      var on = false, top = 24, bottom = 34, native = false, W = 0, Hh = 0, anchor = null, settle = null, busy = false, checks = [], heldPage = -1, target = 0;
      var style = document.createElement('style'); document.documentElement.appendChild(style);
      var label = document.createElement('div'); label.id = 'm-page-no';
      function post(m){ try { window.webkit.messageHandlers.page.postMessage(m); } catch (x) {} }

      window.marginsPaged = function(){ return on; };
      // the exact viewport size: with larger text it isn't a whole number of pixels, and
      // innerWidth's rounding would add up to a visible shift after a few pages
      function vw(){ return window.visualViewport ? window.visualViewport.width * window.visualViewport.scale : window.innerWidth; }
      function vh(){ return window.visualViewport ? window.visualViewport.height * window.visualViewport.scale : window.innerHeight; }
      var pad = 20;
      // where the first line of the page starts (to read the exact position)
      window.marginsPageOrigin = function(){ return on ? { x: pad + 3, y: top + 6 } : null; };
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
        if (window.marginsFixOverflow) window.marginsFixOverflow();
      }

      function show(){
        if (!on) return;
        var p = page(), t = pages();
        label.textContent = cols() > 1 ? (2 * p + 1) + '–' + Math.min(2 * p + 2, 2 * t) + ' of ' + (2 * t)
                                       : (p + 1) + ' of ' + t;
      }

      window.marginsGoToPage = function(n, smooth){
        if (!on) return;
        n = Math.max(0, Math.min(pages() - 1, n));
        target = n;
        // iPhone: the scroll view moves itself (WebKit can override a script's scroll after a text-size change)
        if (native) post({ page: n, smooth: !!smooth });
        else window.scrollTo({ left: n * W, top: 0, behavior: smooth ? 'smooth' : 'instant' });
        setTimeout(show, smooth ? 380 : 30);
      };
      window.marginsTurn = function(d){ window.marginsGoToPage(page() + d, true); };

      // the page holding a rectangle (from getBoundingClientRect or a range)
      window.marginsShowRect = function(r, smooth){
        if (!on || !r) return false;
        var x = r.left + window.scrollX + 1;
        window.marginsGoToPage(Math.floor(x / W), smooth);
        return true;
      };

      // contents, narration and highlights call scrollIntoView: turn to the element's page instead
      var plain = Element.prototype.scrollIntoView;
      Element.prototype.scrollIntoView = function(o){
        if (!on) return plain.apply(this, arguments);
        var rs = this.getClientRects();
        window.marginsShowRect(rs.length ? rs[0] : this.getBoundingClientRect(), !!(o && o.behavior === 'smooth'));
      };

      function remember(){ if (window.marginsReadingOffset) anchor = window.marginsReadingOffset(); }
      function restore(){
        if (!(anchor != null && window.marginsScrollToOffset && window.marginsScrollToOffset(anchor))) window.marginsGoToPage(page(), false);
      }
      // while you stay on the page a relayout put you on, keep the place you were at (re-reading it
      // would give that page's first word, and the place would creep back with every change)
      function relayout(){ busy = true; layout(); restore(); busy = false; heldPage = target; show(); }

      // for the app after a text-size change: lay out again if the size changed, and say which page
      // holds the place being read (the app then moves the scroll view there)
      window.marginsTargetPage = function(){
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

      window.marginsSetPaged = function(want, t, b, isNative){
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
      window.marginsSetTextScale = function(x){
        var root = document.documentElement;
        if (root.style.getPropertyValue('--m-scale') === String(x)) return;
        if (on) {
          root.style.setProperty('--m-scale', String(x));
          relayout(); watchSize();
          setTimeout(function(){ relayout(); }, 400);     // again once images have settled
        } else {
          var off = window.marginsReadingOffset ? window.marginsReadingOffset() : null;
          root.style.setProperty('--m-scale', String(x));
          if (off != null && off > 0 && window.marginsScrollToOffset) window.marginsScrollToOffset(off);
          if (window.marginsFixOverflow) window.marginsFixOverflow();
        }
      };

      window.addEventListener('scroll', function(){
        if (!on || busy) return;
        clearTimeout(settle);
        settle = setTimeout(function(){ if (Math.abs(vw() - W) < 0.01 && page() !== heldPage) { heldPage = -1; remember(); } show(); }, 150);
      });
      var resizing = null;
      window.addEventListener('resize', function(){
        if (!on) return;
        clearTimeout(resizing);
        resizing = setTimeout(function(){ relayout(); watchSize(); }, 120);
      });
      if (window.visualViewport) window.visualViewport.addEventListener('resize', function(){ if (on) watchSize(); });
      window.addEventListener('load', function(){ if (on) { show(); } });

      // taps: edges turn the page, the middle shows or hides the bars (iPhone)
      document.addEventListener('click', function(e){
        if (!on || !native || e.defaultPrevented) return;
        if (document.body.classList.contains('listening')) return;
        if (e.target.closest && e.target.closest('a, img, mark, button, input, .m-scroll, svg')) return;
        var s = window.getSelection(); if (s && !s.isCollapsed) return;
        var x = e.clientX / window.innerWidth;
        if (x < 0.22) window.marginsTurn(-1);
        else if (x > 0.78) window.marginsTurn(1);
        else post('chrome');
      });

      // keys and trackpad (Mac)
      document.addEventListener('keydown', function(e){
        if (!on || e.metaKey || e.ctrlKey || e.altKey) return;
        var k = e.key;
        if (k === 'ArrowRight' || k === 'PageDown' || (k === ' ' && !e.shiftKey) || k === 'ArrowDown') { window.marginsTurn(1); e.preventDefault(); }
        else if (k === 'ArrowLeft' || k === 'PageUp' || (k === ' ' && e.shiftKey) || k === 'ArrowUp') { window.marginsTurn(-1); e.preventDefault(); }
        else if (k === 'Home') { window.marginsGoToPage(0, true); e.preventDefault(); }
        else if (k === 'End') { window.marginsGoToPage(pages() - 1, true); e.preventDefault(); }
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
          window.marginsTurn(acc > 0 ? 1 : -1);
          acc = 0; lockedUntil = now + 450;
        }
      }, { passive: false });

      post('hello');
      if (window.__marginsPagedStart) { var s0 = window.__marginsPagedStart; window.marginsSetPaged(true, s0[0], s0[1], s0[2]); }
    })();
    """#
}

/// What the page should look like; sent to the page whenever it changes or the page loads.
struct PagedState: Equatable {
    var on: Bool
    var top: Double
    var bottom: Double
    var native: Bool

    var call: String { "marginsSetPaged(\(on), \(Int(top)), \(Int(bottom)), \(native))" }
    var start: String { on ? "window.__marginsPagedStart = [\(Int(top)), \(Int(bottom)), \(native)];" : "" }
}

/// Receives "page" messages: the page loaded ("hello") or the middle was tapped ("chrome").
final class PageMessageHandler: NSObject, WKScriptMessageHandler {
    var state: PagedState?
    var onChrome: () -> Void = {}
    func userContentController(_ c: WKUserContentController, didReceive m: WKScriptMessage) {
        #if os(iOS)
        if let d = m.body as? [String: Any], let n = d["page"] as? Int, let v = m.webView {
            Self.show(page: n, in: v, animated: d["smooth"] as? Bool ?? false)
            return
        }
        #endif
        guard let s = m.body as? String else { return }
        if s == "hello", let state, let v = m.webView { v.evaluateJavaScript(state.call) }
        if s == "chrome" { Task { @MainActor in self.onChrome() } }
    }
}

#if os(iOS)
extension PageMessageHandler {
    /// Moves the scroll view to a page (a page is exactly the view's width).
    @MainActor static func show(page n: Int, in v: WKWebView, animated: Bool) {
        let sv = v.scrollView
        let w = sv.bounds.width
        guard w > 0 else { return }
        let maxX = max(0, sv.contentSize.width - w)
        let x = min(CGFloat(n) * w, (maxX / w).rounded(.up) * w)
        sv.setContentOffset(CGPoint(x: max(0, x), y: 0), animated: animated)
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
