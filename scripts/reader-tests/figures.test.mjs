// The figure script's page side (apple/Shared/FigureViewer.swift): labels, captions and mentions,
// for papers ("Figure 2", "Fig. 2B", "Figures 3A and 3C") and books, which number figures by
// chapter ("Figure 1.2", "Figs. 3.4–3.6", "Figure A.1", "Figure 3-4").
// Run: node --test scripts/reader-tests/

import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parse, run, Element } from './mini-dom.mjs';

const repo = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
const src = readFileSync(join(repo, 'apple/Shared/FigureViewer.swift'), 'utf8').replace(/\r\n/g, '\n');
const m = src.match(/static let figureJS = #"""\n([\s\S]*?)\n([ \t]*)"""#/);
assert.ok(m, 'figureJS literal');
const script = m[1].split('\n').map((l) => l.slice(m[2].length)).join('\n');
globalThis.Element ??= Element;   // the script wraps Element.prototype.scrollIntoView (later, in a timeout)

function figures(html) {
  const doc = parse(html);
  const sent = [];
  const win = run(script, doc, { webkit: { messageHandlers: { fig: { postMessage: (x) => sent.push(x) } } } });
  const list = sent.find((x) => x.list)?.list ?? [];
  // the mentions made tappable: their text and the label of the figure each opens
  const refs = doc.querySelectorAll('span.m-figref').map((s) => [s.textContent, list[+s.getAttribute('data-fig-ref')].label, s.getAttribute('data-panel') ?? '']);
  return { list, refs, win };
}

const fig = (n, cap) => `<figure><img src="../images/f${n}.png" alt="Figure"/><figcaption>${cap}</figcaption></figure>`;

test('a book: chapter-numbered labels and their captions', () => {
  const { list } = figures(`
    <p>Figure 1.2 depicts several regression and classification problems.</p>
    ${fig(1, '<strong>Figure 1.1</strong> Machine learning is an area of artificial intelligence.')}
    ${fig(2, '<strong>Figure 1.2</strong> Regression and classification problems. a) This regression model.')}
    ${fig(3, 'Figure 10.14. Many layers.')}
    ${fig(4, 'Figure A.1 Appendix figure.')}
    ${fig(5, 'Figure S1.2: A supplementary one.')}
    ${fig(6, 'Figure 3-4 Hyphenated.')}
    ${fig(7, 'Figure 1. Overview of a paper.')}`);
  assert.deepEqual(list.map((f) => f.label), ['Figure 1.1', 'Figure 1.2', 'Figure 10.14', 'Figure A.1', 'Figure S1.2', 'Figure 3-4', 'Figure 1']);
  assert.equal(list[0].caption, 'Machine learning is an area of artificial intelligence.');
  assert.equal(list[1].caption, 'Regression and classification problems. a) This regression model.');
  assert.equal(list[2].caption, 'Many layers.');
  assert.equal(list[3].caption, 'Appendix figure.');
  assert.equal(list[6].caption, 'Overview of a paper.');
});

test('a book: mentions open the right figure, ranges and lists of dotted numbers', () => {
  const { refs } = figures(`
    ${fig(1, 'Figure 1.1 One.')}${fig(2, 'Figure 1.2 Two.')}${fig(3, 'Figure 3.4 Three.')}
    ${fig(4, 'Figure 3.5 Four.')}${fig(5, 'Figure 3.6 Five.')}${fig(6, 'Figure A.1 Six.')}${fig(7, 'Figure 2-3 Seven.')}
    <p>Figure 1.2 depicts problems; the model in figure 1.2a predicts a price (Figs. 3.4–3.6), see also
    Figures 1.1 and 3.5, Fig. A.1, Figure 2-3 and figure 1.10, which does not exist. Fig. 1.</p>`);
  assert.deepEqual(refs, [
    ['Figure 1.2', 'Figure 1.2', ''], ['figure 1.2a', 'Figure 1.2', 'a'], ['Figs. 3.4', 'Figure 3.4', ''], ['3.6', 'Figure 3.6', ''],
    ['Figures 1.1', 'Figure 1.1', ''], ['3.5', 'Figure 3.5', ''], ['Fig. A.1', 'Figure A.1', ''], ['Figure 2-3', 'Figure 2-3', ''],
  ]);
});

test('a paper: labels, panels, ranges and lists as before', () => {
  const { list, refs } = figures(`
    ${fig(1, '<strong>Figure 1.</strong> Overview of the study.')}${fig(2, 'Fig. 2 | Results')}${fig(3, 'Figure 3 Maps')}
    ${fig(4, 'Figure S1. Supplementary.')}
    <p>As in Figure 1, Fig. 2B and Figures 1A and 3C (Figs. 2–3; Figure S1). Figure 1.5 is not one of them.</p>`);
  assert.deepEqual(list.map((f) => f.label), ['Figure 1', 'Fig. 2', 'Figure 3', 'Figure S1']);
  assert.equal(list[0].caption, 'Overview of the study.');
  assert.deepEqual(refs, [
    ['Figure 1', 'Figure 1', ''], ['Fig. 2B', 'Fig. 2', 'B'], ['Figures 1A', 'Figure 1', 'A'], ['3C', 'Figure 3', 'C'],
    ['Figs. 2', 'Fig. 2', ''], ['3', 'Figure 3', ''], ['Figure S1', 'Figure S1', ''],
  ]);
});
