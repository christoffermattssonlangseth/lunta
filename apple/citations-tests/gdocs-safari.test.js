// Citing in Google Docs from the extension in Safari: the panel puts the placeholder on the
// clipboard (gdocs/clipboard.js), the background script passes the requests to Lunta for Mac
// (POST /gdocs), and the app answers them with the extension's own code (gdocs/app-host.js in
// JavaScriptCore). Here with stand-ins: the Docs API (fake-docs-api.js), the app's native side
// (Docs API with its token, Lunta's citation API, gzip) and the clipboard.
// The Swift side of the app: gdocs-safari-swift.test.js.
// Run: node --test apple/citations-tests/
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const zlib = require('node:zlib');
const C = require('../citations/common.js');
const D = require('../../extension/gdocs/docs-model.js');
const makeLuntaGDocs = require('../../extension/gdocs/flow.js');
const LuntaClipboard = require('../../extension/gdocs/clipboard.js');
const { FakeDocsAPI, P, link } = require('./fake-docs-api.js');

const GDOCS = path.join(__dirname, '..', '..', 'extension', 'gdocs');
const DOC = 'abcdefghijklmnopqrstuvwxyz0123';
const BIB = 'https://margins.local/bib';
const LIB = {
  a: { title: 'Alpha cells', authors: ['Ada Lee'], year: '2020', journal: 'Cell' },
  b: { title: 'Beta waves', authors: ['Per Wu', 'Kim Ng'], year: '2021', journal: 'Neuron' }
};
function entries(body) {
  return { entries: body.ids.map((id) => {
    const p = LIB[id];
    if (!p) return { id, missing: true, entry: '' };
    return { id, title: p.title, authors: p.authors, families: p.authors.map(C.familyOf), year: p.year, journal: p.journal,
      meta: { title: p.title, authors: p.authors, date: p.year, journal: p.journal },
      entry: `${C.familyOf(p.authors[0])} (${p.year}). ${p.title}. *${p.journal}*.` };
  }) };
}

/**
 * Lunta for Mac's JavaScriptCore context, in a vm: the extension's files as the app loads them,
 * and LuntaNative as GoogleDocsScript.swift answers it (the Docs API with the app's token).
 */
function appHost(api, opts) {
  opts = opts || {};
  const calls = { docs: [], lunta: [], gzip: 0 };
  const ctx = { console };
  vm.createContext(ctx);
  const answer = (status, j) => ({ status, json: j });
  ctx.LuntaNative = {
    call(id, name, argsJSON) {
      const args = JSON.parse(argsJSON);
      (async () => {
        switch (name) {
          case 'docs': {
            calls.docs.push(args.method + ' ' + args.url);
            if (opts.signedOut) { const e = new Error('Connect your Google account to cite in this document.'); e.kind = 'auth'; throw e; }
            assert.ok(args.url.startsWith('https://docs.googleapis.com/v1/documents/' + DOC));
            try {
              if (args.method === 'GET') return answer(200, await api.get(DOC));
              return answer(200, await api.batchUpdate(DOC, args.body.requests, args.body.writeControl.requiredRevisionId));
            } catch (e) { return answer(e.status || 500, { error: { message: e.message } }); }
          }
          case 'lunta':
            calls.lunta.push(args.path);
            if (args.path === '/bibliography') return answer(200, entries(args.body));
            return answer(404, { error: 'not found' });
          case 'gzip': calls.gzip++; return zlib.gzipSync(Buffer.from(args, 'utf8')).toString('base64');
          case 'gunzip': return zlib.gunzipSync(Buffer.from(args, 'base64')).toString('utf8');
          case 'sleep': await new Promise((ok) => setTimeout(ok, args)); return null;
          default: throw new Error('unknown ' + name);
        }
      })().then(
        (v) => ctx.LuntaAppHost.settle(id, true, JSON.stringify(v)),
        (e) => ctx.LuntaAppHost.settle(id, false, JSON.stringify({ message: e.message, kind: e.kind || '' })));
    },
    done(id, json) { const w = waiting.get(id); waiting.delete(id); w(JSON.parse(json)); }
  };
  // as JavaScriptCore: no btoa/atob, no CompressionStream, no setTimeout in the scripts' reach
  for (const f of ['citations-common.js', 'docs-model.js', 'flow.js', 'app-host.js']) {
    vm.runInContext(fs.readFileSync(path.join(GDOCS, f), 'utf8'), ctx, { filename: f });
  }
  const waiting = new Map();
  let n = 0;
  const handle = (msg) => new Promise((ok) => { const id = 'r' + (++n); waiting.set(id, ok); ctx.LuntaAppHost.handle(id, JSON.stringify(msg)); });
  return { handle, calls, ctx };
}

test('the app runs the extension’s Google Docs code without browser APIs: insert, Update All, Edit Citation; its store is the extension’s', async () => {
  const api = new FakeDocsAPI({ paragraphs: [P('Some text.'), P(link('Refs', BIB))] });
  const app = appHost(api);
  assert.equal(typeof app.ctx.btoa, 'function');   // app-host.js brings its own
  const st = await app.handle({ op: 'state', docId: DOC });
  assert.equal(st.error, undefined, st.error);
  assert.equal(st.hasBibliography, true);
  // Insert Citation: the placeholder, as the clipboard gets it
  const prep = await app.handle({ op: 'prepareInsert', docId: DOC, ids: ['a'] });
  assert.ok(prep.token && prep.html.includes(prep.token));
  assert.equal(prep.plain, '(…)');
  assert.equal(prep.bare, 0);
  assert.deepEqual(D.parsePlaceholder(prep.url).ids, ['a']);
  // not pasted yet: asked again later
  const early = await app.handle({ op: 'finishInsert', docId: DOC, token: prep.token, bare: prep.bare, timeout: 1000, style: 'apa' });
  assert.equal(early.kind, 'notInserted');
  api.paste(api.indexOf('.'), prep.html);
  const r = await app.handle({ op: 'finishInsert', docId: DOC, token: prep.token, bare: prep.bare, timeout: 1000, style: 'apa' });
  assert.equal(r.error, undefined, r.error);
  assert.deepEqual(api.paragraphs(), ['Some text(Lee, 2020).', 'Lee (2020). Alpha cells. Cell.']);
  assert.deepEqual(app.calls.lunta, ['/bibliography']);
  assert.ok(app.calls.gzip >= 1);
  // the store the app wrote (its gzip) reads in the browser's code, and the other way round
  const g = makeLuntaGDocs({ cite: C, model: D, docs: api, lunta: { bibliography: async (b) => entries(b) }, sleep: async () => {} });
  const store = await g.decodeStore(D.read(await api.get(DOC)).storeNames);
  assert.equal(store.style, 'apa');
  assert.ok(store.papers.a);
  await g.update(DOC, { style: 'vancouver' });
  const u = await app.handle({ op: 'update', docId: DOC });
  assert.equal(u.style, 'vancouver');
  assert.deepEqual(api.paragraphs(), ['Some text[1].', '1. Lee (2020). Alpha cells. Cell.']);
  // Edit Citation and its papers
  const d = await app.handle({ op: 'details', docId: DOC, ids: ['a'] });
  assert.equal(d.entries[0].title, 'Alpha cells');
  const e = await app.handle({ op: 'editCitation', docId: DOC, index: 0, expect: ['a'], ids: ['a', 'b'] });
  assert.equal(e.removed, false);
  assert.deepEqual(api.paragraphs()[0], 'Some text[1,2].');
  // Insert Bibliography when there is one: just an update
  assert.deepEqual(await app.handle({ op: 'prepareInsert', docId: DOC, ids: null }), { exists: true });
  // errors as the panel expects them
  assert.match((await app.handle({ op: 'state', docId: '../x' })).error, /isn’t a Google Docs document/);
  assert.equal((await appHost(api, { signedOut: true }).handle({ op: 'state', docId: DOC })).kind, 'auth');
});

test('pasted as plain text (⌘⇧V, or the link dropped): said so, and a paste after undoing it still works', async () => {
  const api = new FakeDocsAPI({ paragraphs: [P('Already (…) here.')] });
  const app = appHost(api);
  const prep = await app.handle({ op: 'prepareInsert', docId: DOC, ids: ['b'] });
  assert.equal(prep.bare, 1);   // the text "(…)" was there before
  const at = api.indexOf(' here');
  api.type(at, '(…)');
  const r = await app.handle({ op: 'finishInsert', docId: DOC, token: prep.token, bare: prep.bare, timeout: 1000, style: 'apa' });
  assert.equal(r.kind, 'plainPaste');
  assert.match(r.error, /plain text/);
  // undo, then ⌘V
  api.apply({ deleteContentRange: { range: { startIndex: at, endIndex: at + 3 } } });
  api.paste(at, prep.html);
  const ok = await app.handle({ op: 'finishInsert', docId: DOC, token: prep.token, bare: prep.bare, timeout: 1000, style: 'apa' });
  assert.equal(ok.error, undefined, ok.error);
  assert.equal(api.paragraphs()[0], 'Already (…)(Wu & Ng, 2021) here.');
});

// ---- the background script in Safari: requests go to Lunta for Mac ----

function background(opts) {
  const api = new FakeDocsAPI({ paragraphs: [P('Some text.'), P(link('Refs', BIB))] });
  const app = appHost(api);
  const sent = [];
  const account = { configured: opts.configured !== false, connected: false, connecting: false };
  const json = (status, j) => new Response(JSON.stringify(j), { status, headers: { 'Content-Type': 'application/json' } });
  const fetch = async (url, init) => {
    init = init || {};
    if (opts.offline) throw new TypeError('Load failed');
    assert.equal((init.headers || {})['X-Margins-Client'], 'extension');
    if (url === 'http://127.0.0.1:47480/gdocs') {
      if (opts.oldApp) return json(404, { error: 'not found' });
      const m = JSON.parse(init.body);
      sent.push(m.op);
      // as GoogleDocsSafari.answer (apple/Mac/GoogleDocsScript.swift)
      if (m.op === 'status') return json(200, { viaApp: true, configured: account.configured, supported: true, connected: account.connected, connecting: account.connecting, lunta: { papers: 2 } });
      if (!account.configured) return json(200, { error: 'Citing in Google Docs from Safari isn’t set up yet', kind: 'setup' });
      if (m.op === 'connect') { account.connected = true; return json(200, { started: true }); }
      if (!account.connected) return json(200, { error: 'Connect your Google account to cite in this document.', kind: 'auth' });
      return json(200, await app.handle(m));
    }
    if (url.startsWith('http://127.0.0.1:47480/cite/search')) return json(200, [{ id: 'a', title: 'Alpha cells' }]);
    throw new TypeError('fetch failed: ' + url);
  };
  // Safari: chrome.* without identity
  const chrome = { storage: { session: { get: async () => ({}), set: async () => {}, remove: async () => {} } } };
  const ctx = { chrome, fetch, LuntaCite: C, LuntaDocs: D, makeLuntaGDocs, LUNTA_GOOGLE_CLIENT_ID: 'web-client.apps.googleusercontent.com',
    URL, URLSearchParams, AbortController, setTimeout, clearTimeout, console, Blob, Response, CompressionStream, DecompressionStream, btoa, atob };
  if (opts.iOS) ctx.useNative = async () => true;
  vm.createContext(ctx);
  vm.runInContext(fs.readFileSync(path.join(GDOCS, 'background-gdocs.js'), 'utf8'), ctx);
  const ask = async (m) => JSON.parse(JSON.stringify(await ctx.handleGDocs(m)));
  return { ask, sent, api, account };
}

test('Safari on the Mac: the panel’s document requests go to Lunta for Mac, searching to its /cite routes', async () => {
  const b = background({});
  let s = await b.ask({ op: 'status' });
  assert.deepEqual([s.viaApp, s.configured, s.connected], [true, true, false]);
  assert.equal((await b.ask({ op: 'state', docId: DOC })).kind, 'auth');
  assert.deepEqual(await b.ask({ op: 'connect' }), { started: true });
  s = await b.ask({ op: 'status' });
  assert.equal(s.connected, true);
  const prep = await b.ask({ op: 'prepareInsert', docId: DOC, ids: ['a'] });
  assert.equal(prep.plain, '(…)');
  b.api.paste(b.api.indexOf('.'), prep.html);   // the reader's ⌘V
  const r = await b.ask({ op: 'finishInsert', docId: DOC, token: prep.token, bare: prep.bare, timeout: 1000, style: 'apa' });
  assert.equal(r.error, undefined, r.error);
  assert.deepEqual(b.api.paragraphs(), ['Some text(Lee, 2020).', 'Lee (2020). Alpha cells. Cell.']);
  assert.equal((await b.ask({ op: 'search', q: 'alpha' })).hits[0].id, 'a');
  assert.deepEqual(b.sent, ['status', 'state', 'connect', 'status', 'prepareInsert', 'finishInsert']);
});

test('Safari: no Google client in the app yet, Lunta closed, an older Lunta, iPhone and iPad', async () => {
  const notSetUp = background({ configured: false });
  const s = await notSetUp.ask({ op: 'status' });
  assert.deepEqual([s.viaApp, s.configured], [true, false]);
  assert.equal((await notSetUp.ask({ op: 'connect' })).kind, 'setup');
  const closed = await background({ offline: true }).ask({ op: 'status' });
  assert.equal(closed.kind, 'offline');
  assert.match(closed.error, /Open Lunta for Mac/);
  assert.match((await background({ oldApp: true }).ask({ op: 'state', docId: DOC })).error, /Update Lunta/);
  // iPhone and iPad: no app nearby; the panel points to the Apps Script
  const ios = background({ iOS: true });
  const st = await ios.ask({ op: 'status' });
  assert.equal(st.supported, false);
  assert.equal(st.viaApp, undefined);
  assert.deepEqual(ios.sent, []);
});

// ---- the panel's clipboard (Safari) ----

class FakeClipboardItem {
  constructor(parts) { this.parts = parts; }
}
function fakeClipboard() {
  const written = [];
  return {
    written,
    async write(items) {
      for (const it of items) {
        const out = {};
        for (const [type, p] of Object.entries(it.parts)) out[type] = await (await p).text();
        written.push(out);
      }
    }
  };
}

test('Safari’s Insert: the placeholder goes on the clipboard (HTML with the link, and plain text) in the click; nothing when there is nothing to paste', async () => {
  const cb = fakeClipboard();
  const prep = Promise.resolve({ token: 't1', html: D.placeholderHTML('t1', ['a'], '(…)'), plain: '(…)' });
  await LuntaClipboard.copy(cb, FakeClipboardItem, prep);
  assert.equal(cb.written.length, 1);
  assert.equal(D.parsePlaceholder(/href="([^"]*)"/.exec(cb.written[0]['text/html'])[1].replace(/&amp;/g, '&')).token, 't1');
  assert.equal(cb.written[0]['text/plain'], '(…)');
  // a bibliography already there, or an error: nothing copied
  await assert.rejects(LuntaClipboard.copy(cb, FakeClipboardItem, Promise.resolve({ exists: true })));
  await assert.rejects(LuntaClipboard.copy(cb, FakeClipboardItem, Promise.resolve({ error: 'x', kind: 'auth' })));
  await assert.rejects(LuntaClipboard.copy(null, FakeClipboardItem, prep), /clipboard/);
  assert.equal(cb.written.length, 1);
});

test('Safari’s Insert: waiting for the reader’s ⌘V; plain text said once; Cancel; errors; giving up', async () => {
  const answers = (list) => {
    const asked = [];
    return { asked, send: async (m) => { asked.push(m); return list.shift(); } };
  };
  // two "not yet", then pasted
  let a = answers([{ error: 'no', kind: 'notInserted' }, { error: 'no', kind: 'notInserted' }, { summary: 'done', style: 'apa' }]);
  let r = await LuntaClipboard.wait(a.send, { docId: DOC, token: 't', bare: 0, extra: { style: 'apa' } });
  assert.equal(r.summary, 'done');
  assert.equal(a.asked.length, 3);
  assert.deepEqual([a.asked[0].op, a.asked[0].token, a.asked[0].bare, a.asked[0].timeout, a.asked[0].style], ['finishInsert', 't', 0, 4000, 'apa']);
  // plain text: told once, keeps waiting (undo and ⌘V again)
  const plain = [];
  let paused = 0;
  a = answers([{ error: 'plain', kind: 'plainPaste' }, { error: 'plain', kind: 'plainPaste' }, { summary: 'ok' }]);
  r = await LuntaClipboard.wait(a.send, { docId: DOC, token: 't', onPlain: (m) => plain.push(m), pause: async () => { paused++; } });
  assert.deepEqual(plain, ['plain']);
  assert.equal(paused, 2);
  assert.equal(r.summary, 'ok');
  // Cancel
  let cancelled = false;
  a = { send: async () => { cancelled = true; return { error: 'no', kind: 'notInserted' }; } };
  assert.deepEqual(await LuntaClipboard.wait(a.send, { docId: DOC, token: 't', cancelled: () => cancelled }), { cancelled: true });
  // another error: as it is
  a = answers([{ error: 'The Google account connected to Lunta can’t edit this document', kind: 'access' }]);
  await assert.rejects(LuntaClipboard.wait(a.send, { docId: DOC, token: 't' }), (e) => e.kind === 'access');
  // never pasted
  a = { send: async () => ({ error: 'no', kind: 'notInserted' }) };
  await assert.rejects(LuntaClipboard.wait(a.send, { docId: DOC, token: 't', limit: -1 }), /didn’t reach the document/);
});
