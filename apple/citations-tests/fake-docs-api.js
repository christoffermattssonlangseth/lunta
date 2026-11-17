// A small stand-in for the Google Docs REST API (documents.get with tabs, documents.batchUpdate):
// enough of it to run the browser extension's Google Docs code (extension/gdocs) in node. A
// segment (a tab's body, a footnote) is a list of characters with their text style; a paragraph's
// style sits on its "\n". Indexes are UTF-16 units as in the API; the body starts at 1 (0 is the
// section break). Inserted text takes the style of the character before it in its paragraph
// (else the one after), as Docs does; a new paragraph mark copies its paragraph's style.

/** Paragraph content: strings, {text, link, sup, bold, italic}, or {footnote: [paragraphs]}. */
function P(...pieces) { return { pieces, style: {} }; }
/** A paragraph with a paragraph style ({namedStyleType, bullet, indentStart, …}). */
function PS(style, ...pieces) { return { pieces, style }; }
function link(text, url, extra) { return Object.assign({ text, link: url }, extra || {}); }

class FakeDocsAPI {
  /** tabs: [{tabId, paragraphs}] or paragraphs: one tab. */
  constructor(opts) {
    this.revision = 1;
    this.fnCount = 0;
    this.calls = { get: 0, batchUpdate: 0, requests: [] };
    this.onGet = null;
    const tabs = opts.tabs || [{ tabId: 't.0', paragraphs: opts.paragraphs }];
    this.tabs = tabs.map((t) => ({ tabId: t.tabId, body: [], base: 1, footnotes: {}, named: [] }));
    tabs.forEach((t, k) => { this.tabs[k].body = this.build(this.tabs[k], t.paragraphs); });
  }

  build(tab, paragraphs) {
    const chars = [];
    paragraphs.forEach((p) => {
      p.pieces.forEach((x) => {
        if (typeof x === 'string') x = { text: x };
        if (x.footnote) {
          const id = 'kix.fn' + (++this.fnCount);
          tab.footnotes[id] = { chars: this.build(tab, x.footnote), base: 0 };
          chars.push({ c: '\u0001', fn: id, st: {} });
          return;
        }
        const st = {};
        if (x.link) st.link = x.link;
        if (x.sup) st.baselineOffset = 'SUPERSCRIPT';
        if (x.bold) st.bold = true;
        if (x.italic) st.italic = true;
        for (const c of x.text.split('')) chars.push({ c, st: Object.assign({}, st) });
      });
      chars.push({ c: '\n', st: {}, ps: Object.assign({}, p.style) });
    });
    return chars;
  }

  // ---- reading ----

  tab(tabId) {
    const t = tabId ? this.tabs.find((x) => x.tabId === tabId) : this.tabs[0];
    if (!t) throw this.error(400, 'Invalid tab ' + tabId);
    return t;
  }
  segment(tabId, segmentId) {
    const t = this.tab(tabId);
    if (!segmentId) return { chars: t.body, base: t.base };
    const f = t.footnotes[segmentId];
    if (!f) throw this.error(400, 'Invalid segment ' + segmentId);
    return f;
  }

  render(chars, base) {
    const content = [];
    if (base === 1) content.push({ startIndex: 0, endIndex: 1, sectionBreak: {} });
    let i = 0;
    while (i < chars.length) {
      let j = i;
      while (chars[j].c !== '\n') j++;
      const elements = [];
      let k = i;
      while (k <= j) {
        const ch = chars[k];
        if (ch.fn) { elements.push({ startIndex: base + k, endIndex: base + k + 1, footnoteReference: { footnoteId: ch.fn } }); k++; continue; }
        let m = k;
        const key = JSON.stringify(ch.st);
        while (m + 1 <= j && !chars[m + 1].fn && JSON.stringify(chars[m + 1].st) === key) m++;
        const ts = {};
        if (ch.st.link) ts.link = { url: ch.st.link };
        ['bold', 'italic', 'underline'].forEach((f) => { if (ch.st[f]) ts[f] = true; });
        if (ch.st.baselineOffset) ts.baselineOffset = ch.st.baselineOffset;
        elements.push({ startIndex: base + k, endIndex: base + m + 1, textRun: { content: chars.slice(k, m + 1).map((x) => x.c).join(''), textStyle: ts } });
        k = m + 1;
      }
      const para = { elements, paragraphStyle: { namedStyleType: chars[j].ps.namedStyleType || 'NORMAL_TEXT' } };
      if (chars[j].ps.bullet) para.bullet = { listId: 'kix.list' };
      content.push({ startIndex: base + i, endIndex: base + j + 1, paragraph: para });
      i = j + 1;
    }
    return content;
  }

  async get(docId) {
    this.calls.get++;
    const out = {
      documentId: docId, title: 'Test document', revisionId: 'rev' + this.revision,
      tabs: this.tabs.map((t) => {
        const namedRanges = {};
        t.named.forEach((n) => { (namedRanges[n.name] = namedRanges[n.name] || { name: n.name, namedRanges: [] }).namedRanges.push({ namedRangeId: n.id, name: n.name, ranges: [n.range] }); });
        const footnotes = {};
        Object.keys(t.footnotes).forEach((id) => { footnotes[id] = { footnoteId: id, content: this.render(t.footnotes[id].chars, 0) }; });
        return { tabProperties: { tabId: t.tabId }, documentTab: { body: { content: this.render(t.body, 1) }, footnotes, namedRanges } };
      })
    };
    const snapshot = JSON.parse(JSON.stringify(out));
    if (this.onGet) this.onGet(this.calls.get);
    return snapshot;
  }

  // ---- writing ----

  error(status, message) { const e = new Error(message); e.status = status; return e; }

  async batchUpdate(docId, requests, requiredRevisionId) {
    this.calls.batchUpdate++;
    this.calls.requests.push(requests);
    if (requiredRevisionId && requiredRevisionId !== 'rev' + this.revision) {
      const e = this.error(400, 'The required revision ID does not match the latest revision.');
      e.kind = 'revision';
      throw e;
    }
    // all or nothing, as the API
    const saved = JSON.stringify({ tabs: this.tabs });
    try {
      requests.forEach((r) => this.apply(r));
    } catch (e) {
      this.tabs = JSON.parse(saved).tabs;
      throw e;
    }
    this.revision++;
    return { replies: requests.map(() => ({})) };
  }

  pos(seg, index, allowEnd) {
    const p = index - seg.base;
    if (p < 0 || p > seg.chars.length - (allowEnd ? 0 : 1)) throw this.error(400, 'Index ' + index + ' must be less than the end index of the referenced segment, ' + (seg.base + seg.chars.length));
    return p;
  }

  apply(r) {
    const name = Object.keys(r)[0], q = r[name];
    if (name === 'insertText') {
      const seg = this.segment(q.location.tabId, q.location.segmentId);
      const p = this.pos(seg, q.location.index);
      if (!q.text) throw this.error(400, 'Text must not be empty');
      const prev = seg.chars[p - 1], like = prev && prev.c !== '\n' && !prev.fn ? prev : seg.chars[p];
      let mark = p; while (seg.chars[mark].c !== '\n') mark++;
      const ps = seg.chars[mark].ps;
      const add = q.text.split('').map((c) => (c === '\n' ? { c, st: {}, ps: Object.assign({}, ps) } : { c, st: Object.assign({}, like && !like.fn ? like.st : {}) }));
      seg.chars.splice(p, 0, ...add);
    } else if (name === 'deleteContentRange') {
      const seg = this.segment(q.range.tabId, q.range.segmentId);
      const s = this.pos(seg, q.range.startIndex), e = this.pos(seg, q.range.endIndex, true);
      if (e >= seg.chars.length) throw this.error(400, 'The range cannot include the last newline character of the segment');
      if (e <= s) throw this.error(400, 'Empty range');
      // a paragraph mark removed: the paragraph takes the next one's mark (style)
      seg.chars.splice(s, e - s);
    } else if (name === 'updateTextStyle') {
      const seg = this.segment(q.range.tabId, q.range.segmentId);
      const s = this.pos(seg, q.range.startIndex), e = this.pos(seg, q.range.endIndex, true);
      if (e <= s) throw this.error(400, 'Empty range');
      const fields = q.fields.split(',');
      for (let i = s; i < e; i++) {
        const st = seg.chars[i].st;
        fields.forEach((f) => {
          const v = f === 'link' ? (q.textStyle.link && q.textStyle.link.url) : q.textStyle[f];
          const k = f === 'foregroundColor' ? 'color' : f;
          if (v === undefined || v === false || v === 'NONE') delete st[k]; else st[k] = v;
        });
      }
    } else if (name === 'updateParagraphStyle' || name === 'deleteParagraphBullets') {
      const seg = this.segment(q.range.tabId, q.range.segmentId);
      const s = this.pos(seg, q.range.startIndex), e = this.pos(seg, q.range.endIndex, true);
      let i = s; while (seg.chars[i].c !== '\n') i++;   // the mark of the first paragraph in range
      for (; i < seg.chars.length; i++) {
        if (seg.chars[i].c !== '\n') continue;
        if (name === 'deleteParagraphBullets') delete seg.chars[i].ps.bullet;
        else q.fields.split(',').forEach((f) => { if (q.paragraphStyle[f] === undefined) delete seg.chars[i].ps[f]; else seg.chars[i].ps[f] = q.paragraphStyle[f]; });
        if (i >= e - 1) break;
      }
    } else if (name === 'createNamedRange') {
      if (!q.name || q.name.length > 256) throw this.error(400, 'Invalid name');
      const t = this.tab(q.range.tabId), seg = this.segment(q.range.tabId, q.range.segmentId);
      const s = this.pos(seg, q.range.startIndex), e = this.pos(seg, q.range.endIndex, true);
      if (e <= s) throw this.error(400, 'Empty range');
      t.named.push({ id: 'nr' + Math.random().toString(36).slice(2), name: q.name, range: Object.assign({}, q.range) });
    } else if (name === 'deleteNamedRange') {
      this.tabs.forEach((t) => { t.named = t.named.filter((n) => n.name !== q.name); });
    } else {
      throw this.error(400, 'Unsupported request ' + name);
    }
  }

  // ---- what a person does in the editor ----

  /** Someone types text at an index (a co-author, or the writer meanwhile). */
  type(index, text, tabId) {
    this.apply({ insertText: { location: { index, tabId }, text } });
    this.revision++;
  }

  /** Docs' paste of the extension's placeholder HTML at the cursor (index): the link and its text. */
  paste(index, html, tabId, segmentId) {
    const m = /<a href="([^"]*)">([^<]*)<\/a>/.exec(html);
    const url = m[1].replace(/&amp;/g, '&').replace(/&quot;/g, '"');
    const text = m[2].replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&amp;/g, '&');
    this.apply({ insertText: { location: { index, tabId, segmentId }, text } });
    // pasted links look like links
    this.apply({ updateTextStyle: { range: { startIndex: index, endIndex: index + text.length, tabId, segmentId }, textStyle: { link: { url }, underline: true }, fields: 'link,underline' } });
    this.revision++;
  }

  // ---- looking at the result ----

  text(tabId, segmentId) { return this.segment(tabId, segmentId).chars.map((c) => (c.fn ? '^' : c.c)).join(''); }
  paragraphs(tabId) { return this.text(tabId).split('\n').slice(0, -1); }
  /** The link at an index (body of the first tab unless given). */
  linkAt(index, tabId, segmentId) { const seg = this.segment(tabId, segmentId); return seg.chars[index - seg.base].st.link || null; }
  styleAt(index, tabId, segmentId) { const seg = this.segment(tabId, segmentId); return seg.chars[index - seg.base].st; }
  indexOf(s, tabId, segmentId) { const seg = this.segment(tabId, segmentId); const i = this.text(tabId, segmentId).indexOf(s); return i < 0 ? -1 : i + seg.base; }
  paragraphStyles(tabId) { return this.segment(tabId).chars.filter((c) => c.c === '\n').map((c) => c.ps); }
  footnoteIds(tabId) { return Object.keys(this.tab(tabId).footnotes); }
  storeNames(tabId) { return this.tab(tabId).named.map((n) => n.name).filter((n) => n.startsWith('lunta:')); }
}

module.exports = { FakeDocsAPI, P, PS, link };
