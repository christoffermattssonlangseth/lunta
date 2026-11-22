// Check Citations in the panels (common.js): the citations' places in their paragraphs, the
// requests to Lunta's /claims, the results list, "Looks right" / "Dismiss", and the extension's
// reading of a Google Doc (docs-model.js claimParagraphs).
// Run: node --test apple/citations-tests/
const test = require('node:test');
const assert = require('node:assert/strict');
const C = require('../citations/common.js');
const D = require('../../extension/gdocs/docs-model.js');

// ---- a page, as small as claimsReview needs ----
function makeDoc() {
  function el(tag) {
    const e = {
      tag, className: '', textContent: '', children: [], href: '', title: '', open: false, onclick: null,
      appendChild(c) { e.children.push(c); return c; },
      set innerHTML(v) { if (v === '') e.children = []; },
      get innerHTML() { return ''; }
    };
    return e;
  }
  return { createElement: el };
}
function all(e, out = []) { out.push(e); e.children.forEach((c) => all(c, out)); return out; }
const byClass = (root, cls) => all(root).filter((e) => (' ' + e.className + ' ').includes(' ' + cls + ' '));
const text = (e) => all(e).map((x) => x.textContent).join(' ');

function memory() { const m = {}; return { getItem: (k) => (k in m ? m[k] : null), setItem: (k, v) => { m[k] = String(v); } }; }

test('the citations in their paragraphs: places, the others, the paragraph before', () => {
  const sites = C.claimSites([
    { text: 'Microglia are immune cells.', cites: [] },
    { text: 'Rims have iron (Kukanja 2024). Remyelination is early (Lee 2020; Wu 2018).', cites: [{ text: '(Kukanja 2024)', ids: ['a'] }, { text: '(Lee 2020; Wu 2018)', ids: ['b', 'c', 'b'] }] },
    { text: 'Text where the citation was removed.', cites: [{ text: '(gone)', ids: ['x'] }] },
    { text: 'Again (Kukanja 2024) and again (Kukanja 2024).', cites: [{ text: '(Kukanja 2024)', ids: ['a'] }, { text: '(Kukanja 2024)', ids: ['a'] }] }
  ]);
  assert.equal(sites.length, 4);
  const [a, b] = sites;
  assert.deepEqual([a.start, a.end, a.ids, a.before], [15, 29, ['a'], 'Microglia are immune cells.']);
  assert.equal(a.text.slice(a.start, a.end), '(Kukanja 2024)');
  assert.deepEqual(a.others, [[b.start, b.end]]);
  assert.deepEqual(b.ids, ['b', 'c']);
  assert.equal(b.text.slice(b.start, b.end), '(Lee 2020; Wu 2018)');
  // the same text twice: each found after the one before
  assert.ok(sites[3].start > sites[2].start && sites[2].text === sites[3].text);
  assert.equal(sites[2].before, 'Text where the citation was removed.');
  // what is sent: the paragraph, the place, the papers' details the document keeps
  const body = C.claimBody(a, { a: { m: { title: 'Rims', doi: '10.1/x' } } });
  assert.deepEqual(body, { text: a.text, start: 15, end: 29, others: a.others, before: a.before, ids: ['a'], papers: { a: { title: 'Rims', doi: '10.1/x' } }, judge: true });
  assert.equal(C.claimBody(a, {}, false).judge, false);
});

// what Lunta's /claims answers (the core's checks are tested in core/src/claims/tests.rs)
function answer(claim, results, judged) {
  return { claim, sentence: claim, context: '', judged: !!judged, note: '', privacy: C.CLAIM_PRIVACY, results };
}
const passage = (t, s) => ({ text: t, section: 'Results', score: s, link: 'lunta://open?id=p1&find=' + encodeURIComponent(t.split(' ').slice(0, 4).join(' ')) });

test('checking one citation after another, offline stops, other errors are kept', async () => {
  const sites = C.claimSites([{ text: 'A (x). B (y). C (z).', cites: [{ text: '(x)', ids: ['1'] }, { text: '(y)', ids: ['2'] }, { text: '(z)', ids: ['3'] }] }]);
  const asked = [], seen = [];
  const items = await C.runClaims(sites, (body) => {
    asked.push(body.ids[0]);
    if (body.ids[0] === '2') return Promise.reject(Object.assign(new Error('Lunta answered 500.'), { kind: 'http' }));
    return Promise.resolve(answer('S' + body.ids[0], []));
  }, (i, n) => seen.push(i + '/' + n), null, {});
  assert.deepEqual(asked, ['1', '2', '3']);
  assert.deepEqual(seen, ['0/3', '1/3', '2/3']);
  assert.deepEqual(items.map((x) => [x.answer && x.answer.claim, x.error]), [['S1', null], [null, 'Lunta answered 500.'], ['S3', null]]);
  // Stop
  let n = 0;
  const some = await C.runClaims(sites, () => Promise.resolve(answer('S', [])), null, () => n++ >= 1, {});
  assert.equal(some.length, 1);
  // Lunta closed: the whole check fails with the message to open it
  await assert.rejects(C.runClaims(sites, () => Promise.reject(Object.assign(new Error('offline'), { kind: 'offline' })), null, null, {}), /offline/);
});

test('verdicts, counts and the summary line', () => {
  assert.deepEqual(C.claimVerdict({ verdict: 'notFound', byModel: false }), { cls: 'none', name: 'No matching passage', key: 'noMatch' });
  assert.equal(C.claimVerdict({ verdict: 'notFound', byModel: true }).name, 'Not found in this paper');
  assert.equal(C.claimVerdict({ verdict: 'partly', byModel: true, verdictLabel: 'Partly supported' }).cls, 'part');
  const site = (ids) => ({ ids, text: 't', start: 0, end: 1, cite: '(x)' });
  const items = [
    { site: site(['a']), answer: answer('One.', [{ verdict: 'supported', byModel: true }], true) },
    { site: site(['b', 'c']), answer: answer('Two.', [{ verdict: 'partly', byModel: true }, { verdict: 'contradicted', byModel: true }], true) },
    { site: site(['d']), answer: answer('Three.', [{ verdict: 'notFound', byModel: false }]) },
    { site: site(['e']), answer: null, error: 'x' }
  ];
  const c = C.claimCounts(items, { [C.claimKey(['a'], 'One.')]: 'right' });
  assert.deepEqual([c.checked, c.papers, c.judged, c.right, c.failed], [3, 4, true, 1, 1]);
  assert.equal(C.claimSummary(c), '3 citations checked (4 papers): 1 supported, 1 partly supported, 1 contradicted, 1 no matching passage · 1 marked Looks right, 1 couldn’t be checked');
  assert.equal(C.claimKey(['b', 'a', 'b'], ' Two  words. '), 'b,a|Two words.');
});

test('the lunta:// link to a passage', () => {
  assert.deepEqual(C.parseOpenLink('lunta://open?id=p-1&find=chronic%20active%20lesions'), { id: 'p-1', find: 'chronic active lesions' });
  assert.deepEqual(C.parseOpenLink('lunta://open?id=p-1'), { id: 'p-1', find: '' });
  assert.equal(C.parseOpenLink('https://evil.example/?id=x'), null);
});

test('the results list: badges, quotes and passages open the paper, Looks right and Dismiss are remembered', () => {
  C.useStorage(memory());
  const doc = makeDoc(), box = doc.createElement('div');
  const items = [
    { site: { ids: ['p1'], text: 'Rims (K 2024).', start: 5, end: 13, cite: '(K 2024)' },
      answer: answer('Rims have iron.', [{ id: 'p1', label: 'Kukanja et al. 2024', title: 'Iron rims', verdict: 'supported', verdictLabel: 'Supported', byModel: true,
        reason: 'Stated.', note: '', evidence: [{ quote: 'a rim of iron-laden microglia', section: 'Results', link: 'lunta://open?id=p1&find=a%20rim' }],
        passages: [passage('We found a rim of iron-laden microglia.', 0.8)] }], true) },
    { site: { ids: ['p2'], text: 'Old (L 2020).', start: 4, end: 12, cite: '(L 2020)' },
      answer: answer('Old lesions.', [{ id: 'p2', label: 'Lee 2020', title: 'Lesions', verdict: 'contradicted', verdictLabel: 'Contradicted', byModel: true,
        reason: 'Opposite.', note: '', evidence: [{ quote: 'old lesions never remyelinate', section: 'Discussion', link: 'lunta://open?id=p2&find=old' }], passages: [] }], true) }
  ];
  const opened = [];
  let changes = 0;
  const r = C.claimsReview(doc, box, items, { open: (l) => opened.push(l), changed: () => changes++ });
  // the one to look at first
  assert.deepEqual(r.rows.map((x) => x.item.site.ids[0]), ['p2', 'p1']);
  assert.match(text(box), /2 citations checked: 1 supported, 1 contradicted\./);
  assert.match(text(box), /Lunta proposes, you decide/);
  assert.match(text(box), /nothing is sent anywhere/);
  assert.doesNotMatch(text(box), /No on-device model/);
  const badges = byClass(box, 'cl-badge').map((b) => [b.textContent, b.className]);
  assert.deepEqual(badges, [['Contradicted', 'cl-badge cl-bad'], ['Supported', 'cl-badge cl-ok']]);
  assert.match(text(box), /Suggested by the on-device model: Opposite\./);
  const quotes = byClass(box, 'cl-q');
  assert.equal(quotes[1].textContent, '“a rim of iron-laden microglia” — Results');
  quotes[1].onclick({ preventDefault() {} });
  byClass(box, 'cl-p')[0].onclick({ preventDefault() {} });
  assert.deepEqual(opened, ['lunta://open?id=p1&find=a%20rim', items[0].answer.results[0].passages[0].link]);
  assert.match(text(byClass(box, 'cl-p')[0]), /Results · 80 % word overlap/);
  // Looks right: remembered, the row redrawn with Undo
  byClass(box, 'cl-ok').find((b) => b.tag === 'button').onclick();
  assert.equal(changes, 1);
  assert.equal(C.claimDecisions()[C.claimKey(['p2'], 'Old lesions.')], 'right');
  assert.match(text(box), /✓ Looks right/);
  assert.match(text(box), /1 marked Looks right/);
  // Dismiss the other: its results are folded away
  byClass(box, 'cl-no')[0].onclick();
  assert.equal(C.claimDecisions()[C.claimKey(['p1'], 'Rims have iron.')], 'dismissed');
  assert.equal(byClass(box, 'cl-badge').length, 1);
  // only those not decided
  C.claimsReview(doc, box, items, { onlyOpen: true });
  assert.equal(byClass(box, 'cl-item').length, 0);
  // Undo
  C.claimsReview(doc, box, items, {});
  byClass(box, 'cl-undo')[0].onclick();
  assert.equal(Object.keys(C.claimDecisions()).length, 1);
  C.useStorage(null);
});

test('without a model the passages are open and it is said', () => {
  C.useStorage(memory());
  const doc = makeDoc(), box = doc.createElement('div');
  const items = [{ site: { ids: ['p1'], text: 'x', start: 0, end: 1, cite: '[1]' },
    answer: answer('Rims have iron.', [{ id: 'p1', label: 'K 2024', verdict: 'possible', verdictLabel: 'Possible support', byModel: false, reason: '', evidence: [],
      note: 'Only the abstract is in your library (no reading view of the full text here), so the paper may say more than was checked.',
      passages: [passage('Rims of iron.', 0.5), passage('Iron in rims of lesions.', 0.42)] }]) }];
  C.claimsReview(doc, box, items, {});
  assert.match(text(box), /No on-device model in Lunta here/);
  assert.match(text(box), /Possible support/);
  assert.match(text(box), /Only the abstract/);
  const det = byClass(box, 'cl-ps')[0];
  assert.equal(det.open, true);
  assert.match(text(det), /The 2 best-matching passages/);
  assert.match(text(det), /Results · 42 % word overlap/);
  C.useStorage(null);
});

test('the extension reads a Google Doc’s paragraphs and citations', () => {
  const link = (ids) => ({ link: { url: D.makeLink(ids) } });
  const run = (content, style) => ({ textRun: { content, textStyle: style || {} } });
  const doc = {
    body: { content: [
      { paragraph: { elements: [run('Microglia are immune cells.\n')] } },
      { paragraph: { elements: [run('Rims have iron '), run('(Kukanja', link(['a'])), run(' 2024)', link(['a'])), run('.'), { footnoteReference: { footnoteId: 'f1' } }, run(' More (Lee 2020).\n', {})] } },
      { table: { tableRows: [{ tableCells: [{ content: [{ paragraph: { elements: [run('In a table '), run('[2]', link(['b'])), run('.\n')] } }] }] }] } }
    ] },
    footnotes: { f1: { content: [{ paragraph: { elements: [run('See '), run('(Wu 2018)', link(['c'])), run('.\n')] } }] } }
  };
  const paras = D.claimParagraphs(doc);
  assert.deepEqual(paras, [
    { text: 'Microglia are immune cells.', cites: [] },
    { text: 'Rims have iron (Kukanja 2024). More (Lee 2020).', cites: [{ text: '(Kukanja 2024)', ids: ['a'] }] },
    { text: 'See (Wu 2018).', cites: [{ text: '(Wu 2018)', ids: ['c'] }] },
    { text: 'In a table [2].', cites: [{ text: '[2]', ids: ['b'] }] }
  ]);
  const sites = C.claimSites(paras);
  assert.deepEqual(sites.map((s) => s.ids[0]), ['a', 'c', 'b']);
  assert.equal(sites[0].before, 'Microglia are immune cells.');
});
