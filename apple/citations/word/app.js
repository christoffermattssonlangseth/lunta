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
var VERSION = '20';   // bump with the ?v= in index.html, so Word loads the new files
var NS = 'https://lunta.app/ns/word-citations/1';
var picked = [];          // [{id, title, ...}]
var style = 'apa';
var state = { papers: {} };   // what the document keeps: {style, styleTitle, bibStyle, papers}
var styleUnknown = false;     // citations in the document, but no saved style: ask before updating
var busy = false, ready = false, lastSearch = 0;
var checking = false, stopChecking = false, claimItems = [];   // Check Citations
var current = null;           // the citation the cursor is in: {id (content control), ids, opts}
var editing = null;           // the paper of the current citation whose options are open

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
  if ($('convert')) $('convert').disabled = busy || !ready;
  if ($('plain')) $('plain').disabled = busy || !ready;
  if ($('plainGo')) $('plainGo').disabled = busy || !ready;
  if ($('claims')) $('claims').disabled = !ready || checking;
}

/** One change to the document at a time (a second click while updating does nothing). */
function exclusive(fn) {
  return function () {
    if (busy || !ready) return;
    busy = true; setButtons();
    return Promise.resolve().then(fn).catch(fail).then(idle);
  };
}

/** Done changing the document: the buttons back, and a click in the text meanwhile looked at now. */
function idle() {
  busy = false; setButtons();
  if (selPending) onSelection();
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
        parent.tag = LuntaCite.makeTag(had.concat(ids));   // its title (the options) stays
        return ctx.sync();
      }
      // at the end of the selection, so selected text isn't replaced; the cursor after it, outside
      // the control, so what is typed next isn't part of the citation
      var cc = wrapCitation(sel.getRange('End'), ids);
      try { cc.getRange('After').select('End'); } catch (e) {}
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
var lastShown = '', selPending = false, selSeq = 0, selRunning = false, selAgain = false;
var seen = {};      // papers' details Lunta gave for the citation box this session: {id: entry}
var inside = {};    // a control inside a Lunta citation: {control id: {id, tag, title} of the citation}
var outside = {};   // controls not in a citation (asked Word once): {control id: true}
function onSelection() {
  if (busy || !ready) { selPending = true; return; }
  if (selRunning) { selAgain = true; return; }
  checkSelection();
}
function checkSelection() {
  if (busy || !ready) { selPending = true; return Promise.resolve(); }
  selPending = false; selAgain = false; selRunning = true;
  var mine = ++selSeq;
  return Word.run(function (ctx) {
    var cc = ctx.document.getSelection().parentContentControlOrNullObject;
    cc.load('isNullObject,id,tag,title');
    return ctx.sync().then(function () { return citationAt(ctx, cc, 0); });
  }).then(function (cc) {
    if (mine !== selSeq) return;   // a later click answered first
    var ids = cc ? LuntaCite.parseTag(cc.tag) : null;
    current = ids && ids.length ? { id: cc.id, ids: ids, opts: LuntaCite.optionsOfTitle(cc.title) || {} } : null;
    if (!current) editing = null;
    showCurrent();
  }).catch(function () {}).then(function () {
    selRunning = false;
    if (selAgain) onSelection();
  });
}

/** The Lunta citation the loaded control `cc` is or is inside, {id, tag, title}, or null.
 *  Word is asked for its parents only for a control not seen when the citations were read. */
function citationAt(ctx, cc, depth) {
  if (cc.isNullObject) return null;
  if (LuntaCite.parseTag(cc.tag)) return { id: cc.id, tag: cc.tag, title: cc.title };
  if (cc.tag === LuntaCite.BIB_TAG || depth >= 3) return null;
  if (inside[cc.id]) return inside[cc.id];
  if (outside[cc.id]) return null;
  var parent = cc.parentContentControlOrNullObject;
  parent.load('isNullObject,id,tag,title');
  return ctx.sync().then(function () {
    var found = citationAt(ctx, parent, depth + 1);
    return Promise.resolve(found).then(function (c) {
      if (c) inside[cc.id] = c; else outside[cc.id] = true;
      return c;
    });
  });
}

/** Remembers the document's citations' inner controls (`inside`): `cites` are the loaded Lunta
 *  citation controls, `inner` their loaded contentControls collections. */
function rememberInside(cites, inner) {
  inside = {}; outside = {};
  cites.forEach(function (c, i) {
    var entry = { id: c.id, tag: c.tag, title: c.title };
    (inner[i] && inner[i].items || []).forEach(function (x) { inside[x.id] = entry; });
  });
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
  }).catch(function () {});
}

var removeFromCitation = function (id) {
  if (!current) return;
  var target = current;
  return exclusive(function () {
    var left = target.ids.filter(function (x) { return x !== id; });
    var opts = LuntaCite.cleanOptions(target.opts, left);
    return Word.run(function (ctx) {
      var cc = ctx.document.contentControls.getById(target.id);
      if (left.length) { cc.tag = LuntaCite.makeTag(left); cc.title = LuntaCite.optionsTitle(opts); }
      else cc.delete(false);   // its last paper: the citation goes, text and all
      return ctx.sync();
    }).then(function () {
      current = left.length ? { id: target.id, ids: left, opts: opts || {} } : null;
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
    return Word.run(function (ctx) {
      var cc = ctx.document.contentControls.getById(target.id);
      cc.title = LuntaCite.optionsTitle(map);
      return ctx.sync();
    }).then(function () {
      current = { id: target.id, ids: target.ids, opts: map || {} };
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
  return Word.run(function (ctx) {
    var all = ctx.document.contentControls;
    all.load('items/id,items/tag,items/text,items/title');
    return ctx.sync().then(function () {
      var bibs = all.items.filter(function (c) { return c.tag === LuntaCite.BIB_TAG; });
      // citations inside the bibliography go when it is rewritten: not counted
      var inner = bibs.map(function (b) { var cs = b.contentControls; cs.load('items/id'); return cs; });
      var cands = all.items.filter(function (c) { return LuntaCite.parseTag(c.tag) !== null; });
      cands.forEach(function (c) { c.font.load('superscript'); });
      var within = cands.map(function (c) { var cs = c.contentControls; cs.load('items/id'); return cs; });
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
          var changed = 0, strays = [];
          cites.forEach(function (c, i) {
            var t = f.texts[i], sup = !!f.superscripts[i];
            if (!t) return;
            // the writer's words typed into the control ("28 with GSEApy"): kept, outside it
            var stray = c.text === t ? null : LuntaCite.strayText(c.text, t);
            if (stray) {
              var whole = c.getRange('Whole');
              if (stray.before) whole.insertText(stray.before, 'Before').font.superscript = false;
              if (stray.after) whole.insertText(stray.after, 'After').font.superscript = false;
              strays.push(stray);
            }
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
                   (empty ? ' · ' + empty + ' empty citation' + (empty === 1 ? '' : 's') + ' left out' : '') + warn +
                   (strays.length ? ' ' + LuntaCite.strayNote(strays) : ''));
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

// ---- Check Citations: each citation's sentence against the cited papers (common.js, Lunta's /claims) ----
// The document is only read: the body's paragraphs and the Lunta citations in each.

function claimParagraphs() {
  return Word.run(function (ctx) {
    var paras = ctx.document.body.paragraphs;
    paras.load('items/text');
    return ctx.sync().then(function () {
      var ccs = paras.items.map(function (p) { var c = p.contentControls; c.load('items/tag,items/text'); return c; });
      return ctx.sync().then(function () {
        return paras.items.map(function (p, i) {
          return { text: p.text, cites: ccs[i].items.map(function (c) {
            var ids = LuntaCite.parseTag(c.tag);
            return ids && ids.length ? { text: c.text, ids: ids } : null;
          }).filter(Boolean) };
        });
      });
    });
  });
}

/** Shows the paper at the passage in Lunta (the API asks the app; the lunta:// link is the way back). */
function openPassage(link) {
  var l = LuntaCite.parseOpenLink(link);
  if (!l) return;
  getJSON('/open', { id: l.id, find: l.find }, 8000).catch(fail);
}

function showClaims() {
  LuntaCite.claimsReview(document, $('cllist'), claimItems, {
    open: openPassage,
    changed: function () { status(LuntaCite.claimSummary(LuntaCite.claimCounts(claimItems, LuntaCite.claimDecisions())) + '.'); }
  });
}

function checkCitations() {
  if (!ready || checking) return;
  checking = true; stopChecking = false; setButtons();
  status('Reading the citations…');
  return getJSON('/status').then(function (s) {
    if ((s.api || 0) < 5) throw new Error('This version of Lunta can’t check citations. Update Lunta and try again.');
    return claimParagraphs();
  }).then(function (paras) {
    var sites = LuntaCite.claimSites(paras);
    if (!sites.length) { status('This document has no Lunta citations to check.'); return; }
    $('claimsBox').hidden = false; $('clstop').hidden = false;
    $('cllist').textContent = LuntaCite.CLAIM_PRIVACY;
    return LuntaCite.runClaims(sites, function (body) { return getJSON('/claims', body, 180000); },
      function (i, n) { status('Checking citation ' + (i + 1) + ' of ' + n + '…'); },
      function () { return stopChecking; }, state.papers).then(function (items) {
      claimItems = items;
      showClaims();
      status(LuntaCite.claimSummary(LuntaCite.claimCounts(items, LuntaCite.claimDecisions())) + (stopChecking ? ' (stopped).' : '.'));
    });
  }).catch(fail).then(function () { checking = false; if ($('clstop')) $('clstop').hidden = true; setButtons(); });
}

function closeClaims() { stopChecking = true; $('claimsBox').hidden = true; $('cllist').innerHTML = ''; claimItems = []; status(''); }

// ---- Convert Citations from Zotero, EndNote, Mendeley or ReadCube Papers ----
// The other manager's citations are read (word/fields.js: Word's fields in the body's OOXML, and
// Mendeley Cite's content controls), their works found in the library (/match), the writer
// reviews what will be added, and only then are the citations replaced, one by one: a field is
// selected (all of it, codes and shown text) and replaced by a Lunta citation (a content control
// with the papers and the options the other manager kept); a Mendeley Cite or ReadCube Papers
// (SmartCite) control gets Lunta's tag and title, so it stays where it is, superscript or not.
// SmartCite keeps only the works' DOIs and PubMed ids in its citations: their titles, authors and
// years come from its bibliography's entries (word/fields.js parseBibliography).
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
  ccs.load('items/id,items/tag,items/text,items/title');
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
    // ReadCube Papers (SmartCite): the works' details are in its bibliography's entries
    var formatted = [], entries = [];
    ccs.items.forEach(function (cc) {
      var c = LuntaFields.classifyTag(cc.tag) || LuntaFields.classifyControl(cc.title, cc.tag);
      if (!c) return;
      var ref = { cc: cc.id, tag: cc.tag };
      if (c.kind === 'bib') {
        out.bibs.push({ ref: ref, source: c.source, style: c.style || '' });
        if (c.source === LuntaFields.SMARTCITE) {
          // its text with a paragraph a line (the OOXML's), else Word's
          var same = scan.controls.filter(function (x) { return x.title === cc.title && x.tag === cc.tag; })[0];
          entries = entries.concat(LuntaFields.parseBibliography(same && same.text ? same.text : cc.text));
        }
      } else if (c.items) formatted.push({ ref: ref, items: c.items, text: cc.text, source: c.source });
      else if (!c.works || !c.works.length) unreadable(c.source);
      else out.cites.push({ ref: ref, works: c.works, text: cc.text, source: c.source });
    });
    // in the document's order among the others (content controls come after the fields anyway)
    formatted.forEach(function (f) {
      var works = LuntaFields.formattedCitation(f.text, f.items, entries);
      if (!works || !works.length) { unreadable(f.source); return; }
      out.cites.push({ ref: f.ref, works: works.map(function (w) { return { meta: w.meta, options: w.options }; }), text: f.text, source: f.source });
    });
    out.bibs.forEach(function (b) {
      if (b.style) out.notes.push('The ' + b.source + ' bibliography was in the “' + b.style + '” style: choose it in the style menu (Find a journal style…) if it isn’t the document’s yet.');
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
  status('Looking for citations from Zotero, EndNote, Mendeley and ReadCube Papers…');
  return getJSON('/status').then(function (s) {
    if ((s.api || 0) < 4) throw new Error('This version of Lunta can’t convert citations. Update Lunta and try again.');
    return Word.run(readForeign);
  }).then(function (found) {
    var box = $('convertBox');
    if (!found.cites.length) {
      status(found.notes.length ? found.notes.join(' ') : 'This document has no citations from Zotero, EndNote, Mendeley or ReadCube Papers.', !!found.notes.length);
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
      return Word.run(function (ctx) {
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

// ---- Convert Citations to Plain Text: for the version you submit ----
// Every Lunta citation and the bibliography become ordinary text as they are after Update All
// (their content controls removed, their text and formatting kept), and the document's copy of
// the papers (the custom XML part, the add-in settings) goes: what Zotero calls Unlink Citations.

function openPlain() {
  $('plainBox').hidden = false;
  $('plainText').textContent = LuntaCite.PLAIN_WARNING;
  status('');
}
function closePlain() { $('plainBox').hidden = true; }

var convertToPlain = exclusive(function () {
  closePlain();
  var cites = 0, bib = 0;
  return update().catch(function (e) {
    throw new Error(LuntaCite.describe(e, 'word') + ' Nothing was converted to plain text.');
  }).then(function () {
    return Word.run(function (ctx) {
      var all = ctx.document.contentControls;
      all.load('items/id,items/tag');
      return ctx.sync().then(function () {
        var ours = all.items.filter(function (c) { return c.tag === LuntaCite.BIB_TAG || LuntaCite.parseTag(c.tag) !== null; });
        // the last first: a citation inside the bibliography before the bibliography
        ours.slice().reverse().forEach(function (c) {
          if (c.tag === LuntaCite.BIB_TAG) bib++; else cites++;
          c.delete(true);   // the control goes, its text stays
        });
        return ctx.sync();
      });
    });
  }).then(function () {
    return removeState();
  }).then(function () {
    status(LuntaCite.plainDone(cites, bib));
  });
});

/** The document's copy of its papers and style removed (custom XML part, settings). */
function removeState() {
  state = { papers: {} };
  try {
    ['luntaStyle', 'luntaState'].forEach(function (k) { Office.context.document.settings.remove(k); });
    Office.context.document.settings.saveAsync(function () {});
  } catch (e) {}
  var parts = xmlParts();
  if (!parts) return Promise.resolve();
  return asyncCall(function (cb) { parts.getByNamespaceAsync(NS, cb); }).then(function (list) {
    return Promise.all(list.map(function (p) { return asyncCall(function (cb) { p.deleteAsync(cb); }).catch(function () {}); }));
  }).catch(function () {});
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
  if ($('claims')) $('claims').onclick = checkCitations;
  if ($('plain')) $('plain').onclick = openPlain;
  if ($('plainGo')) $('plainGo').onclick = convertToPlain;
  if ($('plainCancel')) $('plainCancel').onclick = closePlain;
  if ($('plainClose')) $('plainClose').onclick = closePlain;
  if ($('clclose')) $('clclose').onclick = closeClaims;
  if ($('clstop')) $('clstop').onclick = function () { stopChecking = true; };
  // clicking a citation in the text shows its papers here
  var watching = null;   // null: Word hasn't answered yet
  var watch = function () {
    try {
      Office.context.document.addHandlerAsync(Office.EventType.DocumentSelectionChanged, onSelection, function (r) {
        watching = !r || r.status === Office.AsyncResultStatus.Succeeded;
      });
    } catch (e) { watching = false; }
  };
  watch();
  status('Loading…');
  // the document's style before anything may be updated
  loadState().then(function (st) {
    state = st;
    var saved = LuntaCite.isStyle(st.style);
    style = saved ? st.style : lastStyle();
    // the document's citations: is there any without a saved style
    // and the controls inside them, so a click there is answered without asking Word for parents
    return Word.run(function (ctx) {
      var all = ctx.document.contentControls;
      all.load('items/id,items/tag,items/title');
      return ctx.sync().then(function () {
        var cites = all.items.filter(function (c) { return LuntaCite.parseTag(c.tag); });
        styleUnknown = !saved && cites.length > 0;
        var inner = cites.map(function (c) { var cs = c.contentControls; cs.load('items/id'); return cs; });
        return (cites.length ? ctx.sync() : Promise.resolve()).then(function () { rememberInside(cites, inner); }, function () {});
      });
    }).catch(function (e) {
      if (!saved) throw e;   // can't tell: never update in a guessed style
      if (window.console) console.error('reading the citations', e);
    });
  }).then(function () {
    fillStyles();
    ready = true; setButtons();
    if (watching === false) watch();   // Word wasn't ready for it at first
    // the citation the cursor is in already, without waiting for a click or Update All
    checkSelection();
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
