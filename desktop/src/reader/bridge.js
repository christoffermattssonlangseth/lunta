/* The desktop shell's stand-in for WKWebView's script messaging, loaded into the reading view
   before the apps' reader scripts (src/reader/generated/). Those scripts post to
   window.webkit.messageHandlers.<name>.postMessage(...) as on the Mac and iPhone; here each
   message goes to the app window with postMessage. The app runs scripts in the page (the
   apps' evaluateJavaScript) by sending {lunta: 'eval', id, code}; the result comes back as
   {lunta: 'result', id, result, error}. Written by hand (not generated). */
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

  function send(name, body){ try { parent.postMessage({ lunta: 'message', name: name, body: body }, '*'); } catch (x) {} }
  var handlers = {};
  ['pos', 'fig', 'narr', 'page', 'ann'].forEach(function(n){ handlers[n] = { postMessage: function(b){ send(n, b); } }; });
  window.webkit = { messageHandlers: handlers };

  window.addEventListener('message', function(e){
    if (e.source !== parent) return;
    var d = e.data; if (!d || d.lunta !== 'eval') return;
    var result = null, error = null;
    try { result = (0, eval)(d.code); } catch (x) { error = String(x); }
    if (result !== null && typeof result === 'object') { try { result = JSON.parse(JSON.stringify(result)); } catch (x) { result = null; } }
    if (typeof result === 'function' || typeof result === 'undefined') result = null;
    parent.postMessage({ lunta: 'result', id: d.id, result: result, error: error }, '*');
  });

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

  // the selection (for the shell's highlight bar) — its rectangle in the page's viewport
  var t = null;
  function selection(){
    var s = window.getSelection();
    if (!s || s.isCollapsed || !s.rangeCount || !s.toString().trim()) { send('__selection', null); return; }
    var r = s.getRangeAt(0).getBoundingClientRect();
    send('__selection', { x: r.left, y: r.top, w: r.width, h: r.height });
  }
  document.addEventListener('selectionchange', function(){ clearTimeout(t); t = setTimeout(selection, 250); });
  document.addEventListener('mouseup', function(){ clearTimeout(t); t = setTimeout(selection, 30); });
  // shortcuts the shell handles while the page has the keyboard
  document.addEventListener('keydown', function(e){
    var mod = e.metaKey || e.ctrlKey;
    if (e.ctrlKey && e.altKey && !e.shiftKey && e.code === 'KeyH') { send('__key', 'hlmode'); e.preventDefault(); }
    else if (mod && e.shiftKey && !e.altKey && e.code === 'KeyH') { send('__key', 'highlight'); e.preventDefault(); }
    else if (mod && e.shiftKey && !e.altKey && e.code === 'KeyF') { send('__key', 'figures'); e.preventDefault(); }
    else if (mod && !e.shiftKey && !e.altKey && e.code === 'KeyF') { send('__key', 'find'); e.preventDefault(); }
    else if (e.key === 'F3' || (mod && !e.altKey && e.code === 'KeyG')) { send('__key', e.shiftKey ? 'findprev' : 'findnext'); e.preventDefault(); }
    else if (e.key === 'Escape') send('__key', 'escape');
    else if (mod && (e.key === '=' || e.key === '+')) { send('__key', 'larger'); e.preventDefault(); }
    else if (mod && e.key === '-') { send('__key', 'smaller'); e.preventDefault(); }
  });
  // links out of the paper open in the browser, not inside the reader; the citation card's
  // "Add to Lunta" (lunta://cite?doi=…|arxiv=…) goes to the app, which answers with luntaCiteDone
  document.addEventListener('click', function(e){
    var a = e.target.closest && e.target.closest('a[href]'); if (!a) return;
    var h = a.getAttribute('href') || '';
    if (/^(https?:|mailto:)/i.test(h)) { e.preventDefault(); send('__link', a.href); }
    else if (/^lunta:\/\/cite\b/i.test(h)) { e.preventDefault(); send('__cite', h); }
  });
})();
