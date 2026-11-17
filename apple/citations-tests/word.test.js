// Tests for the Word panel (word/app.js) against stand-ins for Office.js, Word, the page and Lunta.
// Run: node --test apple/citations-tests/
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const ROOT = path.join(__dirname, '..', 'citations');

// ---- the page ----
function element(id) {
  const el = {
    id, textContent: '', className: '', disabled: false, hidden: false, value: '', innerHTML: '', children: [], dataset: {},
    classList: { toggle() {} }, focus() {}, addEventListener(type, fn) { el['on' + type] = fn; },
    appendChild(c) { el.children.push(c); return c; },
    querySelector() { return element(); }
  };
  return el;
}

// ---- Word: content controls in document order ----
function makeWord(doc) {
  let nextId = 1;
  const calls = { insertText: 0, insertHtml: 0, selected: null };
  function cc(tag, text) {
    const c = {
      id: nextId++, tag, text, title: '', appearance: '',
      font: { superscript: false, load() {} },
      contentControls: { items: [], load() {} },
      insertText(t) { calls.insertText++; c.text = t; return { font: c.font }; },
      insertHtml(html) { calls.insertHtml++; c.html = html; c.text = html.replace(/<[^>]+>/g, '').replace(/&amp;/g, '&').replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&quot;/g, '"'); },
      select() { calls.selected = c; },
      delete() { doc.ccs.splice(doc.ccs.indexOf(c), 1); }
    };
    return c;
  }
  doc.ccs = doc.ccs || [];
  const collection = (items) => ({ items, load() {} });
  const Word = {
    run(fn) {
      const ctx = {
        sync: () => Promise.resolve(),
        document: {
          contentControls: Object.assign(collection(doc.ccs), {
            getByTag: (t) => collection(doc.ccs.filter((c) => c.tag === t)),
            getById: (id) => doc.ccs.find((c) => c.id === id)
          }),
          getSelection() {
            const inside = doc.cursorIn || null;
            return {
              parentContentControlOrNullObject: inside ? Object.assign(inside, { isNullObject: false, load() {} }) : { isNullObject: true, load() {} },
              getRange() {
                return {
                  insertText(t) {
                    return { insertContentControl() { const c = cc('', t); doc.ccs.splice(doc.cursorAt == null ? doc.ccs.length : doc.cursorAt, 0, c); return c; } };
                  }
                };
              },
              paragraphs: { getLast: () => ({ insertParagraph(t) { return { insertContentControl() { const c = cc('', t); doc.ccs.push(c); return c; } }; } }) }
            };
          }
        }
      };
      return Promise.resolve().then(() => fn(ctx));
    }
  };
  return { Word, cc, calls };
}

// ---- Office: settings and custom XML parts ----
function makeOffice(store) {
  store.parts = store.parts || [];
  store.settings = store.settings || {};
  const ok = (cb, value) => setTimeout(() => cb({ status: 'succeeded', value }), 0);
  const part = (xml) => {
    const p = { xml, getXmlAsync: (cb) => ok(cb, p.xml), deleteAsync: (cb) => { store.parts = store.parts.filter((x) => x !== p); ok(cb); } };
    return p;
  };
  store.parts = store.parts.map((x) => (typeof x === 'string' ? part(x) : x));
  return {
    AsyncResultStatus: { Succeeded: 'succeeded' },
    onReady(cb) { this.ready = cb; },
    context: {
      requirements: { isSetSupported: () => true },
      document: {
        settings: { get: (k) => (k in store.settings ? store.settings[k] : null), set: (k, v) => { store.settings[k] = v; }, saveAsync: (cb) => cb && cb() },
        customXmlParts: {
          getByNamespaceAsync: (ns, cb) => ok(cb, store.parts.filter((p) => p.xml.includes(ns))),
          addAsync: (xml, cb) => { store.parts.push(part(xml)); ok(cb); }
        }
      }
    }
  };
}

// ---- Lunta ----
const LIB = {
  a: { id: 'a', title: 'Alpha', authors: ['Jane Smith'], families: ['Smith'], year: '2020', meta: { title: 'Alpha', authors: ['Jane Smith'], date: '2020' } },
  b: { id: 'b', title: 'Beta', authors: ['Jane Smith'], families: ['Smith'], year: '2020', meta: { title: 'Beta', authors: ['Jane Smith'], date: '2020' } },
  c: { id: 'c', title: 'Gamma <&>', authors: ['Per Wu'], families: ['Wu'], year: '2019', meta: { title: 'Gamma <&>', authors: ['Per Wu'], date: '2019' } }
};
function entry(p, style) { return p.families[0] + ' (' + p.year + '). ' + p.title + '. *' + style + '*.'; }
function makeFetch(lunta) {
  return (url, init) => {
    if (lunta.down) return Promise.reject(new TypeError('Failed to fetch'));
    lunta.requests.push({ url, body: init.body && JSON.parse(init.body) });
    let body;
    if (url.endsWith('/api/status')) body = { ok: true, papers: Object.keys(lunta.lib).length, api: 2 };
    else if (url.endsWith('/api/bibliography')) {
      const j = JSON.parse(init.body);
      body = { entries: j.ids.map((id) => lunta.lib[id] ? Object.assign({}, lunta.lib[id], { entry: entry(lunta.lib[id], j.style) }) : { id, missing: true, entry: '' }) };
    } else return Promise.resolve({ ok: false, status: 404, text: () => Promise.resolve('{"error":"Not found."}') });
    return Promise.resolve({ ok: true, status: 200, text: () => Promise.resolve(JSON.stringify(body)) });
  };
}

/** The panel loaded into a document: {ctx, doc, store, lunta, calls, status()}. */
async function panel(opts) {
  opts = opts || {};
  const doc = { ccs: [], cursorAt: null, cursorIn: null };
  const store = { parts: opts.parts || [], settings: opts.settings || {} };
  const lunta = { lib: Object.assign({}, LIB), down: !!opts.down, requests: [] };
  const w = makeWord(doc);
  (opts.ccs || []).forEach(([tag, text]) => doc.ccs.push(w.cc(tag, text)));
  const els = {};
  const local = {};
  const ctx = {
    Office: makeOffice(store), Word: w.Word, location: { origin: 'https://localhost:47481' },
    fetch: makeFetch(lunta), AbortController, setTimeout, clearTimeout, console: { error() {}, log() {} },
    localStorage: { getItem: (k) => local[k] ?? null, setItem: (k, v) => { local[k] = String(v); } },
    DOMParser: function () { this.parseFromString = (x) => ({ documentElement: { textContent: x.replace(/^<[^>]+>|<\/[^>]+>$/g, '').replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&amp;/g, '&') } }); },
    document: {
      getElementById: (id) => (els[id] = els[id] || element(id)),
      createElement: () => element(), querySelectorAll: () => []
    }
  };
  ctx.window = ctx;
  vm.createContext(ctx);
  for (const f of ['common.js', 'word/app.js']) vm.runInContext(fs.readFileSync(path.join(ROOT, f), 'utf8'), ctx, { filename: f });
  ctx.Office.ready();
  await settle();
  return { ctx, doc, store, lunta, calls: w.calls, cc: w.cc, status: () => els.status.textContent, els };
}
const settle = () => new Promise((r) => setTimeout(r, 20));
const texts = (p) => p.doc.ccs.map((c) => c.text);

test('cite, number, bibliography; Update All twice writes nothing the second time', async () => {
  const p = await panel();
  assert.match(p.status(), /Connected to Lunta · 3 papers/);
  p.ctx.picked = [LIB.c];
  await p.ctx.insertCitation();
  assert.deepEqual(p.doc.ccs.map((c) => c.tag), ['margins:c']);
  assert.deepEqual(texts(p), ['(Wu, 2019)']);
  p.ctx.picked = [LIB.a, LIB.b];
  await p.ctx.insertCitation();
  await p.ctx.insertBibliography();
  assert.deepEqual(texts(p).slice(0, 2), ['(Wu, 2019)', '(Smith, 2020a, 2020b)']);
  const bib = p.doc.ccs[2];
  assert.equal(bib.tag, 'margins-bib');
  assert.match(bib.html, /Smith \(2020a\)\. Alpha\. <i>apa<\/i>/);
  assert.match(bib.html, /Gamma &lt;&amp;&gt;/);
  assert.match(p.status(), /2 citations, 3 references · APA 7/);
  const before = p.calls.insertText + p.calls.insertHtml;
  await p.ctx.updateAll();
  assert.equal(p.calls.insertText + p.calls.insertHtml, before, 'nothing rewritten');
  // the style and the papers are in the document, in one XML part
  assert.equal(p.store.parts.length, 1);
  const saved = JSON.parse(p.store.parts[0].xml.replace(/^<[^>]+>|<\/[^>]+>$/g, '').replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&amp;/g, '&'));
  assert.equal(saved.style, 'apa');
  assert.deepEqual(Object.keys(saved.papers).sort(), ['a', 'b', 'c']);
  assert.equal(p.store.settings.luntaStyle, 'apa');
});

test('numbered styles renumber; the cursor in a citation adds to it', async () => {
  const p = await panel({ settings: { luntaStyle: 'vancouver' }, ccs: [['margins:b', '(…)'], ['margins:a', '(…)'], ['margins:b', '(…)'], ['margins-bib', 'x']] });
  await p.ctx.updateAll();
  assert.deepEqual(texts(p).slice(0, 3), ['[1]', '[2]', '[1]']);
  p.doc.cursorIn = p.doc.ccs[1];
  p.ctx.picked = [LIB.c, LIB.b];
  await p.ctx.insertCitation();
  assert.equal(p.doc.ccs[1].tag, 'margins:a,c,b');
  assert.deepEqual(texts(p).slice(0, 3), ['[1]', '[1–3]', '[1]']);
  assert.equal(p.doc.ccs.length, 4, 'no new content control');
  // in the bibliography: refused
  p.doc.cursorIn = p.doc.ccs[3];
  p.ctx.picked = [LIB.c];
  await p.ctx.insertCitation();
  assert.match(p.status(), /That’s the bibliography/);
  assert.equal(p.doc.ccs.length, 4);
});

test('a deleted citation (empty control) leaves the list; Lunta closed: from the document’s copy', async () => {
  const p = await panel({ settings: { luntaStyle: 'nature' }, ccs: [['margins:a', '(…)'], ['margins:c', '(…)'], ['margins-bib', 'x']] });
  await p.ctx.updateAll();
  assert.deepEqual(texts(p).slice(0, 2), ['1', '2']);
  assert.equal(p.doc.ccs[0].font.superscript, true);
  p.doc.ccs[0].text = '';                  // its text deleted
  p.lunta.down = true;                     // and Lunta quit
  await p.ctx.updateAll();
  assert.equal(p.doc.ccs[1].text, '1');
  assert.match(p.doc.ccs[2].html, /1\. Wu/);
  assert.doesNotMatch(p.doc.ccs[2].html, /Smith/);
  assert.match(p.status(), /Lunta isn’t running: updated from the copy saved in this document · 1 empty citation left out/);
});

test('a paper deleted from the library stays; one with nothing to go on stops everything', async () => {
  const p = await panel({ settings: { luntaStyle: 'apa' }, ccs: [['margins:a', '(…)'], ['margins:c', '(…)'], ['margins-bib', 'x']] });
  await p.ctx.updateAll();
  delete p.lunta.lib.c;
  await p.ctx.updateAll();
  assert.equal(p.doc.ccs[1].text, '(Wu, 2019)');
  assert.match(p.status(), /1 from the copy saved in this document/);
  // a citation of a paper nobody knows: nothing changes, it is selected
  p.doc.ccs.splice(1, 0, p.cc('margins:zzz', '(Old, 1999)'));
  const before = texts(p).join('|') + p.doc.ccs[3].html;
  await p.ctx.updateAll();
  assert.equal(texts(p).join('|') + p.doc.ccs[3].html, before);
  assert.equal(p.calls.selected, p.doc.ccs[1]);
  assert.match(p.status(), /1 cited paper isn’t in your Lunta library.*Nothing was changed/);
});

test('a document whose style wasn’t saved with it isn’t rewritten in another style', async () => {
  const p = await panel({ ccs: [['margins:a', '[1]'], ['margins-bib', '1. Smith']] });
  assert.match(p.status(), /style wasn’t saved/);
  await p.ctx.updateAll();
  assert.deepEqual(texts(p), ['[1]', '1. Smith']);
  assert.match(p.status(), /Choose its style/);
  await p.ctx.setStyle('vancouver');
  assert.equal(texts(p)[0], '[1]');
  assert.match(p.status(), /1 citation, 1 reference · Vancouver/);
  // reopened: the style is read from the document
  const again = await panel({ parts: p.store.parts.map((x) => x.xml), ccs: [['margins:a', '[1]']] });
  assert.equal(again.ctx.style, 'vancouver');
  assert.doesNotMatch(again.status(), /wasn’t saved/);
});

test('clicking a citation shows its papers; ✕ removes one; the last one removes the citation', async () => {
  const p = await panel({ settings: { luntaStyle: 'vancouver' }, ccs: [['margins:a,b', '(…)'], ['margins:c', '(…)'], ['margins-bib', 'x']] });
  await p.ctx.updateAll();
  assert.deepEqual(texts(p).slice(0, 2), ['[1,2]', '[3]']);
  // the cursor in the first citation
  p.doc.cursorIn = p.doc.ccs[0];
  p.ctx.onSelection();
  await new Promise((r) => setTimeout(r, 260));
  assert.equal(p.els.current.hidden, false, 'the citation box shows');
  assert.equal(p.els.citelist.children.length, 2, 'one line per paper');
  assert.equal(p.els.insert.textContent, 'Add to This Citation');
  // remove Smith 2020 (a): the citation keeps b, numbers follow
  await p.ctx.removeFromCitation('a');
  assert.equal(p.doc.ccs[0].tag, 'margins:b');
  assert.deepEqual(texts(p).slice(0, 2), ['[1]', '[2]']);
  // remove its last paper: the citation goes
  p.doc.cursorIn = p.doc.ccs[0];
  await p.ctx.removeFromCitation('b');
  assert.equal(p.doc.ccs.length, 2);
  assert.deepEqual(texts(p).slice(0, 1), ['[1]']);
  assert.equal(p.els.current.hidden, true, 'nothing to show any more');
  // the cursor outside citations: the box goes, the button is Insert Citation again
  p.doc.cursorIn = null;
  p.ctx.onSelection();
  await new Promise((r) => setTimeout(r, 260));
  assert.equal(p.els.insert.textContent, 'Insert Citation');
});
