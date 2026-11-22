// Highlights in the figure viewer's caption (apple/Shared/FigureViewer.swift, figureJS, with the
// highlight script annotationJS in apple/Shared/Highlights.swift). The viewer shows a figure's title
// and caption as text of its own; a passage selected there is turned into a passage of the reading
// view's text (luntaFigures.select), so it is the same highlight as one made by selecting those words
// in the reading view: same quote, prefix, suffix and offset (in a book read chapter by chapter, with
// the chapter's base). The highlights drawn in the page show in the viewer as runs of its text
// (luntaFigures.marks).
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
const annotationJS = literal('apple/Shared/Highlights.swift', 'annotationJS').split('CSS').join('');
globalThis.Element ??= Element;

// both scripts in one page (one window), with a selection the test sets
function page(html) {
  const doc = parse(html);
  const sent = [];
  let selection = null;
  const sel = {
    get rangeCount() { return selection ? 1 : 0; },
    get isCollapsed() { return !selection; },
    getRangeAt: () => selection,
    toString: () => (selection ? 'selected text' : ''),
    removeAllRanges: () => { selection = null; },
  };
  const win = {
    document: doc,
    webkit: { messageHandlers: { ann: { postMessage: () => {} }, fig: { postMessage: (x) => sent.push(x) } } },
    getSelection: () => sel,
    addEventListener: () => {},
  };
  new Function('window', 'document', annotationJS)(win, doc);
  new Function('window', 'document', figureJS)(win, doc);
  const text = () => {
    let s = '';
    const walk = (n) => n.childNodes.forEach((c) => (c.nodeType === 3 ? (s += c.data) : walk(c)));
    walk(doc.body);
    return s;
  };
  const list = () => sent.find((x) => x.list)?.list ?? [];
  const fig = (i) => JSON.parse(win.luntaFigures.data(i)).all[i];
  return {
    doc, win, text, list, fig,
    shown: (i) => { const f = fig(i); return f.title ? (f.caption ? `${f.title} ${f.caption}` : f.title) : f.caption; },
    select: (i, from, to) => { const j = win.luntaFigures.select(i, from, to); return j ? JSON.parse(j) : null; },
    marks: (i) => JSON.parse(win.luntaFigures.marks(i)),
    // a selection in the reading view itself, from a text node and offset to another
    pageSelect: (sc, so, ec, eo) => { selection = { startContainer: sc, startOffset: so, endContainer: ec, endOffset: eo }; return JSON.parse(win.luntaSelection()); },
    apply: (list) => JSON.parse(win.luntaApplyAnnotations(JSON.stringify(list))),
  };
}

const PAPER = `<section>
  <p id="p1">Iron accumulates in microglia (Figure 2).</p>
  <figure id="f1"><img src="a.png"/><figcaption id="c1"><strong>Figure 1.</strong> <strong>Overview of the study.</strong>
    Cells were   sorted by <em>flow cytometry</em> before sequencing<sup><a href="#r14">14</a></sup> and imaging.</figcaption></figure>
  <figure id="f2"><img src="b.png"/><figcaption id="c2">Fig. 2 | Iron-laden microglia in MS lesions. Scale bar, 50 µm.</figcaption></figure>
  <p id="p2">More text after the figures.</p>
</section>`;

test('the viewer shows the caption as before', () => {
  const p = page(PAPER);
  const f = p.fig(0);
  assert.equal(f.label, 'Figure 1');
  assert.equal(f.title, 'Overview of the study.');
  assert.equal(f.caption, 'Cells were sorted by flow cytometry before sequencing and imaging.');
  assert.equal(p.fig(1).caption, 'Iron-laden microglia in MS lesions. Scale bar, 50 µm.');
});

test('a selection in the caption is the reading view\'s passage: same quote, context and offset', () => {
  const p = page(PAPER);
  const shown = p.shown(0);
  const from = shown.indexOf('sorted'), to = shown.indexOf(' before');
  const s = p.select(0, from, to);
  // the page's own text: its spacing kept
  assert.equal(s.quote, 'sorted by flow cytometry');
  const t = p.text();
  assert.equal(s.start, t.indexOf('sorted by'));
  assert.equal(s.prefix, t.slice(s.start - 40, s.start));
  assert.equal(s.suffix, t.slice(s.start + s.quote.length, s.start + s.quote.length + 40));
  // exactly what selecting the same words in the reading view gives
  const c1 = p.doc.getElementById('c1');
  const first = c1.childNodes[3];                   // "\n    Cells were   sorted by "
  const em = c1.querySelector('em').firstChild;     // "flow cytometry"
  assert.deepEqual(p.pageSelect(first, first.data.indexOf('sorted'), em, em.data.length), s);
});

test('a passage over the title and the caption, and over a reference number', () => {
  const p = page(PAPER);
  const shown = p.shown(0);
  const s = p.select(0, shown.indexOf('study.'), shown.indexOf('Cells') + 5);
  assert.match(s.quote, /^study\.\s+Cells$/);   // the page's text between them: the newline and indent
  // the reference number is in the page's text, so it is in the quote (as in the reading view)
  const r = p.select(0, shown.indexOf('sequencing'), shown.indexOf('imaging') + 7);
  assert.equal(r.quote, 'sequencing14 and imaging');
  // whitespace at the ends is left out; nothing selected: nothing
  assert.equal(p.select(0, shown.indexOf(' sorted'), shown.indexOf(' by')).quote, 'sorted');
  assert.equal(p.select(0, 3, 3), null);
  assert.equal(p.select(5, 0, 4), null);
});

test('highlights drawn in the page show in the viewer at their words, by id and colour', () => {
  const p = page(PAPER);
  const shown = p.shown(0);
  const s = p.select(0, shown.indexOf('flow'), shown.indexOf(' before'));
  const t = p.select(0, shown.indexOf('study'), shown.indexOf('Cells') + 5);
  const before = p.text();
  assert.deepEqual(p.apply([{ id: 'h1', color: 'green', ...s }, { id: 'h2', color: 'pink', note: 'why?', ...t }]), []);
  assert.equal(p.text(), before);
  const marks = p.marks(0);
  assert.deepEqual(marks.map((m) => [m.id, m.c, m.n, m.t]), [
    ['h2', 'pink', true, 'study. Cells'],   // over the space between title and caption
    ['h1', 'green', false, 'flow cytometry'],
  ]);
  assert.deepEqual([marks[1].s, marks[1].e], [shown.indexOf('flow'), shown.indexOf(' before')]);
  // also in the figure's data (the viewer opens with them), the label and caption unchanged
  assert.equal(p.fig(0).marks.length, 2);
  assert.equal(p.fig(0).caption, 'Cells were sorted by flow cytometry before sequencing and imaging.');
  // a highlight made in the reading view over the caption's end and the next paragraph: its part in the caption
  const c2 = p.doc.getElementById('c2').firstChild, p2 = p.doc.getElementById('p2').firstChild;
  const u = p.pageSelect(c2, c2.data.indexOf('Scale'), p2, 4);
  p.apply([{ id: 'h1', color: 'green', ...s }, { id: 'h3', color: 'blue', ...u }]);
  const m2 = p.marks(1);
  assert.deepEqual(m2.map((m) => [m.id, m.t]), [['h3', 'Scale bar, 50 µm.']]);
  // the pending mark of highlight mode isn't one to tap
  assert.deepEqual(p.marks(5), []);
});

test('a book read chapter by chapter: the offset is in the whole book\'s text', () => {
  const html = `<html><head><meta name="lunta-text-base" content="5000"/></head><body class="continuous lunta-part">
    <h2>Chapter 3</h2><p id="p1">Regression is shown in Figure 3.1.</p>
    <figure><img src="c.png"/><figcaption><span class="ltx_tag">Figure 3.1</span> Regression and classification problems.</figcaption></figure>
  </body></html>`;
  const p = page(html);
  const shown = p.shown(0);
  assert.equal(p.fig(0).label, 'Figure 3.1');
  const s = p.select(0, shown.indexOf('classification'), shown.indexOf(' problems'));
  assert.equal(s.quote, 'classification');
  assert.equal(s.start, 5000 + p.text().indexOf('classification'));
  // drawn from that offset (the chapter's highlights are given with whole-book offsets)
  assert.deepEqual(p.apply([{ id: 'b1', color: 'yellow', ...s }]), []);
  assert.deepEqual(p.marks(0).map((m) => m.t), ['classification']);
});

test('figures shown as cards: the caption\'s text is still the page\'s, highlights found there', () => {
  const p = page(PAPER);   // cards are on by default (__luntaFigureCards unset)
  assert.ok(p.doc.querySelector('.m-figcard'));
  const shown = p.shown(1);
  const s = p.select(1, shown.indexOf('Iron-laden'), shown.indexOf(' in MS'));
  assert.equal(s.quote, 'Iron-laden microglia');
  assert.equal(s.start, p.text().indexOf('Iron-laden'));
});
