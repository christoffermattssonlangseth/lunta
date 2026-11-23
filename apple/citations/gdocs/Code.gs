// Lunta citations for Google Docs (Apps Script, bound to your document).
// Citations are text linked to https://margins.local/cite?ids=…; bibliography paragraphs are
// linked to https://margins.local/bib. The sidebar talks to the Lunta app on your Mac; this script
// only reads and writes the document. Paste this file and Sidebar.html again when Lunta says so.
// The Lunta browser extension edits the same documents through the Google Docs API: same links,
// and the same style and papers, kept in the document's named ranges (see "the document's store").
// A citation's options (each paper's page, prefix, suffix, author left out or in the text) are in
// named ranges over its text, "lunta-o:<key>:<i>:<n>:<part of the code>" (common.js); scripts from
// before them don't see them, and drop them when they rewrite that citation's text.
/**
 * @OnlyCurrentDoc
 * Apps Script then asks only for documents.currentonly (this document), not every document.
 */
var CODE_VERSION = 6;
var OPTS = 'lunta-o:';
var ZOTERO = 'https://www.zotero.org/google-docs/?';
var CITE = 'https://margins.local/cite?ids=';
var BIB = 'https://margins.local/bib';
// What the document keeps of the papers it cites (so a co-author, another library or a deleted
// paper doesn't break it), gzipped in chunks: document properties hold 9 KB per value.
var STORE = 'lunta.store.', CHUNK = 8000;
// The same, readable by the browser extension too (document properties belong to this script):
// named ranges "lunta:<generation>:<i>:<n>:<base64url of gzipped JSON>" over the first tab's text,
// {v, at, style, styleTitle, bibStyle, papers}. The newest complete generation counts.
var NR = 'lunta:', NR_CHUNK = 220, NR_MAX = 400;

function onOpen() {
  DocumentApp.getUi().createMenu('Lunta').addItem('Cite…', 'showSidebar').addToUi();
}

function onInstall() { onOpen(); }

function showSidebar() {
  DocumentApp.getUi().showSidebar(HtmlService.createHtmlOutputFromFile('Sidebar').setTitle('Lunta'));
}

function version() { return CODE_VERSION; }

// ---- reading citations ----

function idsOf_(url) {
  var raw = url.slice(CITE.length);
  try { raw = decodeURIComponent(raw); } catch (e) {}
  var seen = {}, out = [];
  raw.split(',').forEach(function (x) { x = x.trim(); if (x && !seen[x]) { seen[x] = true; out.push(x); } });
  return out;
}

/** Citation runs in one container (body or a footnote), in order: [{el, start, end, url, ids}]. */
function scan_(container) {
  var out = [];
  var found = null;
  while ((found = container.findElement(DocumentApp.ElementType.TEXT, found))) {
    var t = found.getElement().asText();
    var idx = t.getTextAttributeIndices();
    var len = t.getText().length;
    for (var i = 0; i < idx.length; i++) {
      var s = idx[i], e = (i + 1 < idx.length ? idx[i + 1] : len) - 1;
      if (e < s) continue;
      var url = t.getLinkUrl(s);
      if (url && url.indexOf(CITE) === 0) {
        // one citation split into runs (a word in it made bold, say)
        var last = out[out.length - 1];
        if (last && last.el === t && last.end + 1 === s && last.url === url) { last.end = e; continue; }
        out.push({ el: t, start: s, end: e, url: url, ids: idsOf_(url) });
      }
    }
  }
  return out.filter(function (r) { return r.ids.length; });
}

/** Where an element sits: child indexes from its body down. */
function path_(el) {
  var p = [];
  while (el.getType() !== DocumentApp.ElementType.BODY_SECTION && el.getParent()) {
    var parent = el.getParent();
    p.unshift(parent.getChildIndex(el));
    el = parent;
  }
  return p;
}

function before_(a, b) {
  for (var i = 0; i < Math.min(a.length, b.length); i++) if (a[i] !== b[i]) return a[i] < b[i];
  return a.length < b.length;
}

/** The document's tabs in order (a thesis may keep a chapter per tab): [{body, footnotes}]. */
function sections_(doc) {
  var out = [];
  var tabs = typeof doc.getTabs === 'function' ? doc.getTabs() : null;
  if (!tabs || !tabs.length) return [{ body: doc.getBody(), footnotes: doc.getFootnotes() || [], host: doc }];
  (function walk(list) {
    list.forEach(function (tab) {
      if (tab.getType() === DocumentApp.TabType.DOCUMENT_TAB) {
        var d = tab.asDocumentTab();
        out.push({ body: d.getBody(), footnotes: d.getFootnotes() || [], host: d });
      }
      walk(tab.getChildTabs() || []);
    });
  })(tabs);
  return out;
}

/** Every citation in document order (tab by tab), footnotes included where their marks are; each
 *  with its options' named ranges (`opts`: [NamedRange]) and its tab (`host`). */
function citations_(doc) {
  return [].concat.apply([], sections_(doc).map(function (sec) {
    var runs = scan_(sec.body);
    var notes = [];
    sec.footnotes.forEach(function (fn) {
      var c = fn.getFootnoteContents();
      var rs = c ? scan_(c) : [];
      if (rs.length) notes.push({ path: path_(fn), runs: rs });
    });
    withOptions_(sec, runs.concat([].concat.apply([], notes.map(function (n) { return n.runs; }))));
    if (!notes.length) return runs;
    var items = runs.map(function (r) { return { path: path_(r.el).concat([r.start]), runs: [r] }; }).concat(notes);
    items.sort(function (a, b) { return before_(a.path, b.path) ? -1 : before_(b.path, a.path) ? 1 : 0; });
    return [].concat.apply([], items.map(function (x) { return x.runs; }));
  }));
}

/** Where a piece of text is, comparable across calls: its root (body or a footnote) and path. */
function where_(el) {
  var p = [], root = el;
  while (root.getParent()) { var parent = root.getParent(); p.unshift(parent.getChildIndex(root)); root = parent; }
  return root.getType() + ':' + p.join('.');
}

/** Attaches the options' named ranges of a section to its runs (by where their text is). */
function withOptions_(sec, runs) {
  var list = [];
  try { list = sec.host.getNamedRanges(); } catch (e) { list = []; }
  var opts = [];
  (list || []).forEach(function (nr) {
    if (String(nr.getName()).indexOf(OPTS) !== 0) return;
    var els = nr.getRange().getRangeElements();
    if (!els.length || els[0].getElement().getType() !== DocumentApp.ElementType.TEXT) return;
    var e = els[0];
    opts.push({ nr: nr, at: where_(e.getElement()), s: e.isPartial() ? e.getStartOffset() : 0, e: e.isPartial() ? e.getEndOffsetInclusive() : e.getElement().asText().getText().length - 1 });
  });
  runs.forEach(function (r) {
    r.host = sec.host;
    var at = where_(r.el);
    r.opts = opts.filter(function (o) { return o.at === at && o.s <= r.end && o.e >= r.start; }).map(function (o) { return o.nr; });
  });
  return runs;
}

/** The options code of a citation's named ranges ("" when none or incomplete). */
function optionsCode_(r) {
  var groups = {};
  (r.opts || []).forEach(function (nr) {
    var m = /^lunta-o:([A-Za-z0-9]+):(\d+):(\d+):([A-Za-z0-9_-]*)$/.exec(nr.getName());
    if (!m) return;
    var g = groups[m[1]] = groups[m[1]] || { n: +m[3], parts: {} };
    if (+m[3] === g.n) g.parts[+m[2]] = m[4];
  });
  var best = '';
  Object.keys(groups).forEach(function (k) {
    var g = groups[k], code = '';
    for (var i = 0; i < g.n; i++) { if (g.parts[i] === undefined) return; code += g.parts[i]; }
    if (!best) best = code;
  });
  return best;
}

/** Puts the options ranges `names` over a citation's text [start, end] (the old ones removed). */
function setOptionRanges_(r, names, start, end) {
  (r.opts || []).forEach(function (nr) { try { nr.remove(); } catch (e) {} });
  r.opts = [];
  if (!names || !names.length || end < start) return;
  var range = r.host.newRange().addElement(r.el, start, end).build();
  names.forEach(function (n) { r.opts.push(r.host.addNamedRange(n, range)); });
}

/** Citations in order, as lists of ids. */
function listCitations() {
  return citations_(DocumentApp.getActiveDocument()).map(function (r) { return r.ids; });
}

// ---- Check Citations: the document is only read ----

/** Every paragraph (tab by tab, tables included, then the footnotes) with the Lunta citations in
 *  it, for common.js claimSites: [{text, cites: [{text, ids}]}]. */
function claimParagraphs() {
  var out = [];
  sections_(DocumentApp.getActiveDocument()).forEach(function (sec) {
    var containers = [sec.body].concat(sec.footnotes.map(function (fn) { return fn.getFootnoteContents(); }).filter(function (c) { return c; }));
    containers.forEach(function (c) {
      c.getParagraphs().forEach(function (p) {
        var text = p.getText();
        if (!text) return;
        var cites = scan_(p).map(function (r) { return { text: r.el.getText().slice(r.start, r.end + 1), ids: r.ids }; });
        out.push({ text: text, cites: cites });
      });
    });
  });
  return out;
}

// ---- what the sidebar needs ----

/** The document's citations, style and saved papers, and this user's pairing key (if set). */
function getState() {
  var doc = DocumentApp.getActiveDocument();
  var props = PropertiesService.getDocumentProperties().getProperties();
  var key = '';
  try { key = PropertiesService.getUserProperties().getProperty('luntaPairingKey') || ''; } catch (e) {}
  var ds = readDocStore_(doc);
  return {
    version: CODE_VERSION,
    citations: citations_(doc).map(function (r) { return r.ids; }),
    options: citations_(doc).map(optionsCode_),
    style: (ds && ds.style) || props.luntaStyle || '',
    styleTitle: ds && ds.style ? (ds.styleTitle || '') : (props.luntaStyleTitle || ''),
    papers: ds && ds.papers ? ds.papers : readStore_(props),
    hasBibliography: bibParagraphs_(doc).length > 0,
    key: key
  };
}

function setStyle(s, title) {
  var p = PropertiesService.getDocumentProperties();
  p.setProperty('luntaStyle', s);
  if (title) p.setProperty('luntaStyleTitle', title); else p.deleteProperty('luntaStyleTitle');
  var doc = DocumentApp.getActiveDocument(), ds = readDocStore_(doc);
  if (ds) { ds.style = s; ds.styleTitle = title || ''; writeDocStore_(doc, ds); }
}

function getStyle() {
  var ds = readDocStore_(DocumentApp.getActiveDocument());
  return (ds && ds.style) || PropertiesService.getDocumentProperties().getProperty('luntaStyle') || 'apa';
}

// ---- the document's store (shared with the browser extension) ----

/** The first tab (named ranges live per tab), or the document itself before tabs. */
function storeHost_(doc) {
  var tabs = typeof doc.getTabs === 'function' ? doc.getTabs() : null;
  return tabs && tabs.length ? tabs[0].asDocumentTab() : doc;
}

/** {gen, i, n, data} of a store range's name, or null. */
function storePart_(name) {
  var m = /^lunta:([a-z0-9]+):(\d+):(\d+):([A-Za-z0-9_\-=]*)$/.exec(String(name || ''));
  return m ? { gen: m[1], i: +m[2], n: +m[3], data: m[4] } : null;
}

/** The newest complete store in the document's named ranges, or null. */
function readDocStore_(doc) {
  var list;
  try { list = storeHost_(doc).getNamedRanges(); } catch (e) { return null; }
  var gens = {};
  (list || []).forEach(function (nr) {
    var p = storePart_(nr.getName());
    if (!p) return;
    var g = gens[p.gen] = gens[p.gen] || { n: p.n, parts: {} };
    if (p.n === g.n && p.i < p.n) g.parts[p.i] = p.data;
  });
  var best = null;
  Object.keys(gens).forEach(function (k) {
    var g = gens[k], b64 = '';
    for (var i = 0; i < g.n; i++) { if (g.parts[i] === undefined) return; b64 += g.parts[i]; }
    try {
      var blob = Utilities.newBlob(Utilities.base64DecodeWebSafe(b64), 'application/x-gzip');
      var j = JSON.parse(Utilities.ungzip(blob).getDataAsString());
      if (j && typeof j === 'object' && (!best || (j.at || 0) > (best.at || 0))) best = j;
    } catch (e) {}
  });
  return best;
}

function sameStore_(a, b) {
  if (!a || !b) return false;
  var strip = function (x) { var c = {}; Object.keys(x).forEach(function (k) { if (k !== 'at') c[k] = x[k]; }); return JSON.stringify(c); };
  return strip(a) === strip(b);
}

/** Writes {style, styleTitle, bibStyle, papers} as named ranges; false when it doesn't fit. Unchanged: nothing written. */
function writeDocStore_(doc, store) {
  var host = storeHost_(doc);
  var old;
  try { old = host.getNamedRanges(); } catch (e) { return false; }
  var cur = readDocStore_(doc);
  var next = { v: 1, at: Date.now(), style: store.style || '', styleTitle: store.styleTitle || '', bibStyle: store.bibStyle || '', papers: store.papers || {} };
  if (sameStore_(cur, next)) return true;
  var encode = function (x) {
    return Utilities.base64EncodeWebSafe(Utilities.gzip(Utilities.newBlob(JSON.stringify(x), 'application/json')).getBytes());
  };
  var b64 = encode(next);
  if (Math.ceil(b64.length / NR_CHUNK) > NR_MAX) {
    // too large: without the saved entries (Lunta formats the papers from their details)
    var lean = {};
    Object.keys(next.papers).forEach(function (id) { var r = next.papers[id]; lean[id] = { m: r.m, f: r.f, y: r.y, t: r.t, e: {} }; });
    next.papers = lean;
    b64 = encode(next);
    if (Math.ceil(b64.length / NR_CHUNK) > NR_MAX) return false;
  }
  var body = host.getBody(), count = body.getNumChildren();
  if (!count) return false;
  var range = host.newRange().addElementsBetween(body.getChild(0), body.getChild(count - 1)).build();
  var gen = (Date.now().toString(36) + Math.floor(Math.random() * 1e6).toString(36)).slice(-10);
  var n = Math.max(1, Math.ceil(b64.length / NR_CHUNK));
  for (var i = 0; i < n; i++) host.addNamedRange(NR + gen + ':' + i + ':' + n + ':' + b64.slice(i * NR_CHUNK, (i + 1) * NR_CHUNK), range);
  // the old generations once the new one is complete
  (old || []).forEach(function (nr) { var p = storePart_(nr.getName()); if (p && p.gen !== gen) nr.remove(); });
  return true;
}

/** The pairing key for this user (each Mac running Lunta has its own). */
function setPairingKey(key) {
  PropertiesService.getUserProperties().setProperty('luntaPairingKey', String(key || '').trim());
}

function readStore_(props) {
  var n = parseInt(props[STORE + 'n'] || '0', 10);
  if (!n) return {};
  var b64 = '';
  for (var i = 0; i < n; i++) {
    if (props[STORE + i] === undefined) return {};
    b64 += props[STORE + i];
  }
  try {
    var blob = Utilities.newBlob(Utilities.base64Decode(b64), 'application/x-gzip');
    return JSON.parse(Utilities.ungzip(blob).getDataAsString()) || {};
  } catch (e) {
    return {};
  }
}

/** Saves the papers; false when they don't fit (the document still updates). */
function writeStore_(store) {
  var dp = PropertiesService.getDocumentProperties();
  var props = dp.getProperties();
  var json = JSON.stringify(store || {});
  var b64 = Utilities.base64Encode(Utilities.gzip(Utilities.newBlob(json, 'application/json')).getBytes());
  var chunks = {};
  var n = Math.ceil(b64.length / CHUNK);
  for (var i = 0; i < n; i++) chunks[STORE + i] = b64.slice(i * CHUNK, (i + 1) * CHUNK);
  chunks[STORE + 'n'] = String(n);
  try {
    dp.setProperties(chunks, false);
  } catch (e) {
    return false;
  }
  var old = parseInt(props[STORE + 'n'] || '0', 10);
  for (var k = n; k < old; k++) dp.deleteProperty(STORE + k);
  return true;
}

// ---- writing ----

/** Links look like normal text: no underline, no link color. */
function plain_(t, s, e) {
  t.setUnderline(s, e, false);
  try { t.setForegroundColor(s, e, null); } catch (x) { t.setForegroundColor(s, e, '#000000'); }
}

/** Where a new citation goes: the cursor, or the end of the selection. {text, offset} */
function insertionPoint_(doc) {
  var cursor = doc.getCursor();
  if (cursor) {
    var t = cursor.getSurroundingText();
    if (t) return { text: t, offset: cursor.getSurroundingTextOffset() };
    throw new Error('Can’t insert here. Click inside a paragraph.');
  }
  var sel = doc.getSelection();
  if (sel) {
    var els = sel.getRangeElements();
    var last = els[els.length - 1];
    var el = last.getElement();
    if (el.getType() === DocumentApp.ElementType.TEXT) {
      var tx = el.asText();
      return { text: tx, offset: last.isPartial() ? last.getEndOffsetInclusive() + 1 : tx.getText().length };
    }
    if (el.editAsText) return { text: el.editAsText(), offset: el.editAsText().getText().length };
  }
  throw new Error('Click in the document where the citation should go.');
}

/**
 * Inserts a citation at the cursor (after a selection, which is kept). With the cursor inside a
 * citation, the papers are added to it. Returns {added} or {merged}.
 */
function insertCitation(ids, text, superscript) {
  var doc = DocumentApp.getActiveDocument();
  var at = insertionPoint_(doc);
  var t = at.text, o = at.offset, len = t.getText().length;
  if (isBibText_(t)) throw new Error('That’s the bibliography. Click in your text where the citation should go.');
  var left = o > 0 ? t.getLinkUrl(o - 1) : null, right = o < len ? t.getLinkUrl(o) : null;
  if (left && left === right && left.indexOf(CITE) === 0) {
    // inside a citation: its papers and these, as one citation
    var s = o - 1, e = o;
    while (s > 0 && t.getLinkUrl(s - 1) === left) s--;
    while (e + 1 < len && t.getLinkUrl(e + 1) === left) e++;
    var had = idsOf_(left);
    ids.forEach(function (id) { if (had.indexOf(id) < 0) had.push(id); });
    t.setLinkUrl(s, e, CITE + encodeURIComponent(had.join(',')));
    return { merged: true };
  }
  t.insertText(o, text);
  var end = o + text.length - 1;
  t.setLinkUrl(o, end, CITE + encodeURIComponent(ids.join(',')));
  plain_(t, o, end);
  t.setItalic(o, end, false);
  t.setTextAlignment(o, end, superscript ? DocumentApp.TextAlignment.SUPERSCRIPT : DocumentApp.TextAlignment.NORMAL);
  try { doc.setCursor(doc.newPosition(t, end + 1)); } catch (x) {}
  return { added: true };
}

/**
 * Writes what the sidebar worked out: plan = {keys (each citation's ids, joined), texts,
 * superscripts, bibliography (runs per entry, or null), style, styleTitle, papers}. Checks first
 * that the document still has exactly the citations the plan was made for (someone may have typed
 * meanwhile) and answers {changed: true} if not, so the sidebar can start over. Text that is
 * already right is left alone (Update All twice changes nothing).
 */
function applyPlan(plan) {
  var lock = LockService.getDocumentLock();
  if (!lock.tryLock(15000)) throw new Error('Another Update All is running in this document. Try again in a moment.');
  try {
    var doc = DocumentApp.getActiveDocument();
    var runs = citations_(doc);
    var keys = runs.map(function (r) { return r.ids.join(','); });
    if (keys.length !== plan.keys.length || keys.some(function (k, i) { return k !== plan.keys[i]; })) return { changed: true };
    var updated = 0;
    for (var i = runs.length - 1; i >= 0; i--) {
      var r = runs[i], text = plan.texts[i];
      if (text === null || text === undefined || text === '') continue;
      var align = plan.superscripts[i] ? DocumentApp.TextAlignment.SUPERSCRIPT : DocumentApp.TextAlignment.NORMAL;
      var now = r.el.getText().slice(r.start, r.end + 1);
      if (now === text && r.el.getTextAlignment(r.start) === align && sameAlignment_(r)) continue;
      // the new text after the old, then the old removed: the element never empties
      var optNames = (r.opts || []).map(function (nr) { return nr.getName(); });
      r.el.insertText(r.end + 1, text);
      r.el.deleteText(r.start, r.end);
      var end = r.start + text.length - 1;
      r.el.setLinkUrl(r.start, end, r.url);
      plain_(r.el, r.start, end);
      r.el.setTextAlignment(r.start, end, align);
      // the options over the new text
      if (optNames.length) setOptionRanges_(r, optNames, r.start, end);
      updated++;
    }
    var props = PropertiesService.getDocumentProperties();
    var ds = readDocStore_(doc);
    var bibStyle = (ds && ds.bibStyle) || props.getProperty('luntaBibStyle');
    var bibChanged = false;
    if (plan.bibliography) bibChanged = writeBibliography_(doc, plan.bibliography, false, bibStyle !== plan.style);
    props.setProperty('luntaStyle', plan.style);
    props.setProperty('luntaBibStyle', plan.style);
    if (plan.styleTitle) props.setProperty('luntaStyleTitle', plan.styleTitle);
    var saved = plan.papers ? writeStore_(plan.papers) : true;
    var shared = writeDocStore_(doc, {
      style: plan.style, styleTitle: plan.styleTitle || (ds && ds.style === plan.style ? ds.styleTitle : '') || '',
      bibStyle: plan.style,
      papers: plan.papers || (ds && ds.papers) || readStore_(props.getProperties())
    });
    return { changed: false, count: runs.length, updated: updated, bibliography: bibChanged, saved: saved && shared };
  } finally {
    lock.releaseLock();
  }
}

function sameAlignment_(r) {
  return r.el.getTextAlignment(r.end) === r.el.getTextAlignment(r.start);
}

/** Older sidebars: texts by position, no checks. Kept so a document with an old Sidebar.html works. */
function applyAll(texts, superscript, bibliography) {
  var runs = citations_(DocumentApp.getActiveDocument());
  return applyPlan({
    keys: runs.map(function (r) { return r.ids.join(','); }), texts: texts,
    superscripts: runs.map(function () { return !!superscript; }), bibliography: bibliography,
    style: getStyle(), papers: null
  }).count;
}

/** The citation at the cursor (or the selection's start): {index, ids, options (code), text}, or null. */
function citationAtCursor() {
  var doc = DocumentApp.getActiveDocument();
  var cursor = doc.getCursor(), el = null, off = 0;
  if (cursor) { el = cursor.getSurroundingText(); off = cursor.getSurroundingTextOffset(); }
  else if (doc.getSelection()) {
    var re = doc.getSelection().getRangeElements()[0];
    if (re && re.getElement().getType() === DocumentApp.ElementType.TEXT) { el = re.getElement().asText(); off = re.isPartial() ? re.getStartOffset() : 0; }
  }
  if (!el) return null;
  var at = where_(el);
  var runs = citations_(doc);
  for (var i = 0; i < runs.length; i++) {
    var r = runs[i];
    if (where_(r.el) === at && off >= r.start && off <= r.end + 1) {
      return { index: i, ids: r.ids, options: optionsCode_(r), text: r.el.getText().slice(r.start, r.end + 1) };
    }
  }
  return null;
}

/**
 * Sets the options of citation `index` (its papers `key`, joined, as the sidebar saw them):
 * `names` are the named ranges' names (LuntaCite.optionRangeNames), none to clear. False when the
 * citation moved or changed meanwhile.
 */
function setCitationOptions(index, key, names) {
  var runs = citations_(DocumentApp.getActiveDocument());
  var r = runs[index];
  if (!r || r.ids.join(',') !== key) return false;
  setOptionRanges_(r, names || [], r.start, r.end);
  return true;
}

// ---- converting Zotero's citations ----
// Zotero's Google Docs plugin links each citation to https://www.zotero.org/google-docs/?<key>
// (6 characters) and keeps its field code in named ranges "Z_F<key><000…><part>" over it:
// "ITEM CSL_CITATION {…}", or "BIBL {…} CSL_BIBLIOGRAPHY" for the bibliography, whose paragraphs
// are linked to its key too. The sidebar reads the codes (common.js), finds the papers, and asks
// the writer; zoteroConvert then relinks the citations to Lunta's.

/** Zotero's links in the document: [{el, start, end, key, host}] in document order. */
function zoteroRuns_(doc) {
  var out = [];
  sections_(doc).forEach(function (sec) {
    var containers = [sec.body].concat(sec.footnotes.map(function (fn) { return fn.getFootnoteContents(); }).filter(Boolean));
    containers.forEach(function (container) {
      var found = null;
      while ((found = container.findElement(DocumentApp.ElementType.TEXT, found))) {
        var t = found.getElement().asText(), idx = t.getTextAttributeIndices(), len = t.getText().length;
        for (var i = 0; i < idx.length; i++) {
          var s = idx[i], e = (i + 1 < idx.length ? idx[i + 1] : len) - 1;
          if (e < s) continue;
          var url = t.getLinkUrl(s);
          if (!url || url.indexOf(ZOTERO) !== 0) continue;
          var last = out[out.length - 1];
          if (last && last.el === t && last.end + 1 === s && last.url === url) { last.end = e; continue; }
          out.push({ el: t, start: s, end: e, url: url, key: url.substr(ZOTERO.length, 6), host: sec.host });
        }
      }
    });
  });
  return out;
}

/** Zotero's named ranges' names, in every tab. */
function zoteroNames_(doc) {
  var names = [];
  sections_(doc).forEach(function (sec) {
    try { sec.host.getNamedRanges().forEach(function (nr) { if (nr.getName().indexOf('Z_F') === 0) names.push(nr.getName()); }); } catch (e) {}
  });
  return names;
}

/** What Zotero left in the document: {runs: [{key, text}] in order, names: [the Z_F names], paperpile}. */
function zoteroScan() {
  var doc = DocumentApp.getActiveDocument();
  var paperpile = 0;
  sections_(doc).forEach(function (sec) {
    var found = null;
    while ((found = sec.body.findElement(DocumentApp.ElementType.TEXT, found))) {
      var t = found.getElement().asText(), idx = t.getTextAttributeIndices(), len = t.getText().length;
      for (var i = 0; i < idx.length; i++) {
        if (idx[i] >= len) continue;
        var u = t.getLinkUrl(idx[i]);
        if (u && u.indexOf('https://paperpile.com/c/') === 0) paperpile++;
      }
    }
  });
  return {
    runs: zoteroRuns_(doc).map(function (r) { return { key: r.key, text: r.el.getText().slice(r.start, r.end + 1) }; }),
    names: zoteroNames_(doc), paperpile: paperpile
  };
}

/**
 * Makes Zotero's citations Lunta's: `cites` [{key, ids, names (options ranges)}] are relinked to
 * their papers; `bibKeys`: Zotero bibliographies, whose paragraphs become Lunta's bibliography
 * (`bibToLunta`) or are removed; the converted keys' Z_F named ranges go. Then the sidebar runs
 * Update All. Returns {converted, bibliography}.
 */
function zoteroConvert(cites, bibKeys, bibToLunta) {
  var lock = LockService.getDocumentLock();
  if (!lock.tryLock(15000)) throw new Error('Another Update All is running in this document. Try again in a moment.');
  try {
    var doc = DocumentApp.getActiveDocument();
    var byKey = {};
    (cites || []).forEach(function (c) { byKey[c.key] = c; });
    var bib = {};
    (bibKeys || []).forEach(function (k) { bib[k] = true; });
    var runs = zoteroRuns_(doc), converted = 0, bibParas = [];
    for (var i = runs.length - 1; i >= 0; i--) {
      var r = runs[i], c = byKey[r.key];
      if (c) {
        r.el.setLinkUrl(r.start, r.end, CITE + encodeURIComponent(c.ids.join(',')));
        plain_(r.el, r.start, r.end);
        if (c.names && c.names.length) setOptionRanges_({ el: r.el, host: r.host, opts: [] }, c.names, r.start, r.end);
        converted++;
      } else if (bib[r.key]) {
        var p = r.el.getParent();
        if (bibParas.indexOf(p) < 0) bibParas.push(p);
        if (bibToLunta) { r.el.setLinkUrl(r.start, r.end, BIB); plain_(r.el, r.start, r.end); }
      }
    }
    if (!bibToLunta) {
      bibParas.forEach(function (p) {
        var body = p.getParent();
        if (body.getChildIndex(p) === body.getNumChildren() - 1) p.clear(); else body.removeChild(p);
      });
    }
    // the converted fields' codes
    sections_(doc).forEach(function (sec) {
      try {
        sec.host.getNamedRanges().forEach(function (nr) {
          var n = nr.getName();
          if (n.indexOf('Z_F') === 0 && (byKey[n.substr(3, 6)] || bib[n.substr(3, 6)])) nr.remove();
        });
      } catch (e) {}
    });
    return { converted: converted, bibliography: bibParas.length > 0 };
  } finally {
    lock.releaseLock();
  }
}

/** Selects the first citation of a paper (to show the writer which one). */
function selectCitation(id) {
  var doc = DocumentApp.getActiveDocument();
  var runs = citations_(doc);
  for (var i = 0; i < runs.length; i++) {
    if (runs[i].ids.indexOf(id) < 0) continue;
    try { doc.setSelection(doc.newRange().addElement(runs[i].el, runs[i].start, runs[i].end).build()); } catch (e) {}
    return true;
  }
  return false;
}

// ---- the bibliography ----

// ---- Convert Citations to Plain Text: for the version you submit ----

/**
 * Every citation and the bibliography made ordinary text (their links removed; their text and look
 * stay) and what Lunta keeps in the document removed: the store's and the options' named ranges,
 * the document properties. The sidebar runs Update All first. Returns {cites, bib}.
 */
function plainText() {
  var lock = LockService.getDocumentLock();
  if (!lock.tryLock(15000)) throw new Error('Another Update All is running in this document. Try again in a moment.');
  try {
    var doc = DocumentApp.getActiveDocument();
    var runs = citations_(doc);
    runs.forEach(function (r) { r.el.setLinkUrl(r.start, r.end, null); });
    var bib = bibParagraphs_(doc);
    bib.forEach(function (b) {
      var t = b.el.editAsText(), n = t.getText().length;
      if (n) t.setLinkUrl(0, n - 1, null);
    });
    sections_(doc).forEach(function (sec) {
      var list = [];
      try { list = sec.host.getNamedRanges(); } catch (e) { list = []; }
      (list || []).forEach(function (nr) {
        var name = String(nr.getName());
        if (name.indexOf(OPTS) === 0 || storePart_(name)) nr.remove();
      });
    });
    var dp = PropertiesService.getDocumentProperties();
    Object.keys(dp.getProperties()).forEach(function (k) { if (k.indexOf('lunta') === 0) dp.deleteProperty(k); });
    return { cites: runs.length, bib: bib.length ? 1 : 0 };
  } finally {
    lock.releaseLock();
  }
}

function isBibText_(t) {
  var n = t.getText().length;
  return n > 0 && (t.getLinkUrl(0) === BIB || t.getLinkUrl(n - 1) === BIB);
}

/** The bibliography's paragraphs (also list items), in the first tab that has one: [{body, index, el}]. */
function bibParagraphs_(doc) {
  var secs = sections_(doc);
  for (var k = 0; k < secs.length; k++) {
    var body = secs[k].body, out = [];
    for (var i = 0; i < body.getNumChildren(); i++) {
      var c = body.getChild(i), type = c.getType();
      if (type !== DocumentApp.ElementType.PARAGRAPH && type !== DocumentApp.ElementType.LIST_ITEM) continue;
      var p = type === DocumentApp.ElementType.PARAGRAPH ? c.asParagraph() : c.asListItem();
      if (isBibText_(p.editAsText())) out.push({ body: body, index: i, el: p });
    }
    if (out.length) return out;
  }
  return [];
}

var COPIED = ['FONT_FAMILY', 'FONT_SIZE', 'LINE_SPACING', 'SPACING_BEFORE', 'SPACING_AFTER', 'INDENT_START', 'INDENT_FIRST_LINE', 'INDENT_END', 'HORIZONTAL_ALIGNMENT'];

/**
 * Replaces the bibliography with `entries` (each a list of runs {text, italic, bold}); with
 * `create`, makes one after the paragraph with the cursor if there is none. The new paragraphs
 * keep the old ones' font, spacing and indents. Unchanged text is left alone unless `force`.
 * Returns whether the document changed.
 */
function writeBibliography_(doc, entries, create, force) {
  var found = bibParagraphs_(doc);
  var body = found.length ? found[0].body : null;
  if (!entries.length) entries = [[{ text: 'References will appear here.' }]];
  var texts = entries.map(function (runs) { return runs.map(function (r) { return r.text || ''; }).join(''); });
  if (found.length && !force && found.length === texts.length &&
      found.every(function (f, i) { return f.el.getText() === texts[i]; })) return false;
  var at, attrs = null;
  if (found.length) {
    at = found[0].index;
    var a = found[0].el.getAttributes();
    attrs = {};
    COPIED.forEach(function (k) { var v = a[DocumentApp.Attribute[k]]; if (v !== null && v !== undefined) attrs[DocumentApp.Attribute[k]] = v; });
  } else {
    if (!create) return false;
    var here = topLevelAtCursor_(doc);
    body = here.body;
    at = here.index + 1;
  }
  entries.forEach(function (runs, k) {
    var p = body.insertParagraph(at + k, '');
    if (attrs) p.setAttributes(attrs);
    var pos = 0;
    runs.forEach(function (run) {
      if (!run.text) return;
      p.appendText(run.text);
      var t = p.editAsText(), end = pos + run.text.length - 1;
      t.setItalic(pos, end, !!run.italic);
      t.setBold(pos, end, !!run.bold);
      pos = end + 1;
    });
    if (pos === 0) { p.appendText(' '); pos = 1; }
    var t = p.editAsText();
    t.setLinkUrl(0, pos - 1, BIB);
    plain_(t, 0, pos - 1);
    t.setTextAlignment(0, pos - 1, DocumentApp.TextAlignment.NORMAL);
  });
  // the old paragraphs (now after the new ones); the body's last paragraph can't be removed, so it is emptied
  for (var j = found.length - 1; j >= 0; j--) {
    var idx = found[j].index + entries.length;
    var old = body.getChild(idx);
    if (idx === body.getNumChildren() - 1) {
      var ot = old.editAsText();
      if (ot.getText().length) { ot.setLinkUrl(0, ot.getText().length - 1, null); }
      old.clear();
    } else {
      body.removeChild(old);
    }
  }
  return true;
}

/** The body (tab) and index in it of the top-level paragraph with the cursor (or the selection's end). */
function topLevelAtCursor_(doc) {
  var cursor = doc.getCursor(), el = null;
  if (cursor) el = cursor.getElement();
  else if (doc.getSelection()) {
    var els = doc.getSelection().getRangeElements();
    el = els[els.length - 1].getElement();
  }
  if (!el) { var b = doc.getBody(); return { body: b, index: b.getNumChildren() - 1 }; }
  if (el.getType() === DocumentApp.ElementType.BODY_SECTION) { var bb = el.asBody(); return { body: bb, index: bb.getNumChildren() - 1 }; }
  while (el.getParent() && el.getParent().getType() !== DocumentApp.ElementType.BODY_SECTION) {
    el = el.getParent();
    if (el.getType() === DocumentApp.ElementType.FOOTNOTE_SECTION) throw new Error('Click in the main text (not a footnote) where the bibliography should go.');
  }
  if (!el.getParent()) throw new Error('Click in the main text where the bibliography should go.');
  var body = el.getParent().asBody();
  return { body: body, index: body.getChildIndex(el) };
}

/** Makes a bibliography after the cursor's paragraph: {exists: true} if the document has one. */
function insertBibliography(entries) {
  var doc = DocumentApp.getActiveDocument();
  if (bibParagraphs_(doc).length) return { exists: true };
  writeBibliography_(doc, entries || [], true, true);
  return { created: true };
}
