// Highlights in the reading view (apple/Shared/Highlights.swift, annotationJS): a passage that runs
// over paragraphs and a figure (as one that runs over a page's end does) is read from the selection,
// found again and drawn as one highlight; Extend… and Highlight to… end a passage at the word
// tapped, forwards, backwards or inside it; and highlight mode waits while a handle is dragged.
// Run: node --test scripts/reader-tests/highlights.test.mjs

import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parse, run } from './mini-dom.mjs';

const repo = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
const src = readFileSync(join(repo, 'apple/Shared/Highlights.swift'), 'utf8').replace(/\r\n/g, '\n');
const m = src.match(/static let annotationJS = #"""\n([\s\S]*?)\n([ \t]*)"""#/);
assert.ok(m, 'annotationJS literal');
// (Swift fills in the CSS; the tests don't need it)
const script = m[1].split('\n').map((l) => l.slice(m[2].length)).join('\n').split('CSS').join('');

// a page in two columns would put "bottom of the page" at one page's end and "continues here" at
// the next one's start; for the text (and the highlight) that is just the next paragraph
const PAGE = `<section>
  <p id="p1">Some text before. A sentence at the bottom of the page</p>
  <figure><img src="x"/><figcaption>Figure 1. A figure in between.</figcaption></figure>
  <p id="p2">continues here, on the next page, and ends.</p>
  <p id="p3">Another paragraph further on, with more words in it.</p>
</section>`;

function page(html = PAGE, extra = {}) {
  const doc = parse(html);
  const sent = [];
  let selection = null;
  const sel = {
    get rangeCount() { return selection ? 1 : 0; },
    get isCollapsed() { return !selection; },
    getRangeAt: () => selection,
    toString: () => (selection ? 'selected text' : ''),   // (only its length is looked at)
    removeAllRanges: () => { selection = null; },
  };
  const listeners = [];
  const win = run(script, doc, {
    webkit: { messageHandlers: { ann: { postMessage: (x) => sent.push(x) } } },
    getSelection: () => sel,
    addEventListener: (type, fn, capture) => listeners.push({ type, fn, capture }),
    ...extra,
  });
  const text = () => {
    let s = '';
    const walk = (n) => n.childNodes.forEach((c) => (c.nodeType === 3 ? (s += c.data) : walk(c)));
    walk(doc.body);
    return s;
  };
  const textNode = (id, i = 0) => doc.getElementById(id).childNodes.filter((n) => n.nodeType === 3)[i];
  return {
    doc, win, sent, text, textNode, listeners,
    select: (sc, so, ec, eo) => { selection = { startContainer: sc, startOffset: so, endContainer: ec, endOffset: eo }; },
    hasSelection: () => !!selection,
    marks: (id) => doc.querySelectorAll('mark.m-hl').filter((x) => !id || x.getAttribute('data-ann') === id),
    apply: (list) => JSON.parse(win.luntaApplyAnnotations(JSON.stringify(list))),
    at: (needle) => text().indexOf(needle),
  };
}

test('a selection over two paragraphs and a figure: one quote with the text before and after', () => {
  const p = page();
  const a = p.textNode('p1'), b = p.textNode('p2');
  p.select(a, a.data.indexOf('bottom'), b, b.data.indexOf(', on'));
  const s = JSON.parse(p.win.luntaSelection());
  assert.match(s.quote, /^bottom of the page\s+Figure 1\. A figure in between\.\s+continues here$/);
  assert.equal(s.start, p.at('bottom'));
  assert.equal(s.prefix, p.text().slice(Math.max(0, s.start - 40), s.start));
  assert.equal(s.suffix, ', on the next page, and ends.\n  Another ');
});

test('a selection that ends between elements (a paragraph\'s edge) is still read', () => {
  const p = page();
  const a = p.textNode('p1');
  const p2 = p.doc.getElementById('p2');
  // WebKit can end a selection at (element, child index) rather than in a text node
  p.select(a, a.data.indexOf('bottom'), p2, 1);
  const s = JSON.parse(p.win.luntaSelection());
  assert.equal(s.start, p.at('bottom'));
  assert.ok(s.quote.endsWith('and ends.'), s.quote);
  // and one that ends after the last child of a paragraph: the text after it
  p.select(a, a.data.indexOf('bottom'), p.doc.getElementById('p1'), 1);
  assert.equal(JSON.parse(p.win.luntaSelection()).quote, 'bottom of the page');
  // starting at an element (before the figure's caption)
  const fig = p.doc.querySelector('figure');
  p.select(fig, 0, p.textNode('p2'), 9);
  assert.match(JSON.parse(p.win.luntaSelection()).quote, /^\s*Figure 1\.[\s\S]*continues$/);
});

test('a highlight over paragraphs and a figure: drawn in each, found again, the text unchanged', () => {
  const p = page();
  const before = p.text();
  const s = p.at('bottom'), e = p.at(', on the next');
  const t = before;
  const h = { id: 'h1', quote: t.slice(s, e), prefix: t.slice(Math.max(0, s - 40), s), suffix: t.slice(e, e + 40), start: s, color: 'green' };
  assert.deepEqual(p.apply([h]), []);
  const marks = p.marks('h1');
  // the end of p1, the caption, the start of p2 (the whitespace between them isn't coloured)
  assert.deepEqual(marks.map((x) => x.textContent), ['bottom of the page', 'Figure 1. A figure in between.', 'continues here']);
  assert.ok(marks.every((x) => x.className.includes('m-green')));
  assert.equal(p.text(), before);
  // drawn again (a sync, a text-size change, pages ↔ scrolling): the same marks, the same text
  assert.deepEqual(p.apply([h]), []);
  assert.equal(p.marks('h1').length, 3);
  assert.equal(p.text(), before);
});

test('a highlight over paragraphs is found in a rebuilt reading view with other spacing between them', () => {
  const p = page();
  const t = p.text(), s = p.at('bottom'), e = p.at(', on the next');
  const h = { id: 'h1', quote: t.slice(s, e), prefix: t.slice(Math.max(0, s - 40), s), suffix: t.slice(e, e + 40), start: s };
  const rebuilt = page(`<div><p>Some text before. A sentence at the bottom of the page</p><figure><figcaption>Figure 1. A figure in between.</figcaption></figure>
<p>continues here, on the next page, and ends.</p></div>`);
  assert.deepEqual(rebuilt.apply([h]), []);
  assert.deepEqual(rebuilt.marks('h1').map((x) => x.textContent), ['bottom of the page', 'Figure 1. A figure in between.', 'continues here']);
});

test('Extend…: to a word on a later page; backwards; back inside the passage', () => {
  const p = page();
  const t = p.text(), s = p.at('bottom'), e = s + 'bottom of the page'.length;
  p.apply([{ id: 'h1', quote: t.slice(s, e), prefix: t.slice(Math.max(0, s - 40), s), suffix: t.slice(e, e + 40), start: s }]);
  assert.equal(p.win.luntaExtendStart('h1', 'yellow'), true);
  assert.equal(p.win.luntaExtending(), true);
  // tapping in the middle of "further" (two paragraphs on): the passage ends after that word
  let o = JSON.parse(p.win.luntaExtendToOffset(p.at('further') + 3));
  assert.equal(o.id, 'h1');
  assert.equal(o.start, s);
  assert.match(o.quote, /^bottom of the page[\s\S]*and ends\.\s+Another paragraph further$/);
  assert.equal(o.prefix, t.slice(Math.max(0, s - 40), s));
  assert.equal(o.suffix, t.slice(s + o.quote.length, s + o.quote.length + 40));
  // tapping just after a word ends there too
  o = JSON.parse(p.win.luntaExtendToOffset(p.at('here,') + 4));
  assert.ok(o.quote.endsWith('continues here'), o.quote);
  // before the start: the start moves to that word, the end stays
  o = JSON.parse(p.win.luntaExtendToOffset(p.at('before.') + 2));
  assert.equal(o.start, p.at('before.'));
  assert.equal(o.quote, 'before. A sentence at the bottom of the page');
  // inside: the end comes back to that word
  o = JSON.parse(p.win.luntaExtendToOffset(p.at('of the page')));
  assert.equal(o.quote, 'bottom of');
  p.win.luntaExtendCancel();
  assert.equal(p.win.luntaExtending(), false);
  assert.equal(p.win.luntaExtendToOffset(p.at('further')), '');
  // a highlight that isn't in the page: nothing to extend
  assert.equal(p.win.luntaExtendStart('nope'), false);
});

test('Highlight to…: the selection is coloured meanwhile, the tap ends it, the click goes no further', () => {
  const p = page();
  const a = p.textNode('p1');
  p.select(a, a.data.indexOf('bottom'), a, a.data.length);
  assert.equal(p.win.luntaExtendStart('', 'pink'), true);
  assert.equal(p.hasSelection(), false);
  assert.deepEqual(p.marks('__pending').map((x) => x.textContent), ['bottom of the page']);
  assert.ok(p.doc.documentElement.className.includes('m-extending'));
  // the tap: WebKit's caret at the tapped point is in "next" (p2)
  const b = p.textNode('p2');
  p.doc.caretRangeFromPoint = () => ({ startContainer: b, startOffset: b.data.indexOf('next') + 2 });
  const click = p.listeners.find((l) => l.type === 'click' && l.capture);
  assert.ok(click, 'a capturing click listener on the window');
  const ev = { type: 'click', clientX: 10, clientY: 20, preventDefault() { this.prevented = true; }, stopImmediatePropagation() { this.stopped = true; } };
  click.fn(ev);
  assert.ok(ev.prevented && ev.stopped);
  const msg = p.sent.find((x) => x.startsWith('__extend__:'));
  assert.ok(msg, 'the passage is sent to the app');
  const o = JSON.parse(msg.slice(11));
  assert.equal(o.id, '');
  assert.equal(o.start, p.at('bottom'));
  assert.match(o.quote, /^bottom of the page[\s\S]*continues here, on the next$/);
  assert.equal(p.win.luntaExtending(), false);
  // the next click is an ordinary one again
  const ev2 = { type: 'click', clientX: 10, clientY: 20, preventDefault() { this.prevented = true; }, stopImmediatePropagation() {} };
  click.fn(ev2);
  assert.ok(!ev2.prevented);
});

test('a tapped highlight opens its editor, but not while extending', () => {
  const p = page();
  const t = p.text(), s = p.at('bottom'), e = s + 6;
  p.apply([{ id: 'h1', quote: t.slice(s, e), prefix: t.slice(Math.max(0, s - 40), s), suffix: t.slice(e, e + 40), start: s }]);
  const tap = (ev) => p.doc.dispatch({ type: 'click', target: p.marks('h1')[0], ...ev });
  tap({});
  assert.deepEqual(p.sent.filter((x) => x === 'h1'), ['h1']);
  p.win.luntaExtendStart('h1');
  tap({});
  assert.deepEqual(p.sent.filter((x) => x === 'h1'), ['h1']);
});

test('merging: an extended highlight takes the new passage as it is; others it reaches join it', () => {
  const p = page();
  const t = p.text();
  const hl = (id, from, len) => { const s = p.at(from); return { id, quote: t.slice(s, s + len), prefix: t.slice(Math.max(0, s - 40), s), suffix: t.slice(s + len, s + len + 40), start: s }; };
  p.apply([hl('h1', 'bottom', 18), hl('h2', 'continues', 9)]);
  // the passage from "bottom" to "next": it covers h2
  const s = p.at('bottom'), e = p.at('next') + 4;
  let o = JSON.parse(p.win.luntaMergeRange(s, e, 'h1'));
  assert.deepEqual(o.ids, ['h2']);
  assert.equal(o.start, s);
  assert.equal(o.start + o.quote.length, e);
  // shorter than h1 was: h1 left out, so the passage stays short
  o = JSON.parse(p.win.luntaMergeRange(s, s + 6, 'h1'));
  assert.deepEqual(o.ids, []);   // (no other highlight: the app keeps the passage as tapped)
  assert.equal(o.quote, 'bottom');
  // without exclusion h1 is one of them, and the passage covers all of it
  o = JSON.parse(p.win.luntaMergeRange(s, s + 6));
  assert.deepEqual(o.ids, ['h1']);
  assert.equal(o.quote, 'bottom of the page');
});

test('highlight mode (iPhone) waits while a selection handle is still dragged', async () => {
  const p = page(PAGE, { ontouchstart: null });
  p.win.luntaHLMode = true;
  const a = p.textNode('p3');
  p.select(a, 0, a, 17);
  p.win.luntaDragging = true;
  p.doc.dispatch({ type: 'selectionchange' });
  await new Promise((r) => setTimeout(r, 700));
  assert.equal(p.sent.filter((x) => x.startsWith('__hl__:')).length, 0);
  p.win.luntaDragging = false;
  await new Promise((r) => setTimeout(r, 400));
  const hl = p.sent.filter((x) => x.startsWith('__hl__:'));
  assert.equal(hl.length, 1);
  assert.equal(JSON.parse(hl[0].slice(7)).quote, 'Another paragraph');
});
