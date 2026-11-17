// Lunta citations in Google Docs through the Google Docs API (the browser extension's way; the Apps
// Script sidebar, apple/citations/gdocs, is the other). Plain functions: reading what
// documents.get answers, and the batchUpdate requests that write citations, the bibliography and
// the document's store. Documents stay the same either way:
//   - a citation is text linked to https://margins.local/cite?ids=<id>,<id> (escaped);
//   - bibliography paragraphs are linked to https://margins.local/bib;
//   - the style and the cited papers' details are kept in named ranges over the first tab's text,
//     "lunta:<generation>:<i>:<n>:<base64url of gzipped JSON>" (Code.gs reads and writes them too).
// Inserting goes through Docs' own paste (the API doesn't know where the cursor is): a placeholder
// linked to https://margins.local/insert?t=<token>&ids=… (or &bib=1) is pasted at the cursor, then
// found here and made a citation (or the bibliography). A placeholder left behind (the browser
// closed meanwhile) isn't a citation for the Apps Script and becomes one at the next Update All.
// Tested in node: apple/citations-tests/extension-gdocs.test.js.
var LuntaDocs = (function () {
  var CITE = 'https://margins.local/cite?ids=', BIB = 'https://margins.local/bib';
  var INSERT = 'https://margins.local/insert?';
  var STORE_PREFIX = 'lunta:', STORE_CHUNK = 220, STORE_MAX = 400;

  function unique(list) {
    var seen = {}, out = [];
    (list || []).forEach(function (x) {
      x = String(x == null ? '' : x).trim();
      if (x && !seen.hasOwnProperty(x)) { seen[x] = true; out.push(x); }
    });
    return out;
  }

  function parseLink(url) {
    if (typeof url !== 'string' || url.indexOf(CITE) !== 0) return null;
    var raw = url.slice(CITE.length);
    try { raw = decodeURIComponent(raw); } catch (e) {}
    return unique(raw.split(','));
  }
  function makeLink(ids) { return CITE + encodeURIComponent(unique(ids).join(',')); }

  // ---- placeholders ----

  function placeholderUrl(token, ids) {
    return INSERT + 't=' + encodeURIComponent(token) + (ids ? '&ids=' + encodeURIComponent(unique(ids).join(',')) : '&bib=1');
  }

  /** {token, ids, bib} of a placeholder's link, or null. */
  function parsePlaceholder(url) {
    if (typeof url !== 'string' || url.indexOf(INSERT) !== 0) return null;
    var out = { token: '', ids: null, bib: false };
    url.slice(INSERT.length).split('&').forEach(function (kv) {
      var i = kv.indexOf('='), k = i < 0 ? kv : kv.slice(0, i), v = i < 0 ? '' : kv.slice(i + 1);
      try { v = decodeURIComponent(v); } catch (e) {}
      if (k === 't') out.token = v;
      else if (k === 'ids') out.ids = unique(v.split(','));
      else if (k === 'bib') out.bib = true;
    });
    if (!out.token || (!out.bib && !(out.ids && out.ids.length))) return null;
    return out;
  }

  /** The HTML pasted at the cursor: the placeholder's text, linked. */
  function placeholderHTML(token, ids, text) {
    var t = String(text || '(…)').replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
    return '<a href="' + placeholderUrl(token, ids).replace(/&/g, '&amp;').replace(/"/g, '&quot;') + '">' + t + '</a>';
  }

  function newToken() {
    var s = '';
    for (var i = 0; i < 12; i++) s += 'abcdefghijklmnopqrstuvwxyz0123456789'.charAt(Math.floor(Math.random() * 36));
    return s;
  }

  // ---- reading a document ----

  /** The document's tabs in order (child tabs after their parent): [{tabId, documentTab}]. */
  function tabsOf(doc) {
    if (!doc.tabs || !doc.tabs.length) {
      return [{ tabId: undefined, documentTab: { body: doc.body || { content: [] }, footnotes: doc.footnotes || {}, namedRanges: doc.namedRanges || {} } }];
    }
    var out = [];
    (function walk(list) {
      (list || []).forEach(function (t) {
        if (t.documentTab) out.push({ tabId: t.tabProperties && t.tabProperties.tabId, documentTab: t.documentTab });
        walk(t.childTabs);
      });
    })(doc.tabs);
    return out;
  }

  function linkOf(run) {
    var st = run.textStyle || {};
    return st.link && st.link.url ? st.link.url : null;
  }

  /**
   * Everything Lunta needs of a documents.get answer (with includeTabsContent):
   *   items:          citations in document order (tab by tab, footnotes where their marks are),
   *                   placeholders included: {tabId, segmentId, start, end, text, ids, url, sups,
   *                   kind: "cite" | "new" (a placeholder) | "merged" (a placeholder inside a
   *                   citation: the whole citation, its papers and the new ones), placeholder}
   *   bibliography:   {tabId, bodyEnd, paragraphs: [{start, end, text}]} of the first tab that has one, or null
   *   bibPlaceholders: [{tabId, segmentId, start, end, token, paragraph: {start, end} | null (not in the main text), bodyEnd}]
   *   storeNames:     the names of the first tab's store ranges
   *   firstTab:       {tabId, bodyEnd}
   */
  function read(doc) {
    var tabs = tabsOf(doc);
    var items = [], bibPlaceholders = [], bibliography = null;
    tabs.forEach(function (tab) {
      var dt = tab.documentTab, content = (dt.body && dt.body.content) || [];
      var bodyEnd = content.length ? content[content.length - 1].endIndex : 1;
      var footnotes = dt.footnotes || {};

      function walkContent(list, segmentId, topLevel) {
        (list || []).forEach(function (el) {
          if (el.paragraph) paragraph(el, segmentId, topLevel);
          else if (el.table) {
            (el.table.tableRows || []).forEach(function (row) {
              (row.tableCells || []).forEach(function (cell) { walkContent(cell.content, segmentId, false); });
            });
          }
          // a table of contents only links to headings
        });
      }

      function paragraph(el, segmentId, topLevel) {
        var runs = [];
        var flush = function () { if (runs.length) { collect(runs, tab.tabId, segmentId, topLevel ? { start: el.startIndex, end: el.endIndex } : null, bodyEnd); runs = []; } };
        (el.paragraph.elements || []).forEach(function (pe) {
          if (pe.textRun) {
            var text = pe.textRun.content || '';
            if (text === '\n' || !text) return;
            if (text.charAt(text.length - 1) === '\n') text = text.slice(0, -1);
            runs.push({ start: pe.startIndex, end: pe.startIndex + text.length, url: linkOf(pe.textRun), text: text,
              sup: (pe.textRun.textStyle || {}).baselineOffset === 'SUPERSCRIPT' });
          } else if (pe.footnoteReference) {
            // the citations before the mark, then the footnote's own
            flush();
            var fn = footnotes[pe.footnoteReference.footnoteId];
            if (fn) walkContent(fn.content, pe.footnoteReference.footnoteId, false);
          } else {
            // an image, a person chip…: not text, and nothing joins across it
            flush();
          }
        });
        flush();
      }

      function collect(runs, tabId, segmentId, para, bEnd) {
        // one link split into runs (a word made bold): one
        var rs = [];
        runs.forEach(function (r) {
          var last = rs[rs.length - 1];
          if (last && last.url && last.url === r.url && last.end === r.start) {
            last.end = r.end; last.text += r.text; last.sups.push(r.sup);
          } else rs.push({ start: r.start, end: r.end, url: r.url, text: r.text, sups: [r.sup] });
        });
        for (var i = 0; i < rs.length; i++) {
          var r = rs[i], ids = parseLink(r.url), ph = ids ? null : parsePlaceholder(r.url);
          var base = { tabId: tabId, segmentId: segmentId };
          if (ph && ph.bib) {
            bibPlaceholders.push({ tabId: tabId, segmentId: segmentId, start: r.start, end: r.end, token: ph.token, paragraph: segmentId ? null : para, bodyEnd: bEnd });
            continue;
          }
          if (ids && ids.length) {
            var mid = rs[i + 1], right = rs[i + 2];
            var midPh = mid && mid.start === r.end ? parsePlaceholder(mid.url) : null;
            if (midPh && midPh.ids && right && right.url === r.url && right.start === mid.end) {
              // pasted inside a citation (which Docs split in two): its papers and the new ones, as one
              var all = unique(ids.concat(midPh.ids));
              items.push(Object.assign(base, { start: r.start, end: right.end, text: r.text + mid.text + right.text, ids: all, url: makeLink(all),
                sups: r.sups.concat(mid.sups, right.sups), kind: 'merged', placeholder: { start: mid.start, end: mid.end, token: midPh.token } }));
              i += 2;
              continue;
            }
            items.push(Object.assign(base, { start: r.start, end: r.end, text: r.text, ids: ids, url: r.url, sups: r.sups, kind: 'cite', placeholder: null }));
          } else if (ph && ph.ids) {
            items.push(Object.assign(base, { start: r.start, end: r.end, text: r.text, ids: ph.ids, url: makeLink(ph.ids), sups: r.sups, kind: 'new',
              placeholder: { start: r.start, end: r.end, token: ph.token } }));
          }
        }
      }

      walkContent(content, undefined, true);

      if (!bibliography) {
        var paras = [];
        content.forEach(function (el) {
          if (!el.paragraph) return;
          var text = '', first = null, last = null;
          (el.paragraph.elements || []).forEach(function (pe) {
            if (!pe.textRun) return;
            var t = pe.textRun.content || '';
            if (t.charAt(t.length - 1) === '\n') t = t.slice(0, -1);
            if (!t) return;
            if (first === null) first = linkOf(pe.textRun);
            last = linkOf(pe.textRun);
            text += t;
          });
          if (text && (first === BIB || last === BIB)) paras.push({ start: el.startIndex, end: el.endIndex, text: text });
        });
        if (paras.length) bibliography = { tabId: tab.tabId, bodyEnd: bodyEnd, paragraphs: paras };
      }
    });

    var first = tabs[0], storeNames = [];
    var nr = (first && first.documentTab.namedRanges) || {};
    Object.keys(nr).forEach(function (name) { if (storePart(name)) storeNames.push(name); });
    var fc = (first && first.documentTab.body && first.documentTab.body.content) || [];
    return {
      revisionId: doc.revisionId, title: doc.title,
      items: items, bibliography: bibliography, bibPlaceholders: bibPlaceholders, storeNames: storeNames,
      firstTab: { tabId: first ? first.tabId : undefined, bodyEnd: fc.length ? fc[fc.length - 1].endIndex : 1 }
    };
  }

  // ---- the document's store ----

  function storePart(name) {
    var m = /^lunta:([a-z0-9]+):(\d+):(\d+):([A-Za-z0-9_\-=]*)$/.exec(String(name || ''));
    return m ? { gen: m[1], i: +m[2], n: +m[3], data: m[4] } : null;
  }

  /** The complete generations among the store's names, as base64url strings: [{gen, b64}]. */
  function storeGenerations(names) {
    var gens = {};
    (names || []).forEach(function (name) {
      var p = storePart(name);
      if (!p) return;
      var g = gens[p.gen] = gens[p.gen] || { n: p.n, parts: {} };
      if (p.n === g.n && p.i < p.n) g.parts[p.i] = p.data;
    });
    var out = [];
    Object.keys(gens).forEach(function (k) {
      var g = gens[k], b64 = '';
      for (var i = 0; i < g.n; i++) { if (g.parts[i] === undefined) return; b64 += g.parts[i]; }
      out.push({ gen: k, b64: b64 });
    });
    return out;
  }

  /** Names for a store's base64url text (null when it is too large). */
  function storeNames(b64, gen) {
    var n = Math.max(1, Math.ceil(b64.length / STORE_CHUNK));
    if (n > STORE_MAX) return null;
    var out = [];
    for (var i = 0; i < n; i++) out.push(STORE_PREFIX + gen + ':' + i + ':' + n + ':' + b64.slice(i * STORE_CHUNK, (i + 1) * STORE_CHUNK));
    return out;
  }

  function newGeneration() {
    return (Date.now().toString(36) + Math.floor(Math.random() * 1e6).toString(36)).slice(-10);
  }

  function sameStore(a, b) {
    if (!a || !b) return false;
    var strip = function (x) { var c = {}; Object.keys(x).forEach(function (k) { if (k !== 'at') c[k] = x[k]; }); return JSON.stringify(c); };
    return strip(a) === strip(b);
  }

  // ---- writing ----

  function loc(tabId, segmentId, index) {
    var l = { index: index };
    if (segmentId) l.segmentId = segmentId;
    if (tabId) l.tabId = tabId;
    return l;
  }
  function range(tabId, segmentId, s, e) {
    var r = { startIndex: s, endIndex: e };
    if (segmentId) r.segmentId = segmentId;
    if (tabId) r.tabId = tabId;
    return r;
  }

  /** Text that is a citation: linked, looking like plain text. */
  function citeStyle(tabId, segmentId, s, e, url, sup, fresh) {
    var style = { link: { url: url }, underline: false, baselineOffset: sup ? 'SUPERSCRIPT' : 'NONE' };
    var fields = 'link,underline,baselineOffset,foregroundColor';
    if (fresh) { style.italic = false; fields += ',italic'; }
    return { updateTextStyle: { range: range(tabId, segmentId, s, e), textStyle: style, fields: fields } };
  }

  /** Bibliography text: the entries (runs with italic and bold), linked, otherwise plain. */
  function bibStyles(tabId, at, entries) {
    var out = [], pos = at;
    entries.forEach(function (runs, k) {
      var len = runs.reduce(function (n, r) { return n + (r.text || '').length; }, 0);
      out.push({ updateTextStyle: { range: range(tabId, undefined, pos, pos + len),
        textStyle: { link: { url: BIB }, underline: false, bold: false, italic: false, baselineOffset: 'NONE' },
        fields: 'link,underline,bold,italic,baselineOffset,foregroundColor' } });
      var p = pos;
      runs.forEach(function (r) {
        var n = (r.text || '').length;
        if (n && (r.italic || r.bold)) {
          var st = {}, f = [];
          if (r.italic) { st.italic = true; f.push('italic'); }
          if (r.bold) { st.bold = true; f.push('bold'); }
          out.push({ updateTextStyle: { range: range(tabId, undefined, p, p + n), textStyle: st, fields: f.join(',') } });
        }
        p += n;
      });
      pos += len + (k < entries.length - 1 ? 1 : 0);
    });
    return out;
  }

  function entriesText(entries) {
    return entries.map(function (runs) { return runs.map(function (r) { return r.text || ''; }).join(''); });
  }

  /**
   * The batchUpdate requests for an Update All worked out from `parsed` (see read):
   *   plan.texts:         one per item (null or "": the text stays; a placeholder still becomes a citation)
   *   plan.superscripts:  one per item
   *   plan.bibliography:  entries (each a list of runs {text, italic, bold}) for the bibliography and
   *                       new ones asked for by placeholders; null: the bibliography is left alone
   *   plan.forceBibliography: rewrite it even if its text is the same (the style changed)
   *   plan.store:         names of the new store ranges (null: the store is left as it is)
   * Returns {requests, updated (citations rewritten), bibliography (whether it changed), placeholders}.
   * Requests go from the end of each part of the document to its start, so each one's positions
   * hold when it is applied.
   */
  function planRequests(parsed, plan) {
    var ops = [];   // {key, start, reqs}
    var key = function (tabId, segmentId) { return (tabId || '') + '|' + (segmentId || ''); };
    var updated = 0, placeholders = 0, bibChanged = false;

    parsed.items.forEach(function (it, i) {
      var text = plan.texts ? plan.texts[i] : null, sup = !!(plan.superscripts && plan.superscripts[i]);
      var reqs = [];
      if (text) {
        if (it.kind === 'cite' && it.text === text && it.sups.every(function (s) { return s === sup; })) return;
        // the new text after the old, then the old removed (the paragraph never empties)
        reqs.push({ insertText: { location: loc(it.tabId, it.segmentId, it.end), text: text } });
        reqs.push({ deleteContentRange: { range: range(it.tabId, it.segmentId, it.start, it.end) } });
        reqs.push(citeStyle(it.tabId, it.segmentId, it.start, it.start + text.length, it.url, sup, it.kind !== 'cite'));
        updated++;
      } else if (it.kind === 'new') {
        // nothing to write it with yet: at least a citation the Apps Script sees too
        reqs.push(citeStyle(it.tabId, it.segmentId, it.start, it.end, it.url, sup, true));
      } else if (it.kind === 'merged') {
        var ph = it.placeholder, n = ph.end - ph.start;
        reqs.push({ deleteContentRange: { range: range(it.tabId, it.segmentId, ph.start, ph.end) } });
        reqs.push(citeStyle(it.tabId, it.segmentId, it.start, it.end - n, it.url, it.sups[0], false));
      } else return;
      if (it.kind !== 'cite') placeholders++;
      ops.push({ key: key(it.tabId, it.segmentId), start: it.start, reqs: reqs });
    });

    var entries = plan.bibliography ? (plan.bibliography.length ? plan.bibliography : [[{ text: 'References will appear here.' }]]) : null;
    var bib = parsed.bibliography;
    if (entries && bib) {
      var texts = entriesText(entries);
      var same = bib.paragraphs.length === texts.length && bib.paragraphs.every(function (p, k) { return p.text === texts[k]; });
      if (!same || plan.forceBibliography) {
        bibChanged = true;
        // runs of adjacent paragraphs; the first run gets the new entries, the others go
        var groups = [];
        bib.paragraphs.forEach(function (p) {
          var g = groups[groups.length - 1];
          if (g && g[g.length - 1].end === p.start) g.push(p); else groups.push([p]);
        });
        groups.slice(1).forEach(function (g) {
          g.forEach(function (p) {
            // the body's last paragraph can't go: it is emptied
            var end = p.end === bib.bodyEnd ? p.end - 1 : p.end;
            if (end > p.start) ops.push({ key: key(bib.tabId), start: p.start, reqs: [{ deleteContentRange: { range: range(bib.tabId, undefined, p.start, end) } }] });
          });
        });
        var g0 = groups[0], gs = g0[0].start, ge = g0[g0.length - 1].end;
        var all = texts.join('\n'), L = all.length;
        // new text before the old (it takes over the paragraphs' look, list included), the old text
        // removed up to (not with) its last paragraph mark
        var reqs = [{ insertText: { location: loc(bib.tabId, undefined, gs), text: all } }];
        if (ge - 1 > gs) reqs.push({ deleteContentRange: { range: range(bib.tabId, undefined, gs + L, ge - 1 + L) } });
        ops.push({ key: key(bib.tabId), start: gs, reqs: reqs.concat(bibStyles(bib.tabId, gs, entries)) });
      }
    }

    // Insert Bibliography: new paragraphs after the one with the placeholder (once)
    var made = !!bib;
    parsed.bibPlaceholders.forEach(function (ph) {
      var at = ph.paragraph && !made ? ph.paragraph.end - 1 : null;
      if (at !== null && entries) {
        made = true;
        // before its paragraph mark: "\n" + the entries (applied before the placeholder goes, so
        // these positions are the document's as read)
        var all = '\n' + entriesText(entries).join('\n'), s = at + 1, e = at + all.length;
        ops.push({ key: key(ph.tabId), start: at, reqs: [
          { insertText: { location: loc(ph.tabId, undefined, at), text: all } },
          { updateParagraphStyle: { range: range(ph.tabId, undefined, s, e + 1),
            paragraphStyle: { namedStyleType: 'NORMAL_TEXT' }, fields: 'namedStyleType,alignment,indentStart,indentFirstLine,indentEnd' } },
          { deleteParagraphBullets: { range: range(ph.tabId, undefined, s, e + 1) } }
        ].concat(bibStyles(ph.tabId, s, entries)) });
        bibChanged = true;
      }
      placeholders++;
      ops.push({ key: key(ph.tabId, ph.segmentId), start: ph.start, reqs: [{ deleteContentRange: { range: range(ph.tabId, ph.segmentId, ph.start, ph.end) } }] });
    });

    ops.sort(function (a, b) { return a.key < b.key ? -1 : a.key > b.key ? 1 : b.start - a.start; });
    var requests = [];
    if (plan.store) {
      // first: the new ranges cover the text as it is now, and move with the edits after them
      unique(parsed.storeNames).forEach(function (name) { requests.push({ deleteNamedRange: { name: name } }); });
      var end = parsed.firstTab.bodyEnd - 1;
      if (end > 1) plan.store.forEach(function (name) { requests.push({ createNamedRange: { name: name, range: range(parsed.firstTab.tabId, undefined, 1, end) } }); });
    }
    ops.forEach(function (o) { requests = requests.concat(o.reqs); });
    return { requests: requests, updated: updated, bibliography: bibChanged, placeholders: placeholders };
  }

  return {
    CITE: CITE, BIB: BIB, INSERT: INSERT, STORE_MAX: STORE_MAX,
    parseLink: parseLink, makeLink: makeLink, placeholderUrl: placeholderUrl, parsePlaceholder: parsePlaceholder,
    placeholderHTML: placeholderHTML, newToken: newToken,
    read: read, storePart: storePart, storeGenerations: storeGenerations, storeNames: storeNames, newGeneration: newGeneration,
    sameStore: sameStore, planRequests: planRequests
  };
})();
if (typeof module !== 'undefined') module.exports = LuntaDocs;
