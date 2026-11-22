// Regions marked on figures (apple/Shared/FigureMarks.swift, figureJS in FigureViewer.swift; desktop
// figures.ts). The page tells the app where each figure is in its text (`at`, a highlight's offset,
// in a book read chapter by chapter the whole book's), so a mark is listed among the highlights at its
// figure; the figure's card shows a badge for the marks on it, drawn from an attribute so the page's
// text (and every highlight's offset) stays as it was. The desktop viewer maps a drag on the image to
// the image's own 0–1 coordinates (figureRegion, figure-marks.ts), at any zoom.
// Run: node --test scripts/reader-tests/

import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parse, Element } from './mini-dom.mjs';

const repo = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
function literal(file, name) {
  const src = readFileSync(join(repo, file), 'utf8').replace(/\r\n/g, '\n');
  const m = src.match(new RegExp(`static let ${name} = #"""\\n([\\s\\S]*?)\\n([ \\t]*)"""#`));
  assert.ok(m, `${name} literal`);
  return m[1].split('\n').map((l) => l.slice(m[2].length)).join('\n');
}
const figureJS = literal('apple/Shared/FigureViewer.swift', 'figureJS');
globalThis.Element ??= Element;

function page(html) {
  const doc = parse(html);
  const sent = [];
  const win = {
    document: doc,
    webkit: { messageHandlers: { fig: { postMessage: (x) => sent.push(x) } } },
    getSelection: () => ({ isCollapsed: true, rangeCount: 0 }),
    addEventListener: () => {},
  };
  new Function('window', 'document', figureJS)(win, doc);
  const text = () => {
    let s = '';
    const walk = (n) => n.childNodes.forEach((c) => (c.nodeType === 3 ? (s += c.data) : walk(c)));
    walk(doc.body);
    return s;
  };
  return { doc, win, text, list: () => sent.find((x) => x.list)?.list ?? [], fig: (i) => JSON.parse(win.luntaFigures.data(i)).all[i] };
}

const PAPER = `<html><head></head><body><section>
  <p>Iron accumulates in microglia (Figure 2).</p>
  <figure id="f1"><img src="file:///book/EPUB/images/fig1.png"/><figcaption>Figure 1. Overview.</figcaption></figure>
  <p>Between the figures.</p>
  <figure id="f2"><img src="file:///book/EPUB/images/fig2a.png"/><img src="file:///book/EPUB/images/fig2b.png"/><figcaption>Fig. 2 | UMAP of microglia.</figcaption></figure>
  <figure id="f3"><img src="file:///book/EPUB/images/ga.png"/></figure>
  <p>After.</p>
</section></body></html>`;

test('each figure says where it is in the page text, for ordering its marks among the highlights', () => {
  const p = page(PAPER);
  const t = p.text();
  assert.equal(p.fig(0).at, t.indexOf('Figure 1.'));
  assert.equal(p.fig(1).at, t.indexOf('Fig. 2 |'));
  // a figure without any text: where the text after it starts
  assert.ok(p.fig(2).at > p.fig(1).at);
  assert.equal(t.slice(p.fig(2).at, t.indexOf('After.')).trim(), '');
  // the list the page sends as it loads has them too
  assert.deepEqual(p.list().map((f) => f.at), [0, 1, 2].map((i) => p.fig(i).at));
  assert.equal(p.fig(1).srcs.length, 2);   // (the images' URLs: img.src, which this small DOM doesn't resolve)
});

test('in a book read chapter by chapter, in the whole book\'s text', () => {
  const p = page(`<html><head><meta name="lunta-text-base" content="5000"/></head><body>
    <p>Regression is shown in Figure 3.1.</p>
    <figure><img src="c.png"/><figcaption>Figure 3.1 Regression.</figcaption></figure></body></html>`);
  assert.equal(p.fig(0).at, 5000 + p.text().indexOf('Figure 3.1 Regression'));
});

test('a badge on a figure\'s card for the regions marked on it, adding no text to the page', () => {
  const p = page(PAPER);
  const before = p.text();
  const badges = () => p.doc.querySelectorAll('.m-fc-marks').map((b) => [b.getAttribute('data-fig-marks'), b.getAttribute('data-t'), b.className]);
  assert.deepEqual(badges(), [['0', null, 'm-fc-marks'], ['1', null, 'm-fc-marks'], ['2', null, 'm-fc-marks']]);
  p.win.luntaFigures.setRegions(JSON.stringify({ 1: ['green', 'pink'], 2: ['nonsense'] }));
  assert.deepEqual(badges(), [['0', null, 'm-fc-marks'], ['1', '2 marks', 'm-fc-marks m-green'], ['2', '1 mark', 'm-fc-marks m-yellow']]);
  assert.equal(p.text(), before);
  // all marks gone (deleted, undone): no badge
  p.win.luntaFigures.setRegions('{}');
  assert.deepEqual(badges().map((b) => b[1]), [null, null, null]);
  p.win.luntaFigures.setRegions('not json');
  assert.deepEqual(badges().map((b) => b[1]), [null, null, null]);
});

// the desktop viewer's mapping (desktop/src/figure-marks.ts: plain TypeScript, which node runs as it is)
const lib = await import(join(repo, 'desktop/src/figure-marks.ts'));

test('a drag on the image as 0–1 of the image, at any zoom and scroll', () => {
  // the image drawn at 400×200 with its top left at (50, 30) on screen (zoomed, scrolled: its box)
  const box = { left: 50, top: 30, width: 400, height: 200 };
  assert.deepEqual(lib.figureRegion({ x: 150, y: 80 }, { x: 250, y: 130 }, box), { x: 0.25, y: 0.25, w: 0.25, h: 0.25 });
  // dragged up and to the left, and beyond the image: kept inside it
  assert.deepEqual(lib.figureRegion({ x: 500, y: 300 }, { x: 250, y: 130 }, box), { x: 0.5, y: 0.5, w: 0.5, h: 0.5 });
  // zoomed in twice: the same place on the image is the same region
  const big = { left: -100, top: -60, width: 800, height: 400 };
  assert.deepEqual(lib.figureRegion({ x: 100, y: 40 }, { x: 300, y: 140 }, big), { x: 0.25, y: 0.25, w: 0.25, h: 0.25 });
  // and back to where it is on screen
  assert.deepEqual(lib.regionBox({ x: 0.25, y: 0.25, w: 0.25, h: 0.25 }, big), { left: 100, top: 40, width: 200, height: 100 });
  // the smallest mark under a point (one inside another), none outside
  const marks = [{ id: 'big', x: 0, y: 0, w: 1, h: 1 }, { id: 'small', x: 0.2, y: 0.2, w: 0.1, h: 0.1 }];
  assert.equal(lib.markAt({ x: 50 + 0.25 * 400, y: 30 + 0.25 * 200 }, box, marks), 'small');
  assert.equal(lib.markAt({ x: 50 + 0.8 * 400, y: 30 + 0.8 * 200 }, box, marks), 'big');
  assert.equal(lib.markAt({ x: 10, y: 10 }, box, marks), null);
});

test('the content hash is the core\'s (FNV-1a, 64 bits), and marks are placed as the core places them', async () => {
  assert.equal(lib.hashBytes(new TextEncoder().encode('three')), '5a73f1720ca645a3');   // core figmark::hash_bytes(b"three")
  const all = new Uint8Array(768); for (let i = 0; i < all.length; i++) all[i] = i % 256;
  assert.equal(lib.hashBytes(all), '4f33c513c2e95c25');
  assert.equal(lib.labelKey('Fig. 3'), lib.labelKey('FIGURE 3.'));
  assert.equal(lib.fileName('file:///b/My%20Book/fig%203.jpg?x=1'), 'fig 3.jpg');
  const mark = (id, r) => ({ id, location: 'figure', position: JSON.stringify({ figure: 'Figure 3', index: 2, image: 'fig3.png', hash: 'h3', image_index: 0, x: 0.1, y: 0.1, w: 0.2, h: 0.2, shape: 'rect', start: 0, ...r }) });
  const hashes = { 'file:///x/renamed.png': 'h3' };
  const hash = async (src) => hashes[src] ?? 'other';
  const place = async (marks, figs) => Object.fromEntries(await lib.placeMarks(marks, figs.map(([label, srcs]) => ({ label, srcs })), hash));
  const m = mark('m');
  assert.deepEqual(await place([m], [['Figure 1', ['a.png']], ['Figure 3', ['file:///x/fig3.png']]]), { m: { figure: 1, image: 0 } });
  // remade: renamed and numbered anew, the same image
  assert.deepEqual(await place([m], [['Fig. 4', ['b.png']], ['Fig. 9', ['file:///x/renamed.png']]]), { m: { figure: 1, image: 0 } });
  // the image made anew: by the label
  assert.deepEqual(await place([m], [['FIGURE 3', ['new1.png', 'new2.png']]]), { m: { figure: 0, image: 0 } });
  // gone; a highlight; an unlabelled figure by its image's name
  assert.deepEqual(await place([m], [['Figure 1', ['a.png']]]), {});
  assert.deepEqual(await place([{ id: 'h', location: 'epub', position: '12' }], [['Figure 3', ['fig3.png']]]), {});
  assert.deepEqual(await place([mark('g', { figure: '', image: 'ga.png', hash: '' })], [['Figure', ['x.png']], ['Figure', ['ga.png']]]), { g: { figure: 1, image: 0 } });
  // the CSS crop: the region (with a little around it) fills the box
  assert.match(lib.cropStyle('a.png', { x: 0, y: 0, w: 1, h: 1 }), /background-size: 100% 100%;.*background-position: 0% 0%/);
});
