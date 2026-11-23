// Citation formatting shared by the Word add-in and the Google Docs sidebar (and tested with node:
// apple/citations-tests). A citation is a list of paper ids; numbered styles number papers by
// first appearance. Everything here is plain functions of the document's citations, what Lunta
// answers and what the document keeps of its papers; the panels only read and write documents.
var LuntaCite = (function () {
  // The styles of Lunta's core (CitationStyle), with their in-text form. Author–year styles:
  // `and` joins two authors, `sep` goes between name and year, `noYear` leaves the year out (MLA),
  // `etAl` is the number of authors from which only the first is named. Numbered styles: `open`
  // and `close` wrap the numbers, `superscript` raises them, `prefix` numbers the reference list.
  // `barePage`: a page locator without "p." ("(Smith 2020, 12)").
  var STYLES = {
    apa: { name: 'APA 7', and: '&', sep: ', ', etAl: 3 },
    harvard: { name: 'Harvard', and: 'and', sep: ', ', etAl: 4 },
    chicago: { name: 'Chicago (author-date)', and: 'and', sep: ' ', etAl: 4, serial: true, barePage: true },
    cell: { name: 'Cell', and: 'and', sep: ', ', etAl: 3 },
    mla: { name: 'MLA 9', and: 'and', noYear: true, etAl: 3, barePage: true },
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

  var PARTICLES = ['van', 'von', 'de', 'der', 'den', 'da', 'das', 'do', 'dos', 'di', 'du', 'del', 'della', 'des', 'la', 'le', 'ten', 'ter', 'zu', 'vom'];
  /** The family name in an author's name, as the core reads it ("Ludwig van Beethoven" and
   *  "Beethoven, Ludwig van" → "van Beethoven"; a stray full stop after it dropped: "Leonard." → "Leonard"). */
  function familyOf(name) {
    name = String(name || '').trim();
    var family;
    var comma = name.indexOf(',');
    if (comma >= 0) {
      family = name.slice(0, comma).trim();
      var given = name.slice(comma + 1).split(/\s+/).filter(Boolean);
      if (PARTICLES.indexOf(family.split(/\s+/)[0]) < 0) {
        while (given.length > 1 && PARTICLES.indexOf(given[given.length - 1]) >= 0) family = given.pop() + ' ' + family;
      }
    } else {
      var parts = name.split(/\s+/).filter(Boolean);
      if (parts.length <= 1) return name;
      var start = parts.length - 1;
      while (start > 1 && PARTICLES.indexOf(parts[start - 1].toLowerCase()) >= 0) start--;
      family = parts.slice(start).join(' ');
    }
    var last = family.split(' ').pop();
    if (/^\S[a-z\u00df-\u024f'-]{3,}\.$/.test(last)) family = family.slice(0, -1);
    return family;
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

  // ---- what the writer adds to a cited paper (Zotero's citation dialog) ----
  // Per paper in a citation: {prefix ("see"), suffix ("for a review"), locator ("12"), label (a CSL
  // locator type, "page" when empty), mode ("" | "suppress": without the author, "(2020)" |
  // "narrative": the author in the text, "Smith (2020)")}. A citation's options: {id: options}.
  // Kept outside the paper ids, so panels from before them still read the citation: in Word in
  // the content control's title ("Citation lunta-o:<code>"), in Google Docs in named ranges over
  // the citation's text ("lunta-o:<key>:<i>:<n>:<part of the code>"); the code is base64url of
  // the JSON with short keys.

  var LOCATORS = [
    // [CSL type, short, short plural, name in the menu]
    ['page', 'p.', 'pp.', 'Page'], ['chapter', 'chap.', 'chaps.', 'Chapter'], ['section', 'sec.', 'secs.', 'Section'],
    ['figure', 'fig.', 'figs.', 'Figure'], ['table', 'tbl.', 'tbls.', 'Table'], ['paragraph', 'para.', 'paras.', 'Paragraph'],
    ['line', 'l.', 'll.', 'Line'], ['note', 'n.', 'nn.', 'Note'], ['volume', 'vol.', 'vols.', 'Volume'],
    ['equation', 'eq.', 'eqs.', 'Equation'], ['supplement', 'suppl.', 'suppls.', 'Supplement'], ['appendix', 'app.', 'apps.', 'Appendix']
  ];
  var MODES = ['', 'suppress', 'narrative'];
  var OPTS_TITLE = 'Citation', OPTS_MARK = 'lunta-o:', OPTS_CHUNK = 200;

  function locatorType(label) {
    var l = String(label || '').trim().toLowerCase();
    for (var i = 0; i < LOCATORS.length; i++) if (LOCATORS[i][0] === l) return LOCATORS[i];
    return LOCATORS[0];
  }

  /** One paper's options without empty fields; null when there is nothing. */
  function cleanOption(o) {
    if (!o || typeof o !== 'object') return null;
    var out = {}, any = false;
    ['prefix', 'suffix', 'locator'].forEach(function (k) {
      var v = clip(String(o[k] == null ? '' : o[k]).replace(/\s+/g, ' ').trim(), 300);
      if (v) { out[k] = v; any = true; }
    });
    if (out.locator && o.label && locatorType(o.label)[0] !== 'page') out.label = locatorType(o.label)[0];
    if (o.mode && MODES.indexOf(o.mode) > 0) { out.mode = o.mode; any = true; }
    return any ? out : null;
  }

  /** A citation's options, only for its papers and only what isn't empty; null when none. */
  function cleanOptions(map, ids) {
    if (!map) return null;
    var out = {}, any = false;
    (ids || Object.keys(map)).forEach(function (id) {
      var o = cleanOption(map[id]);
      if (o) { out[id] = o; any = true; }
    });
    return any ? out : null;
  }

  // UTF-8 and base64url by hand: the same in browsers, node, JavaScriptCore and Apps Script
  var B64 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_';
  function utf8(str) {
    var out = [];
    for (var i = 0; i < str.length; i++) {
      var c = str.charCodeAt(i);
      if (c >= 0xd800 && c < 0xdc00 && i + 1 < str.length) { c = 0x10000 + ((c - 0xd800) << 10) + (str.charCodeAt(++i) - 0xdc00); }
      if (c < 0x80) out.push(c);
      else if (c < 0x800) out.push(0xc0 | (c >> 6), 0x80 | (c & 63));
      else if (c < 0x10000) out.push(0xe0 | (c >> 12), 0x80 | ((c >> 6) & 63), 0x80 | (c & 63));
      else out.push(0xf0 | (c >> 18), 0x80 | ((c >> 12) & 63), 0x80 | ((c >> 6) & 63), 0x80 | (c & 63));
    }
    return out;
  }
  function fromUtf8(b) {
    var s = '';
    for (var i = 0; i < b.length;) {
      var c = b[i++];
      if (c >= 0xf0) c = ((c & 7) << 18) | ((b[i++] & 63) << 12) | ((b[i++] & 63) << 6) | (b[i++] & 63);
      else if (c >= 0xe0) c = ((c & 15) << 12) | ((b[i++] & 63) << 6) | (b[i++] & 63);
      else if (c >= 0xc0) c = ((c & 31) << 6) | (b[i++] & 63);
      if (c >= 0x10000) { c -= 0x10000; s += String.fromCharCode(0xd800 + (c >> 10), 0xdc00 + (c & 1023)); } else s += String.fromCharCode(c);
    }
    return s;
  }
  function b64url(bytes) {
    var out = '';
    for (var i = 0; i < bytes.length; i += 3) {
      var n = (bytes[i] << 16) | ((bytes[i + 1] || 0) << 8) | (bytes[i + 2] || 0);
      out += B64[n >> 18] + B64[(n >> 12) & 63] + (i + 1 < bytes.length ? B64[(n >> 6) & 63] : '') + (i + 2 < bytes.length ? B64[n & 63] : '');
    }
    return out;
  }
  function unB64url(s) {
    var out = [], acc = 0, bits = 0;
    for (var i = 0; i < s.length; i++) {
      var v = B64.indexOf(s.charAt(i));
      if (v < 0) throw new Error('bad code');
      acc = (acc << 6) | v; bits += 6;
      if (bits >= 8) { bits -= 8; out.push((acc >> bits) & 255); }
    }
    return out;
  }

  var SHORT = { prefix: 'p', suffix: 's', locator: 'l', label: 't', mode: 'm' };
  /** A citation's options as a code ("" when none). */
  function encodeOptions(map) {
    map = cleanOptions(map);
    if (!map) return '';
    var j = {};
    Object.keys(map).forEach(function (id) {
      var o = {};
      Object.keys(map[id]).forEach(function (k) { o[SHORT[k]] = map[id][k]; });
      j[id] = o;
    });
    return b64url(utf8(JSON.stringify(j)));
  }
  /** The options in a code; null when none or unreadable. */
  function decodeOptions(code) {
    if (!code) return null;
    try {
      var j = JSON.parse(fromUtf8(unB64url(String(code)))), out = {};
      Object.keys(j || {}).forEach(function (id) {
        var o = {};
        Object.keys(SHORT).forEach(function (k) { if (j[id][SHORT[k]] != null) o[k] = j[id][SHORT[k]]; });
        out[id] = o;
      });
      return cleanOptions(out);
    } catch (e) { return null; }
  }

  /** Word: the content control's title for a citation with these options. */
  function optionsTitle(map) {
    var code = encodeOptions(map);
    return code ? OPTS_TITLE + ' ' + OPTS_MARK + code : OPTS_TITLE;
  }
  /** Word: the options in a content control's title (null when none). */
  function optionsOfTitle(title) {
    var t = String(title || ''), i = t.indexOf(OPTS_MARK);
    return i < 0 ? null : decodeOptions(t.slice(i + OPTS_MARK.length).split(/\s/)[0]);
  }

  /** Google Docs: the named ranges' names for a citation's options code ([] when none). */
  function optionRangeNames(code, key) {
    if (!code) return [];
    key = key || newKey();
    var n = Math.ceil(code.length / OPTS_CHUNK), out = [];
    for (var i = 0; i < n; i++) out.push(OPTS_MARK + key + ':' + i + ':' + n + ':' + code.slice(i * OPTS_CHUNK, (i + 1) * OPTS_CHUNK));
    return out;
  }
  /** {key, i, n, data} of an options range's name, or null. */
  function optionRangePart(name) {
    var m = /^lunta-o:([A-Za-z0-9]+):(\d+):(\d+):([A-Za-z0-9_-]*)$/.exec(String(name || ''));
    return m ? { key: m[1], i: +m[2], n: +m[3], data: m[4] } : null;
  }
  /** The options code from the names of the ranges over one citation ("" when incomplete). */
  function optionsCodeOf(names) {
    var groups = {};
    (names || []).forEach(function (name) {
      var p = optionRangePart(name);
      if (!p) return;
      var g = groups[p.key] = groups[p.key] || { n: p.n, parts: {} };
      if (p.n === g.n && p.i < p.n) g.parts[p.i] = p.data;
    });
    var best = '';
    Object.keys(groups).forEach(function (k) {
      var g = groups[k], code = '';
      for (var i = 0; i < g.n; i++) { if (g.parts[i] === undefined) return; code += g.parts[i]; }
      if (!best) best = code;
    });
    return best;
  }
  function newKey() {
    var s = '';
    for (var i = 0; i < 8; i++) s += 'abcdefghijklmnopqrstuvwxyz0123456789'.charAt(Math.floor(Math.random() * 36));
    return s;
  }

  /** "p. 12", "pp. 3–5", "chap. 2"; `bare`: a page without its label ("12"). */
  function locatorText(o, bare) {
    if (!o || !o.locator) return '';
    var loc = String(o.locator).replace(/(\d)\s*-\s*(\d)/g, '$1–$2');
    var t = locatorType(o.label), plural = /[–,&]|\band\b/.test(loc);
    if (bare && t[0] === 'page') return loc;
    return (plural ? t[2] : t[1]) + ' ' + loc;
  }

  /** A prefix and a suffix around a part of a citation ("see " + x + " for a review"). */
  function affix(o, text) {
    if (!o) return text;
    var out = text;
    if (o.prefix) out = o.prefix + ' ' + out;
    if (o.suffix) out += (/^[,.;:)\]]/.test(o.suffix) ? '' : ' ') + o.suffix;
    return out;
  }

  /** A short line for a paper's options, for the panels ("see · p. 12 · without author"). */
  function optionsLabel(o) {
    o = cleanOption(o);
    if (!o) return '';
    var out = [];
    if (o.prefix) out.push('“' + o.prefix + '” before');
    if (o.locator) out.push(locatorText(o));
    if (o.suffix) out.push('“' + o.suffix + '” after');
    if (o.mode === 'suppress') out.push('without author');
    if (o.mode === 'narrative') out.push('author in text');
    return out.join(' · ');
  }

  /** The options for Lunta's /render (one per citation, null when none), or null when no citation has any. */
  function renderOptions(options, citations) {
    var any = false;
    var out = (citations || []).map(function (l, i) {
      var m = cleanOptions((options || [])[i], l);
      if (m) any = true;
      return m;
    });
    return any ? out : null;
  }

  // ---- formatting a document ----

  /**
   * Everything Update All writes, for a built-in style.
   *   citations: the document's citations in order (lists of ids)
   *   entries:   Lunta's answer to /api/bibliography (null: Lunta can't be reached)
   *   store:     what the document keeps of its papers (see above)
   *   options:   per citation, its papers' options ({id: options}, see above) or null
   * Returns {texts (one per citation), superscript, superscripts (one per citation: a narrative
   * citation isn't raised), befores and afters (one per citation: the start and end of a raised
   * citation's text that stay on the line, "see " in "see ⁴"; '' when none), bibliography [{id, text}], unknown (ids with
   * nothing to format them from: write nothing), fromDocument (ids formatted from the document's
   * copy), store (to keep), references}.
   */
  function format(style, citations, entries, store, options) {
    options = options || [];
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
    var result = { texts: [], superscript: !!st.superscript, superscripts: citations.map(function () { return !!st.superscript; }),
      befores: citations.map(function () { return ''; }), afters: citations.map(function () { return ''; }),
      bibliography: [], unknown: unknown, fromDocument: fromDocument, store: newStore, references: 0 };
    if (unknown.length) return result;

    // the same paper under two ids (merged in the library): one reference
    var canon = function (id) { return byId[id].libraryId || id; };
    var lists = citations.map(function (l) { return unique(l.map(canon)); });
    // each citation's options, by the papers' ids as numbered
    var opts = citations.map(function (l, i) {
      var m = options[i], out = {};
      if (!m) return out;
      l.forEach(function (id) { var o = cleanOption(m[id]); if (o) out[canon(id)] = o; });
      return out;
    });
    var papers = {};
    ids.forEach(function (id) { if (!papers[canon(id)]) papers[canon(id)] = byId[id]; });
    var num = numbering(lists);
    result.references = num.list.length;

    if (st.numeric) {
      result.texts = lists.map(function (l, i) {
        if (!l.length) return null;
        var o = opts[i], plainNums = [], extra = [], narrative = [];
        l.forEach(function (id) {
          var n = num.order[id], x = o[id];
          var inner = x && x.locator ? (st.superscript ? n + '(' + locatorText(x).replace(/\.\s?/g, '') + ')' : n + ', ' + locatorText(x)) : String(n);
          if (x && x.mode === 'narrative') narrative.push(affix(x, names(papers[id], st) + ' ' + (st.open || '[') + n + (x.locator ? ', ' + locatorText(x) : '') + (st.close || ']')));
          else if (x && (x.prefix || x.suffix || x.locator)) extra.push({ n: n, text: affix(x, inner), inner: inner, o: x });
          else plainNums.push(n);
        });
        // raised: the first paper's prefix and the last one's suffix stay on the line ("see ⁴")
        var before = '', after = '';
        if (st.superscript && !narrative.length && extra.length) {
          var nums = plainNums.concat(extra.map(function (x) { return x.n; }));
          var lo = Math.min.apply(null, nums), hi = Math.max.apply(null, nums);
          extra.forEach(function (x) {
            if (x.n === lo && x.o.prefix) { before = affix({ prefix: x.o.prefix }, '').replace(/^\s*/, ''); x.text = affix({ suffix: x.n === hi ? '' : x.o.suffix }, x.inner); }
            if (x.n === hi && x.o.suffix) { after = affix({ suffix: x.o.suffix }, ''); x.text = affix({ prefix: x.n === lo ? '' : x.o.prefix }, x.inner); }
          });
        }
        result.befores[i] = before; result.afters[i] = after;
        // in number order; plain numbers between them as ranges ("[1–3; see 4, p. 2]")
        var parts = [];
        if (!extra.length) parts = plainNums.length ? [compress(plainNums)] : [];
        else {
          var all = plainNums.map(function (n) { return { n: n }; }).concat(extra).sort(function (a, b) { return a.n - b.n; }), run = [];
          all.forEach(function (x) {
            if (x.text === undefined) { run.push(x.n); return; }
            if (run.length) { parts.push(compress(run)); run = []; }
            parts.push(x.text);
          });
          if (run.length) parts.push(compress(run));
        }
        var body = parts.length ? (st.open || '') + parts.join(extra.length ? '; ' : ',') + (st.close || '') : '';
        if (!narrative.length) return before + body + after;
        result.superscripts[i] = false;
        // the rest of a narrative citation can't be raised in the same text: in brackets
        if (body && st.superscript) body = '[' + parts.join(extra.length ? '; ' : ',') + ']';
        return narrative.join('; ') + (body ? ' ' + body : '');
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
    result.texts = lists.map(function (l, i) {
      if (!l.length) return null;
      var o = opts[i];
      var parts = l.map(function (id) {
        var p = papers[id], who = names(p, st);
        var what = st.noYear ? (suffix[id] === 'title' ? '“' + shortTitle(p.title || '') + '”' : '') : yearOf(p) + (suffix[id] || '');
        return { who: who, what: what, o: o[id] || null };
      }).sort(function (a, b) {
        return a.who.localeCompare(b.who, undefined, { sensitivity: 'base' }) || a.what.localeCompare(b.what);
      });
      // works by the same names together: "Smith, 2020a, 2020b" (not those with options)
      var out = [], narrative = [];
      parts.forEach(function (x) {
        var last = out[out.length - 1];
        if (x.o && x.o.mode === 'narrative') narrative.push(x);
        else if (last && !last.o && !x.o && last.who === x.who && x.what) last.whats.push(x.what);
        else out.push({ who: x.who, whats: x.what ? [x.what] : [], o: x.o });
      });
      // "(Smith, 2020, p. 12)", MLA "(Smith 12)"
      var loc = function (x) {
        var t = locatorText(x.o, st.barePage);
        if (!t) return '';
        return (st.noYear && locatorType(x.o.label)[0] === 'page' ? ' ' : ', ') + t;
      };
      var inner = out.map(function (x) {
        var suppress = x.o && x.o.mode === 'suppress';
        var text = suppress ? x.whats.join(', ') : !x.whats.length ? x.who : x.who + (st.noYear ? ', ' : st.sep) + x.whats.join(', ');
        if (!text) return affix(x.o, x.who + loc(x));   // MLA without the author: nothing else to cite by
        return affix(x.o, text + (suppress && !x.whats.length ? '' : loc(x)));
      }).join('; ');
      // "Smith (2020, p. 12)": the prefix before the names, the suffix in the brackets
      var told = narrative.map(function (x) {
        var t = (x.what + loc(x)).replace(/^[ ,]+/, '');
        t = affix({ suffix: x.o.suffix }, t).trim();
        return (x.o.prefix ? x.o.prefix + ' ' : '') + x.who + (t ? ' (' + t + ')' : '');
      });
      return told.concat(inner ? ['(' + inner + ')'] : []).join(told.length && inner ? ' ' : '');
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
    // a raised citation's start and end that aren't raised ("see " before "⁴"); an older Lunta doesn’t say
    var ends = function (i, k) {
      var c = res.citations && res.citations[i], t = texts[i] || '', v = c && c.superscript && typeof c[k] === 'string' ? c[k] : '';
      return v && (k === 'before' ? t.indexOf(v) === 0 : t.length >= v.length && t.slice(t.length - v.length) === v) ? v : '';
    };
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
      befores: citations.map(function (l, i) { return ends(i, 'before'); }),
      afters: citations.map(function (l, i) { return ends(i, 'after'); }),
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

  // ---- the options form (the panels' Edit Citation: one per paper) ----

  /**
   * A form for one paper's options: locator (kind and number), prefix, suffix and how the author
   * is shown. Enter or Apply calls `save(options)`, Escape or Cancel `cancel()`. Every field is a
   * plain input or select, in reading order, so Tab moves through it.
   */
  function optionsForm(doc, o, save, cancel) {
    o = cleanOption(o) || {};
    var box = doc.createElement('div');
    box.className = 'opts';
    var el = function (tag, cls, attrs) {
      var e = doc.createElement(tag);
      if (cls) e.className = cls;
      Object.keys(attrs || {}).forEach(function (k) { e.setAttribute ? e.setAttribute(k, attrs[k]) : (e[k] = attrs[k]); });
      return e;
    };
    var row = function () { var r = el('div', 'orow'); box.appendChild(r); return r; };
    var r1 = row();
    var label = el('select', 'olabel', { title: 'Kind of locator', 'aria-label': 'Kind of locator' });
    LOCATORS.forEach(function (t) { var op = doc.createElement('option'); op.value = t[0]; op.textContent = t[3]; label.appendChild(op); });
    label.value = locatorType(o.label)[0];
    var loc = el('input', 'oloc', { type: 'text', placeholder: '12 or 3–5', 'aria-label': 'Locator', autocomplete: 'off' });
    loc.value = o.locator || '';
    r1.appendChild(label); r1.appendChild(loc);
    var r2 = row();
    var pre = el('input', 'opre', { type: 'text', placeholder: 'Prefix: see, e.g.', 'aria-label': 'Prefix', autocomplete: 'off' });
    var suf = el('input', 'osuf', { type: 'text', placeholder: 'Suffix: for a review', 'aria-label': 'Suffix', autocomplete: 'off' });
    pre.value = o.prefix || ''; suf.value = o.suffix || '';
    r2.appendChild(pre); r2.appendChild(suf);
    var r3 = row();
    var mode = el('select', 'omode', { title: 'The author', 'aria-label': 'The author' });
    [['', 'Author and year: (Smith, 2020)'], ['suppress', 'Without author: (2020)'], ['narrative', 'Author in the text: Smith (2020)']].forEach(function (m) {
      var op = doc.createElement('option'); op.value = m[0]; op.textContent = m[1]; mode.appendChild(op);
    });
    mode.value = o.mode || '';
    r3.appendChild(mode);
    var r4 = row();
    var apply = el('button', 'oapply primary btn'), no = el('button', 'ocancel btn');
    apply.textContent = 'Apply'; no.textContent = 'Cancel';
    r4.appendChild(apply); r4.appendChild(no);
    var value = function () {
      return cleanOption({ label: label.value, locator: loc.value, prefix: pre.value, suffix: suf.value, mode: mode.value }) || {};
    };
    apply.onclick = function () { save(value()); };
    no.onclick = function () { if (cancel) cancel(); };
    var keys = function (e) {
      if (e.key === 'Enter' && e.target && e.target.tagName !== 'BUTTON') { e.preventDefault(); save(value()); }
      else if (e.key === 'Escape') { e.preventDefault(); if (cancel) cancel(); }
    };
    if (box.addEventListener) box.addEventListener('keydown', keys);
    box.focusFirst = function () { try { loc.focus(); } catch (e) {} };
    box.value = value;
    return box;
  }

  // ---- converting citations made with Zotero, EndNote or Mendeley ----
  // The panels read the other manager's citations from the document (Word: word/fields.js; Google
  // Docs: Zotero's links and named ranges), each a list of works {meta, options}. Lunta finds the
  // works in the library (/match), the writer confirms the papers to add, and only then is anything
  // written. A citation is converted only when every one of its works has a paper; the others stay
  // as they are and are listed.

  /** The JSON object starting at the first "{" from `from` (Zotero may put text after it). */
  function jsonAt(s, from) {
    s = String(s || '');
    var start = s.indexOf('{', from || 0);
    if (start < 0) return null;
    var depth = 0, inStr = false;
    for (var i = start; i < s.length; i++) {
      var c = s.charAt(i);
      if (inStr) {
        if (c === '\\') i++;
        else if (c === '"') inStr = false;
      } else if (c === '"') inStr = true;
      else if (c === '{') depth++;
      else if (c === '}' && --depth === 0) {
        try { return JSON.parse(s.slice(start, i + 1)); } catch (e) { return null; }
      }
    }
    return null;
  }

  function stripTags(s) {
    // the markup CSL-JSON allows in titles (<i>, <b>, <sup>, <sub>, <span style=…>, <sc>), not "<10 Hz"
    return String(s == null ? '' : s).replace(/<\/?(?:i|b|em|strong|sup|sub|sc|span)(?:\s[^>]*)?>/gi, '').replace(/&amp;/g, '&').replace(/&lt;/g, '<').replace(/&gt;/g, '>')
      .replace(/&quot;/g, '"').replace(/&#39;/g, "'").replace(/\s+/g, ' ').trim();
  }

  function pad(n) { n = String(n); return n.length === 1 ? '0' + n : n; }

  /** A CSL-JSON date ({date-parts}, {raw}, {literal}) as "2020-03-01", "2020" or "". */
  function cslDate(d) {
    if (!d) return '';
    var parts = d['date-parts'] && d['date-parts'][0];
    if (parts && parts.length && /^\d{4}$/.test(String(parts[0]).trim())) {
      var out = String(parts[0]).trim();
      if (parts[1] && +parts[1] >= 1 && +parts[1] <= 12) out += '-' + pad(+parts[1]);
      if (parts[1] && parts[2] && +parts[2] >= 1 && +parts[2] <= 31) out += '-' + pad(+parts[2]);
      return out;
    }
    var y = /\b(1[5-9]\d\d|20\d\d)\b/.exec(String(d.raw || d.literal || d.season || ''));
    return y ? y[1] : '';
  }

  function cslName(n) {
    if (!n) return '';
    if (n.literal) return stripTags(n.literal);
    var family = [n['non-dropping-particle'], n.family].filter(Boolean).join(' ');
    var given = [n.given, n['dropping-particle']].filter(Boolean).join(' ');
    var suffix = n.suffix ? ', ' + n.suffix : '';
    return stripTags(given ? given + ' ' + family + suffix : family + suffix);
  }

  /** A work's details (as Lunta's API takes them) from CSL-JSON (Zotero's and Mendeley's itemData). */
  function cslMeta(d) {
    d = d || {};
    var get = function (k) { var v = d[k] != null ? d[k] : d[k.toLowerCase()]; return v == null ? '' : stripTags(v); };
    var m = { title: get('title'), authors: (d.author || []).map(cslName).filter(Boolean), date: cslDate(d.issued),
      journal: get('container-title'), volume: get('volume'), issue: get('issue'), pages: get('page'), publisher: get('publisher'),
      doi: get('DOI').replace(/^https?:\/\/(dx\.)?doi\.org\//i, '').replace(/^doi:\s*/i, ''), pmid: get('PMID'), pmcid: get('PMCID'), url: get('URL') };
    if (!m.authors.length && d.editor) m.authors = d.editor.map(cslName).filter(Boolean);
    // a web page or blog post (Zotero's "Web Page" and "Blog Post"): its type, and the day it was read
    var type = String(d.type || '').toLowerCase();
    if (type === 'webpage' || type === 'post-weblog' || type === 'post') {
      m.work_type = type === 'post' ? 'webpage' : type;
      m.accessed = cslDate(d.accessed);
    }
    if (!m.pmid) { var pm = /PMID:\s*(\d+)/i.exec(String(d.note || '')); if (pm) m.pmid = pm[1]; }
    Object.keys(m).forEach(function (k) { if (!m[k] || (Array.isArray(m[k]) && !m[k].length)) delete m[k]; });
    return m;
  }

  /** The options of a CSL cite item (locator, label, prefix, suffix, suppress-author, author-only). */
  function cslOptions(it) {
    return cleanOption({
      locator: it.locator != null ? stripTags(it.locator) : '', label: it.label || '',
      prefix: stripTags(it.prefix), suffix: stripTags(it.suffix),
      mode: it['suppress-author'] ? 'suppress' : it['author-only'] ? 'narrative' : ''
    });
  }

  /** The works of a CSL citation (Zotero's or Mendeley's field code JSON): [{meta, options}] or null. */
  function cslCitation(json) {
    if (!json || !Array.isArray(json.citationItems)) return null;
    return json.citationItems.map(function (it) {
      return { meta: cslMeta(it.itemData || {}), options: cslOptions(it) };
    });
  }

  /**
   * A Zotero code in Google Docs (named ranges "Z_F<key><nnn><part>", put together):
   * {kind: "cite", works} for "ITEM CSL_CITATION {…}", {kind: "bib"} for "BIBL …", or null.
   */
  function zoteroDocsCode(code) {
    code = String(code || '');
    if (/^BIBL\b/.test(code)) return { kind: 'bib' };
    if (!/^ITEM CSL_CITATION/.test(code)) return null;
    var works = cslCitation(jsonAt(code, 0));
    return works ? { kind: 'cite', works: works } : null;
  }

  /** Zotero's named ranges for one field key put together in order ("" when a part is missing). */
  function zoteroDocsJoin(names, key) {
    var prefix = 'Z_F' + key, parts = [];
    (names || []).forEach(function (n) {
      if (String(n).indexOf(prefix) !== 0) return;
      var i = parseInt(n.substr(prefix.length, 3), 10);
      if (!isNaN(i)) parts[i] = n.substr(prefix.length + 3);
    });
    for (var i = 0; i < parts.length; i++) if (parts[i] === undefined) return '';
    return parts.join('');
  }
  var ZOTERO_DOCS_URL = 'https://www.zotero.org/google-docs/?';

  /** A work's key for finding the same work in several citations: its DOI, else its title and year, else its PubMed id. */
  function workKey(m) {
    var doi = String(m.doi || '').toLowerCase().replace(/^https?:\/\/(dx\.)?doi\.org\//, '').trim();
    if (doi) return 'doi:' + doi;
    var t = String(m.title || '').toLowerCase().replace(/[^a-z0-9À-￿]+/g, '');
    if (t) return 't:' + t + ':' + String(m.date || '').slice(0, 4);
    // only a PubMed id (a ReadCube Papers citation whose bibliography entry wasn't found)
    return /^\d+$/.test(String(m.pmid || '').trim()) ? 'pmid:' + String(m.pmid).trim() : '';
  }

  /**
   * The conversion worked out from the citations found ([{works: [{meta, options}], text, source}])
   * and Lunta's /match answer for `plan.works` (in that order): {citations, works: [{key, meta,
   * id, how}], unusable (works with nothing to find them by)}. Call convertPlan(found) first for the
   * works to ask about, then convertPlan(found, matches).
   */
  function convertPlan(found, matches) {
    var works = [], byKey = {};
    (found || []).forEach(function (c) {
      (c.works || []).forEach(function (w) {
        var k = workKey(w.meta || {});
        if (!k) return;
        if (!byKey.hasOwnProperty(k)) { byKey[k] = works.length; works.push({ key: k, meta: w.meta, id: '', how: '' }); }
      });
    });
    (matches || []).forEach(function (m, i) { if (works[i] && m && m.id) { works[i].id = m.id; works[i].how = m.how || ''; } });
    return { citations: found || [], works: works, byKey: byKey };
  }

  /** "12 citations, 34 papers: 30 in your library, 4 to add" */
  function convertSummary(plan) {
    var n = plan.citations.length, w = plan.works.length;
    var have = plan.works.filter(function (x) { return x.id; }).length;
    var s = n + ' citation' + (n === 1 ? '' : 's') + ', ' + w + ' paper' + (w === 1 ? '' : 's') + ': ' + have + ' in your library';
    if (w - have) s += ', ' + (w - have) + ' to add';
    return s;
  }

  /**
   * What to write once the papers to add are added (`added`: {work key: library id}): {convert:
   * [{index, ids, options}], left: [{index, text, reason}]} — a citation is converted only when
   * every work in it has a paper.
   */
  function convertResolve(plan, added) {
    added = added || {};
    var out = { convert: [], left: [] };
    plan.citations.forEach(function (c, i) {
      var ids = [], options = {}, missing = '';
      (c.works || []).forEach(function (w) {
        var k = workKey(w.meta || {}), x = k ? plan.works[plan.byKey[k]] : null;
        // added: also a work the writer said isn't the library's paper found by a similar title
        var id = x ? (added[k] || x.id || '') : '';
        if (!id) { if (!missing) missing = (w.meta && w.meta.title) ? '“' + w.meta.title + '” isn’t in your library' : 'a work without a title or DOI'; return; }
        if (ids.indexOf(id) < 0) ids.push(id);
        if (w.options) options[id] = w.options;
      });
      if (!c.works || !c.works.length) missing = 'no details in the citation';
      if (missing) out.left.push({ index: i, text: c.text || '', reason: missing });
      else out.convert.push({ index: i, ids: ids, options: cleanOptions(options, ids) });
    });
    return out;
  }

  /**
   * The review before converting, in `box`: the summary, the papers to add (each can be unticked),
   * the papers found only by a similar title (each can be unticked: then it's added as a paper of
   * its own), what can't be converted, and Convert / Cancel. `onConvert(keysToAdd)`; nothing is
   * written before.
   */
  function convertReview(doc, box, plan, extra, onConvert, onCancel) {
    box.innerHTML = '';
    var add = plan.works.filter(function (w) { return !w.id; });
    var el = function (tag, cls, text) { var e = doc.createElement(tag); if (cls) e.className = cls; if (text != null) e.textContent = text; return e; };
    box.appendChild(el('p', 'cv-sum', convertSummary(plan) + '.'));
    var checks = [];
    if (add.length) {
      box.appendChild(el('div', 'cv-h', 'To add to your library'));
      add.forEach(function (w) {
        var row = el('label', 'cv-row');
        var cb = el('input'); cb.type = 'checkbox'; cb.checked = true;
        checks.push({ cb: cb, key: w.key });
        var t = el('span', 'cv-t');
        t.textContent = (w.meta.title || (w.meta.doi ? 'doi:' + w.meta.doi : w.meta.pmid ? 'PMID ' + w.meta.pmid : 'Untitled')) + ' · ' + hitMeta({ authors: w.meta.authors, year: String(w.meta.date || '').slice(0, 4), journal: w.meta.journal });
        row.appendChild(cb); row.appendChild(t);
        box.appendChild(row);
      });
    }
    // found by first author, year and a similar title only: a companion paper may look the same
    var close = plan.works.filter(function (w) { return w.id && w.how === 'close'; });
    var notSame = [];
    if (close.length) {
      box.appendChild(el('div', 'cv-h', 'Found in your library by a similar title (untick any that is another paper: it’s added instead)'));
      close.forEach(function (w) {
        var row = el('label', 'cv-row');
        var cb = el('input'); cb.type = 'checkbox'; cb.checked = true;
        notSame.push({ cb: cb, key: w.key });
        var t = el('span', 'cv-t');
        t.textContent = (w.meta.title || (w.meta.doi ? 'doi:' + w.meta.doi : w.meta.pmid ? 'PMID ' + w.meta.pmid : 'Untitled')) + ' · ' + hitMeta({ authors: w.meta.authors, year: String(w.meta.date || '').slice(0, 4), journal: w.meta.journal });
        row.appendChild(cb); row.appendChild(t);
        box.appendChild(row);
      });
    }
    (extra || []).forEach(function (line) { box.appendChild(el('p', 'cv-note', line)); });
    box.appendChild(el('p', 'cv-note', 'Nothing changes until you click Convert. A citation whose papers aren’t all in your library (or added now) stays as it is and is listed afterwards. Keep a copy of the document first: File › Save a Copy in Word; Google Docs keeps the old version in its version history.'));
    var b = el('div', 'buttons');
    var go = el('button', 'primary btn cv-go', 'Convert'), no = el('button', 'btn cv-no', 'Cancel');
    go.onclick = function () {
      onConvert(checks.filter(function (c) { return c.cb.checked; }).concat(notSame.filter(function (c) { return !c.cb.checked; })).map(function (c) { return c.key; }));
    };
    no.onclick = function () { onCancel(); };
    b.appendChild(go); b.appendChild(no);
    box.appendChild(b);
    return { go: go, checks: checks, close: notSame };
  }

  /** What the panel says after converting. */
  function convertDone(n, left, added) {
    var s = 'Converted ' + n + ' citation' + (n === 1 ? '' : 's') + (added ? ' (' + added + ' paper' + (added === 1 ? '' : 's') + ' added to your library)' : '') + '.';
    if (left.length) s += ' ' + left.length + ' left as they were: ' + left.slice(0, 5).map(function (l) { return (l.text ? '“' + l.text + '”: ' : '') + l.reason; }).join('; ') + (left.length > 5 ? '; …' : '') + '.';
    return s;
  }

  // ---- Check Citations (Lunta's /claims; core/src/claims) ----
  // For each citation, the sentence it is in and the cited papers' passages that best match it,
  // with (when Lunta has an on-device model) a suggested verdict whose quotes are checked word for
  // word against the paper. Lunta proposes, the writer decides: "Looks right" or "Dismiss" per
  // citation, remembered on this computer. Nothing in the document changes.

  var CLAIM_VERDICTS = ['contradicted', 'notFound', 'partly', 'possible', 'noText', 'supported'];
  var CLAIM_NAMES = { supported: 'supported', partly: 'partly supported', contradicted: 'contradicted', notFound: 'not found in the paper',
    possible: 'possible support', noText: 'no text to check', noMatch: 'no matching passage' };
  var CLAIM_PRIVACY = 'Checked on this computer by Lunta: the sentences and the papers stay on it, nothing is sent anywhere.';
  var CLAIM_PRINCIPLE = 'Lunta proposes, you decide: for each citation, the passages of the cited paper that best match the sentence, and, with an on-device model, a suggested verdict whose quotes are checked word for word against the paper. It can be wrong: read the passages.';

  /**
   * The citations to check, from paragraphs as the panel reads them: [{text, cites: [{text, ids}]}]
   * (a paragraph's text as shown, its citations in order, each as its text shows). Each citation
   * is found in its paragraph after the one before it: [{ids, text, start, end, others, before,
   * cite, key}] (offsets are JavaScript's, UTF-16, as Lunta wants them). `before`: the paragraph before.
   */
  function claimSites(paras) {
    var out = [], prev = '';
    (paras || []).forEach(function (p) {
      var text = String(p.text || ''), at = 0, spans = [];
      (p.cites || []).forEach(function (c) {
        var t = String(c.text || '');
        if (!t) return;
        var i = text.indexOf(t, at);
        if (i < 0) return;
        spans.push({ start: i, end: i + t.length, ids: unique(c.ids), cite: t });
        at = i + t.length;
      });
      spans.forEach(function (s, k) {
        if (!s.ids.length) return;
        out.push({ ids: s.ids, text: text, start: s.start, end: s.end, cite: s.cite, before: prev,
          others: spans.filter(function (o, j) { return j !== k; }).map(function (o) { return [o.start, o.end]; }) });
      });
      if (text.trim()) prev = text;
    });
    return out;
  }

  /** What to ask Lunta for a citation (POST /claims). */
  function claimBody(site, store, judge) {
    return { text: clip(site.text, 20000), start: site.start, end: site.end, others: site.others || [], before: clip(site.before || '', 20000),
      ids: site.ids, papers: detailsFor(store, site.ids), judge: judge !== false };
  }

  /** Where a decision about a citation is remembered: its papers and the sentence checked. */
  function claimKey(ids, claim) { return unique(ids).join(',') + '|' + String(claim || '').replace(/\s+/g, ' ').trim(); }

  function claimDecisions() {
    try { return JSON.parse((storage() && storage().getItem('luntaClaims')) || '{}') || {}; } catch (e) { return {}; }
  }
  /** Remembers "right" (Looks right), "dismissed", or nothing (null). At most 2,000 kept. */
  function rememberClaim(key, state) {
    var all = claimDecisions();
    if (state) all[key] = state; else delete all[key];
    var keys = Object.keys(all);
    if (keys.length > 2000) keys.slice(0, keys.length - 2000).forEach(function (k) { delete all[k]; });
    try { storage().setItem('luntaClaims', JSON.stringify(all)); } catch (e) {}
    return all;
  }

  /**
   * Checks the citations one after another: ask(body) → Lunta's answer (a promise); progress(i, n)
   * before each; stopped() true ends early. → [{site, answer | null, error | null}].
   */
  function runClaims(sites, ask, progress, stopped, store, judge) {
    var out = [], i = 0;
    function next() {
      if (i >= sites.length || (stopped && stopped())) return Promise.resolve(out);
      var s = sites[i];
      if (progress) progress(i, sites.length);
      i++;
      return Promise.resolve().then(function () { return ask(claimBody(s, store, judge)); }).then(function (a) {
        out.push({ site: s, answer: a, error: null });
      }, function (e) {
        if (e && e.kind === 'offline') throw e;
        out.push({ site: s, answer: null, error: (e && e.message) || String(e) });
      }).then(next);
    }
    return next();
  }

  /** The verdict's class and name: "Not found in this paper" is the model's; without one, "No matching passage". */
  function claimVerdict(r) {
    var v = r && r.verdict;
    if (v === 'notFound' && !r.byModel) return { cls: 'none', name: 'No matching passage', key: 'noMatch' };
    var names = { supported: 'Supported', partly: 'Partly supported', contradicted: 'Contradicted', notFound: 'Not found in this paper',
      possible: 'Possible support', noText: 'No text to check' };
    var cls = { supported: 'ok', partly: 'part', contradicted: 'bad', notFound: 'none', possible: 'maybe', noText: 'none' };
    return { cls: cls[v] || 'none', name: r.verdictLabel || names[v] || 'Checked', key: v || 'noText' };
  }

  /** How the citations came out: {citations, checked, papers, byVerdict: {key: n}, judged, right, dismissed, failed}. */
  function claimCounts(items, decisions) {
    decisions = decisions || {};
    var c = { citations: items.length, checked: 0, papers: 0, byVerdict: {}, judged: false, right: 0, dismissed: 0, failed: 0 };
    items.forEach(function (it) {
      if (!it.answer) { c.failed++; return; }
      c.checked++;
      if (it.answer.judged) c.judged = true;
      var d = decisions[claimKey(it.site.ids, it.answer.claim)];
      if (d === 'right') c.right++;
      if (d === 'dismissed') c.dismissed++;
      (it.answer.results || []).forEach(function (r) {
        c.papers++;
        var k = claimVerdict(r).key;
        c.byVerdict[k] = (c.byVerdict[k] || 0) + 1;
      });
    });
    return c;
  }

  /** "12 citations checked (14 papers): 7 supported, 2 partly supported, … · 3 marked Looks right". */
  function claimSummary(c) {
    var s = c.checked + ' citation' + (c.checked === 1 ? '' : 's') + ' checked';
    if (c.papers !== c.checked) s += ' (' + c.papers + ' paper' + (c.papers === 1 ? '' : 's') + ')';
    var parts = [];
    ['supported', 'partly', 'contradicted', 'notFound', 'noMatch', 'possible', 'noText'].forEach(function (k) {
      if (c.byVerdict[k]) parts.push(c.byVerdict[k] + ' ' + CLAIM_NAMES[k]);
    });
    if (parts.length) s += ': ' + parts.join(', ');
    var tail = [];
    if (c.right) tail.push(c.right + ' marked Looks right');
    if (c.dismissed) tail.push(c.dismissed + ' dismissed');
    if (c.failed) tail.push(c.failed + ' couldn’t be checked');
    return s + (tail.length ? ' · ' + tail.join(', ') : '');
  }

  /** The lunta:// link's paper and words: {id, find}. */
  function parseOpenLink(link) {
    var m = /^lunta:\/\/open\?(.*)$/.exec(String(link || ''));
    if (!m) return null;
    var out = { id: '', find: '' };
    m[1].split('&').forEach(function (kv) {
      var i = kv.indexOf('='), k = i < 0 ? kv : kv.slice(0, i), v = i < 0 ? '' : kv.slice(i + 1);
      try { v = decodeURIComponent(v.replace(/\+/g, ' ')); } catch (e) {}
      if (k === 'id') out.id = v; else if (k === 'find') out.find = v;
    });
    return out.id ? out : null;
  }

  /** Most in need of a look first: contradicted, not found, partly, possible; supported last. */
  function claimRank(it) {
    if (!it.answer) return 1;
    var rs = it.answer.results || [];
    if (!rs.length) return 4;
    return Math.min.apply(null, rs.map(function (r) { var i = CLAIM_VERDICTS.indexOf(r.verdict); return i < 0 ? 4 : i; }));
  }

  /**
   * The results in `box`: the summary, what to know (privacy, model or not), and per citation the
   * sentence, each paper's verdict badge, the model's reason, the quotes and passages (click: the
   * paper at that passage in Lunta, opts.open(link)), "Looks right" / "Dismiss". opts: {open(link),
   * onlyOpen, sort ('rank' | 'document'), changed()}. Returns {rows}.
   */
  function claimsReview(doc, box, items, opts) {
    opts = opts || {};
    var decisions = claimDecisions();
    var el = function (tag, cls, text) { var e = doc.createElement(tag); if (cls) e.className = cls; if (text != null) e.textContent = text; return e; };
    box.innerHTML = '';
    var counts = claimCounts(items, decisions);
    box.appendChild(el('p', 'cl-sum', claimSummary(counts) + '.'));
    box.appendChild(el('p', 'cl-note', CLAIM_PRINCIPLE));
    if (counts.checked && !counts.judged) {
      box.appendChild(el('p', 'cl-note', 'No on-device model in Lunta here, so the passages that share the most words with each sentence are shown, with their word overlap, for you to judge.'));
    }
    box.appendChild(el('p', 'cl-note', CLAIM_PRIVACY));
    var list = items.map(function (it, i) { return { it: it, i: i }; });
    if (opts.sort !== 'document') list.sort(function (a, b) { return claimRank(a.it) - claimRank(b.it) || a.i - b.i; });
    var rows = [];
    list.forEach(function (x) {
      var it = x.it, a = it.answer;
      var key = a ? claimKey(it.site.ids, a.claim) : '';
      var state = key ? decisions[key] : null;
      if (opts.onlyOpen && state) return;
      var row = el('div', 'cl-item' + (state ? ' cl-' + state : ''));
      row.appendChild(el('div', 'cl-cite', it.site.cite));
      row.appendChild(el('div', 'cl-s', a ? (a.claim || a.sentence) : it.site.text.slice(Math.max(0, it.site.start - 160), it.site.end)));
      if (!a) {
        row.appendChild(el('div', 'cl-err', it.error || 'Couldn’t be checked.'));
      } else if (!(a.results || []).length) {
        row.appendChild(el('div', 'cl-rnote', a.note || 'Nothing to check.'));
      } else if (state !== 'dismissed') {
        (a.results || []).forEach(function (r) {
          var v = claimVerdict(r), res = el('div', 'cl-r');
          var head = el('div', 'cl-rh');
          head.appendChild(el('span', 'cl-badge cl-' + v.cls, v.name));
          head.appendChild(el('span', 'cl-paper', (r.label || 'Paper') + (r.title ? ' · ' + r.title : '')));
          res.appendChild(head);
          if (r.byModel) res.appendChild(el('div', 'cl-by', 'Suggested by the on-device model' + (r.reason ? ': ' + r.reason : '.')));
          (r.evidence || []).forEach(function (e) {
            var q = el('a', 'cl-q', '“' + e.quote + '”' + (e.section ? ' — ' + e.section : ''));
            q.href = e.link; q.title = 'Open the paper at this passage in Lunta';
            q.onclick = function (ev) { if (ev && ev.preventDefault) ev.preventDefault(); if (opts.open) opts.open(e.link); };
            res.appendChild(q);
          });
          if (r.note) res.appendChild(el('div', 'cl-rnote', r.note));
          var ps = r.passages || [];
          if (ps.length) {
            var det = el('details', 'cl-ps');
            if (!r.byModel) det.open = true;
            det.appendChild(el('summary', null, ps.length === 1 ? 'The best-matching passage' : 'The ' + ps.length + ' best-matching passages'));
            ps.forEach(function (p) {
              var pe = el('a', 'cl-p');
              pe.href = p.link; pe.title = 'Open the paper at this passage in Lunta';
              pe.appendChild(el('span', 'cl-pm', (p.section || 'Passage') + ' · ' + Math.round((p.score || 0) * 100) + ' % word overlap'));
              pe.appendChild(el('span', 'cl-pt', p.text));
              pe.onclick = function (ev) { if (ev && ev.preventDefault) ev.preventDefault(); if (opts.open) opts.open(p.link); };
              det.appendChild(pe);
            });
            res.appendChild(det);
          }
          row.appendChild(res);
        });
      }
      if (a) {
        var b = el('div', 'cl-btns');
        if (state) {
          b.appendChild(el('span', 'cl-state', state === 'right' ? '✓ Looks right' : 'Dismissed'));
          var undo = el('button', 'btn cl-undo', 'Undo');
          undo.onclick = function () { rememberClaim(key, null); claimsReview(doc, box, items, opts); if (opts.changed) opts.changed(); };
          b.appendChild(undo);
        } else {
          var ok = el('button', 'btn cl-ok', 'Looks right'), no = el('button', 'btn cl-no', 'Dismiss');
          ok.onclick = function () { rememberClaim(key, 'right'); claimsReview(doc, box, items, opts); if (opts.changed) opts.changed(); };
          no.onclick = function () { rememberClaim(key, 'dismissed'); claimsReview(doc, box, items, opts); if (opts.changed) opts.changed(); };
          b.appendChild(ok); b.appendChild(no);
        }
        row.appendChild(b);
      }
      box.appendChild(row);
      rows.push({ el: row, item: it, key: key, state: state });
    });
    return { rows: rows, counts: counts };
  }

  // ---- talking to Lunta ----

  var OFFLINE = {
    word: 'Can’t reach Lunta. Open Lunta on this computer (Mac, Windows or Linux). If it is open: in Lunta, open Citations in Word & Google Docs… and trust the certificate (needed once, and again if Lunta made a new certificate). Don’t have Lunta yet? It’s free: getlunta.app',
    gdocs: 'Can’t reach Lunta. Open Lunta on this computer (Mac, Windows or Linux). If it is open: in Lunta, open Citations in Word & Google Docs… and trust the certificate, and allow this page to reach this device if Chrome asks (the lock icon › Site settings › Local network access).'
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

  var customStorage = null;
  function storage() { if (customStorage) return customStorage; try { return window.localStorage; } catch (e) { return null; } }
  /** Where journal styles used before are remembered, {getItem, setItem} (the browser extension's own storage); default the page's localStorage. */
  function useStorage(s) { customStorage = s || null; }

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
    detailsFor: detailsFor, format: format,
    LOCATORS: LOCATORS, cleanOption: cleanOption, cleanOptions: cleanOptions, encodeOptions: encodeOptions, decodeOptions: decodeOptions,
    optionsTitle: optionsTitle, optionsOfTitle: optionsOfTitle, optionRangeNames: optionRangeNames, optionRangePart: optionRangePart,
    optionsCodeOf: optionsCodeOf, locatorText: locatorText, optionsLabel: optionsLabel, renderOptions: renderOptions, optionsForm: optionsForm,
    jsonAt: jsonAt, stripTags: stripTags, cslMeta: cslMeta, cslOptions: cslOptions, cslCitation: cslCitation, cslDate: cslDate,
    zoteroDocsCode: zoteroDocsCode, zoteroDocsJoin: zoteroDocsJoin, ZOTERO_DOCS_URL: ZOTERO_DOCS_URL, workKey: workKey,
    convertPlan: convertPlan, convertSummary: convertSummary, convertResolve: convertResolve, convertReview: convertReview, convertDone: convertDone, formatJournal: formatJournal, unknownMessage: unknownMessage, summary: summary,
    request: request, describe: describe,
    isJournal: isJournal, isStyle: isStyle, styleName: styleName, fillStyles: fillStyles, showJournals: showJournals,
    rememberJournal: rememberJournal, hitMeta: hitMeta, pickedLabel: pickedLabel, useStorage: useStorage,
    claimSites: claimSites, claimBody: claimBody, claimKey: claimKey, claimDecisions: claimDecisions, rememberClaim: rememberClaim, runClaims: runClaims,
    claimVerdict: claimVerdict, claimCounts: claimCounts, claimSummary: claimSummary, claimsReview: claimsReview, parseOpenLink: parseOpenLink,
    CLAIM_PRIVACY: CLAIM_PRIVACY
  };
})();
if (typeof module !== 'undefined') module.exports = LuntaCite;
