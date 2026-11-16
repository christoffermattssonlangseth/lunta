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
  // figures in the text as cards or in full (the apps' FigureSetting; figureJS reads it as it runs)
  if (q.get('cards')) window.__luntaFigureCards = q.get('cards') !== '0';

  // Reader themes (ReaderTheme.swift): <html data-theme>, with the light or dark appearance forced
  // underneath (Light and Paper light, Quiet and Black dark, Automatic the system's). A web view
  // here can't be given an appearance of its own, so the page's prefers-color-scheme rules are
  // switched on or off by hand (their original condition kept to switch back).
  var schemeRules = new Map(), forced = null;
  function walkRules(rules){
    for (var j = 0; rules && j < rules.length; j++) {
      var r = rules[j];
      if (!r.media || !r.cssRules) continue;
      if (!schemeRules.has(r) && /prefers-color-scheme/.test(r.media.mediaText)) schemeRules.set(r, r.media.mediaText);
      if (schemeRules.has(r)) {
        var orig = schemeRules.get(r), dark = /prefers-color-scheme\s*:\s*dark/.test(orig);
        var want = forced ? ((forced === 'dark') === dark ? 'all' : 'not all') : orig;
        if (r.media.mediaText !== want) r.media.mediaText = want;
      }
      walkRules(r.cssRules);
    }
  }
  function applyScheme(){
    for (var i = 0; i < document.styleSheets.length; i++) {
      var rules = null; try { rules = document.styleSheets[i].cssRules; } catch (x) {}
      walkRules(rules);
    }
  }
  window.luntaSetTheme = function(t){
    t = /^(automatic|light|paper|quiet|black)$/.test(t || '') ? t : 'automatic';
    forced = t === 'light' || t === 'paper' ? 'light' : t === 'quiet' || t === 'black' ? 'dark' : null;
    document.documentElement.setAttribute('data-theme', t);
    document.documentElement.style.colorScheme = forced || '';
    applyScheme();
  };
  window.luntaSetTheme(q.get('theme'));
  // the reader's own style sheets are added by its scripts as the page loads: switch them too
  document.addEventListener('DOMContentLoaded', function(){ applyScheme(); setTimeout(applyScheme, 0); });
  window.addEventListener('load', applyScheme);

  function send(name, body){ try { LuntaNative.post(name, JSON.stringify(body === undefined ? null : body)); } catch (x) {} }
  var handlers = {};
  ['pos', 'fig', 'narr', 'page', 'ann'].forEach(function(n){ handlers[n] = { postMessage: function(b){ send(n, b); } }; });
  window.webkit = { messageHandlers: handlers };

  // Pages: selecting text never turns or shifts the page (cf. PageMessageHandler in PageTurn.swift,
  // which does this for the iPhone's scroll view). Here the pages are turned by script (the root
  // doesn't scroll by hand), but dragging a selection past the page's edge makes the web view
  // scroll the page towards it; while text is selected the page is held where it was, and when the
  // selection ends it is lined up on a whole page again. Turns asked for meanwhile (contents,
  // narration, the highlight list) still go through.
  var selOn = false, selAt = 0, held = null, turning = 0;
  function pagedNow(){ return !!(window.luntaPaged && window.luntaPaged()); }
  function pageW(){ return window.visualViewport ? window.visualViewport.width * window.visualViewport.scale : window.innerWidth; }
  function wrapTurns(){
    var go = window.luntaGoToPage;
    if (!go || go.__lunta) return;
    var w = function(n, smooth){ turning = Date.now() + (smooth ? 700 : 150); return go.apply(this, arguments); };
    w.__lunta = true;
    window.luntaGoToPage = w;
  }
  document.addEventListener('DOMContentLoaded', wrapTurns);
  document.addEventListener('selectionchange', function(){
    wrapTurns();
    var s = window.getSelection(), has = !!(s && !s.isCollapsed);
    if (has || selOn) selAt = Date.now();
    if (has === selOn) return;
    selOn = has;
    if (has) { held = pagedNow() ? window.scrollX : null; return; }
    var was = held; held = null;
    if (was == null || !pagedNow()) return;
    var W = pageW(), n = W ? Math.round(window.scrollX / W) : 0;
    if (W && Math.abs(window.scrollX - n * W) > 1) window.luntaGoToPage(n, false);
  });
  window.addEventListener('scroll', function(){
    if (held == null || !selOn || !pagedNow()) return;
    if (Date.now() < turning) { held = window.scrollX; return; }
    if (Math.abs(window.scrollX - held) > 0.5) window.scrollTo({ left: held, top: window.scrollY, behavior: 'instant' });
  });

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
    // the tap that only dismissed a selection (or ended a handle drag) doesn't turn the page
    if (Date.now() - selAt < 600) return;
    var x = e.clientX / window.innerWidth;
    if (window.luntaPaged && window.luntaPaged()) {
      if (x < 0.22) { window.luntaTurn(-1); return; }
      if (x > 0.78) { window.luntaTurn(1); return; }
    }
    send('__chrome', null);
  });
})();
