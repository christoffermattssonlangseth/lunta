// Lunta for Mac's side of Google Docs citing in Safari, in Swift: apple/Mac/GoogleDocsScript.swift
// (the requests POST /gdocs gets, the extension's code in JavaScriptCore, gzip) compiled with a
// test driver (swift/GoogleDocsDriver.swift) and run against a stand-in for the Docs API served
// here. Needs the Swift compiler (macOS with Xcode); skipped elsewhere.
// Run: node --test apple/citations-tests/
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const http = require('node:http');
const readline = require('node:readline');
const { spawn, spawnSync } = require('node:child_process');
const C = require('../citations/common.js');
const D = require('../../extension/gdocs/docs-model.js');
const makeLuntaGDocs = require('../../extension/gdocs/flow.js');
const { FakeDocsAPI, P, link } = require('./fake-docs-api.js');

const ROOT = path.join(__dirname, '..', '..');
const GDOCS = path.join(ROOT, 'extension', 'gdocs');
const DOC = 'abcdefghijklmnopqrstuvwxyz0123';
const BIB = 'https://margins.local/bib';
const LIB = { a: { title: 'Alpha cells', authors: ['Ada Lee'], year: '2020', journal: 'Cell' } };
function entries(body) {
  return { entries: body.ids.map((id) => {
    const p = LIB[id];
    if (!p) return { id, missing: true, entry: '' };
    return { id, title: p.title, authors: p.authors, families: p.authors.map(C.familyOf), year: p.year, journal: p.journal,
      meta: { title: p.title, authors: p.authors, date: p.year, journal: p.journal }, entry: `${C.familyOf(p.authors[0])} (${p.year}). ${p.title}. *${p.journal}*.` };
  }) };
}

const swiftc = process.platform === 'darwin' && spawnSync('xcrun', ['--find', 'swiftc'], { encoding: 'utf8' }).status === 0;

test('Lunta for Mac answers Safari’s Google Docs requests with the extension’s code in JavaScriptCore', { skip: !swiftc && 'no Swift compiler', timeout: 300000 }, async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'lunta-gdocs-swift-'));
  const bin = path.join(dir, 'driver');
  const built = spawnSync('xcrun', ['swiftc', '-Onone', '-parse-as-library', '-o', bin,
    path.join(ROOT, 'apple', 'Mac', 'GoogleDocsScript.swift'), path.join(__dirname, 'swift', 'GoogleDocsDriver.swift')], { encoding: 'utf8' });
  assert.equal(built.status, 0, built.stderr);

  const api = new FakeDocsAPI({ paragraphs: [P('Some text.'), P(link('Refs', BIB))] });
  const server = http.createServer(async (req, res) => {
    let body = '';
    for await (const c of req) body += c;
    const send = (status, j) => { res.writeHead(status, { 'Content-Type': 'application/json' }); res.end(JSON.stringify(j)); };
    try {
      if (req.url === '/docs/' + DOC + '?includeTabsContent=true') return send(200, await api.get(DOC));
      if (req.url === '/docs/' + DOC + ':batchUpdate') {
        const b = JSON.parse(body);
        try { return send(200, await api.batchUpdate(DOC, b.requests, b.writeControl.requiredRevisionId)); } catch (e) { return send(e.status || 500, { error: { message: e.message } }); }
      }
      if (req.url === '/docs/' + 'nopenopenopenopenopenope' + '?includeTabsContent=true') return send(403, { error: { message: 'The caller does not have permission' } });
      if (req.url === '/lunta/bibliography') return send(200, entries(JSON.parse(body)));
      send(404, { error: 'not found' });
    } catch (e) { send(500, { error: String(e) }); }
  });
  await new Promise((ok) => server.listen(0, '127.0.0.1', ok));
  const base = 'http://127.0.0.1:' + server.address().port;

  const run = (folder) => {
    const p = spawn(bin, [folder, base], { stdio: ['pipe', 'pipe', 'inherit'] });
    const lines = readline.createInterface({ input: p.stdout });
    const queue = [];
    lines.on('line', (l) => queue.shift()(JSON.parse(l)));
    return {
      ask: (m) => new Promise((ok) => { queue.push(ok); p.stdin.write(JSON.stringify(m) + '\n'); }),
      end: () => new Promise((ok) => { p.on('exit', ok); p.stdin.end(); })
    };
  };
  const app = run(GDOCS);
  try {
    let s = await app.ask({ op: 'status' });
    assert.deepEqual([s.viaApp, s.configured, s.supported, s.connected, s.lunta.papers], [true, true, true, false, 2]);
    assert.equal((await app.ask({ op: 'state', docId: DOC })).kind, 'auth');
    assert.deepEqual(await app.ask({ op: 'connect' }), { started: true });
    assert.equal((await app.ask({ op: 'status' })).connected, true);

    // a document written in the browser (its store gzipped by CompressionStream) reads in the app
    const g = makeLuntaGDocs({ cite: C, model: D, docs: api, lunta: { bibliography: async (b) => entries(b) }, sleep: async () => {} });
    await g.update(DOC, { style: 'harvard' });
    const st = await app.ask({ op: 'state', docId: DOC });
    assert.equal(st.error, undefined, st.error);
    assert.equal(st.style, 'harvard');

    // Insert Citation as Safari does it: the placeholder, the reader's ⌘V, then finished
    const prep = await app.ask({ op: 'prepareInsert', docId: DOC, ids: ['a'] });
    assert.ok(prep.token && prep.html.includes(prep.token));
    assert.equal(prep.plain, '(…)');
    const early = await app.ask({ op: 'finishInsert', docId: DOC, token: prep.token, bare: prep.bare, timeout: 1000 });
    assert.equal(early.kind, 'notInserted');
    api.paste(api.indexOf('.'), prep.html);
    const r = await app.ask({ op: 'finishInsert', docId: DOC, token: prep.token, bare: prep.bare, timeout: 1000 });
    assert.equal(r.error, undefined, r.error);
    assert.match(api.paragraphs()[0], /^Some text.*Lee.*2020.*\.$/);
    // the app's store (gzipped in Swift) reads in the browser
    const store = await g.decodeStore(D.read(await api.get(DOC)).storeNames);
    assert.equal(store.style, 'harvard');
    assert.ok(store.papers.a);

    // Update All in another style, Edit Citation, its papers
    const u = await app.ask({ op: 'update', docId: DOC, style: 'vancouver' });
    assert.equal(u.error, undefined, u.error);
    assert.deepEqual(api.paragraphs(), ['Some text[1].', '1. Lee (2020). Alpha cells. Cell.']);
    assert.equal((await app.ask({ op: 'details', docId: DOC, ids: ['a'] })).entries[0].title, 'Alpha cells');
    const e = await app.ask({ op: 'editCitation', docId: DOC, index: 0, expect: ['a'], ids: [] });
    assert.equal(e.removed, true);
    assert.equal(api.paragraphs()[0], 'Some text.');

    // errors as the panel expects them
    assert.equal((await app.ask({ op: 'state', docId: 'nopenopenopenopenopenope' })).kind, 'access');
    assert.match((await app.ask({ op: 'state', docId: '../x' })).error, /isn’t a Google Docs document/);
    assert.equal((await app.ask({ op: 'nonsense' })).error, 'unknown request');
    await app.ask({ _set: { configured: false } });
    assert.equal((await app.ask({ op: 'state', docId: DOC })).kind, 'setup');
    assert.equal((await app.ask({ op: 'connect' })).kind, 'setup');
    assert.equal((await app.ask({ op: 'status' })).configured, false);
  } finally {
    await app.end();
  }

  // the extension's code missing from the app
  const broken = run(dir);
  try {
    await broken.ask({ _set: { connected: true } });
    assert.match((await broken.ask({ op: 'state', docId: DOC })).error, /missing|Reinstall/);
  } finally {
    await broken.end();
    server.close();
    fs.rmSync(dir, { recursive: true, force: true });
  }
});
