// Pages on the iPhone and iPad (apple/Shared/PageTurn.swift, pagedJS): a selection handle dragged
// over the page's edge turns the page and the selection goes on onto the next one; where its moving
// end has to be for that (luntaEdgeDirection), one turn per rest, the selection kept or made again.
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
globalThis.Element = Element;   // (the script turns scrollIntoView into a page turn)

// an iPhone: 390 × 844, pages start 90 px down and end 56 px above the bottom, 20 px margins
const PHONE = { W: 390, H: 844, cols: 1, pad: 20, top0: 90, bottom0: 56, scrollX: 0 };
const dir = (win, g) => win.luntaEdgeDirection({ ...PHONE, height: 22, ...g });

function book(pages = 3) {
  const doc = parse('<p id="a">The first page ends with this line</p><p id="b">and the next one goes on here.</p>');
  doc.documentElement.scrollWidth = pages * PHONE.W;
  const posted = [];
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
    addEventListener() {},
    webkit: { messageHandlers: { page: { postMessage: (x) => posted.push(x) } } },
    getSelection: () => sel,
  });
  win.luntaSetPaged(true, PHONE.top0, PHONE.bottom0, true);
  const t = (id) => doc.getElementById(id).childNodes[0];
  return {
    doc, win, posted, sel,
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

test('a rest at the edge turns one page, animated, keeping the selection; then not again at once', () => {
  const b = book();
  const a = b.t('a');
  b.select(a, 4, a, 10, { left: 20, right: 200, top: 766, bottom: 788, width: 180, height: 22 });
  // the finger rests at the right edge of the screen (zone 1), though the end is mid-line
  assert.equal(b.win.luntaSelectionEdgeTurn(1), 1);
  assert.deepEqual(b.turns(), [{ page: 1, smooth: true, keep: true }]);
  // the next rest right after: no second turn (one per rest, and the turn is still settling)
  assert.equal(b.win.luntaSelectionEdgeTurn(1), 0);
  assert.equal(b.turns().length, 1);
});

test('no turn without a selection, in the middle of the page, or past the last page', () => {
  const b = book(1);
  assert.equal(b.win.luntaSelectionEdgeTurn(1), 0);
  const a = b.t('a');
  b.select(a, 0, a, 5, { left: 20, right: 366, top: 766, bottom: 788, width: 346, height: 22 });
  assert.equal(b.win.luntaSelectionEdgeTurn(1), 0);   // a one-page paper
  const c = book();
  c.select(c.t('a'), 0, c.t('a'), 5, { left: 20, right: 120, top: 400, bottom: 422, width: 100, height: 22 });
  assert.equal(c.win.luntaSelectionEdgeTurn(0), 0);
  assert.equal(c.win.luntaSelectionEdgeTurn(-1), 0);   // back from the first page
});

test('the end on the last line turns without the finger at the edge; a dropped selection is made again', async () => {
  const b = book();
  const a = b.t('a'), n = b.t('b');
  b.select(a, 0, a, 10, { left: 20, right: 366, top: 766, bottom: 788, width: 346, height: 22 });
  assert.equal(b.win.luntaSelectionEdgeTurn(0), 1);
  // WebKit dropped it over the turn: made again from the start that stayed to the new page's first word
  b.drop();
  b.doc.caretRangeFromPoint = () => ({ startContainer: n, startOffset: 1 });
  await new Promise((r) => setTimeout(r, 600));
  assert.deepEqual(b.sel.made, [a, 0, n, 3]);
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
