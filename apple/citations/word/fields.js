// Citations made in Word with Zotero, EndNote or Mendeley, read from the document so Lunta can
// convert them (word/app.js: Convert Citations from Zotero, EndNote or Mendeley…). Plain functions,
// tested in node (apple/citations-tests/word-convert.test.js).
//
// What each manager leaves in a .docx (word/document.xml):
//   Zotero      a field " ADDIN ZOTERO_ITEM CSL_CITATION {CSL citation JSON}" (the JSON may follow
//               CSL_CITATION without a space, and older versions add " RND…" after it); the
//               bibliography " ADDIN ZOTERO_BIBL {…} CSL_BIBLIOGRAPHY". With "Store citations as
//               bookmarks" there are no fields (bookmarks "ZOTERO_BREF_…"): not converted, the
//               panel says how to switch Zotero back to fields.
//   Mendeley    Desktop (until 2022): " ADDIN CSL_CITATION {…}" (older: " ADDIN Mendeley Citation{uuid}
//               CSL_CITATION {…}"), bibliography " ADDIN Mendeley Bibliography CSL_BIBLIOGRAPHY".
//               Mendeley Cite: content controls tagged "MENDELEY_CITATION_v3_<base64 of the JSON>",
//               the bibliography's tagged "MENDELEY_BIBLIOGRAPHY".
//   EndNote     " ADDIN EN.CITE <EndNote><Cite>…<record>…</record></Cite></EndNote>"; a long one keeps
//               the XML base64 in the field's <w:fldData> and nests a field " ADDIN EN.CITE.DATA "
//               (with the same data) inside; the bibliography " ADDIN EN.REFLIST ".
//   ReadCube Papers (SmartCite)  content controls titled (<w:alias>) "SmartCite Citation", tagged
//               "<documentId>:<citationId>,<documentId>:<citationId>…+{JSON}" (one id per work,
//               in the citation's order; the JSON maps each id to its {doi, pmid, pmcid} when it has
//               them; no title, authors or year); the shown text is the formatted citation ("1–4",
//               "(Ke et al., 2013)"). The bibliography is one control titled "SmartCite
//               Bibliography" (tagged "<style name>+{…}") holding the formatted entries. The rest
//               of a work's details come from its bibliography entry (parseBibliography), found by
//               DOI, else by the citation's number or author and year.
// A field is <w:fldChar w:fldCharType="begin"/> (holding <w:fldData>), the instruction in one or
// more <w:instrText> (split wherever the formatting changes, across runs and paragraphs), then
// "separate", the shown text, and "end"; or <w:fldSimple w:instr="…">. Word's own Field objects
// (body.fields) come in the same order as the fields begin, nested ones included, which is how a
// field read here is found again to replace it.
var LuntaFields = (function () {
  function unescapeXML(s) {
    return String(s).replace(/&(#x[0-9a-f]+|#\d+|lt|gt|amp|quot|apos);/gi, function (m, e) {
      var l = e.toLowerCase();
      if (l === 'lt') return '<';
      if (l === 'gt') return '>';
      if (l === 'amp') return '&';
      if (l === 'quot') return '"';
      if (l === 'apos') return "'";
      var n = l.charAt(1) === 'x' ? parseInt(l.slice(2), 16) : parseInt(l.slice(1), 10);
      return n > 0xffff ? String.fromCharCode(0xd800 + ((n - 0x10000) >> 10), 0xdc00 + ((n - 0x10000) & 1023)) : String.fromCharCode(n);
    });
  }

  function attr(tag, name) {
    var m = new RegExp('\\s' + name.replace(':', '\\:') + '="([^"]*)"').exec(tag);
    return m ? unescapeXML(m[1]) : null;
  }

  /**
   * The fields in the OOXML of the document's body, in the order they begin (nested ones too):
   * [{instr, data (base64 of <w:fldData>, ""), text (shown), depth}], its content controls'
   * tags in order, and its content controls [{title (<w:alias>), tag, text (paragraphs on lines)}]
   * in the order they begin: {fields, tags, controls}.
   */
  function scan(xml) {
    xml = String(xml || '');
    // only the main document's body (getOoxml wraps it in a package with styles, numbering…)
    var b = xml.search(/<w:body[\s>]/);
    if (b >= 0) {
      var e = xml.indexOf('</w:body>', b);
      xml = xml.slice(b, e < 0 ? xml.length : e);
    }
    var fields = [], stack = [], tags = [], controls = [], open = [];
    var re = /<w:fldSimple\b([^>]*?)(\/?)>|<\/w:fldSimple>|<w:fldChar\b([^>]*?)(\/?)>|<w:fldData\b[^>]*>([\s\S]*?)<\/w:fldData>|<w:instrText\b[^>]*>([\s\S]*?)<\/w:instrText>|<w:instrText\b[^>]*\/>|<w:t\b[^>]*>([\s\S]*?)<\/w:t>|<w:tab\/>|<w:br\/>|<\/w:p>|<w:tag\b([^>]*)\/>|<w:alias\b([^>]*)\/>|<w:sdt\b[^>]*>|<\/w:sdt>/g;
    var m, simple = [];
    while ((m = re.exec(xml))) {
      var top = stack[stack.length - 1];
      if (m[0].indexOf('<w:fldSimple') === 0) {
        var f = { instr: attr(m[1], 'w:instr') || '', data: '', text: '', depth: stack.length + simple.length, simple: true };
        fields.push(f);
        if (!m[2]) simple.push(f);
      } else if (m[0] === '</w:fldSimple>') {
        simple.pop();
      } else if (m[0].indexOf('<w:fldChar') === 0) {
        var type = attr(m[3], 'w:fldCharType');
        if (type === 'begin') {
          var fc = { instr: '', data: '', text: '', depth: stack.length + simple.length, state: 'instr' };
          fields.push(fc);
          stack.push(fc);
        } else if (type === 'separate') {
          if (top) top.state = 'result';
        } else if (type === 'end') {
          stack.pop();
        }
      } else if (m[5] !== undefined) {
        // <w:fldData> sits inside the begin <w:fldChar>, which is already on the stack
        if (top && top.state === 'instr') top.data += m[5].replace(/\s+/g, '');
      } else if (m[6] !== undefined) {
        if (top && top.state === 'instr') top.instr += unescapeXML(m[6]);
      } else if (m[7] !== undefined || m[0] === '<w:tab/>' || m[0] === '<w:br/>' || m[0] === '</w:p>') {
        var t = m[7] !== undefined ? unescapeXML(m[7]) : m[0] === '<w:tab/>' ? '\t' : '\n';
        stack.forEach(function (x) { if (x.state === 'result') x.text += t; });
        simple.forEach(function (x) { x.text += t; });
        open.forEach(function (x) { x.text += t; });
      } else if (m[8] !== undefined) {
        tags.push(attr(m[8], 'w:val') || '');
        if (open.length) open[open.length - 1].tag = attr(m[8], 'w:val') || '';
      } else if (m[9] !== undefined) {
        if (open.length) open[open.length - 1].title = attr(m[9], 'w:val') || '';
      } else if (m[0] === '</w:sdt>') {
        open.pop();
      } else if (m[0].indexOf('<w:sdt') === 0) {
        var sdt = { title: '', tag: '', text: '' };
        controls.push(sdt);
        if (m[0].slice(-2) !== '/>') open.push(sdt);
      }
    }
    fields.forEach(function (f) { f.text = f.text.replace(/\s+$/, ''); delete f.state; });
    controls.forEach(function (c) { c.text = c.text.replace(/\s+$/, ''); });
    return { fields: fields, tags: tags, controls: controls };
  }

  // ---- base64 (fldData, Mendeley Cite's tags) ----
  var B64 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
  function bytesOf(b64) {
    b64 = String(b64 || '').replace(/[^A-Za-z0-9+/_-]/g, '').replace(/-/g, '+').replace(/_/g, '/');
    var out = [], acc = 0, bits = 0;
    for (var i = 0; i < b64.length; i++) {
      acc = (acc << 6) | B64.indexOf(b64.charAt(i));
      bits += 6;
      if (bits >= 8) { bits -= 8; out.push((acc >> bits) & 255); }
    }
    return out;
  }
  /** Text of bytes: UTF-16 (a byte-order mark, or every other byte zero) or UTF-8. */
  function textOf(bytes) {
    var le = bytes[0] === 0xff && bytes[1] === 0xfe, be = bytes[0] === 0xfe && bytes[1] === 0xff;
    var zeros = 0;
    for (var k = 1; k < Math.min(bytes.length, 64); k += 2) if (bytes[k] === 0) zeros++;
    var s = '', i;
    if (le || be || (bytes.length > 8 && zeros > 8)) {
      for (i = le || be ? 2 : 0; i + 1 < bytes.length; i += 2) s += String.fromCharCode(be ? (bytes[i] << 8) | bytes[i + 1] : bytes[i] | (bytes[i + 1] << 8));
      return s;
    }
    for (i = 0; i < bytes.length;) {
      var c = bytes[i++];
      if (c >= 0xf0) c = ((c & 7) << 18) | ((bytes[i++] & 63) << 12) | ((bytes[i++] & 63) << 6) | (bytes[i++] & 63);
      else if (c >= 0xe0) c = ((c & 15) << 12) | ((bytes[i++] & 63) << 6) | (bytes[i++] & 63);
      else if (c >= 0xc0) c = ((c & 31) << 6) | (bytes[i++] & 63);
      if (c >= 0x10000) { c -= 0x10000; s += String.fromCharCode(0xd800 + (c >> 10), 0xdc00 + (c & 1023)); } else s += String.fromCharCode(c);
    }
    return s.replace(/^﻿/, '');
  }
  function decodeBase64Text(b64) { return textOf(bytesOf(b64)); }

  // ---- EndNote's XML ----

  /** The text of the first <name>…</name> in `xml` (styles inside it dropped), or "". */
  function tagText(xml, name) {
    var m = new RegExp('<' + name + '(?:\\s[^>]*)?>([\\s\\S]*?)</' + name + '>').exec(xml);
    return m ? clean(m[1]) : '';
  }
  function tagsText(xml, name) {
    var out = [], re = new RegExp('<' + name + '(?:\\s[^>]*)?>([\\s\\S]*?)</' + name + '>', 'g'), m;
    while ((m = re.exec(xml))) out.push(clean(m[1]));
    return out.filter(Boolean);
  }
  // EndNote wraps text in <style face="normal" font="default" size="100%">…</style>; "`" keeps a
  // space at the edge of a prefix or suffix
  function clean(s) { return unescapeXML(String(s).replace(/<[^>]+>/g, '')).replace(/`/g, ' ').replace(/\s+/g, ' ').trim(); }
  function cleanAffix(s) { return unescapeXML(String(s).replace(/<[^>]+>/g, '')).replace(/`/g, ' ').replace(/\s+/g, ' '); }

  /** "Smith, John" → "John Smith" (Lunta keeps names as written; the core reads both). */
  function enName(a) {
    var i = a.indexOf(',');
    if (i < 0) return a;
    var family = a.slice(0, i).trim(), given = a.slice(i + 1).trim();
    return given ? given + ' ' + family : family;
  }

  /** A work's details from an EndNote <record>. */
  function endnoteMeta(rec) {
    var titles = /<titles>([\s\S]*?)<\/titles>/.exec(rec), t = titles ? titles[1] : '';
    var authors = /<contributors>[\s\S]*?<authors>([\s\S]*?)<\/authors>/.exec(rec);
    var doiRaw = tagText(rec, 'electronic-resource-num');
    var doi = (/10\.\d{4,9}\/\S+/.exec(doiRaw) || [''])[0].replace(/[.,;]$/, '');
    var date = tagText(/<dates>([\s\S]*?)<\/dates>/.exec(rec) ? /<dates>([\s\S]*?)<\/dates>/.exec(rec)[1] : '', 'year');
    var m = {
      title: tagText(t, 'title'), authors: authors ? tagsText(authors[1], 'author').map(enName) : [],
      date: (/\d{4}/.exec(date) || [''])[0],
      journal: tagText(t, 'secondary-title') || tagText(/<periodical>([\s\S]*?)<\/periodical>/.exec(rec) ? /<periodical>([\s\S]*?)<\/periodical>/.exec(rec)[1] : '', 'full-title'),
      volume: tagText(rec, 'volume'), issue: tagText(rec, 'number'), pages: tagText(rec, 'pages'), publisher: tagText(rec, 'publisher'), doi: doi
    };
    var acc = tagText(rec, 'accession-num');
    if (/^\d{5,9}$/.test(acc)) m.pmid = acc;
    var url = tagsText(rec, 'url')[0];
    if (url) m.url = url;
    if (!m.doi && url) { var d = /doi\.org\/(10\.\d{4,9}\/\S+)/i.exec(url); if (d) m.doi = d[1]; }
    Object.keys(m).forEach(function (k) { if (!m[k] || (Array.isArray(m[k]) && !m[k].length)) delete m[k]; });
    return m;
  }

  /**
   * The works of an EndNote citation's XML: [{meta, options}]. <Cite> may carry <Prefix>,
   * <Suffix> and <Pages> (the cited pages, outside the <record>), ExcludeAuth="1" (without the
   * author), ExcludeYear="1" and AuthorYear="1" (the author in the text, on <Cite> or <EndNote>).
   */
  function endnoteCitation(xml) {
    xml = String(xml || '');
    var root = /<EndNote\b([^>]*)>/.exec(xml);
    if (!root) return null;
    var authorYear = /AuthorYear="1"/.test(root[1]);
    var out = [], re = /<Cite\b([^>]*)>([\s\S]*?)<\/Cite>/g, m;
    while ((m = re.exec(xml))) {
      var a = m[1], body = m[2], rec = /<record>([\s\S]*?)<\/record>/.exec(body);
      var head = rec ? body.slice(0, rec.index) : body;
      var o = {}, pre = /<Prefix>([\s\S]*?)<\/Prefix>/.exec(head), suf = /<Suffix>([\s\S]*?)<\/Suffix>/.exec(head), pg = /<Pages>([\s\S]*?)<\/Pages>/.exec(head);
      if (pre) o.prefix = cleanAffix(pre[1]).trim();
      if (suf) o.suffix = cleanAffix(suf[1]).trim();
      if (pg) o.locator = clean(pg[1]);
      if (/ExcludeAuth="1"/.test(a)) o.mode = 'suppress';
      if (authorYear || /AuthorYear="1"/.test(a)) o.mode = 'narrative';
      var meta = rec ? endnoteMeta(rec[1]) : {};
      // without a <record> (rare): the author and year EndNote shows
      if (!rec) {
        var au = tagText(head, 'Author'), yr = tagText(head, 'Year');
        if (au) meta.authors = [au];
        if (yr) meta.date = yr;
      }
      out.push({ meta: meta, options: LuntaCite.cleanOption(o) });
    }
    return out;
  }

  // ---- what a field is ----

  /**
   * What a field's instruction (and data) is: {kind: "cite", source, works} | {kind: "bib", source}
   * | {kind: "data"} (EndNote's inner EN.CITE.DATA, part of the field around it) | null.
   */
  function classify(instr, data) {
    var s = String(instr || '').replace(/^\s+/, '');
    var m;
    if (!/^ADDIN\b/.test(s)) return null;
    s = s.replace(/^ADDIN\s*/, '');
    if (/^ZOTERO_BIBL\b/.test(s)) return { kind: 'bib', source: 'Zotero' };
    if (/^Mendeley Bibliography\b/.test(s) || /^CSL_BIBLIOGRAPHY\b/.test(s)) return { kind: 'bib', source: 'Mendeley' };
    if (/^EN\.REFLIST\b/.test(s)) return { kind: 'bib', source: 'EndNote' };
    if (/^EN\.CITE\.DATA\b/.test(s)) return { kind: 'data' };
    if ((m = /^(ZOTERO_ITEM\s*)?(Mendeley Citation\{[^}]*\}\s*)?CSL_CITATION/.exec(s))) {
      var works = LuntaCite.cslCitation(LuntaCite.jsonAt(s, m[0].length));
      var json = LuntaCite.jsonAt(s, m[0].length) || {};
      var source = m[1] ? 'Zotero' : (json.mendeley || m[2] || /mendeley/i.test(JSON.stringify((json.citationItems || []).map(function (i) { return i.uris || i.uri || ''; })))) ? 'Mendeley' : 'Zotero';
      return works ? { kind: 'cite', source: source, works: works } : { kind: 'cite', source: source, works: null };
    }
    if (/^EN\.CITE\b/.test(s)) {
      var xml = s.slice('EN.CITE'.length);
      if (xml.indexOf('<EndNote') < 0 && data) xml = decodeBase64Text(data);
      return { kind: 'cite', source: 'EndNote', works: endnoteCitation(xml) };
    }
    return null;
  }

  /** A Mendeley Cite content control's tag: {kind, source, works} or null. */
  function classifyTag(tag) {
    tag = String(tag || '');
    if (tag === 'MENDELEY_BIBLIOGRAPHY') return { kind: 'bib', source: 'Mendeley' };
    var m = /^MENDELEY_CITATION_v\d+_(.*)$/.exec(tag);
    if (!m) return null;
    var json = null;
    try { json = JSON.parse(decodeBase64Text(m[1])); } catch (e) { json = null; }
    return { kind: 'cite', source: 'Mendeley', works: json ? LuntaCite.cslCitation(json) : null };
  }

  /** Whether the document has Zotero citations stored as bookmarks (no fields to read). */
  function zoteroBookmarks(xml) { return /<w:bookmarkStart\b[^>]*w:name="ZOTERO_BREF_/.test(String(xml || '')); }

  // ---- ReadCube Papers (SmartCite): formatted citations and a formatted bibliography ----

  var SMARTCITE = 'ReadCube Papers (SmartCite)';
  var SC_ID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}:[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

  /** A DOI as written in a reference ("https://doi.org/10.…", "doi:10.…"), without what follows it. */
  function cleanDOI(s) {
    var d = String(s || '').trim().replace(/^https?:\/\/(dx\.)?doi\.org\//i, '').replace(/^doi:\s*/i, '');
    if (!/^10\.\d{4,9}\//.test(d)) return '';
    // a full stop or comma after it, or a bracket it doesn't open
    for (;;) {
      var last = d.charAt(d.length - 1);
      if (/[.,;:'"”’>\]]/.test(last)) { d = d.slice(0, -1); continue; }
      if (last === ')' && (d.split('(').length - 1) < (d.split(')').length - 1)) { d = d.slice(0, -1); continue; }
      break;
    }
    return d;
  }

  /** A SmartCite citation's tag: [{id, doi, pmid, pmcid}] in the citation's order, or null. */
  function smartCiteTag(tag) {
    tag = String(tag || '');
    var plus = tag.indexOf('+');
    var ids = (plus < 0 ? tag : tag.slice(0, plus)).split(',').map(function (s) { return s.trim(); });
    if (!ids.length || !ids.every(function (id) { return SC_ID.test(id); })) return null;
    var json = {};
    if (plus >= 0) {
      var b = tag.indexOf('{', plus);
      try { json = b >= 0 ? JSON.parse(tag.slice(b)) || {} : {}; } catch (e) { json = {}; }
    }
    return ids.map(function (id) {
      var d = json[id] || {}, w = { id: id };
      var doi = cleanDOI(d.doi || d.DOI || ''), pmid = String(d.pmid || '').trim(), pmcid = String(d.pmcid || '').trim();
      if (doi) w.doi = doi;
      if (/^\d{1,9}$/.test(pmid)) w.pmid = pmid;
      if (/^PMC\d+$/i.test(pmcid)) w.pmcid = pmcid.toUpperCase();
      return w;
    });
  }

  /**
   * What a content control (its title, Word's <w:alias>, and tag) is: {kind: "cite", source,
   * items} (SmartCite's works, maybe without details) | {kind: "bib", source, style} | null.
   */
  function classifyControl(title, tag) {
    title = String(title || '').trim();
    if (title === 'SmartCite Bibliography') {
      var t = String(tag || ''), plus = t.indexOf('+');
      return { kind: 'bib', source: SMARTCITE, style: (plus > 0 ? t.slice(0, plus) : '').trim() };
    }
    var items = smartCiteTag(tag);
    if (title === 'SmartCite Citation' || items) return { kind: 'cite', source: SMARTCITE, items: items || [] };
    return null;
  }

  /** The numbers of a numbered citation ("1–4", "6,7", "[2, 5-7]", "(3)") in order, or null. */
  function citeNumbers(text) {
    var s = String(text || '').replace(/[\s\u200b ]+/g, ' ').trim().replace(/^[[(]\s*/, '').replace(/\s*[\])]$/, '');
    if (!/^\d+(\s*[-–—‒]\s*\d+)?(\s*[,;]\s*\d+(\s*[-–—‒]\s*\d+)?)*$/.test(s)) return null;
    var out = [];
    s.split(/\s*[,;]\s*/).forEach(function (part) {
      var r = part.split(/\s*[-–—‒]\s*/).map(Number);
      if (r.length === 2 && r[1] >= r[0] && r[1] - r[0] < 500) for (var n = r[0]; n <= r[1]; n++) out.push(n);
      else out.push(r[0]);
    });
    return out;
  }

  var LETTER = 'A-Za-zÀ-ÖØ-öø-ɏ';
  function fold(s) {
    var from = 'àáâãäåæçèéêëìíîïñòóôõöøùúûüýÿšžčćłńśźżőűåø', to = 'aaaaaaaceeeeiiiinoooooouuuuyyszcclnszzouao';
    return String(s || '').toLowerCase().replace(/./g, function (c) { var i = from.indexOf(c); return i >= 0 ? to.charAt(i) : c; }).replace(/[^a-z]/g, '');
  }

  /**
   * The works of an author–year citation ("(Ke et al., 2013; Chen and Zhuang, 2015a)", "Ke et al.
   * (2013)", "(see Smith 2019, 2020)"): [{family, year}] (year with its letter), or null.
   */
  function citeAuthorYears(text) {
    var s = String(text || '').replace(/[\s\u200b ]+/g, ' ').trim();
    // "Ke et al. (2013)": the author in the text
    var narrative = new RegExp('^([' + LETTER + '][^()]*?)\\s*\\((\\d{4}[a-z]?)\\)$').exec(s);
    if (narrative) s = narrative[1] + ', ' + narrative[2];
    s = s.replace(/^[([]\s*/, '').replace(/\s*[)\]]$/, '');
    if (!s) return null;
    var out = [];
    var ok = s.split(/\s*;\s*/).every(function (part) {
      part = part.replace(/^(see also|see|e\.g\.,?|cf\.|i\.e\.,?|reviewed in)\s+/i, '');
      var years = part.match(/\b(1[6-9]\d\d|20\d\d)[a-z]?\b|\bn\.d\./g);
      if (!years) return false;
      var name = part.slice(0, part.search(/,?\s*(\b(1[6-9]\d\d|20\d\d)|\bn\.d\.)/)).trim();
      name = name.replace(/\s+(et al\.?|and others)$/i, '').split(/\s+(?:and|&)\s+|,\s*/)[0].trim();
      if (!new RegExp('[' + LETTER + ']').test(name)) return false;
      years.forEach(function (y) { out.push({ family: name, year: y }); });
      return true;
    });
    return ok && out.length ? out : null;
  }

  // ---- a formatted reference (Cell, Nature, APA, Vancouver, and the like) ----

  var INITIAL_BEFORE = /(^|[\s.,\-‐])[A-Z]$/;   // "…, M" right before the ". " ending the authors

  /** Where the authors end and the title starts (no year in brackets after the authors): an index or -1. */
  function authorsEnd(s) {
    var etal = /\bet al\.?(?=[\s,]|$)/.exec(s);
    if (etal && etal.index < 600) return etal.index + etal[0].length;
    var re = /\.\s+/g, m;
    while ((m = re.exec(s))) {
      var before = s.slice(0, m.index), after = s.slice(m.index + m[0].length);
      // "Chen, K. H., …": the next word is another initial
      if (/^[A-Z](\.|,|-|\s*[.,])/.test(after) || /^[A-Z]\.?-[A-Z]/.test(after)) continue;
      if (/^(&|and)\s/.test(after)) continue;
      if (INITIAL_BEFORE.test(before)) return m.index + 1;
    }
    return -1;
  }

  /** "Ke, R., Mignardi, M., and Nilsson, M." / "Ke R, Mignardi M" / "Ke, R. et al." → ["R. Ke", …]. */
  function parseAuthors(s) {
    s = String(s || '').replace(/\bet al\.?/g, '').replace(/\s+/g, ' ').trim().replace(/[.,;\s]+$/, '');
    if (!s) return [];
    var out = [], m;
    // "Family, I. I." pairs (Cell, Nature, APA)
    var pair = new RegExp("(?:^|,\\s*(?:and|&)\\s+|,\\s*|\\s*&\\s*|\\s+and\\s+)([" + LETTER + "][" + LETTER + "'’\\-]*(?:\\s+[" + LETTER + "][" + LETTER + "'’\\-]*)*),\\s*((?:[A-Z][a-z]?\\.?[\\s-]?)+)(?=,|\\s*&|\\s+and\\b|$)", 'g');
    var pairs = [];
    while ((m = pair.exec(s))) {
      // the last author's full stop went with the list's
      var ini = m[2].trim();
      if (!/\.$/.test(ini) && (/\./.test(ini) || /^[A-Z]$/.test(ini))) ini += '.';
      pairs.push(ini + ' ' + m[1].trim());
    }
    if (pairs.length) return pairs.map(function (n) { return n.replace(/\s+/g, ' '); });
    // "Family II" (Vancouver)
    s.split(/\s*,\s*|\s+(?:and|&)\s+/).forEach(function (part) {
      var v = /^(.+?)\s+([A-Z]{1,4})$/.exec(part.trim());
      if (v) out.push(v[2].split('').join('. ') + '. ' + v[1]);
      else if (part.trim()) out.push(part.trim());
    });
    return out;
  }

  /** The journal, volume, issue and pages after a reference's title ("Nat Methods 10, 857–860",
   *  "Nature Methods, 10(9), 857–860", "Nat Methods. 2013;10(9):857-60"). */
  function journalPart(s, m) {
    s = String(s || '').replace(/\(\d{4}[a-z]?\)\.?\s*$/, '').trim().replace(/[.\s]+$/, '');
    if (!s) return;
    var v = /^(.+?)\.?\s+(?:(\d{4})[^;]*;\s*)(\d+)(?:\s*\(([^)]+)\))?(?::\s*([A-Za-z]?\d+[A-Za-z]?(?:\s*[-–]\s*[A-Za-z]?\d+[A-Za-z]?)?))?/.exec(s);
    var c = v || /^(.+?)[,.]?\s+(\d+)(?:\s*\(([^)]+)\))?(?:[,:]\s*([A-Za-z]*\d+[A-Za-z]?(?:\s*[-–]\s*[A-Za-z]*\d+[A-Za-z]?)?))?(?:[.,]|$)/.exec(s);
    // an abbreviated name keeps its last full stop ("Nat. Genet. 56, …")
    var name = function (j) { j = j.trim().replace(/,$/, ''); return /\.\s?\S/.test(j) && !/\.$/.test(j) ? j + '.' : j; };
    if (v) { m.journal = name(v[1]); m.volume = v[3]; if (v[4]) m.issue = v[4]; if (v[5]) m.pages = v[5]; return; }
    if (c && !/^\d/.test(c[1])) { m.journal = name(c[1]); m.volume = c[2]; if (c[3]) m.issue = c[3]; if (c[4]) m.pages = c[4]; return; }
    // no volume ("Cell", "Nat. Methods, 1–5", "Nucleic Acids Res, gkaa792-")
    m.journal = name(s.split(/,\s/)[0]);
  }

  /** A work's details from a formatted reference (one bibliography entry, its number taken off). */
  function referenceMeta(text) {
    var s = String(text || '').replace(/[\s ]+/g, ' ').trim();
    var m = {};
    var doi = /(?:https?:\/\/(?:dx\.)?doi\.org\/|\bdoi:\s*|\bDOI:?\s+)?(10\.\d{4,9}\/[^\s"<>]+)/i.exec(s);
    if (doi) m.doi = cleanDOI(doi[1]);
    var pmid = /\bPMID:?\s*(\d{1,9})\b/i.exec(s), pmcid = /\b(PMC\d{4,9})\b/.exec(s);
    var arxiv = /\barXiv:\s*(\d{4}\.\d{4,5}(?:v\d+)?)|arxiv\.org\/abs\/(\d{4}\.\d{4,5}(?:v\d+)?)/i.exec(s);
    if (pmid) m.pmid = pmid[1];
    if (pmcid) m.pmcid = pmcid[1];
    if (arxiv) m.arxiv = arxiv[1] || arxiv[2];
    var url = /https?:\/\/\S+/.exec(s);
    if (url && !doi) m.url = url[0].replace(/[.,;]+$/, '');
    // what's before the identifiers and links
    var cut = s.search(/\s(?:https?:\/\/|doi:|DOI:?\s|PMID|PMCID|arXiv:|Available|Retrieved|\[?Epub)/i);
    var body = (cut >= 0 ? s.slice(0, cut) : s).trim();
    var year = /\((1[6-9]\d\d|20\d\d)[a-z]?\)|\((?:[A-Z][a-z]+\.? )?\d{0,2},? ?(1[6-9]\d\d|20\d\d)[a-z]?\)/.exec(body);
    var authors = '', rest = '';
    if (year && body.slice(year.index + year[0].length).replace(/^[.,:\s]+/, '').length > 15) {
      // Cell, APA, Harvard: Authors (2013). Title. Journal…
      authors = body.slice(0, year.index);
      rest = body.slice(year.index + year[0].length).replace(/^[.,:\s]+/, '');
      m.date = year[1] || year[2];
    } else {
      // Nature, Vancouver: Authors. Title. Journal… (2013) / Journal. 2013;…
      var e = authorsEnd(body);
      if (e > 0) { authors = body.slice(0, e); rest = body.slice(e).replace(/^[.,:\s]+/, ''); } else rest = body;
      var ys = body.match(/\b(1[6-9]\d\d|20\d\d)\b/g);
      var paren = /\((1[6-9]\d\d|20\d\d)\)\.?\s*$/.exec(body), van = /\.\s*(1[6-9]\d\d|20\d\d)\b[^;]*;/.exec(rest);
      if (paren) m.date = paren[1];
      else if (van) m.date = van[1];
      else if (ys) m.date = ys[ys.length - 1];
    }
    var names = parseAuthors(authors);
    if (names.length) m.authors = names;
    // the title ends at the first ". " (or "? ", "! ") that isn't after an initial ("E. coli")
    var re = /([.?!])\s+/g, t, end = -1;
    while ((t = re.exec(rest))) {
      if (t[1] === '.' && /(^|\s)[A-Z]$/.test(rest.slice(0, t.index))) continue;
      end = t.index + (t[1] === '.' ? 0 : 1);
      break;
    }
    if (end > 0) {
      m.title = rest.slice(0, end).trim();
      journalPart(rest.slice(end).replace(/^[.?!\s]+/, '').replace(/^In:\s*/i, ''), m);
    } else if (rest) m.title = rest.replace(/[.\s]+$/, '');
    Object.keys(m).forEach(function (k) { if (!m[k] || (Array.isArray(m[k]) && !m[k].length)) delete m[k]; });
    return m;
  }

  /**
   * A formatted bibliography's entries (its text, a paragraph a line): [{n (its number, or 0),
   * text, meta}]. Numbered ("1. …", "[1] …", "1) …", "1\t…") or not (author–year), a heading
   * ("References") left out; entries run together on one line are split at their numbers.
   */
  function parseBibliography(text) {
    var lines = String(text || '').split(/\r\n|[\r\n\u000b\u2028\u2029]/).map(function (l) { return l.replace(/\s+/g, ' ').trim(); }).filter(Boolean);
    var NUM = /^\[?(\d{1,4})(?:[.)\]]\]?|\t)\s*(?=\S)/;
    // all the entries on one line: split where the next number starts
    if (lines.length === 1 && /^\[?1[.)\]]\s/.test(lines[0])) {
      var one = lines[0], parts = [], from = 0;
      for (var n = 2; ; n++) {
        var re = new RegExp('\\s\\[?' + n + '[.)\\]]\\]?\\s'), at = re.exec(one.slice(from));
        if (!at) break;
        parts.push(one.slice(0, from + at.index).trim());
        one = one.slice(from + at.index).trim();
        from = String(n).length + 2;
      }
      parts.push(one);
      lines = parts;
    }
    var numbered = lines.filter(function (l) { return NUM.test(l); }).length >= Math.max(1, Math.ceil(lines.length / 2));
    var out = [];
    lines.forEach(function (l) {
      var m = NUM.exec(l);
      if (numbered && !m) {
        if (out.length) out[out.length - 1].text += ' ' + l;   // a paragraph break inside an entry
        return;
      }
      var body = numbered ? l.slice(m[0].length).trim() : l;
      // a heading: short, no year, no identifier
      if (!numbered && body.length < 40 && !/\b(1[6-9]\d\d|20\d\d)\b|10\.\d{4,9}\//.test(body)) return;
      out.push({ n: numbered ? +m[1] : 0, text: body });
    });
    out.forEach(function (e) { e.meta = referenceMeta(e.text); });
    return out;
  }

  /** Works found by their author and year in a bibliography's entries: the entry or null (none, or several). */
  function entryByAuthorYear(entries, family, year) {
    var f = fold(String(family).split(/\s+/).pop()), y = String(year), letter = /[a-z]$/.test(y) ? y.slice(-1) : '';
    var hits = entries.filter(function (e) {
      var a = (e.meta.authors || [])[0] || '';
      var fam = /^(?:[A-Z]\.?\s*-?)+\s/.test(a) ? a.replace(/^(?:[A-Z]\.?\s*-?)+\s/, '') : a;
      return fold(fam.split(/\s+/).pop()) === f && String(e.meta.date || '').slice(0, 4) === y.slice(0, 4);
    });
    if (hits.length > 1 && letter) {
      // "2013a" / "2013b": in the bibliography's order (the letters follow it)
      var i = letter.charCodeAt(0) - 97;
      var withLetter = entries.filter(function (e) { return new RegExp('\\b' + y.slice(0, 4) + letter + '\\b').test(e.text); });
      if (withLetter.length === 1 && hits.indexOf(withLetter[0]) >= 0) return withLetter[0];
      return hits[i] || null;
    }
    return hits.length === 1 ? hits[0] : null;
  }

  /**
   * The works of a formatted citation (its shown text) with what its control says (`items`:
   * SmartCite's [{doi, pmid, pmcid}], [] if nothing) and the bibliography's entries: [{meta,
   * options}] (a work's details: its entry's, with the control's identifiers), or null when the
   * citation can't be read. Each work's entry: the one with its DOI (or PubMed id), else the
   * citation's number (or author and year) in the same place.
   */
  function formattedCitation(text, items, entries) {
    items = items || []; entries = entries || [];
    var nums = citeNumbers(text), keys = nums ? null : citeAuthorYears(text);
    var byNumber = {};
    entries.forEach(function (e) { if (e.n && !byNumber[e.n]) byNumber[e.n] = e; });
    var refs = nums ? nums.map(function (n) { return byNumber[n] || null; })
      : keys ? keys.map(function (k) { return entryByAuthorYear(entries, k.family, k.year); }) : [];
    var count = items.length || refs.length;
    if (!count) return null;
    var lower = function (s) { return String(s || '').toLowerCase(); };
    var out = [];
    for (var i = 0; i < count; i++) {
      var it = items[i] || {}, entry = null;
      if (it.doi) entry = entries.filter(function (e) { return lower(e.meta.doi) === lower(it.doi); })[0] || null;
      if (!entry && it.pmid) entry = entries.filter(function (e) { return e.meta.pmid === it.pmid; })[0] || null;
      if (!entry && refs.length === count) {
        entry = refs[i];
        // the entry in its place is another work (its DOI isn't this one's)
        if (entry && it.doi && entry.meta.doi && lower(entry.meta.doi) !== lower(it.doi)) entry = null;
      }
      var meta = {};
      if (entry) Object.keys(entry.meta).forEach(function (k) { meta[k] = entry.meta[k]; });
      ['doi', 'pmid', 'pmcid'].forEach(function (k) { if (it[k]) meta[k] = it[k]; });
      out.push({ meta: meta, options: {}, entry: entry ? entry.n || entries.indexOf(entry) + 1 : 0 });
    }
    return out;
  }

  /** Field codes compared loosely (Word's Field.code and the OOXML's instruction): the same start
   *  (Word may give a field's code with a nested field's after it, EndNote's EN.CITE.DATA), ending
   *  where a word ends: "ADDIN EN.CITE" isn't the same as "ADDIN EN.CITE.DATA". */
  function sameCode(a, b) {
    var n = function (s) { return String(s || '').replace(/\s+/g, ' ').trim().slice(0, 60); };
    var x = n(a), y = n(b);
    if (!x || !y) return x === y;
    if (x === y) return true;
    var short = x.length < y.length ? x : y, long = x.length < y.length ? y : x;
    return long.indexOf(short) === 0 && long.charAt(short.length) === ' ';
  }

  return { scan: scan, classify: classify, classifyTag: classifyTag, endnoteCitation: endnoteCitation, endnoteMeta: endnoteMeta,
           decodeBase64Text: decodeBase64Text, zoteroBookmarks: zoteroBookmarks, sameCode: sameCode, unescapeXML: unescapeXML,
           SMARTCITE: SMARTCITE, classifyControl: classifyControl, smartCiteTag: smartCiteTag, citeNumbers: citeNumbers,
           citeAuthorYears: citeAuthorYears, referenceMeta: referenceMeta, parseBibliography: parseBibliography,
           formattedCitation: formattedCitation, cleanDOI: cleanDOI };
})();
if (typeof module !== 'undefined') module.exports = LuntaFields;
