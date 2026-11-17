// Citation formatting shared by the Word add-in and the Google Docs sidebar (and tested with node:
// apple/citations-tests). A citation is a list of paper ids; numbered styles number papers by
// first appearance. Everything here is plain functions of the document's citations, what Lunta
// answers and what the document keeps of its papers; the panels only read and write documents.
var LuntaCite = (function () {
  // The styles of Lunta's core (CitationStyle), with their in-text form. Author–year styles:
  // `and` joins two authors, `sep` goes between name and year, `noYear` leaves the year out (MLA),
  // `etAl` is the number of authors from which only the first is named. Numbered styles: `open`
  // and `close` wrap the numbers, `superscript` raises them, `prefix` numbers the reference list.
  var STYLES = {
    apa: { name: 'APA 7', and: '&', sep: ', ', etAl: 3 },
    harvard: { name: 'Harvard', and: 'and', sep: ', ', etAl: 4 },
    chicago: { name: 'Chicago (author-date)', and: 'and', sep: ' ', etAl: 4, serial: true },
    cell: { name: 'Cell', and: 'and', sep: ', ', etAl: 3 },
    mla: { name: 'MLA 9', and: 'and', noYear: true, etAl: 3 },
    vancouver: { name: 'Vancouver', numeric: true, open: '[', close: ']' },
    ieee: { name: 'IEEE', numeric: true, open: '[', close: ']', prefix: function (n) { return '[' + n + '] '; } },
    ama: { name: 'AMA', numeric: true, superscript: true },
    nature: { name: 'Nature', numeric: true, superscript: true },
    science: { name: 'Science', numeric: true, open: '(', close: ')' },
    pnas: { name: 'PNAS', numeric: true, open: '(', close: ')' },
    acs: { name: 'ACS', numeric: true, superscript: true }
  };

  // ---- citations in documents ----
  // Word: a content control tagged "margins:<id>,<id>"; the bibliography is tagged "margins-bib".
  // Google Docs: text linked to https://margins.local/cite?ids=<id>,<id> (escaped). These names
  // are in documents people have written, so they stay.
  var TAG = 'margins:', BIB_TAG = 'margins-bib';
  var CITE_URL = 'https://margins.local/cite?ids=', BIB_URL = 'https://margins.local/bib';

  function unique(list) {
    var seen = {}, out = [];
    (list || []).forEach(function (x) {
      x = String(x == null ? '' : x).trim();
      if (x && !seen.hasOwnProperty(x)) { seen[x] = true; out.push(x); }
    });
    return out;
  }

  /** The paper ids of a citation's Word tag; null when the tag isn't a citation's. */
  function parseTag(tag) {
    if (typeof tag !== 'string' || tag.indexOf(TAG) !== 0) return null;
    return unique(tag.slice(TAG.length).split(','));
  }
  function makeTag(ids) { return TAG + unique(ids).join(','); }

  /** The paper ids of a Google Docs citation link; null when the link isn't a citation's. */
  function parseLink(url) {
    if (typeof url !== 'string' || url.indexOf(CITE_URL) !== 0) return null;
    var raw = url.slice(CITE_URL.length);
    try { raw = decodeURIComponent(raw); } catch (e) {}
    return unique(raw.split(','));
  }
  function makeLink(ids) { return CITE_URL + encodeURIComponent(unique(ids).join(',')); }

  /** Ids in order of first citation. */
  function citedIds(citations) {
    return unique([].concat.apply([], citations || []));
  }

  // ---- numbers ----

  /** "1–3,5,7": sorted, without repeats, runs of three or more as ranges. */
  function compress(nums) {
    var seen = {};
    nums = nums.filter(function (n) { if (seen[n]) return false; seen[n] = true; return true; }).sort(function (a, b) { return a - b; });
    var out = [], i = 0;
    while (i < nums.length) {
      var j = i;
      while (j + 1 < nums.length && nums[j + 1] === nums[j] + 1) j++;
      if (j - i >= 2) out.push(nums[i] + '–' + nums[j]);
      else for (var k = i; k <= j; k++) out.push(String(nums[k]));
      i = j + 1;
    }
    return out.join(',');
  }

  /** Numbers papers by first appearance in a list of citations (each a list of ids). */
  function numbering(citations) {
    var order = {}, list = [], n = 0;
    citations.forEach(function (ids) {
      ids.forEach(function (id) { if (!order.hasOwnProperty(id)) { order[id] = ++n; list.push(id); } });
    });
    return { order: order, list: list };
  }

  // ---- names ----

  var PARTICLES = ['van', 'von', 'de', 'der', 'den', 'da', 'di', 'du', 'del', 'della', 'la', 'le', 'ten', 'ter'];
  /** The family name in an author's name, as the core reads it ("Ludwig van Beethoven" → "van Beethoven"). */
  function familyOf(name) {
    name = String(name || '').trim();
    var comma = name.indexOf(',');
    if (comma >= 0) return name.slice(0, comma).trim();
    var parts = name.split(/\s+/).filter(Boolean);
    if (parts.length <= 1) return name;
    var start = parts.length - 1;
    while (start > 1 && PARTICLES.indexOf(parts[start - 1].toLowerCase()) >= 0) start--;
    return parts.slice(start).join(' ');
  }

  /** The names part of an author–year citation: "Smith", "Smith & Lee", "Smith et al." */
  function names(p, st) {
    var f = (p && p.families) || [];
    if (!f.length) return (p && p.title ? shortTitle(p.title) : 'Anonymous');
    if (f.length === 1) return f[0];
    if (f.length >= (st.etAl || 3)) return f[0] + ' et al.';
    if (f.length === 2) return f[0] + ' ' + (st.and || '&') + ' ' + f[1];
    return f.slice(0, -1).join(', ') + (st.serial ? ', ' : ' ') + (st.and || '&') + ' ' + f[f.length - 1];   // three (Harvard, Chicago)
  }

  function shortTitle(t) {
    var w = String(t).replace(/[*]/g, '').split(/\s+/).filter(Boolean);
    return w.slice(0, 3).join(' ') + (w.length > 3 ? '…' : '');
  }

  function yearOf(p) { return (p && p.year) ? String(p.year) : 'n.d.'; }

  // ---- markup: *italic* and **bold** in Lunta's entries ----
  // A "*" counts as markup only where Markdown would take it (an opening one before a letter and
  // after a space or punctuation, a closing one the other way round), so "A* search" or "p*"
  // in a title stays text.

  function isSpace(c) { return !c || /\s/.test(c); }
  function isPunct(c) { return !!c && /[!-\/:-@\[-`{-~‐-‧　-〿]/.test(c); }
  // CommonMark's flanking rules: an opening "*" is followed by a non-space, and when that is
  // punctuation it must itself follow a space or punctuation; a closing one the mirror image
  function opens(s, i, len) {
    var after = s.charAt(i + len), before = s.charAt(i - 1);
    if (isSpace(after)) return false;
    return !isPunct(after) || isSpace(before) || isPunct(before);
  }
  function closes(s, i, len) {
    var before = s.charAt(i - 1), after = s.charAt(i + len);
    if (isSpace(before)) return false;
    return !isPunct(before) || isSpace(after) || isPunct(after);
  }

  /** Splits *italic* and **bold** markup into runs [{text, italic, bold}]. */
  function runs(md) {
    md = String(md == null ? '' : md);
    // a closing "*" pairs with the nearest opening one before it, as in CommonMark
    var stack = [], pairs = [];
    for (var i = 0; i < md.length;) {
      if (md.charAt(i) !== '*') { i++; continue; }
      var j = i;
      while (md.charAt(j) === '*') j++;
      var n = j - i;
      if (n <= 2) {
        var matched = false;
        if (closes(md, i, n)) {
          for (var k = stack.length - 1; k >= 0; k--) {
            if (stack[k].len === n) { pairs.push({ open: stack[k].at, close: i, len: n }); stack.length = k; matched = true; break; }
          }
        }
        if (!matched && opens(md, i, n)) stack.push({ at: i, len: n });
      }
      i = j;
    }
    pairs.sort(function (a, b) { return a.open - b.open; });
    var out = [], pos = 0;
    pairs.forEach(function (p) {
      if (p.open < pos) return;   // inside another pair: stays text
      if (p.open > pos) out.push({ text: md.slice(pos, p.open) });
      var inner = md.slice(p.open + p.len, p.close);
      out.push(p.len === 2 ? { text: inner, bold: true } : { text: inner, italic: true });
      pos = p.close + p.len;
    });
    if (pos < md.length) out.push({ text: md.slice(pos) });
    return out.filter(function (r) { return r.text; });
  }

  function escapeHTML(s) {
    return String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
  }

  /** *italic* and **bold** to HTML, everything else escaped. */
  function toHTML(md) {
    return runs(md).map(function (r) {
      var t = escapeHTML(r.text);
      return r.bold ? '<b>' + t + '</b>' : r.italic ? '<i>' + t + '</i>' : t;
    }).join('');
  }

  /** The text without markup. */
  function plain(md) {
    return runs(md).map(function (r) { return r.text; }).join('');
  }

  // ---- what a document keeps of the papers it cites ----
  // {id: {m: details (as Lunta's API gives them), f: family names, y: year, t: title, e: {style: entry}}}
  // With it, Update All still works for a paper deleted from the library, merged into another
  // (Lunta finds it again by DOI or title), cited by a co-author with another library, or when
  // Lunta isn't running (the entries of the last style used).

  function clip(s, n) { s = String(s == null ? '' : s); return s.length > n ? s.slice(0, n) : s; }

  /** A paper's details for the document, from what the API returned (also older Lunta versions). */
  function detailsOf(e) {
    var m = e.meta || { title: e.title, authors: e.authors, date: e.year ? String(e.year) : '', journal: e.journal, doi: e.doi };
    var out = {};
    Object.keys(m).forEach(function (k) {
      var v = m[k];
      if (v == null || v === '') return;
      if (k === 'authors') {
        var a = (v || []).map(function (x) { return clip(x, 300); });
        out.authors = a.length > 21 ? a.slice(0, 20).concat([a[a.length - 1]]) : a;
      } else out[k] = clip(v, 1000);
    });
    return out;
  }

  function record(e, style, old) {
    var rec = {
      m: detailsOf(e),
      f: (e.families && e.families.length ? e.families : (e.authors || []).map(familyOf)).slice(0, 21),
      y: e.year ? String(e.year) : '',
      t: clip(e.title || '', 1000),
      e: {}
    };
    if (style && e.entry) rec.e[style] = clip(e.entry, 4000);
    else if (old && old.e) rec.e = old.e;
    return rec;
  }

  /** The details to send Lunta with a request: the ones the document keeps of these ids. */
  function detailsFor(store, ids) {
    var out = {};
    (ids || []).forEach(function (id) { if (store && store[id] && store[id].m) out[id] = store[id].m; });
    return out;
  }

  // ---- formatting a document ----

  /**
   * Everything Update All writes, for a built-in style.
   *   citations: the document's citations in order (lists of ids)
   *   entries:   Lunta's answer to /api/bibliography (null: Lunta can't be reached)
   *   store:     what the document keeps of its papers (see above)
   * Returns {texts (one per citation), superscript, bibliography [{id, text}], unknown (ids with
   * nothing to format them from: write nothing), fromDocument (ids formatted from the document's
   * copy), store (to keep), references}.
   */
  function format(style, citations, entries, store) {
    var st = STYLES[style];
    if (!st) throw new Error('Unknown citation style: ' + style);
    store = store || {};
    var ids = citedIds(citations);
    var byId = {}, unknown = [], fromDocument = [];
    (entries || []).forEach(function (e) { if (e && e.id && !e.missing && e.entry) byId[e.id] = e; });
    ids.forEach(function (id) {
      var e = byId[id];
      if (e) { if (e.fromDocument) fromDocument.push(id); return; }
      var rec = store[id];
      if (rec && rec.e && rec.e[style]) {
        byId[id] = { id: id, families: rec.f, year: rec.y, title: rec.t, entry: rec.e[style], meta: rec.m, fromDocument: true };
        fromDocument.push(id);
      } else unknown.push(id);
    });
    var newStore = {};
    ids.forEach(function (id) {
      if (byId[id]) newStore[id] = record(byId[id], style, store[id]);
      else if (store[id]) newStore[id] = store[id];
    });
    var result = { texts: [], superscript: !!st.superscript, bibliography: [], unknown: unknown, fromDocument: fromDocument, store: newStore, references: 0 };
    if (unknown.length) return result;

    // the same paper under two ids (merged in the library): one reference
    var canon = function (id) { return byId[id].libraryId || id; };
    var lists = citations.map(function (l) { return unique(l.map(canon)); });
    var papers = {};
    ids.forEach(function (id) { if (!papers[canon(id)]) papers[canon(id)] = byId[id]; });
    var num = numbering(lists);
    result.references = num.list.length;

    if (st.numeric) {
      result.texts = lists.map(function (l) {
        return l.length ? (st.open || '') + compress(l.map(function (id) { return num.order[id]; })) + (st.close || '') : null;
      });
      var prefix = st.prefix || function (n) { return n + '. '; };
      result.bibliography = num.list.map(function (id) { return { id: id, text: prefix(num.order[id]) + papers[id].entry }; });
      return result;
    }

    // author–year: the reference list sorted, then a/b for works with the same names and year
    var sorted = num.list.slice().sort(function (a, b) {
      return plain(papers[a].entry).localeCompare(plain(papers[b].entry), undefined, { sensitivity: 'base' }) || (a < b ? -1 : 1);
    });
    var groups = {}, suffix = {};
    sorted.forEach(function (id) {
      var k = names(papers[id], st) + '\u0000' + (st.noYear ? '' : yearOf(papers[id]));
      (groups[k] = groups[k] || []).push(id);
    });
    Object.keys(groups).forEach(function (k) {
      var g = groups[k];
      if (g.length > 1) g.forEach(function (id, i) { suffix[id] = st.noYear ? '' : letter(i); });
      if (g.length > 1 && st.noYear) g.forEach(function (id) { suffix[id] = 'title'; });
    });
    result.bibliography = sorted.map(function (id) {
      return { id: id, text: suffix[id] && suffix[id] !== 'title' ? addSuffix(papers[id].entry, papers[id].year, suffix[id]) : papers[id].entry };
    });
    result.texts = lists.map(function (l) {
      if (!l.length) return null;
      var parts = l.map(function (id) {
        var p = papers[id], who = names(p, st);
        var what = st.noYear ? (suffix[id] === 'title' ? '“' + shortTitle(p.title || '') + '”' : '') : yearOf(p) + (suffix[id] || '');
        return { who: who, what: what };
      }).sort(function (a, b) {
        return a.who.localeCompare(b.who, undefined, { sensitivity: 'base' }) || a.what.localeCompare(b.what);
      });
      // works by the same names together: "Smith, 2020a, 2020b"
      var out = [];
      parts.forEach(function (x) {
        var last = out[out.length - 1];
        if (last && last.who === x.who && x.what) last.whats.push(x.what);
        else out.push({ who: x.who, whats: x.what ? [x.what] : [] });
      });
      return '(' + out.map(function (x) {
        if (!x.whats.length) return x.who;
        return x.who + (st.noYear ? ', ' : st.sep) + x.whats.join(', ');
      }).join('; ') + ')';
    });
    return result;
  }

  function letter(i) {
    var s = '';
    i += 1;
    while (i > 0) { var r = (i - 1) % 26; s = String.fromCharCode(97 + r) + s; i = Math.floor((i - 1) / 26); }
    return s;
  }

  /** "Smith, J. (2020)." → "Smith, J. (2020a)."; without a year, "n.d." → "n.d.-a". */
  function addSuffix(entry, year, s) {
    if (year) {
      var re = new RegExp('(^|[^0-9])' + String(year).replace(/[^0-9a-z]/gi, '') + '(?![0-9])');
      if (re.test(entry)) return entry.replace(re, function (m, pre) { return pre + year + s; });
      return entry;
    }
    return entry.indexOf('n.d.') >= 0 ? entry.replace('n.d.', 'n.d.-' + s) : entry;
  }

  /**
   * Everything Update All writes, for a journal style: Lunta's answer to /api/render. A citation
   * Lunta left empty (it couldn't find its papers) is not written over.
   */
  function formatJournal(res, citations, store) {
    store = store || {};
    var ids = citedIds(citations);
    var unknown = unique((res.missing || []).filter(function (id) { return ids.indexOf(id) >= 0; }));
    var texts = (res.citations || []).map(function (c) { return c && c.text ? c.text : null; });
    // an older Lunta doesn't say which papers it lacks: an empty citation tells
    citations.forEach(function (l, i) {
      if (l.length && !texts[i] && !(res.missing)) l.forEach(function (id) { if (unknown.indexOf(id) < 0) unknown.push(id); });
    });
    var items = {};
    (res.items || []).forEach(function (e) { if (e && e.id) items[e.id] = e; });
    var newStore = {};
    ids.forEach(function (id) {
      if (items[id]) newStore[id] = record(items[id], null, store[id]);
      else if (store[id]) newStore[id] = store[id];
    });
    return {
      texts: citations.map(function (l, i) { return texts[i] === undefined ? null : texts[i]; }),
      superscripts: citations.map(function (l, i) { return !!(res.citations && res.citations[i] && res.citations[i].superscript); }),
      bibliography: (res.bibliography || []).map(function (e) { return { id: e.id, text: e.text }; }),
      unknown: unknown,
      fromDocument: ids.filter(function (id) { return items[id] && items[id].fromDocument; }),
      store: newStore,
      references: (res.bibliography || []).length,
      title: res.title
    };
  }

  /** What to tell the writer when papers can't be formatted. */
  function unknownMessage(n, offline) {
    var s = n === 1 ? '1 cited paper isn’t' : n + ' cited papers aren’t';
    if (offline) return 'Lunta isn’t running, and this document has no saved copy of ' + (n === 1 ? 'one cited paper' : n + ' cited papers') + ' in this style. Open Lunta and click Update All again. Nothing was changed.';
    return s + ' in your Lunta library, and this document has no copy of ' + (n === 1 ? 'it' : 'them') +
      ' (deleted from the library, or the document was written with another library). Nothing was changed. ' +
      'The first such citation is selected: delete it and cite the paper again, or add the paper back to Lunta.';
  }

  /** "12 citations, 9 references · APA 7", with what came from the document's own copy. */
  function summary(n, refs, name, fromDocument, offline) {
    var s = n + ' citation' + (n === 1 ? '' : 's') + ', ' + refs + ' reference' + (refs === 1 ? '' : 's') + ' · ' + name;
    if (offline) s += ' · Lunta isn’t running: updated from the copy saved in this document';
    else if (fromDocument && fromDocument.length) s += ' · ' + fromDocument.length + ' from the copy saved in this document (not in your library)';
    return s;
  }

  // ---- talking to Lunta ----

  var OFFLINE = {
    word: 'Can’t reach Lunta. Open the Lunta app on this Mac. If it is open: choose Lunta › Citations in Word & Google Docs… and click Trust Certificate (needed once, and again if Lunta made a new certificate). Don’t have Lunta yet? It’s free: github.com/christoffermattssonlangseth/lunta/releases/latest',
    gdocs: 'Can’t reach Lunta. Open the Lunta app on this Mac. If it is open: choose Lunta › Citations in Word & Google Docs… and click Trust Certificate, and allow this page to reach this device if Chrome asks (the lock icon › Site settings › Local network access).'
  };

  /**
   * fetch() with a time limit and errors that say what to do: err.kind is "offline" (no answer:
   * Lunta closed or its certificate not trusted), "timeout", or "http" (err.status, Lunta's message).
   */
  function request(url, opts) {
    opts = opts || {};
    var init = { method: opts.body ? 'POST' : 'GET', headers: Object.assign({}, opts.headers || {}) };
    if (opts.body) { init.headers['Content-Type'] = 'application/json'; init.body = JSON.stringify(opts.body); }
    var ctrl = typeof AbortController !== 'undefined' ? new AbortController() : null;
    if (ctrl) init.signal = ctrl.signal;
    var timedOut = false;
    var timer = ctrl ? setTimeout(function () { timedOut = true; ctrl.abort(); }, opts.timeout || 20000) : null;
    var f = opts.fetch || fetch;
    return f(url, init).then(function (r) {
      return r.text().then(function (t) {
        var j = null;
        try { j = t ? JSON.parse(t) : null; } catch (e) {}
        if (!r.ok) {
          var err = new Error((j && j.error) || ('Lunta answered ' + r.status + '.'));
          err.kind = 'http'; err.status = r.status;
          throw err;
        }
        if (j === null) { var bad = new Error('Lunta’s answer couldn’t be read.'); bad.kind = 'http'; bad.status = r.status; throw bad; }
        return j;
      });
    }, function (e) {
      var err = new Error(timedOut ? 'Lunta didn’t answer in time. It may be busy (importing, or downloading a journal style): try again in a moment.' : 'offline');
      err.kind = timedOut ? 'timeout' : 'offline';
      err.cause = e;
      throw err;
    }).then(function (j) { if (timer) clearTimeout(timer); return j; }, function (e) { if (timer) clearTimeout(timer); throw e; });
  }

  /** The message for an error from `request` (or anything else). */
  function describe(e, where) {
    if (e && e.kind === 'offline') return OFFLINE[where] || OFFLINE.word;
    return (e && e.message) || String(e);
  }

  // ---- journal styles (the CSL catalogue, formatted by Lunta): a style "csl:<id>" ----

  function isJournal(s) { return String(s).indexOf('csl:') === 0; }
  function isStyle(s) { return !!STYLES[s] || (isJournal(s) && /^csl:[a-z0-9._-]+$/i.test(s)); }

  function storage() { try { return window.localStorage; } catch (e) { return null; } }

  /** Journal styles used before on this computer, most recent first: [{id, title}]. */
  function recentJournals() {
    try { return JSON.parse(storage().getItem('luntaJournals') || '[]'); } catch (e) { return []; }
  }
  function rememberJournal(id, title) {
    var list = recentJournals().filter(function (j) { return j.id !== id; });
    list.unshift({ id: id, title: title });
    try { storage().setItem('luntaJournals', JSON.stringify(list.slice(0, 8))); } catch (e) {}
  }

  /** A style's display name: built-in, or the journal's title (`title` as the document has it). */
  function styleName(s, title) {
    if (!isJournal(s)) return (STYLES[s] || STYLES.apa).name;
    var id = s.slice(4), j = recentJournals().filter(function (x) { return x.id === id; })[0];
    return j ? j.title : (title || id);
  }

  /** Fills a <select>: the built-in styles, journal styles used before, and "Find journal style…" (value "__find"). */
  function fillStyles(sel, current, title) {
    sel.innerHTML = '';
    var common = document.createElement('optgroup'); common.label = 'Styles';
    Object.keys(STYLES).forEach(function (k) {
      var o = document.createElement('option'); o.value = k; o.textContent = STYLES[k].name; common.appendChild(o);
    });
    sel.appendChild(common);
    var recent = recentJournals();
    if (isJournal(current) && !recent.some(function (j) { return 'csl:' + j.id === current; })) recent.unshift({ id: current.slice(4), title: title || current.slice(4) });
    var journals = document.createElement('optgroup'); journals.label = 'Journal styles';
    recent.forEach(function (j) {
      var o = document.createElement('option'); o.value = 'csl:' + j.id; o.textContent = j.title; journals.appendChild(o);
    });
    var find = document.createElement('option'); find.value = '__find'; find.textContent = 'Find journal style…';
    journals.appendChild(find);
    sel.appendChild(journals);
    sel.value = current;
  }

  /** Lists journal-style search hits in `box`; `pick(style, title)` gets "csl:<id>" (remembered). */
  function showJournals(box, hits, pick) {
    box.innerHTML = '';
    if (!hits.length) { box.innerHTML = '<p class="status">No journal styles match.</p>'; return; }
    hits.forEach(function (h) {
      var el = document.createElement('div');
      el.className = 'hit';
      el.innerHTML = '<div class="t"></div><div class="m"></div>';
      el.querySelector('.t').textContent = h.title;
      el.querySelector('.m').textContent = h.format === 'numeric' ? 'numbered' : h.format;
      el.onclick = function () { rememberJournal(h.id, h.title); pick('csl:' + h.id, h.title); };
      box.appendChild(el);
    });
  }

  /** A search hit's line under the title: "Smith, Lee, Wu et al. · 2020 · Nature". */
  function hitMeta(h) {
    var a = h.authors || [];
    return [a.slice(0, 3).join(', ') + (a.length > 3 ? ' et al.' : ''), h.year, h.journal].filter(Boolean).join(' · ');
  }

  /** "Smith 2020; Lee 2021" for the papers picked for a citation. */
  function pickedLabel(list) {
    return list.map(function (p) {
      var f = (p.families && p.families[0]) || ((p.authors || [])[0] && familyOf(p.authors[0])) || shortTitle(p.title || 'Untitled');
      return f + (p.year ? ' ' + p.year : '');
    }).join('; ');
  }

  return {
    STYLES: STYLES, TAG: TAG, BIB_TAG: BIB_TAG, CITE_URL: CITE_URL, BIB_URL: BIB_URL,
    parseTag: parseTag, makeTag: makeTag, parseLink: parseLink, makeLink: makeLink, citedIds: citedIds, unique: unique,
    compress: compress, numbering: numbering, familyOf: familyOf, names: names, addSuffix: addSuffix,
    runs: runs, toHTML: toHTML, plain: plain, escapeHTML: escapeHTML,
    detailsFor: detailsFor, format: format, formatJournal: formatJournal, unknownMessage: unknownMessage, summary: summary,
    request: request, describe: describe,
    isJournal: isJournal, isStyle: isStyle, styleName: styleName, fillStyles: fillStyles, showJournals: showJournals,
    rememberJournal: rememberJournal, hitMeta: hitMeta, pickedLabel: pickedLabel
  };
})();
if (typeof module !== 'undefined') module.exports = LuntaCite;
