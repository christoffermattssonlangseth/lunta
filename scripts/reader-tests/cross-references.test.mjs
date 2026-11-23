// Cross-references in the reading view (apple/Shared/CrossReferences.swift): "chapter 8",
// "section 10.4.1", "appendix C", "problem 3.5", "Table 2", "Eq. (2.3)" become tappable when the
// book or paper has that target; a book read chapter by chapter knows the other parts' targets from
// <meta name="lunta-xrefs"> (core: epub::xref_keys) and links there through the part's page.
// Run: node --test scripts/reader-tests/

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
const xrefJS = literal('apple/Shared/CrossReferences.swift', 'xrefJS');
const figureJS = literal('apple/Shared/FigureViewer.swift', 'figureJS');
globalThis.Element ??= Element;

function text(doc) {
  let t = '';
  const walk = (n) => n.childNodes.forEach((c) => (c.nodeType === 3 ? (t += c.data) : walk(c)));
  walk(doc.body);
  return t;
}

function page(html, { figures = false } = {}) {
  const doc = parse(html);
  const before = text(doc);
  const jumps = [];
  const location = { href: '' };
  if (figures) run(figureJS, doc, { webkit: { messageHandlers: { fig: { postMessage() {} } } } });
  const win = run(xrefJS, doc, { location, luntaMarkJump: () => jumps.push(1), getSelection: () => ({ isCollapsed: true }) });
  const links = doc.querySelectorAll('span.m-xref').map((s) => [s.textContent, s.getAttribute('data-xref'), s.getAttribute('data-xref-part')]);
  const tap = (span) => doc.dispatch({ type: 'click', target: span });
  return { doc, win, before, after: text(doc), links, jumps, location, tap };
}

// a few chapters of the textbook, with its numbered headings and problems
const BOOK = `
  <section><h1 id="s8">1 Introduction</h1><p>Chapter 12 (transformers) and chapter 3 follow.</p></section>
  <section><h1 id="s30">2 Supervised learning</h1><h2 id="s31">2.1 Supervised learning overview</h2>
    <h3 id="s35">2.2.3 Training</h3>
    <p>so we can calculate</p><p>Problems 2.1–2.2 the loss for every combination</p>
    <h3 id="s41">Problems</h3><p><strong>Problem 2.1</strong> To walk “downhill” (equation 2.5), see section 2.2.3.</p>
    <p><strong>Problem 2.2</strong> Setting the derivatives from problem 2.1 to zero.</p></section>
  <section><h1 id="s42">3 Shallow neural networks</h1><h2 id="s46">3.2 Universal approximation theorem</h2>
    <h2 id="s52">3.4 Shallow neural networks: general case</h2><h2 id="s53">3.5 Terminology</h2></section>
  <section><h1 id="s208">12 Transformers</h1></section>
  <section><h1 id="s620">A Notation</h1></section>
  <section><h1 id="s629">B Mathematics</h1><h2 id="s635">B.2 Binomial coefficients</h2></section>
  <section><h1 id="s655">C Probability</h1><h3 id="s685">C.5.2 Kullback-Leibler divergence</h3></section>`;

test('a book: chapters, sections, appendices and problems from its numbered headings and labels', () => {
  const p = page(`<html><head></head><body>${BOOK}
    <p>Chapters 2–3 walk through supervised learning; in section 3.2, we argued (see section 3.4), as in §2.1.</p>
    <p>Readers are encouraged to consult appendix A, Appendix B.2 and Appendix C.5.2 before chapter 2.</p>
    <p>Sections 3.2–3.4 and 3.5 discuss it, as do Chapters 1, 2 and 12.</p></body></html>`);
  assert.deepEqual(p.links.map((l) => [l[0], l[1]]), [
    ['Chapter 12', '12'], ['chapter 3', '3'],
    ['Problems 2.1', 'problem:2.1'], ['2.2', 'problem:2.2'],
    ['section 2.2.3', '2.2.3'], ['problem 2.1', 'problem:2.1'],
    ['Chapters 2', '2'], ['section 3.2', '3.2'], ['section 3.4', '3.4'], ['§2.1', '2.1'],
    ['appendix A', 'A'], ['Appendix B.2', 'B.2'], ['Appendix C.5.2', 'C.5.2'], ['chapter 2', '2'],
    ['Sections 3.2', '3.2'], ['3.4', '3.4'], ['3.5', '3.5'], ['Chapters 1', '1'], ['2', '2'], ['12', '12'],
  ]);
  // the problem's own label isn't a link to itself; the headings aren't linked
  const spans = p.doc.querySelectorAll('span.m-xref');
  assert.equal(spans.filter((s) => s.closest('strong, h1, h2, h3')).length, 0);
  // the text is exactly as it was: highlights and reading places keep their offsets
  assert.equal(p.after, p.before);
});

test('what is not a cross-reference', () => {
  const p = page(`<html><head></head><body>${BOOK}
    <p>This book has 8 chapters and 12 sections; the model (2.3) and equation 2.5 (no number tags here).</p>
    <p>A cross-section 3 view, Chapter8, chapter 9 and section 3.3 (no such targets), section 3.2a, problem 7.1.</p>
    <p>An appendix a reader skips; Section 3.2.7 doesn't exist. <a href="#s46">section 3.2</a> is already a link.</p>
    <p><code>chapter 3</code></p></body></html>`);
  assert.deepEqual(p.links.filter((l) => !['Chapter 12', 'chapter 3', 'Problems 2.1', '2.2', 'section 2.2.3', 'problem 2.1'].includes(l[0])), []);
  assert.equal(p.after, p.before);
});

test('a lone letter is an appendix only next to other appendices', () => {
  const p = page(`<html><head></head><body><h1>1 Start</h1><h2>A Brief History</h2><p>See appendix A and section A.</p></body></html>`);
  assert.deepEqual(p.links, []);
  const q = page(`<html><head></head><body><h1>1 Start</h1><h1>Appendix A Proofs</h1><p>See appendix A and Appendix A.</p></body></html>`);
  assert.deepEqual(q.links.map((l) => l[1]), ['app:A', 'app:A']);
});

test('papers: numbered sections, supplementary notes, tables and numbered equations', () => {
  const p = page(`<html><head></head><body>
    <h2>1. Introduction</h2><p>We describe the method in Section 2 and the data in Supplementary Note 3; Section 5 does not exist.</p>
    <h2>2 Methods</h2><p>Results are in Table 2 and Supplementary Table 1 (Tables 2 and 3), using Eq. (2.3) and Eqs. (2.3) and (2.4); see equation 2.4.</p>
    <span class="ltx_equation"><span class="ltx_tag ltx_tag_equation">(2.3)</span></span>
    <table class="ltx_equation"><tr><td><span class="ltx_tag_equation">(2.4)</span></td></tr></table>
    <figure class="ltx_table"><figcaption>Table 2: Accuracy. Table 2 lists the results.</figcaption><table><tr><td>1</td></tr></table></figure>
    <div class="table-wrap"><div class="caption">Supplementary Table 1. Cohort</div><table></table></div>
    <h2>Supplementary Note 3: Data</h2><p>Section 1 introduced it.</p></body></html>`);
  assert.deepEqual(p.links.map((l) => [l[0], l[1]]), [
    ['Section 2', '2'], ['Supplementary Note 3', 'note:3'],
    ['Table 2', 'table:2'], ['Supplementary Table 1', 'table:S1'], ['Tables 2', 'table:2'],
    ['Eq. (2.3)', 'eq:2.3'], ['Eqs. (2.3)', 'eq:2.3'], ['(2.4)', 'eq:2.4'], ['equation 2.4', 'eq:2.4'],
    ['Section 1', '1'],
  ]);
  assert.equal(p.after, p.before);
});

test('a book made from a PDF: the equation numbers it keeps as text, appendix ones too', () => {
  const p = page(`<html><head></head><body><h1>7 Gradients</h1>
    <p>Combining equations 7.9 and 7.10 with equation C.8 gives it.</p>
    <div class="equation" id="eq-7-9"><span class="eqlabel eqno">(7.9)</span><img class="equation" src="e.png" alt="f0 = b0 + w0 x"/></div>
    <div class="equation" id="eq-C-8"><span class="eqlabel eqno">(C.8)</span><img class="equation" src="c.png" alt="E[x]"/></div>
    <div class="equation" id="eq-7-10"><span class="eqlabel eqno">(7.10)</span><img class="equation" src="d.png" alt="d"/></div></body></html>`);
  assert.deepEqual(p.links.map((l) => [l[0], l[1]]), [['equations 7.9', 'eq:7.9'], ['7.10', 'eq:7.10'], ['equation C.8', 'eq:C.8']]);
  assert.equal(p.after, p.before);
});

test('with figure mentions: both linked, neither in the other, the text unchanged', () => {
  const p = page(`<html><head></head><body><h1>2 Supervised</h1><h2>2.1 Overview</h2>
    <figure><img src="f.png" alt="Figure"/><figcaption><strong>Figure 2.1</strong> A model (see section 2.1).</figcaption></figure>
    <p>Figure 2.1 shows it, section 2.1 explains it (Smith et al., <a href="#b1">2019</a>).</p></body></html>`, { figures: true });
  assert.deepEqual(p.links.map((l) => l[0]), ['section 2.1', 'section 2.1']);
  assert.equal(p.doc.querySelectorAll('span.m-figref').length, 1);
  assert.equal(p.doc.querySelectorAll('span.m-xref').filter((s) => s.closest('.m-figref') || s.querySelector('.m-figref')).length, 0);
  assert.equal(p.doc.querySelector('a').textContent, '2019');
  assert.equal(p.after, p.before);
});

test('a tap goes to the target in this page, with Back', () => {
  const p = page(`<html><head></head><body>${BOOK}<p>See section 3.4 and problem 2.2.</p></body></html>`);
  const spans = p.doc.querySelectorAll('span.m-xref');
  const ev = p.tap(spans.find((s) => s.textContent === 'section 3.4'));
  assert.equal(ev.defaultPrevented, true);
  assert.equal(p.doc.scrolledTo, p.doc.getElementById('s52'));
  assert.equal(p.doc.scrollOptions.block, 'start');
  assert.equal(p.jumps.length, 1);
  p.tap(spans.find((s) => s.textContent === 'problem 2.2'));
  assert.equal(p.doc.scrolledTo.textContent.startsWith('Problem 2.2'), true);
  assert.equal(p.doc.scrollOptions.block, 'center');
  assert.equal(p.location.href, '');
});

test('chapter by chapter: a target in another part opens that part at it', () => {
  // part 3 of the book: chapter 2; the meta lists the whole book's targets and their parts
  const map = { 1: 2, 2: 3, '2.1': 3, 3: 4, '3.4': 4, 12: 9, A: 20, B: 21, 'B.2': 21, C: 22, 'problem:2.1': 3, 'problem:3.5': 4 };
  const meta = `<meta name="lunta-text-base" content="51234"/><meta name="lunta-part" content="3" data-parts="27"/>` +
    `<meta name="lunta-xrefs" content="${JSON.stringify(map).replace(/"/g, '&quot;')}"/>`;
  const p = page(`<html><head>${meta}</head><body class="continuous lunta-part">
    <h1 id="s30">2 Supervised learning</h1><h2 id="s31">2.1 Supervised learning overview</h2>
    <p>As chapter 1 said, section 2.1 and section 3.4 (in chapter 3), problem 3.5, appendix A, Appendix B.2; chapter 7 isn't in the book.</p>
    <p><strong>Problem 2.1</strong> See problem 2.1.</p></body></html>`);
  assert.deepEqual(p.links, [
    ['chapter 1', '1', '2'], ['section 2.1', '2.1', null], ['section 3.4', '3.4', '4'], ['chapter 3', '3', '4'],
    ['problem 3.5', 'problem:3.5', '4'], ['appendix A', 'A', '20'], ['Appendix B.2', 'B.2', '21'],
  ]);
  assert.equal(p.after, p.before);
  // a tap on one in another part: the reader opens that part's page (ChapterReader.open(link:))
  const ev = p.tap(p.doc.querySelectorAll('span.m-xref')[2]);
  assert.equal(ev.defaultPrevented, true);
  assert.equal(p.location.href, 'lunta-part-4.xhtml#lunta-xref-3.4');
  // ...and there goes to the target (no Back: it's another page)
  const q = page(`<html><head><meta name="lunta-part" content="4" data-parts="27"/></head><body>
    <h1 id="s42">3 Shallow neural networks</h1><p>text</p><h2 id="s52">3.4 General case</h2></body></html>`);
  assert.equal(q.win.luntaXrefGo('3.4'), true);
  assert.equal(q.doc.scrolledTo, q.doc.getElementById('s52'));
  assert.equal(q.jumps.length, 0);
  assert.equal(q.win.luntaXrefGo('9.9'), false);
});

test('a page without any targets is left as it was', () => {
  const p = page('<html><head></head><body><p>See chapter 3 and section 2.1, problem 1.1.</p></body></html>');
  assert.deepEqual(p.links, []);
  assert.equal(p.doc.querySelectorAll('span').length, 0);
});
