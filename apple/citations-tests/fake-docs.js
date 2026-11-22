// A small stand-in for Apps Script's DocumentApp, PropertiesService, LockService and Utilities:
// enough of them to run gdocs/Code.gs in node. Text keeps attributes per character, as Docs does
// (inserted text takes the attributes of the character before it).
const zlib = require('node:zlib');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const ElementType = { BODY_SECTION: 'BODY_SECTION', PARAGRAPH: 'PARAGRAPH', LIST_ITEM: 'LIST_ITEM', TEXT: 'TEXT',
  FOOTNOTE: 'FOOTNOTE', FOOTNOTE_SECTION: 'FOOTNOTE_SECTION' };
const TextAlignment = { NORMAL: 'NORMAL', SUPERSCRIPT: 'SUPERSCRIPT', SUBSCRIPT: 'SUBSCRIPT' };
const Attribute = {};
['FONT_FAMILY', 'FONT_SIZE', 'LINE_SPACING', 'SPACING_BEFORE', 'SPACING_AFTER', 'INDENT_START', 'INDENT_FIRST_LINE', 'INDENT_END', 'HORIZONTAL_ALIGNMENT']
  .forEach((k) => { Attribute[k] = k; });

class Text {
  constructor(s, attrs) { this.parent = null; this.chars = [...(s || '')].map((c) => ({ c, a: Object.assign({}, attrs || {}) })); }
  getType() { return ElementType.TEXT; }
  asText() { return this; }
  editAsText() { return this; }
  getParent() { return this.parent; }
  getText() { return this.chars.map((x) => x.c).join(''); }
  check(s, e) { if (s < 0 || e >= this.chars.length || s > e) throw new Error(`Index out of range ${s}..${e} of ${this.chars.length}`); }
  getTextAttributeIndices() {
    const out = [];
    let prev = null;
    this.chars.forEach((x, i) => { const k = JSON.stringify(x.a); if (k !== prev) out.push(i); prev = k; });
    return out.length ? out : [0];
  }
  attr(i, k) { if (i < 0 || i >= this.chars.length) throw new Error('offset ' + i); const v = this.chars[i].a[k]; return v === undefined ? null : v; }
  set(s, e, k, v) { this.check(s, e); for (let i = s; i <= e; i++) { if (v === null) delete this.chars[i].a[k]; else this.chars[i].a[k] = v; } return this; }
  getLinkUrl(i) { return this.attr(i, 'link'); }
  setLinkUrl(s, e, url) { return this.set(s, e, 'link', url); }
  getTextAlignment(i) { return this.attr(i, 'align') || TextAlignment.NORMAL; }
  setTextAlignment(s, e, a) { return this.set(s, e, 'align', a === TextAlignment.NORMAL ? null : a); }
  setUnderline(s, e, v) { return this.set(s, e, 'underline', v ? true : null); }
  setForegroundColor(s, e, v) { return this.set(s, e, 'color', v); }
  setItalic(s, e, v) { return this.set(s, e, 'italic', v ? true : null); }
  isItalic(i) { return !!this.attr(i, 'italic'); }
  setBold(s, e, v) { return this.set(s, e, 'bold', v ? true : null); }
  insertText(off, s) {
    if (off < 0 || off > this.chars.length) throw new Error('insert offset ' + off);
    if (!s) throw new Error('Text cannot be empty');
    // named ranges over this text move with it (text inserted right after one isn't in it)
    const n = [...s].length;
    (this.ranges || []).forEach((x) => { if (x.s >= off) { x.s += n; x.e += n; } else if (x.e >= off) x.e += n; });
    const like = this.chars[off - 1] || this.chars[off];
    const a = like ? like.a : {};
    this.chars.splice(off, 0, ...[...s].map((c) => ({ c, a: Object.assign({}, a) })));
    return this;
  }
  deleteText(s, e) {
    this.check(s, e);
    this.chars.splice(s, e - s + 1);
    const n = e - s + 1;
    (this.ranges || []).slice().forEach((x) => {
      if (x.s >= s && x.e <= e) { x.gone(); return; }   // all its text deleted: Docs removes the range
      const cut = (i) => (i > e ? i - n : i >= s ? s : i);
      x.s = cut(x.s); x.e = x.e > e ? x.e - n : Math.min(x.e, s - 1);
    });
    return this;
  }
  appendText(s) { return this.insertText(this.chars.length, s); }
}

class Container {
  constructor(type, children) { this.type = type; this.parent = null; this.children = []; this.attrs = {}; (children || []).forEach((c) => this.append(c)); }
  getType() { return this.type; }
  getParent() { return this.parent; }
  append(c) { c.parent = this; this.children.push(c); return c; }
  getNumChildren() { return this.children.length; }
  getChild(i) { if (!this.children[i]) throw new Error('no child ' + i); return this.children[i]; }
  getChildIndex(c) { const i = this.children.indexOf(c); if (i < 0) throw new Error('not a child'); return i; }
  asParagraph() { if (this.type !== ElementType.PARAGRAPH) throw new Error('not a paragraph'); return this; }
  asListItem() { if (this.type !== ElementType.LIST_ITEM) throw new Error('not a list item'); return this; }
  asBody() { if (this.type !== ElementType.BODY_SECTION) throw new Error('not a body'); return this; }
  // Paragraph-like: one Text child stands for the paragraph's text
  text() { let t = this.children.find((c) => c instanceof Text); if (!t) { t = new Text(''); this.append(t); } return t; }
  editAsText() { return this.text(); }
  getText() { return this.children.filter((c) => c instanceof Text).map((c) => c.getText()).join(''); }
  appendText(s) { return this.text().appendText(s); }
  clear() { this.children = []; return this; }
  getAttributes() { return Object.assign({}, this.attrs); }
  setAttributes(a) { Object.assign(this.attrs, a); return this; }
  findElement(type, from) {
    const all = [];
    (function walk(el) {
      if (el.getType() === type) all.push(el);
      if (el instanceof Footnote) return;   // footnote text isn't in the body
      (el.children || []).forEach(walk);
    })(this);
    const i = from ? all.indexOf(from.getElement()) + 1 : 0;
    return i < all.length ? { getElement: () => all[i] } : null;
  }
  getParagraphs() {
    const out = [];
    (function walk(el) {
      if (el.getType() === ElementType.PARAGRAPH || el.getType() === ElementType.LIST_ITEM) out.push(el);
      if (el instanceof Footnote) return;
      (el.children || []).forEach(walk);
    })(this);
    return out;
  }
  // Body
  insertParagraph(i, s) { const p = new Container(ElementType.PARAGRAPH, s ? [new Text(s)] : []); p.parent = this; this.children.splice(i, 0, p); return p; }
  removeChild(c) {
    const i = this.getChildIndex(c);
    if (this.type === ElementType.BODY_SECTION && this.children.length === 1) throw new Error('Can’t remove the last paragraph in a document section.');
    if (this.type === ElementType.BODY_SECTION && i === this.children.length - 1) throw new Error('Can’t remove the last paragraph in a document section.');
    this.children.splice(i, 1);
    return this;
  }
}

class Footnote {
  constructor(paragraphs) { this.parent = null; this.section = new Container(ElementType.FOOTNOTE_SECTION, paragraphs); }
  getType() { return ElementType.FOOTNOTE; }
  getParent() { return this.parent; }
  getFootnoteContents() { return this.section; }
}

const P = (...kids) => new Container(ElementType.PARAGRAPH, kids.map((k) => typeof k === 'string' ? new Text(k) : k));
const LI = (...kids) => new Container(ElementType.LIST_ITEM, kids.map((k) => typeof k === 'string' ? new Text(k) : k));
const Body = (...paras) => new Container(ElementType.BODY_SECTION, paras);

/** Named ranges, as Apps Script keeps them per tab: names of 1–256 characters. */
function namedRanges(list) {
  return {
    getNamedRanges: () => list.slice(),
    addNamedRange: (name, range) => {
      if (!name || name.length > 256) throw new Error('Invalid argument: name');
      const nr = { name, range, getName: () => name, getRange: () => range, remove: () => { const i = list.indexOf(nr); if (i >= 0) list.splice(i, 1); } };
      list.push(nr);
      // its own copy of the range's offsets, kept up to date as the text changes
      const els = range.els.map((x) => Object.assign({}, x));
      nr.range = { getRangeElements: () => els.map((x) => ({ getElement: () => x.el, getStartOffset: () => x.s, getEndOffsetInclusive: () => x.e, isPartial: () => x.s != null })) };
      nr.getRange = () => nr.range;
      els.forEach((x) => {
        if (x.el instanceof Text && x.s != null) {
          x.gone = () => { nr.remove(); x.el.ranges = x.el.ranges.filter((y) => y !== x); };
          x.el.ranges = (x.el.ranges || []).concat([x]);
        }
      });
      return nr;
    },
    newRange: () => {
      const r = { els: [] };
      r.addElement = (el, s, e) => { r.els.push({ el, s, e }); return r; };
      r.addElementsBetween = (a, b) => { r.els.push({ el: a }, { el: b }); return r; };
      r.build = () => r;
      return r;
    }
  };
}

/** A document: body (or tabs: [{body}]), the cursor {text, offset} and what scripts store. */
function makeDocument(opts) {
  const tabs = opts.tabs || [{ body: opts.body }];
  tabs.forEach((t) => { t.named = t.named || []; });
  const footnotesOf = (body) => { const out = []; (function walk(el) { if (el instanceof Footnote) out.push(el); else (el.children || []).forEach(walk); })(body); return out; };
  const doc = {
    cursor: opts.cursor || null, selection: null,
    getBody: () => tabs[0].body,
    getFootnotes: () => footnotesOf(tabs[0].body),
    getCursor: () => doc.cursor && {
      getSurroundingText: () => doc.cursor.text,
      getSurroundingTextOffset: () => doc.cursor.offset,
      getElement: () => doc.cursor.text
    },
    getSelection: () => doc.selection,
    setCursor: () => {},
    newPosition: (el, off) => ({ el, off }),
    setSelection: (r) => { doc.selected = r; },
    named: tabs[0].named
  };
  // before tabs, Document had the named ranges (of its only tab)
  Object.assign(doc, namedRanges(tabs[0].named));
  if (opts.tabs) {
    doc.getTabs = () => tabs.map((t) => ({
      getType: () => 'DOCUMENT_TAB', getChildTabs: () => [],
      asDocumentTab: () => Object.assign({ getBody: () => t.body, getFootnotes: () => footnotesOf(t.body) }, namedRanges(t.named))
    }));
  }
  return doc;
}

function properties(limitValue) {
  const data = {};
  return {
    data,
    getProperties: () => Object.assign({}, data),
    getProperty: (k) => (k in data ? data[k] : null),
    setProperty: (k, v) => { if (String(v).length > limitValue) throw new Error('Argument too large: value'); data[k] = String(v); },
    setProperties: (m) => {
      Object.keys(m).forEach((k) => { if (String(m[k]).length > limitValue) throw new Error('Argument too large: value'); });
      const total = Object.keys(Object.assign({}, data, m)).reduce((n, k) => n + k.length + String(k in m ? m[k] : data[k]).length, 0);
      if (total > 500 * 1024) throw new Error('You have exceeded the property storage quota.');
      Object.keys(m).forEach((k) => { data[k] = String(m[k]); });
    },
    deleteProperty: (k) => { delete data[k]; }
  };
}

function blob(bytes) {
  const b = Buffer.from(bytes);
  return { getBytes: () => [...b].map((x) => (x > 127 ? x - 256 : x)), getDataAsString: () => b.toString('utf8'), setContentType: () => {}, raw: b };
}
const Utilities = {
  newBlob: (data) => blob(typeof data === 'string' ? Buffer.from(data, 'utf8') : Buffer.from(data.map((x) => x & 255))),
  gzip: (b) => blob(zlib.gzipSync(b.raw)),
  ungzip: (b) => blob(zlib.gunzipSync(b.raw)),
  base64Encode: (bytes) => Buffer.from(bytes.map((x) => x & 255)).toString('base64'),
  base64Decode: (s) => [...Buffer.from(s, 'base64')].map((x) => (x > 127 ? x - 256 : x)),
  base64EncodeWebSafe: (bytes) => Buffer.from(bytes.map((x) => x & 255)).toString('base64').replace(/\+/g, '-').replace(/\//g, '_'),
  base64DecodeWebSafe: (s) => [...Buffer.from(s.replace(/-/g, '+').replace(/_/g, '/'), 'base64')].map((x) => (x > 127 ? x - 256 : x))
};

/** Code.gs loaded against a document. */
function load(doc) {
  const docProps = properties(9 * 1024), userProps = properties(9 * 1024);
  const ctx = {
    DocumentApp: { ElementType, TextAlignment, Attribute, TabType: { DOCUMENT_TAB: 'DOCUMENT_TAB' }, getActiveDocument: () => doc, getUi: () => ({}) },
    PropertiesService: { getDocumentProperties: () => docProps, getUserProperties: () => userProps },
    LockService: { getDocumentLock: () => ({ tryLock: () => true, releaseLock: () => {} }) },
    Utilities, HtmlService: {}, console
  };
  vm.createContext(ctx);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '..', 'citations', 'gdocs', 'Code.gs'), 'utf8'), ctx, { filename: 'Code.gs' });
  ctx.docProps = docProps;
  // results come back across the vm boundary: plain copies for deepEqual
  ctx.call = (fn, ...args) => { const r = ctx[fn](...args); return r === undefined ? undefined : JSON.parse(JSON.stringify(r)); };
  return ctx;
}

module.exports = { Text, Container, Footnote, P, LI, Body, makeDocument, load, TextAlignment };
