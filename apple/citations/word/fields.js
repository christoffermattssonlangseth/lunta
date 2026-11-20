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
   * [{instr, data (base64 of <w:fldData>, ""), text (shown), depth}], and its content controls'
   * tags in order: {fields, tags}.
   */
  function scan(xml) {
    xml = String(xml || '');
    // only the main document's body (getOoxml wraps it in a package with styles, numbering…)
    var b = xml.search(/<w:body[\s>]/);
    if (b >= 0) {
      var e = xml.indexOf('</w:body>', b);
      xml = xml.slice(b, e < 0 ? xml.length : e);
    }
    var fields = [], stack = [], tags = [];
    var re = /<w:fldSimple\b([^>]*?)(\/?)>|<\/w:fldSimple>|<w:fldChar\b([^>]*?)(\/?)>|<w:fldData\b[^>]*>([\s\S]*?)<\/w:fldData>|<w:instrText\b[^>]*>([\s\S]*?)<\/w:instrText>|<w:instrText\b[^>]*\/>|<w:t\b[^>]*>([\s\S]*?)<\/w:t>|<w:tab\/>|<w:br\/>|<\/w:p>|<w:tag\b([^>]*)\/>/g;
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
      } else if (m[8] !== undefined) {
        tags.push(attr(m[8], 'w:val') || '');
      }
    }
    fields.forEach(function (f) { f.text = f.text.replace(/\s+$/, ''); delete f.state; });
    return { fields: fields, tags: tags };
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
           decodeBase64Text: decodeBase64Text, zoteroBookmarks: zoteroBookmarks, sameCode: sameCode, unescapeXML: unescapeXML };
})();
if (typeof module !== 'undefined') module.exports = LuntaFields;
