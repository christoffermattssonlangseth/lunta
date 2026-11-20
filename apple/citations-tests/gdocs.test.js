// Tests for the Google Docs script (gdocs/Code.gs) against a stand-in for Apps Script's DocumentApp.
// Run: node --test apple/citations-tests/
const test = require('node:test');
const assert = require('node:assert/strict');
const { Text, Footnote, P, LI, Body, makeDocument, load, TextAlignment } = require('./fake-docs.js');
const C = require('../citations/common.js');

const LINK = (ids) => C.makeLink(ids);
const BIB = 'https://margins.local/bib';

/** A citation as the add-in writes it: text linked to its ids. */
function cite(text, ids, sup) {
  const t = new Text(text);
  t.setLinkUrl(0, text.length - 1, LINK(ids));
  if (sup) t.setTextAlignment(0, text.length - 1, TextAlignment.SUPERSCRIPT);
  return t;
}
function bibPara(text, list) {
  const p = (list ? LI : P)(text);
  p.editAsText().setLinkUrl(0, text.length - 1, BIB);
  return p;
}
/** The plan the sidebar sends for these texts. */
function plan(gs, texts, opts) {
  const st = gs.call('getState');
  opts = opts || {};
  return {
    keys: st.citations.map((l) => l.join(',')), texts, superscripts: texts.map(() => !!opts.sup),
    bibliography: opts.bib === undefined ? null : opts.bib.map((t) => C.runs(t)), style: opts.style || 'apa', papers: opts.papers || null
  };
}
function bodyText(body) { return body.children.map((c) => c.getText()).join('\n'); }

test('citations in document order, footnotes where their marks are, runs split by formatting joined', () => {
  const t1 = new Text('Intro ');
  const c1 = cite('(A)', ['a']);
  const split = cite('(B, C)', ['b', 'c']);
  split.setBold(0, 2, true);   // the citation is now two runs
  const note = new Footnote([P('See ', cite('(N)', ['n']))]);
  const body = Body(P(t1, c1), P('More ', note, cite('[2]', ['b'])), P(split));
  const gs = load(makeDocument({ body }));
  assert.deepEqual(gs.call('getState').citations, [['a'], ['n'], ['b'], ['b', 'c']]);
  assert.equal(gs.call('version'), 4);
});

test('Update All writes each citation; twice changes nothing', () => {
  const c1 = cite('(…)', ['a']), c2 = cite('(…)', ['b', 'a']);
  const body = Body(P('One ', c1), P(new Text('Two '), c2), bibPara('References will appear here.'));
  const gs = load(makeDocument({ body }));
  const p = plan(gs, ['[1]', '[1,2]'], { bib: ['1. Alpha *Cell*.', '2. Beta.'], style: 'vancouver' });
  const r = gs.call('applyPlan', p);
  assert.deepEqual(r, { changed: false, count: 2, updated: 2, bibliography: true, saved: true });
  assert.equal(c1.getText(), '[1]');
  assert.equal(c1.getLinkUrl(0), LINK(['a']));
  assert.equal(c1.getLinkUrl(2), LINK(['a']));
  assert.equal(bodyText(body), 'One [1]\nTwo [1,2]\n1. Alpha Cell.\n2. Beta.\n');   // the old last paragraph is emptied, not removed
  const bibText = body.children[2].editAsText();
  assert.ok(bibText.isItalic(9) && !bibText.isItalic(3));
  assert.equal(bibText.getLinkUrl(0), BIB);
  // again: nothing rewritten
  const again = gs.call('applyPlan', plan(gs, ['[1]', '[1,2]'], { bib: ['1. Alpha *Cell*.', '2. Beta.'], style: 'vancouver' }));
  assert.deepEqual(again, { changed: false, count: 2, updated: 0, bibliography: false, saved: true });
  assert.equal(gs.docProps.data.luntaStyle, 'vancouver');
});

test('a document edited while Update All worked it out is left alone', () => {
  const c1 = cite('(A)', ['a']), c2 = cite('(B)', ['b']);
  const para = P(c1, new Text(' and '), c2);
  const body = Body(para);
  const gs = load(makeDocument({ body }));
  const p = plan(gs, ['(Lee, 2020)', '(Wu, 2021)']);
  // meanwhile a co-author deletes the first citation
  para.children.splice(0, 1);
  const r = gs.call('applyPlan', p);
  assert.deepEqual(r, { changed: true });
  assert.equal(c2.getText(), '(B)');   // not given A's text
});

test('superscript numbers and back', () => {
  const c1 = cite('(…)', ['a']);
  const body = Body(P('Text', c1));
  const gs = load(makeDocument({ body }));
  gs.call('applyPlan', plan(gs, ['1'], { sup: true, style: 'nature' }));
  assert.equal(c1.getTextAlignment(0), TextAlignment.SUPERSCRIPT);
  gs.call('applyPlan', plan(gs, ['(Lee, 2020)'], { sup: false }));
  assert.equal(c1.getTextAlignment(0), TextAlignment.NORMAL);
  assert.equal(c1.getText(), '(Lee, 2020)');
});

test('the bibliography as the last paragraph, as a list, and twice in a row', () => {
  // last paragraph of the document: Docs refuses to remove it
  const body = Body(P('Text ', cite('(A)', ['a'])), bibPara('Old entry one'), bibPara('Old entry two'));
  const gs = load(makeDocument({ body }));
  gs.call('applyPlan', plan(gs, ['(A, 2020)'], { bib: ['A. (2020). New.'] }));
  assert.deepEqual(body.children.map((c) => c.getText()), ['Text (A, 2020)', 'A. (2020). New.', '']);
  // as list items (someone made it a numbered list), the list kept
  const body2 = Body(P('x ', cite('(A)', ['a'])), bibPara('Old', true), P('After'));
  const gs2 = load(makeDocument({ body: body2 }));
  const r = gs2.call('applyPlan', plan(gs2, ['(A, 2020)'], { bib: ['New one.', 'New two.'] }));
  assert.equal(r.bibliography, true);
  assert.deepEqual(body2.children.map((c) => c.getText()), ['x (A, 2020)', 'New one.', 'New two.', 'After']);
  // Insert Bibliography again: the existing one stays the only one
  assert.deepEqual(gs2.call('insertBibliography', []), { exists: true });
});

test('the bibliography keeps its font and indents', () => {
  const old = bibPara('Old');
  old.setAttributes({ FONT_SIZE: 10, INDENT_START: 36, INDENT_FIRST_LINE: 0 });
  const body = Body(P('x ', cite('(A)', ['a'])), old, P('end'));
  const gs = load(makeDocument({ body }));
  gs.call('applyPlan', plan(gs, ['(A)'], { bib: ['New.'] }));
  assert.deepEqual(body.children[1].getAttributes(), { FONT_SIZE: 10, INDENT_START: 36, INDENT_FIRST_LINE: 0 });
});

test('Insert Bibliography after the cursor’s paragraph; never inside a footnote', () => {
  const here = new Text('Chapter end.');
  const body = Body(P(here), P('Appendix'));
  const doc = makeDocument({ body, cursor: { text: here, offset: 3 } });
  const gs = load(doc);
  assert.deepEqual(gs.call('insertBibliography', []), { created: true });
  assert.deepEqual(body.children.map((c) => c.getText()), ['Chapter end.', 'References will appear here.', 'Appendix']);
  const inNote = new Text('note');
  const note = new Footnote([P(inNote)]);
  const body2 = Body(P('x', note));
  const gs2 = load(makeDocument({ body: body2, cursor: { text: inNote, offset: 1 } }));
  assert.throws(() => gs2.insertBibliography([]), /not a footnote/);
});

test('Insert Citation: at the cursor, added to the citation the cursor is in, not in the bibliography', () => {
  const t = new Text('As shown before.');
  const body = Body(P(t), bibPara('Ref'));
  const doc = makeDocument({ body, cursor: { text: t, offset: 15 } });
  const gs = load(doc);
  assert.deepEqual(gs.call('insertCitation', ['a', 'b'], '(…)', false), { added: true });
  assert.equal(t.getText(), 'As shown before(…).');
  assert.equal(t.getLinkUrl(15), LINK(['a', 'b']));
  assert.equal(t.getLinkUrl(18), null);   // the full stop after it isn't linked
  // cursor inside it: the papers join it
  doc.cursor = { text: t, offset: 16 };
  assert.deepEqual(gs.call('insertCitation', ['c', 'a'], '(…)', false), { merged: true });
  assert.deepEqual(gs.call('getState').citations, [['a', 'b', 'c']]);
  assert.equal(t.getText(), 'As shown before(…).');
  // in the bibliography: refused
  doc.cursor = { text: body.children[1].editAsText(), offset: 1 };
  assert.throws(() => gs.insertCitation(['a'], '(…)', false), /bibliography/);
  // right after a citation: a new one, not merged
  doc.cursor = { text: t, offset: 18 };
  gs.call('insertCitation', ['d'], '(…)', false);
  assert.deepEqual(gs.call('getState').citations, [['a', 'b', 'c'], ['d']]);
});

test('the papers a document keeps survive a round trip, also when large', () => {
  const body = Body(P('x ', cite('(A)', ['a'])));
  const gs = load(makeDocument({ body }));
  const store = {};
  for (let i = 0; i < 400; i++) {
    store['id-' + i] = { m: { title: 'A title about cells number ' + i, authors: ['Ada Lee', 'Per Wu ' + i], date: '2020', journal: 'Cell', doi: '10.1/' + i },
      f: ['Lee', 'Wu'], y: '2020', t: 'A title about cells number ' + i, e: { apa: 'Lee, A., & Wu, P. (2020). A title about cells number ' + i + '. *Cell*.' } };
  }
  const r = gs.call('applyPlan', Object.assign(plan(gs, ['(A)']), { papers: store }));
  assert.equal(r.saved, true);
  const keys = Object.keys(gs.docProps.data).filter((k) => k.startsWith('lunta.store.'));
  assert.ok(keys.length > 2, 'chunked: ' + keys.length);
  assert.deepEqual(gs.call('getState').papers, store);
  // a smaller one later: the extra chunks go
  gs.call('applyPlan', Object.assign(plan(gs, ['(A)']), { papers: { a: store['id-1'] } }));
  assert.deepEqual(gs.call('getState').papers, { a: store['id-1'] });
  assert.equal(Object.keys(gs.docProps.data).filter((k) => k.startsWith('lunta.store.')).length, 2);
});

test('chapters in tabs: cited in tab order, the bibliography in the last tab', () => {
  const ch1 = Body(P('One ', cite('(…)', ['a'])));
  const ch2 = Body(P('Two ', cite('(…)', ['b'])), bibPara('Refs'));
  const gs = load(makeDocument({ tabs: [{ body: ch1 }, { body: ch2 }] }));
  const st = gs.call('getState');
  assert.deepEqual(st.citations, [['a'], ['b']]);
  assert.equal(st.hasBibliography, true);
  gs.call('applyPlan', plan(gs, ['[1]', '[2]'], { bib: ['1. A', '2. B'], style: 'vancouver' }));
  assert.deepEqual(ch2.children.map((c) => c.getText()), ['Two [2]', '1. A', '2. B', '']);
});

test('an older Sidebar.html still works with this script', () => {
  const c1 = cite('(…)', ['a']);
  const body = Body(P('x ', c1), bibPara('Refs'), P('end'));
  const gs = load(makeDocument({ body }));
  assert.deepEqual(gs.call('listCitations'), [['a']]);
  assert.equal(gs.call('applyAll', ['(Lee, 2020)'], false, [[{ text: 'Lee (2020).' }]]), 1);
  assert.equal(c1.getText(), '(Lee, 2020)');
  assert.equal(gs.call('getStyle'), 'apa');
});

test('selecting the citation of a missing paper', () => {
  const body = Body(P('x ', cite('(A)', ['a']), new Text(' y '), cite('(Z)', ['z'])));
  const doc = makeDocument({ body });
  const gs = load(doc);
  assert.equal(gs.call('selectCitation', 'z'), true);
  assert.equal(doc.selected.els[0].el.getText(), '(Z)');
  assert.equal(gs.call('selectCitation', 'nope'), false);
});

test('the style and papers are also kept in named ranges (shared with the browser extension)', () => {
  const body = Body(P('x ', cite('(A)', ['a'])), bibPara('Refs'));
  const doc = makeDocument({ body });
  const gs = load(doc);
  const papers = { a: { m: { title: 'Alpha', authors: ['Ada Lee'], date: '2020' }, f: ['Lee'], y: '2020', t: 'Alpha', e: { 'csl:nature': 'Lee, A. Alpha.' } } };
  gs.call('applyPlan', Object.assign(plan(gs, ['(Lee, 2020)'], { bib: ['Lee (2020).'], style: 'csl:nature' }), { papers, styleTitle: 'Nature' }));
  const names = doc.named.map((n) => n.getName());
  assert.ok(names.length >= 1 && names.every((n) => /^lunta:[a-z0-9]+:\d+:\d+:[A-Za-z0-9_\-=]*$/.test(n) && n.length <= 256), names.join('\n'));
  // the script's own properties gone (another script, or the extension): read from the named ranges
  Object.keys(gs.docProps.data).forEach((k) => delete gs.docProps.data[k]);
  const st = gs.call('getState');
  assert.equal(st.style, 'csl:nature');
  assert.equal(st.styleTitle, 'Nature');
  assert.deepEqual(st.papers, papers);
  // Update All again with nothing new: the ranges are left alone
  const before = names.join();
  gs.call('applyPlan', Object.assign(plan(gs, ['(Lee, 2020)'], { bib: ['Lee (2020).'], style: 'csl:nature' }), { papers, styleTitle: 'Nature' }));
  assert.equal(doc.named.map((n) => n.getName()).join(), before);
  // a style change: one generation replaces the other
  gs.call('setStyle', 'vancouver', '');
  assert.equal(gs.call('getState').style, 'vancouver');
  const gens = new Set(doc.named.map((n) => n.getName().split(':')[1]));
  assert.equal(gens.size, 1);
});

test('the shared store: a half-written generation is ignored, the newest complete one counts', () => {
  const body = Body(P('x ', cite('(A)', ['a'])));
  const doc = makeDocument({ body });
  const gs = load(doc);
  gs.call('applyPlan', Object.assign(plan(gs, ['(A)'], { style: 'harvard' }), { papers: {} }));
  const good = doc.named.map((n) => n.getName());
  // an interrupted write elsewhere: part 0 of 3 only
  doc.addNamedRange('lunta:zzzz:0:3:H4sIAAAA', doc.newRange().build());
  assert.equal(gs.call('getState').style, 'harvard');
  assert.ok(good.length >= 1);
});

// ---- a citation's options, and converting Zotero's citations ----

test('options: named ranges over a citation, kept when Update All rewrites it, set at the cursor', () => {
  const c1 = cite('(…)', ['a']), c2 = cite('(…)', ['b']);
  const body = Body(P(new Text('One '), c1), P(c2));
  const doc = makeDocument({ body });
  const gs = load(doc);
  // the cursor in the second citation
  doc.cursor = { text: c2, offset: 1 };
  const at = gs.call('citationAtCursor');
  assert.deepEqual(at, { index: 1, ids: ['b'], options: '', text: '(…)' });
  const code = C.encodeOptions({ b: { locator: '12', prefix: 'see' } });
  assert.equal(gs.call('setCitationOptions', 1, 'b', C.optionRangeNames(code, 'k1')), true);
  assert.equal(gs.call('setCitationOptions', 1, 'a', []), false, 'a citation that changed meanwhile');
  assert.deepEqual(gs.call('getState').options, ['', code]);
  // Update All rewrites the text; the options move to the new text
  const r = gs.call('applyPlan', plan(gs, ['(Lee, 2020)', '(see Wu, 2021, p. 12)']));
  assert.equal(r.updated, 2);
  assert.equal(c2.getText(), '(see Wu, 2021, p. 12)');
  assert.deepEqual(gs.call('getState').options, ['', code]);
  const nr = doc.named.find((x) => x.getName().indexOf('lunta-o:') === 0);
  const el = nr.getRange().getRangeElements()[0];
  assert.deepEqual([el.getStartOffset(), el.getEndOffsetInclusive()], [0, 20]);
  // cleared
  gs.call('setCitationOptions', 1, 'b', []);
  assert.deepEqual(gs.call('getState').options, ['', '']);
});

const ZKEY = 'Ab12Cd', ZBIB = 'Xy98Zw';
function zoteroDoc() {
  const code = 'ITEM CSL_CITATION ' + JSON.stringify({
    citationID: 'q1w2e3', properties: { formattedCitation: '(Smith, 2020, p. 3)', plainCitation: '(Smith, 2020, p. 3)', noteIndex: 0 },
    citationItems: [{ id: 7, uris: ['http://zotero.org/users/1/items/AAAA1111'], itemData: { id: 7, type: 'article-journal', title: 'Alpha', DOI: '10.1000/alpha',
      author: [{ family: 'Smith', given: 'Jane' }], issued: { 'date-parts': [['2020']] } }, locator: '3', label: 'page' }],
    schema: 'https://github.com/citation-style-language/schema/raw/master/csl-citation.json'
  });
  const z = new Text('(Smith, 2020, p. 3)');
  z.setLinkUrl(0, 18, C.ZOTERO_DOCS_URL + ZKEY);
  const b1 = P('Smith, J. (2020). Alpha.'), b2 = P('Wu, P. (2019). Gamma.');
  [b1, b2].forEach((p) => p.editAsText().setLinkUrl(0, p.getText().length - 1, C.ZOTERO_DOCS_URL + ZBIB));
  const body = Body(P(new Text('Known '), z, new Text('.')), P('References'), b1, b2, P(''));
  const doc = makeDocument({ body });
  // Zotero's field codes: "Z_F<key><000><part>" (255 characters a name)
  for (let i = 0, n = 0; i < code.length; i += 243, n++) doc.addNamedRange('Z_F' + ZKEY + String(n).padStart(3, '0') + code.slice(i, i + 243), doc.newRange().addElement(z, 0, 18).build());
  doc.addNamedRange('Z_F' + ZBIB + '000BIBL {"uncited":[],"omitted":[],"custom":[]} CSL_BIBLIOGRAPHY', doc.newRange().addElement(b1.editAsText(), 0, 5).build());
  return { doc, body, z };
}

test('Zotero in Google Docs: its links and named ranges read, converted to Lunta citations with their options', () => {
  const { doc, body, z } = zoteroDoc();
  const gs = load(doc);
  const scan = gs.call('zoteroScan');
  assert.deepEqual(scan.runs.map((r) => r.key), [ZKEY, ZBIB, ZBIB]);
  const code = C.zoteroDocsCode(C.zoteroDocsJoin(scan.names, ZKEY));
  assert.equal(code.kind, 'cite');
  assert.deepEqual(code.works[0].meta.doi, '10.1000/alpha');
  assert.deepEqual(code.works[0].options, { locator: '3' });
  assert.equal(C.zoteroDocsCode(C.zoteroDocsJoin(scan.names, ZBIB)).kind, 'bib');
  // nothing changes before the conversion is confirmed
  assert.equal(z.getLinkUrl(0), C.ZOTERO_DOCS_URL + ZKEY);
  const names = C.optionRangeNames(C.encodeOptions({ a: code.works[0].options }), 'k2');
  const r = gs.call('zoteroConvert', [{ key: ZKEY, ids: ['a'], names }], [ZBIB], true);
  assert.deepEqual(r, { converted: 1, bibliography: true });
  assert.equal(z.getLinkUrl(0), LINK(['a']));
  assert.equal(body.children[2].editAsText().getLinkUrl(0), BIB, 'Zotero’s bibliography is Lunta’s now');
  const st = gs.call('getState');
  assert.deepEqual(st.citations, [['a']]);
  assert.deepEqual(C.decodeOptions(st.options[0]), { a: { locator: '3' } });
  assert.equal(st.hasBibliography, true);
  assert.ok(!doc.named.some((x) => x.getName().indexOf('Z_F') === 0), 'Zotero’s codes are gone');
});

test('Zotero in Google Docs: with a Lunta bibliography already, Zotero’s goes; unconverted keys stay', () => {
  const { doc, body } = zoteroDoc();
  body.children.push(bibPara('Lunta’s list'));
  const gs = load(doc);
  gs.call('zoteroConvert', [], [ZBIB], false);
  assert.equal(bodyText(body), 'Known (Smith, 2020, p. 3).\nReferences\n\nLunta’s list');
  assert.ok(doc.named.some((x) => x.getName().indexOf('Z_F' + ZKEY) === 0), 'the citation not converted keeps its code');
});
