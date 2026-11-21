// Tests for the citation formatting shared by the Word add-in and the Google Docs sidebar.
// Run: node --test apple/citations-tests/
const test = require('node:test');
const assert = require('node:assert/strict');
const C = require('../citations/common.js');

// What Lunta's /api/bibliography answers for a paper (entries in the built-in styles come from the core)
function paper(id, families, year, entry, extra) {
  return Object.assign({ id, families, authors: families.map((f) => 'A. ' + f), year: String(year), title: 'Title of ' + id, entry,
    meta: { title: 'Title of ' + id, authors: families.map((f) => 'A. ' + f), date: String(year) } }, extra || {});
}

const smithA = paper('a', ['Smith'], 2020, 'Smith, A. (2020). Alpha. *Cell*.');
const smithB = paper('b', ['Smith'], 2020, 'Smith, A. (2020). Beta. *Cell*.');
const lee = paper('c', ['Lee', 'Wu'], 2019, 'Lee, A., & Wu, A. (2019). Gamma. *Nature*.');
const many = paper('d', ['Kukanja', 'Langseth', 'Castelo-Branco'], 2024, 'Kukanja, P., Langseth, C. M., & Castelo-Branco, G. (2024). Delta.');
const all = [smithA, smithB, lee, many];

test('tags and links: read as written by every version, ids cleaned', () => {
  assert.deepEqual(C.parseTag('margins:a,b'), ['a', 'b']);
  assert.deepEqual(C.parseTag('margins: a , b,,a'), ['a', 'b']);
  assert.deepEqual(C.parseTag('margins:'), []);
  assert.equal(C.parseTag('margins-bib'), null);
  assert.equal(C.parseTag('something else'), null);
  assert.equal(C.parseTag(null), null);
  assert.equal(C.makeTag(['a', 'b', 'a']), 'margins:a,b');
  // the Google Docs link as the first version wrote it
  const old = 'https://margins.local/cite?ids=' + encodeURIComponent(['x-1', 'y-2'].join(','));
  assert.deepEqual(C.parseLink(old), ['x-1', 'y-2']);
  assert.equal(C.makeLink(['x-1', 'y-2']), old);
  assert.deepEqual(C.parseLink('https://margins.local/cite?ids=%E0%A4%A'), ['%E0%A4%A']);   // broken escape: kept as is
  assert.equal(C.parseLink('https://margins.local/bib'), null);
});

test('numbers: by first citation, repeats keep their number, ranges from three', () => {
  assert.equal(C.compress([3, 1, 2, 5]), '1–3,5');
  assert.equal(C.compress([1, 2]), '1,2');
  assert.equal(C.compress([2, 2, 1]), '1,2');
  assert.equal(C.compress([1, 2, 3, 4, 7, 8, 9, 11]), '1–4,7–9,11');
  const cites = [['c'], ['a', 'b'], ['c'], ['d', 'a', 'c']];
  const f = C.format('vancouver', cites, all, {});
  assert.deepEqual(f.texts, ['[1]', '[2,3]', '[1]', '[1,2,4]']);
  assert.deepEqual(f.bibliography.map((e) => e.text.slice(0, 3)), ['1. ', '2. ', '3. ', '4. ']);
  assert.deepEqual(f.bibliography.map((e) => e.id), ['c', 'a', 'b', 'd']);
  assert.equal(f.references, 4);
  assert.equal(f.superscript, false);
  const ieee = C.format('ieee', cites, all, {});
  assert.ok(ieee.bibliography[0].text.startsWith('[1] Lee'));
  const nat = C.format('nature', [['a', 'b', 'c']], all, {});
  assert.deepEqual(nat.texts, ['1–3']);
  assert.equal(nat.superscript, true);
});

test('the bibliography is exactly the cited papers, whatever Lunta sends', () => {
  const f = C.format('apa', [['c']], all, {});   // Lunta answered for more papers than are cited
  assert.deepEqual(f.bibliography.map((e) => e.id), ['c']);
  const none = C.format('apa', [], [], {});
  assert.deepEqual(none.bibliography, []);
  assert.deepEqual(none.texts, []);
});

test('author–year: names, et al., two works by the same names in one year get a and b', () => {
  const f = C.format('apa', [['a'], ['b'], ['c'], ['d'], ['b', 'a', 'c']], all, {});
  assert.deepEqual(f.texts, ['(Smith, 2020a)', '(Smith, 2020b)', '(Lee & Wu, 2019)', '(Kukanja et al., 2024)', '(Lee & Wu, 2019; Smith, 2020a, 2020b)']);
  // sorted reference list, letters in it too
  assert.deepEqual(f.bibliography.map((e) => e.text), [
    'Kukanja, P., Langseth, C. M., & Castelo-Branco, G. (2024). Delta.',
    'Lee, A., & Wu, A. (2019). Gamma. *Nature*.',
    'Smith, A. (2020a). Alpha. *Cell*.',
    'Smith, A. (2020b). Beta. *Cell*.'
  ]);
  // only one Smith 2020 cited: no letter
  assert.deepEqual(C.format('apa', [['b']], all, {}).texts, ['(Smith, 2020)']);
  // Harvard: "and", et al. from four; Chicago: serial comma, no comma before the year
  const h = C.format('harvard', [['c'], ['d']], all, {});
  assert.deepEqual(h.texts, ['(Lee and Wu, 2019)', '(Kukanja, Langseth and Castelo-Branco, 2024)']);
  const ch = C.format('chicago', [['d']], all, {});
  assert.deepEqual(ch.texts, ['(Kukanja, Langseth, and Castelo-Branco 2024)']);
  // MLA: no year, works by the same author told apart by title
  const mla = C.format('mla', [['a'], ['c']], all, {});
  assert.deepEqual(mla.texts, ['(Smith)', '(Lee and Wu)']);
  const mla2 = C.format('mla', [['a'], ['b']], all, {});
  assert.deepEqual(mla2.texts, ['(Smith, “Title of a”)', '(Smith, “Title of b”)']);
});

test('year suffixes: in the entry once, n.d. as n.d.-a', () => {
  assert.equal(C.addSuffix('Smith (2020). In 2020 we…', '2020', 'a'), 'Smith (2020a). In 2020 we…');
  assert.equal(C.addSuffix('Smith (12020). 2020 x', '2020', 'b'), 'Smith (12020). 2020b x');
  assert.equal(C.addSuffix('Smith (n.d.). X.', '', 'a'), 'Smith (n.d.-a). X.');
  const nd1 = paper('n1', ['Doe'], '', 'Doe, J. (n.d.). One.'), nd2 = paper('n2', ['Doe'], '', 'Doe, J. (n.d.). Two.');
  nd1.year = ''; nd2.year = '';
  const f = C.format('apa', [['n1', 'n2']], [nd1, nd2], {});
  assert.deepEqual(f.texts, ['(Doe, n.d.a, n.d.b)']);
  assert.deepEqual(f.bibliography.map((e) => e.text), ['Doe, J. (n.d.-a). One.', 'Doe, J. (n.d.-b). Two.']);
});

test('Update All twice gives the same result (nothing to rewrite the second time)', () => {
  const cites = [['d', 'a'], ['c'], ['a']];
  const one = C.format('vancouver', cites, all, {});
  const two = C.format('vancouver', cites, all, one.store);
  assert.deepEqual(two.texts, one.texts);
  assert.deepEqual(two.bibliography, one.bibliography);
  assert.deepEqual(two.store, one.store);
});

test('a merged duplicate (old id) is one reference with one number', () => {
  const merged = Object.assign({}, smithA, { id: 'old', libraryId: 'a' });
  const f = C.format('vancouver', [['a'], ['old'], ['c']], [smithA, merged, lee], {});
  assert.deepEqual(f.texts, ['[1]', '[1]', '[2]']);
  assert.equal(f.bibliography.length, 2);
  assert.deepEqual(Object.keys(f.store).sort(), ['a', 'c', 'old']);
});

test('papers Lunta lacks: from the document’s copy, or nothing is written', () => {
  const first = C.format('apa', [['a'], ['c']], [smithA, lee], {});
  // deleted from the library: an older Lunta says missing; the document's copy has this style's entry
  const later = C.format('apa', [['a'], ['c']], [smithA, { id: 'c', missing: true, entry: '' }], first.store);
  assert.deepEqual(later.unknown, []);
  assert.deepEqual(later.texts, first.texts);
  assert.deepEqual(later.fromDocument, ['c']);
  assert.deepEqual(later.bibliography, first.bibliography);
  // a new Lunta formats it from the details the document sent
  const fromDoc = Object.assign({}, lee, { fromDocument: true });
  assert.deepEqual(C.format('vancouver', [['c']], [fromDoc], first.store).fromDocument, ['c']);
  // another style and no copy in it: nothing to go on
  const other = C.format('vancouver', [['a'], ['c']], [smithA, { id: 'c', missing: true }], first.store);
  assert.deepEqual(other.unknown, ['c']);
  assert.deepEqual(other.texts, []);
  // never seen: unknown, and the copy of the others is kept
  const never = C.format('apa', [['a'], ['zzz']], [smithA, { id: 'zzz', missing: true }], {});
  assert.deepEqual(never.unknown, ['zzz']);
  assert.ok(never.store.a);
  // details sent back to Lunta for the cited ids only
  assert.deepEqual(Object.keys(C.detailsFor(first.store, ['c', 'x'])), ['c']);
  assert.equal(C.detailsFor(first.store, ['c']).c.title, 'Title of c');
});

test('Lunta not running: the document’s copy of the last style', () => {
  const first = C.format('nature', [['a'], ['c', 'a']], [smithA, lee], {});
  // a citation removed and another moved first while Lunta was closed
  const off = C.format('nature', [['c']], null, first.store);
  assert.deepEqual(off.unknown, []);
  assert.deepEqual(off.texts, ['1']);
  assert.deepEqual(off.bibliography.map((e) => e.id), ['c']);
  assert.deepEqual(Object.keys(off.store), ['c']);   // only cited papers kept
  assert.deepEqual(C.format('apa', [['c']], null, first.store).unknown, ['c']);
});

test('what a document keeps is small: long author lists cut, no abstracts', () => {
  const authors = Array.from({ length: 300 }, (_, i) => 'Author' + i + ' Name');
  const big = paper('big', ['Author0'], 2021, 'Big entry', { authors, meta: { title: 'T', authors, date: '2021', abstract: 'x'.repeat(10000) } });
  const f = C.format('apa', [['big']], [big], {});
  assert.equal(f.store.big.m.authors.length, 21);
  assert.equal(f.store.big.m.authors[20], 'Author299 Name');
  assert.ok(JSON.stringify(f.store).length < 4000);
  // an older Lunta sends no meta: kept from the fields it does send
  const oldLunta = { id: 'o', title: 'Old', authors: ['Ada Lee'], families: ['Lee'], year: '2001', journal: 'J', doi: '10.1/x', entry: 'Lee (2001)' };
  const g = C.format('apa', [['o']], [oldLunta], {});
  assert.deepEqual(g.store.o.m, { title: 'Old', authors: ['Ada Lee'], date: '2001', journal: 'J', doi: '10.1/x' });
});

test('journal styles: empty citations aren’t written, missing papers reported', () => {
  const res = { title: 'Nature Neuroscience', citations: [{ text: '1,2', superscript: true }, { text: '', superscript: false }],
    bibliography: [{ id: 'a', text: '1. A' }, { id: 'c', text: '2. C' }], items: [smithA, lee], missing: ['gone'] };
  const f = C.formatJournal(res, [['a', 'c'], ['gone']], {});
  assert.deepEqual(f.texts, ['1,2', null]);
  assert.deepEqual(f.superscripts, [true, false]);
  assert.deepEqual(f.unknown, ['gone']);
  assert.deepEqual(Object.keys(f.store).sort(), ['a', 'c']);
  // an older Lunta: no "missing", an empty citation gives it away
  const old = C.formatJournal({ title: 'X', citations: [{ text: '' }], bibliography: [] }, [['gone']], {});
  assert.deepEqual(old.unknown, ['gone']);
});

test('markup: *italic* and **bold**, and asterisks that are text', () => {
  assert.deepEqual(C.runs('Lee. Gamma. *Nature* **187**, 1–2.'), [
    { text: 'Lee. Gamma. ' }, { text: 'Nature', italic: true }, { text: ' ' }, { text: '187', bold: true }, { text: ', 1–2.' }]);
  assert.deepEqual(C.runs('Solving A* search in 2*3 grids. *Cell*.'), [{ text: 'Solving A* search in 2*3 grids. ' }, { text: 'Cell', italic: true }, { text: '.' }]);
  assert.deepEqual(C.runs('p* < 0.05 and q*'), [{ text: 'p* < 0.05 and q*' }]);
  assert.deepEqual(C.runs('*Cell* **187,** 1990'), [{ text: 'Cell', italic: true }, { text: ' ' }, { text: '187,', bold: true }, { text: ' 1990' }]);
  assert.equal(C.plain('A *b* **c**'), 'A b c');
  assert.equal(C.toHTML('Signals <10 Hz & "quiet" *Cell*'), 'Signals &lt;10 Hz &amp; &quot;quiet&quot; <i>Cell</i>');
  assert.equal(C.toHTML('<script>alert(1)</script>'), '&lt;script&gt;alert(1)&lt;/script&gt;');
  assert.equal(C.toHTML('单细胞 Ωmega β-cells *Αθήνα*'), '单细胞 Ωmega β-cells <i>Αθήνα</i>');
  assert.deepEqual(C.runs(''), []);
});

test('family names as the core reads them', () => {
  assert.equal(C.familyOf('Ludwig van Beethoven'), 'van Beethoven');
  assert.equal(C.familyOf('Langseth, Christoffer M.'), 'Langseth');
  assert.equal(C.familyOf('WHO'), 'WHO');
  assert.equal(C.familyOf('Gonçalo Castelo-Branco'), 'Castelo-Branco');
  assert.equal(C.pickedLabel([{ title: 'T', authors: ['Ada Lee'], year: '2020' }, { families: ['Wu'], year: '' }]), 'Lee 2020; Wu');
});

test('styles: known keys and journal ids only', () => {
  assert.ok(C.isStyle('apa'));
  assert.ok(C.isStyle('csl:nature-neuroscience'));
  assert.ok(!C.isStyle('csl:'));
  assert.ok(!C.isStyle('csl:../../etc'));
  assert.ok(!C.isStyle(''));
  assert.ok(!C.isStyle(null));
  assert.throws(() => C.format('nope', [], [], {}));
});

// ---- talking to Lunta ----

function fakeFetch(answer) {
  return (url, init) => {
    if (answer === 'hang') return new Promise((_, no) => init.signal.addEventListener('abort', () => no(new Error('aborted'))));
    if (answer === 'down') return Promise.reject(new TypeError('Failed to fetch'));
    return Promise.resolve({ ok: answer.status === 200, status: answer.status, text: () => Promise.resolve(answer.body) });
  };
}

test('requests: answers, Lunta’s errors, no answer, too slow', async () => {
  assert.deepEqual(await C.request('u', { fetch: fakeFetch({ status: 200, body: '{"ok":true}' }) }), { ok: true });
  await assert.rejects(C.request('u', { fetch: fakeFetch({ status: 403, body: '{"error":"Not paired."}' }) }),
    (e) => e.kind === 'http' && e.status === 403 && e.message === 'Not paired.');
  await assert.rejects(C.request('u', { fetch: fakeFetch({ status: 500, body: 'oops' }) }), (e) => e.kind === 'http' && /500/.test(e.message));
  await assert.rejects(C.request('u', { fetch: fakeFetch({ status: 200, body: '<html>' }) }), (e) => e.kind === 'http');
  await assert.rejects(C.request('u', { fetch: fakeFetch('down') }), (e) => e.kind === 'offline' && /Open Lunta on this computer/.test(C.describe(e, 'word')));
  await assert.rejects(C.request('u', { fetch: fakeFetch('hang'), timeout: 30 }), (e) => e.kind === 'timeout' && /didn’t answer in time/.test(C.describe(e)));
  // the body goes as JSON
  let sent = null;
  await C.request('u', { body: { a: 1 }, headers: { 'X-Margins-Token': 'k' }, fetch: (url, init) => { sent = init; return fakeFetch({ status: 200, body: '{}' })(url, init); } });
  assert.equal(sent.method, 'POST');
  assert.equal(sent.headers['Content-Type'], 'application/json');
  assert.equal(sent.headers['X-Margins-Token'], 'k');
  assert.equal(sent.body, '{"a":1}');
});

test('messages say what to do', () => {
  assert.match(C.unknownMessage(1, false), /1 cited paper isn’t in your Lunta library.*Nothing was changed/);
  assert.match(C.unknownMessage(2, true), /Lunta isn’t running.*2 cited papers/);
  assert.equal(C.summary(1, 1, 'APA 7', [], false), '1 citation, 1 reference · APA 7');
  assert.match(C.summary(3, 2, 'APA 7', ['x'], false), /1 from the copy saved in this document/);
});

// ---- a paper's options in a citation ----

test('options: built-in styles write locators, prefixes, suffixes, without the author and in the text', () => {
  const e = (id, f, y) => ({ id, families: [f], year: y, title: 'T ' + id, entry: f + ' (' + y + '). T.' });
  const E = [e('a', 'Smith', '2020'), e('b', 'Lee', '2021'), e('c', 'Wu', '2019')];
  const cits = [['a'], ['a', 'b'], ['b'], ['c'], ['a', 'b', 'c']];
  const op = [{ a: { locator: '12' } }, { a: { prefix: 'see', locator: '3-5', label: 'chapter' }, b: { suffix: 'for a review' } },
    { b: { mode: 'suppress', locator: '4' } }, { c: { mode: 'narrative', locator: '7' } }, { b: { locator: '9' } }];
  const t = (st) => C.format(st, cits, E, {}, op).texts;
  assert.deepEqual(t('apa'), ['(Smith, 2020, p. 12)', '(Lee, 2021 for a review; see Smith, 2020, chaps. 3–5)', '(2021, p. 4)', 'Wu (2019, p. 7)', '(Lee, 2021, p. 9; Smith, 2020; Wu, 2019)']);
  assert.deepEqual(t('chicago').slice(0, 1), ['(Smith 2020, 12)']);
  assert.deepEqual(t('mla').slice(0, 1), ['(Smith 12)']);
  assert.deepEqual(t('vancouver'), ['[1, p. 12]', '[see 1, chaps. 3–5; 2 for a review]', '[2, p. 4]', 'Wu [3, p. 7]', '[1; 2, p. 9; 3]']);
  const nature = C.format('nature', cits, E, {}, op);
  assert.equal(nature.texts[0], '1(p12)');
  assert.deepEqual(nature.superscripts, [true, true, true, false, true], 'a narrative citation isn’t raised');
  // without options: as before
  assert.equal(C.format('apa', cits, E, {}).texts[1], '(Lee, 2021; Smith, 2020)');
});

test('options: the code in titles and named ranges, as old documents and add-ins see it', () => {
  const o = { a: { prefix: 'siehe', suffix: 'für Details – „Zitat“', locator: '3–5', label: 'figure', mode: 'narrative' }, b: {} };
  const code = C.encodeOptions(o);
  assert.match(code, /^[A-Za-z0-9_-]+$/);
  assert.deepEqual(C.decodeOptions(code), { a: o.a });
  assert.equal(C.encodeOptions({ b: {} }), '');
  assert.equal(C.optionsTitle(null), 'Citation');
  assert.deepEqual(C.optionsOfTitle(C.optionsTitle(o)), { a: o.a });
  assert.equal(C.optionsOfTitle('Citation'), null, 'citations from before options');
  assert.equal(C.decodeOptions('!!not a code'), null);
  const long = C.encodeOptions({ a: { suffix: 'x'.repeat(290) }, b: { prefix: 'y'.repeat(290) } });
  const names = C.optionRangeNames(long, 'k');
  assert.ok(names.length > 1 && names.every((n) => n.length <= 256));
  assert.equal(C.optionsCodeOf(names.slice().reverse()), long);
  assert.equal(C.optionsCodeOf(names.slice(1)), '', 'a part missing');
  // the extension's docs-model reads them the same way
  assert.equal(require('../../extension/gdocs/docs-model.js').optionsCode(names), long);
  assert.equal(C.optionsLabel({ prefix: 'see', locator: '12', mode: 'suppress' }), '“see” before · p. 12 · without author');
});

test('converting: CSL-JSON (Zotero, Mendeley) to Lunta’s details and options; the plan and what is left', () => {
  const m = C.cslMeta({ title: 'A <i>Drosophila</i> study <10 Hz', author: [{ family: 'Beethoven', 'non-dropping-particle': 'van', given: 'Ludwig' }, { literal: 'WHO' }],
    issued: { 'date-parts': [['2020', '3']] }, 'container-title': 'Cell', DOI: 'https://doi.org/10.1/X', note: 'PMID: 123456' });
  assert.deepEqual(m, { title: 'A Drosophila study <10 Hz', authors: ['Ludwig van Beethoven', 'WHO'], date: '2020-03', journal: 'Cell', doi: '10.1/X', pmid: '123456' });
  assert.equal(C.cslDate({ raw: 'Spring 2019' }), '2019');
  assert.deepEqual(C.cslOptions({ locator: '7', label: 'figure', prefix: 'see ', suffix: ' and more', 'suppress-author': true }),
    { prefix: 'see', suffix: 'and more', locator: '7', label: 'figure', mode: 'suppress' });
  assert.deepEqual(C.jsonAt('ITEM CSL_CITATION {"a":"}{"} RND1234567890'), { a: '}{' }, 'Zotero’s random tail and braces in strings');
  const found = [
    { text: '(A)', works: [{ meta: { title: 'Alpha', doi: '10.1/a' }, options: { locator: '2' } }, { meta: { title: 'Beta paper', date: '2020' } }] },
    { text: '(B)', works: [{ meta: { title: 'Beta paper', date: '2020' } }] },
    { text: '(C)', works: [{ meta: { title: 'Gamma' } }] },
    { text: '(?)', works: [{ meta: {} }] }
  ];
  const first = C.convertPlan(found);
  assert.equal(first.works.length, 3, 'the same work once');
  const plan = C.convertPlan(found, [{ id: 'a', how: 'doi' }, null, null]);
  assert.equal(C.convertSummary(plan), '4 citations, 3 papers: 1 in your library, 2 to add');
  const res = C.convertResolve(plan, { [plan.works[1].key]: 'b' });
  assert.deepEqual(res.convert, [{ index: 0, ids: ['a', 'b'], options: { a: { locator: '2' } } }, { index: 1, ids: ['b'], options: null }]);
  assert.deepEqual(res.left.map((l) => [l.index, l.reason]), [[2, '“Gamma” isn’t in your library'], [3, 'a work without a title or DOI']]);
  assert.match(C.convertDone(2, res.left, 1), /^Converted 2 citations \(1 paper added to your library\)\. 2 left as they were: “\(C\)”: “Gamma” isn’t in your library/);
});

test('converting: a paper found only by a similar title is listed, and unticked it is added instead', () => {
  const found = [{ text: '(A)', works: [{ meta: { title: 'Spinal cord, part II', authors: ['Jane Smith'], date: '2022' } }] },
    { text: '(B)', works: [{ meta: { title: 'Beta', doi: '10.1/b' } }] }];
  const plan = C.convertPlan(found, [{ id: 'p1', how: 'close' }, { id: 'b', how: 'doi' }]);
  const el = () => { const e = { children: [], innerHTML: '', appendChild(c) { e.children.push(c); return c; } }; return e; };
  const doc = { createElement: el };
  const box = el();
  let asked = null;
  const r = C.convertReview(doc, box, plan, [], (keys) => { asked = keys; }, () => {});
  assert.equal(r.close.length, 1);
  assert.ok(box.children.some((c) => /similar title/.test(c.textContent || '')));
  assert.ok(box.children.some((c) => c.children.some((x) => /Spinal cord, part II/.test(x.textContent || ''))));
  r.go.onclick();
  assert.deepEqual(asked, [], 'ticked: the library’s paper is used');
  r.close[0].cb.checked = false;
  r.go.onclick();
  assert.deepEqual(asked, [plan.works[0].key]);
  // added as its own paper: the citation gets the new one, not the library's companion
  const res = C.convertResolve(plan, { [plan.works[0].key]: 'new' });
  assert.deepEqual(res.convert.map((c) => c.ids), [['new'], ['b']]);
});

test('web pages and blog posts: cited by author and year, Zotero’s web items keep their type and access date', () => {
  // as Lunta answers for the blog post (the entry is the core's APA)
  const apaEntry = 'Karpathy, A. (2019, April 25). A Recipe for Training Neural Networks. Andrej Karpathy blog. https://karpathy.github.io/2019/04/25/recipe/';
  const k = paper('k', ['Karpathy'], 2019, apaEntry);
  const f = C.format('apa', [['k']], [k], {});
  assert.deepEqual(f.texts, ['(Karpathy, 2019)']);
  assert.equal(f.bibliography[0].text, apaEntry);
  // a page without author or date: its title and n.d.
  const docs = Object.assign(paper('d', [], '', '*Configuration*. (n.d.). Example Tool Docs. Retrieved November 21, 2026, from https://tool.example.org/docs/config'), { year: '', title: 'Configuration' });
  assert.deepEqual(C.format('apa', [['d']], [docs], {}).texts, ['(Configuration, n.d.)']);
  // a Zotero "Blog Post" in a document being converted
  const m = C.cslMeta({ type: 'post-weblog', title: 'A Recipe for Training Neural Networks', author: [{ family: 'Karpathy', given: 'Andrej' }],
    issued: { 'date-parts': [[2019, 4, 25]] }, accessed: { 'date-parts': [[2026, 11, 21]] }, 'container-title': 'Andrej Karpathy blog',
    URL: 'https://karpathy.github.io/2019/04/25/recipe/' });
  assert.deepEqual(m, { title: 'A Recipe for Training Neural Networks', authors: ['Andrej Karpathy'], date: '2019-04-25', journal: 'Andrej Karpathy blog',
    url: 'https://karpathy.github.io/2019/04/25/recipe/', work_type: 'post-weblog', accessed: '2026-11-21' });
});
