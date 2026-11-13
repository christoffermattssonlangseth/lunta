// Lunta citations for Word: citations are content controls tagged "margins:<id>,<id>", the
// bibliography is a content control tagged "margins-bib". "Update All" renumbers and reformats
// every citation and rewrites the bibliography.
// served by Lunta itself (https://localhost:47481) or from the hosted copy (GitHub Pages, the
// Microsoft store listing): either way the library comes from the Lunta app on this computer
var LUNTA = 'https://localhost:47481';
var API = (location.origin === LUNTA ? '' : LUNTA) + '/api';
var VERSION = '4';   // bump with the ?v= in index.html, so Word loads the new files
var UNREACHABLE = 'Can’t reach Lunta. Open the Lunta app on this Mac; the first time, choose Lunta › Citations in Word & Google Docs… and click Trust Certificate.';
var picked = [];         // [{id, title, ...}]
var lastHits = [];
var style = 'apa';

function $(id) { return document.getElementById(id); }
function status(msg, err) { var s = $('status'); s.textContent = msg || ''; s.className = 'status' + (err ? ' err' : ''); }
// Word's errors say little ("Check the OfficeExtension.Error.debugInfo"): show where and what
function fail(e) {
  var d = e && e.debugInfo, more = '';
  try { more = d ? JSON.stringify(d) : (e && e.code) || ''; } catch (x) {}
  if (window.console) console.error(e, d);
  status((e && e.message || String(e)) + (more ? ' · ' + more : ''), true);
}

function getJSON(url, body) {
  var opts = body ? { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) } : {};
  return fetch(API + url, opts).then(function (r) {
    if (!r.ok) throw new Error('Lunta answered ' + r.status);
    return r.json();
  });
}

function renderPicked() {
  $('picked').textContent = picked.length ? 'Citing: ' + picked.map(function (p) {
    return (p.families && p.families[0] ? p.families[0] : p.title.slice(0, 30)) + ' ' + (p.year || '');
  }).join('; ') : '';
  $('insert').disabled = picked.length === 0;
  document.querySelectorAll('.hit').forEach(function (el) {
    el.classList.toggle('on', picked.some(function (p) { return p.id === el.dataset.id; }));
  });
}

function search(q) {
  if (!q.trim()) { $('results').innerHTML = ''; return; }
  getJSON('/search?q=' + encodeURIComponent(q)).then(function (hits) {
    lastHits = hits;
    $('results').innerHTML = '';
    if (!hits.length) { $('results').innerHTML = '<p class="status">No papers match.</p>'; return; }
    hits.forEach(function (h) {
      var el = document.createElement('div');
      el.className = 'hit'; el.dataset.id = h.id;
      var who = (h.authors || []).slice(0, 3).join(', ') + ((h.authors || []).length > 3 ? ' et al.' : '');
      el.innerHTML = '<div class="t"></div><div class="m"></div>';
      el.querySelector('.t').textContent = h.title || 'Untitled';
      el.querySelector('.m').textContent = [who, h.year, h.journal].filter(Boolean).join(' · ');
      el.onclick = function () {
        var i = picked.findIndex(function (p) { return p.id === h.id; });
        if (i >= 0) picked.splice(i, 1); else picked.push(h);
        renderPicked();
      };
      $('results').appendChild(el);
    });
    renderPicked();
  }).catch(function () { status(UNREACHABLE, true); });
}

function insertCitation() {
  var ids = picked.map(function (p) { return p.id; });
  return Word.run(function (ctx) {
    var sel = ctx.document.getSelection();
    var parent = sel.parentContentControlOrNullObject;
    parent.load('isNullObject,tag');
    return ctx.sync().then(function () {
      // the cursor is in a citation: add the papers to it, "(A, 2021; B, 2023)"
      if (!parent.isNullObject && parent.tag && parent.tag.indexOf('margins:') === 0) {
        var had = parent.tag.slice(8).split(',').filter(Boolean);
        parent.tag = 'margins:' + had.concat(ids.filter(function (id) { return had.indexOf(id) < 0; })).join(',');
        return ctx.sync();
      }
      // text first, then the content control around it: Word on the web won't wrap an empty
      // range; at the end of the selection, so selected text isn't replaced
      var cc = sel.getRange('End').insertText('(…)', 'Replace').insertContentControl();
      cc.tag = 'margins:' + ids.join(',');
      cc.title = 'Citation';
      cc.appearance = 'Hidden';
      return ctx.sync();
    });
  }).then(function () {
    picked = []; renderPicked();
    return updateAll();
  }).catch(fail);
}

function insertBibliography() {
  return Word.run(function (ctx) {
    var existing = ctx.document.contentControls.getByTag('margins-bib');
    existing.load('items');
    return ctx.sync().then(function () {
      if (existing.items.length) { existing.items[0].select(); return ctx.sync(); }
      // its own paragraph after the cursor's, with text in it before it is wrapped (see above)
      var p = ctx.document.getSelection().paragraphs.getLast().insertParagraph('References will appear here.', 'After');
      var cc = p.insertContentControl();
      cc.tag = 'margins-bib'; cc.title = 'Bibliography (Lunta)';
      return ctx.sync();
    });
  }).then(updateAll).catch(fail);
}

function updateAll() {
  status('Updating…');
  return Word.run(function (ctx) {
    var all = ctx.document.contentControls;
    all.load('items/tag');
    return ctx.sync().then(function () {
      var cites = all.items.filter(function (c) { return c.tag && c.tag.indexOf('margins:') === 0; });
      var lists = cites.map(function (c) { return c.tag.slice(8).split(',').filter(Boolean); });
      var num = LuntaCite.numbering(lists);
      return getJSON('/bibliography', { style: style, ids: num.list }).then(function (res) {
        var papers = {};
        res.entries.forEach(function (e) { papers[e.id] = e; });
        cites.forEach(function (c, i) {
          var text = LuntaCite.inText(style, lists[i], num.order, papers);
          var r = c.insertText(text, 'Replace');
          r.font.superscript = style === 'nature';
        });
        var bibs = all.items.filter(function (c) { return c.tag === 'margins-bib'; });
        if (bibs.length) {
          var entries = LuntaCite.bibliography(style, res.entries, num.order);
          var html = entries.length
            ? entries.map(function (e) { return '<p style="margin:0 0 6pt 0">' + LuntaCite.toHTML(e.text) + '</p>'; }).join('')
            : '<p>No citations yet.</p>';
          bibs.forEach(function (b) { b.insertHtml(html, 'Replace'); });
        }
        return ctx.sync().then(function () {
          status(cites.length + ' citation' + (cites.length === 1 ? '' : 's') + ', ' + num.list.length + ' reference' + (num.list.length === 1 ? '' : 's') + ' · ' + LuntaCite.STYLES[style].name);
        });
      });
    });
  }).catch(fail);
}

Office.onReady(function () {
  $('ver').textContent = 'v' + VERSION;
  var sel = $('style');
  Object.keys(LuntaCite.STYLES).forEach(function (k) {
    var o = document.createElement('option'); o.value = k; o.textContent = LuntaCite.STYLES[k].name; sel.appendChild(o);
  });
  try { style = Office.context.document.settings.get('luntaStyle') || 'apa'; } catch (e) {}
  sel.value = style;
  sel.onchange = function () {
    style = sel.value;
    try { Office.context.document.settings.set('luntaStyle', style); Office.context.document.settings.saveAsync(); } catch (e) {}
    updateAll();
  };
  var t = null;
  $('q').addEventListener('input', function (e) { clearTimeout(t); t = setTimeout(function () { search(e.target.value); }, 220); });
  $('insert').onclick = insertCitation;
  $('bib').onclick = insertBibliography;
  $('refresh').onclick = updateAll;
  getJSON('/status').then(function (s) { status('Connected to Lunta · ' + s.papers + ' papers'); })
    .catch(function () { status(UNREACHABLE, true); });
  $('q').focus();
});
