// Resolve Citations, the part the Word panel and the Google Docs panel share (the extension keeps a
// copy, extension/gdocs/citations-resolve.js; a test fails when they differ). Write first, cite
// later: placeholders typed while writing ("(Langseth et al. 2025)", "[Langseth 2025 spatial]",
// "{Langseth 2025}") are found by Lunta (core/src/placeholders, /placeholders), and for each part
// Lunta proposes papers (/suggest: the library first, then Crossref or OpenAlex). Lunta proposes,
// the writer decides: nothing is written into the document or added to the library until the
// writer confirms each one (Confirm, or Confirm All Exact Matches for parts with exactly one
// library paper that fits). A placeholder becomes a citation once each of its parts is confirmed
// or skipped, with at least one confirmed; parts skipped stay in the text after it, as written.
//   LuntaResolve.create({box, doc, api, status, changed}) → controller
//     api.suggest(cite), api.search(query) → {candidates, exact, note}
//     api.add(candidate) → the library's item for a paper from outside it (POST /add)
//     api.apply(placeholder, ids, leftover) → writes the citation in place of the placeholder
//   controller.load(placeholders) [{text, cites, …the panel's own fields}], confirm(i, j, k),
//   skip(i, j), search(i, j, query), confirmExact(), items
var LuntaResolve = (function () {
  /** "(Smith 2019)" for the parts left as written ("" when none), in the placeholder's brackets. */
  function leftover(ph, cites) {
    var left = cites.filter(function (c) { return c.state === 'skipped'; }).map(function (c) { return c.cite.text; });
    // "Smith 2019, 2020" is one part with two cites: once
    left = left.filter(function (t, i) { return left.indexOf(t) === i; });
    if (!left.length) return '';
    var o = ph.text.charAt(0), close = { '(': ')', '[': ']', '{': '}' }[o];
    if (!close) { o = '('; close = ')'; }
    return o + left.join('; ') + close;
  }

  function authorsLine(c) {
    var f = (c.families && c.families.length ? c.families : c.authors) || [];
    var who = f.length > 2 ? f[0] + ' et al.' : f.join(f.length === 2 ? ' & ' : '');
    return [who, c.year, c.journal].filter(Boolean).join(' · ');
  }

  function sourceLine(c) {
    var where = c.inLibrary ? 'In your library' : (c.source === 'openalex' ? 'OpenAlex' : 'Crossref') + ' · added to your library when confirmed';
    return where + ' · ' + Math.round((c.confidence || 0) * 100) + '%' + (c.exact ? ' · exact match' : '');
  }

  function create(opts) {
    var api = opts.api, doc = opts.doc || (typeof document !== 'undefined' ? document : null);
    var items = [], queue = Promise.resolve(), generation = 0;
    var status = opts.status || function () {};

    function el(tag, cls, text) {
      var e = doc.createElement(tag);
      if (cls) e.className = cls;
      if (text != null) e.textContent = text;
      return e;
    }
    function button(text, cls, fn) {
      var b = el('button', cls, text);
      b.onclick = fn;
      return b;
    }

    function load(list) {
      generation++;
      items = (list || []).map(function (ph) {
        return { ph: ph, state: 'pending', error: '', cites: ph.cites.map(function (c) {
          return { cite: c, state: 'loading', candidates: [], pick: 0, exact: false, note: '', chosen: null, searching: false };
        }) };
      });
      render();
      var mine = generation, jobs = [];
      items.forEach(function (it, i) { it.cites.forEach(function (c, j) { jobs.push([i, j]); }); });
      // two at a time: Crossref is asked for parts the library can't settle
      function next() {
        var job = jobs.shift();
        if (!job || mine !== generation) return Promise.resolve();
        var c = items[job[0]].cites[job[1]];
        return Promise.resolve().then(function () { return api.suggest(c.cite); }).then(function (r) {
          if (mine !== generation) return;
          fill(c, r);
        }, function (e) {
          if (mine !== generation) return;
          c.state = 'open'; c.note = (e && e.message) || String(e);
        }).then(function () { if (mine === generation) render(); return next(); });
      }
      return Promise.all([next(), next()]).then(function () { if (mine === generation) summarize(); });
    }

    function fill(c, r) {
      c.candidates = (r && r.candidates) || [];
      c.exact = !!(r && r.exact) && !!c.candidates.length && !!c.candidates[0].exact && !!c.candidates[0].inLibrary;
      c.note = (r && r.note) || (c.candidates.length ? '' : 'No paper found. Search, or skip it.');
      c.pick = 0;
      c.state = 'open';
    }

    function decided(it) { return it.cites.every(function (c) { return c.state === 'confirmed' || c.state === 'skipped'; }); }

    /** Confirms candidate k (default: the one picked) of part j of placeholder i. */
    function confirm(i, j, k) {
      var it = items[i], c = it && it.cites[j];
      if (!c || c.state !== 'open' || it.state !== 'pending') return Promise.resolve();
      var cand = c.candidates[k == null ? c.pick : k];
      if (!cand) return Promise.resolve();
      c.chosen = cand; c.state = 'confirmed';
      return settle(i);
    }

    function skip(i, j) {
      var it = items[i], c = it && it.cites[j];
      if (!c || (c.state !== 'open' && c.state !== 'loading') || it.state !== 'pending') return Promise.resolve();
      c.state = 'skipped';
      return settle(i);
    }

    function settle(i) {
      var it = items[i];
      render();
      if (!decided(it)) return Promise.resolve();
      if (!it.cites.some(function (c) { return c.state === 'confirmed'; })) {
        it.state = 'skipped';
        render(); summarize();
        return Promise.resolve();
      }
      it.state = 'queued';
      render();
      queue = queue.then(function () { return write(i); });
      return queue;
    }

    /** The placeholder made a citation: outside papers added to the library first. */
    function write(i) {
      var it = items[i];
      it.state = 'applying'; it.error = '';
      render();
      var ids = [], added = 0;
      var chain = Promise.resolve();
      it.cites.forEach(function (c) {
        if (c.state !== 'confirmed') return;
        chain = chain.then(function () {
          if (c.chosen.inLibrary && c.chosen.id) { ids.push(c.chosen.id); return; }
          return api.add(c.chosen).then(function (p) {
            if (!p || !p.id) throw new Error('Lunta couldn’t add “' + (c.chosen.title || 'the paper') + '” to your library.');
            if (p.added) added++;
            // added once, for another part citing it too
            c.chosen = Object.assign({}, c.chosen, { id: p.id, inLibrary: true });
            ids.push(p.id);
          });
        });
      });
      return chain.then(function () {
        ids = ids.filter(function (x, n) { return ids.indexOf(x) === n; });
        return api.apply(it.ph, ids, leftover(it.ph, it.cites));
      }).then(function () {
        it.state = 'done'; it.added = added;
        render(); summarize();
        if (opts.changed) opts.changed();
      }, function (e) {
        it.state = 'failed'; it.error = (e && e.message) || String(e);
        render();
        status(it.error, true);
      });
    }

    function retry(i) {
      var it = items[i];
      if (!it || it.state !== 'failed') return Promise.resolve();
      it.state = 'queued';
      queue = queue.then(function () { return write(i); });
      return queue;
    }

    /** Back to choosing: a failed placeholder's parts open again. */
    function reopen(i) {
      var it = items[i];
      if (!it || (it.state !== 'failed' && it.state !== 'skipped')) return;
      it.state = 'pending'; it.error = '';
      it.cites.forEach(function (c) { c.state = 'open'; c.chosen = null; });
      render();
    }

    /** Every part whose first candidate is the only library paper that fits; others stay. */
    function confirmExact() {
      var waits = [];
      items.forEach(function (it, i) {
        if (it.state !== 'pending') return;
        var any = false;
        it.cites.forEach(function (c) {
          if (c.state === 'open' && c.exact) { c.chosen = c.candidates[0]; c.state = 'confirmed'; any = true; }
        });
        if (any) waits.push(settle(i));
      });
      render();
      return Promise.all(waits);
    }

    function search(i, j, q) {
      var it = items[i], c = it && it.cites[j];
      if (!c || it.state !== 'pending' || !String(q || '').trim()) return Promise.resolve();
      c.state = 'loading';
      render();
      return Promise.resolve().then(function () { return api.search(String(q).trim()); }).then(function (r) {
        fill(c, r);
        c.exact = false;   // chosen by searching: confirmed one by one
        c.searching = false;
        render();
      }, function (e) {
        c.state = 'open'; c.note = (e && e.message) || String(e);
        render();
      });
    }

    function counts() {
      var n = { done: 0, skipped: 0, open: 0, added: 0, failed: 0, exact: 0 };
      items.forEach(function (it) {
        if (it.state === 'done') { n.done++; n.added += it.added || 0; }
        else if (it.state === 'skipped') n.skipped++;
        else if (it.state === 'failed') n.failed++;
        else n.open++;
        if (it.state === 'pending') it.cites.forEach(function (c) { if (c.state === 'open' && c.exact) n.exact++; });
      });
      return n;
    }

    function summarize() {
      var n = counts();
      if (!items.length) { status('No placeholders found. Type them as you write, e.g. (Langseth et al. 2025), [Smith 2019 cortex] or {Lee 2020}.'); return; }
      var parts = [];
      if (n.done) parts.push(n.done + ' cited' + (n.added ? ' (' + n.added + ' paper' + (n.added === 1 ? '' : 's') + ' added to your library)' : ''));
      if (n.skipped) parts.push(n.skipped + ' left as written');
      if (n.failed) parts.push(n.failed + ' not written');
      if (n.open) parts.push(n.open + ' to decide');
      status((items.length === 1 ? '1 placeholder' : items.length + ' placeholders') + ': ' + parts.join(' · ') + '.');
    }

    // ---- the list ----

    function render() {
      var box = opts.box;
      if (!box || !doc) return;
      box.innerHTML = '';
      var n = counts();
      if (n.exact) {
        var row = el('div', 'buttons');
        row.appendChild(button('Confirm All Exact Matches (' + n.exact + ')', 'btn primary rs-exact', function () { confirmExact(); }));
        box.appendChild(row);
      }
      items.forEach(function (it, i) {
        var b = el('div', 'rs-ph rs-' + it.state);
        b.appendChild(el('div', 'rs-text', it.ph.text));
        if (it.state === 'done') { b.appendChild(el('div', 'rs-state', '✓ Cited')); box.appendChild(b); return; }
        if (it.state === 'skipped') {
          b.appendChild(el('div', 'rs-state', 'Left as written'));
          b.appendChild(button('Choose again', 'link', function () { reopen(i); }));
          box.appendChild(b);
          return;
        }
        if (it.state === 'queued' || it.state === 'applying') { b.appendChild(el('div', 'rs-state', 'Citing…')); box.appendChild(b); return; }
        if (it.state === 'failed') {
          b.appendChild(el('div', 'rs-state err', it.error));
          var fr = el('div', 'buttons');
          fr.appendChild(button('Try Again', 'btn', function () { retry(i); }));
          fr.appendChild(button('Choose Again', 'btn', function () { reopen(i); }));
          b.appendChild(fr);
          box.appendChild(b);
          return;
        }
        it.cites.forEach(function (c, j) {
          var cb = el('div', 'rs-cite');
          if (it.cites.length > 1) {
            // "Smith 2019, 2020" is two cites: told apart by their years
            var twin = it.cites.filter(function (x) { return x.cite.text === c.cite.text; }).length > 1;
            cb.appendChild(el('div', 'rs-label', c.cite.text + (twin ? ': ' + c.cite.year : '')));
          }
          if (c.state === 'loading') { cb.appendChild(el('div', 'rs-note', 'Looking…')); b.appendChild(cb); return; }
          if (c.state === 'confirmed') { cb.appendChild(el('div', 'rs-chosen', '✓ ' + (c.chosen.title || 'Untitled'))); b.appendChild(cb); return; }
          if (c.state === 'skipped') { cb.appendChild(el('div', 'rs-chosen', 'Skipped: stays as written')); b.appendChild(cb); return; }
          c.candidates.forEach(function (cand, k) {
            var r = el('div', 'hit rs-cand' + (k === c.pick ? ' on' : ''));
            r.appendChild(el('div', 't', cand.title || 'Untitled'));
            r.appendChild(el('div', 'm', authorsLine(cand)));
            r.appendChild(el('div', 'm rs-src', sourceLine(cand)));
            r.onclick = function () { c.pick = k; render(); };
            cb.appendChild(r);
          });
          if (c.note) cb.appendChild(el('div', 'rs-note', c.note));
          var row = el('div', 'buttons');
          var ok = button('Confirm', 'btn primary', function () { confirm(i, j); });
          ok.disabled = !c.candidates.length;
          row.appendChild(ok);
          row.appendChild(button('Skip', 'btn', function () { skip(i, j); }));
          row.appendChild(button('Search…', 'btn', function () { c.searching = !c.searching; render(); }));
          cb.appendChild(row);
          if (c.searching) {
            var q = el('input', 'rs-q');
            q.type = 'search';
            q.placeholder = 'Title words, authors, a DOI…';
            q.value = c.cite.surnames ? c.cite.surnames.join(' ') + (c.cite.hints && c.cite.hints.length ? ' ' + c.cite.hints.join(' ') : '') : '';
            q.onkeydown = function (e) { if (e.key === 'Enter') search(i, j, q.value); };
            var sr = el('div', 'buttons');
            sr.appendChild(q);
            sr.appendChild(button('Search', 'btn', function () { search(i, j, q.value); }));
            cb.appendChild(sr);
          }
          b.appendChild(cb);
        });
        box.appendChild(b);
      });
    }

    return {
      load: load, confirm: confirm, skip: skip, search: search, confirmExact: confirmExact, retry: retry, reopen: reopen,
      get items() { return items; }, idle: function () { return queue; }, render: render
    };
  }

  return { create: create, leftover: leftover };
})();
if (typeof module !== 'undefined') module.exports = LuntaResolve;
