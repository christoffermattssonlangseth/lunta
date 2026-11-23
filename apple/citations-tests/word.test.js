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
    id, textContent: '', className: '', disabled: false, hidden: false, value: '', children: [], dataset: {},
    // emptied, as the page's: its children go
    html: '', get innerHTML() { return el.html; }, set innerHTML(v) { el.html = v; if (!v) el.children = []; },
    classList: { toggle() {} }, focus() {}, addEventListener(type, fn) { el['on' + type] = fn; },
    appendChild(c) { el.children.push(c); return c; },
    // the same element for the same selector, so a row's texts can be read
    querySelector(sel) { el.found = el.found || {}; return (el.found[sel] = el.found[sel] || element()); }
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
      // as Word: null for a mix of raised and not
      font: {
        load() {},
        get superscript() { const v = c.runs.map((r) => r.font.superscript); return v.length && v.some((x) => x !== v[0]) ? null : (v.length ? v[0] : false); },
        set superscript(v) { c.runs.forEach((r) => { r.font.superscript = v; }); }
      },
      contentControls: { items: [], load() {} },
      delete(keepContent) {
        // keepContent: the control goes, its text (with its runs' fonts) stays where it was
        if (keepContent) {
          (doc.unwrapped = doc.unwrapped || []).push({ tag: c.tag, text: c.text, runs: c.runs, html: c.html });
          if (doc.paras) doc.paras.forEach((para) => { const i = para.indexOf(c); if (i >= 0) para.splice(i, 1, c.text); });
        }
        if (doc.nodes) removeNode(c); else if (doc.ccs.indexOf(c) >= 0) doc.ccs.splice(doc.ccs.indexOf(c), 1);
      },
      // text written outside the control, before and after it (Range "Whole" insertText Before/After)
      outside: { before: '', after: '', runs: [] },
      getRange(where) {
        return {
          insertText(t, loc) {
            calls.insertText++;
            const run = { text: t, font: { superscript: null } };
            if (loc === 'Before') c.outside.before = t + c.outside.before; else c.outside.after += t;
            c.outside.runs.push(run);
            return run;
          },
          select(mode) { calls.cursorAfter = where === 'After' ? c : null; calls.selectMode = mode; }
        };
      },
      // runs: what was written, each with its own font ("see " not raised, "4" raised)
      insertText(t, where) {
        calls.insertText++;
        if (where === 'Start' || where === 'End') {
          const run = { text: t, font: { superscript: false } };
          if (where === 'Start') { c.text = t + c.text; c.runs.unshift(run); } else { c.text += t; c.runs.push(run); }
          return run;
        }
        const run = { text: t, font: { superscript: false } };
        c.text = t; c.runs = [run];
        return run;
      },
      runs: [],
      insertHtml(html) {
        calls.insertHtml++; c.html = html; c.text = html.replace(/<[^>]+>/g, '').replace(/&amp;/g, '&').replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&quot;/g, '"');
        // as Word does: the HTML's paragraphs in its own font and spacing, the last one keeping
        // the control's old paragraph mark (the theme's font, the old spacing)
        const n = (html.match(/<p[ >]/g) || []).length;
        c.paras = Array.from({ length: n }, (_, i) => i < n - 1 || n === 1
          ? { font: { name: 'Times New Roman', size: 12, color: '#000000' }, lineSpacing: 12, spaceAfter: 6, spaceBefore: 0, leftIndent: 0, firstLineIndent: 0, alignment: 'Left' }
          : { font: { name: 'Aptos', size: 11, color: '#000000' }, lineSpacing: 13, spaceAfter: 8, spaceBefore: 0, leftIndent: 0, firstLineIndent: 0, alignment: 'Left' });
      },
      paras: [],
      get paragraphs() {
        return { items: c.paras, load() {}, getFirst: () => ({ font: doc.bodyFont || { name: null, size: null, color: null }, load() {} }) };
      },
      select() { calls.selected = c; }
    };
    return c;
  }
  doc.ccs = doc.ccs || [];
  const collection = (items) => ({ items, load() {} });
  // the body as paragraphs of text and content controls (doc.paras: [[string | cc]]), for Resolve Citations
  const segText = (seg) => (typeof seg === 'string' ? seg : seg.text);
  const rebuild = () => { doc.ccs = [].concat(...doc.paras.map((p) => p.filter((seg) => typeof seg !== 'string'))); };
  function plainRange(para, si, i, t) {
    let after = '';
    return {
      text: t, parentContentControlOrNullObject: { isNullObject: true, load() {} },
      insertText(x, where) {
        if (where === 'After') { after += x; return {}; }
        return {
          insertContentControl() {
            const seg = para[si], c = cc('', x);
            para.splice(si, 1, seg.slice(0, i), c, after + seg.slice(i + t.length));
            rebuild();
            return c;
          }
        };
      }
    };
  }
  // fields (Zotero, EndNote, Mendeley) among the content controls: doc.nodes [{kind: 'field', code} | cc]
  const refresh = () => { if (doc.nodes) doc.ccs.splice(0, doc.ccs.length, ...doc.nodes.filter((n) => n.kind !== 'field')); };
  function removeNode(n) {
    doc.nodes = doc.nodes.filter((x) => x !== n && x.parent !== n);
    refresh();
  }
  doc.field = (code, parent) => {
    const f = { kind: 'field', code, parent, select() { doc.selectedField = f; }, delete() { calls.fieldsDeleted = (calls.fieldsDeleted || 0) + 1; removeNode(f); } };
    return f;
  };
  const body = {
    getOoxml() { return { value: doc.ooxml || '' }; },
    get fields() { return collection((doc.nodes || []).filter((n) => n.kind === 'field')); },
    get paragraphs() { return collection((doc.paras || []).map((p) => ({ text: p.map(segText).join(''), contentControls: collection(p.filter((seg) => typeof seg !== 'string')) }))); },
    search(t) {
      const items = [];
      (doc.paras || []).forEach((para) => para.forEach((seg, si) => {
        if (typeof seg !== 'string') {
          if (seg.text.includes(t)) items.push({ text: t, parentContentControlOrNullObject: Object.assign(seg, { isNullObject: false, load() {} }) });
          return;
        }
        for (let i = seg.indexOf(t); i >= 0; i = seg.indexOf(t, i + t.length)) items.push(plainRange(para, si, i, t));
      }));
      return collection(items);
    }
  };
  const Word = {
    run(fn) {
      const ctx = {
        sync: () => Promise.resolve(),
        document: {
          body,
          contentControls: Object.assign(collection(doc.ccs), {
            getByTag: (t) => collection(doc.ccs.filter((c) => c.tag === t)),
            getById: (id) => doc.ccs.find((c) => c.id === id)
          }),
          getSelection() {
            if (doc.selectedField) {
              const f = doc.selectedField;
              doc.selectedField = null;
              return {
                insertText(t) {
                  return { insertContentControl() { const c = cc('', t); doc.nodes.splice(doc.nodes.indexOf(f), 1, c); removeNode(f); return c; } };
                }
              };
            }
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
        settings: { get: (k) => (k in store.settings ? store.settings[k] : null), set: (k, v) => { store.settings[k] = v; }, remove: (k) => { delete store.settings[k]; }, saveAsync: (cb) => cb && cb() },
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
  a: { id: 'a', title: 'Alpha', authors: ['Jane Smith'], families: ['Smith'], year: '2020', meta: { title: 'Alpha', authors: ['Jane Smith'], date: '2020', doi: '10.1000/alpha' } },
  b: { id: 'b', title: 'Beta', authors: ['Jane Smith'], families: ['Smith'], year: '2020', meta: { title: 'Beta', authors: ['Jane Smith'], date: '2020' } },
  c: { id: 'c', title: 'Gamma <&>', authors: ['Per Wu'], families: ['Wu'], year: '2019', meta: { title: 'Gamma <&>', authors: ['Per Wu'], date: '2019' } }
};
function entry(p, style) { return p.families[0] + ' (' + p.year + '). ' + p.title + '. *' + style + '*.'; }
function makeFetch(lunta) {
  return (url, init) => {
    if (lunta.down) return Promise.reject(new TypeError('Failed to fetch'));
    lunta.requests.push({ url, body: init.body && JSON.parse(init.body) });
    let body;
    if (url.endsWith('/api/status')) body = { ok: true, papers: Object.keys(lunta.lib).length, api: 5 };
    else if (url.endsWith('/api/claims')) {
      // the sentence and a result per paper, roughly (the real ones: core/src/claims, tested there)
      const j = JSON.parse(init.body);
      const claim = (j.text.slice(0, j.start) + j.text.slice(j.end)).replace(/\s+\./g, '.').trim();
      body = { claim, sentence: j.text, context: j.before, judged: false, note: '', privacy: 'p', results: j.ids.map((id) => ({ id, label: id, title: (lunta.lib[id] || {}).title || '',
        source: 'fullText', verdict: 'possible', verdictLabel: 'Possible support', byModel: false, reason: '', note: '', evidence: [],
        passages: [{ text: 'A passage of ' + id + '.', section: 'Results', score: 0.5, link: 'lunta://open?id=' + id + '&find=A%20passage' }] })) };
    }
    else if (url.endsWith('/api/open')) body = { ok: true };
    else if (url.endsWith('/api/render') && lunta.render) body = lunta.render(JSON.parse(init.body));
    else if (url.endsWith('/api/match')) {
      // by DOI or title, roughly as the core does (core/src/citeimport.rs, tested there)
      body = { matches: JSON.parse(init.body).items.map((m) => {
        const p = Object.values(lunta.lib).find((x) => (m.doi && x.meta.doi && x.meta.doi.toLowerCase() === m.doi.toLowerCase()) || (m.title && x.title.toLowerCase() === m.title.toLowerCase()));
        return p ? { id: p.id, how: 'title' } : null;
      }) };
    }
    else if (url.endsWith('/api/placeholders')) body = { placeholders: JSON.parse(init.body).texts.map(findPlaceholders) };
    else if (url.endsWith('/api/suggest')) body = suggest(lunta, JSON.parse(init.body));
    else if (url.endsWith('/api/add')) {
      const m = JSON.parse(init.body).meta, id = 'n' + (Object.keys(lunta.lib).length + 1);
      const fam = m.authors.map((a) => a.split(' ').pop());
      lunta.lib[id] = { id, title: m.title, authors: m.authors, families: fam, year: m.date.slice(0, 4), meta: m };
      body = Object.assign({ added: true }, lunta.lib[id]);
    }
    else if (url.endsWith('/api/bibliography')) {
      const j = JSON.parse(init.body);
      body = { entries: j.ids.map((id) => lunta.lib[id] ? Object.assign({}, lunta.lib[id], { entry: entry(lunta.lib[id], j.style) }) : { id, missing: true, entry: '' }) };
    } else return Promise.resolve({ ok: false, status: 404, text: () => Promise.resolve('{"error":"Not found."}') });
    return Promise.resolve({ ok: true, status: 200, text: () => Promise.resolve(JSON.stringify(body)) });
  };
}

// Lunta's placeholders and suggestions, roughly (the real ones: core/src/placeholders, tested there)
function findPlaceholders(text) {
  const out = [];
  for (const m of text.matchAll(/[([{]([^()[\]{}]*)[)\]}]/g)) {
    const cites = m[1].split(';').map((part) => {
      const c = /^\s*([A-Z][\w-]*)( et al\.)?,? (\d{4})\s*(.*)$/.exec(part);
      return c && { text: part.trim(), surnames: [c[1]], etAl: !!c[2], year: c[3], hints: c[4] ? c[4].toLowerCase().split(/\s+/) : [], key: '' };
    });
    if (cites.every(Boolean)) out.push({ start: m.index, end: m.index + m[0].length, text: m[0], kind: 'parentheses', cites });
  }
  return out;
}
function suggest(lunta, j) {
  const lib = Object.values(lunta.lib);
  const mine = (p) => Object.assign({}, p, { source: 'library', inLibrary: true, confidence: 0.9, exact: false });
  if (j.query) return { candidates: lib.filter((p) => p.title.toLowerCase().includes(j.query.toLowerCase())).map(mine), exact: false, note: '' };
  const c = j.cite;
  const fits = lib.filter((p) => p.families[0] === c.surnames[0] && p.year === c.year && c.hints.every((h) => p.title.toLowerCase().includes(h)));
  const found = fits.map(mine);
  if (found.length === 1) found[0].exact = true;
  (lunta.outside[c.surnames[0]] || []).forEach((m) => found.push({ id: '', title: m.title, authors: m.authors, families: m.authors.map((a) => a.split(' ').pop()),
    year: m.date, meta: m, source: 'crossref', inLibrary: false, confidence: 0.5, exact: false }));
  return { candidates: found, exact: found.length > 0 && found[0].exact, note: found.length ? '' : 'No paper found.' };
}

/** The panel loaded into a document: {ctx, doc, store, lunta, calls, status()}. */
async function panel(opts) {
  opts = opts || {};
  const doc = { ccs: [], cursorAt: null, cursorIn: null, paras: null };
  const store = { parts: opts.parts || [], settings: opts.settings || {} };
  const lunta = { lib: Object.assign({}, LIB), down: !!opts.down, requests: [], outside: opts.outside || {} };
  const w = makeWord(doc);
  (opts.ccs || []).forEach(([tag, text, title]) => { const c = w.cc(tag, text); if (title) c.title = title; doc.ccs.push(c); });
  if (opts.nodes) {
    doc.ooxml = opts.ooxml;
    doc.nodes = [];
    const fields = {};
    opts.nodes.forEach((n) => {
      if (n.field) { const f = doc.field(n.field, n.parent ? fields[n.parent] : undefined); fields[n.name || n.field] = f; doc.nodes.push(f); }
      else { const c = w.cc(n[0], n[1]); if (n[2]) c.title = n[2]; doc.nodes.push(c); }
    });
    doc.ccs.push(...doc.nodes.filter((n) => n.kind !== 'field'));
  }
  if (opts.paras) {
    doc.paras = opts.paras.map((p) => p.map((seg) => (typeof seg === 'string' ? seg : w.cc(seg[0], seg[1]))));
    doc.ccs = [].concat(...doc.paras.map((p) => p.filter((seg) => typeof seg !== 'string')));
  }
  // the cursor already in a citation when the panel opens
  if (opts.cursorIn != null) doc.cursorIn = doc.ccs[opts.cursorIn];
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
  for (const f of ['common.js', 'resolve.js', 'word/fields.js', 'word/app.js']) vm.runInContext(fs.readFileSync(path.join(ROOT, f), 'utf8'), ctx, { filename: f });
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

const rowTexts = (p) => p.els.citelist.children.map((r) => r.querySelector('.t').textContent);

test('a document opened again shows the papers of the citation under the cursor at once, without Update All', async () => {
  const first = await panel({ settings: { luntaStyle: 'vancouver' }, ccs: [['margins:a,b', '(…)'], ['margins:c', '(…)'], ['margins-bib', 'x']] });
  await first.ctx.updateAll();
  const saved = first.store.parts.map((x) => x.xml), ccs = first.doc.ccs.map((c) => [c.tag, c.text]);
  // reopened with the cursor in the first citation: the box shows it as soon as the panel is ready
  const p = await panel({ parts: saved, ccs, cursorIn: 0 });
  await settle();
  assert.equal(p.els.current.hidden, false, 'the citation box shows');
  assert.deepEqual(rowTexts(p), ['Alpha', 'Beta']);
  assert.equal(p.calls.insertText + p.calls.insertHtml, 0, 'nothing updated');
  assert.ok(!p.lunta.requests.some((r) => r.url.endsWith('/api/render')), 'no Update All');
  // Lunta closed: from the document's copy of the papers
  const off = await panel({ parts: saved, ccs, cursorIn: 1, down: true });
  assert.deepEqual(rowTexts(off), ['Gamma <&>']);
  // a click while Update All runs isn't dropped: looked at when it is done
  p.doc.cursorIn = p.doc.ccs[1];
  const running = p.ctx.updateAll();
  p.ctx.checkSelection();
  await running;
  await new Promise((r) => setTimeout(r, 260));
  assert.deepEqual(rowTexts(p), ['Gamma <&>']);
  // a citation inside a control another add-in left (the cursor in the inner one): the outer citation
  const inner = p.cc('', '3');
  inner.parentContentControlOrNullObject = Object.assign(p.doc.ccs[0], { isNullObject: false, load() {} });
  p.doc.cursorIn = inner;
  p.ctx.onSelection();
  await new Promise((r) => setTimeout(r, 260));
  assert.deepEqual(rowTexts(p), ['Alpha', 'Beta']);
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

// ---- Resolve Citations ----

const plainJSON = (x) => JSON.parse(JSON.stringify(x));
const paraTexts = (p) => p.doc.paras.map((para) => para.map((seg) => (typeof seg === 'string' ? seg : seg.text)).join(''));
const LEE = { title: 'Islet cells in situ', authors: ['Ada Lee'], date: '2021', doi: '10.1/lee' };

test('Resolve Citations: placeholders listed, written only when confirmed, one by one', async () => {
  const p = await panel({
    settings: { luntaStyle: 'apa' }, outside: { Lee: [LEE] },
    paras: [['Cells (Smith 2020 alpha) differ (Wu 2019; Nobody 2018). Known ', ['margins:c', '(Wu, 2019)'], '.'],
            ['And (Lee 2021 islet) too.'], [['margins-bib', 'References']]]
  });
  await p.ctx.updateAll();
  const before = JSON.stringify(paraTexts(p));
  await p.ctx.resolveCitations();
  await settle();
  const R = p.ctx.resolver;
  // Lunta's own citation "(Wu, 2019)" is not a placeholder
  assert.deepEqual(plainJSON(R.items.map((it) => it.ph.text)), ['(Smith 2020 alpha)', '(Wu 2019; Nobody 2018)', '(Lee 2021 islet)']);
  assert.deepEqual(plainJSON(R.items.map((it) => it.cites.map((c) => c.state))), [['open'], ['open', 'open'], ['open']]);
  assert.equal(R.items[0].cites[0].candidates[0].id, 'a');
  assert.equal(JSON.stringify(paraTexts(p)), before, 'nothing written before confirming');
  assert.ok(!p.lunta.requests.some((r) => r.url.endsWith('/api/add')), 'nothing added to the library');

  // Confirm All Exact Matches: Smith 2020 (alpha) and Wu 2019; "Nobody" isn't exact, so its placeholder waits
  await R.confirmExact();
  assert.equal(R.items[0].state, 'done');
  assert.equal(R.items[1].state, 'pending');
  assert.equal(paraTexts(p)[0], 'Cells (Smith, 2020) differ (Wu 2019; Nobody 2018). Known (Wu, 2019).');
  // the part found nowhere is skipped: stays as written after the citation
  await R.skip(1, 1);
  assert.equal(paraTexts(p)[0], 'Cells (Smith, 2020) differ (Wu, 2019) (Nobody 2018). Known (Wu, 2019).');
  assert.deepEqual(p.doc.ccs.map((c) => c.tag).slice(0, 3), ['margins:a', 'margins:c', 'margins:c']);
  // a paper from outside the library: added when confirmed, then cited, the bibliography updated
  const lee = R.items[2].cites[0];
  assert.equal(lee.candidates[0].inLibrary, false);
  await R.confirm(2, 0);
  const adds = p.lunta.requests.filter((r) => r.url.endsWith('/api/add'));
  assert.equal(adds.length, 1);
  assert.equal(adds[0].body.meta.doi, '10.1/lee');
  assert.equal(paraTexts(p)[1], 'And (Lee, 2021) too.');
  assert.match(paraTexts(p)[2], /Lee \(2021\)\. Islet cells in situ/);
  assert.match(paraTexts(p)[2], /Smith \(2020\)\. Alpha/);
  assert.match(p.status(), /3 placeholders: 3 cited \(1 paper added to your library\)/);
});

test('Resolve Citations: Skip leaves it; Search finds another; the same text twice', async () => {
  const p = await panel({ settings: { luntaStyle: 'apa' }, paras: [['A (Smith 2020) and B (Smith 2020) and C (Wu 2019).']] });
  await p.ctx.resolveCitations();
  await settle();
  const R = p.ctx.resolver;
  assert.equal(R.items.length, 3);
  // two Smith 2020 papers: not exact; nothing to confirm all at once but Wu
  assert.equal(R.items[0].cites[0].exact, false);
  await R.skip(2, 0);
  assert.equal(R.items[2].state, 'skipped');
  // the second "(Smith 2020)" first: Beta, by searching
  await R.search(1, 0, 'beta');
  assert.equal(R.items[1].cites[0].candidates[0].id, 'b');
  await R.confirm(1, 0);
  assert.equal(paraTexts(p)[0], 'A (Smith 2020) and B (Smith, 2020) and C (Wu 2019).');
  // then the first: the first candidate (Alpha); the letters follow
  await R.confirm(0, 0, 0);
  assert.equal(paraTexts(p)[0], 'A (Smith, 2020a) and B (Smith, 2020b) and C (Wu 2019).');
  assert.deepEqual(p.doc.ccs.map((c) => c.tag), ['margins:a', 'margins:b']);
  assert.match(p.status(), /3 placeholders: 2 cited · 1 left as written/);
});

// ---- a paper's options in a citation ----

test('options: page, prefix, suffix, author left out or in the text; kept in the title, read by old add-ins as before', async () => {
  const title = 'Citation ' + 'lunta-o:' + LuntaCiteForTests.encodeOptions({ a: { prefix: 'see', locator: '12' } });
  const p = await panel({ settings: { luntaStyle: 'apa' }, ccs: [['margins:a', '(…)', title], ['margins:c', '(…)'], ['margins-bib', 'x']] });
  await p.ctx.updateAll();
  assert.deepEqual(texts(p).slice(0, 2), ['(see Smith, 2020, p. 12)', '(Wu, 2019)']);
  // an add-in from before options reads the same papers from the tag
  assert.deepEqual(LuntaCiteForTests.parseTag(p.doc.ccs[0].tag), ['a']);
  // the cursor in the second citation: Page… for Wu, without the author
  p.doc.cursorIn = p.doc.ccs[1];
  p.ctx.onSelection();
  await new Promise((r) => setTimeout(r, 260));
  await p.ctx.setPaperOptions('c', { locator: '3-5', mode: 'suppress' });
  assert.equal(p.doc.ccs[1].text, '(2019, pp. 3–5)');
  assert.match(p.doc.ccs[1].title, /^Citation lunta-o:/);
  assert.deepEqual(plainJSON(LuntaCiteForTests.optionsOfTitle(p.doc.ccs[1].title)), { c: { locator: '3-5', mode: 'suppress' } });
  // in the text, and in a numbered style
  await p.ctx.setPaperOptions('c', { mode: 'narrative' });
  assert.equal(p.doc.ccs[1].text, 'Wu (2019)');
  await p.ctx.setStyle('vancouver');
  assert.deepEqual(texts(p).slice(0, 2), ['[see 1, p. 12]', 'Wu [2]']);
  // emptied: plain again, the title too
  await p.ctx.setPaperOptions('c', {});
  assert.equal(p.doc.ccs[1].title, 'Citation');
  assert.equal(p.doc.ccs[1].text, '[2]');
});

test('options in a journal style go to Lunta with the citations', async () => {
  const title = 'Citation lunta-o:' + LuntaCiteForTests.encodeOptions({ a: { locator: '7', label: 'figure' } });
  const p = await panel({ parts: ['<lunta xmlns="https://lunta.app/ns/word-citations/1">{"style":"csl:nature","papers":{}}</lunta>'], ccs: [['margins:a', '(…)', title], ['margins:c', '(…)']] });
  await p.ctx.updateAll().catch(() => {});
  const r = p.lunta.requests.find((x) => x.url.endsWith('/api/render'));
  assert.ok(r, 'asked Lunta to render');
  assert.deepEqual(r.body.options, [{ a: { locator: '7', label: 'figure' } }, null]);
});

test('a raised citation’s prefix and suffix stay on the line; the bibliography’s entries all in the text’s font', async () => {
  const title = 'Citation lunta-o:' + LuntaCiteForTests.encodeOptions({ c: { prefix: 'see', locator: '5' } });
  const p = await panel({ parts: ['<lunta xmlns="https://lunta.app/ns/word-citations/1">{"style":"csl:cell-systems","papers":{}}</lunta>'],
    ccs: [['margins:a,b', '(…)'], ['margins:c', '(…)', title], ['margins-bib', 'x']] });
  p.doc.bodyFont = { name: 'Times New Roman', size: 12, color: '#000000' };
  // Cell Systems as the core gives it (core/src/csl.rs: the locator added, the prefix not raised)
  p.lunta.render = () => ({ title: 'Cell Systems', citations: [{ text: '1,2', superscript: true, before: '', after: '' }, { text: 'see 3(p5)', superscript: true, before: 'see ', after: '' }],
    bibliography: [{ id: 'a', text: '1. Smith, J. Alpha.' }, { id: 'b', text: '2. Smith, J. Beta.' }, { id: 'c', text: '3. Wu, P. Gamma.' }], items: [LIB.a, LIB.b, LIB.c], missing: [] });
  await p.ctx.updateAll();
  assert.deepEqual(texts(p).slice(0, 2), ['1,2', 'see 3(p5)']);
  assert.deepEqual(p.doc.ccs[1].runs.map((r) => [r.text, r.font.superscript]), [['see ', false], ['3(p5)', true]]);
  assert.deepEqual(p.doc.ccs[0].runs.map((r) => [r.text, r.font.superscript]), [['1,2', true]]);
  // Word left the last entry in the old paragraph's font and spacing: made like the others
  const bib = p.doc.ccs[2];
  assert.equal(bib.paras.length, 3);
  assert.deepEqual(bib.paras.map((x) => [x.font.name, x.font.size, x.spaceAfter, x.lineSpacing]), [['Times New Roman', 12, 6, 12], ['Times New Roman', 12, 6, 12], ['Times New Roman', 12, 6, 12]]);
  // the text in another font: the entries follow it
  p.doc.bodyFont = { name: 'Aptos', size: 11, color: '#000000' };
  p.lunta.render = (j) => Object.assign({}, p.lunta.render0(j), { bibliography: [{ id: 'a', text: '1. Smith, J. Alpha!' }, { id: 'b', text: '2. Smith, J. Beta.' }, { id: 'c', text: '3. Wu, P. Gamma.' }] });
  p.lunta.render0 = () => ({ title: 'Cell Systems', citations: [{ text: '1,2', superscript: true }, { text: 'see 3(p5)', superscript: true }], items: [LIB.a, LIB.b, LIB.c], missing: [] });
  await p.ctx.updateAll();
  assert.deepEqual(bib.paras.map((x) => x.font.name), ['Aptos', 'Aptos', 'Aptos']);
  // an older Lunta that doesn't say what isn't raised: all of it raised, as before
  assert.deepEqual(p.doc.ccs[1].runs.map((r) => [r.text, r.font.superscript]), [['see 3(p5)', true]]);
});

// ---- Convert Citations from Zotero, EndNote or Mendeley ----

const X = require('./fixtures-word.js');
const LuntaCiteForTests = require('../citations/common.js');

test('Convert: reads Zotero, EndNote (inline and fldData) and Mendeley (Desktop and Cite); nothing changes before Convert', async () => {
  const p = await panel({ settings: { luntaStyle: 'apa' }, ooxml: X.documentXML(), nodes: X.NODES });
  const before = JSON.stringify(p.doc.nodes.map((n) => n.code || n.tag));
  await p.ctx.convertCitations();
  await settle();
  const plan = p.ctx.conversion.plan;
  assert.equal(plan.citations.length, 5);
  assert.deepEqual(plainJSON(plan.citations.map((c) => c.source)), ['Zotero', 'EndNote', 'EndNote', 'Mendeley', 'Mendeley']);
  // Alpha (DOI, in another case), Gamma, Beta (twice: once) in the library; Delta and Epsilon to add
  assert.deepEqual(plainJSON(plan.works.map((w) => [w.meta.title, w.id])),
    [['Alpha', 'a'], ['Gamma <&>', 'c'], ['Beta', 'b'], ['Delta rays & cells', ''], ['Epsilon waves in sleep', '']]);
  assert.equal(LuntaCiteForTests.convertSummary(plan), '5 citations, 5 papers: 3 in your library, 2 to add');
  assert.equal(JSON.stringify(p.doc.nodes.map((n) => n.code || n.tag)), before, 'nothing written yet');
  assert.ok(!p.lunta.requests.some((r) => r.url.endsWith('/api/add')), 'nothing added yet');
  // EndNote's details: authors as names, the DOI and PubMed id, the prefix and suffix kept
  const delta = plan.citations[2].works[0];
  assert.deepEqual(plainJSON(delta.meta.authors), ['Priya Rao', 'Åsa Lindqvist', 'Kim Ng']);
  assert.equal(delta.meta.doi, '10.5555/delta');
  assert.equal(delta.meta.pmid, '36543210');
  assert.deepEqual(plainJSON(delta.options), { prefix: 'e.g.', suffix: ', for the method' });

  // Convert, adding Delta but not Epsilon: Epsilon's (Mendeley Desktop) citation stays, and is listed
  await p.ctx.runConversion([plan.works[3].key]);
  const adds = p.lunta.requests.filter((r) => r.url.endsWith('/api/add'));
  assert.deepEqual(adds.map((r) => r.body.meta.title), ['Delta rays & cells']);
  const kinds = p.doc.nodes.map((n) => (n.kind === 'field' ? 'field:' + n.code.trim().split(/\s+/).slice(0, 2).join(' ') : n.tag.replace(/^(MENDELEY_CITATION_v3_).*/, '$1')));
  const newId = Object.keys(p.lunta.lib).find((k) => p.lunta.lib[k].title === 'Delta rays & cells');
  assert.deepEqual(kinds, ['margins:a,c', 'field:HYPERLINK "https://example.org"', 'margins:b', 'margins:' + newId, 'field:ADDIN CSL_CITATION', 'margins:b', 'margins-bib']);
  // the options came along
  assert.deepEqual(plainJSON(LuntaCiteForTests.optionsOfTitle(p.doc.ccs[0].title)), { a: { prefix: 'see', locator: '12' } });
  assert.deepEqual(plainJSON(LuntaCiteForTests.optionsOfTitle(p.doc.ccs[3].title)), { b: { locator: '7', label: 'figure' } });
  assert.deepEqual(texts(p).slice(0, 4), ['(see Smith, 2020a, p. 12; Wu, 2019)', '(2020b, p. 4)', '(e.g. Rao et al., 2023, for the method)', '(Smith, 2020b, fig. 7)']);
  // the Zotero bibliography became Lunta's, EndNote's went; Mendeley's Desktop citation is left
  assert.match(p.doc.ccs[4].html, /Rao \(2023\)/);
  assert.match(p.status(), /Converted 4 citations \(1 paper added to your library\)\. 1 left as they were: “\(Okafor, 2018\)”: “Epsilon waves in sleep” isn’t in your library/);
});

test('Convert: a document with nothing to convert says so; Zotero bookmarks are explained', async () => {
  const p = await panel({ settings: { luntaStyle: 'apa' }, ooxml: '<w:body><w:p><w:bookmarkStart w:id="0" w:name="ZOTERO_BREF_abc123"/></w:p></w:body>', nodes: [] });
  await p.ctx.convertCitations();
  assert.match(p.status(), /stored as bookmarks.*Store citations as: Fields/);
  const q = await panel({ settings: { luntaStyle: 'apa' }, ooxml: '<w:body></w:body>', nodes: [] });
  await q.ctx.convertCitations();
  assert.match(q.status(), /no citations from Zotero, EndNote, Mendeley or ReadCube Papers/);
});

test('Convert: Word listing EndNote’s nested field inside the outer one’s code still lines up', async () => {
  const nodes = X.NODES.filter((n) => !n.parent).map((n) => (n.name === 'en-outer' ? { field: ' ADDIN EN.CITE  ADDIN EN.CITE.DATA ', name: 'en-outer' } : n));
  const p = await panel({ settings: { luntaStyle: 'vancouver' }, ooxml: X.documentXML(), nodes });
  await p.ctx.convertCitations();
  const plan = p.ctx.conversion.plan;
  await p.ctx.runConversion(plan.works.filter((w) => !w.id).map((w) => w.key));
  assert.deepEqual(p.doc.nodes.filter((n) => n.kind === 'field').map((n) => n.code.trim().split(' ')[0]), ['HYPERLINK']);
  assert.match(p.status(), /Converted 5 citations \(2 papers added to your library\)\./);
  assert.deepEqual(texts(p).slice(0, 5), ['[see 1, p. 12; 2]', '[3, p. 4]', '[e.g. 4, for the method]', '[5]', '[3, fig. 7]']);
});

test('Convert: several long EndNote citations (nested EN.CITE.DATA fields listed by Word) each get their own papers', async () => {
  const titles = ['Long one', 'Long two', 'Long three'];
  const d = X.endnoteLong(titles);
  const p = await panel({ settings: { luntaStyle: 'apa' }, ooxml: d.xml, nodes: d.nodes });
  await p.ctx.convertCitations();
  const plan = p.ctx.conversion.plan;
  assert.deepEqual(plainJSON(plan.citations.map((c) => c.works.map((w) => w.meta.title))), titles.map((t) => [t]));
  await p.ctx.runConversion(plan.works.map((w) => w.key));
  const titleOf = (id) => p.lunta.lib[id].title;
  const cites = p.doc.nodes.filter((n) => n.kind !== 'field' && n.tag.startsWith('margins:'));
  assert.deepEqual(cites.map((c) => titleOf(c.tag.slice('margins:'.length))), titles);
  assert.deepEqual(p.doc.nodes.filter((n) => n.kind === 'field').map((n) => n.code), [], 'no EndNote field is left');
  assert.match(p.status(), /Converted 3 citations \(3 papers added to your library\)\./);
});

test('sameCode: a field code matches its own start, not a longer word', () => {
  const F = require('../citations/word/fields.js');
  assert.ok(F.sameCode(' ADDIN EN.CITE ', 'ADDIN EN.CITE'));
  assert.ok(F.sameCode('ADDIN EN.CITE  ADDIN EN.CITE.DATA', ' ADDIN EN.CITE '));
  assert.ok(!F.sameCode('ADDIN EN.CITE.DATA', 'ADDIN EN.CITE'));
  assert.ok(!F.sameCode('ADDIN EN.CITE', ' ADDIN EN.CITE.DATA '));
});

test('Convert: the other manager’s bibliography stays while an unreadable citation of it does', async () => {
  const d = X.endnoteUnreadable();
  const p = await panel({ settings: { luntaStyle: 'apa' }, ooxml: d.xml, nodes: d.nodes });
  await p.ctx.convertCitations();
  assert.equal(p.ctx.conversion.plan.citations.length, 1);
  await p.ctx.runConversion([]);
  assert.deepEqual(p.doc.nodes.map((n) => (n.kind === 'field' ? n.code.trim() : n.tag)), ['ADDIN EN.CITE', 'margins:b', 'ADDIN EN.REFLIST']);
  assert.match(p.status(), /Converted 1 citation\. 1 left as they were: the EndNote bibliography stays until its citations are converted/);
});

// ---- ReadCube Papers (SmartCite) ----

const KE = { id: 'k', title: 'In situ sequencing for RNA analysis in preserved tissue and cells', authors: ['Rongqin Ke'], families: ['Ke'], year: '2013',
  meta: { title: 'In situ sequencing for RNA analysis in preserved tissue and cells', authors: ['Rongqin Ke'], date: '2013', doi: '10.1038/nmeth.2563' } };
const SC = 'ReadCube Papers (SmartCite)';
const tagsOf = (p) => p.doc.nodes.map((n) => (n.tag.startsWith('margins') ? n.tag : n.title));

test('Convert: ReadCube Papers (SmartCite) numbered citations ("1–3", "2,4"), their details from the bibliography; the controls stay where they are', async () => {
  const d = X.smartCite([['1–3', [1, 2, 3]], ['2,4', [2, 4]], ['5', [5]], ['6', [6]]], X.SC_BIB_CELL);
  const p = await panel({ settings: { luntaStyle: 'vancouver' }, ooxml: d.xml, nodes: d.nodes });
  p.lunta.lib.k = KE;
  const ids = p.doc.nodes.map((n) => n.id);
  await p.ctx.convertCitations();
  const plan = p.ctx.conversion.plan;
  assert.deepEqual(plainJSON(plan.citations.map((c) => [c.source, c.text, c.works.length])), [[SC, '1–3', 3], [SC, '2,4', 2], [SC, '5', 1], [SC, '6', 1]]);
  assert.deepEqual(plainJSON(plan.works.map((w) => [w.meta.doi || w.meta.arxiv || '', w.id])),
    [['10.1038/nmeth.2563', 'k'], ['10.1126/science.aaa6090', ''], ['10.1126/science.aaf2403', ''], ['10.1038/s41586-021-03819-2', ''], ['', ''], ['1706.03762', '']]);
  assert.equal(LuntaCiteForTests.convertSummary(plan), '4 citations, 6 papers: 1 in your library, 5 to add');
  // the titles, authors and years from the bibliography; the ids from the citation
  const jumper = plan.works[3].meta;
  assert.equal(jumper.title, 'Highly accurate protein structure prediction with AlphaFold');
  assert.equal(jumper.authors[0], 'J. Jumper');
  assert.equal(jumper.date, '2021');
  assert.equal(jumper.pmcid, 'PMC8371605');
  assert.equal(plan.works[2].meta.title, 'Visualization and analysis of gene expression in tissue sections by spatial transcriptomics', 'an entry over two paragraphs');
  assert.equal(plan.works[1].meta.pmid, '25858977');
  assert.match(plan.works[4].meta.title, /^Molecular Biology of the Cell/);
  // the review: where they're from, and the style they were in
  const notes = p.els.cvlist.children.map((c) => c.textContent).join(' ');
  assert.match(notes, /From ReadCube Papers \(SmartCite\)\./);
  assert.match(notes, /“Cell Systems” style/);
  assert.match(notes, /The ReadCube Papers \(SmartCite\) bibliography becomes Lunta’s/);
  assert.ok(!p.lunta.requests.some((r) => r.url.endsWith('/api/add')), 'nothing added yet');

  await p.ctx.runConversion(plan.works.filter((w) => !w.id).map((w) => w.key));
  assert.deepEqual(p.doc.nodes.map((n) => n.id), ids, 'the same controls, in their places');
  const titleOf = (id) => p.lunta.lib[id].title.slice(0, 20);
  const cites = p.doc.nodes.slice(0, 4).map((n) => n.tag.slice('margins:'.length).split(',').map(titleOf));
  assert.deepEqual(plainJSON(cites), [['In situ sequencing f', 'Spatially resolved, ', 'Visualization and an'], ['Spatially resolved, ', 'Highly accurate prot'],
    ['Molecular Biology of'], ['Attention is all you']]);
  assert.equal(p.doc.nodes[4].tag, 'margins-bib');
  assert.equal(p.doc.nodes[4].title, 'Bibliography (Lunta)');
  assert.deepEqual(texts(p).slice(0, 4), ['[1–3]', '[2,4]', '[5]', '[6]']);
  assert.match(p.status(), /Converted 4 citations \(5 papers added to your library\)\./);
});

test('Convert: SmartCite, a paper not added: its citation and SmartCite’s bibliography stay', async () => {
  const d = X.smartCite([['1–3', [1, 2, 3]], ['5', [5]]], X.SC_BIB_CELL);
  const p = await panel({ settings: { luntaStyle: 'vancouver' }, ooxml: d.xml, nodes: d.nodes });
  p.lunta.lib.k = KE;
  await p.ctx.convertCitations();
  const plan = p.ctx.conversion.plan;
  await p.ctx.runConversion(plan.works.filter((w) => !w.id && w.meta.doi).map((w) => w.key));
  assert.deepEqual(tagsOf(p).map((t) => t.replace(/:.*/, '')), ['margins', 'SmartCite Citation', 'SmartCite Bibliography']);
  assert.match(p.status(), /Converted 1 citation \(2 papers added to your library\)\. 2 left as they were: “5”: “Molecular Biology of the Cell.*” isn’t in your library; the ReadCube Papers \(SmartCite\) bibliography stays until its citations are converted\./);
});

test('Convert: SmartCite author–year citations without identifiers in their tags, found in an APA bibliography; an unreadable one keeps the bibliography', async () => {
  const d = X.smartCite([['(Ke et al., 2013; Chen et al., 2015)', [1, 2]], ['Ståhl et al. (2016)', [3]], ['(see above)', null]], X.SC_BIB_APA, { withIds: false, superscript: false });
  const p = await panel({ settings: { luntaStyle: 'apa' }, ooxml: d.xml, nodes: d.nodes });
  await p.ctx.convertCitations();
  const plan = p.ctx.conversion.plan;
  assert.deepEqual(plainJSON(plan.citations.map((c) => c.works.map((w) => w.meta.doi))), [['10.1038/nmeth.2563', '10.1126/science.aaa6090'], ['10.1126/science.aaf2403']]);
  assert.equal(plan.works[0].meta.journal, 'Nature Methods');
  assert.equal(plan.works[0].meta.issue, '9');
  const notes = p.els.cvlist.children.map((c) => c.textContent).join(' ');
  assert.match(notes, /1 citation couldn’t be read/);
  assert.match(notes, /The ReadCube Papers \(SmartCite\) bibliography stays, as some of its citations can’t be read\./);
  await p.ctx.runConversion(plan.works.map((w) => w.key));
  assert.deepEqual(tagsOf(p).map((t) => t.replace(/:.*/, '')), ['margins', 'margins', 'SmartCite Citation', 'SmartCite Bibliography']);
});

test('Check Citations: each citation’s sentence sent with its paragraph; the document isn’t changed; a passage opens in Lunta', async () => {
  const p = await panel({
    settings: { luntaStyle: 'apa' },
    paras: [['Microglia are immune cells.'], ['Rims have iron ', ['margins:a', '(Smith, 2020)'], '. Lesions grow ', ['margins:b,c', '(Smith, 2020; Wu, 2019)'], '.'],
            [['margins-bib', 'References']]]
  });
  await p.ctx.updateAll();
  const before = JSON.stringify(paraTexts(p));
  const writes = p.calls.insertText + p.calls.insertHtml;
  await p.ctx.checkCitations();
  await settle();
  const asked = p.lunta.requests.filter((r) => r.url.endsWith('/api/claims')).map((r) => r.body);
  assert.equal(asked.length, 2);
  assert.deepEqual(plainJSON(asked.map((b) => b.ids)), [['a'], ['b', 'c']]);
  assert.equal(asked[0].text.slice(asked[0].start, asked[0].end), '(Smith, 2020a)');
  assert.equal(asked[0].before, 'Microglia are immune cells.');
  assert.deepEqual(plainJSON(asked[0].others), [[asked[1].start, asked[1].end]]);
  assert.ok(asked[0].papers.a && asked[0].papers.a.title === 'Alpha', 'the details the document keeps go along');
  assert.equal(JSON.stringify(paraTexts(p)), before, 'the document is only read');
  assert.equal(p.calls.insertText + p.calls.insertHtml, writes);
  assert.equal(p.els.claimsBox.hidden, false);
  assert.match(p.status(), /2 citations checked \(3 papers\): 3 possible support\./);
  // a passage clicked: Lunta asked to show the paper there
  const all = (e, out = []) => { out.push(e); (e.children || []).forEach((c) => all(c, out)); return out; };
  const link = all(p.els.cllist).find((e) => e.onclick && e.href && e.href.startsWith('lunta://'));
  link.onclick({ preventDefault() {} });
  await settle();
  const open = p.lunta.requests.find((r) => r.url.endsWith('/api/open'));
  assert.deepEqual(plainJSON(open.body), { id: 'a', find: 'A passage' });
});

// ---- the writer's words typed into a citation ----

test('Update All keeps words typed into a citation ("²⁸ with GSEApy"), outside it; the cursor goes after a new citation', async () => {
  const p = await panel({ settings: { luntaStyle: 'nature' }, ccs: [['margins:a', '28 with GSEApy'], ['margins:b', '29'], ['margins-bib', 'x']] });
  await p.ctx.updateAll();
  assert.deepEqual(texts(p).slice(0, 2), ['1', '2'], 'the citations renumbered');
  const c = p.doc.ccs[0];
  assert.deepEqual(c.runs.map((r) => [r.text, r.font.superscript]), [['1', true]]);
  assert.deepEqual([c.outside.before, c.outside.after], ['', ' with GSEApy'], 'the words after the control, not deleted');
  assert.equal(c.outside.runs[0].font.superscript, false, 'on the line, not raised');
  assert.equal(p.doc.ccs[1].outside.after, '');
  assert.match(p.status(), /Text typed inside a citation was kept, outside it: “with GSEApy”\./);
  // the next Update All: nothing to move, nothing rewritten
  const writes = p.calls.insertText + p.calls.insertHtml;
  await p.ctx.updateAll();
  assert.equal(p.calls.insertText + p.calls.insertHtml, writes);
  // words before and after a citation: the old number goes, the words stay
  p.doc.ccs[1].text = 'as in 29, and';
  await p.ctx.updateAll();
  assert.deepEqual(texts(p).slice(0, 2), ['1', '2']);
  assert.deepEqual([p.doc.ccs[1].outside.before, p.doc.ccs[1].outside.after], ['as in ', ', and']);
  // a new citation: the cursor after it, outside its control
  p.ctx.picked = [LIB.c];
  await p.ctx.insertCitation();
  const added = p.doc.ccs[p.doc.ccs.length - 1];
  assert.equal(added.tag, 'margins:c');
  assert.equal(p.calls.cursorAfter, added);
});

test('Convert: a SmartCite citation with the writer’s words inside its control ("28 with GSEApy") converts, and the words stay outside', async () => {
  const d = X.smartCite([['1 with GSEApy', [1]]], X.SC_BIB_CELL, { withIds: false });
  const p = await panel({ settings: { luntaStyle: 'vancouver' }, ooxml: d.xml, nodes: d.nodes });
  p.lunta.lib.k = KE;
  await p.ctx.convertCitations();
  const plan = p.ctx.conversion.plan;
  assert.deepEqual(plainJSON(plan.works.map((w) => [w.meta.title, w.id])), [['In situ sequencing for RNA analysis in preserved tissue and cells', 'k']]);
  await p.ctx.runConversion([]);
  assert.equal(p.doc.nodes[0].tag, 'margins:k');
  assert.equal(p.doc.nodes[0].text, '[1]');
  assert.equal(p.doc.nodes[0].outside.after, ' with GSEApy');
  assert.match(p.status(), /Converted 1 citation\./);
});

// ---- Convert Citations to Plain Text ----

test('Convert Citations to Plain Text: warned first; then every citation and the bibliography are ordinary text and the document keeps no Lunta data', async () => {
  const p = await panel({ settings: { luntaStyle: 'nature' }, ccs: [['margins:a', '(…)'], ['margins:b,c', '(…)'], ['margins-bib', 'x'], ['other', 'a form field']] });
  await p.ctx.updateAll();
  assert.equal(p.store.parts.length, 1);
  p.ctx.openPlain();
  assert.equal(p.els.plainBox.hidden, false);
  assert.equal(p.els.plainText.textContent, 'This can’t be undone. Every Lunta citation and the bibliography become ordinary text, as they look now (superscript and italics kept), ' +
    'and the copy of the cited papers Lunta keeps in the document is removed: Lunta can’t update them any more. Save a copy of the document first and do this on the copy, for the version you submit.');
  assert.deepEqual(tagsOfCcs(p), ['margins:a', 'margins:b,c', 'margins-bib', 'other'], 'nothing changed before Convert to Plain Text');
  p.ctx.closePlain();
  assert.equal(p.els.plainBox.hidden, true);
  p.ctx.openPlain();
  await p.ctx.convertToPlain();
  assert.deepEqual(tagsOfCcs(p), ['other'], 'only Lunta’s controls removed');
  // their text and formatting stay: raised numbers, the bibliography's entries
  const kept = p.doc.unwrapped.slice().reverse();
  assert.deepEqual(kept.map((u) => [u.tag, u.text]).slice(0, 2), [['margins:a', '1'], ['margins:b,c', '2,3']]);
  assert.deepEqual(kept[0].runs.map((r) => [r.text, r.font.superscript]), [['1', true]]);
  assert.match(kept[2].html, /Alpha/);
  assert.equal(p.store.parts.length, 0, 'the document’s copy of the papers removed');
  assert.ok(!('luntaStyle' in p.store.settings) && !('luntaState' in p.store.settings));
  assert.equal(p.status(), 'Converted 2 citations and the bibliography to plain text. The document has no Lunta citations now.');
  assert.equal(p.els.plainBox.hidden, true);
  // again: nothing left
  await p.ctx.convertToPlain();
  assert.equal(p.status(), 'This document has no Lunta citations or bibliography.');
});

test('Convert Citations to Plain Text: when Update All can’t run, nothing is converted', async () => {
  const p = await panel({ ccs: [['margins:a', '(Smith, 2020)'], ['margins:zz', '(Gone, 2001)']] });
  await p.ctx.convertToPlain();
  assert.match(p.status(), /Nothing was converted to plain text\./);
  assert.deepEqual(tagsOfCcs(p), ['margins:a', 'margins:zz']);
});
const tagsOfCcs = (p) => p.doc.ccs.map((c) => c.tag);
