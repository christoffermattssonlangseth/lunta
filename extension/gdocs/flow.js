// What the extension does in a Google Doc, step by step: read it through the Docs API, ask Lunta
// for the formatted citations (the same requests the Apps Script sidebar makes), work out the text
// with the shared citation code (citations-common.js, a copy of apple/citations/common.js) and
// write it in one batchUpdate that applies only to the revision it was worked out for (someone
// typing meanwhile: start over). Runs in the background script; `deps` are the Docs API and Lunta,
// so tests can stand in for both (apple/citations-tests/extension-gdocs.test.js).
//   deps.docs.get(docId) → documents.get (with tabs content)
//   deps.docs.batchUpdate(docId, requests, revisionId) → rejects with err.kind "revision" if the
//     document changed since that revision
//   deps.lunta.bibliography(body), deps.lunta.render(body) → Lunta's /api/bibliography, /api/render
//   deps.cite, deps.model: LuntaCite and LuntaDocs; deps.sleep(ms) optional
function makeLuntaGDocs(deps) {
  var C = deps.cite, D = deps.model;
  var sleep = deps.sleep || function (ms) { return new Promise(function (ok) { setTimeout(ok, ms); }); };

  function fail(message, kind, extra) {
    var e = new Error(message);
    e.kind = kind;
    if (extra) Object.assign(e, extra);
    return e;
  }

  // ---- the store: gzipped JSON in base64url (as Apps Script's Utilities writes it) ----

  function toB64url(bytes) {
    var s = '';
    for (var i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode.apply(null, bytes.subarray(i, i + 0x8000));
    return btoa(s).replace(/\+/g, '-').replace(/\//g, '_');
  }
  function fromB64url(s) {
    var bin = atob(s.replace(/-/g, '+').replace(/_/g, '/'));
    var out = new Uint8Array(bin.length);
    for (var i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
    return out;
  }
  async function gzip(text) {
    var stream = new Blob([text]).stream().pipeThrough(new CompressionStream('gzip'));
    return new Uint8Array(await new Response(stream).arrayBuffer());
  }
  async function gunzip(bytes) {
    var stream = new Blob([bytes]).stream().pipeThrough(new DecompressionStream('gzip'));
    return new Response(stream).text();
  }

  /** The newest complete store among the document's store ranges, or null. */
  async function decodeStore(names) {
    var best = null;
    for (var g of D.storeGenerations(names)) {
      try {
        var j = JSON.parse(await gunzip(fromB64url(g.b64)));
        if (j && typeof j === 'object' && (!best || (j.at || 0) > (best.at || 0))) best = j;
      } catch (e) { /* a damaged one: the others */ }
    }
    return best;
  }

  /** Range names for a store; without the saved entries if too large; null if it still doesn't fit. */
  async function encodeStore(store) {
    var gen = D.newGeneration();
    var names = D.storeNames(toB64url(await gzip(JSON.stringify(store))), gen);
    if (names) return names;
    var lean = Object.assign({}, store, { papers: {} });
    Object.keys(store.papers || {}).forEach(function (id) {
      var r = store.papers[id];
      lean.papers[id] = { m: r.m, f: r.f, y: r.y, t: r.t, e: {} };
    });
    return D.storeNames(toB64url(await gzip(JSON.stringify(lean))), gen);
  }

  async function load(docId) {
    var doc = await deps.docs.get(docId);
    var parsed = D.read(doc);
    return { parsed: parsed, store: await decodeStore(parsed.storeNames) };
  }

  /** What the panel shows: the document's citations, style and bibliography. */
  async function state(docId) {
    var x = await load(docId), s = x.store;
    return {
      title: x.parsed.title,
      citations: x.parsed.items.map(function (it) { return it.ids; }),
      texts: x.parsed.items.map(function (it) { return it.text; }),
      style: (s && s.style) || '', styleTitle: (s && s.styleTitle) || '',
      hasBibliography: !!x.parsed.bibliography,
      pending: x.parsed.items.filter(function (it) { return it.kind !== 'cite'; }).length + x.parsed.bibPlaceholders.length
    };
  }

  /** Placeholders made into citations as they are (Lunta couldn't format them now). */
  async function settlePlaceholders(docId, parsed) {
    if (!parsed.items.some(function (it) { return it.kind !== 'cite'; }) && !parsed.bibPlaceholders.length) return;
    var p = D.planRequests(parsed, { texts: null, superscripts: null, bibliography: null, store: null });
    if (p.requests.length) await deps.docs.batchUpdate(docId, p.requests, parsed.revisionId).catch(function () {});
  }

  /**
   * Update All: every citation, the bibliography and the document's store. opts.style (and
   * styleTitle for a journal style): a new style; otherwise the document's own. Returns
   * {count, references, updated, bibliography, saved, style, styleTitle, summary}.
   */
  async function update(docId, opts) {
    opts = opts || {};
    for (var attempt = 0; ; attempt++) {
      var x = await load(docId), parsed = x.parsed, store = x.store;
      var style = opts.style || (store && store.style) || '';
      if (!style) {
        // never rewrite a document in a guessed style; a new one starts in APA
        if (parsed.items.some(function (it) { return it.kind === 'cite'; }) || parsed.bibliography) {
          throw fail('Choose this document’s citation style in the menu above first: Lunta didn’t find it saved in the document ' +
            '(written with an older copy of the Apps Script?). Nothing was changed.', 'style');
        }
        style = 'apa';
      }
      if (!C.isStyle(style)) throw fail('Unknown citation style: ' + style, 'style');
      var journal = C.isJournal(style);
      var styleTitle = journal ? (opts.styleTitle || (store && store.style === style ? store.styleTitle : '') || '') : '';
      var lists = parsed.items.map(function (it) { return it.ids; });
      var ids = C.citedIds(lists), kept = (store && store.papers) || {}, papers = C.detailsFor(kept, ids), offline = false;
      var f;
      if (journal) {
        var res = await deps.lunta.render({ style: style.slice(4), citations: lists, papers: papers });
        styleTitle = res.title || styleTitle;
        f = C.formatJournal(res, lists, kept);
      } else {
        var got = ids.length
          ? await deps.lunta.bibliography({ style: style, ids: ids, papers: papers }).catch(function (e) {
            if (e.kind !== 'offline') throw e;
            offline = true;
            return { entries: null };
          })
          : { entries: [] };
        f = C.format(style, lists, got.entries, kept);
        f.superscripts = lists.map(function () { return f.superscript; });
      }
      if (f.unknown.length) {
        await settlePlaceholders(docId, parsed);
        var n = f.unknown.length;
        if (offline) throw fail(C.unknownMessage(n, true), 'unknown');
        var first = parsed.items.filter(function (it) { return it.ids.some(function (id) { return f.unknown.indexOf(id) >= 0; }); })[0];
        throw fail((n === 1 ? '1 cited paper isn’t' : n + ' cited papers aren’t') + ' in your Lunta library, and this document has no copy of ' +
          (n === 1 ? 'it' : 'them') + ' (deleted from the library, or the document was written with another library). Nothing was changed. ' +
          (first ? 'The first is the citation “' + first.text + '”: delete it and cite the paper again, or add the paper back to Lunta.' : ''), 'unknown');
      }
      var wantBib = !!parsed.bibliography || parsed.bibPlaceholders.length > 0;
      var next = { v: 1, at: Date.now(), style: style, styleTitle: styleTitle, bibStyle: style, papers: f.store };
      var names = null, saved = true;
      if (!D.sameStore(store, next)) {
        names = await encodeStore(next);
        if (!names) saved = false;
      }
      var p = D.planRequests(parsed, {
        texts: f.texts, superscripts: f.superscripts,
        bibliography: wantBib ? f.bibliography.map(function (e) { return C.runs(e.text); }) : null,
        forceBibliography: !!parsed.bibliography && ((store && store.bibStyle) || '') !== style,
        store: names
      });
      if (p.requests.length) {
        try {
          await deps.docs.batchUpdate(docId, p.requests, parsed.revisionId);
        } catch (e) {
          if (e.kind !== 'revision') throw e;
          if (attempt < 3) { await sleep(300 * (attempt + 1)); continue; }
          throw fail('The document kept changing while updating (someone typing?). Click Update All again.', 'revision');
        }
      }
      var name = journal ? (f.title || C.styleName(style, styleTitle)) : C.styleName(style);
      return {
        count: parsed.items.length, references: f.references, updated: p.updated, bibliography: p.bibliography, saved: saved,
        style: style, styleTitle: styleTitle,
        summary: C.summary(parsed.items.length, f.references, name, f.fromDocument, offline) +
          (saved ? '' : ' · (too many papers to keep a copy in the document)')
      };
    }
  }

  /** Waits for a pasted placeholder to reach the document (Docs saves it within a second or two), then updates. */
  async function finishInsert(docId, token, opts) {
    opts = opts || {};
    var until = Date.now() + (opts.timeout || 15000);
    for (;;) {
      var doc = await deps.docs.get(docId), parsed = D.read(doc);
      var here = parsed.items.some(function (it) { return it.placeholder && it.placeholder.token === token; }) ||
        parsed.bibPlaceholders.some(function (b) { return b.token === token; });
      if (here) {
        var bad = parsed.bibPlaceholders.filter(function (b) { return b.token === token && !b.paragraph; })[0];
        try {
          var r = await update(docId, opts);
          if (bad) r.summary = 'The bibliography goes in the main text (not a footnote or a table): click there and try again.';
          return r;
        } catch (e) {
          if (e.kind !== 'unknown') {
            await settlePlaceholders(docId, D.read(await deps.docs.get(docId))).catch(function () {});
            e.message += ' The citation is in the document; Update All fills it in.';
          }
          throw e;
        }
      }
      if (Date.now() > until) {
        throw fail('The citation didn’t reach the document. Click in the text where it should go (so the cursor blinks there) and try again.', 'notInserted');
      }
      await sleep(opts.interval || 700);
    }
  }

  /** The character at a document index (null past the text), in a tab's body or a footnote. */
  function charAt(doc, tabId, segmentId, index) {
    var tabs = [];
    (function walk(list) { (list || []).forEach(function (t) { tabs.push(t); walk(t.childTabs); }); })(doc.tabs);
    var tab = tabs.filter(function (t) { return !tabId || t.tabProperties && t.tabProperties.tabId === tabId; })[0];
    var dt = tab ? tab.documentTab : doc;
    if (!dt) return null;
    var content = segmentId ? ((dt.footnotes || {})[segmentId] || {}).content : (dt.body || {}).content;
    var found = null;
    (function walk(list) {
      (list || []).forEach(function (el) {
        if (found !== null) return;
        if (el.paragraph) {
          (el.paragraph.elements || []).forEach(function (pe) {
            var t = pe.textRun && pe.textRun.content;
            if (found === null && t && index >= pe.startIndex && index < pe.startIndex + t.length) found = t.charAt(index - pe.startIndex);
          });
        } else if (el.table) {
          (el.table.tableRows || []).forEach(function (row) { (row.tableCells || []).forEach(function (c) { walk(c.content); }); });
        }
      });
    })(content);
    return found === '\n' ? null : found;
  }

  /** The papers of a citation, for the panel: from Lunta, else from the document's copy. */
  async function details(docId, ids) {
    var x = await load(docId), kept = (x.store && x.store.papers) || {};
    try {
      var got = await deps.lunta.bibliography({ style: 'apa', ids: ids, papers: C.detailsFor(kept, ids) });
      return { entries: got.entries || [] };
    } catch (e) {
      if (e.kind !== 'offline') throw e;
      return { entries: ids.map(function (id) {
        var r = kept[id];
        return r ? { id: id, title: r.t, families: r.f, authors: r.f, year: r.y } : { id: id, missing: true };
      }) };
    }
  }

  /**
   * Changes the papers of one citation (opts.index: its place among the document's citations;
   * opts.expect: its papers as the panel showed them, so a citation that changed meanwhile isn't
   * touched; opts.ids: the new papers, none = the citation goes), then Update All.
   */
  async function editCitation(docId, opts) {
    var want = C.unique(opts.ids || []), expect = C.unique(opts.expect || []);
    for (var attempt = 0; ; attempt++) {
      var doc = await deps.docs.get(docId), parsed = D.read(doc);
      var it = parsed.items[opts.index];
      if (!it || it.kind !== 'cite' || it.ids.join(',') !== expect.join(',')) {
        throw fail('That citation changed or moved in the meantime. Click Edit Citation again.', 'moved');
      }
      var r = { startIndex: it.start, endIndex: it.end };
      if (it.segmentId) r.segmentId = it.segmentId;
      if (it.tabId) r.tabId = it.tabId;
      if (!want.length) {
        // "as shown (Lee, 2020)." → "as shown." : the space before goes too, when punctuation,
        // a space or the paragraph's end follows
        var before = charAt(doc, it.tabId, it.segmentId, it.start - 1), after = charAt(doc, it.tabId, it.segmentId, it.end);
        if (before === ' ' && (after === null || /[\s.,;:!?)\]]/.test(after))) r.startIndex -= 1;
      }
      var req = want.length
        ? { updateTextStyle: { range: r, textStyle: { link: { url: D.makeLink(want) } }, fields: 'link' } }
        : { deleteContentRange: { range: r } };
      try {
        await deps.docs.batchUpdate(docId, [req], parsed.revisionId);
        break;
      } catch (e) {
        if (e.kind !== 'revision') throw e;
        if (attempt < 3) { await sleep(300 * (attempt + 1)); continue; }
        throw fail('The document kept changing (someone typing?). Try again.', 'revision');
      }
    }
    var out = await update(docId, opts);
    out.removed = !want.length;
    return out;
  }

  return { state: state, update: update, finishInsert: finishInsert, details: details, editCitation: editCitation,
           decodeStore: decodeStore, encodeStore: encodeStore };
}
if (typeof module !== 'undefined') module.exports = makeLuntaGDocs;
