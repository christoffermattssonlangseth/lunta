// A very small DOM for testing the reader scripts in node: parses well-formed HTML samples and
// supports what those scripts use (compound selectors in lists, closest, siblings, textContent,
// cloneNode, classList, style as a plain object), text nodes as the highlight scripts walk them
// (createTreeWalker, splitText, normalize, compareDocumentPosition), fragments (replaceChild) and
// event listeners (dispatched by hand). No combinators, no layout.

// the reader scripts use NodeFilter as a global
globalThis.NodeFilter ??= { SHOW_TEXT: 4, FILTER_ACCEPT: 1, FILTER_REJECT: 2, FILTER_SKIP: 3 };

const VOID = new Set(['area', 'base', 'br', 'col', 'embed', 'hr', 'img', 'input', 'link', 'meta', 'source', 'track', 'wbr']);
const ENT = { amp: '&', lt: '<', gt: '>', quot: '"', apos: "'", nbsp: ' ' };
const unescape = (s) => s.replace(/&(#x[0-9a-f]+|#\d+|\w+);/gi, (all, e) =>
  e[0] === '#' ? String.fromCodePoint(e[1] === 'x' || e[1] === 'X' ? parseInt(e.slice(2), 16) : parseInt(e.slice(1), 10)) : (ENT[e] ?? all));

class Node {
  constructor(doc) { this.ownerDocument = doc; this.parentNode = null; this.childNodes = []; }
  get parentElement() { return this.parentNode instanceof Element ? this.parentNode : null; }
  get isConnected() { let n = this; while (n.parentNode) n = n.parentNode; return n === this.ownerDocument; }
  remove() { if (this.parentNode) { const c = this.parentNode.childNodes; c.splice(c.indexOf(this), 1); this.parentNode = null; } }
  appendChild(n) { n.remove(); n.parentNode = this; this.childNodes.push(n); return n; }
  insertBefore(n, ref) {
    if (!ref) return this.appendChild(n);
    n.remove(); n.parentNode = this; this.childNodes.splice(this.childNodes.indexOf(ref), 0, n); return n;
  }
  get nextSibling() { if (!this.parentNode) return null; const c = this.parentNode.childNodes; return c[c.indexOf(this) + 1] ?? null; }
  get firstChild() { return this.childNodes[0] ?? null; }
  get lastChild() { return this.childNodes[this.childNodes.length - 1] ?? null; }
  removeChild(n) { n.remove(); return n; }
  /** Puts `n` (a fragment: its children) where `old` is. */
  replaceChild(n, old) {
    const kids = n instanceof Fragment ? [...n.childNodes] : [n];
    for (const k of kids) this.insertBefore(k, old);
    old.remove();
    return old;
  }
  contains(n) { for (let e = n; e; e = e.parentNode) if (e === this) return true; return false; }
  /** Merges adjacent text nodes and drops empty ones, as the DOM does. */
  normalize() {
    for (let i = 0; i < this.childNodes.length; i++) {
      const n = this.childNodes[i];
      if (n instanceof Text) {
        while (this.childNodes[i + 1] instanceof Text) { n.data += this.childNodes[i + 1].data; this.childNodes[i + 1].remove(); }
        if (!n.data) { n.remove(); i--; }
      } else n.normalize();
    }
  }
  /** DOCUMENT_POSITION_* bits for `other` relative to this node. */
  compareDocumentPosition(other) {
    if (other === this) return 0;
    if (this.contains(other)) return 16 | 4;
    if (other.contains(this)) return 8 | 2;
    const order = [];
    const walk = (n) => { order.push(n); n.childNodes.forEach(walk); };
    let root = this; while (root.parentNode) root = root.parentNode;
    walk(root);
    return order.indexOf(other) > order.indexOf(this) ? 4 : 2;
  }
  addEventListener(type, fn, capture) { (this.listeners ??= []).push({ type, fn, capture: !!capture }); }
  removeEventListener(type, fn) { this.listeners = (this.listeners ?? []).filter((l) => l.type !== type || l.fn !== fn); }
  /** Calls this node's listeners for `ev.type` (no propagation). */
  dispatch(ev) {
    ev.stopped ??= false;
    ev.preventDefault ??= () => { ev.defaultPrevented = true; };
    ev.stopPropagation ??= () => { ev.stopped = true; };
    ev.stopImmediatePropagation ??= () => { ev.stopped = true; ev.immediate = true; };
    for (const l of this.listeners ?? []) { if (l.type === ev.type) l.fn(ev); if (ev.immediate) break; }
    return ev;
  }
}

class Text extends Node {
  constructor(doc, data) { super(doc); this.nodeType = 3; this.data = data; }
  get nodeName() { return '#text'; }
  get nodeValue() { return this.data; }
  set nodeValue(v) { this.data = String(v); }
  get textContent() { return this.data; }
  /** Keeps the text before `off` here; the rest becomes the next node (returned). */
  splitText(off) {
    const rest = new Text(this.ownerDocument, this.data.slice(off));
    this.data = this.data.slice(0, off);
    if (this.parentNode) this.parentNode.insertBefore(rest, this.nextSibling);
    return rest;
  }
  cloneNode() { return new Text(this.ownerDocument, this.data); }
}

class Fragment extends Node {
  constructor(doc) { super(doc); this.nodeType = 11; }
  get nodeName() { return '#document-fragment'; }
  get textContent() { return this.childNodes.map((n) => n.textContent).join(''); }
}

class Element extends Node {
  constructor(doc, name) { super(doc); this.nodeType = 1; this.localName = name.toLowerCase(); this.attrs = new Map(); }
  get tagName() { return this.localName.toUpperCase(); }
  get nodeName() { return this.tagName; }
  get children() { return this.childNodes.filter((n) => n instanceof Element); }
  get firstElementChild() { return this.children[0] ?? null; }
  sib(d) {
    if (!this.parentNode) return null;
    const s = this.parentNode.childNodes;
    for (let i = s.indexOf(this) + d; i >= 0 && i < s.length; i += d) if (s[i] instanceof Element) return s[i];
    return null;
  }
  get nextElementSibling() { return this.sib(1); }
  get previousElementSibling() { return this.sib(-1); }
  get textContent() { return this.childNodes.map((n) => n.textContent).join(''); }
  set textContent(t) { this.childNodes = []; this.appendChild(new Text(this.ownerDocument, t)); }
  getAttribute(k) { return this.attrs.has(k) ? this.attrs.get(k) : null; }
  setAttribute(k, v) { this.attrs.set(k, String(v)); }
  hasAttribute(k) { return this.attrs.has(k); }
  removeAttribute(k) { this.attrs.delete(k); }
  get style() { return (this.styles ??= {}); }
  get id() { return this.getAttribute('id') ?? ''; }
  set id(v) { this.setAttribute('id', v); }
  get className() { return this.getAttribute('class') ?? ''; }
  set className(v) { this.setAttribute('class', v); }
  get classList() {
    const el = this;
    const list = () => el.className.split(/\s+/).filter(Boolean);
    return {
      contains: (c) => list().includes(c),
      add: (c) => { if (!list().includes(c)) el.setAttribute('class', [...list(), c].join(' ')); },
      remove: (c) => el.setAttribute('class', list().filter((x) => x !== c).join(' ')),
      toggle: (c, on) => { (on ?? !list().includes(c)) ? el.classList.add(c) : el.classList.remove(c); },
    };
  }
  cloneNode(deep) {
    const c = new Element(this.ownerDocument, this.localName);
    c.attrs = new Map(this.attrs);
    if (deep) for (const n of this.childNodes) c.appendChild(n.cloneNode(true));
    return c;
  }
  *descendants() { for (const c of this.children) { yield c; yield* c.descendants(); } }
  matches(sel) { return parseSelector(sel).some((s) => s(this)); }
  closest(sel) { const ss = parseSelector(sel); for (let e = this; e; e = e.parentElement) if (ss.some((s) => s(e))) return e; return null; }
  querySelectorAll(sel) {
    const ss = parseSelector(sel), out = [];
    for (const e of this.descendants()) if (ss.some((s) => s(e))) out.push(e);
    return out;
  }
  querySelector(sel) { return this.querySelectorAll(sel)[0] ?? null; }
  scrollIntoView(o) { this.ownerDocument.scrolledTo = this; this.ownerDocument.scrollOptions = o; }
}

// "a.b#c[x], [y*="z"]": a list of compound selectors
function parseSelector(sel) {
  return sel.split(',').map((part) => {
    const s = part.trim();
    const re = /([a-zA-Z][\w-]*)|\.([\w-]+)|#([\w.-]+)|\[([\w-]+)(?:([*^$]?=)"([^"]*)")?\]/y;
    const tests = [];
    let m;
    re.lastIndex = 0;
    while (re.lastIndex < s.length && (m = re.exec(s))) {
      if (m[1]) { const t = m[1].toLowerCase(); tests.push((e) => e.localName === t); }
      else if (m[2]) { const c = m[2]; tests.push((e) => e.classList.contains(c)); }
      else if (m[3]) { const i = m[3]; tests.push((e) => e.id === i); }
      else {
        const [k, op, v] = [m[4], m[5], m[6]];
        tests.push((e) => {
          const a = e.getAttribute(k);
          if (a === null) return false;
          if (!op) return true;
          if (op === '=') return a === v;
          if (op === '*=') return a.includes(v);
          if (op === '^=') return a.startsWith(v);
          return a.endsWith(v);
        });
      }
    }
    if (re.lastIndex !== s.length && s.length) throw new Error(`mini-dom: unsupported selector "${s}"`);
    return (e) => tests.every((t) => t(e));
  });
}

class Document extends Node {
  constructor() { super(null); this.ownerDocument = this; this.nodeType = 9; }
  get documentElement() { return this.childNodes.find((n) => n instanceof Element) ?? null; }
  get body() { return this.documentElement?.querySelector('body') ?? null; }
  createElement(n) { return new Element(this, n); }
  createTextNode(t) { return new Text(this, t); }
  createDocumentFragment() { return new Fragment(this); }
  get nodeName() { return '#document'; }
  /** SHOW_TEXT walkers only (what the reader scripts use), in document order. */
  createTreeWalker(root, what, filter) {
    const list = [];
    const walk = (n) => {
      for (const c of n.childNodes) {
        if (c instanceof Text) { if (!filter || filter.acceptNode(c) === NodeFilter.FILTER_ACCEPT) list.push(c); }
        else walk(c);
      }
    };
    walk(root);
    let i = 0;
    return { nextNode: () => list[i++] ?? null };
  }
  getElementById(id) { for (const e of this.documentElement.descendants()) if (e.id === id) return e; return null; }
  querySelectorAll(s) { return this.documentElement.querySelectorAll(s); }
  querySelector(s) { return this.documentElement.querySelector(s); }
}

/** Parses an HTML document (or a body fragment) into a Document. */
export { Element, Text };

export function parse(html) {
  if (!/<html[\s>]/i.test(html)) html = `<html><head></head><body>${html}</body></html>`;
  const doc = new Document();
  let cur = doc;
  const re = /<!--[\s\S]*?-->|<!DOCTYPE[^>]*>|<\?[^>]*>|<\/([\w:-]+)\s*>|<([\w:-]+)((?:\s+[\w:-]+(?:\s*=\s*(?:"[^"]*"|'[^']*'|[^\s>]+))?)*)\s*(\/?)>|([^<]+)/gi;
  let m;
  while ((m = re.exec(html))) {
    if (m[5] !== undefined) { cur.appendChild(new Text(doc, unescape(m[5]))); continue; }
    if (m[1]) {
      const name = m[1].toLowerCase();
      for (let e = cur; e && e !== doc; e = e.parentNode) if (e.localName === name) { cur = e.parentNode; break; }
      continue;
    }
    if (!m[2]) continue;
    const el = new Element(doc, m[2]);
    const ar = /([\w:-]+)(?:\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+)))?/g;
    let a;
    while ((a = ar.exec(m[3] || ''))) el.setAttribute(a[1].toLowerCase(), unescape(a[2] ?? a[3] ?? a[4] ?? ''));
    cur.appendChild(el);
    if (!m[4] && !VOID.has(el.localName)) cur = el;
  }
  return doc;
}

/** Runs a reader script against a document; returns the window object it filled in. */
export function run(script, doc, extra = {}) {
  const win = { document: doc, ...extra };
  // eslint-disable-next-line no-new-func
  new Function('window', 'document', script)(win, doc);
  return win;
}
