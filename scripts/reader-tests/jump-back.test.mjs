// Back after a jump (apple/Shared/JumpBack.swift), in a book read chapter by chapter: the app
// (ChapterReader.swift) keeps the places left by jumps into other chapters, asks the page for the
// place to keep (luntaBackPlace, in the whole book's text) and shows Back in the chapter opened
// (luntaBackAcross); Back and × go back to the app ("partBack", "partBackClear").
// Run: node --test scripts/reader-tests/

import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parse, run } from './mini-dom.mjs';

const repo = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
const src = readFileSync(join(repo, 'apple/Shared/JumpBack.swift'), 'utf8').replace(/\r\n/g, '\n');
const m = src.match(/static let jumpBackJS = #"""\n([\s\S]*?)\n([ \t]*)"""#/);
assert.ok(m, 'jumpBackJS literal');
const script = m[1].split('\n').map((l) => l.slice(m[2].length)).join('\n');

const wait = (ms) => new Promise((r) => setTimeout(r, ms));

function page({ base = null, place = 0, offset = 0 } = {}) {
  const doc = parse(`<html><head>${base != null ? `<meta name="lunta-text-base" content="${base}"/>` : ''}</head>` +
    '<body><p>Some text of the chapter.</p></body></html>');
  const posted = [], scrolled = [];
  const listeners = {};
  const win = run(script, doc, {
    scrollX: 0, scrollY: 0, innerWidth: 400, innerHeight: 800,
    addEventListener(t, fn) { (listeners[t] ??= []).push(fn); },
    scrollTo(o) { scrolled.push(o); win.scrollY = o.top; },
    luntaPlace: () => place + (base ?? 0),
    luntaReadingOffset: () => offset,
    luntaScrollToOffset: (o) => { scrolled.push({ off: o }); return true; },
    webkit: { messageHandlers: { page: { postMessage: (x) => posted.push(x) } } },
  });
  const ui = () => doc.documentElement.childNodes.find((n) => n.id === 'm-back');
  const shown = () => !!ui() && ui().classList.contains('on');
  const tap = (cls) => ui().querySelector('.' + cls).dispatch({ type: 'click' });
  const scroll = (y) => { win.scrollY = y; (listeners.scroll ?? []).forEach((f) => f()); };
  const key = (ev) => doc.dispatch({ type: 'keydown', ...ev });
  return { doc, win, posted, scrolled, shown, tap, scroll, key };
}

test('the place to keep: the place shown, in the whole book’s text', () => {
  const p = page({ base: 5000, place: 120 });
  assert.equal(p.win.luntaBackPlace(), 5120);
  assert.equal(page({ place: 7 }).win.luntaBackPlace(), 7);   // a paper: no base
});

test('while Back shows for a jump within the chapter, its place is kept (the first place wins)', async () => {
  const p = page({ base: 5000, place: 900, offset: 40 });
  p.win.luntaMarkJump();          // at offset 40, near the top
  p.scroll(8000);                 // the jump goes far
  await wait(300);
  assert.ok(p.shown());
  assert.equal(p.win.luntaBackPlace(), 5040);
});

test('arriving from another chapter: Back shows, stays while reading, and goes back through the app', async () => {
  const p = page({ base: 5000 });
  assert.ok(!p.shown());
  p.win.luntaBackAcross(2);
  assert.ok(p.shown());
  p.scroll(5000); p.scroll(0);    // reading here (and at the arrival place) doesn't end it
  await wait(300);
  assert.ok(p.shown());
  p.win.luntaMarkJump();          // a jump within this chapter keeps the other chapter's place
  p.scroll(20000);
  await wait(300);
  assert.ok(p.shown());
  p.tap('m-back-go');
  assert.deepEqual(p.posted, ['partBack']);
  assert.ok(!p.shown());
  assert.deepEqual(p.scrolled.filter((s) => s.off != null), []);   // the page doesn't move by itself
});

test('× forgets the places in other chapters; luntaBackAcross(0) hides Back', () => {
  const p = page();
  p.win.luntaBackAcross(1);
  p.tap('m-back-x');
  assert.deepEqual(p.posted, ['partBackClear']);
  assert.ok(!p.shown());
  p.win.luntaBackAcross(3);
  p.win.luntaBackAcross(0);
  assert.ok(!p.shown());
  assert.deepEqual(p.posted, ['partBackClear']);
});

test('⌘[ goes back to the other chapter too; a jump within the page still scrolls back itself', async () => {
  const p = page();
  p.win.luntaBackAcross(1);
  p.key({ key: '[', metaKey: true });
  assert.deepEqual(p.posted, ['partBack']);
  assert.equal(p.win.luntaJumpBack(), false);   // nothing more here

  const q = page({ offset: 33 });
  q.win.luntaMarkJump();
  q.scroll(9000);
  await wait(300);
  assert.equal(q.win.luntaJumpBack(), true);
  assert.deepEqual(q.posted, []);
  assert.deepEqual(q.scrolled.at(-1), { off: 33 });
});
