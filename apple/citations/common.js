// Citation formatting shared by the Word add-in and the Google Docs sidebar.
// A citation is a list of paper ids; papers are numbered by first appearance for numeric styles.
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

  function compress(nums) {
    nums = nums.slice().sort(function (a, b) { return a - b; });
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

  /** The names part of an author–year citation: "Smith", "Smith & Lee", "Smith et al." */
  function names(p, st) {
    var f = (p && p.families) || [];
    if (!f.length) return (p && p.title ? p.title.split(' ').slice(0, 3).join(' ') : 'Unknown');
    if (f.length === 1) return f[0];
    if (f.length >= (st.etAl || 3)) return f[0] + ' et al.';
    if (f.length === 2) return f[0] + ' ' + (st.and || '&') + ' ' + f[1];
    return f.slice(0, -1).join(', ') + (st.serial ? ', ' : ' ') + (st.and || '&') + ' ' + f[f.length - 1];   // three (Harvard, Chicago)
  }

  /** In-text text for one citation. `order` maps id -> number (numeric styles), `papers` id -> info. */
  function inText(style, ids, order, papers) {
    var st = STYLES[style] || STYLES.apa;
    if (st.numeric) {
      var nums = ids.map(function (id) { return order[id]; }).filter(function (n) { return n; });
      return (st.open || '') + compress(nums) + (st.close || '');
    }
    // several works in one citation: alphabetical, as in the reference list
    var parts = ids.map(function (id) {
      var p = papers[id] || {};
      return names(p, st) + (st.noYear ? '' : st.sep + (p.year || 'n.d.'));
    }).sort(function (a, b) { return a.localeCompare(b); });
    return '(' + parts.join('; ') + ')';
  }

  /** Numbers papers by first appearance in a list of citations (each a list of ids). */
  function numbering(citations) {
    var order = {}, list = [], n = 0;
    citations.forEach(function (ids) {
      ids.forEach(function (id) { if (!order[id]) { order[id] = ++n; list.push(id); } });
    });
    return { order: order, list: list };
  }

  /** Bibliography entries, in the order the style wants, as {text (markdown italics), id}. */
  function bibliography(style, entries, order) {
    var e = entries.filter(function (x) { return !x.missing; }).slice();
    if (STYLES[style] && STYLES[style].numeric) {
      e.sort(function (a, b) { return order[a.id] - order[b.id]; });
      var prefix = STYLES[style].prefix || function (n) { return n + '. '; };
      return e.map(function (x) { return { id: x.id, text: prefix(order[x.id]) + x.entry }; });
    }
    e.sort(function (a, b) { return a.entry.localeCompare(b.entry); });
    return e.map(function (x) { return { id: x.id, text: x.entry }; });
  }

  function escapeHTML(s) {
    return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
  }

  /** *italic* and **bold** to HTML. */
  function toHTML(md) {
    return escapeHTML(md).replace(/\*\*(.+?)\*\*/g, '<b>$1</b>').replace(/\*(.+?)\*/g, '<i>$1</i>');
  }

  /** Splits *italic* markup into runs [{text, italic, bold}]. */
  function runs(md) {
    var out = [], re = /(\*\*(.+?)\*\*|\*(.+?)\*)/g, last = 0, m;
    while ((m = re.exec(md))) {
      if (m.index > last) out.push({ text: md.slice(last, m.index) });
      if (m[2] !== undefined) out.push({ text: m[2], bold: true });
      else out.push({ text: m[3], italic: true });
      last = re.lastIndex;
    }
    if (last < md.length) out.push({ text: md.slice(last) });
    return out;
  }

  return { STYLES: STYLES, inText: inText, numbering: numbering, bibliography: bibliography, toHTML: toHTML, runs: runs, compress: compress };
})();
if (typeof module !== 'undefined') module.exports = LuntaCite;
