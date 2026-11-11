// Margins citations for Google Docs (Apps Script, bound to your document).
// Citations are text linked to https://margins.local/cite?ids=…; bibliography paragraphs are
// linked to https://margins.local/bib. The sidebar talks to the Margins app on your Mac.
var CITE = 'https://margins.local/cite?ids=';
var BIB = 'https://margins.local/bib';

function onOpen() {
  DocumentApp.getUi().createMenu('Margins').addItem('Cite…', 'showSidebar').addToUi();
}

function showSidebar() {
  DocumentApp.getUi().showSidebar(HtmlService.createHtmlOutputFromFile('Sidebar').setTitle('Margins'));
}

/** Every citation run in document order: [{el, start, end, ids}]. */
function citationRuns_(body) {
  var out = [];
  var search = null;
  while ((search = body.findElement(DocumentApp.ElementType.TEXT, search))) {
    var t = search.getElement().asText();
    var idx = t.getTextAttributeIndices();
    var len = t.getText().length;
    for (var i = 0; i < idx.length; i++) {
      var s = idx[i], e = (i + 1 < idx.length ? idx[i + 1] : len) - 1;
      if (e < s) continue;
      var url = t.getLinkUrl(s);
      if (url && url.indexOf(CITE) === 0) {
        // merge with a previous run of the same link (Docs may split runs on formatting)
        var last = out[out.length - 1];
        if (last && last.el === t && last.end + 1 === s && last.url === url) { last.end = e; continue; }
        out.push({ el: t, start: s, end: e, url: url, ids: decodeURIComponent(url.slice(CITE.length)).split(',') });
      }
    }
  }
  return out;
}

/** Citations in order, as lists of ids (for numbering). */
function listCitations() {
  return citationRuns_(DocumentApp.getActiveDocument().getBody()).map(function (r) { return r.ids; });
}

function insertCitation(ids, text, superscript) {
  var doc = DocumentApp.getActiveDocument();
  var cursor = doc.getCursor();
  if (!cursor) throw new Error('Click in the document where the citation should go.');
  var t = cursor.getSurroundingText(), at = cursor.getSurroundingTextOffset();
  if (!t) throw new Error('Can’t insert here. Click inside a paragraph.');
  t.insertText(at, text);
  var end = at + text.length - 1;
  t.setLinkUrl(at, end, CITE + encodeURIComponent(ids.join(',')));
  plain_(t, at, end);
  t.setTextAlignment(at, end, superscript ? DocumentApp.TextAlignment.SUPERSCRIPT : DocumentApp.TextAlignment.NORMAL);
  doc.setCursor(doc.newPosition(t, end + 1));
}

/** Rewrites every citation's text (texts[i] for the i-th citation) and the bibliography. */
function applyAll(texts, superscript, bibliography) {
  var body = DocumentApp.getActiveDocument().getBody();
  var runs = citationRuns_(body);
  for (var i = runs.length - 1; i >= 0; i--) {
    var r = runs[i], text = texts[i];
    if (text === undefined) continue;
    r.el.deleteText(r.start, r.end);
    r.el.insertText(r.start, text);
    var end = r.start + text.length - 1;
    r.el.setLinkUrl(r.start, end, r.url);
    plain_(r.el, r.start, end);
    r.el.setTextAlignment(r.start, end, superscript ? DocumentApp.TextAlignment.SUPERSCRIPT : DocumentApp.TextAlignment.NORMAL);
  }
  writeBibliography_(body, bibliography, false);
  return runs.length;
}

/** Links look like normal text: no underline, no link color. */
function plain_(t, s, e) {
  t.setUnderline(s, e, false);
  try { t.setForegroundColor(s, e, null); } catch (x) { t.setForegroundColor(s, e, '#000000'); }
}

function isBib_(p) {
  var t = p.editAsText();
  return t.getText().length > 0 && t.getLinkUrl(0) === BIB;
}

/** Replaces the bibliography paragraphs (or inserts them at the cursor when `create`). */
function writeBibliography_(body, entries, create) {
  var at = -1;
  for (var i = body.getNumChildren() - 1; i >= 0; i--) {
    var c = body.getChild(i);
    if (c.getType() === DocumentApp.ElementType.PARAGRAPH && isBib_(c.asParagraph())) { at = i; body.removeChild(c); }
  }
  if (at < 0) {
    if (!create) return;
    var cursor = DocumentApp.getActiveDocument().getCursor();
    var el = cursor ? cursor.getElement() : null;
    while (el && el.getParent() && el.getParent().getType() !== DocumentApp.ElementType.BODY_SECTION) el = el.getParent();
    at = el ? body.getChildIndex(el) + 1 : body.getNumChildren();
  }
  if (!entries.length) entries = [[{ text: 'References will appear here.' }]];
  entries.forEach(function (runs, k) {
    var p = body.insertParagraph(at + k, '');
    var pos = 0;
    runs.forEach(function (run) {
      if (!run.text) return;
      p.appendText(run.text);
      var t = p.editAsText(), end = pos + run.text.length - 1;
      t.setItalic(pos, end, !!run.italic);
      t.setBold(pos, end, !!run.bold);
      pos = end + 1;
    });
    var t = p.editAsText();
    t.setLinkUrl(0, pos - 1, BIB);
    plain_(t, 0, pos - 1);
    t.setTextAlignment(0, pos - 1, DocumentApp.TextAlignment.NORMAL);
  });
}

function insertBibliography(entries) {
  writeBibliography_(DocumentApp.getActiveDocument().getBody(), entries, true);
}

function getStyle() {
  return PropertiesService.getDocumentProperties().getProperty('marginsStyle') || 'apa';
}

function setStyle(s) {
  PropertiesService.getDocumentProperties().setProperty('marginsStyle', s);
}
