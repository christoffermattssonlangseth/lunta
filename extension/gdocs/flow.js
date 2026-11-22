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
//   deps.lunta.placeholders(body) → /api/placeholders (Resolve Citations)
//   deps.cite, deps.model: LuntaCite and LuntaDocs; deps.sleep(ms) optional
//   deps.gzip(text) → Uint8Array, deps.gunzip(bytes) → text: optional (default: the browser's
//     CompressionStream; Lunta for Mac, which runs this file for Safari, brings its own)
// The same file runs in Lunta for Mac for the extension in Safari (apple/Mac/GoogleDocsScript.swift,
// through gdocs/app-host.js), so both browsers' citations come from one implementation.
function makeLuntaGDocs(deps) {
  var C = deps.cite, D = deps.model;
  var DOC_ID = /^[A-Za-z0-9_-]{20,}$/, PLACEHOLDER_TEXT = '(…)';
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
    if (deps.gzip) return deps.gzip(text);
    var stream = new Blob([text]).stream().pipeThrough(new CompressionStream('gzip'));
    return new Uint8Array(await new Response(stream).arrayBuffer());
  }
  async function gunzip(bytes) {
    if (deps.gunzip) return deps.gunzip(bytes);
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
      options: x.parsed.items.map(function (it) { return it.optCode || ''; }),
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
      var options = parsed.items.map(function (it) { return C.decodeOptions(it.optCode); });
      var f;
      if (journal) {
        var body = { style: style.slice(4), citations: lists, papers: papers }, ropts = C.renderOptions(options, lists);
        if (ropts) body.options = ropts;
        var res = await deps.lunta.render(body);
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
        f = C.format(style, lists, got.entries, kept, options);
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
      // pasted without its link (⌘⇧V, or the link dropped): "(…)" as plain text, one more than before
      if (opts.bare != null && bareCount(doc) > opts.bare) {
        throw fail('Google Docs pasted the citation as plain text, without its link, so it can’t become a citation. ' +
          'Undo that (⌘Z) and paste with ⌘V (not Paste without formatting, ⌘⇧V).', 'plainPaste');
      }
      if (Date.now() > until) {
        throw fail('The citation didn’t reach the document. Click in the text where it should go (so the cursor blinks there) and try again.', 'notInserted');
      }
      await sleep(opts.interval || 700);
    }
  }

  /**
   * How many times the placeholder's text, "(…)", is in the document without a Lunta link: a
   * placeholder pasted as plain text is one more than when it was put on the clipboard.
   */
  function bareCount(doc) {
    var n = 0;
    function walk(list) {
      (list || []).forEach(function (el) {
        if (el.paragraph) {
          var text = (el.paragraph.elements || []).map(function (pe) {
            if (!pe.textRun) return '\u0000';
            var link = pe.textRun.textStyle && pe.textRun.textStyle.link && pe.textRun.textStyle.link.url;
            return link && link.indexOf('https://margins.local/') === 0 ? '\u0000' : (pe.textRun.content || '');
          }).join('');
          n += text.split(PLACEHOLDER_TEXT).length - 1;
        } else if (el.table) {
          (el.table.tableRows || []).forEach(function (row) { (row.tableCells || []).forEach(function (c) { walk(c.content); }); });
        }
      });
    }
    function tab(dt) {
      walk((dt.body || {}).content);
      Object.keys(dt.footnotes || {}).forEach(function (k) { walk(dt.footnotes[k].content); });
    }
    if (doc.tabs && doc.tabs.length) {
      (function each(list) { (list || []).forEach(function (t) { if (t.documentTab) tab(t.documentTab); each(t.childTabs); }); })(doc.tabs);
    } else {
      tab(doc);
    }
    return n;
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
   * touched; opts.ids: the new papers, none = the citation goes; opts.options: its options code,
   * when they change), then Update All.
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
      var reqs = [want.length
        ? { updateTextStyle: { range: r, textStyle: { link: { url: D.makeLink(want) } }, fields: 'link' } }
        : { deleteContentRange: { range: r } }];
      if (want.length && opts.options !== undefined) {
        // only the papers still in it keep options
        var code = C.encodeOptions(C.cleanOptions(C.decodeOptions(opts.options), want));
        if (code !== (it.optCode || '')) reqs = reqs.concat(D.optionRequests(it, code));
      } else if (want.length && it.optCode) {
        var kept2 = C.encodeOptions(C.cleanOptions(C.decodeOptions(it.optCode), want));
        if (kept2 !== it.optCode) reqs = reqs.concat(D.optionRequests(it, kept2));
      }
      try {
        await deps.docs.batchUpdate(docId, reqs, parsed.revisionId);
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

  // ---- Resolve Citations: placeholders typed while writing, made citations once confirmed ----

  /** The placeholders in the document's plain text (not in citations), in order: [{text, cites, kind, tabId, segmentId, start, end}]. */
  async function scanPlaceholders(docId) {
    var doc = await deps.docs.get(docId), segs = D.plainSegments(doc);
    if (!segs.length) return { placeholders: [] };
    var res = await deps.lunta.placeholders({ texts: segs.map(function (g) { return g.text; }) });
    var out = [];
    (res.placeholders || []).forEach(function (list, i) {
      var g = segs[i];
      (list || []).forEach(function (ph) {
        out.push({ text: ph.text, cites: ph.cites, kind: ph.kind, tabId: g.tabId, segmentId: g.segmentId, start: g.start + ph.start, end: g.start + ph.end });
      });
    });
    return { placeholders: out };
  }

  /** Where the placeholder is now: where it was, else the same text in the same tab and part nearest to it (text typed before it). */
  function locate(doc, ph) {
    var best = null;
    D.plainSegments(doc).forEach(function (g) {
      if ((g.tabId || '') !== (ph.tabId || '') || (g.segmentId || '') !== (ph.segmentId || '')) return;
      for (var i = g.text.indexOf(ph.text); i >= 0; i = g.text.indexOf(ph.text, i + 1)) {
        var at = g.start + i;
        if (!best || Math.abs(at - ph.start) < Math.abs(best.start - ph.start)) best = { tabId: g.tabId, segmentId: g.segmentId, start: at, end: at + ph.text.length };
      }
    });
    return best;
  }

  /**
   * The citation of opts.ids in place of opts.placeholder (as scanPlaceholders found it), with
   * opts.leftover (the parts left as written) after it, then Update All.
   */
  async function writePlaceholder(docId, opts) {
    var ph = opts.placeholder || {}, ids = C.unique(opts.ids || []);
    if (!ph.text || !ids.length) throw fail('Nothing to cite.', 'http');
    for (var attempt = 0; ; attempt++) {
      var doc = await deps.docs.get(docId), at = locate(doc, ph);
      if (!at) throw fail('“' + ph.text + '” isn’t in the document any more (edited meanwhile?). Click Resolve Citations to look again.', 'moved');
      try {
        await deps.docs.batchUpdate(docId, D.placeholderRequests(at, ids, opts.leftover || ''), doc.revisionId);
        break;
      } catch (e) {
        if (e.kind !== 'revision') throw e;
        if (attempt < 3) { await sleep(300 * (attempt + 1)); continue; }
        throw fail('The document kept changing (someone typing?). Try again.', 'revision');
      }
    }
    try {
      return await update(docId, opts);
    } catch (e) {
      if (e.kind !== 'unknown') e.message += ' The citation is in the document as “(…)”; Update All fills it in.';
      throw e;
    }
  }

  // ---- converting Zotero's citations ----

  /**
   * Zotero's citations in the document: {cites: [{key, works, text, source}], bibs: [key],
   * unreadable, paperpile}. Their codes are in the named ranges "Z_F<key>…" (common.js reads them).
   */
  async function scanZotero(docId) {
    var doc = await deps.docs.get(docId), parsed = D.read(doc);
    var names = parsed.zoteroNames.map(function (n) { return n.name; });
    var seen = {}, out = { cites: [], bibs: [], unreadable: 0, paperpile: paperpileCount(doc) };
    parsed.zotero.forEach(function (z) {
      if (seen[z.key]) return;
      seen[z.key] = true;
      var code = C.zoteroDocsCode(C.zoteroDocsJoin(names, z.key));
      if (!code) out.unreadable++;
      else if (code.kind === 'bib') out.bibs.push(z.key);
      else out.cites.push({ key: z.key, works: code.works, text: z.text, source: 'Zotero' });
    });
    return out;
  }

  function paperpileCount(doc) {
    var n = 0;
    (function walk(x) {
      if (!x || typeof x !== 'object') return;
      if (x.link && typeof x.link.url === 'string' && x.link.url.indexOf('https://paperpile.com/c/') === 0) n++;
      Object.keys(x).forEach(function (k) { if (typeof x[k] === 'object') walk(x[k]); });
    })(doc);
    return n;
  }

  /**
   * Makes Zotero's citations Lunta's: opts.cites [{key, ids, options (code)}], opts.bibs (Zotero
   * bibliographies to replace: they become Lunta's if the document has none, else they go), in one
   * batchUpdate for the revision read; then Update All.
   */
  async function convertZotero(docId, opts) {
    var cites = {};
    (opts.cites || []).forEach(function (c) { if (c && c.key && c.ids && c.ids.length) cites[String(c.key)] = { ids: c.ids.map(String), code: String(c.options || '') }; });
    var converted = 0;
    for (var attempt = 0; ; attempt++) {
      var doc = await deps.docs.get(docId), parsed = D.read(doc);
      var p = D.zoteroRequests(parsed, cites, opts.bibs || [], !parsed.bibliography);
      converted = p.converted;
      if (!p.requests.length) break;
      try {
        await deps.docs.batchUpdate(docId, p.requests, parsed.revisionId);
        break;
      } catch (e) {
        if (e.kind !== 'revision') throw e;
        if (attempt < 3) { await sleep(300 * (attempt + 1)); continue; }
        throw fail('The document kept changing (someone typing?). Try again.', 'revision');
      }
    }
    var out = await update(docId, opts);
    out.converted = converted;
    return out;
  }

  // ---- Check Citations: the citations and their paragraphs (Lunta's /claims checks them) ----

  /** {sites: LuntaCite.claimSites(…), papers: the details the document keeps of the cited papers}. */
  async function claimSites(docId) {
    var doc = await deps.docs.get(docId), store = await decodeStore(D.read(doc).storeNames);
    var sites = C.claimSites(D.claimParagraphs(doc));
    var ids = C.unique([].concat.apply([], sites.map(function (s) { return s.ids; })));
    return { sites: sites, papers: (store && store.papers) ? C.detailsFor(store.papers, ids) : {} };
  }

  /**
   * The document requests of the panel ({op, docId, …}, see gdocs/panel.js), the same in every
   * browser: the background script answers them in Chrome, Edge and Firefox, Lunta for Mac in Safari.
   */
  async function op(msg) {
    if (!msg || !DOC_ID.test(String(msg.docId))) throw fail('This isn’t a Google Docs document.', 'http');
    var style = { style: msg.style, styleTitle: msg.styleTitle };
    switch (msg.op) {
      case 'state':
        return state(msg.docId);
      case 'update':
        return update(msg.docId, style);
      case 'prepareInsert': {
        // nothing pasted unless the document can be written afterwards
        var doc = await deps.docs.get(msg.docId), parsed = D.read(doc);
        if (!msg.ids && parsed.bibliography) return { exists: true };
        var token = D.newToken(), ids = msg.ids ? msg.ids.map(String) : null;
        return { token: token, html: D.placeholderHTML(token, ids, PLACEHOLDER_TEXT), url: D.placeholderUrl(token, ids),
                 text: PLACEHOLDER_TEXT, plain: PLACEHOLDER_TEXT, bare: bareCount(doc) };
      }
      case 'finishInsert':
        return finishInsert(msg.docId, msg.token, Object.assign(style, {
          // a short wait, asked again and again (Safari: until the reader pastes), or the usual 15 s
          timeout: msg.timeout ? Math.min(Math.max(Number(msg.timeout) || 0, 1000), 30000) : undefined,
          bare: msg.bare == null ? null : Number(msg.bare)
        }));
      case 'details':
        return details(msg.docId, (msg.ids || []).map(String));
      case 'editCitation':
        return editCitation(msg.docId, Object.assign(style, { index: Number(msg.index), expect: msg.expect || [], ids: (msg.ids || []).map(String),
          options: msg.options === undefined ? undefined : String(msg.options || '') }));
      case 'scanZotero':
        return scanZotero(msg.docId);
      case 'convertZotero':
        return convertZotero(msg.docId, Object.assign(style, { cites: msg.cites || [], bibs: (msg.bibs || []).map(String) }));
      case 'scanPlaceholders':
        return scanPlaceholders(msg.docId);
      case 'claimSites':
        return claimSites(msg.docId);
      case 'writePlaceholder': {
        var p = msg.placeholder || {};
        return writePlaceholder(msg.docId, Object.assign(style, {
          placeholder: { text: String(p.text || ''), tabId: p.tabId || undefined, segmentId: p.segmentId || undefined, start: Number(p.start), end: Number(p.end) },
          ids: (msg.ids || []).map(String), leftover: String(msg.leftover || '')
        }));
      }
      default:
        throw fail('unknown request', 'http');
    }
  }

  return { claimSites: claimSites, state: state, update: update, finishInsert: finishInsert, details: details, editCitation: editCitation, scanZotero: scanZotero, convertZotero: convertZotero,
           scanPlaceholders: scanPlaceholders, writePlaceholder: writePlaceholder,
           decodeStore: decodeStore, encodeStore: encodeStore, bareCount: bareCount, op: op };
}

/**
 * The Google Docs API (documents.get with tabs, documents.batchUpdate) and its errors, said so the
 * reader knows what to do. `send(method, url, body)` makes the request with the Google token and
 * answers {status, json} (a 401 already retried with a new token); it rejects only when Google
 * can't be reached, or with an error that has its own `kind` (no token: "auth", "setup").
 */
makeLuntaGDocs.docsAPI = function (send) {
  var API = 'https://docs.googleapis.com/v1/documents/';
  function fail(message, kind) { var e = new Error(message); e.kind = kind; return e; }
  async function call(method, path, body) {
    var r;
    try {
      r = await send(method, API + path, body);
    } catch (e) {
      if (e && e.kind) throw e;
      throw fail('Google Docs couldn’t be reached. Check the internet connection and try again.', 'network');
    }
    var j = r.json || {};
    if (r.status >= 200 && r.status < 300) return j;
    var msg = (j.error && j.error.message) || ('Google Docs answered ' + r.status + '.');
    if (r.status === 400 && /revision/i.test(msg)) throw fail(msg, 'revision');
    if (r.status === 401) throw fail('Connect your Google account to cite in this document.', 'auth');
    if (r.status === 403 || r.status === 404) {
      throw fail('The Google account connected to Lunta can’t edit this document (another account owns it, or you can only view or comment). ' +
        'Use Switch account in Lunta’s panel, or ask for edit access.', 'access');
    }
    throw fail('Google Docs: ' + msg, 'http');
  }
  return {
    get: function (docId) { return call('GET', encodeURIComponent(docId) + '?includeTabsContent=true'); },
    batchUpdate: function (docId, requests, revisionId) {
      return call('POST', encodeURIComponent(docId) + ':batchUpdate', { requests: requests, writeControl: { requiredRevisionId: revisionId } });
    }
  };
};
if (typeof module !== 'undefined') module.exports = makeLuntaGDocs;
