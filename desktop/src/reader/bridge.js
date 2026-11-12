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
    if (mod && e.shiftKey && (e.key === 'h' || e.key === 'H')) { send('__key', 'highlight'); e.preventDefault(); }
    else if (e.key === 'Escape') send('__key', 'escape');
    else if (mod && (e.key === '=' || e.key === '+')) { send('__key', 'larger'); e.preventDefault(); }
    else if (mod && e.key === '-') { send('__key', 'smaller'); e.preventDefault(); }
  });
  // links out of the paper open in the browser, not inside the reader
  document.addEventListener('click', function(e){
    var a = e.target.closest && e.target.closest('a[href]'); if (!a) return;
    var h = a.getAttribute('href') || '';
    if (/^(https?:|mailto:)/i.test(h)) { e.preventDefault(); send('__link', a.href); }
  });
})();
