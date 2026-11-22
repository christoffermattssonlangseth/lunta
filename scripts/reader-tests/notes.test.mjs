// Footnotes in the reading view (apple/Shared/CitationPopover.swift): tapping a note's marker shows the
// note in a card ("Show in Notes" jumps to it) instead of jumping to the chapter's end; the same for a
// JATS paper's notes; a citation still opens its reference. Run: node --test scripts/reader-tests/

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

function page(html) {
  const doc = parse(html);
  const El = doc.createElement('x').constructor;
  const listeners = [];
  const on = function (type, fn) { listeners.push({ node: this, type, fn }); };
  El.prototype.addEventListener = on;
  doc.addEventListener = on;
  El.prototype.remove = function () { const p = this.parentNode; if (p) p.childNodes.splice(p.childNodes.indexOf(this), 1); this.parentNode = null; };
  El.prototype.getBoundingClientRect = () => ({ left: 10, top: 10, right: 30, bottom: 25, width: 20, height: 15 });
  El.prototype.contains = function (n) { for (; n; n = n.parentNode) if (n === this) return true; return false; };
  El.prototype.scrollIntoView = function () { scrolled.push(this); };
  const scrolled = [];
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
  return { doc, win, listeners, scrolled };
}

function click(p, target) {
  let stopped = false, prevented = false;
  const ev = { target, preventDefault() { prevented = true; }, stopPropagation() { stopped = true; } };
  for (const l of p.listeners) if (l.node === p.doc && l.type === 'click') l.fn(ev);
  return stopped && prevented;
}

function withGlobals(p, fn) {
  const saved = {};
  for (const k of ['getComputedStyle', 'getSelection', 'innerWidth', 'innerHeight', 'location']) saved[k] = globalThis[k];
  const loc = { hash: '' };
  Object.assign(globalThis, { getComputedStyle: p.win.getComputedStyle, getSelection: p.win.getSelection, innerWidth: 800, innerHeight: 600, location: loc });
  try { return fn(loc); } finally { for (const k of Object.keys(saved)) { if (saved[k] === undefined) delete globalThis[k]; else globalThis[k] = saved[k]; } }
}

const card = (p) => p.doc.documentElement.children.find((e) => (e.getAttribute('class') ?? '').split(' ').includes('m-cite'));

// what the PDF converters write (PDFAnalyzer.swift / notes.rs)
const pdfView = `
  <section><h1 id="s1">2 Supervised learning</h1>
  <p>Then we repeat this process until the gradient is flat and we can improve no further.<a epub:type="noteref" role="doc-noteref" href="#fn-p36-2" id="fnref-fn-p36-2" class="fnref">2</a> Squares x<sup>2</sup> stay.</p>
  <p>Cited work.<sup>1</sup></p>
  <aside class="sidenote" role="note"><p>Notebook 2.1 Supervised learning</p></aside>
  <div class="footnotes"><aside epub:type="footnote" role="doc-footnote" id="fn-p36-2" class="footnote"><p><a href="#fnref-fn-p36-2">2</a> This iterative approach is not actually necessary for the linear regression model.</p></aside></div>
  </section>
  <section><h2>References</h2><ol class="refs"><li>1. Smith J. A title. Nature 2019.</li></ol></section>`;

test("a footnote's marker opens the note in a card, without its label; Show in Notes goes to it", () => {
  const p = page(pdfView);
  withGlobals(p, (loc) => {
    const a = p.doc.getElementById('fnref-fn-p36-2');
    assert.equal(click(p, a), true);
    const c = card(p);
    assert.ok(c, 'card');
    assert.ok(c.getAttribute('class').includes('m-note'));
    assert.equal(c.querySelector('.m-cite-ref').textContent.trim(), 'This iterative approach is not actually necessary for the linear regression model.');
    const go = c.querySelectorAll('a').find((b) => b.textContent === 'Show in Notes');
    assert.ok(go, 'Show in Notes');
    let fired = false;
    for (const l of go.listeners ?? []) { if (l.type === 'click') { l.fn({ preventDefault() {}, stopPropagation() {} }); fired = true; } }
    if (!fired) for (const l of p.listeners) if (l.node === go && l.type === 'click') { l.fn({ preventDefault() {}, stopPropagation() {} }); fired = true; }
    assert.ok(fired);
    assert.equal(loc.hash, 'fn-p36-2');
    assert.equal(card(p), undefined, 'the card closes');
  });
});

test("the note's own label leads back to the marker as a plain link, and a citation still opens its reference", () => {
  const p = page(pdfView);
  withGlobals(p, () => {
    const back = p.doc.querySelectorAll('a').find((a) => a.getAttribute('href') === '#fnref-fn-p36-2');
    assert.equal(click(p, back), false, 'not taken over: the link jumps');
    assert.equal(card(p), undefined);
    const sup = p.doc.querySelectorAll('sup').find((s) => s.textContent === '1');
    assert.equal(click(p, sup), true);
    assert.match(card(p).querySelector('.m-cite-ref').textContent, /Smith J/);
    assert.ok(!card(p).getAttribute('class').includes('m-note'));
  });
});

test("a JATS paper's notes (an xref to an fn of the back matter's fn-group) open in the card too", () => {
  const p = page(`
    <p>As noted<a id="x" href="#fn1">a</a>.</p>
    <section><h1>Notes</h1><ol class="notes"><li id="fn1"><p>Data were collected in 2019.</p></li></ol></section>`);
  withGlobals(p, () => {
    assert.equal(click(p, p.doc.getElementById('x')), true);
    assert.equal(card(p).querySelector('.m-cite-ref').textContent.trim(), 'Data were collected in 2019.');
  });
});

test('notes and margin notes are styled: a "Notes" heading from CSS only, so no text is added', () => {
  assert.match(script, /\.footnotes::before \{ content: "Notes"/);
  assert.match(script, /aside\.sidenote \{[^}]*font-size: \.8em/);
  const p = page(pdfView);
  assert.ok(!p.doc.body.textContent.includes('Notes '), 'no "Notes" text in the page');
});
