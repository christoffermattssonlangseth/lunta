// Lunta citations for Word: citations are content controls tagged "margins:<id>,<id>", the
// bibliography is a content control tagged "margins-bib". "Update All" renumbers and reformats
// every citation and rewrites the bibliography; text that is already right is left alone.
// A citation's options (each paper's locator, prefix, suffix, author left out or in the text) are
// in its content control's title, "Citation lunta-o:<code>" (common.js), so add-ins from before
// them still read its papers from the tag; they format it without the options and leave the title.
// The document keeps its style and a copy of the papers it cites in a custom XML part (shared by
// every copy of the add-in, unlike add-in settings), so it updates the same on any computer.
// Served by Lunta itself (https://localhost:47481) or from the hosted copy (GitHub Pages, the
// Microsoft store listing): either way the library comes from the Lunta app on this computer.
var LUNTA = 'https://localhost:47481';
var API = (location.origin === LUNTA ? '' : LUNTA) + '/api';
var VERSION = '13.5';   // bump with the ?v= in index.html, so Word loads the new files
var NS = 'https://lunta.app/ns/word-citations/1';
var picked = [];          // [{id, title, ...}]
var style = 'apa';
var state = { papers: {} };   // what the document keeps: {style, styleTitle, bibStyle, papers}
var styleUnknown = false;     // citations in the document, but no saved style: ask before updating
var busy = false, ready = false, lastSearch = 0;
var current = null;           // the citation the cursor is in: {id (content control), ids, opts}
var editing = null;           // the paper of the current citation whose options are open
// Word for Mac may not answer at all while it saves the document: every round trip to Word has a
// time limit, so the panel never waits for ever (busy, the buttons grey, a click never looked at)
var WORD_WAIT = 30000;        // ms for one round trip to Word (Lunta's requests have their own)
var RETRY_WAIT = 500;         // ms before Word is asked again after an error
var POLL_MS = 1000;           // the cursor looked at this often while the panel shows ...
var QUIET_MS = 3000;          // ... and Word hasn't said the selection changed for this long
var STALLED = 'Lunta’s update stopped (Word didn’t answer). Try again.';

function $(id) { return document.getElementById(id); }
function status(msg, err) { var s = $('status'); s.textContent = msg || ''; s.className = 'status' + (err ? ' err' : ''); }
// Word's errors say little ("Check the OfficeExtension.Error.debugInfo"): show where and what
function fail(e) {
  if (e && e.kind) { status(LuntaCite.describe(e, 'word'), true); return; }
  if (window.console) console.error(e, e && e.debugInfo);
  status(isWordError(e) ? 'Word stopped Lunta: ' + wordText(e) + '. Try again.' : (e && e.message || String(e)), true);
}
/** An error from Word (not Lunta's, not the panel's own message). */
function isWordError(e) { return !!e && !e.kind && !e.stalled && (e.name === 'OfficeExtension.Error' || !!e.code); }
/** Word's error in words: its message, code and where ("The action isn't supported · GeneralException · ContentControl.getById"). */
function wordText(e) {
  var d = e && e.debugInfo, more = '';
  try { more = d && d.errorLocation ? d.errorLocation : ''; } catch (x) {}
  return [e && e.message || String(e), e && e.code && e.code !== e.message ? e.code : '', more].filter(Boolean).join(' · ');
}

/** setTimeout that doesn't keep a test's process alive. */
function later(fn, ms) { var t = setTimeout(fn, ms); if (t && t.unref) t.unref(); return t; }
function stalled() { var e = new Error(STALLED); e.stalled = true; return e; }

/**
 * Word.run with a time limit on every wait for Word (the start, each sync, the end; not Lunta's
 * requests in between, which have their own) and, if `retry`, Word's errors (GeneralException or
 * ItemNotFound after a save, "The action isn't supported" in Word for Mac) asked once more after a
 * moment: only for what can be done twice (reading, Update All), never for an insertion.
 */
function wordRun(fn, retry) {
  var once = function () {
    return new Promise(function (ok, no) {
      var over = false, timer = null;
      var stop = function () { if (timer) clearTimeout(timer); timer = null; };
      var wait = function () { stop(); timer = later(function () { over = true; no(stalled()); }, WORD_WAIT); };
      wait();
      var run;
      try {
        run = Word.run(function (ctx) {
          stop();
          if (over) throw stalled();
          var sync = ctx.sync;
          ctx.sync = function () {
            wait();
            return sync.apply(ctx, arguments).then(function (v) { stop(); return v; }, function (e) { stop(); throw e; });
          };
          var out = fn(ctx);
          // Word's last round trip, once the batch is done
          return Promise.resolve(out).then(function (v) { wait(); return v; }, function (e) { wait(); throw e; });
        });
      } catch (e) { stop(); no(e); return; }
      Promise.resolve(run).then(function (v) { stop(); if (!over) ok(v); }, function (e) { stop(); if (!over) no(e); });
    });
  };
  return once().catch(function (e) {
    if (!retry || !isWordError(e)) throw e;
    if (window.console) console.error('Word: ' + wordText(e) + ' (asking again)', e);
    return new Promise(function (r) { later(r, RETRY_WAIT); }).then(once);
  });
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
  if ($('convert')) $('convert').disabled = busy || !ready;
}

/** One change to the document at a time (a second click while updating does nothing). */
function exclusive(fn) {
  return function () {
    if (!ready) { status('Lunta is still loading this document. Wait a moment, or close and reopen the Lunta panel.', true); return; }
    if (busy) { status('Lunta is still changing the document. Wait a moment.'); return; }
    busy = true; setButtons();
    // the buttons back whatever happens (every wait for Word or Lunta has a time limit)
    return Promise.resolve().then(fn).catch(fail).then(idle, idle);
  };
}

/** Done changing the document: the buttons back, and a click in the text meanwhile looked at now. */
function idle() {
  busy = false; setButtons();
  if (selPending) lookAtSelection();
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
    // Word may not call back while it saves: never wait for ever
    var t = later(function () { no(stalled()); }, WORD_WAIT);
    fn(function (r) { clearTimeout(t); if (r.status === Office.AsyncResultStatus.Succeeded) ok(r.value); else no(r.error); });
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
  return wordRun(function (ctx) {
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
        parent.tag = LuntaCite.makeTag(had.concat(ids));   // its title (the options) stays
        return ctx.sync();
      }
      // at the end of the selection, so selected text isn't replaced
      wrapCitation(sel.getRange('End'), ids);
      return ctx.sync();
    });
  }).then(function () {
    picked = []; renderPicked();
    if (current) { current.ids = LuntaCite.unique(current.ids.concat(ids)); current.tag = LuntaCite.makeTag(current.ids); lastShown = ''; showCurrent(); }
    return update().catch(function (e) {
      var err = new Error(LuntaCite.describe(e, 'word') + ' The citation is in the document as “(…)”; Update All fills it in.');
      throw err;
    });
  });
});

/** A new citation of `ids` in place of the range: text first, then the content control around
 *  it (Word on the web won't wrap an empty range). Update All fills in its text. */
function wrapCitation(range, ids, options) {
  var cc = range.insertText('(…)', 'Replace').insertContentControl();
  dressCitation(cc, ids, options);
  return cc;
}

/** A content control made a Lunta citation of `ids` with these options. */
function dressCitation(cc, ids, options) {
  cc.tag = LuntaCite.makeTag(ids);
  cc.title = LuntaCite.optionsTitle(LuntaCite.cleanOptions(options, ids));
  cc.appearance = 'Hidden';
  return cc;
}

var insertBibliography = exclusive(function () {
  var existed = false;
  return wordRun(function (ctx) {
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

// Word says when the selection changes, not where it is when the panel opens: the citation under
// the cursor is looked at once the panel is ready, and a click while it was loading or changing the
// document (Update All) is looked at when it is done, not dropped. Before, a citation clicked before
// opening the panel, or while it loaded, showed nothing until Update All rewrote the text under the
// cursor (and Word said the selection had changed).
// A click is looked at at once, with one round trip to Word (each is slow in Word for Mac): the
// control the cursor is in, with its tag and title. A control inside a citation (one another add-in
// left in it, as in documents converted from ReadCube Papers) is found in `inside`, read with the
// document's citations when the panel opens and at every Update All, not by asking Word for its
// parents one by one. Clicks while one is looked at are looked at once it is done, the last one only.
// After a save, Word for Mac may stop saying when the selection changes (the card kept the citation
// clicked before the save), may not answer a round trip at all, and may give the controls new ids.
// So: every wait for Word has a time limit (wordRun) and the card says when it couldn't read the
// citation; the cursor is also looked at every second while the panel shows and Word has said
// nothing for a while (when the cursor moved without Word saying so, its handler is registered
// again), and the handler is registered again when the panel shows or gets the focus; a control
// remembered in `inside` or `outside` is used only if its tag, title and text are still the same,
// else Word is asked for its parents as for one not seen.
var lastShown = '', selPending = false, selSeq = 0, selRunning = false, selAgain = false;
var lastEvent = 0;  // when Word last said the selection changed
var seen = {};      // papers' details Lunta gave for the citation box this session: {id: entry}
var inside = {};    // a control inside a Lunta citation: {control id: {cite: {id, tag, title} of the citation, key}}
var outside = {};   // controls not in a citation (asked Word once): {control id: key}
var watching = null;   // the selection handler registered: null while Word hasn't answered
/** What a remembered control must still be: its tag, title and text. */
function controlKey(c) { return [c.tag || '', c.title || '', c.text || ''].join('\u0001'); }
/** Word said the selection changed. */
function onSelection() {
  lastEvent = Date.now();
  lookAtSelection();
}
function lookAtSelection() {
  if (busy || !ready) { selPending = true; return; }
  if (selRunning) { selAgain = true; return; }
  checkSelection();
}
/** Looks at the citation under the cursor; resolves true if the card changed. `quiet`: not a click
 *  (the poll), so a failure isn't shown in the card. */
function checkSelection(quiet) {
  if (busy || !ready) { selPending = true; return Promise.resolve(false); }
  selPending = false; selAgain = false; selRunning = true;
  var mine = ++selSeq;
  var before = current ? current.id + ':' + current.tag + ':' + current.title : '';
  return wordRun(function (ctx) {
    var cc = ctx.document.getSelection().parentContentControlOrNullObject;
    cc.load('isNullObject,id,tag,title,text');
    return ctx.sync().then(function () { return citationAt(ctx, cc, 0); });
  }, true).then(function (cc) {
    if (mine !== selSeq) return false;   // a later click answered first
    var ids = cc ? LuntaCite.parseTag(cc.tag) : null;
    current = ids && ids.length ? { id: cc.id, ids: ids, opts: LuntaCite.optionsOfTitle(cc.title) || {}, tag: cc.tag, title: cc.title } : null;
    if (!current) editing = null;
    showCurrent();
    return before !== (current ? current.id + ':' + current.tag + ':' + current.title : '');
  }).catch(function (e) {
    if (window.console) console.error('reading the citation under the cursor', e);
    if (mine === selSeq && !quiet) cardError(e);
    return false;
  }).then(function (changed) {
    selRunning = false;
    if (selAgain || selPending) lookAtSelection();
    return changed;
  });
}

/** The card says it couldn't read the citation under the cursor, rather than showing the last one. */
function cardError(e) {
  current = null; editing = null; lastShown = '';
  var list = $('citelist');
  list.innerHTML = '';
  var row = document.createElement('div');
  row.className = 'status err';
  row.textContent = 'Couldn’t read the citation here (' + (e && e.stalled ? 'Word didn’t answer' : 'Word: ' + wordText(e)) + '). Click it again.';
  list.appendChild(row);
  $('current').hidden = false;
  setButtons();
}

/** The Lunta citation the loaded control `cc` is or is inside, {id, tag, title}, or null.
 *  Word is asked for its parents only for a control not seen (or changed since) when the citations were read. */
function citationAt(ctx, cc, depth) {
  if (cc.isNullObject) return null;
  if (LuntaCite.parseTag(cc.tag)) return { id: cc.id, tag: cc.tag, title: cc.title };
  if (cc.tag === LuntaCite.BIB_TAG || depth >= 3) return null;
  var key = controlKey(cc);
  if (inside[cc.id] && inside[cc.id].key === key) return inside[cc.id].cite;
  if (outside[cc.id] === key) return null;
  var parent = cc.parentContentControlOrNullObject;
  parent.load('isNullObject,id,tag,title,text');
  return ctx.sync().then(function () {
    var found = citationAt(ctx, parent, depth + 1);
    return Promise.resolve(found).then(function (c) {
      delete inside[cc.id]; delete outside[cc.id];
      if (c) inside[cc.id] = { cite: c, key: key }; else outside[cc.id] = key;
      return c;
    });
  });
}

/** Remembers the document's citations' inner controls (`inside`): `cites` are the loaded Lunta
 *  citation controls, `inner` their loaded contentControls collections (items/id,tag,title,text). */
function rememberInside(cites, inner) {
  inside = {}; outside = {};
  cites.forEach(function (c, i) {
    var cite = { id: c.id, tag: c.tag, title: c.title };
    (inner[i] && inner[i].items || []).forEach(function (x) { inside[x.id] = { cite: cite, key: controlKey(x) }; });
  });
}

/** The selection handler registered (`again`: removed first, as Word for Mac may lose it on a save). */
function watch(again) {
  try {
    var d = Office.context.document, type = Office.EventType.DocumentSelectionChanged;
    var added = false;
    var add = function () {
      if (added) return;
      added = true;
      d.addHandlerAsync(type, onSelection, function (r) {
        watching = !r || r.status === Office.AsyncResultStatus.Succeeded;
        if (!watching && window.console) console.error('Word refused the selection handler', r && r.error);
      });
    };
    if (again && d.removeHandlerAsync) {
      d.removeHandlerAsync(type, { handler: onSelection }, add);
      later(add, 2000);   // Word may not call back
    } else add();
  } catch (e) { watching = false; }
}

/** Looks at the cursor every POLL_MS while the panel shows, when Word hasn't said for QUIET_MS. */
function poll() {
  if (!POLL_MS) return;
  later(function () {
    var shown = !document.visibilityState || document.visibilityState === 'visible';
    if (!POLL_MS) return;
    if (!ready || busy || selRunning || !shown || Date.now() - lastEvent < QUIET_MS) { poll(); return; }
    checkSelection(true).then(function (changed) {
      // the cursor moved and Word didn't say: its handler is lost
      if (changed && Date.now() - lastEvent >= QUIET_MS) watch(true);
    }).then(poll, poll);
  }, POLL_MS);
}

/** One line per paper of the current citation: from Lunta, or from the document's copy. */
function showCurrent() {
  var box = $('current');
  setButtons();
  if (!current) { box.hidden = true; lastShown = ''; return; }
  var key = current.id + ':' + current.ids.join(',') + ':' + LuntaCite.encodeOptions(current.opts) + ':' + (editing || '');
  box.hidden = false;
  if (key === lastShown) return;
  lastShown = key;
  var list = $('citelist');
  list.innerHTML = '';
  var rows = {};
  current.ids.forEach(function (id) {
    var row = document.createElement('div');
    row.className = 'cite-row';
    row.innerHTML = '<div class="cite-text"><div class="t"></div><div class="m"></div><div class="o"></div></div>' +
      '<button class="cite-o" title="Page, prefix, suffix, author (for this paper in this citation)">Page…</button><button class="cite-x" title="Remove from this citation">✕</button>';
    // at once: what Lunta gave this session, else the document's copy; fresh from Lunta below
    var rec = state.papers[id], got = seen[id];
    row.querySelector('.t').textContent = got ? (got.title || 'Untitled') : rec ? (rec.t || 'Untitled') : '…';
    row.querySelector('.m').textContent = got ? LuntaCite.hitMeta(got) : rec ? [(rec.f || []).slice(0, 3).join(', ') + ((rec.f || []).length > 3 ? ' et al.' : ''), rec.y].filter(Boolean).join(' · ') : '';
    row.querySelector('.o').textContent = LuntaCite.optionsLabel((current.opts || {})[id]);
    row.querySelector('.cite-x').onclick = function () { removeFromCitation(id); };
    row.querySelector('.cite-o').onclick = function () { editing = editing === id ? null : id; lastShown = ''; showCurrent(); };
    rows[id] = row;
    list.appendChild(row);
    if (editing === id) {
      var form = LuntaCite.optionsForm(document, (current.opts || {})[id], function (o) { setPaperOptions(id, o); },
        function () { editing = null; lastShown = ''; showCurrent(); });
      list.appendChild(form);
      if (form.focusFirst) setTimeout(form.focusFirst, 0);
    }
  });
  // fresh details from Lunta, once a session (also for papers cited before the document kept their copy)
  var ask = current.ids.filter(function (id) { return !seen[id]; });
  if (!ask.length) return;
  var st = LuntaCite.isJournal(style) ? 'apa' : style;
  getJSON('/bibliography', { style: st, ids: ask, papers: LuntaCite.detailsFor(state.papers, ask) }).then(function (res) {
    (res.entries || []).forEach(function (e) { if (e && e.id && !e.missing) seen[e.id] = e; });
    if (lastShown !== key) return;
    var put = function (el, text) { if (el.textContent !== text) el.textContent = text; };   // no flicker
    (res.entries || []).forEach(function (e) {
      var row = rows[e.id]; if (!row) return;
      if (e.missing) { put(row.querySelector('.t'), 'Not in your library'); put(row.querySelector('.m'), 'Remove it, or add the paper to Lunta and click Update All.'); return; }
      put(row.querySelector('.t'), e.title || 'Untitled');
      put(row.querySelector('.m'), LuntaCite.hitMeta(e));
    });
  }).catch(function (e) {
    // rows with nothing from the document's copy say why
    if (lastShown !== key) return;
    Object.keys(rows).forEach(function (id) {
      var t = rows[id].querySelector('.t');
      if (t.textContent === '…') { t.textContent = 'Details not available'; rows[id].querySelector('.m').textContent = LuntaCite.describe(e, 'word'); }
    });
  });
}

/** The current citation's control in the document: by its id, or (Word gave the controls new ids,
 *  after a save) the one citation with the same tag and title. */
function findCitation(ctx, target) {
  var all = ctx.document.contentControls;
  all.load('items/id,items/tag,items/title');
  return ctx.sync().then(function () {
    var cites = all.items.filter(function (c) { return LuntaCite.parseTag(c.tag); });
    var byId = cites.filter(function (c) { return c.id === target.id; })[0];
    if (byId) return byId;
    var same = cites.filter(function (c) { return c.tag === target.tag && c.title === target.title; });
    if (same.length === 1) return same[0];
    throw new Error('That citation isn’t where Lunta saw it (the document changed or was saved meanwhile). Click it again, then try again.');
  });
}

var removeFromCitation = function (id) {
  if (!current) return;
  var target = current;
  return exclusive(function () {
    var left = target.ids.filter(function (x) { return x !== id; });
    var opts = LuntaCite.cleanOptions(target.opts, left);
    var id2 = target.id;
    return wordRun(function (ctx) {
      return findCitation(ctx, target).then(function (cc) {
        id2 = cc.id;
        if (left.length) { cc.tag = LuntaCite.makeTag(left); cc.title = LuntaCite.optionsTitle(opts); }
        else cc.delete(false);   // its last paper: the citation goes, text and all
        return ctx.sync();
      });
    }, true).then(function () {
      current = left.length ? { id: id2, ids: left, opts: opts || {}, tag: LuntaCite.makeTag(left), title: LuntaCite.optionsTitle(opts) } : null;
      editing = null;
      lastShown = '';
      showCurrent();
      return update();
    });
  })();
};

/** Sets one paper's options in the current citation (the content control's title), then Update All. */
var setPaperOptions = function (id, o) {
  if (!current) return;
  var target = current;
  return exclusive(function () {
    var map = Object.assign({}, target.opts || {});
    map[id] = o;
    map = LuntaCite.cleanOptions(map, target.ids);
    var id2 = target.id;
    return wordRun(function (ctx) {
      return findCitation(ctx, target).then(function (cc) {
        id2 = cc.id;
        cc.title = LuntaCite.optionsTitle(map);
        return ctx.sync();
      });
    }, true).then(function () {
      current = { id: id2, ids: target.ids, opts: map || {}, tag: target.tag, title: LuntaCite.optionsTitle(map) };
      editing = null; lastShown = '';
      showCurrent();
      return update();
    });
  })();
};

// ---- updating ----

/** What to write: formatted by Lunta, or from the document's copy when Lunta can't be reached. */
function compute(lists, options) {
  var ids = LuntaCite.citedIds(lists), papers = LuntaCite.detailsFor(state.papers, ids);
  if (LuntaCite.isJournal(style)) {
    var body = { style: style.slice(4), citations: lists, papers: papers }, opts = LuntaCite.renderOptions(options, lists);
    if (opts) body.options = opts;
    return getJSON('/render', body, 90000).then(function (res) {
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
    var f = LuntaCite.format(style, lists, res.entries, state.papers, options);
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

/**
 * Every entry of a rewritten bibliography formatted alike. Word gives the HTML's paragraphs its own
 * font (Times New Roman) and spacing but keeps the control's last paragraph mark, with the font and
 * spacing of what was there before (another manager's bibliography, the theme's font): the last
 * entry came out in another font. All take the first entry's paragraph spacing and indents, and
 * the font of the text the citations are in (the first entry's when that can't be read).
 */
function evenBibliography(ctx, bibs, cite) {
  if (!bibs.length) return Promise.resolve();
  var body = null;
  if (cite && cite.paragraphs) {
    body = cite.paragraphs.getFirst();
    body.load('font/name,font/size,font/color');
  }
  var lists = bibs.map(function (b) {
    var ps = b.paragraphs;
    if (!ps) return null;
    ps.load('items/font/name,items/font/size,items/font/color,items/lineSpacing,items/spaceAfter,items/spaceBefore,items/leftIndent,items/firstLineIndent,items/alignment');
    return ps;
  });
  return ctx.sync().then(function () {
    lists.forEach(function (ps) {
      if (!ps || ps.items.length < 2) return;
      var first = ps.items[0];
      var font = {
        name: (body && body.font.name) || first.font.name,
        size: (body && body.font.size) || first.font.size,
        color: (body && body.font.color) || first.font.color
      };
      ps.items.forEach(function (p) {
        ['lineSpacing', 'spaceAfter', 'spaceBefore', 'leftIndent', 'firstLineIndent', 'alignment'].forEach(function (k) {
          if (first[k] !== null && first[k] !== undefined && p[k] !== first[k]) p[k] = first[k];
        });
        ['name', 'size', 'color'].forEach(function (k) {
          if (font[k] && p.font[k] !== font[k]) p.font[k] = font[k];
        });
      });
    });
    return ctx.sync();
  });
}

function update() {
  if (styleUnknown) {
    return Promise.reject(new Error('This document’s citation style wasn’t saved with it. Choose its style in the menu above first (Update All would otherwise rewrite every citation in ' + LuntaCite.styleName(style) + ').'));
  }
  status('Updating…');
  return wordRun(function (ctx) {
    var all = ctx.document.contentControls;
    all.load('items/id,items/tag,items/text,items/title');
    return ctx.sync().then(function () {
      var bibs = all.items.filter(function (c) { return c.tag === LuntaCite.BIB_TAG; });
      // citations inside the bibliography go when it is rewritten: not counted
      var inner = bibs.map(function (b) { var cs = b.contentControls; cs.load('items/id'); return cs; });
      var cands = all.items.filter(function (c) { return LuntaCite.parseTag(c.tag) !== null; });
      cands.forEach(function (c) { c.font.load('superscript'); });
      var within = cands.map(function (c) { var cs = c.contentControls; cs.load('items/id,items/tag,items/title,items/text'); return cs; });
      return ctx.sync().then(function () {
        rememberInside(cands, within);
        var nested = {};
        inner.forEach(function (cs) { cs.items.forEach(function (c) { nested[c.id] = true; }); });
        var cites = [], lists = [], options = [], empty = 0;
        cands.forEach(function (c) {
          var ids = LuntaCite.parseTag(c.tag);
          if (nested[c.id] || !ids.length) return;
          // its text deleted (the empty control is invisible): not a citation any more
          if (!c.text.replace(/[\s​]/g, '')) { empty++; return; }
          cites.push(c); lists.push(ids); options.push(LuntaCite.optionsOfTitle(c.title));
        });
        return compute(lists, options).then(function (f) {
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
            // a raised citation's prefix and suffix stay on the line: "see ⁴"
            var before = sup && f.befores ? f.befores[i] || '' : '', after = sup && f.afters ? f.afters[i] || '' : '';
            var mid = t.slice(before.length, t.length - after.length);
            if (!mid) { before = ''; after = ''; mid = t; }
            // Word gives null for a mix of raised and not
            if (c.text === t && c.font.superscript === (before || after ? null : sup)) return;
            var r = c.insertText(mid, 'Replace');
            r.font.superscript = sup;
            if (before) c.insertText(before, 'Start').font.superscript = false;
            if (after) c.insertText(after, 'End').font.superscript = false;
            changed++;
          });
          var rewritten = [];
          var plainBib = f.bibliography.length ? f.bibliography.map(function (e) { return LuntaCite.plain(e.text); }).join('') : 'No citations yet.';
          bibs.forEach(function (b) {
            if (state.bibStyle === style && squash(b.text) === squash(plainBib)) return;
            b.insertHtml(bibliographyHTML(f.bibliography), 'Replace');
            changed++;
            rewritten.push(b);
          });
          return ctx.sync().then(function () {
            // formatting only: a Word without these properties keeps the entries as written
            return evenBibliography(ctx, rewritten, cites[0]).catch(function (e) {
              if (window.console) console.error('formatting the bibliography', e);
            });
          }).then(function () {
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
  }, true);
}

var updateAll = exclusive(function () { return update(); });

// ---- Resolve Citations: placeholders typed while writing, made citations one by one ----
// The panel lists the placeholders in the text (not in citations: Lunta's citations are content
// controls) with Lunta's candidates; nothing is written until the writer confirms (resolve.js).

var resolver = null;
var resolved = [];   // placeholders written: {text, occurrence}, so the next of the same text is found

/** The placeholders in the document's text, in order: {text, cites, occurrence (among the same text, outside citations)}. */
function scanPlaceholders() {
  return wordRun(function (ctx) {
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
  }, true);
}

/** Writes the citation of `ids` where the placeholder is (and the parts left as written after it), then Update All. */
function writePlaceholder(ph, ids, leftover) {
  if (busy) return Promise.reject(new Error('Lunta is updating the document. Try again in a moment.'));
  busy = true; setButtons();
  var before = resolved.filter(function (r) { return r.text === ph.text && r.occurrence < ph.occurrence; }).length;
  return wordRun(function (ctx) {
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
  }).then(idle, function (e) {
    idle();
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

// ---- Convert Citations from Zotero, EndNote or Mendeley ----
// The other manager's citations are read (word/fields.js: Word's fields in the body's OOXML, and
// Mendeley Cite's content controls), their works found in the library (/match), the writer
// reviews what will be added, and only then are the citations replaced, one by one: a field is
// selected (all of it, codes and shown text) and replaced by a Lunta citation (a content control
// with the papers and the options the other manager kept); a Mendeley Cite control gets Lunta's tag.
// The other manager's bibliography becomes Lunta's (or goes, if there is one already) once none of
// its citations is left. Then Update All.

var conversion = null;   // {plan, found: [{ref, works, text, source}], bibs: [{ref, source}]}

/** The other managers' citations and bibliographies in the document, in order. */
function readForeign(ctx) {
  var body = ctx.document.body;
  var ooxml = body.getOoxml();
  var fields = body.fields;
  fields.load('items/code');
  var ccs = ctx.document.contentControls;
  ccs.load('items/id,items/tag,items/text');
  return ctx.sync().then(function () {
    var scan = LuntaFields.scan(ooxml.value);
    // unreadable: how many citations can't be converted, per manager (its bibliography stays then)
    var out = { cites: [], bibs: [], notes: [], unreadable: 0, unreadableBy: {}, fieldsOK: true };
    var unreadable = function (source) { out.unreadable++; out.unreadableBy[source] = (out.unreadableBy[source] || 0) + 1; };
    // Word's fields in the same order as the OOXML's; a field whose code differs isn't touched
    var k = 0;
    scan.fields.forEach(function (f) {
      var c = LuntaFields.classify(f.instr, f.data);
      // only the citations and bibliographies are looked for (Word may not list nested fields)
      if (!c || c.kind === 'data') return;
      var at = -1;
      for (var j = k; j < fields.items.length; j++) {
        // Word may list EndNote's nested EN.CITE.DATA as a field of its own: never a citation
        if (/^\s*ADDIN\s*EN\.CITE\.DATA\b/.test(fields.items[j].code || '')) continue;
        if (LuntaFields.sameCode(fields.items[j].code, f.instr)) { at = j; k = j + 1; break; }
      }
      if (at < 0) { if (c.kind !== 'bib') unreadable(c.source); return; }
      var ref = { field: at, code: fields.items[at].code };
      if (c.kind === 'bib') out.bibs.push({ ref: ref, source: c.source });
      else if (!c.works || !c.works.length) unreadable(c.source);
      else out.cites.push({ ref: ref, works: c.works, text: f.text, source: c.source });
    });
    ccs.items.forEach(function (cc) {
      var c = LuntaFields.classifyTag(cc.tag);
      if (!c) return;
      var ref = { cc: cc.id, tag: cc.tag };
      if (c.kind === 'bib') out.bibs.push({ ref: ref, source: c.source });
      else if (!c.works || !c.works.length) unreadable(c.source);
      else out.cites.push({ ref: ref, works: c.works, text: cc.text, source: c.source });
    });
    if (LuntaFields.zoteroBookmarks(ooxml.value)) {
      out.notes.push('Some Zotero citations here are stored as bookmarks, which Lunta can’t read. In Zotero’s Document Preferences (Zotero tab in Word), choose “Store citations as: Fields”, then convert again.');
    }
    if (out.unreadable) out.notes.push(out.unreadable + ' citation' + (out.unreadable === 1 ? '' : 's') + ' couldn’t be read (no details in it) and will stay as ' + (out.unreadable === 1 ? 'it is' : 'they are') + '.');
    return out;
  });
}

function sourcesOf(list) {
  var names = [];
  list.forEach(function (c) { if (names.indexOf(c.source) < 0) names.push(c.source); });
  return names.join(', ');
}

var convertCitations = exclusive(function () {
  status('Looking for citations from Zotero, EndNote and Mendeley…');
  return getJSON('/status').then(function (s) {
    if ((s.api || 0) < 4) throw new Error('This version of Lunta can’t convert citations. Update Lunta and try again.');
    return wordRun(readForeign, true);
  }).then(function (found) {
    var box = $('convertBox');
    if (!found.cites.length) {
      status(found.notes.length ? found.notes.join(' ') : 'This document has no citations from Zotero, EndNote or Mendeley.', !!found.notes.length);
      return;
    }
    var first = LuntaCite.convertPlan(found.cites);
    status('Finding ' + first.works.length + ' papers in your library…');
    return getJSON('/match', { items: first.works.map(function (w) { return w.meta; }) }, 60000).then(function (r) {
      var plan = LuntaCite.convertPlan(found.cites, r.matches || []);
      conversion = { plan: plan, found: found };
      box.hidden = false;
      var notes = ['From ' + sourcesOf(found.cites) + '.'].concat(found.notes);
      var stays = found.bibs.filter(function (b) { return found.unreadableBy[b.source]; });
      var goes = found.bibs.filter(function (b) { return !found.unreadableBy[b.source]; });
      if (goes.length) notes.push('The ' + sourcesOf(goes) + ' bibliography becomes Lunta’s once all its citations are converted.');
      if (stays.length) notes.push('The ' + sourcesOf(stays) + ' bibliography stays, as some of its citations can’t be read.');
      LuntaCite.convertReview(document, $('cvlist'), plan, notes, function (keys) { runConversion(keys); }, closeConvert);
      status(LuntaCite.convertSummary(plan) + '. Review, then Convert.');
    });
  });
});

function closeConvert() { conversion = null; $('convertBox').hidden = true; $('cvlist').innerHTML = ''; }

/** Adds the papers chosen, then replaces the citations whose papers are all in the library. */
var runConversion = function (keys) {
  var conv = conversion;
  if (!conv) return;
  return exclusive(function () {
    var added = {}, newPapers = 0;
    var chain = Promise.resolve();
    keys.forEach(function (k) {
      var w = conv.plan.works[conv.plan.byKey[k]];
      if (!w) return;
      chain = chain.then(function () {
        status('Adding “' + (w.meta.title || 'a paper') + '” to your library…');
        return getJSON('/add', { meta: w.meta }, 60000).then(function (p) { if (p && p.id) { added[k] = p.id; if (p.added) newPapers++; } });
      });
    });
    return chain.then(function () {
      var res = LuntaCite.convertResolve(conv.plan, added);
      status('Converting ' + res.convert.length + ' citations…');
      var left = res.left.map(function (l) { return { text: conv.found.cites[l.index].text, reason: l.reason, source: conv.found.cites[l.index].source }; });
      return wordRun(function (ctx) {
        return replaceForeign(ctx, conv, res, left);
      }).then(function (done) {
        closeConvert();
        return update().then(function () { return done; }, function (e) { status(LuntaCite.describe(e, 'word'), true); return null; });
      }).then(function (done) {
        if (done === null) return;
        var msg = LuntaCite.convertDone(done, left, newPapers);
        status(msg + ' ' + $('status').textContent, left.length > 0);
      });
    });
  })();
};

/** Replaces the citations (`res.convert`) and the bibliographies; returns how many citations were replaced. */
function replaceForeign(ctx, conv, res, left) {
  var fields = ctx.document.body.fields;
  fields.load('items/code');
  var ours = ctx.document.contentControls.getByTag(LuntaCite.BIB_TAG);
  ours.load('items');
  var done = 0;
  return ctx.sync().then(function () {
    var fieldsOK = typeof Office === 'undefined' || !Office.context.requirements.isSetSupported || Office.context.requirements.isSetSupported('WordApi', '1.5');
    var hasBib = ours.items.length > 0;
    // the last first, so Word's field list keeps its order for the ones before
    var jobs = res.convert.map(function (c) { return { c: c, cite: conv.found.cites[c.index] }; });
    var at = function (ref) { return ref.field != null ? ref.field : -1; };
    jobs.sort(function (a, b) { return at(b.cite.ref) - at(a.cite.ref); });
    var step = Promise.resolve();
    jobs.forEach(function (j) {
      step = step.then(function () {
        var ref = j.cite.ref;
        if (ref.cc !== undefined) {
          dressCitation(ctx.document.contentControls.getById(ref.cc), j.c.ids, j.c.options);
          done++;
          return ctx.sync();
        }
        var f = fields.items[ref.field];
        if (!fieldsOK || !f || !LuntaFields.sameCode(f.code, ref.code)) {
          left.push({ text: j.cite.text, source: j.cite.source, reason: fieldsOK ? 'it changed while converting' : 'this version of Word can’t replace fields (Word 2021, Microsoft 365 or Word on the web can)' });
          return;
        }
        // the whole field (codes and shown text) selected, and replaced by a Lunta citation
        f.select();
        wrapCitation(ctx.document.getSelection(), j.c.ids, j.c.options);
        done++;
        return ctx.sync();
      });
    });
    return step.then(function () {
      // a bibliography goes once none of its manager's citations is left
      var leftSources = {};
      left.forEach(function (l) { leftSources[l.source] = true; });
      Object.keys(conv.found.unreadableBy || {}).forEach(function (s) { leftSources[s] = true; });
      var bibs = conv.found.bibs.filter(function (b) { return !leftSources[b.source]; });
      var keptBibs = conv.found.bibs.filter(function (b) { return leftSources[b.source]; });
      if (keptBibs.length) left.push({ text: '', source: '', reason: 'the ' + sourcesOf(keptBibs) + ' bibliography stays until its citations are converted' });
      bibs.sort(function (a, b) { return at(b.ref) - at(a.ref); });
      // the first in the document becomes Lunta's (unless it has one)
      var firstBib = bibs.length ? bibs[bibs.length - 1] : null;
      var bstep = Promise.resolve();
      bibs.forEach(function (b) {
        bstep = bstep.then(function () {
          var makeOurs = !hasBib && b === firstBib;
          if (b.ref.cc !== undefined) {
            var cc = ctx.document.contentControls.getById(b.ref.cc);
            if (makeOurs) { cc.tag = LuntaCite.BIB_TAG; cc.title = 'Bibliography (Lunta)'; cc.appearance = 'BoundingBox'; }
            else cc.delete(false);
            return ctx.sync();
          }
          var f = fields.items[b.ref.field];
          if (!fieldsOK || !f || !LuntaFields.sameCode(f.code, b.ref.code)) return;
          if (makeOurs) {
            f.select();
            var bc = ctx.document.getSelection().insertText('References will appear here.', 'Replace').insertContentControl();
            bc.tag = LuntaCite.BIB_TAG; bc.title = 'Bibliography (Lunta)';
          } else f.delete();
          return ctx.sync();
        });
      });
      return bstep;
    }).then(function () { return done; });
  });
}

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
  if ($('convert')) $('convert').onclick = convertCitations;
  if ($('cvclose')) $('cvclose').onclick = closeConvert;
  // clicking a citation in the text shows its papers here
  watch();
  // Word for Mac may lose the handler on a save: registered again when the panel shows or gets the focus
  if (document.addEventListener) document.addEventListener('visibilitychange', function () {
    if (document.visibilityState === 'visible' && ready) { watch(true); lookAtSelection(); }
  });
  if (window.addEventListener) window.addEventListener('focus', function () { if (ready) watch(true); });
  status('Loading…');
  // the document's style before anything may be updated
  loadState().then(function (st) {
    state = st;
    var saved = LuntaCite.isStyle(st.style);
    style = saved ? st.style : lastStyle();
    // the document's citations: is there any without a saved style
    // and the controls inside them, so a click there is answered without asking Word for parents
    return wordRun(function (ctx) {
      var all = ctx.document.contentControls;
      all.load('items/id,items/tag,items/title');
      return ctx.sync().then(function () {
        var cites = all.items.filter(function (c) { return LuntaCite.parseTag(c.tag); });
        styleUnknown = !saved && cites.length > 0;
        var inner = cites.map(function (c) { var cs = c.contentControls; cs.load('items/id,items/tag,items/title,items/text'); return cs; });
        return (cites.length ? ctx.sync() : Promise.resolve()).then(function () { rememberInside(cites, inner); }, function (e) {
          if (window.console) console.error('reading the controls inside citations', e);   // Word is asked for parents instead
        });
      });
    }, true).catch(function (e) {
      if (!saved) throw e;   // can't tell: never update in a guessed style
      if (window.console) console.error('reading the citations', e);
    });
  }).then(function () {
    fillStyles();
    ready = true; setButtons();
    if (watching === false) watch();   // Word wasn't ready for it at first
    // the citation the cursor is in already, without waiting for a click or Update All
    checkSelection();
    poll();
    if (styleUnknown) {
      status('This document has citations but its style wasn’t saved with it (it was made with another copy of the add-in). Choose its style in the menu above.', true);
      return;
    }
    return getJSON('/status').then(function (s) { status('Connected to Lunta · ' + s.papers + ' papers'); }, function (e) {
      if (e.kind === 'offline' && Object.keys(state.papers).length) status('Lunta isn’t running. Update All still works from the copy of the cited papers saved in this document; to search and cite, open Lunta.', true);
      else fail(e);
    });
  }).catch(function (e) {
    if (ready) { fail(e); return; }
    if (window.console) console.error('loading', e);
    var why = e && e.stalled ? 'Word didn’t answer' : isWordError(e) ? 'Word: ' + wordText(e) : e && e.kind ? LuntaCite.describe(e, 'word') : (e && e.message || String(e));
    status('Lunta couldn’t read this document (' + why + '). Close and reopen the Lunta panel.', true);
  });
  $('q').focus();
});
