// Google Docs citing for the extension in Safari, the part Lunta for Mac runs (not the browser).
// Safari has no Google sign-in for extensions, so Lunta for Mac signs in to Google and answers the
// panel's document requests itself: apple/Mac/GoogleDocsScript.swift loads citations-common.js,
// docs-model.js, flow.js and this file (from the Safari extension in the app) into a JavaScriptCore
// context, so the citations are worked out by the same code as in Chrome and Firefox. What
// JavaScriptCore doesn't have is asked of the app through LuntaNative.call(id, name, argsJSON),
// answered with LuntaAppHost.settle(id, ok, payloadJSON):
//   "docs"   {method, url, body} → {status, json}: the Docs API with the app's Google token
//            (rejects {message, kind: "auth"} when not connected; the token never comes here)
//   "lunta"  {path, body} → {status, json}: Lunta's citation API (/bibliography, /render, /placeholders)
//   "gzip"   text → base64 of its gzip;  "gunzip" base64 → text;  "sleep" ms → null
// A request: LuntaAppHost.handle(id, msgJSON) → LuntaNative.done(id, answerJSON) ({error, kind} if it failed).
// Tested in node with stand-ins for the app: apple/citations-tests/gdocs-safari.test.js.
/* global LuntaCite, LuntaDocs, makeLuntaGDocs, LuntaNative */
var LuntaAppHost = (function () {
  var B64 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';

  // base64 of bytes (JavaScriptCore has no btoa/atob)
  function bytesToB64(b) {
    var out = '', i;
    for (i = 0; i + 2 < b.length; i += 3) {
      var n = (b[i] << 16) | (b[i + 1] << 8) | b[i + 2];
      out += B64[n >> 18] + B64[(n >> 12) & 63] + B64[(n >> 6) & 63] + B64[n & 63];
    }
    if (i < b.length) {
      var m = (b[i] << 16) | ((i + 1 < b.length ? b[i + 1] : 0) << 8);
      out += B64[m >> 18] + B64[(m >> 12) & 63] + (i + 1 < b.length ? B64[(m >> 6) & 63] : '=') + '=';
    }
    return out;
  }
  function b64ToBytes(s) {
    s = String(s).replace(/[^A-Za-z0-9+/]/g, '');
    var out = new Uint8Array(Math.floor(s.length * 3 / 4)), o = 0, acc = 0, bits = 0;
    for (var i = 0; i < s.length; i++) {
      acc = (acc << 6) | B64.indexOf(s.charAt(i));
      bits += 6;
      if (bits >= 8) { bits -= 8; out[o++] = (acc >> bits) & 255; }
    }
    return out.subarray(0, o);
  }
  // flow.js's store is base64url through btoa/atob, on "binary strings"
  var g = (function () { return this; })() || {};
  if (typeof g.btoa !== 'function') {
    g.btoa = function (bin) {
      var b = new Uint8Array(bin.length);
      for (var i = 0; i < bin.length; i++) b[i] = bin.charCodeAt(i) & 255;
      return bytesToB64(b);
    };
  }
  if (typeof g.atob !== 'function') {
    g.atob = function (s) {
      var b = b64ToBytes(s), out = '';
      for (var i = 0; i < b.length; i += 0x8000) out += String.fromCharCode.apply(null, b.subarray(i, i + 0x8000));
      return out;
    };
  }

  var pending = {}, next = 0;
  function native(name, args) {
    return new Promise(function (ok, fail) {
      var id = 'n' + (++next);
      pending[id] = { ok: ok, fail: fail };
      LuntaNative.call(id, name, JSON.stringify(args === undefined ? null : args));
    });
  }
  /** The app's answer to a native request. */
  function settle(id, ok, payload) {
    var p = pending[id];
    if (!p) return;
    delete pending[id];
    var v = null;
    try { v = JSON.parse(payload); } catch (e) { /* null */ }
    if (ok) { p.ok(v); return; }
    var e = new Error((v && v.message) || 'Lunta couldn’t do that.');
    e.kind = (v && v.kind) || '';
    p.fail(e);
  }

  function lunta(path) {
    return function (body) {
      return native('lunta', { path: path, body: body }).then(function (r) {
        if (r && r.status >= 200 && r.status < 300 && r.json) return r.json;
        var e = new Error((r && r.json && r.json.error) || ('Lunta answered ' + (r && r.status) + '.'));
        e.kind = 'http';
        e.status = r && r.status;
        throw e;
      });
    };
  }

  var flow = makeLuntaGDocs({
    cite: LuntaCite, model: LuntaDocs,
    docs: makeLuntaGDocs.docsAPI(function (method, url, body) { return native('docs', { method: method, url: url, body: body || null }); }),
    lunta: { bibliography: lunta('/bibliography'), render: lunta('/render'), placeholders: lunta('/placeholders') },
    sleep: function (ms) { return native('sleep', ms); },
    gzip: function (text) { return native('gzip', text).then(b64ToBytes); },
    gunzip: function (bytes) { return native('gunzip', bytesToB64(bytes)); }
  });

  /** One of the panel's document requests (flow.op), answered through LuntaNative.done. */
  function handle(id, json) {
    var answer = function (r) { LuntaNative.done(id, JSON.stringify(r)); };
    var msg;
    try { msg = JSON.parse(json); } catch (e) { answer({ error: 'bad request', kind: 'http' }); return; }
    flow.op(msg).then(answer, function (e) {
      answer({ error: String((e && e.message) || e), kind: (e && e.kind) || '' });
    });
  }

  return { handle: handle, settle: settle, flow: flow };
})();
if (typeof module !== 'undefined') module.exports = LuntaAppHost;
