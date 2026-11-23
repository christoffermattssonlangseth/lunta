// Tests for the browser extension's Google Docs citations (extension/gdocs) against a stand-in for
// the Google Docs REST API, and for documents shared with the Apps Script (gdocs/Code.gs).
// Run: node --test apple/citations-tests/
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const C = require('../citations/common.js');
const D = require('../../extension/gdocs/docs-model.js');
const makeLuntaGDocs = require('../../extension/gdocs/flow.js');
const { FakeDocsAPI, P, PS, link } = require('./fake-docs-api.js');
const Apps = require('./fake-docs.js');

const CITE = (ids) => C.makeLink(ids);
const BIB = 'https://margins.local/bib';

// A library as Lunta's /api/bibliography answers for it
const LIB = {
  a: { title: 'Alpha cells', authors: ['Ada Lee'], year: '2020', journal: 'Cell' },
  b: { title: 'Beta waves', authors: ['Per Wu', 'Kim Ng'], year: '2021', journal: 'Neuron' },
  c: { title: 'Gamma rays', authors: ['Eva Smith'], year: '2019', journal: 'Nature' },
  d: { title: 'Delta rivers', authors: ['Bo Ek'], year: '2018', journal: 'Science' }
};
function item(id, p) {
  return { id, title: p.title, authors: p.authors, families: p.authors.map(C.familyOf), year: p.year, journal: p.journal,
    meta: { title: p.title, authors: p.authors, date: p.year, journal: p.journal } };
}
function fakeLunta(lib, opts) {
  opts = opts || {};
  const calls = [];
  return {
    calls,
    bibliography: async (body) => {
      calls.push(['bibliography', body]);
      if (opts.offline) { const e = new Error('offline'); e.kind = 'offline'; throw e; }
      return { entries: body.ids.map((id) => {
        const p = lib[id];
        if (!p) return { id, missing: true, entry: '' };
        return Object.assign(item(id, p), { entry: `${C.familyOf(p.authors[0])} (${p.year}). ${p.title}. *${p.journal}*.` });
      }) };
    },
    render: async (body) => {
      calls.push(['render', body]);
      const order = C.numbering(body.citations);
      return {
        title: 'Journal of Tests',
        citations: body.citations.map((l) => ({ text: l.map((id) => order.order[id]).join(','), superscript: true })),
        bibliography: order.list.map((id) => ({ id, text: `${order.order[id]}. ${lib[id].title}.` })),
        items: order.list.map((id) => item(id, lib[id])), missing: []
      };
    }
  };
}
function flow(api, lunta) {
  return makeLuntaGDocs({ cite: C, model: D, docs: api, lunta: lunta || fakeLunta(LIB), sleep: async () => {} });
}

test('the extension uses the same citation code as the add-ins (citations-common.js is a copy of common.js)', () => {
  const a = fs.readFileSync(path.join(__dirname, '..', 'citations', 'common.js'), 'utf8');
  const b = fs.readFileSync(path.join(__dirname, '..', '..', 'extension', 'gdocs', 'citations-common.js'), 'utf8');
  assert.ok(a === b, 'copy apple/citations/common.js to extension/gdocs/citations-common.js');
});

test('Resolve Citations: the extension uses the add-ins’ resolve.js (citations-resolve.js is a copy)', () => {
  const a = fs.readFileSync(path.join(__dirname, '..', 'citations', 'resolve.js'), 'utf8');
  const b = fs.readFileSync(path.join(__dirname, '..', '..', 'extension', 'gdocs', 'citations-resolve.js'), 'utf8');
  assert.ok(a === b, 'copy apple/citations/resolve.js to extension/gdocs/citations-resolve.js');
});

test('reading: citations in order, footnotes where their marks are, split runs joined, tables, the bibliography', () => {
  const api = new FakeDocsAPI({ paragraphs: [
    P('Intro ', link('(A)', CITE(['a'])), ' text', { footnote: [P('See ', link('(N)', CITE(['n'])))] }, ' ', link('(B', CITE(['b', 'c'])), link(', C)', CITE(['b', 'c']), { bold: true })),
    P('Plain ', link('a web link', 'https://example.org')),
    P(link('Lee (2020). Alpha.', BIB)),
    P(link('Wu (2021). Beta.', BIB))
  ] });
  return api.get('doc').then((doc) => {
    const r = D.read(doc);
    assert.deepEqual(r.items.map((x) => x.ids), [['a'], ['n'], ['b', 'c']]);
    assert.equal(r.items[2].text, '(B, C)');
    assert.equal(r.items[1].segmentId, api.footnoteIds()[0]);
    assert.deepEqual(r.bibliography.paragraphs.map((p) => p.text), ['Lee (2020). Alpha.', 'Wu (2021). Beta.']);
    // a table (the stand-in has none: as the API gives it)
    const t = D.read({ revisionId: 'r', body: { content: [{ startIndex: 1, endIndex: 20, table: { tableRows: [{ tableCells: [{ content: [
      { startIndex: 3, endIndex: 9, paragraph: { elements: [{ startIndex: 3, endIndex: 6, textRun: { content: '[1]', textStyle: { link: { url: CITE(['t']) } } } },
        { startIndex: 6, endIndex: 7, textRun: { content: '\n', textStyle: {} } }] } }] }] }] } }] } });
    assert.deepEqual(t.items.map((x) => x.ids), [['t']]);
    assert.equal(t.bibliography, null);
  });
});

test('Update All: numbers, the bibliography, the document’s store; twice changes nothing', async () => {
  const api = new FakeDocsAPI({ paragraphs: [
    P('One ', link('(…)', CITE(['b'])), '.'),
    P('Two ', link('(…)', CITE(['a', 'b'])), '.'),
    PS({ namedStyleType: 'HEADING_1' }, 'References'),
    P(link('References will appear here.', BIB))
  ] });
  const g = flow(api);
  const r = await g.update('doc', { style: 'vancouver' });
  assert.equal(r.count, 2);
  assert.equal(r.references, 2);
  assert.deepEqual(api.paragraphs(), ['One [1].', 'Two [1,2].', 'References', '1. Wu (2021). Beta waves. Neuron.', '2. Lee (2020). Alpha cells. Cell.']);
  // citations: linked, the full stop after them isn't
  const one = api.indexOf('[1]');
  assert.equal(api.linkAt(one), CITE(['b']));
  assert.equal(api.linkAt(one + 2), CITE(['b']));
  assert.equal(api.linkAt(one + 3), null);
  assert.equal(api.styleAt(one).underline, undefined);
  // the bibliography: linked, italics where the entry has them, the heading left alone
  const e1 = api.indexOf('1. Wu');
  assert.equal(api.linkAt(e1), BIB);
  assert.equal(api.styleAt(api.indexOf('Neuron')).italic, true);
  assert.equal(api.styleAt(e1).italic, undefined);
  assert.equal(api.paragraphStyles()[2].namedStyleType, 'HEADING_1');
  assert.equal(api.paragraphStyles()[3].namedStyleType, undefined);
  // the store: style and papers
  const st = await g.state('doc');
  assert.equal(st.style, 'vancouver');
  assert.ok(api.storeNames().length >= 1);
  // again: nothing to write at all
  const before = api.calls.batchUpdate;
  const again = await g.update('doc');
  assert.equal(again.updated, 0);
  assert.equal(again.bibliography, false);
  assert.equal(api.calls.batchUpdate, before);
});

test('a style change rewrites citations and the list; superscripts; a journal style', async () => {
  const api = new FakeDocsAPI({ paragraphs: [P('Text', link('(…)', CITE(['a'])), ' and', link('(…)', CITE(['c']))), P(link('x', BIB))] });
  const g = flow(api);
  await g.update('doc', { style: 'nature' });
  assert.deepEqual(api.paragraphs(), ['Text1 and2', '1. Lee (2020). Alpha cells. Cell.', '2. Smith (2019). Gamma rays. Nature.']);
  assert.equal(api.styleAt(api.indexOf('1')).baselineOffset, 'SUPERSCRIPT');
  await g.update('doc', { style: 'apa' });
  assert.deepEqual(api.paragraphs(), ['Text(Lee, 2020) and(Smith, 2019)', 'Lee (2020). Alpha cells. Cell.', 'Smith (2019). Gamma rays. Nature.']);
  assert.equal(api.styleAt(api.indexOf('(Lee')).baselineOffset, undefined);
  const r = await g.update('doc', { style: 'csl:journal-of-tests', styleTitle: 'Journal of Tests' });
  assert.equal(r.styleTitle, 'Journal of Tests');
  assert.deepEqual(api.paragraphs(), ['Text1 and2', '1. Alpha cells.', '2. Gamma rays.']);
  const st = await g.state('doc');
  assert.deepEqual([st.style, st.styleTitle], ['csl:journal-of-tests', 'Journal of Tests']);
});

test('someone typing while Update All works: it starts over, and no citation gets another’s text', async () => {
  const api = new FakeDocsAPI({ paragraphs: [P(link('(A)', CITE(['a'])), ' and ', link('(B)', CITE(['b'])))] });
  // the first read is followed by a co-author deleting the first citation
  api.onGet = (n) => { if (n === 1) api.apply({ deleteContentRange: { range: { startIndex: 1, endIndex: 9 } } }), api.revision++; };
  const r = await flow(api).update('doc', { style: 'apa' });
  assert.deepEqual(api.paragraphs(), ['(Wu & Ng, 2021)']);
  assert.equal(r.count, 1);
  assert.equal(api.calls.batchUpdate, 2);   // the first was refused (old revision)
  // always changing: gives up with a message, nothing written
  const busy = new FakeDocsAPI({ paragraphs: [P(link('(A)', CITE(['a'])))] });
  busy.onGet = () => { busy.revision++; };
  await assert.rejects(flow(busy).update('doc', { style: 'apa' }), /kept changing/);
  assert.deepEqual(busy.paragraphs(), ['(A)']);
});

test('Insert Citation: the pasted placeholder becomes a citation; inside a citation the papers join it', async () => {
  const api = new FakeDocsAPI({ paragraphs: [P('As shown before.'), P(link('Refs', BIB))] });
  const g = flow(api);
  await g.update('doc', { style: 'vancouver' });
  const tok = D.newToken();
  api.paste(api.indexOf('.'), D.placeholderHTML(tok, ['b', 'a']));
  const r = await g.finishInsert('doc', tok);
  assert.equal(r.count, 1);
  assert.deepEqual(api.paragraphs(), ['As shown before[1,2].', '1. Wu (2021). Beta waves. Neuron.', '2. Lee (2020). Alpha cells. Cell.']);
  const at = api.indexOf('[1,2]');
  assert.equal(api.linkAt(at), CITE(['b', 'a']));
  assert.equal(api.styleAt(at).underline, undefined);
  // the cursor inside it: the new paper joins
  const tok2 = D.newToken();
  api.paste(at + 2, D.placeholderHTML(tok2, ['c', 'a']));
  await g.finishInsert('doc', tok2);
  assert.equal(api.paragraphs()[0], 'As shown before[1–3].');
  assert.equal(api.linkAt(api.indexOf('[1–3]')), CITE(['b', 'a', 'c']));
  // right after a citation: a citation of its own
  const tok3 = D.newToken();
  api.paste(api.indexOf('.'), D.placeholderHTML(tok3, ['d']));
  await g.finishInsert('doc', tok3);
  assert.equal(api.paragraphs()[0], 'As shown before[1–3][4].');
  // a placeholder that never arrives (the paste went nowhere)
  await assert.rejects(g.finishInsert('doc', 'nothere', { timeout: 1, interval: 1 }), /didn’t reach the document/);
});

test('Insert Bibliography: new paragraphs after the cursor’s, in plain body text; once', async () => {
  const api = new FakeDocsAPI({ paragraphs: [P('Cited ', link('(…)', CITE(['a']))), PS({ namedStyleType: 'HEADING_1', bullet: true }, 'References'), P('Appendix')] });
  const g = flow(api);
  await g.update('doc', { style: 'apa' });
  const tok = D.newToken();
  api.paste(api.indexOf('References') + 10, D.placeholderHTML(tok, null));
  const r = await g.finishInsert('doc', tok);
  assert.equal(r.bibliography, true);
  assert.deepEqual(api.paragraphs(), ['Cited (Lee, 2020)', 'References', 'Lee (2020). Alpha cells. Cell.', 'Appendix']);
  const ps = api.paragraphStyles();
  assert.equal(ps[1].namedStyleType, 'HEADING_1');
  assert.equal(ps[2].namedStyleType, 'NORMAL_TEXT');
  assert.equal(ps[2].bullet, undefined);
  assert.equal(api.linkAt(api.indexOf('Lee (2020)')), BIB);
  assert.equal((await g.state('doc')).hasBibliography, true);
  // again: the placeholder goes, the one bibliography stays
  const tok2 = D.newToken();
  api.paste(api.indexOf('Appendix'), D.placeholderHTML(tok2, null));
  await g.finishInsert('doc', tok2);
  assert.deepEqual(api.paragraphs(), ['Cited (Lee, 2020)', 'References', 'Lee (2020). Alpha cells. Cell.', 'Appendix']);
  // in a footnote: not there
  const fn = new FakeDocsAPI({ paragraphs: [P('x', { footnote: [P('note')] })] });
  const tok3 = D.newToken();
  fn.paste(0, D.placeholderHTML(tok3, null), undefined, fn.footnoteIds()[0]);
  const r3 = await flow(fn).finishInsert('doc', tok3, { style: 'apa' });
  assert.match(r3.summary, /main text/);
  assert.deepEqual(fn.paragraphs(), ['x^']);
  assert.equal(fn.text(undefined, fn.footnoteIds()[0]), 'note\n');
});

test('the bibliography as the last paragraph, as a list, with its look kept', async () => {
  const api = new FakeDocsAPI({ paragraphs: [P('x ', link('(A)', CITE(['a', 'c']))),
    PS({ bullet: true, indentStart: 36 }, link('Old one', BIB)), PS({ bullet: true, indentStart: 36 }, link('Old two', BIB)), PS({ bullet: true, indentStart: 36 }, link('Old three', BIB))] });
  await flow(api).update('doc', { style: 'apa' });
  assert.deepEqual(api.paragraphs(), ['x (Lee, 2020; Smith, 2019)', 'Lee (2020). Alpha cells. Cell.', 'Smith (2019). Gamma rays. Nature.']);
  assert.deepEqual(api.paragraphStyles().slice(1), [{ bullet: true, indentStart: 36 }, { bullet: true, indentStart: 36 }]);
});

test('a document without a saved style is never rewritten in a guessed one; a new one starts in APA', async () => {
  const api = new FakeDocsAPI({ paragraphs: [P('x ', link('[1]', CITE(['a'])))] });
  await assert.rejects(flow(api).update('doc'), (e) => e.kind === 'style');
  assert.deepEqual(api.paragraphs(), ['x [1]']);
  assert.equal(api.calls.batchUpdate, 0);
  const fresh = new FakeDocsAPI({ paragraphs: [P('Nothing cited yet.')] });
  const tok = D.newToken();
  fresh.paste(fresh.indexOf('.'), D.placeholderHTML(tok, ['a']));
  await flow(fresh).finishInsert('doc', tok);
  assert.deepEqual(fresh.paragraphs(), ['Nothing cited yet(Lee, 2020).']);
});

test('a paper neither the library nor the document has: nothing changes; the new citation still becomes one', async () => {
  const api = new FakeDocsAPI({ paragraphs: [P('x ', link('(Z)', CITE(['zz'])), ' y')] });
  const g = flow(api);
  const tok = D.newToken();
  api.paste(api.indexOf(' y') + 2, D.placeholderHTML(tok, ['a']));
  await assert.rejects(g.finishInsert('doc', tok, { style: 'apa' }), (e) => e.kind === 'unknown' && /“\(Z\)”/.test(e.message));
  assert.deepEqual(api.paragraphs(), ['x (Z) y(…)']);
  // what the Apps Script would see: two citations
  const r = D.read(await api.get('doc'));
  assert.deepEqual(r.items.map((i) => [i.kind, i.url]), [['cite', CITE(['zz'])], ['cite', CITE(['a'])]]);
});

test('Lunta closed: Update All works from the document’s copy of the papers', async () => {
  const api = new FakeDocsAPI({ paragraphs: [P('x ', link('(…)', CITE(['a']))), P(link('r', BIB))] });
  await flow(api).update('doc', { style: 'apa' });
  // a citation moved before another one meanwhile; Lunta isn't running
  api.type(1, 'See ');
  const r = await flow(api, fakeLunta(LIB, { offline: true })).update('doc');
  assert.match(r.summary, /Lunta isn’t running/);
  assert.deepEqual(api.paragraphs(), ['See x (Lee, 2020)', 'Lee (2020). Alpha cells. Cell.']);
  // a co-author with another library (a lacks there): from the copy, found by its details
  const other = fakeLunta({});
  const r2 = await flow(api, other).update('doc');
  assert.equal(other.calls[0][1].papers.a.title, 'Alpha cells');
  assert.match(r2.summary, /from the copy saved in this document/);
});

test('chapters in tabs: cited in tab order, the bibliography in the tab that has it', async () => {
  const api = new FakeDocsAPI({ tabs: [
    { tabId: 't.1', paragraphs: [P('One ', link('(…)', CITE(['a'])))] },
    { tabId: 't.2', paragraphs: [P('Two ', link('(…)', CITE(['b']))), P(link('Refs', BIB))] }
  ] });
  await flow(api).update('doc', { style: 'ieee' });
  assert.deepEqual(api.paragraphs('t.1'), ['One [1]']);
  assert.deepEqual(api.paragraphs('t.2'), ['Two [2]', '[1] Lee (2020). Alpha cells. Cell.', '[2] Wu (2021). Beta waves. Neuron.']);
  assert.ok(api.storeNames('t.1').length > 0);
});

test('documents shared with the Apps Script: each reads what the other saved', async () => {
  // the extension writes; the script (gdocs/Code.gs) reads the named ranges
  const api = new FakeDocsAPI({ paragraphs: [P('x ', link('(…)', CITE(['a'])))] });
  const g = flow(api);
  await g.update('doc', { style: 'csl:journal-of-tests', styleTitle: 'Journal of Tests' });
  const body = Apps.Body(Apps.P('x'));
  const doc = Apps.makeDocument({ body });
  api.storeNames().forEach((n) => doc.addNamedRange(n, doc.newRange().build()));
  const gs = Apps.load(doc);
  const st = gs.call('getState');
  assert.equal(st.style, 'csl:journal-of-tests');
  assert.equal(st.styleTitle, 'Journal of Tests');
  assert.equal(st.papers.a.t, 'Alpha cells');
  // the script writes; the extension reads
  const t = new Apps.Text('(A)');
  t.setLinkUrl(0, 2, CITE(['b']));
  const doc2 = Apps.makeDocument({ body: Apps.Body(Apps.P('y ', t)) });
  const gs2 = Apps.load(doc2);
  const papers = { b: { m: { title: 'Beta waves', authors: ['Per Wu'] }, f: ['Wu'], y: '2021', t: 'Beta waves', e: { harvard: 'Wu (2021) Beta.' } } };
  gs2.call('applyPlan', { keys: ['b'], texts: ['(Wu, 2021)'], superscripts: [false], bibliography: null, style: 'harvard', papers });
  const store = await g.decodeStore(doc2.named.map((n) => n.getName()));
  assert.equal(store.style, 'harvard');
  assert.deepEqual(store.papers, papers);
});

test('the store: large ones in many ranges, too large without entries; placeholders never confused with citations', async () => {
  const g = flow(new FakeDocsAPI({ paragraphs: [P('x')] }));
  const papers = {};
  for (let i = 0; i < 300; i++) {
    papers['id' + i] = { m: { title: 'Title ' + i + ' ' + Math.random().toString(36), authors: ['A ' + Math.random().toString(36)] }, f: ['A'], y: '2020', t: 'T' + i,
      e: { apa: 'Entry ' + Array.from({ length: 30 }, () => Math.random().toString(36).slice(2)).join(' ') } };
  }
  const names = await g.encodeStore({ v: 1, at: 1, style: 'apa', papers });
  assert.ok(names.length <= D.STORE_MAX && names.every((n) => n.length <= 256));
  const back = await g.decodeStore(names);
  assert.equal(back.style, 'apa');
  assert.deepEqual(back.papers.id7.e, {});   // entries dropped to fit
  assert.equal(back.papers.id7.t, 'T7');
  assert.equal(D.parseLink(D.placeholderUrl('tok', ['a'])), null);
  assert.deepEqual(D.parsePlaceholder(D.placeholderUrl('tok', ['a', 'b,c'])).ids, ['a', 'b', 'c']);
  assert.equal(D.parsePlaceholder('https://margins.local/insert?t=x'), null);
});

test('the background script: Google sign-in, the Docs API over fetch, Lunta’s /cite routes, errors that say what to do', async () => {
  const vm = require('node:vm');
  const api = new FakeDocsAPI({ paragraphs: [P('Some text.'), P(link('Refs', BIB))] });
  const lunta = fakeLunta(LIB);
  const DOC = 'abcdefghijklmnopqrstuvwxyz0123';
  let consented = false, tokenN = 0, valid = new Set(), luntaRoutes = true;
  const flows = [], pickers = [];
  // drive.file: Google opens for Lunta only the documents given to it in the picker
  const given = new Set();
  let pickerAnswer = (doc) => '#picked=' + doc, viewOnly = false;
  const json = (status, j) => new Response(JSON.stringify(j), { status, headers: { 'Content-Type': 'application/json' } });
  const fetch = async (url, init) => {
    init = init || {};
    const h = init.headers || {};
    if (url.startsWith('https://docs.googleapis.com/v1/documents/')) {
      if (!valid.has((h.Authorization || '').replace('Bearer ', ''))) return json(401, { error: { message: 'Invalid Credentials' } });
      const rest = url.slice('https://docs.googleapis.com/v1/documents/'.length);
      if (!given.has(DOC)) return json(404, { error: { message: 'Requested entity was not found.' } });
      if (viewOnly && init.method !== 'GET') return json(403, { error: { message: 'The caller does not have permission' } });
      try {
        if (init.method === 'GET') { assert.equal(rest, DOC + '?includeTabsContent=true'); return json(200, await api.get(DOC)); }
        const b = JSON.parse(init.body);
        assert.equal(rest, DOC + ':batchUpdate');
        return json(200, await api.batchUpdate(DOC, b.requests, b.writeControl.requiredRevisionId));
      } catch (e) { return json(e.status || 500, { error: { message: e.message } }); }
    }
    if (url.startsWith('http://127.0.0.1:47480/cite/')) {
      assert.equal(h['X-Margins-Client'], 'extension');
      if (!luntaRoutes) return json(404, { error: 'not found' });
      const route = url.slice('http://127.0.0.1:47480/cite'.length);
      if (route === '/status') return json(200, { ok: true, papers: 4, api: 2 });
      if (route.startsWith('/search')) return json(200, [item('a', LIB.a)]);
      if (route === '/bibliography') return json(200, await lunta.bibliography(JSON.parse(init.body)));
    }
    if (url.startsWith('https://oauth2.googleapis.com/revoke')) return json(200, {});
    throw new TypeError('fetch failed: ' + url);
  };
  const session = {}, local = {};
  const grants = () => JSON.parse(JSON.stringify(local.grantedDocs || []));   // plain arrays across the vm boundary
  const chrome = {
    identity: {
      getRedirectURL: () => 'https://abcdefghijklmnop.chromiumapp.org/',
      launchWebAuthFlow: async ({ url, interactive }) => {
        if (url.startsWith('https://getlunta.app/picker.html')) {
          // Lunta's picker page: everything in the fragment, nothing in the query
          const u = new URL(url), f = new URLSearchParams(u.hash.slice(1));
          assert.equal(u.search, '');
          assert.equal(interactive, true);
          assert.ok(valid.has(f.get('token')), 'a token Google takes');
          assert.equal(f.get('back'), 'https://abcdefghijklmnop.chromiumapp.org/');
          pickers.push(f.get('doc'));
          const a = pickerAnswer(f.get('doc'));
          if (a === null) throw new Error('The user did not approve access.');
          if (a.startsWith('#picked=') && a.slice(8) === f.get('doc')) given.add(f.get('doc'));
          return f.get('back') + a;
        }
        const q = new URL(url).searchParams;
        flows.push({ interactive, prompt: q.get('prompt') });
        assert.equal(q.get('client_id'), 'test-client.apps.googleusercontent.com');
        assert.equal(q.get('redirect_uri'), 'https://abcdefghijklmnop.chromiumapp.org/');
        assert.equal(q.get('scope'), 'https://www.googleapis.com/auth/drive.file');
        if (!interactive && !consented) throw new Error('User interaction required.');
        consented = true;
        const t = 'tok' + (++tokenN);
        valid.add(t);
        return `https://abcdefghijklmnop.chromiumapp.org/#access_token=${t}&token_type=Bearer&expires_in=3599&scope=${encodeURIComponent(q.get('scope'))}`;
      }
    },
    storage: {
      session: { get: async (k) => ({ [k]: session[k] }), set: async (o) => Object.assign(session, o), remove: async (k) => { delete session[k]; } },
      local: { get: async (k) => ({ [k]: local[k] }), set: async (o) => Object.assign(local, o), remove: async (k) => { delete local[k]; } }
    }
  };
  const ctx = { chrome, fetch, LuntaCite: C, LuntaDocs: D, makeLuntaGDocs, LUNTA_GOOGLE_CLIENT_ID: 'test-client.apps.googleusercontent.com',
    URL, URLSearchParams, AbortController, setTimeout, clearTimeout, console, Blob, Response, CompressionStream, DecompressionStream, btoa, atob };
  vm.createContext(ctx);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '..', '..', 'extension', 'gdocs', 'background-gdocs.js'), 'utf8'), ctx);
  const ask = async (m) => JSON.parse(JSON.stringify(await ctx.handleGDocs(m)));   // plain objects across the vm boundary

  let s = await ask({ op: 'status' });
  assert.deepEqual([s.configured, s.supported, s.connected, s.lunta.papers], [true, true, false, 4]);
  assert.equal((await ask({ op: 'state', docId: DOC })).kind, 'auth');
  assert.deepEqual(await ask({ op: 'connect' }), { connected: true });
  s = await ask({ op: 'status' });
  assert.equal(s.connected, true);
  // a document not given to Lunta yet: the panel offers Google's picker, set to it
  let a = await ask({ op: 'state', docId: DOC });
  assert.equal(a.kind, 'access');
  assert.match(a.error, /Give Lunta access to this document/);
  // closed without choosing, cancelled, another document: still not given, said so
  pickerAnswer = () => null;
  assert.equal((await ask({ op: 'grant', docId: DOC })).kind, 'access');
  pickerAnswer = () => '#error=cancelled';
  assert.match((await ask({ op: 'grant', docId: DOC })).error, /without choosing/);
  pickerAnswer = () => '#picked=zzzzzzzzzzzzzzzzzzzzzzzzzz';
  assert.match((await ask({ op: 'grant', docId: DOC })).error, /wasn’t this document/);
  pickerAnswer = () => '#error=setup';
  assert.equal((await ask({ op: 'grant', docId: DOC })).kind, 'setup');
  assert.deepEqual(grants(), []);
  assert.equal((await ask({ op: 'grant', docId: '../x' })).kind, 'http');
  // chosen: remembered, and the same request works
  pickerAnswer = (doc) => '#picked=' + doc;
  assert.deepEqual(await ask({ op: 'grant', docId: DOC }), { granted: true });
  assert.deepEqual(pickers, [DOC, DOC, DOC, DOC, DOC]);
  assert.deepEqual(grants(), [DOC]);
  a = await ask({ op: 'state', docId: DOC });
  assert.equal(a.error, undefined, a.error);
  // insert a citation the way the panel does
  const prep = await ask({ op: 'prepareInsert', docId: DOC, ids: ['a'] });
  assert.ok(prep.token && prep.html.includes(prep.token));
  assert.equal(D.parsePlaceholder(prep.url).ids[0], 'a');
  api.paste(api.indexOf('.'), prep.html);
  const r = await ask({ op: 'finishInsert', docId: DOC, token: prep.token, style: 'apa' });
  assert.equal(r.error, undefined, r.error);
  assert.deepEqual(api.paragraphs(), ['Some text(Lee, 2020).', 'Lee (2020). Alpha cells. Cell.']);
  // a bibliography exists: Insert Bibliography just updates
  assert.deepEqual(await ask({ op: 'prepareInsert', docId: DOC, ids: null }), { exists: true });
  // the token runs out: a new one without a window
  valid.clear();
  const u = await ask({ op: 'update', docId: DOC });
  assert.equal(u.error, undefined, u.error);
  assert.equal(flows[flows.length - 1].interactive, false);
  // another account: Google's account picker
  await ask({ op: 'connect', selectAccount: true });
  assert.equal(flows[flows.length - 1].prompt, 'select_account');
  // the other account's documents are not this one's: given again
  assert.deepEqual(grants(), []);
  // a document Lunta may read but this account may only view: no picker, said so
  viewOnly = true;
  assert.equal((await ask({ op: 'update', docId: DOC, style: 'vancouver' })).kind, 'readonly');
  viewOnly = false;
  // a document opened before that Google no longer opens for Lunta: said so, and forgotten
  assert.equal((await ask({ op: 'state', docId: DOC })).error, undefined);
  assert.deepEqual(grants(), [DOC]);
  given.clear();
  a = await ask({ op: 'state', docId: DOC });
  assert.equal(a.kind, 'access');
  assert.match(a.error, /had this document before/);
  assert.deepEqual(grants(), []);
  a = await ask({ op: 'state', docId: DOC });
  assert.match(a.error, /can’t open this document yet/);
  given.add(DOC);
  // an older Lunta; not a document id
  luntaRoutes = false;
  assert.match((await ask({ op: 'search', q: 'alpha' })).error, /Update Lunta/);
  assert.match((await ask({ op: 'state', docId: '../x' })).error, /isn’t a Google Docs document/);
  // disconnect: forgotten, the documents given too
  assert.equal((await ask({ op: 'state', docId: DOC })).error, undefined);
  assert.deepEqual(grants(), [DOC]);
  await ask({ op: 'disconnect' });
  assert.deepEqual(grants(), []);
  consented = false;
  assert.equal((await ask({ op: 'status' })).connected, false);
});

test('Edit Citation: the papers of one citation; remove one, add some, remove the last; a citation that moved is left alone', async () => {
  const api = new FakeDocsAPI({ paragraphs: [
    P('One ', link('(…)', CITE(['a', 'b'])), '. Two ', link('(…)', CITE(['c'])), '.'),
    P(link('Refs', BIB))
  ] });
  const g = flow(api);
  await g.update('doc', { style: 'vancouver' });
  assert.deepEqual(api.paragraphs()[0], 'One [1,2]. Two [3].');
  const st = await g.state('doc');
  assert.deepEqual(st.texts, ['[1,2]', '[3]']);
  // its papers, for the panel
  const d = await g.details('doc', ['a', 'b']);
  assert.deepEqual(d.entries.map((e) => e.title), ['Alpha cells', 'Beta waves']);
  // remove Lee (a): the numbers follow
  let r = await g.editCitation('doc', { index: 0, expect: ['a', 'b'], ids: ['b'] });
  assert.equal(r.removed, false);
  assert.equal(api.paragraphs()[0], 'One [1]. Two [2].');
  assert.equal(api.linkAt(api.indexOf('[1]')), CITE(['b']));
  // add c and d to the first
  await g.editCitation('doc', { index: 0, expect: ['b'], ids: ['b', 'c', 'd'] });
  assert.equal(api.paragraphs()[0], 'One [1–3]. Two [2].');
  // the panel's view is stale (the citation has other papers now): nothing is changed
  await assert.rejects(g.editCitation('doc', { index: 0, expect: ['b'], ids: [] }), /changed or moved/);
  assert.equal(api.paragraphs()[0], 'One [1–3]. Two [2].');
  // remove the last paper of the second: the citation goes, the bibliography follows
  r = await g.editCitation('doc', { index: 1, expect: ['c'], ids: [] });
  assert.equal(r.removed, true);
  assert.equal(api.paragraphs()[0], 'One [1–3]. Two.');
  assert.equal((await g.state('doc')).citations.length, 1);
  // Lunta closed: the papers from the document's copy
  const g2 = flow(api, fakeLunta(LIB, { offline: true }));
  const d2 = await g2.details('doc', ['b']);
  assert.equal(d2.entries[0].title, 'Beta waves');
});

// ---- Resolve Citations ----

// Lunta's placeholders, roughly (the real ones: core/src/placeholders, tested there)
function placeholdersIn(text) {
  const out = [];
  for (const m of text.matchAll(/[([{]([^()[\]{}]*)[)\]}]/g)) {
    const cites = m[1].split(';').map((part) => {
      const c = /^\s*([A-Z][\w-]*)( et al\.)?,? (\d{4})\s*(.*)$/.exec(part);
      return c && { text: part.trim(), surnames: [c[1]], etAl: !!c[2], year: c[3], hints: [], key: '' };
    });
    if (cites.every(Boolean)) out.push({ start: m.index, end: m.index + m[0].length, text: m[0], kind: 'parentheses', cites });
  }
  return out;
}

test('Resolve Citations in Google Docs: placeholders in the plain text only; each written when confirmed, where it is now', async () => {
  const api = new FakeDocsAPI({ paragraphs: [
    P('Cells (Lee 2020) differ ', link('(Wu & Ng, 2021)', CITE(['b'])), ' and (Smith 2019; Nobody 1999).', { footnote: [P('See (Ek 2018).')] }),
    P('A link (Lee 2020) is ', link('a web page (Ek 2018)', 'https://example.org'), '.'),
    P(link('References', BIB))
  ] });
  const lunta = fakeLunta(LIB);
  lunta.placeholders = async (body) => { lunta.calls.push(['placeholders', body]); return { placeholders: body.texts.map(placeholdersIn) }; };
  const g = flow(api, lunta);
  await g.update('doc', { style: 'apa' });
  const before = api.calls.batchUpdate;
  const r = await g.op({ op: 'scanPlaceholders', docId: 'abcdefghijklmnopqrstuvwxyz' });
  assert.equal(api.calls.batchUpdate, before, 'scanning writes nothing');
  const found = r.placeholders;
  // not Lunta's citation "(Wu & Ng, 2021)", nor the bibliography, nor a web link's text; the footnote where its mark is
  assert.deepEqual(found.map((p) => p.text), ['(Lee 2020)', '(Smith 2019; Nobody 1999)', '(Ek 2018)', '(Lee 2020)']);
  assert.equal(found[0].start, api.indexOf('(Lee 2020)'));
  assert.equal(found[2].segmentId, api.footnoteIds()[0]);
  assert.equal(found[2].start, api.indexOf('(Ek 2018)', undefined, found[2].segmentId));
  // confirmed: the citation where the placeholder was, the bibliography updated
  let w = await g.writePlaceholder('doc', { placeholder: found[0], ids: ['a'] });
  assert.match(w.summary, /2 citations/);
  assert.equal(api.paragraphs()[0], 'Cells (Lee, 2020) differ (Wu & Ng, 2021) and (Smith 2019; Nobody 1999).^');
  assert.equal(api.linkAt(api.indexOf('(Lee, 2020)')), CITE(['a']));
  assert.ok(api.paragraphs().some((t) => t.startsWith('Lee (2020). Alpha cells.')));
  // someone types before the next one meanwhile: found where it is now; the skipped part stays, unlinked
  api.type(1, 'So. ');
  w = await g.writePlaceholder('doc', { placeholder: found[1], ids: ['c'], leftover: '(Nobody 1999)' });
  assert.equal(api.paragraphs()[0], 'So. Cells (Lee, 2020) differ (Wu & Ng, 2021) and (Smith, 2019) (Nobody 1999).^');
  assert.equal(api.linkAt(api.indexOf('(Smith, 2019)')), CITE(['c']));
  assert.equal(api.linkAt(api.indexOf('(Nobody 1999)')), null);
  // in a footnote
  await g.writePlaceholder('doc', { placeholder: found[2], ids: ['d'] });
  assert.equal(api.text(undefined, api.footnoteIds()[0]).trim(), 'See (Ek, 2018).');
  // gone meanwhile: said so, nothing written
  const n = api.calls.batchUpdate;
  await assert.rejects(g.writePlaceholder('doc', { placeholder: { text: '(Gone 2001)', start: 3, end: 14 }, ids: ['a'] }), (e) => e.kind === 'moved');
  assert.equal(api.calls.batchUpdate, n);
  assert.match(api.paragraphs().join('\n'), /Ek \(2018\)\. Delta rivers/);
});

// ---- a citation's options, and converting Zotero's citations ----

test('options: set with Edit Citation, kept in named ranges across rewrites, read by the Apps Script too', async () => {
  const api = new FakeDocsAPI({ paragraphs: [P('One ', link('(…)', CITE(['a'])), ' and ', link('(…)', CITE(['b', 'c'])), '.'), P(link('Refs', BIB))] });
  const g = flow(api);
  await g.update('doc', { style: 'apa' });
  assert.equal(api.paragraphs()[0], 'One (Lee, 2020) and (Smith, 2019; Wu & Ng, 2021).');
  await g.editCitation('doc', { index: 1, expect: ['b', 'c'], ids: ['b', 'c'], options: C.encodeOptions({ b: { locator: '4', prefix: 'see' }, c: { mode: 'suppress' } }) });
  assert.equal(api.paragraphs()[0], 'One (Lee, 2020) and (2019; see Wu & Ng, 2021, p. 4).');
  let st = await g.state('doc');
  assert.deepEqual(C.decodeOptions(st.options[1]), { b: { prefix: 'see', locator: '4' }, c: { mode: 'suppress' } });
  // the first citation gets longer: the second's options stay on its text
  await g.editCitation('doc', { index: 0, expect: ['a'], ids: ['a'], options: C.encodeOptions({ a: { mode: 'narrative' } }) });
  assert.equal(api.paragraphs()[0], 'One Lee (2020) and (2019; see Wu & Ng, 2021, p. 4).');
  await g.update('doc', { style: 'vancouver' });
  assert.equal(api.paragraphs()[0], 'One Lee [1] and [see 2, p. 4; 3].');
  st = await g.state('doc');
  assert.equal(st.options.filter(Boolean).length, 2);
  const names = api.tab('t.0').named.filter((n) => n.name.startsWith('lunta-o:'));
  const second = names.find((n) => C.decodeOptions(C.optionsCodeOf([n.name])).b);
  assert.equal(api.text().slice(second.range.startIndex - 1, second.range.endIndex - 1), '[see 2, p. 4; 3]');
  // a paper removed: its options go with it
  await g.editCitation('doc', { index: 1, expect: ['b', 'c'], ids: ['b'] });
  st = await g.state('doc');
  assert.deepEqual(C.decodeOptions(st.options[1]), { b: { prefix: 'see', locator: '4' } });
});

test('Zotero in Google Docs: read from its links and named ranges, converted after the review, its bibliography made Lunta’s', async () => {
  const ZURL = C.ZOTERO_DOCS_URL;
  const api = new FakeDocsAPI({ paragraphs: [
    P('Known ', link('(Lee, 2020, p. 3)', ZURL + 'Ab12Cd'), ' and new ', link('(Okafor, 2018)', ZURL + 'Qq77Rr'), ' and ', link('(Lee, 2020, p. 3)', ZURL + 'Ab12Cd'), '.'),
    P('References'), P(link('Lee, A. (2020). Alpha cells.', ZURL + 'Bb00Bb')), P(link('Okafor, N. (2018). Epsilon.', ZURL + 'Bb00Bb'))
  ] });
  const t = api.tab('t.0');
  const code = (items) => 'ITEM CSL_CITATION ' + JSON.stringify({ citationID: 'x', properties: { noteIndex: 0 }, citationItems: items, schema: 'https://github.com/citation-style-language/schema/raw/master/csl-citation.json' });
  const put = (key, c) => { for (let i = 0, n = 0; i < c.length; i += 243, n++) t.named.push({ id: 'z' + key + n, name: 'Z_F' + key + String(n).padStart(3, '0') + c.slice(i, i + 243), range: { startIndex: 7, endIndex: 9 } }); };
  put('Ab12Cd', code([{ id: 1, itemData: { title: 'Alpha cells', author: [{ family: 'Lee', given: 'Ada' }], issued: { 'date-parts': [['2020']] } }, locator: '3', label: 'page' }]));
  put('Qq77Rr', code([{ id: 2, itemData: { title: 'Epsilon waves', author: [{ family: 'Okafor', given: 'Ngozi' }], issued: { 'date-parts': [['2018']] }, DOI: '10.5555/eps' } }]));
  put('Bb00Bb', 'BIBL {"uncited":[],"omitted":[],"custom":[]} CSL_BIBLIOGRAPHY');
  const g = flow(api);
  const z = await g.scanZotero('doc');
  assert.deepEqual(z.cites.map((c) => c.key), ['Ab12Cd', 'Qq77Rr']);
  assert.deepEqual(z.bibs, ['Bb00Bb']);
  assert.deepEqual(z.cites[0].works[0].options, { locator: '3' });
  assert.equal(api.calls.batchUpdate, 0, 'nothing written by reading');
  // Epsilon wasn't added: its citation stays, and so does Zotero's bibliography
  await g.convertZotero('doc', { style: 'apa', cites: [{ key: 'Ab12Cd', ids: ['a'], options: C.encodeOptions({ a: { locator: '3' } }) }], bibs: [] });
  assert.equal(api.paragraphs()[0], 'Known (Lee, 2020, p. 3) and new (Okafor, 2018) and (Lee, 2020, p. 3).');
  assert.equal(api.linkAt(api.indexOf('(Lee')), CITE(['a']));
  assert.equal(api.linkAt(api.indexOf('(Okafor')), ZURL + 'Qq77Rr');
  assert.ok(!t.named.some((n) => n.name.startsWith('Z_FAb12Cd')), 'the converted code is gone');
  assert.ok(t.named.some((n) => n.name.startsWith('Z_FQq77Rr')), 'the other stays');
  // then Epsilon too (added meanwhile as d), with the bibliography: Zotero's becomes Lunta's
  const r = await g.convertZotero('doc', { cites: [{ key: 'Qq77Rr', ids: ['d'] }], bibs: ['Bb00Bb'] });
  assert.equal(r.converted, 1);
  assert.equal(api.paragraphs()[0], 'Known (Lee, 2020, p. 3) and new (Ek, 2018) and (Lee, 2020, p. 3).');
  assert.deepEqual(api.paragraphs().slice(2), ['Ek (2018). Delta rivers. Science.', 'Lee (2020). Alpha cells. Cell.']);
  assert.ok(!t.named.some((n) => n.name.startsWith('Z_F')));
});
