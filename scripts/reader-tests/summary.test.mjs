// The reader's Summary card (apple/Shared/SummaryCard.swift): what the summary is made from
// (luntaSummarySource: sections chosen, word budget) and the card itself, which must add no text
// to the page (highlights, find, reading places and Listen all count the page's text nodes).
// Run: node --test scripts/reader-tests/summary.test.mjs

import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parse, run } from './mini-dom.mjs';

const repo = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
const src = readFileSync(join(repo, 'apple/Shared/SummaryCard.swift'), 'utf8').replace(/\r\n/g, '\n');
const m = src.match(/static let summaryJS = #"""\n([\s\S]*?)\n([ \t]*)"""#/);
assert.ok(m, 'summaryJS literal');
const script = m[1].split('\n').map((l) => l.slice(m[2].length)).join('\n');

const words = (n, w = 'word') => Array.from({ length: n }, (_, i) => `${w}${i}`).join(' ') + '.';
const count = (s) => s.trim().split(/\s+/).length;

function page(html) {
  const doc = parse(html);
  const win = run(script, doc);
  return { doc, win, source: (max, abs) => JSON.parse(win.luntaSummarySource(max, abs)) };
}

const PAPER = `
  <section class="titlepage"><h1 class="title">A paper</h1><p class="authors">A. Author, B. Author</p>
    <p class="journal"><em>J Things</em> · 2024</p><p class="doi"><a href="https://doi.org/10.1/x">doi:10.1/x</a></p>
    <div class="frontmatter"><p class="affiliations">Some University, Some City, Some Country, far away</p></div></section>
  <div class="section-start" id="section-1"></div>
  <section><h1 id="abstract">Abstract</h1><p>ABSTRACT we measured many things in many mice and found that things grow.</p></section>
  <section><h1>1. Introduction</h1><p>INTRO1 background about things that have been studied for years by many.</p>
    <p>INTRO2 more background and the gap in what is known about the growth of things.</p>
    <p>AIMS here we asked whether things grow, and we tested it in mice over ten weeks.</p>
    <p>ROADMAP the rest of this paper is organized as follows: section 2 describes the methods.</p></section>
  <section><h1>2. Methods</h1><p>METHODS we did a careful experiment with many controls and a lot of care.</p></section>
  <section><h1>3. Results</h1><p>RESULTS1 things grew by forty percent in treated mice compared with controls.</p>
    <figure><figcaption>FIGCAPTION Figure 1. Things growing over ten weeks in all of the mice.</figcaption></figure></section>
  <section><h1>4. Discussion</h1><p>This section discusses the main results and their implications LEADIN.</p><p>DISC1 we found that things grow, which agrees with earlier work on the matter.</p>
    <p>DISC2 a limitation is that only mice were studied here, not other kinds of animals.</p>
    <h2>4.1 Limitations</h2><p>LIMITS more limitations are discussed in this part of the paper at length.</p></section>
  <section><h1>5. Conclusions</h1><p>CONCL1 things grow, and this matters for anyone who grows things at home.</p></section>
  <section><h1>Acknowledgements</h1><p>ACK we thank everyone who helped with the work on this paper.</p></section>
  <section><h1>References</h1><ol class="refs"><li id="r1">REF Smith J. 2020. Things. J Things 1:2. More words here.</li></ol></section>`;

test('source: abstract, the aims (not the roadmap), the first paragraphs of discussion and conclusion', () => {
  const { source } = page(PAPER);
  const s = source(1900);
  assert.deepEqual(s.parts, ['abstract', 'aims', 'discussion', 'conclusion']);
  for (const w of ['ABSTRACT', 'AIMS', 'DISC1', 'DISC2', 'CONCL1']) assert.match(s.text, new RegExp(w), w);
  // the discussion's lead-in is passed over; its short first paragraphs bring the next ones (up to ~120 words)
  assert.match(s.text, /LIMITS/);
  for (const w of ['INTRO1', 'INTRO2', 'ROADMAP', 'METHODS', 'RESULTS1', 'FIGCAPTION', 'LEADIN', 'ACK', 'REF', 'Author', 'University'])
    assert.doesNotMatch(s.text, new RegExp(w), w);
  assert.ok(s.text.indexOf('ABSTRACT') < s.text.indexOf('AIMS') && s.text.indexOf('AIMS') < s.text.indexOf('DISC1'));
});

test('source: structured abstract stays the abstract; results when there is no discussion', () => {
  const { source } = page(`
    <section><h1>Abstract</h1><h2>Background</h2><p>ABSBACK the background part of the structured abstract here.</p>
      <h2>Results</h2><p>ABSRES the results part of the structured abstract with numbers 1 2 3.</p>
      <h2>Conclusions</h2><p>ABSCONC the conclusions part of the structured abstract in brief.</p></section>
    <section><h1>Background</h1><p>INTRO in this study we examined how the structured abstract is read.</p></section>
    <section><h1>Results and discussion</h1><p>RES1 the first result paragraph of the combined section says much. ${words(130)}</p>
      <p>RES2 the second paragraph of the combined section says much more than the first.</p></section>`);
  const s = source(1900);
  assert.deepEqual(s.parts, ['abstract', 'aims', 'results']);
  for (const w of ['ABSBACK', 'ABSRES', 'ABSCONC', 'INTRO', 'RES1']) assert.match(s.text, new RegExp(w), w);
  assert.doesNotMatch(s.text, /RES2/);
});

test('source: a paper without sections gives its opening, within the budget', () => {
  const paras = Array.from({ length: 40 }, (_, i) => `<p>P${i} ${words(80)}</p>`).join('');
  const { source } = page(`<h1>Untitled</h1>${paras}`);
  const s = source(500);
  assert.deepEqual(s.parts, ['body']);
  assert.match(s.text, /^The beginning of the paper:\nP0 /);
  assert.ok(s.words <= 500 + 10, `${s.words} words`);
  assert.ok(s.words > 400, `${s.words} words`);
  assert.doesNotMatch(s.text, /P7 /);
});

test('source: the budget holds with long sections, and the app can give the abstract', () => {
  const long = (tag) => Array.from({ length: 12 }, (_, i) => `<p>${tag}${i} ${words(150)}</p>`).join('');
  const html = `<section><h1>Introduction</h1>${long('I')}</section><section><h1>Discussion</h1>${long('D')}</section>
    <section><h1>Conclusion</h1>${long('C')}</section>`;
  const { source } = page(html);
  for (const max of [300, 800, 1900]) {
    const s = source(max, 'META the abstract from the paper\'s details, given by the app. ' + words(2000, 'a'));
    assert.deepEqual(s.parts, ['abstract', 'aims', 'discussion', 'conclusion'], `${max}`);
    assert.ok(s.words <= max + 20, `${max}: ${s.words} words`);
    assert.ok(s.words >= max * 0.6, `${max}: ${s.words} words`);
    assert.match(s.text, /^Abstract:\nMETA /);
  }
  // the same page gives the same source (the cached summary's key)
  assert.equal(source(1900).text, page(html).source(1900).text);
});

test('card: under the title block, adds no text to the page, and goes away', () => {
  const { doc, win, source } = page(PAPER);
  const before = doc.body.textContent;
  const s0 = source(1900).text;
  assert.equal(win.luntaSummaryShow(JSON.stringify({ state: 'loading' })), true);
  win.luntaSummaryShow({ state: 'writing', text: 'Things grow by forty percent in treated mice.' });
  win.luntaSummaryShow({ state: 'done', text: 'Things grow by forty percent in treated mice. It matters.', note: 'Made on this device by Apple Intelligence' });
  const cards = doc.querySelectorAll('.m-summary');
  assert.equal(cards.length, 1);
  const card = cards[0];
  // placed in the title page, right after the DOI line (before the affiliations)
  const tp = doc.querySelector('.titlepage');
  assert.equal(card.parentNode, tp);
  assert.equal(card.previousElementSibling.className, 'doi');
  assert.equal(card.nextElementSibling.className, 'frontmatter');
  assert.equal(card.getAttribute('data-lunta-ui'), 'summary');
  // its words are attributes: the page's text (what highlights, find, places and Listen count) is unchanged
  assert.equal(doc.body.textContent, before);
  const texts = [];
  (function walk(n) { for (const c of n.childNodes) { if (c.nodeType === 3) texts.push(c); else walk(c); } })(card);
  assert.equal(texts.length, 0);
  assert.equal(card.querySelector('.m-sum-text').getAttribute('data-t'), 'Things grow by forty percent in treated mice. It matters.');
  assert.equal(card.querySelector('.m-sum-note').getAttribute('data-t'), 'Made on this device by Apple Intelligence');
  // nor what the summary is made from
  assert.equal(source(1900).text, s0);
  assert.equal(win.luntaSummaryHide(), true);
  assert.equal(doc.querySelectorAll('.m-summary').length, 0);
  assert.equal(doc.body.textContent, before);
  assert.equal(win.luntaSummaryHide(), false);
});

test('card: without a title page it goes under the first heading', () => {
  const { doc, win } = page(`<h1>Title</h1><p>First paragraph of the paper, long enough to count.</p>`);
  win.luntaSummaryShow({ state: 'done', text: 'A summary.' });
  const card = doc.querySelector('.m-summary');
  assert.equal(card.previousElementSibling.localName, 'h1');
  assert.equal(card.nextElementSibling.localName, 'p');
});

test('highlight offsets: the text nodes the annotation script walks are the same with the card', () => {
  // the walk annotationJS (Highlights.swift) does: every text node under body but script/style
  const hl = readFileSync(join(repo, 'apple/Shared/Highlights.swift'), 'utf8');
  assert.match(hl, /createTreeWalker\(document\.body, NodeFilter\.SHOW_TEXT/);
  const nodes = (doc) => {
    const out = [];
    (function walk(n) { for (const c of n.childNodes) { if (c.nodeType === 3) out.push(c.data); else if (!['script', 'style'].includes(c.localName)) walk(c); } })(doc.body);
    return out;
  };
  const { doc, win } = page(PAPER);
  const before = nodes(doc);
  win.luntaSummaryShow({ state: 'done', text: 'Some summary with words in it.', note: 'Note.' });
  assert.deepEqual(nodes(doc), before);
});

// While a summary is written the card changed a few times a second, and each change laid the pages
// out again and put the place being read back on its page: a page being turned by a finger was
// pulled back ("I cannot flip pages"). Changes wait for a still page; out of sight only the finished
// summary is drawn.
function live(html, { paged = false, pageNo = 0 } = {}) {
  const listeners = {};
  const calls = { keepPlace: 0, target: 0 };
  const extra = {
    addEventListener: (type, fn) => { (listeners[type] ??= []).push(fn); },
    luntaPaged: () => paged,
    luntaPageInfo: () => (paged ? { page: pageNo, pages: 9, cols: 1 } : null),
    luntaPlace: () => 1234,
    luntaKeepPlace: () => { calls.keepPlace++; },
    luntaTargetPage: () => { calls.target++; return pageNo; },
  };
  const doc = parse(html);
  const win = run(script, doc, extra);
  const fire = (type) => (listeners[type] ?? []).forEach((fn) => fn({}));
  const text = () => doc.querySelector('.m-summary')?.querySelector('.m-sum-text').getAttribute('data-t') ?? null;
  return { doc, win, fire, calls, text, turnTo: (n) => { pageNo = n; } };
}

test('card: no change while the page is touched or moving; the latest is drawn once it is still', (t) => {
  t.mock.timers.enable({ apis: ['setTimeout', 'Date'], now: 100000 });
  const { win, fire, text } = live(PAPER);
  assert.ok(win.luntaSummaryStill() > 1000);
  win.luntaSummaryShow({ state: 'writing', text: 'One.' });
  assert.equal(text(), 'One.');
  fire('touchstart');
  assert.equal(win.luntaSummaryStill(), 0);
  win.luntaSummaryShow({ state: 'writing', text: 'One. Two.' });
  win.luntaSummaryShow({ state: 'writing', text: 'One. Two. Three.' });
  t.mock.timers.tick(2000);
  assert.equal(text(), 'One.', 'not while a finger is on the page');
  fire('touchend');
  t.mock.timers.tick(300);
  assert.equal(text(), 'One.', 'not while the page still glides');
  fire('scroll');
  t.mock.timers.tick(500);
  assert.equal(text(), 'One.');
  t.mock.timers.tick(700);
  assert.equal(text(), 'One. Two. Three.', 'the latest, once the page is still');
  assert.ok(win.luntaSummaryStill() >= 700);
});

test('card: in pages, away from the first page, only the finished summary is drawn (one relayout)', (t) => {
  t.mock.timers.enable({ apis: ['setTimeout', 'Date'], now: 100000 });
  const { win, calls, text, turnTo } = live(PAPER, { paged: true, pageNo: 4 });
  win.luntaSummaryShow({ state: 'loading', text: 'Summarizing…' });
  for (let i = 1; i <= 20; i++) win.luntaSummaryShow({ state: 'writing', text: 'Word '.repeat(i) });
  assert.equal(text(), null, 'nothing drawn while it is written out of sight');
  assert.equal(calls.target, 0, 'the pages are not laid out again');
  win.luntaSummaryShow({ state: 'done', text: 'The summary.', note: 'Note.' });
  assert.equal(text(), 'The summary.');
  assert.equal(calls.keepPlace, 1);
  assert.equal(calls.target, 1, 'one relayout, at the place being read');
  // on the first page it is drawn as it comes
  turnTo(0);
  win.luntaSummaryShow({ state: 'writing', text: 'New words.' });
  assert.equal(text(), 'New words.');
  assert.equal(calls.target, 1, 'the first page needs no place kept');
});

test('card: hiding cancels a change still waiting', (t) => {
  t.mock.timers.enable({ apis: ['setTimeout', 'Date'], now: 100000 });
  const { win, fire, doc } = live(PAPER);
  fire('scroll');
  win.luntaSummaryShow({ state: 'done', text: 'The summary.' });
  assert.equal(win.luntaSummaryHide(), false);
  t.mock.timers.tick(2000);
  assert.equal(doc.querySelectorAll('.m-summary').length, 0);
});

test('source: the page is walked once while the app fits the prompt', () => {
  const { doc, win } = page(PAPER);
  // the walk's cost is a closest() per paragraph: count them on one
  const p = doc.querySelectorAll('p')[3];
  let looks = 0;
  const closest = p.closest.bind(p);
  p.closest = (sel) => { looks++; return closest(sel); };
  const a = JSON.parse(win.luntaSummarySource(1900)).text;
  const once = looks;
  assert.ok(once > 0);
  win.luntaSummarySource(900); win.luntaSummarySource(500);
  assert.equal(looks, once, 'the paragraphs are kept between calls');
  assert.equal(JSON.parse(win.luntaSummarySource(1900)).text, a);
});
