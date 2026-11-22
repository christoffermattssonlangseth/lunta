// Books read chapter by chapter (apple/Shared/ChapterReader.swift): each part's page says where its
// text starts in the whole reading view's text (<meta name="lunta-text-base">), and the page
// scripts count every offset the app gets or gives from there: highlights (Highlights.swift) and
// the reading place (ReadingPosition.swift), so they are stored the same whether the book is read
// whole or chapter by chapter. Citations find their references in the reference list's chapter
// (luntaSetShelf, CitationPopover.swift; References.swift), and turning past a chapter's last page
// asks the app for the next chapter (PageTurn.swift).
// Run: node --test scripts/reader-tests/chapters.test.mjs

import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parse, run, Element } from './mini-dom.mjs';

const repo = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
function literal(file, name) {
  const src = readFileSync(join(repo, file), 'utf8').replace(/\r\n/g, '\n');
  const m = src.match(new RegExp(`static let ${name} = #"""\\n([\\s\\S]*?)\\n([ \\t]*)"""#`));
  assert.ok(m, `${name} literal`);
  return m[1].split('\n').map((l) => l.slice(m[2].length)).join('\n');
}
const annotationJS = literal('apple/Shared/Highlights.swift', 'annotationJS').split('CSS').join('');
const positionJS = literal('apple/Shared/ReadingPosition.swift', 'positionJS');
const citationJS = literal('apple/Shared/CitationPopover.swift', 'citationJS');
const referencesJS = literal('apple/Shared/References.swift', 'referencesJS');
const pagedJS = literal('apple/Shared/PageTurn.swift', 'pagedJS');
globalThis.Element = Element;

const BASE = 975505;   // where "Bibliography" starts in the textbook's whole reading view
const part = (body, base = BASE) =>
  `<html><head><meta name="lunta-text-base" content="${base}"/><meta name="lunta-part" content="3" data-parts="27"/></head><body class="continuous lunta-part">${body}</body></html>`;

function highlights(html) {
  const doc = parse(html);
  let selection = null;
  const sel = {
    get rangeCount() { return selection ? 1 : 0; },
    get isCollapsed() { return !selection; },
    getRangeAt: () => selection,
    toString: () => 'x',
    removeAllRanges: () => { selection = null; },
  };
  const win = run(annotationJS, doc, { webkit: { messageHandlers: { ann: { postMessage() {} } } }, getSelection: () => sel, addEventListener() {} });
  let text = '';
  const walk = (n) => n.childNodes.forEach((c) => (c.nodeType === 3 ? (text += c.data) : walk(c)));
  walk(doc.body);
  return {
    doc, win, text,
    select(node, from, to) { selection = { startContainer: node, startOffset: from, endContainer: node, endOffset: to }; },
    apply: (list) => JSON.parse(win.luntaApplyAnnotations(JSON.stringify(list))),
    marks: () => doc.querySelectorAll('mark.m-hl'),
  };
}

const CHAPTER = '\n<div class="section-start" id="section-7"></div>\n<section><h1 id="s88">5 Loss functions</h1><p id="p">A loss function returns a single number that describes the mismatch.</p></section>\n';

test('a selection in a chapter: its start in the whole reading view\'s text', () => {
  const p = highlights(part(CHAPTER, 119141));
  const t = p.doc.getElementById('p').childNodes[0];
  p.select(t, t.data.indexOf('single'), t.data.indexOf(' that'));
  const s = JSON.parse(p.win.luntaSelection());
  assert.equal(s.quote, 'single number');
  assert.equal(s.start, 119141 + p.text.indexOf('single number'));
  assert.equal(s.prefix, p.text.slice(p.text.indexOf('single number') - 40, p.text.indexOf('single number')));
});

test('a highlight stored with its place in the whole view is drawn in its chapter', () => {
  const p = highlights(part(CHAPTER, 119141));
  const at = p.text.indexOf('number');
  // the same words twice in the chapter would be told apart by the place; here once
  const missing = p.apply([{ id: 'h1', quote: 'number', prefix: 'single ', suffix: ' that', color: 'blue', start: 119141 + at }]);
  assert.deepEqual(missing, []);
  assert.equal(p.marks().map((m) => m.textContent).join(''), 'number');
});

test('the place counts: of two equal passages, the one at the stored place', () => {
  const body = '\n<p id="a">Gradient descent again.</p>\n<p id="b">Gradient descent again.</p>\n';
  const p = highlights(part(body, 5000));
  const second = p.text.lastIndexOf('Gradient descent');
  p.apply([{ id: 'h2', quote: 'Gradient descent', prefix: '', suffix: '', color: 'yellow', start: 5000 + second }]);
  const m = p.marks();
  assert.equal(m.length, 1);
  assert.equal(m[0].parentNode.id, 'b');
});

test('merging with a highlight already there: the new passage\'s offsets are in the whole view too', () => {
  const p = highlights(part(CHAPTER, 119141));
  const at = p.text.indexOf('single');
  p.apply([{ id: 'old', quote: 'single', prefix: '', suffix: '', color: 'yellow', start: 119141 + at }]);
  const n = p.text.indexOf('number');
  const r = JSON.parse(p.win.luntaMergeRange(119141 + n, 119141 + n + 'number'.length, ''));
  assert.deepEqual(r.ids, ['old']);
  assert.equal(r.quote, 'single number');
  assert.equal(r.start, 119141 + at);
});

test('a paper (no lunta-text-base): offsets as before', () => {
  const p = highlights('<p id="p">Plain paper text here.</p>');
  const t = p.doc.getElementById('p').childNodes[0];
  p.select(t, 6, 11);
  assert.equal(JSON.parse(p.win.luntaSelection()).start, 6);
});

test('the reading place: saved and restored in the whole view\'s text', () => {
  const doc = parse(part(CHAPTER, 233478));
  doc.images = [];
  const timers = [];
  const win = run(positionJS, doc, { setTimeout: (f) => timers.push(f), setInterval: () => 0, clearInterval() {}, addEventListener() {}, removeEventListener() {} });
  // pages: the place the pages are kept at (an offset in this page)
  win.luntaPageAnchor = () => 40;
  assert.equal(win.luntaPlace(), 233478 + 40);
  // going to a saved place: this page's own offset, and the place is held as given
  const went = [], kept = [];
  win.luntaScrollToOffset = (o) => { went.push(o); return true; };
  win.luntaKeepPlace = (o) => kept.push(o);
  win.luntaHoldOffset(233478 + 120);
  assert.deepEqual([went[0], kept[0]], [120, 120]);
  assert.equal(win.luntaPlace(), 233478 + 120);
});

// ---- citations and the reference list in another chapter ----

const BIB = part('\n<div class="section-start" id="section-28"></div>\n<section><h1 id="s686">Bibliography</h1><ol>' +
  '<li>Abdal, R., Qin, Y., &amp; Wonka, P. (2019). Image2StyleGAN. ICCV, 4432–4441.</li>' +
  '<li id="r-kingma">Kingma, D. P., &amp; Ba, J. (2015). Adam: A method for stochastic optimization. ICLR. arXiv:1412.6980</li>' +
  '</ol></section>\n', BASE);

// (mini-dom has no combinators: "ol.refs > li", the scripts' last resort, finds nothing here)
const noChild = (doc) => { const q = doc.querySelectorAll.bind(doc); doc.querySelectorAll = (sel) => (sel.includes('>') ? [] : q(sel)); return doc; };
function citePage(html) {
  const doc = noChild(parse(html));
  const El = doc.createElement('x').constructor;
  const listeners = [];
  const on = function (type, fn) { listeners.push({ node: this, type, fn }); };
  El.prototype.addEventListener = on;
  doc.addEventListener = on;
  El.prototype.getBoundingClientRect = () => ({ left: 10, top: 10, right: 30, bottom: 25, width: 20, height: 15 });
  Object.defineProperty(El.prototype, 'style', { configurable: true, get() { return (this._style ??= {}); } });
  Object.defineProperty(El.prototype, 'offsetWidth', { configurable: true, get: () => 300 });
  Object.defineProperty(El.prototype, 'offsetHeight', { configurable: true, get: () => 100 });
  Object.defineProperty(El.prototype, 'innerHTML', { configurable: true, get() { return this.textContent; }, set(t) { this.textContent = t; } });
  // the page's DOMParser: the reference list's chapter, parsed (mini-dom)
  class DOMParser { parseFromString(s) { return noChild(parse(s)); } }
  const location = { href: 'file:///book/lunta-part-6.xhtml' };
  const win = run(citationJS, doc, {
    innerWidth: 800, innerHeight: 600, DOMParser, location,
    getComputedStyle: () => ({ backgroundColor: '#fff', color: '#000' }),
    getSelection: () => ({ isCollapsed: true }),
    addEventListener: () => {},
  });
  globalThis.DOMParser = DOMParser;
  new Function('window', 'document', referencesJS)(win, doc);
  return { doc, win, listeners, location };
}
function click(p, target) {
  let stopped = false;
  const ev = { target, preventDefault() {}, stopPropagation() { stopped = true; } };
  for (const l of p.listeners) if (l.node === p.doc && l.type === 'click') l.fn(ev);
  return stopped;
}

test('a citation linked to the reference list in another chapter shows its reference (the shelf)', () => {
  const p = citePage(part('<p>Adam <a id="c" href="lunta-part-25.xhtml#r-kingma">(Kingma &amp; Ba, 2015)</a> is used.</p>', 155744));
  p.win.luntaSetShelf(BIB, 'lunta-part-25.xhtml');
  const saved = {};
  for (const k of ['getComputedStyle', 'getSelection', 'innerWidth', 'innerHeight']) saved[k] = globalThis[k];
  Object.assign(globalThis, { getComputedStyle: p.win.getComputedStyle, getSelection: p.win.getSelection, innerWidth: 800, innerHeight: 600 });
  try {
    assert.equal(click(p, p.doc.getElementById('c')), true);
    const c = p.doc.documentElement.children.find((e) => e.className === 'm-cite');
    assert.ok(c, 'the card');
    assert.match(c.querySelector('.m-cite-ref').textContent, /^Kingma, D\. P\., & Ba, J\. \(2015\)\. Adam/);
    const btns = c.querySelectorAll('a').filter((a) => a.classList.contains('m-cite-btn'));
    assert.ok(btns.some((b) => b.getAttribute('href') === 'lunta://cite?arxiv=1412.6980'));
    // Show in References: the reference list's chapter, at this entry (the second)
    const go = btns.find((b) => b.textContent === 'Show in References');
    const l = p.listeners.find((x) => x.node === go && x.type === 'click');
    l.fn({ preventDefault() {}, stopPropagation() {} });
    assert.equal(p.location.href, 'lunta-part-25.xhtml#lunta-ref-1');
  } finally { for (const k of Object.keys(saved)) { if (saved[k] === undefined) delete globalThis[k]; else globalThis[k] = saved[k]; } }
  // the shelf is no part of this page: its text isn't the page's
  assert.ok(!p.doc.body.textContent.includes('Abdal'));
});

test('an author–year citation in plain text finds its reference in the reference list\'s chapter', () => {
  const p = citePage(part('<p id="t">We train with Adam (Kingma &amp; Ba, 2015) throughout.</p>', 155744));
  p.win.luntaSetShelf(BIB, 'lunta-part-25.xhtml');
  const text = p.doc.getElementById('t').childNodes[0];
  p.doc.caretRangeFromPoint = () => ({ startContainer: text, startOffset: text.data.indexOf('Kingma') + 1 });
  p.doc.createRange = () => ({ setStart() {}, setEnd() {}, getBoundingClientRect: () => ({ left: 10, top: 10, right: 30, bottom: 25, width: 20, height: 15 }) });
  const saved = {};
  for (const k of ['getComputedStyle', 'getSelection', 'innerWidth', 'innerHeight', 'document']) saved[k] = globalThis[k];
  Object.assign(globalThis, { getComputedStyle: p.win.getComputedStyle, getSelection: p.win.getSelection, innerWidth: 800, innerHeight: 600, document: p.doc });
  try {
    assert.equal(click(p, text.parentNode), true);
    const c = p.doc.documentElement.children.find((e) => e.className === 'm-cite');
    assert.match(c.querySelector('.m-cite-ref').textContent, /^Kingma, D\. P\./);
  } finally { for (const k of Object.keys(saved)) { if (saved[k] === undefined) delete globalThis[k]; else globalThis[k] = saved[k]; } }
});

test('the References list of a chapter: the reference list\'s chapter\'s entries, shown there', () => {
  const p = citePage(part('<p>No references here.</p>', 155744));
  assert.deepEqual(JSON.parse(p.win.luntaReferences()), []);
  p.win.luntaSetShelf(BIB, 'lunta-part-25.xhtml');
  const list = JSON.parse(p.win.luntaReferences());
  assert.equal(list.length, 2);
  assert.match(list[1].text, /^Kingma/);
  assert.equal(list[1].arxiv, '1412.6980');
  assert.equal(p.win.luntaShowReference(1), true);
  assert.equal(p.location.href, 'lunta-part-25.xhtml#lunta-ref-1');
  // in the reference list's own chapter: the entry is in the page
  const b = citePage(BIB);
  assert.equal(JSON.parse(b.win.luntaReferences()).length, 2);
  assert.equal(b.win.luntaShowReference(1), true);
  assert.equal(b.doc.scrolledTo.id, 'r-kingma');
});

// ---- pages: past a chapter's last page ----

function pages(html, n = 3) {
  const doc = parse(html);
  doc.documentElement.scrollWidth = n * 390;
  const posted = [];
  const win = run(pagedJS, doc, {
    innerWidth: 390, innerHeight: 844, scrollX: 0, addEventListener() {}, getSelection: () => ({ isCollapsed: true, rangeCount: 0 }),
    scrollTo(o) { win.scrollX = o.left; },
    webkit: { messageHandlers: { page: { postMessage: (x) => posted.push(x) } } },
  });
  win.luntaSetPaged(true, 90, 56, false);
  return { win, posted };
}

test('turning past the last page of a chapter asks for the next chapter; before the first, the previous', () => {
  const p = pages(part('<p>A chapter of three pages.</p>'));
  p.win.luntaTurn(1);
  p.win.luntaTurn(1);
  assert.equal(p.win.scrollX, 2 * 390);
  assert.ok(!p.posted.includes('partNext'));
  p.win.luntaTurn(1);
  assert.ok(p.posted.includes('partNext'));
  assert.equal(p.win.scrollX, 2 * 390, 'the page stays');
  p.win.luntaGoToPage(0);
  p.win.luntaTurn(-1);
  assert.ok(p.posted.includes('partPrev'));
});

test('a paper\'s last page stays the last page', () => {
  const p = pages('<p>A paper.</p>');
  for (let i = 0; i < 4; i++) p.win.luntaTurn(1);
  assert.ok(!p.posted.includes('partNext'));
  assert.equal(p.win.scrollX, 2 * 390);
});
