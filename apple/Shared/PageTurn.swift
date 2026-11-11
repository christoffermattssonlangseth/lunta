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
      var on = false, top = 24, bottom = 34, native = false, W = 0, anchor = null, settle = null, busy = false;
      var style = document.createElement('style'); document.documentElement.appendChild(style);
      var label = document.createElement('div'); label.id = 'm-page-no';
      function post(m){ try { window.webkit.messageHandlers.page.postMessage(m); } catch (x) {} }

      window.marginsPaged = function(){ return on; };
      function cols(){ return window.innerWidth >= 860 ? 2 : 1; }
      function page(){ return W ? Math.round(window.scrollX / W) : 0; }
      function pages(){ return W ? Math.max(1, Math.round(document.documentElement.scrollWidth / W)) : 1; }

      function layout(){
        if (!on) { style.textContent = ''; document.documentElement.classList.remove('m-paged'); label.remove(); return; }
        W = window.innerWidth;
        var H = window.innerHeight, n = cols();
        var minPad = n > 1 ? 44 : (W < 500 ? 20 : 36);
        var colW = Math.min(W / n - 2 * minPad, 640);
        var pad = (W / n - colW) / 2;
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
        window.scrollTo({ left: n * W, top: 0, behavior: smooth ? 'smooth' : 'instant' });
        setTimeout(show, smooth ? 380 : 0);
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
      function restore(){ if (anchor != null && window.marginsScrollToOffset) window.marginsScrollToOffset(anchor); }

      window.marginsSetPaged = function(want, t, b, isNative){
        var changed = want !== on || t !== top || b !== bottom;
        if (!changed) return;
        remember();
        busy = true;
        on = want; top = t; bottom = b; native = !!isNative;
        layout();
        restore();
        busy = false;
        show();
      };

      window.addEventListener('scroll', function(){
        if (!on || busy) return;
        clearTimeout(settle);
        settle = setTimeout(function(){ remember(); show(); }, 150);
      });
      var resizing = null;
      window.addEventListener('resize', function(){
        if (!on) return;
        clearTimeout(resizing);
        resizing = setTimeout(function(){ busy = true; layout(); restore(); busy = false; show(); }, 120);
      });
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
        guard let s = m.body as? String else { return }
        if s == "hello", let state, let v = m.webView { v.evaluateJavaScript(state.call) }
        if s == "chrome" { Task { @MainActor in self.onChrome() } }
    }
}

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
