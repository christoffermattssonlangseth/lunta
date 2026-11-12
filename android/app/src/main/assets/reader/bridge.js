/* The Android app's stand-in for WKWebView's script messaging (cf. desktop/src/reader/bridge.js),
   loaded into the reading view before the apps' reader scripts. Those scripts post to
   window.webkit.messageHandlers.<name>.postMessage(...) as on the Mac and iPhone; here each message
   goes to the app through the addJavascriptInterface object LuntaNative. The app runs scripts in
   the page with WebView.evaluateJavascript. Written by hand (not generated). */
(function(){
  var q = new URLSearchParams(location.search);
  // text size and page mode the reader opens with (pagedJS reads __luntaPagedStart when it loads)
  if (q.get('scale')) document.documentElement.style.setProperty('--m-scale', q.get('scale'));
  if (q.get('paged') === '1') window.__luntaPagedStart = [30, 40, false];

  function send(name, body){ try { LuntaNative.post(name, JSON.stringify(body === undefined ? null : body)); } catch (x) {} }
  var handlers = {};
  ['pos', 'fig', 'narr', 'page', 'ann'].forEach(function(n){ handlers[n] = { postMessage: function(b){ send(n, b); } }; });
  window.webkit = { messageHandlers: handlers };

  // is there a selection? (the app shows its highlight bar)
  var t = null;
  document.addEventListener('selectionchange', function(){
    clearTimeout(t);
    t = setTimeout(function(){
      var s = window.getSelection();
      send('__selection', !!(s && !s.isCollapsed && s.rangeCount && s.toString().trim()));
    }, 200);
  });

  // swipes turn pages in page mode
  var sx = null, sy = 0;
  document.addEventListener('touchstart', function(e){
    if (e.touches.length === 1) { sx = e.touches[0].clientX; sy = e.touches[0].clientY; } else sx = null;
  }, { passive: true });
  document.addEventListener('touchend', function(e){
    if (sx === null) return;
    var c = e.changedTouches[0], dx = c.clientX - sx, dy = c.clientY - sy; sx = null;
    if (window.luntaPaged && window.luntaPaged() && Math.abs(dx) > 50 && Math.abs(dx) > Math.abs(dy) * 1.5) {
      var s = window.getSelection(); if (s && !s.isCollapsed) return;
      window.luntaTurn(dx < 0 ? 1 : -1);
    }
  }, { passive: true });

  // taps: links out of the paper open in the browser; page edges turn pages; elsewhere show/hide the bars
  document.addEventListener('click', function(e){
    var a = e.target.closest && e.target.closest('a[href]');
    if (a) {
      var h = a.getAttribute('href') || '';
      if (/^(https?:|mailto:)/i.test(h)) { e.preventDefault(); send('__link', a.href); }
      return;
    }
    if (e.defaultPrevented) return;
    if (e.target.closest && e.target.closest('mark, img, .m-scroll, button, input, svg')) return;
    var s = window.getSelection(); if (s && !s.isCollapsed) return;
    var x = e.clientX / window.innerWidth;
    if (window.luntaPaged && window.luntaPaged()) {
      if (x < 0.22) { window.luntaTurn(-1); return; }
      if (x > 0.78) { window.luntaTurn(1); return; }
    }
    send('__chrome', null);
  });
})();
