// Pages on the iPhone and iPad (apple/Shared/PageTurn.swift, pagedJS): a selection handle dragged
// over the page's edge turns the page; where its moving end has to be for that (luntaEdgeDirection),
// one turn per rest, and the passage so far becomes a pending Highlight to… (Highlights.swift), so
// the word tapped on the new page ends one highlight from the selection's start.
// Run: node --test scripts/reader-tests/paged-edge.test.mjs

import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parse, run, Element } from './mini-dom.mjs';

const repo = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
const src = readFileSync(join(repo, 'apple/Shared/PageTurn.swift'), 'utf8').replace(/\r\n/g, '\n');
const m = src.match(/static let pagedJS = #"""\n([\s\S]*?)\n([ \t]*)"""#/);
assert.ok(m, 'pagedJS literal');
const script = m[1].split('\n').map((l) => l.slice(m[2].length)).join('\n');
const hsrc = readFileSync(join(repo, 'apple/Shared/Highlights.swift'), 'utf8').replace(/\r\n/g, '\n');
const hm = hsrc.match(/static let annotationJS = #"""\n([\s\S]*?)\n([ \t]*)"""#/);
assert.ok(hm, 'annotationJS literal');
const annotationJS = hm[1].split('\n').map((l) => l.slice(hm[2].length)).join('\n').split('CSS').join('');
globalThis.Element = Element;   // (the script turns scrollIntoView into a page turn)

// an iPhone: 390 × 844, pages start 90 px down and end 56 px above the bottom, 20 px margins
const PHONE = { W: 390, H: 844, cols: 1, pad: 20, top0: 90, bottom0: 56, scrollX: 0 };
const dir = (win, g) => win.luntaEdgeDirection({ ...PHONE, height: 22, ...g });

// (`annotations`: the highlight script in the same page, as in the app)
function book(pages = 3, annotations = true) {
  const doc = parse('<p id="a">The first page ends with this line</p><p id="b">and the next one goes on here.</p>');
  doc.documentElement.scrollWidth = pages * PHONE.W;
  const posted = [], ann = [], clicks = [];
  let selection = null;
  const sel = {
    get rangeCount() { return selection ? 1 : 0; },
    get isCollapsed() { return !selection; },
    getRangeAt: () => selection,
    removeAllRanges: () => { selection = null; },
    setBaseAndExtent: (a, ao, f, fo) => { sel.made = [a, ao, f, fo]; },
  };
  const win = run(script, doc, {
    innerWidth: PHONE.W, innerHeight: PHONE.H, scrollX: 0,
    addEventListener: (type, fn, capture) => { if (type === 'click' && capture) clicks.push(fn); },
    webkit: { messageHandlers: { page: { postMessage: (x) => posted.push(x) }, ann: { postMessage: (x) => ann.push(x) } } },
    getSelection: () => sel,
  });
  if (annotations) new Function('window', 'document', annotationJS)(win, doc);
  win.luntaSetPaged(true, PHONE.top0, PHONE.bottom0, true);
  const t = (id) => doc.getElementById(id).childNodes[0];
  return {
    doc, win, posted, sel, ann, clicks,
    hasSelection: () => !!selection,
    // a selection whose last line box is `box` (viewport coordinates)
    select(sc, so, ec, eo, box) {
      selection = { startContainer: sc, startOffset: so, endContainer: ec, endOffset: eo, getClientRects: () => [box] };
      doc.dispatch({ type: 'selectionchange' });
    },
    drop() { selection = null; },
    t,
    turns: () => posted.filter((x) => x && typeof x === 'object' && x.keep),
  };
}

test('where the moving end has to be for a turn', () => {
  const { win } = book();
  // the end at the end of the page's last line: on
  assert.equal(dir(win, { end: 1, left: 20, right: 366, top: 766, bottom: 788 }), 1);
  // on the last line, but a word in the middle of it (being picked): stay
  assert.equal(dir(win, { end: 1, left: 20, right: 200, top: 766, bottom: 788 }), 0);
  // at the end of a line higher up: stay
  assert.equal(dir(win, { end: 1, left: 20, right: 366, top: 500, bottom: 522 }), 0);
  // already on the next page (WebKit took it there): on
  assert.equal(dir(win, { end: 1, left: 410, right: 600, top: 90, bottom: 112 }), 1);
  // the start, dragged to the beginning of the page's first line: back
  assert.equal(dir(win, { end: -1, left: 20, right: 300, top: 92, bottom: 114 }), -1);
  assert.equal(dir(win, { end: -1, left: 200, right: 300, top: 92, bottom: 114 }), 0);
  assert.equal(dir(win, { end: -1, left: 20, right: 300, top: 300, bottom: 322 }), 0);
  // on page 2 (scrolled by one page), the same places
  assert.equal(dir(win, { end: 1, scrollX: 390, left: 20, right: 366, top: 766, bottom: 788 }), 1);
  assert.equal(dir(win, { end: -1, scrollX: 390, left: -370, right: -40, top: 500, bottom: 522 }), -1);
});

test('two columns on a page (iPad): only the right column\'s last line turns', () => {
  const { win } = book();
  const IPAD = { W: 1180, H: 820, cols: 2, pad: 44, top0: 80, bottom0: 40, scrollX: 0, height: 24 };
  const g = (x) => win.luntaEdgeDirection({ ...IPAD, ...x });
  assert.equal(g({ end: 1, left: 44, right: 546, top: 750, bottom: 774 }), 0);    // the left column's end
  assert.equal(g({ end: 1, left: 634, right: 1136, top: 750, bottom: 774 }), 1);  // the right one's
  assert.equal(g({ end: -1, left: 634, right: 900, top: 82, bottom: 106 }), 0);   // the right column's top
  assert.equal(g({ end: -1, left: 44, right: 400, top: 82, bottom: 106 }), -1);   // the left one's
});

test('a rest at the edge turns one page, animated; the passage becomes a pending Highlight to…; not again at once', () => {
  const b = book();
  const a = b.t('a');
  b.select(a, 4, a, 10, { left: 20, right: 200, top: 766, bottom: 788, width: 180, height: 22 });
  // the finger rests at the right edge of the screen (zone 1), though the end is mid-line
  assert.equal(b.win.luntaSelectionEdgeTurn(1), 1);
  assert.deepEqual(b.turns(), [{ page: 1, smooth: true, keep: true }]);
  // the selection (and its handles) went; its passage is coloured and waits for the end word
  assert.equal(b.hasSelection(), false);
  assert.deepEqual(b.doc.querySelectorAll('mark.m-hl').map((x) => [x.getAttribute('data-ann'), x.textContent]), [['__pending', 'first ']]);
  assert.deepEqual(b.ann.filter((x) => x !== '__ready__'), ['__extendstart__:yellow']);
  assert.equal(b.win.luntaExtending(), true);
  // the next rest right after: no second turn
  assert.equal(b.win.luntaSelectionEdgeTurn(1), 0);
  assert.equal(b.turns().length, 1);
});

test('after the turn the word tapped on the new page ends one highlight from the start on the page before', () => {
  const b = book();
  const a = b.t('a'), n = b.t('b');
  b.win.luntaSetDragging(true);
  b.select(a, 4, a, a.data.length, { left: 20, right: 366, top: 766, bottom: 788, width: 346, height: 22 });
  assert.equal(b.win.luntaSelectionEdgeTurn(0), 1);
  // the finger lifts (no tap on a word: a click right after the drag is ignored)
  b.win.luntaSetDragging(false);
  b.doc.caretRangeFromPoint = () => ({ startContainer: n, startOffset: n.data.indexOf('goes') + 1 });
  const click = () => { const ev = { clientX: 100, clientY: 300, preventDefault() {}, stopImmediatePropagation() {} }; b.clicks.forEach((f) => f(ev)); };
  click();
  assert.equal(b.ann.filter((x) => x.startsWith('__extend__:')).length, 0);
  assert.equal(b.win.luntaExtending(), true);
  // the tap on "goes", a moment later
  return new Promise((r) => setTimeout(r, 450)).then(() => {
    click();
    const msg = b.ann.find((x) => x.startsWith('__extend__:'));
    assert.ok(msg);
    const o = JSON.parse(msg.slice(11));
    assert.equal(o.quote, 'first page ends with this lineand the next one goes');
    assert.equal(o.start, 4);
  });
});

test('in highlight mode the pending passage has the highlight-mode colour', () => {
  const b = book();
  b.win.luntaHLMode = true; b.win.luntaHLColor = 'green';
  b.select(b.t('a'), 0, b.t('a'), 9, { left: 20, right: 366, top: 766, bottom: 788, width: 346, height: 22 });
  assert.equal(b.win.luntaSelectionEdgeTurn(0), 1);
  assert.deepEqual(b.ann.filter((x) => x !== '__ready__'), ['__extendstart__:green']);
  assert.ok(b.doc.querySelector('mark.m-hl').className.includes('m-green'));
});

test('without the highlight script there is no turn (the selection would only be lost)', () => {
  const b = book(3, false);
  b.select(b.t('a'), 0, b.t('a'), 9, { left: 20, right: 366, top: 766, bottom: 788, width: 346, height: 22 });
  assert.equal(b.win.luntaSelectionEdgeTurn(1), 0);
  assert.equal(b.turns().length, 0);
  assert.equal(b.hasSelection(), true);
});

test('dragging the start handle backwards: the end stays, the page turns back', () => {
  const b = book();
  const a = b.t('a'), n = b.t('b');
  b.win.luntaGoToPage(1, false);
  b.win.scrollX = 390;
  b.select(a, 30, n, 10, { left: 20, right: 300, top: 92, bottom: 114, width: 280, height: 22 });
  // the start moves (the end stays): it is the moving end now
  b.select(a, 20, n, 10, { left: 20, right: 300, top: 92, bottom: 114, width: 280, height: 22 });
  assert.equal(b.win.luntaSelectionEdgeTurn(0), -1);
  assert.deepEqual(b.turns().at(-1), { page: 0, smooth: true, keep: true });
});
