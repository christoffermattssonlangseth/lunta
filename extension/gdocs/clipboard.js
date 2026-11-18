// Inserting at the cursor in Safari, for the panel (gdocs/panel.js). Safari doesn't let the panel
// paste into Google Docs itself (Chrome and Firefox: a synthetic paste), so the placeholder, the
// same linked "(…)" as the paste would bring, goes on the clipboard and the reader pastes it with
// ⌘V; the app (Lunta for Mac) waits for it to reach the document and makes it the citation.
// Tested in node with stand-ins: apple/citations-tests/gdocs-safari.test.js.
// eslint-disable-next-line no-unused-vars
var LuntaClipboard = (function () {
  /**
   * Puts the placeholder on the clipboard. Called in the click itself (Safari writes the clipboard
   * only in answer to one), with `prep` still on its way: Safari takes promises in a ClipboardItem.
   * `prep` resolves to prepareInsert's answer ({html, plain} or {exists} / {error}: nothing copied).
   * Resolves when it is on the clipboard.
   */
  function copy(clipboard, ClipboardItemType, prep) {
    var part = function (type, key) {
      var out = Promise.resolve(prep).then(function (p) {
        if (!p || p.error || p.exists || !p[key]) throw new Error('nothing to copy');
        return new Blob([p[key]], { type: type });
      });
      out.catch(function () {});   // said through write()'s answer
      return out;
    };
    if (!clipboard || typeof clipboard.write !== 'function' || typeof ClipboardItemType !== 'function') {
      return Promise.reject(new Error('This browser doesn’t let extensions write the clipboard.'));
    }
    try {
      return clipboard.write([new ClipboardItemType({ 'text/html': part('text/html', 'html'), 'text/plain': part('text/plain', 'plain') })]);
    } catch (e) {
      return Promise.reject(e);
    }
  }

  /**
   * Waits for the pasted placeholder: asks the app (finishInsert, a few seconds at a time) until
   * it has arrived and become the citation, or the reader cancels, or `opts.limit` ms pass.
   * `send` is the panel's request to the extension; `onPlain(message)` when Docs pasted it as
   * plain text (it keeps waiting: undo and paste again works). Resolves to finishInsert's answer,
   * or {cancelled: true}; rejects with finishInsert's error (err.kind) otherwise.
   */
  async function wait(send, opts) {
    var until = Date.now() + (opts.limit || 180000), plainShown = false;
    for (;;) {
      if (opts.cancelled && opts.cancelled()) return { cancelled: true };
      var r = await send(Object.assign({ op: 'finishInsert', docId: opts.docId, token: opts.token, bare: opts.bare, timeout: opts.step || 4000 }, opts.extra || {}));
      if (opts.cancelled && opts.cancelled()) return { cancelled: true };
      if (!r || !r.error) return r;
      if (r.kind === 'plainPaste') {
        if (!plainShown && opts.onPlain) opts.onPlain(r.error);
        plainShown = true;
        if (opts.pause) await opts.pause();   // finishInsert answered at once
      } else if (r.kind !== 'notInserted') {
        var e = new Error(r.error);
        e.kind = r.kind;
        throw e;
      }
      if (Date.now() > until) {
        var late = new Error('The citation didn’t reach the document. Click Insert again, then press ⌘V in the document where it goes.');
        late.kind = 'notInserted';
        throw late;
      }
    }
  }

  return { copy: copy, wait: wait };
})();
if (typeof module !== 'undefined') module.exports = LuntaClipboard;
