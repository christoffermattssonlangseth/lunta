// The citation card in the reading view (apple/Shared/CitationPopover.swift): clicking a citation
// shows its reference with "Add to Lunta" as a lunta://cite link the app handles: ?doi=, ?arxiv=,
// or ?ref=<the reference's text> when it has neither (the app looks the DOI up).
// Run: node --test scripts/reader-tests/

import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parse, run } from './mini-dom.mjs';

const repo = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
const src = readFileSync(join(repo, 'apple/Shared/CitationPopover.swift'), 'utf8').replace(/\r\n/g, '\n');
const m = src.match(/static let citationJS = #"""\n([\s\S]*?)\n([ \t]*)"""#/);
assert.ok(m, 'citationJS literal');
const script = m[1].split('\n').map((l) => l.slice(m[2].length)).join('\n');

/** The page, with what the card needs from a browser that mini-dom leaves out (events, layout, style). */
function page(html) {
  const doc = parse(html);
  const El = doc.createElement('x').constructor;
  const listeners = [];
  const on = function (type, fn) { listeners.push({ node: this, type, fn }); };
  El.prototype.addEventListener = on;
  doc.addEventListener = on;
  El.prototype.remove = function () { const p = this.parentNode; if (p) p.childNodes.splice(p.childNodes.indexOf(this), 1); this.parentNode = null; };
  El.prototype.getBoundingClientRect = () => ({ left: 10, top: 10, right: 30, bottom: 25, width: 20, height: 15 });
  Object.defineProperty(El.prototype, 'style', { configurable: true, get() { return (this._style ??= {}); } });
  Object.defineProperty(El.prototype, 'offsetWidth', { configurable: true, get: () => 300 });
  Object.defineProperty(El.prototype, 'offsetHeight', { configurable: true, get: () => 100 });
  Object.defineProperty(El.prototype, 'className', { configurable: true, get() { return this.getAttribute('class') ?? ''; }, set(v) { this.setAttribute('class', v); } });
  Object.defineProperty(El.prototype, 'innerHTML', { configurable: true, get() { return this.textContent; }, set(t) { this.textContent = t; } });
  const win = run(script, doc, {
    innerWidth: 800, innerHeight: 600,
    getComputedStyle: () => ({ backgroundColor: '#fff', color: '#000' }),
    getSelection: () => ({ isCollapsed: true }),
    addEventListener: () => {},
  });
  // the script calls the globals; give it the window's
  return { doc, win, listeners };
}

function click(p, target) {
  let stopped = false;
  const ev = { target, preventDefault() {}, stopPropagation() { stopped = true; } };
  for (const l of p.listeners) if (l.node === p.doc && l.type === 'click') l.fn(ev);
  return stopped;
}

const html = `
  <p>As shown <a id="c1" href="#r1">1</a> and <a id="c2" href="#r2">2</a> and <a id="c3" href="#r3">3</a>.</p>
  <section><h2>References</h2><ol class="refs">
    <li id="r1">Smith J. A title. Nature 2019. <a href="https://doi.org/10.1038/s41586-019-1234-5">doi</a></li>
    <li id="r2">Doe A. Preprint. arXiv:2101.01234, 2021.</li>
    <li id="r3">Hanahan D, Weinberg RA. Hallmarks of cancer: the next generation. Cell. 2011;144:646–674.</li>
  </ol></section>`;

function card(p) {
  const c = p.doc.documentElement.children.find((e) => e.className === 'm-cite');
  if (!c) return null;
  const btns = c.querySelectorAll('a').filter((a) => a.classList.contains('m-cite-btn'));
  return { text: c.querySelector('.m-cite-ref').textContent, buttons: btns.map((b) => [b.textContent, b.getAttribute('href')]) };
}

// globalThis stands in for the page's window while the click handler runs
function withGlobals(p, fn) {
  const saved = {};
  for (const k of ['getComputedStyle', 'getSelection', 'innerWidth', 'innerHeight', 'location']) { saved[k] = globalThis[k]; }
  Object.assign(globalThis, { getComputedStyle: p.win.getComputedStyle, getSelection: p.win.getSelection, innerWidth: 800, innerHeight: 600 });
  try { return fn(); } finally { for (const k of Object.keys(saved)) { if (saved[k] === undefined) delete globalThis[k]; else globalThis[k] = saved[k]; } }
}

test('a citation with a DOI: Add to Lunta by DOI', () => {
  const p = page(html);
  withGlobals(p, () => {
    assert.equal(click(p, p.doc.getElementById('c1')), true);
    const c = card(p);
    assert.match(c.text, /^Smith J\. A title/);
    assert.deepEqual(c.buttons.find((b) => b[0] === 'Add to Lunta'), ['Add to Lunta', 'lunta://cite?doi=10.1038%2Fs41586-019-1234-5']);
  });
});

test('a citation with an arXiv number: Add to Lunta by arXiv', () => {
  const p = page(html);
  withGlobals(p, () => {
    click(p, p.doc.getElementById('c2'));
    assert.deepEqual(card(p).buttons.find((b) => b[0] === 'Add to Lunta'), ['Add to Lunta', 'lunta://cite?arxiv=2101.01234']);
  });
});

test('a citation without either: Add to Lunta by its text, and Search', () => {
  const p = page(html);
  withGlobals(p, () => {
    click(p, p.doc.getElementById('c3'));
    const c = card(p);
    const add = c.buttons.find((b) => b[0] === 'Add to Lunta');
    assert.ok(add, 'Add to Lunta');
    assert.ok(add[1].startsWith('lunta://cite?ref='));
    assert.equal(decodeURIComponent(add[1].slice('lunta://cite?ref='.length)),
      'Hanahan D, Weinberg RA. Hallmarks of cancer: the next generation. Cell. 2011;144:646–674.');
    assert.ok(c.buttons.some((b) => b[0] === 'Search' && b[1].startsWith('https://scholar.google.com/')));
    // the app reports back
    p.win.luntaCiteDone('Added to your library');
    const done = card(p).buttons.find((b) => b[0] === 'Added to your library');
    assert.deepEqual(done, ['Added to your library', null]);
  });
});

test("a bioRxiv page's reference: without its link buttons and label, and its DOI from the CrossRef button", () => {
  // as "Use this page's full text" keeps a HighWire reference (bioRxiv, medRxiv, many society journals)
  const p = page(`
    <p>Profiles <a id="c3" href="ch14.xhtml#ref-3">3</a>.</p>
    <section><h2>References</h2><ol class="refs">
      <li>[3].<span id="ref-3"></span><a href="ch02.xhtml#xref-ref-3-1">↵</a>Kuilman T, et al. Preserved genetic diversity in organoids. Proc Natl Acad Sci U S A. 2015;112(43):13308–13311. <a href="https://www.biorxiv.org/content/10.64898/%7Bopenurl%7D?query=x">OpenUrl</a><a href="https://www.biorxiv.org/lookup/ijlink/abc">Abstract/FREE Full Text</a><a href="https://www.biorxiv.org/lookup/external-ref?access_num=10.1073/pnas.1516689112&amp;link_type=DOI">CrossRef</a><a href="https://www.biorxiv.org/lookup/external-ref?access_num=26460009&amp;link_type=MED">PubMed</a></li>
    </ol></section>`);
  withGlobals(p, () => {
    assert.equal(click(p, p.doc.getElementById('c3')), true);
    const c = card(p);
    assert.equal(c.text.trim(), 'Kuilman T, et al. Preserved genetic diversity in organoids. Proc Natl Acad Sci U S A. 2015;112(43):13308–13311.');
    assert.deepEqual(c.buttons.find((b) => b[0] === 'Add to Lunta'), ['Add to Lunta', 'lunta://cite?doi=10.1073%2Fpnas.1516689112']);
  });
});

test('a citation that is not a link (a reading view made from a PDF): superscript numbers and [1-3] open the card', () => {
  const p = page(`
    <p>Protein structure.<sup id="s">1,2</sup> And organoids <span id="t">profiles</span>.</p>
    <section><h2>References</h2><ol class="refs" style="list-style-type: none">
      <li>1. Lam, R. et al. Learning skillful medium-range global weather forecasting. Science 382, 1416–1421 (2023).</li>
      <li>2. Jumper, J. et al. Highly accurate protein structure prediction with AlphaFold. Nature 596, 583–589 (2021).</li>
    </ol></section>`);
  withGlobals(p, () => {
    const sup = p.doc.getElementById('s');
    assert.equal(click(p, sup.childNodes[0].parentNode), true);
    const c = card(p);
    assert.match(c.text.trim(), /^Lam, R\. et al\. Learning skillful/);
  });
  // ordinary words open nothing
  const q = page(`<p>Some <span id="t">profiles</span>.</p><ol class="refs"><li>1. Lam, R. (2023).</li></ol>`);
  withGlobals(q, () => assert.equal(click(q, q.doc.getElementById('t')), false));
});
