// Lunta citations for Word: citations are content controls tagged "margins:<id>,<id>", the
// bibliography is a content control tagged "margins-bib". "Update All" renumbers and reformats
// every citation and rewrites the bibliography; text that is already right is left alone.
// The document keeps its style and a copy of the papers it cites in a custom XML part (shared by
// every copy of the add-in, unlike add-in settings), so it updates the same on any computer.
// Served by Lunta itself (https://localhost:47481) or from the hosted copy (GitHub Pages, the
// Microsoft store listing): either way the library comes from the Lunta app on this computer.
var LUNTA = 'https://localhost:47481';
var API = (location.origin === LUNTA ? '' : LUNTA) + '/api';
var VERSION = '12';   // bump with the ?v= in index.html, so Word loads the new files
var NS = 'https://lunta.app/ns/word-citations/1';
var picked = [];          // [{id, title, ...}]
var style = 'apa';
var state = { papers: {} };   // what the document keeps: {style, styleTitle, bibStyle, papers}
var styleUnknown = false;     // citations in the document, but no saved style: ask before updating
var busy = false, ready = false, lastSearch = 0;
var current = null;           // the citation the cursor is in: {id (content control), ids}

function $(id) { return document.getElementById(id); }
function status(msg, err) { var s = $('status'); s.textContent = msg || ''; s.className = 'status' + (err ? ' err' : ''); }
// Word's errors say little ("Check the OfficeExtension.Error.debugInfo"): show where and what
function fail(e) {
  if (e && e.kind) { status(LuntaCite.describe(e, 'word'), true); return; }
  var d = e && e.debugInfo, more = '';
  try { more = d && d.errorLocation ? d.errorLocation : (d ? JSON.stringify(d) : ''); } catch (x) {}
  if (window.console) console.error(e, d);
  status((e && e.message || String(e)) + (more && e.name === 'OfficeExtension.Error' ? ' · ' + more : ''), true);
}

function getJSON(path, body, timeout) {
  return LuntaCite.request(API + path, { body: body, timeout: timeout });
}

function setButtons() {
  if ($('resolve')) $('resolve').disabled = busy || !ready;
  $('insert').disabled = busy || !ready || picked.length === 0;
  $('insert').textContent = current ? 'Add to This Citation' : 'Insert Citation';
  $('bib').disabled = $('refresh').disabled = busy || !ready;
  $('style').disabled = busy || !ready;
}

/** One change to the document at a time (a second click while updating does nothing). */
function exclusive(fn) {
  return function () {
    if (busy || !ready) return;
    busy = true; setButtons();
    return Promise.resolve().then(fn).catch(fail).then(function () { busy = false; setButtons(); });
  };
}

function renderPicked() {
  $('picked').textContent = picked.length ? 'Citing: ' + LuntaCite.pickedLabel(picked) : '';
  setButtons();
  document.querySelectorAll('.hit').forEach(function (el) {
    el.classList.toggle('on', picked.some(function (p) { return p.id === el.dataset.id; }));
  });
}

function search(q) {
  if (!q.trim()) { $('results').innerHTML = ''; return; }
  var mine = ++lastSearch;
  getJSON('/search?q=' + encodeURIComponent(q)).then(function (hits) {
    if (mine !== lastSearch) return;   // a later search answered first
    $('results').innerHTML = '';
    if (!hits.length) { $('results').innerHTML = '<p class="status">No papers match.</p>'; return; }
    hits.forEach(function (h) {
      var el = document.createElement('div');
      el.className = 'hit'; el.dataset.id = h.id;
      el.innerHTML = '<div class="t"></div><div class="m"></div>';
      el.querySelector('.t').textContent = h.title || 'Untitled';
      el.querySelector('.m').textContent = LuntaCite.hitMeta(h);
      el.onclick = function () {
        var i = picked.findIndex(function (p) { return p.id === h.id; });
        if (i >= 0) picked.splice(i, 1); else picked.push(h);
        renderPicked();
      };
      $('results').appendChild(el);
    });
    renderPicked();
  }).catch(function (e) { if (mine === lastSearch) fail(e); });
}

// ---- what the document keeps ----

function asyncCall(fn) {
  return new Promise(function (ok, no) {
    fn(function (r) { if (r.status === Office.AsyncResultStatus.Succeeded) ok(r.value); else no(r.error); });
  });
}
function xmlParts() {
  try {
    if (Office.context.requirements.isSetSupported('CustomXmlParts', '1.1')) return Office.context.document.customXmlParts;
  } catch (e) {}
  return null;
}
function setting(k) { try { return Office.context.document.settings.get(k); } catch (e) { return null; } }
function escapeXML(s) { return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;'); }

function loadState() {
  var parts = xmlParts();
  var fromSettings = function () {
    try { return JSON.parse(setting('luntaState') || 'null'); } catch (e) { return null; }
  };
  var read = parts ? asyncCall(function (cb) { parts.getByNamespaceAsync(NS, cb); }).then(function (list) {
    return Promise.all(list.map(function (p) { return asyncCall(function (cb) { p.getXmlAsync(cb); }); }));
  }).then(function (xmls) {
    // the newest, if a save was interrupted and left two
    var best = null;
    xmls.forEach(function (x) {
      try {
        var d = new DOMParser().parseFromString(x, 'application/xml');
        var st = JSON.parse(d.documentElement.textContent);
        if (st && (!best || (st.saved || 0) > (best.saved || 0))) best = st;
      } catch (e) {}
    });
    return best || fromSettings();
  }) : Promise.resolve(fromSettings());
  return read.catch(function () { return fromSettings(); }).then(function (st) {
    st = st || {};
    // documents from before: the style in this add-in's settings
    if (!LuntaCite.isStyle(st.style) && LuntaCite.isStyle(setting('luntaStyle'))) st.style = setting('luntaStyle');
    st.papers = st.papers || {};
    return st;
  });
}

function saveState() {
  state.saved = Date.now();
  try {
    Office.context.document.settings.set('luntaStyle', state.style);   // for older copies of the add-in
    if (!xmlParts()) Office.context.document.settings.set('luntaState', JSON.stringify(state));
    Office.context.document.settings.saveAsync(function () {});
  } catch (e) {}
  var parts = xmlParts();
  if (!parts) return Promise.resolve();
  var xml = '<lunta xmlns="' + NS + '">' + escapeXML(JSON.stringify(state)) + '</lunta>';
  // the new part first, then the old ones removed: an interruption leaves two, never none
  return asyncCall(function (cb) { parts.getByNamespaceAsync(NS, cb); }).then(function (old) {
    return asyncCall(function (cb) { parts.addAsync(xml, cb); }).then(function () {
      return Promise.all(old.map(function (p) { return asyncCall(function (cb) { p.deleteAsync(cb); }).catch(function () {}); }));
    });
  });
}

// ---- inserting ----

var insertCitation = exclusive(function () {
  var ids = picked.map(function (p) { return p.id; });
  return Word.run(function (ctx) {
    var sel = ctx.document.getSelection();
    var parent = sel.parentContentControlOrNullObject;
    parent.load('isNullObject,tag');
    return ctx.sync().then(function () {
      if (!parent.isNullObject && parent.tag === LuntaCite.BIB_TAG) {
        throw new Error('That’s the bibliography. Click in your text where the citation should go.');
      }
      // the cursor is in a citation: add the papers to it, "(A, 2021; B, 2023)"
      var had = parent.isNullObject ? null : LuntaCite.parseTag(parent.tag);
      if (had) {
        parent.tag = LuntaCite.makeTag(had.concat(ids));
        return ctx.sync();
      }
      // at the end of the selection, so selected text isn't replaced
      wrapCitation(sel.getRange('End'), ids);
      return ctx.sync();
    });
  }).then(function () {
    picked = []; renderPicked();
    if (current) { current.ids = LuntaCite.unique(current.ids.concat(ids)); lastShown = ''; showCurrent(); }
    return update().catch(function (e) {
      var err = new Error(LuntaCite.describe(e, 'word') + ' The citation is in the document as “(…)”; Update All fills it in.');
      throw err;
    });
  });
});

/** A new citation of `ids` in place of the range: text first, then the content control around
 *  it (Word on the web won't wrap an empty range). Update All fills in its text. */
function wrapCitation(range, ids) {
  var cc = range.insertText('(…)', 'Replace').insertContentControl();
  cc.tag = LuntaCite.makeTag(ids);
  cc.title = 'Citation';
  cc.appearance = 'Hidden';
  return cc;
}

var insertBibliography = exclusive(function () {
  var existed = false;
  return Word.run(function (ctx) {
    var existing = ctx.document.contentControls.getByTag(LuntaCite.BIB_TAG);
    existing.load('items');
    return ctx.sync().then(function () {
      if (existing.items.length) { existed = true; existing.items[0].select(); return ctx.sync(); }
      // its own paragraph after the cursor's, with text in it before it is wrapped (see above)
      var p = ctx.document.getSelection().paragraphs.getLast().insertParagraph('References will appear here.', 'After');
      var cc = p.insertContentControl();
      cc.tag = LuntaCite.BIB_TAG; cc.title = 'Bibliography (Lunta)';
      return ctx.sync();
    });
  }).then(function () { return update(); }).then(function () {
    if (existed) status('This document already has a bibliography (selected); it was updated. ' + $('status').textContent);
  });
});

// ---- the citation under the cursor: its papers, to remove some or add more ----

var selTimer = null, lastShown = '';
function onSelection() {
  clearTimeout(selTimer);
  selTimer = setTimeout(function () {
    if (busy || !ready) return;
    Word.run(function (ctx) {
      var cc = ctx.document.getSelection().parentContentControlOrNullObject;
      cc.load('isNullObject,id,tag');
      return ctx.sync().then(function () {
        var ids = cc.isNullObject ? null : LuntaCite.parseTag(cc.tag);
        current = ids && ids.length ? { id: cc.id, ids: ids } : null;
        showCurrent();
      });
    }).catch(function () {});
  }, 200);
}

/** One line per paper of the current citation: from Lunta, or from the document's copy. */
function showCurrent() {
  var box = $('current');
  setButtons();
  if (!current) { box.hidden = true; lastShown = ''; return; }
  var key = current.id + ':' + current.ids.join(',');
  box.hidden = false;
  if (key === lastShown) return;
  lastShown = key;
  var list = $('citelist');
  list.innerHTML = '';
  var rows = {};
  current.ids.forEach(function (id) {
    var row = document.createElement('div');
    row.className = 'cite-row';
    row.innerHTML = '<div class="cite-text"><div class="t"></div><div class="m"></div></div><button class="cite-x" title="Remove from this citation">✕</button>';
    var rec = state.papers[id];
    row.querySelector('.t').textContent = rec ? (rec.t || 'Untitled') : '…';
    row.querySelector('.m').textContent = rec ? [(rec.f || []).slice(0, 3).join(', ') + ((rec.f || []).length > 3 ? ' et al.' : ''), rec.y].filter(Boolean).join(' · ') : '';
    row.querySelector('.cite-x').onclick = function () { removeFromCitation(id); };
    rows[id] = row;
    list.appendChild(row);
  });
  // fresh details from Lunta (also for papers cited before the document kept their copy)
  var st = LuntaCite.isJournal(style) ? 'apa' : style;
  getJSON('/bibliography', { style: st, ids: current.ids, papers: LuntaCite.detailsFor(state.papers, current.ids) }).then(function (res) {
    if (lastShown !== key) return;
    (res.entries || []).forEach(function (e) {
      var row = rows[e.id]; if (!row) return;
      if (e.missing) { row.querySelector('.t').textContent = 'Not in your library'; row.querySelector('.m').textContent = 'Remove it, or add the paper to Lunta and click Update All.'; return; }
      row.querySelector('.t').textContent = e.title || 'Untitled';
      row.querySelector('.m').textContent = LuntaCite.hitMeta(e);
    });
  }).catch(function () {});
}

var removeFromCitation = function (id) {
  if (!current) return;
  var target = current;
  return exclusive(function () {
    var left = target.ids.filter(function (x) { return x !== id; });
    return Word.run(function (ctx) {
      var cc = ctx.document.contentControls.getById(target.id);
      if (left.length) cc.tag = LuntaCite.makeTag(left);
      else cc.delete(false);   // its last paper: the citation goes, text and all
      return ctx.sync();
    }).then(function () {
      current = left.length ? { id: target.id, ids: left } : null;
      lastShown = '';
      showCurrent();
      return update();
    });
  })();
};

// ---- updating ----

/** What to write: formatted by Lunta, or from the document's copy when Lunta can't be reached. */
function compute(lists) {
  var ids = LuntaCite.citedIds(lists), papers = LuntaCite.detailsFor(state.papers, ids);
  if (LuntaCite.isJournal(style)) {
    return getJSON('/render', { style: style.slice(4), citations: lists, papers: papers }, 90000).then(function (res) {
      if (res.title) state.styleTitle = res.title;
      var f = LuntaCite.formatJournal(res, lists, state.papers);
      f.name = res.title || LuntaCite.styleName(style, state.styleTitle);
      return f;
    });
  }
  var offline = false;
  var ask = ids.length ? getJSON('/bibliography', { style: style, ids: ids, papers: papers }) : Promise.resolve({ entries: [] });
  return ask.catch(function (e) {
    if (e.kind !== 'offline') throw e;
    offline = true;
    return { entries: null };
  }).then(function (res) {
    var f = LuntaCite.format(style, lists, res.entries, state.papers);
    f.superscripts = lists.map(function () { return f.superscript; });
    f.offline = offline;
    f.name = LuntaCite.styleName(style);
    return f;
  });
}

function squash(s) { return String(s || '').replace(/\s+/g, ''); }

function bibliographyHTML(entries) {
  if (!entries.length) return '<p>No citations yet.</p>';
  var hanging = !LuntaCite.isJournal(style) && !(LuntaCite.STYLES[style] || {}).numeric;
  var css = hanging ? 'margin:0 0 6pt 0.5in;text-indent:-0.5in' : 'margin:0 0 6pt 0';
  return entries.map(function (e) { return '<p style="' + css + '">' + LuntaCite.toHTML(e.text) + '</p>'; }).join('');
}

function update() {
  if (styleUnknown) {
    return Promise.reject(new Error('This document’s citation style wasn’t saved with it. Choose its style in the menu above first (Update All would otherwise rewrite every citation in ' + LuntaCite.styleName(style) + ').'));
  }
  status('Updating…');
  return Word.run(function (ctx) {
    var all = ctx.document.contentControls;
    all.load('items/id,items/tag,items/text');
    return ctx.sync().then(function () {
      var bibs = all.items.filter(function (c) { return c.tag === LuntaCite.BIB_TAG; });
      // citations inside the bibliography go when it is rewritten: not counted
      var inner = bibs.map(function (b) { var cs = b.contentControls; cs.load('items/id'); return cs; });
      var cands = all.items.filter(function (c) { return LuntaCite.parseTag(c.tag) !== null; });
      cands.forEach(function (c) { c.font.load('superscript'); });
      return ctx.sync().then(function () {
        var nested = {};
        inner.forEach(function (cs) { cs.items.forEach(function (c) { nested[c.id] = true; }); });
        var cites = [], lists = [], empty = 0;
        cands.forEach(function (c) {
          var ids = LuntaCite.parseTag(c.tag);
          if (nested[c.id] || !ids.length) return;
          // its text deleted (the empty control is invisible): not a citation any more
          if (!c.text.replace(/[\s​]/g, '')) { empty++; return; }
          cites.push(c); lists.push(ids);
        });
        return compute(lists).then(function (f) {
          if (f.unknown.length) {
            var first = lists.findIndex(function (l) { return l.some(function (id) { return f.unknown.indexOf(id) >= 0; }); });
            if (first >= 0) cites[first].select();
            return ctx.sync().then(function () {
              throw new Error(LuntaCite.unknownMessage(f.unknown.length, f.offline));
            });
          }
          var changed = 0;
          cites.forEach(function (c, i) {
            var t = f.texts[i], sup = !!f.superscripts[i];
            if (!t) return;
            if (c.text === t && c.font.superscript === sup) return;
            var r = c.insertText(t, 'Replace');
            r.font.superscript = sup;
            changed++;
          });
          var plainBib = f.bibliography.length ? f.bibliography.map(function (e) { return LuntaCite.plain(e.text); }).join('') : 'No citations yet.';
          bibs.forEach(function (b) {
            if (state.bibStyle === style && squash(b.text) === squash(plainBib)) return;
            b.insertHtml(bibliographyHTML(f.bibliography), 'Replace');
            changed++;
          });
          return ctx.sync().then(function () {
            state.style = style;
            state.papers = f.store;
            if (bibs.length) state.bibStyle = style;
            remember(style);
            return saveState().then(function () { return ''; }, function (e) {
              if (window.console) console.error('saving the document’s copy of its papers', e);
              return ' · (couldn’t save the copy of the papers in the document)';
            });
          }).then(function (warn) {
            status(LuntaCite.summary(cites.length, f.references, f.name, f.fromDocument, f.offline) +
                   (empty ? ' · ' + empty + ' empty citation' + (empty === 1 ? '' : 's') + ' left out' : '') + warn);
          });
        });
      });
    });
  });
}

var updateAll = exclusive(function () { return update(); });

// ---- Resolve Citations: placeholders typed while writing, made citations one by one ----
// The panel lists the placeholders in the text (not in citations: Lunta's citations are content
// controls) with Lunta's candidates; nothing is written until the writer confirms (resolve.js).

var resolver = null;
var resolved = [];   // placeholders written: {text, occurrence}, so the next of the same text is found

/** The placeholders in the document's text, in order: {text, cites, occurrence (among the same text, outside citations)}. */
function scanPlaceholders() {
  return Word.run(function (ctx) {
    var paras = ctx.document.body.paragraphs;
    paras.load('items/text');
    return ctx.sync().then(function () {
      return getJSON('/placeholders', { texts: paras.items.map(function (p) { return p.text; }) });
    }).then(function (res) {
      var found = [], byText = {};
      (res.placeholders || []).forEach(function (list) {
        (list || []).forEach(function (ph) { found.push(ph); if (!byText[ph.text]) byText[ph.text] = ph; });
      });
      // where each text is outside a citation: Word's search, then the content control around each hit
      var texts = Object.keys(byText).filter(function (t) { return t.length <= 255; });
      var searches = texts.map(function (t) {
        var hits = ctx.document.body.search(t, { matchCase: true });
        hits.load('items/text');
        return hits;
      });
      return ctx.sync().then(function () {
        var parents = searches.map(function (hits) {
          return hits.items.map(function (h) { var p = h.parentContentControlOrNullObject; p.load('isNullObject'); return p; });
        });
        return ctx.sync().then(function () {
          var plain = {};
          texts.forEach(function (t, n) { plain[t] = parents[n].filter(function (p) { return p.isNullObject; }).length; });
          var used = {}, out = [];
          found.forEach(function (ph) {
            var u = used[ph.text] || 0;
            if (u >= (plain[ph.text] || 0)) return;   // a citation's own text
            used[ph.text] = u + 1;
            out.push({ text: ph.text, cites: ph.cites, kind: ph.kind, occurrence: u });
          });
          return out;
        });
      });
    });
  });
}

/** Writes the citation of `ids` where the placeholder is (and the parts left as written after it), then Update All. */
function writePlaceholder(ph, ids, leftover) {
  if (busy) return Promise.reject(new Error('Lunta is updating the document. Try again in a moment.'));
  busy = true; setButtons();
  var before = resolved.filter(function (r) { return r.text === ph.text && r.occurrence < ph.occurrence; }).length;
  return Word.run(function (ctx) {
    var hits = ctx.document.body.search(ph.text, { matchCase: true });
    hits.load('items/text');
    return ctx.sync().then(function () {
      var parents = hits.items.map(function (h) { var p = h.parentContentControlOrNullObject; p.load('isNullObject'); return p; });
      return ctx.sync().then(function () {
        var plain = hits.items.filter(function (h, n) { return parents[n].isNullObject; });
        var r = plain[ph.occurrence - before];
        if (!r) throw new Error('“' + ph.text + '” isn’t in the document as it was (edited meanwhile?). Click Resolve Citations to look again.');
        if (leftover) r.insertText(' ' + leftover, 'After');
        wrapCitation(r, ids);
        return ctx.sync();
      });
    });
  }).then(function () {
    resolved.push({ text: ph.text, occurrence: ph.occurrence });
    return update();
  }).then(function () { busy = false; setButtons(); }, function (e) {
    busy = false; setButtons();
    if (e && e.kind) e = new Error(LuntaCite.describe(e, 'word'));
    throw e;
  });
}

var resolveCitations = exclusive(function () {
  status('Looking for placeholders…');
  return getJSON('/status').then(function (s) {
    if ((s.api || 0) < 3) throw new Error('This version of Lunta can’t resolve citations. Update Lunta and try again.');
    return scanPlaceholders();
  }).then(function (list) {
    $('resolveBox').hidden = false;
    resolved = [];
    resolver = resolver || LuntaResolve.create({
      box: $('rslist'), doc: document,
      status: function (m, err) { status(m, err); },
      api: {
        suggest: function (cite) { return getJSON('/suggest', { cite: cite }, 60000); },
        search: function (q) { return getJSON('/suggest', { query: q }, 60000); },
        add: function (cand) { return getJSON('/add', { meta: cand.meta || cand }, 60000); },
        apply: writePlaceholder
      }
    });
    // shown while Lunta looks: the panel stays free for confirming
    resolver.load(list);
  });
});

function closeResolve() {
  $('resolveBox').hidden = true;
  if (resolver) resolver.load([]);
  $('rslist').innerHTML = '';
}

// ---- styles ----

function remember(s) { try { localStorage.setItem('luntaLastStyle', s); } catch (e) {} }
function lastStyle() { try { var s = localStorage.getItem('luntaLastStyle'); return LuntaCite.isStyle(s) ? s : 'apa'; } catch (e) { return 'apa'; } }

function fillStyles() { LuntaCite.fillStyles($('style'), style, state.styleTitle); }

var setStyle = function (s, title) {
  if (busy) { fillStyles(); return; }   // the menu back to the document's style
  return exclusive(function () {
    style = s;
    styleUnknown = false;
    state.style = s;
    state.styleTitle = title || (LuntaCite.isJournal(s) ? state.styleTitle : '');
    fillStyles();
    return update();
  })();
};

function findJournal(q) {
  var box = $('jresults');
  if (!q.trim()) { box.innerHTML = ''; return; }
  box.innerHTML = '<p class="status">Searching…</p>';
  getJSON('/styles?q=' + encodeURIComponent(q), null, 60000).then(function (hits) {
    LuntaCite.showJournals(box, hits, function (s, title) {
      $('journal').hidden = true; $('jq').value = ''; box.innerHTML = '';
      setStyle(s, title);
    });
  }).catch(function (e) { box.innerHTML = ''; fail(e); });
}

Office.onReady(function () {
  $('ver').textContent = 'v' + VERSION;
  var sel = $('style');
  fillStyles();
  setButtons();
  sel.onchange = function () {
    if (sel.value === '__find') {
      sel.value = style;
      $('journal').hidden = false; $('jq').focus();
      return;
    }
    setStyle(sel.value);
  };
  var jt = null;
  $('jq').addEventListener('input', function (e) { clearTimeout(jt); jt = setTimeout(function () { findJournal(e.target.value); }, 300); });
  $('jclose').onclick = function () { $('journal').hidden = true; $('jq').value = ''; $('jresults').innerHTML = ''; };
  var t = null;
  $('q').addEventListener('input', function (e) { clearTimeout(t); t = setTimeout(function () { search(e.target.value); }, 220); });
  $('insert').onclick = insertCitation;
  $('bib').onclick = insertBibliography;
  $('refresh').onclick = updateAll;
  $('resolve').onclick = resolveCitations;
  $('rsclose').onclick = closeResolve;
  // clicking a citation in the text shows its papers here
  try {
    Office.context.document.addHandlerAsync(Office.EventType.DocumentSelectionChanged, onSelection, function () {});
  } catch (e) {}
  status('Loading…');
  // the document's style before anything may be updated
  loadState().then(function (st) {
    state = st;
    if (LuntaCite.isStyle(st.style)) { style = st.style; return; }
    style = lastStyle();
    return Word.run(function (ctx) {
      var all = ctx.document.contentControls;
      all.load('items/tag');
      return ctx.sync().then(function () {
        styleUnknown = all.items.some(function (c) { return LuntaCite.parseTag(c.tag); });
      });
    });
  }).then(function () {
    fillStyles();
    ready = true; setButtons();
    if (styleUnknown) {
      status('This document has citations but its style wasn’t saved with it (it was made with another copy of the add-in). Choose its style in the menu above.', true);
      return;
    }
    return getJSON('/status').then(function (s) { status('Connected to Lunta · ' + s.papers + ' papers'); }, function (e) {
      if (e.kind === 'offline' && Object.keys(state.papers).length) status('Lunta isn’t running. Update All still works from the copy of the cited papers saved in this document; to search and cite, open Lunta.', true);
      else fail(e);
    });
  }).catch(fail);
  $('q').focus();
});
