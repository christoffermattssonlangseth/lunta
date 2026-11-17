// Lunta citations for Google Docs (Apps Script, bound to your document).
// Citations are text linked to https://margins.local/cite?ids=…; bibliography paragraphs are
// linked to https://margins.local/bib. The sidebar talks to the Lunta app on your Mac; this script
// only reads and writes the document. Paste this file and Sidebar.html again when Lunta says so.
var CODE_VERSION = 2;
var CITE = 'https://margins.local/cite?ids=';
var BIB = 'https://margins.local/bib';
// What the document keeps of the papers it cites (so a co-author, another library or a deleted
// paper doesn't break it), gzipped in chunks: document properties hold 9 KB per value.
var STORE = 'lunta.store.', CHUNK = 8000;

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
  if (!tabs || !tabs.length) return [{ body: doc.getBody(), footnotes: doc.getFootnotes() || [] }];
  (function walk(list) {
    list.forEach(function (tab) {
      if (tab.getType() === DocumentApp.TabType.DOCUMENT_TAB) {
        var d = tab.asDocumentTab();
        out.push({ body: d.getBody(), footnotes: d.getFootnotes() || [] });
      }
      walk(tab.getChildTabs() || []);
    });
  })(tabs);
  return out;
}

/** Every citation in document order (tab by tab), footnotes included where their marks are. */
function citations_(doc) {
  return [].concat.apply([], sections_(doc).map(function (sec) {
    var runs = scan_(sec.body);
    var notes = [];
    sec.footnotes.forEach(function (fn) {
      var c = fn.getFootnoteContents();
      var rs = c ? scan_(c) : [];
      if (rs.length) notes.push({ path: path_(fn), runs: rs });
    });
    if (!notes.length) return runs;
    var items = runs.map(function (r) { return { path: path_(r.el).concat([r.start]), runs: [r] }; }).concat(notes);
    items.sort(function (a, b) { return before_(a.path, b.path) ? -1 : before_(b.path, a.path) ? 1 : 0; });
    return [].concat.apply([], items.map(function (x) { return x.runs; }));
  }));
}

/** Citations in order, as lists of ids. */
function listCitations() {
  return citations_(DocumentApp.getActiveDocument()).map(function (r) { return r.ids; });
}

// ---- what the sidebar needs ----

/** The document's citations, style and saved papers, and this user's pairing key (if set). */
function getState() {
  var doc = DocumentApp.getActiveDocument();
  var props = PropertiesService.getDocumentProperties().getProperties();
  var key = '';
  try { key = PropertiesService.getUserProperties().getProperty('luntaPairingKey') || ''; } catch (e) {}
  return {
    version: CODE_VERSION,
    citations: citations_(doc).map(function (r) { return r.ids; }),
    style: props.luntaStyle || '',
    styleTitle: props.luntaStyleTitle || '',
    papers: readStore_(props),
    hasBibliography: bibParagraphs_(doc).length > 0,
    key: key
  };
}

function setStyle(s, title) {
  var p = PropertiesService.getDocumentProperties();
  p.setProperty('luntaStyle', s);
  if (title) p.setProperty('luntaStyleTitle', title); else p.deleteProperty('luntaStyleTitle');
}

function getStyle() {
  return PropertiesService.getDocumentProperties().getProperty('luntaStyle') || 'apa';
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
      r.el.insertText(r.end + 1, text);
      r.el.deleteText(r.start, r.end);
      var end = r.start + text.length - 1;
      r.el.setLinkUrl(r.start, end, r.url);
      plain_(r.el, r.start, end);
      r.el.setTextAlignment(r.start, end, align);
      updated++;
    }
    var props = PropertiesService.getDocumentProperties();
    var bibChanged = false;
    if (plan.bibliography) bibChanged = writeBibliography_(doc, plan.bibliography, false, props.getProperty('luntaBibStyle') !== plan.style);
    props.setProperty('luntaStyle', plan.style);
    props.setProperty('luntaBibStyle', plan.style);
    if (plan.styleTitle) props.setProperty('luntaStyleTitle', plan.styleTitle);
    var saved = plan.papers ? writeStore_(plan.papers) : true;
    return { changed: false, count: runs.length, updated: updated, bibliography: bibChanged, saved: saved };
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
