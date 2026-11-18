import SwiftUI
import WebKit
import PDFKit

// Find in paper: a search bar over the reader that finds a word or phrase in the paper and steps
// through the matches. In the reading view the page script below does the work (shared with the
// desktop and Android apps through scripts/extract-reader-assets.mjs); in the PDF view PDFKit's
// own search. The matches are drawn over the text, never into it: highlights and saved reading
// places are character offsets into the page's text, which must not change.

extension ReaderAssets {
    /// `luntaFind(query, fromTop)` → {count, index, capped}; `luntaFindNext()`, `luntaFindPrev()`
    /// → {count, index}; `luntaFindClear()`. Matching ignores case, accents (é = e, ñ = n),
    /// the kind of quote or dash and runs of whitespace, and runs across markup (<i>, <sup>, links).
    /// Matches are marked with the CSS Custom Highlight API (no change to the page's DOM), or, where
    /// that is missing, with <mark class="m-find"> elements that are taken out again on clear.
    /// The first match shown is the first at or after the place being read (or the paper's start
    /// with `fromTop`); the current one is scrolled to (in pages: its page is turned to). A match in
    /// the caption of a figure shown as a card shows the card. `window.luntaFindZone = [top, bottom]`
    /// (fractions of the window's height) is where a match counts as in view, when scrolling.
    static let findJS = #"""
    (function(){
      if (window.luntaFind) return;
      var useHL = !!(window.CSS && CSS.highlights && window.Highlight);
      var st = document.createElement('style');
      st.textContent =
        '::highlight(lunta-find) { background-color: rgba(255,170,20,.38); }' +
        '::highlight(lunta-find-current) { background-color: #ff9f0a; color: #141414; }' +
        'mark.m-find { background: rgba(255,170,20,.38); color: inherit; padding: 0; border-radius: 0; }' +
        'mark.m-find.m-find-current { background: #ff9f0a; color: #141414; }' +
        '.m-figcard.m-find-card { box-shadow: 0 0 0 3px #ff9f0a; }' +
        '@media (prefers-color-scheme: dark) { ::highlight(lunta-find) { background-color: rgba(255,159,10,.34); }' +
        ' mark.m-find { background: rgba(255,159,10,.34); } }';
      document.documentElement.appendChild(st);

      var MAX = 2000;
      // text that isn't read: scripts, MathML's TeX source, the figure cards' own (drawn) words
      var SKIP = 'script, style, noscript, template, annotation, annotation-xml, .m-figcard, #m-page-no';
      // a word never runs on from one of these into the next
      var BLOCK = 'p, li, dd, dt, td, th, h1, h2, h3, h4, h5, h6, figcaption, caption, blockquote, pre, div, section, figure, table, tr, ul, ol, dl, header, footer, aside, nav, article, body';
      var QUOTES = { '\u2018': "'", '\u2019': "'", '\u201A': "'", '\u201B': "'", '\u2032': "'",
                     '\u201C': '"', '\u201D': '"', '\u201E': '"', '\u201F': '"', '\u2033': '"', '\u00AB': '"', '\u00BB': '"' };
      var folded = {};
      // a spacing accent, as text taken from a PDF has them ("Gudin\u02DC o", "Jos\u00B4e"): left out,
      // with the space beside it
      var ACC = '\u0001';
      // one character as it is compared: lower case, without accents, plain quotes and dashes
      function foldChar(c){
        var f = folded[c]; if (f !== undefined) return f;
        if (/^[\u00AD\u200B-\u200D\u2060\uFEFF]$/.test(c)) f = '';        // soft hyphens, zero-width spaces
        else if (/^[\u00A8\u00AF\u00B4\u00B8\u02C6-\u02DD]$/.test(c)) f = ACC;
        else if (/^\s$/.test(c)) f = ' ';
        else if (QUOTES[c]) f = QUOTES[c];
        else if (/^[\u2010-\u2015\u2212\uFE58\uFE63\uFF0D]$/.test(c)) f = '-';
        else f = c.normalize('NFKD').toLowerCase().replace(/\p{M}/gu, '');
        return (folded[c] = f);
      }
      // the folded form of the character at v[i], and its length (2 for a surrogate pair)
      function unit(v, i){
        var c = v.charCodeAt(i);
        if (c < 128) return [(c === 32 || (c >= 9 && c <= 13)) ? ' ' : (c >= 65 && c <= 90 ? String.fromCharCode(c + 32) : v[i]), 1];
        var len = (c >= 0xD800 && c <= 0xDBFF && i + 1 < v.length) ? 2 : 1;
        return [foldChar(v.substr(i, len)), len];
      }
      function fold(s){
        var out = '', last = ' ', eat = false;
        for (var i = 0; i < s.length;) {
          var u = unit(s, i); i += u[1];
          if (u[0] === ACC) { if (last === ' ' && out) { out = out.slice(0, -1); last = out ? out[out.length - 1] : ' '; } eat = true; continue; }
          for (var k = 0; k < u[0].length; k++) {
            var ch = u[0][k];
            if (ch === ' ' && (last === ' ' || eat)) continue;
            out += ch; last = ch; eat = false;
          }
        }
        return out.replace(/ $/, '');
      }

      // The page's readable text, folded, with the way back from each folded character to the
      // text node and offset it came from.
      function build(){
        var cardsOn = document.documentElement.classList.contains('m-cards');
        var nodes = [], starts = [], cards = [], seen = new Map();
        var out = [], A = [], B = [], last = ' ', total = 0, prevBlock = null, gap = false, eat = false;
        function emit(f, a, b){
          if (f === ACC) {
            if (last === ' ' && out.length) { out.pop(); A.pop(); B.pop(); last = out.length ? out[out.length - 1] : ' '; }
            eat = true; return;
          }
          for (var k = 0; k < f.length; k++) {
            var ch = f[k];
            if (ch === ' ' && (last === ' ' || eat)) continue;
            out.push(ch); A.push(a); B.push(b); last = ch; eat = false;
          }
        }
        var w = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT, null), n;
        while ((n = w.nextNode())) {
          var v = n.nodeValue, p = n.parentElement;
          if (!v || !p) continue;
          var info = seen.get(p);
          if (info === undefined) {
            info = !p.closest(SKIP) && (!p.checkVisibility || p.checkVisibility());
            var card = null;
            // a figure shown as a card: its caption is searched (the card is shown for it), the
            // rest of its collapsed content is not
            if (info && cardsOn) {
              var fig = p.closest('figure.m-carded');
              if (fig) {
                var cap = p.closest('figcaption, .caption');
                if (cap && fig.contains(cap)) card = fig.querySelector('.m-figcard'); else info = false;
              }
            }
            info = info ? { card: card, block: p.closest(BLOCK) } : false;
            seen.set(p, info);
          }
          if (!info) { gap = true; continue; }
          if (info.block !== prevBlock || gap) emit(' ', total, total);
          prevBlock = info.block; gap = false;
          nodes.push(n); starts.push(total); cards.push(info.card);
          for (var i = 0; i < v.length;) {
            var u = unit(v, i), f = u[0];
            if (f.length === 1 && f !== ' ' && f !== ACC) { out.push(f); A.push(total + i); B.push(total + i + u[1]); last = f; eat = false; }
            else emit(f, total + i, total + i + u[1]);
            i += u[1];
          }
          total += v.length;
        }
        return { text: out.join(''), A: A, B: B, nodes: nodes, starts: starts, cards: cards };
      }
      // the text node holding offset g (`end`: the node a match ending at g ends in)
      function point(ix, g, end){
        var lo = 0, hi = ix.starts.length - 1;
        while (lo < hi) {
          var mid = (lo + hi + 1) >> 1;
          if (end ? ix.starts[mid] < g : ix.starts[mid] <= g) lo = mid; else hi = mid - 1;
        }
        var off = Math.max(0, Math.min(ix.nodes[lo].nodeValue.length, g - ix.starts[lo]));
        return [lo, off];
      }

      var ix = null, matches = [], cur = -1, query = '', active = false, capped = false;
      var originOff = null, obs = null, timer = null;

      // where the reader is (a character offset as the reading position counts it), and the text
      // node and offset that is
      function place(){
        try { return window.luntaPlace ? window.luntaPlace() : (window.luntaReadingOffset ? window.luntaReadingOffset() : 0); } catch (x) { return 0; }
      }
      function placePoint(off){
        var w = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT, null), n, pos = 0;
        while ((n = w.nextNode())) { var l = n.nodeValue.length; if (pos + l > off) return [n, off - pos]; pos += l; }
        return null;
      }
      function firstFrom(off){
        if (off == null) return 0;
        var pt = placePoint(off);
        if (!pt) return 0;
        for (var i = 0; i < matches.length; i++) {
          try { if (matches[i].range.comparePoint(pt[0], pt[1]) <= 0) return i; } catch (x) { return 0; }
        }
        return 0;   // none after it: from the start
      }

      function run(go, keepAt){
        unmark();
        matches = []; cur = -1; capped = false;
        var q = fold(query);
        if (q) {
          ix = build();
          var t = ix.text, at = t.indexOf(q);
          while (at >= 0) {
            if (matches.length >= MAX) { capped = true; break; }
            var a = point(ix, ix.A[at], false), b = point(ix, ix.B[at + q.length - 1], true);
            var r = document.createRange();
            try {
              r.setStart(ix.nodes[a[0]], a[1]); r.setEnd(ix.nodes[b[0]], b[1]);
              matches.push({ range: r, start: ix.A[at], a: a, b: b, card: ix.cards[a[0]] });
            } catch (x) {}
            at = t.indexOf(q, at + q.length);
          }
        }
        if (matches.length) {
          if (keepAt != null) {
            cur = matches.length - 1;
            for (var i = 0; i < matches.length; i++) if (matches[i].start >= keepAt) { cur = i; break; }
          } else cur = firstFrom(originOff);
        }
        mark();
        paintAll(); paintCurrent();
        if (obs) obs.takeRecords();      // (our own marks)
        if (go && cur >= 0) reveal(matches[cur]);
      }

      // the fallback: <mark> elements around the matches (last first: splitting a text node keeps
      // the places before it valid)
      function mark(){
        if (useHL || !matches.length) return;
        for (var i = matches.length - 1; i >= 0; i--) {
          var m = matches[i];
          for (var j = m.b[0]; j >= m.a[0]; j--) {
            var node = ix.nodes[j], len = node.nodeValue.length;
            var from = j === m.a[0] ? m.a[1] : 0, to = j === m.b[0] ? m.b[1] : len;
            if (from >= to || !node.parentNode) continue;
            if (to < len) node.splitText(to);
            var target = from > 0 ? node.splitText(from) : node;
            var mk = document.createElement('mark');
            mk.className = 'm-find'; mk.setAttribute('data-find', i);
            target.parentNode.insertBefore(mk, target); mk.appendChild(target);
          }
        }
        // moving the text into the marks moved the ranges' ends: set them again around the marks
        var groups = {};
        document.querySelectorAll('mark.m-find').forEach(function(x){ (groups[x.getAttribute('data-find')] = groups[x.getAttribute('data-find')] || []).push(x); });
        matches.forEach(function(m, k){
          var g = groups[k]; if (!g) return;
          m.range = document.createRange(); m.range.setStartBefore(g[0]); m.range.setEndAfter(g[g.length - 1]);
        });
      }
      function unmark(){
        var ms = document.querySelectorAll('mark.m-find'); if (!ms.length) return;
        var parents = new Set();
        ms.forEach(function(m){
          var p = m.parentNode; if (!p) return;
          while (m.firstChild) p.insertBefore(m.firstChild, m);
          p.removeChild(m); parents.add(p);
        });
        // the text split around the marks joined up again (only the parents' own text: a whole
        // subtree's normalize would be slow, and nothing else was split)
        parents.forEach(function(p){
          if (!p.isConnected) return;
          for (var c = p.firstChild; c; ) {
            var nx = c.nextSibling;
            if (c.nodeType === 3 && nx && nx.nodeType === 3) { c.appendData(nx.nodeValue); p.removeChild(nx); continue; }
            c = nx;
          }
        });
      }

      function paintAll(){
        if (!useHL) return;
        if (!matches.length) { CSS.highlights.delete('lunta-find'); return; }
        var h = new Highlight();
        for (var i = 0; i < matches.length; i++) h.add(matches[i].range);
        CSS.highlights.set('lunta-find', h);
      }
      function paintCurrent(){
        document.querySelectorAll('.m-find-card').forEach(function(x){ x.classList.remove('m-find-card'); });
        if (useHL) {
          if (cur < 0) CSS.highlights.delete('lunta-find-current');
          else { var h = new Highlight(matches[cur].range); h.priority = 1; CSS.highlights.set('lunta-find-current', h); }
        } else {
          document.querySelectorAll('mark.m-find-current').forEach(function(x){ x.classList.remove('m-find-current'); });
          if (cur >= 0) document.querySelectorAll('mark.m-find[data-find="' + cur + '"]').forEach(function(x){ x.classList.add('m-find-current'); });
        }
        if (cur >= 0 && matches[cur].card && document.documentElement.classList.contains('m-cards')) matches[cur].card.classList.add('m-find-card');
      }

      function rectOf(range){
        var rs = range.getClientRects();
        for (var i = 0; i < rs.length; i++) if (rs[i].width > 0 || rs[i].height > 0) return rs[i];
        var b = range.getBoundingClientRect();
        if (b.width > 0 || b.height > 0) return b;
        var el = range.startContainer.nodeType === 1 ? range.startContainer : range.startContainer.parentElement;
        return el ? el.getBoundingClientRect() : null;
      }
      // the current match on the screen: its page (pages) or a little above the middle (scroll),
      // left alone when it is already in view
      function reveal(m){
        if (window.luntaHold) window.luntaHold();    // stop keeping a restored place: you're going elsewhere
        var cardsOn = document.documentElement.classList.contains('m-cards');
        var rect;
        if (m.card && cardsOn) rect = m.card.getBoundingClientRect();
        else {
          rect = rectOf(m.range);
          // inside a box that scrolls by itself (a wide table): into view in the box first
          var el = m.range.startContainer.nodeType === 1 ? m.range.startContainer : m.range.startContainer.parentElement;
          var box = el && el.closest('.m-scroll');
          if (box && rect) {
            var bb = box.getBoundingClientRect();
            if (rect.left < bb.left || rect.right > bb.right) box.scrollLeft += rect.left - bb.left - bb.width / 3;
            if (rect.top < bb.top || rect.bottom > bb.bottom) box.scrollTop += rect.top - bb.top - bb.height / 3;
            rect = rectOf(m.range);
          }
        }
        if (!rect) return;
        if (window.luntaPaged && window.luntaPaged()) { window.luntaShowRect(rect, false); return; }
        var h = window.innerHeight, zone = window.luntaFindZone || [0.15, 0.85];
        if (rect.top < h * zone[0] || rect.bottom > h * zone[1]) {
          var y = rect.top + window.scrollY - h * Math.max(0.05, zone[0] + (zone[1] - zone[0]) * 0.3);
          window.scrollTo({ top: Math.max(0, y), left: window.scrollX, behavior: 'instant' });
        }
      }

      // the page changed under the matches (highlights drawn again, figures as cards or not):
      // find them again, keeping the current one
      function watch(){
        if (obs || !window.MutationObserver) return;
        obs = new MutationObserver(function(){
          clearTimeout(timer);
          timer = setTimeout(function(){ if (active && query) run(false, cur >= 0 ? matches[cur].start : null); }, 150);
        });
        obs.observe(document.body, { childList: true, characterData: true, subtree: true });
        obs.observe(document.documentElement, { attributes: true, attributeFilter: ['class'] });
      }
      function state(){ return { count: matches.length, index: cur, capped: capped }; }

      window.luntaFind = function(q, fromTop){
        if (!active) { active = true; originOff = fromTop ? null : place(); watch(); }
        else if (fromTop) originOff = null;
        query = String(q || '');
        clearTimeout(timer);
        run(true, null);
        return state();
      };
      function step(d){
        if (!active || !matches.length) return state();
        if (obs && obs.takeRecords().length) { clearTimeout(timer); run(false, matches[cur].start); if (!matches.length) return state(); }
        cur = (cur + d + matches.length) % matches.length;
        paintCurrent();
        reveal(matches[cur]);
        return state();
      }
      window.luntaFindNext = function(){ return step(1); };
      window.luntaFindPrev = function(){ return step(-1); };
      window.luntaFindClear = function(){
        active = false; query = ''; originOff = null; clearTimeout(timer);
        if (obs) { obs.disconnect(); obs = null; }
        matches = []; cur = -1; capped = false; ix = null;
        unmark(); paintAll(); paintCurrent();
        return true;
      };
      // for tests: what is found and where the page is
      window.luntaFindState = function(){
        var s = state();
        s.scrollX = Math.round(window.scrollX); s.scrollY = Math.round(window.scrollY);
        s.inCard = cur >= 0 && !!matches[cur].card && document.documentElement.classList.contains('m-cards');
        if (cur >= 0) { var r = rectOf(matches[cur].range); s.text = matches[cur].range.toString(); if (r) { s.top = Math.round(r.top); s.left = Math.round(r.left); } }
        s.marks = document.querySelectorAll('mark.m-hl').length; s.findMarks = document.querySelectorAll('mark.m-find').length;
        s.highlightAPI = useHL;
        return s;
      };
    })();
    """#
}

// MARK: - Searching

struct FindResult: Equatable {
    var count = 0
    var index = -1
    var capped = false
    static let none = FindResult()
}

/// A place in the reader to come back to after finding.
struct FindPlace {
    let key: Double
    let tolerance: Double
    let restore: @MainActor () -> Void
    func differs(from other: FindPlace) -> Bool { abs(key - other.key) > tolerance }
}

/// What the find bar searches: the reading view or the PDF.
@MainActor protocol FindBackend: AnyObject {
    func find(_ query: String, fromTop: Bool) async -> FindResult
    func findStep(_ forward: Bool) async -> FindResult
    func findClear()
    func findPlace() async -> FindPlace?
}

extension ReaderController: FindBackend {
    private func result(_ any: Any?) -> FindResult {
        guard let d = any as? [String: Any] else { return .none }
        return FindResult(count: (d["count"] as? NSNumber)?.intValue ?? 0, index: (d["index"] as? NSNumber)?.intValue ?? -1,
                          capped: (d["capped"] as? NSNumber)?.boolValue ?? false)
    }

    func find(_ query: String, fromTop: Bool) async -> FindResult {
        guard let v = webView else { return .none }
        let arg = (try? JSONSerialization.data(withJSONObject: [query])).flatMap { String(data: $0, encoding: .utf8) } ?? "[\"\"]"
        return result(try? await v.evaluateJavaScript("window.luntaFind ? luntaFind(\(arg)[0], \(fromTop)) : null"))
    }

    func findStep(_ forward: Bool) async -> FindResult {
        result(try? await webView?.evaluateJavaScript(forward ? "window.luntaFindNext && luntaFindNext()" : "window.luntaFindPrev && luntaFindPrev()"))
    }

    func findClear() { webView?.evaluateJavaScript("window.luntaFindClear && luntaFindClear()") }

    func findPlace() async -> FindPlace? {
        guard let o = await readingOffset() else { return nil }
        return FindPlace(key: Double(o), tolerance: 200) { [weak self] in self?.scroll(toOffset: o) }
    }
}

#if os(macOS)
private typealias FindColor = NSColor
#else
private typealias FindColor = UIColor
#endif

/// Finding in the PDF: PDFKit's search (in the background), every match shown with
/// `highlightedSelections` and the current one in a stronger colour. The PDF's own selection is
/// left alone (selecting would bring up the highlight bar), as are the highlights and drawings.
@MainActor
final class PDFFinder: FindBackend {
    weak var highlighter: PDFHighlighter?
    private var matches: [PDFSelection] = []
    private var cur = -1
    private var capped = false
    private var generation = 0
    private var observers: [NSObjectProtocol] = []
    private var pending: CheckedContinuation<[PDFSelection], Never>?
    private var found: [PDFSelection] = []
    private weak var searching: PDFDocument?
    private static let limit = 2000

    private var view: PDFView? { highlighter?.view }

    func find(_ query: String, fromTop: Bool) async -> FindResult {
        let q = query.split(whereSeparator: \.isWhitespace).joined(separator: " ")
        cancelSearch()
        matches = []; cur = -1; capped = false
        paint()
        guard !q.isEmpty, let v = view, let doc = v.document else { return .none }
        let gen = generation
        let all = await search(q, in: doc)
        guard gen + 1 == generation, view === v else { return result }   // a newer search started
        capped = all.count >= Self.limit
        // in reading order (PDFKit can report them out of order)
        func key(_ sel: PDFSelection) -> (Int, Int) {
            guard let page = sel.pages.first else { return (Int.max, 0) }
            let r = sel.range(at: 0, on: page)
            return (doc.index(for: page), r.location == NSNotFound ? 0 : r.location)
        }
        matches = Array(all.map { (key($0), $0) }.sorted { $0.0 < $1.0 }.map(\.1).prefix(Self.limit))
        if !matches.isEmpty {
            cur = 0
            if !fromTop, let page = v.currentPage {
                let here = doc.index(for: page)
                cur = matches.firstIndex { s in s.pages.first.map { doc.index(for: $0) >= here } ?? false } ?? 0
            }
        }
        paint()
        reveal()
        return result
    }

    func findStep(_ forward: Bool) async -> FindResult {
        guard !matches.isEmpty else { return result }
        cur = (cur + (forward ? 1 : -1) + matches.count) % matches.count
        paint()
        reveal()
        return result
    }

    func findClear() {
        cancelSearch()
        matches = []; cur = -1; capped = false
        paint()
    }

    func findPlace() async -> FindPlace? {
        guard let v = view, let doc = v.document, let d = v.currentDestination, let page = d.page else { return nil }
        let key = Double(doc.index(for: page)) * 100_000 + Double(d.point.y)
        return FindPlace(key: key, tolerance: 40) { [weak v] in v?.go(to: d) }
    }

    private var result: FindResult { FindResult(count: matches.count, index: cur, capped: capped) }

    #if DEBUG
    /// The current match's page (1-based) and its bounds there, for tests.
    var currentMatchInfo: String {
        guard matches.indices.contains(cur), let page = matches[cur].pages.first, let doc = page.document else { return "-" }
        let b = matches[cur].bounds(for: page)
        let marks = page.annotations.filter { $0.userName == "lunta-find" }.count
        return "match page=\(doc.index(for: page) + 1) y=\(Int(b.minY)) '\(matches[cur].string ?? "")' currentMarks=\(marks)"
    }
    #endif

    private func search(_ q: String, in doc: PDFDocument) async -> [PDFSelection] {
        generation += 1
        let gen = generation
        searching = doc
        found = []
        return await withCheckedContinuation { c in
            pending = c
            let nc = NotificationCenter.default
            observers = [
                nc.addObserver(forName: .PDFDocumentDidFindMatch, object: doc, queue: .main) { [weak self] n in
                    MainActor.assumeIsolated {
                        guard let self, self.generation == gen,
                              let s = n.userInfo?["PDFDocumentFoundSelection"] as? PDFSelection else { return }
                        self.found.append(s)
                        if self.found.count >= Self.limit { self.searching?.cancelFindString(); self.finish() }
                    }
                },
                nc.addObserver(forName: .PDFDocumentDidEndFind, object: doc, queue: .main) { [weak self] _ in
                    MainActor.assumeIsolated { if let self, self.generation == gen { self.finish() } }
                },
            ]
            doc.beginFindString(q, withOptions: [.caseInsensitive, .diacriticInsensitive])
        }
    }

    private func finish() {
        observers.forEach(NotificationCenter.default.removeObserver)
        observers = []
        let c = pending
        pending = nil
        c?.resume(returning: found)
    }

    private func cancelSearch() {
        if let d = searching, d.isFinding { d.cancelFindString() }
        searching = nil
        generation += 1
        found = []
        finish()
    }

    /// The current match's marks (highlight annotations drawn over the page while it is current;
    /// the PDF is never saved, and Lunta's own highlights are told apart by their "lunta:" name).
    private var currentMarks: [(PDFPage, PDFAnnotation)] = []

    private func paint() {
        for (page, a) in currentMarks { page.removeAnnotation(a) }
        currentMarks = []
        guard let v = view else { return }
        if matches.isEmpty { v.highlightedSelections = nil; return }
        let other = FindColor(red: 1, green: 0.72, blue: 0.1, alpha: 0.35)
        for s in matches { s.color = other }
        // (the current one is drawn by itself, below, not under the others' colour)
        v.highlightedSelections = matches.enumerated().filter { $0.offset != cur }.map(\.element)
        guard matches.indices.contains(cur) else { return }
        let current = FindColor(red: 1, green: 0.55, blue: 0, alpha: 0.7)
        for line in matches[cur].selectionsByLine() {
            for page in line.pages {
                let b = line.bounds(for: page)
                guard b.width > 0.5, b.height > 0.5 else { continue }
                let a = PDFAnnotation(bounds: b.insetBy(dx: -1, dy: -1), forType: .highlight, withProperties: nil)
                a.color = current
                a.userName = "lunta-find"
                page.addAnnotation(a)
                currentMarks.append((page, a))
            }
        }
    }

    /// The current match about a third of the way down the view (left alone when already in view).
    private func reveal() {
        guard let v = view, matches.indices.contains(cur) else { return }
        let sel = matches[cur]
        guard let page = sel.pages.first else { v.go(to: sel); return }
        let b = sel.bounds(for: page)
        let r = v.convert(b, from: page)                 // in the view's coordinates
        let visible = v.bounds
        #if os(macOS)
        let top = v.isFlipped ? r.minY : visible.height - r.maxY   // distance from the view's top
        #else
        let top = r.minY
        #endif
        if top > visible.height * 0.12, top + r.height < visible.height * 0.8, r.minX >= 0, r.maxX <= visible.width { return }
        let scale = max(v.scaleFactor, 0.01)
        let above = visible.height / scale * 0.3         // page units above the match
        v.go(to: PDFDestination(page: page, at: CGPoint(x: max(0, b.minX - 20), y: b.maxY + above)))
    }
}

// MARK: - The find bar's state

@MainActor
final class PaperFind: ObservableObject {
    @Published private(set) var shown = false
    @Published var query = "" { didSet { if query != oldValue, !quiet { schedule() } } }
    @Published private(set) var result = FindResult()
    @Published private(set) var busy = false
    /// Bumped to put the cursor in the field.
    @Published private(set) var focusRequest = 0
    /// The text field has the keyboard focus (Return and Escape then belong to the bar).
    @Published var fieldFocused = false
    /// After closing: the place the reader was at when finding began, offered for a few seconds.
    @Published private(set) var returnOffer: FindPlace?
    @Published private(set) var returnSerial = 0

    private(set) weak var backend: FindBackend?
    private var task: Task<Void, Never>?
    private var start: FindPlace?
    private var quiet = false
    /// A query from the library's search, run once the page is at its place.
    private var prefilled = false

    var canStep: Bool { result.count > 0 }
    private var trimmed: String { query.trimmingCharacters(in: .whitespacesAndNewlines) }

    var status: String? {
        if trimmed.count < 2 { return nil }
        if busy && result.count == 0 { return "…" }
        if result.count == 0 { return "No matches" }
        return "\(result.index + 1) of \(result.count)\(result.capped ? "+" : "")"
    }

    /// The reader's current view to search (the reading view or the PDF); the query runs again in it.
    func use(_ b: FindBackend?) {
        guard b !== backend else { return }
        backend?.findClear()
        backend = b
        result = FindResult()
        start = nil
        if shown, trimmed.count >= 2 { run(go: true) }
    }

    func open() {
        returnOffer = nil
        if !shown {
            shown = true
            Task { start = await backend?.findPlace() }
            if trimmed.count >= 2 { run(go: true) }
        }
        focusRequest += 1
    }

    func close() {
        guard shown else { return }
        shown = false
        task?.cancel()
        busy = false
        prefilled = false
        let found = result.count > 0
        backend?.findClear()
        result = FindResult()
        fieldFocused = false
        // the reader stays where finding took it; the place before is offered for a moment
        let from = start
        start = nil
        guard found, let from, let b = backend else { return }
        Task {
            guard let now = await b.findPlace(), now.differs(from: from), !shown else { return }
            returnOffer = from
            returnSerial += 1
            let serial = returnSerial
            try? await Task.sleep(for: .seconds(6))
            if returnSerial == serial { withAnimation { returnOffer = nil } }
        }
    }

    func goBack() {
        returnOffer?.restore()
        withAnimation { returnOffer = nil }
    }

    func dismissReturnOffer() { returnOffer = nil }

    func next() { Task { await step(true) } }
    func previous() { Task { await step(false) } }

    func step(_ forward: Bool) async {
        if !shown { open() }
        guard let b = backend else { return }
        if result.count == 0 { if trimmed.count >= 2 { await search(go: true) }; return }
        result = await b.findStep(forward)
    }

    private func schedule() {
        task?.cancel()
        prefilled = false
        let q = trimmed
        task = Task {
            if q.count >= 2 { try? await Task.sleep(for: .milliseconds(220)) }
            guard !Task.isCancelled else { return }
            await search(go: true)
        }
    }

    private func run(go: Bool, fromTop: Bool = false) {
        task?.cancel()
        task = Task { await search(go: go, fromTop: fromTop) }
    }

    /// Searches now (the query as it is).
    func search(go: Bool = true, fromTop: Bool = false) async {
        guard shown, let b = backend else { return }
        let q = trimmed
        if q.count < 2 { _ = await b.find("", fromTop: false); if !Task.isCancelled { result = FindResult(); busy = false }; return }
        busy = true
        let r = await b.find(q, fromTop: fromTop)
        guard !Task.isCancelled, trimmed == q else { return }
        result = r
        busy = false
    }

    // MARK: from the library's search

    /// Opened from a library search that found this paper: the bar opens with the search once the
    /// page is ready (`pageSettled`).
    func prefill(_ libraryQuery: String) {
        let q = libraryQuery.replacingOccurrences(of: "\"", with: "").replacingOccurrences(of: "*", with: "")
            .trimmingCharacters(in: .whitespacesAndNewlines)
        guard q.count >= 2 else { return }
        quiet = true; query = q; quiet = false
        prefilled = true
    }

    /// The page is at its place: run a query from the library, from the paper's start. A search of
    /// several words that isn't in the paper as a phrase looks for its longest word instead.
    func pageSettled() {
        guard prefilled, !shown, backend != nil else { return }     // (nothing to search: no reading view or PDF)
        prefilled = false
        shown = true
        task?.cancel()
        task = Task {
            start = await backend?.findPlace()
            await search(go: true, fromTop: true)
            #if DEBUG
            if ProcessInfo.processInfo.arguments.contains("-searchLibrary") {
                let at = await backend?.findPlace()?.key ?? -1
                print("FIND prefill '\(query)': \(result.index + 1) of \(result.count), from \(start?.key ?? -1) to \(at)")
                try? await Task.sleep(for: .seconds(2))
                print("FIND prefill 2s later: \(await backend?.findPlace()?.key ?? -1)"); fflush(stdout)
            }
            #endif
            guard !Task.isCancelled, result.count == 0 else { return }
            let words = trimmed.split(whereSeparator: { $0.isWhitespace || $0 == "," }).map(String.init)
            guard words.count > 1, let longest = words.max(by: { $0.count < $1.count }), longest.count >= 3 else { return }
            quiet = true; query = longest; quiet = false
            await search(go: true, fromTop: true)
        }
    }

    /// The paper the library's search found it in, with a query to look for (nil: opened otherwise).
    static func libraryQuery(for paperID: String, in model: LibraryModel) -> String? {
        let q = model.query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard q.count >= 2, model.snippets[paperID] != nil, let p = model.paper(paperID) else { return nil }
        // Only when the search found the paper by its text. A search that matches the paper's
        // details (its title "scSLIDE…", an author, the journal, a DOI) was a search *for* the paper,
        // not for something in it: it isn't carried into the reader.
        func fold(_ s: String) -> String { s.folding(options: [.caseInsensitive, .diacriticInsensitive], locale: nil) }
        let details = fold([p.meta.title, p.meta.authors.joined(separator: " "), p.meta.journal, p.meta.doi,
                            p.meta.pmid, p.meta.pmcid, p.meta.arxiv, p.meta.year].joined(separator: " "))
        let words = fold(q).replacingOccurrences(of: "\"", with: "").split(whereSeparator: { $0.isWhitespace })
        if !words.isEmpty, words.allSatisfy({ details.contains($0.trimmingCharacters(in: .punctuationCharacters)) }) { return nil }
        return q
    }
}

// MARK: - The bar

/// The find bar, the same for the reading view and the PDF. `theme`: the reading view's (nil in the PDF).
struct FindBar: View {
    @ObservedObject var find: PaperFind
    var theme: ReaderTheme?
    @FocusState private var focused: Bool
    @Environment(\.colorScheme) private var systemScheme

    var body: some View {
        let scheme = theme?.scheme ?? systemScheme
        HStack(spacing: 6) {
            Image(systemName: "magnifyingglass").foregroundStyle(.secondary).imageScale(.small)
            TextField("Find in Paper", text: $find.query)
                .textFieldStyle(.plain)
                .focused($focused)
                .autocorrectionDisabled()
                #if os(iOS)
                .textInputAutocapitalization(.never)
                .submitLabel(.search)
                #endif
                .onSubmit {
                    find.next()
                    #if os(iOS)
                    // iPhone: Return puts the keyboard away so the match can be seen; a hardware keyboard keeps typing
                    if UIDevice.current.userInterfaceIdiom == .pad { focused = true }
                    #else
                    focused = true
                    #endif
                }
                #if os(iOS)
                .onKeyPress(.escape) { find.close(); return .handled }
                .onKeyPress(keys: [.return]) { k in
                    guard k.modifiers.contains(.shift) else { return .ignored }
                    find.previous(); return .handled
                }
                #else
                .onExitCommand { find.close() }
                #endif
                .accessibilityLabel("Find in paper")
            if let s = find.status {
                Text(s).font(.callout.monospacedDigit()).foregroundStyle(.secondary).lineLimit(1).fixedSize()
                    .accessibilityLabel(find.result.count == 0 ? "No matches" : "Match \(find.result.index + 1) of \(find.result.count)")
            }
            Divider().frame(height: 18)
            Button { find.previous() } label: { Image(systemName: "chevron.up") }
                .disabled(!find.canStep).help("Previous match (⇧⌘G)").accessibilityLabel("Previous match")
            Button { find.next() } label: { Image(systemName: "chevron.down") }
                .disabled(!find.canStep).help("Next match (⌘G)").accessibilityLabel("Next match")
            Button("Done") { find.close() }.fontWeight(.medium)
        }
        .buttonStyle(.borderless)
        .padding(.leading, 12).padding(.trailing, 10).padding(.vertical, 8)
        .frame(maxWidth: 520)
        .background {
            let shape = RoundedRectangle(cornerRadius: 12, style: .continuous)
            if let page = theme?.page {
                shape.fill(page).overlay(shape.strokeBorder(theme!.text.opacity(0.18)))
            } else {
                shape.fill(.regularMaterial).overlay(shape.strokeBorder(.primary.opacity(0.08)))
            }
        }
        .foregroundStyle(theme.map { $0.page == nil ? Color.primary : $0.text } ?? .primary)
        .shadow(color: .black.opacity(0.12), radius: 6, y: 2)
        .environment(\.colorScheme, scheme)
        .padding(.horizontal, 12)
        .onAppear { if find.focusRequest > 0 { focused = true } }
        .onChange(of: find.focusRequest) { focused = true }
        .onChange(of: focused) { _, f in find.fieldFocused = f }
    }
}

/// The find bar and the way back, over the top of the reader.
struct FindOverlay: ViewModifier {
    @ObservedObject var find: PaperFind
    var theme: ReaderTheme?
    func body(content: Content) -> some View {
        content.overlay(alignment: .top) {
            VStack(spacing: 0) {
                if find.shown {
                    FindBar(find: find, theme: theme)
                        .padding(.top, 8)
                        .transition(.move(edge: .top).combined(with: .opacity))
                }
                FindReturnNote(find: find)
            }
            .animation(.easeOut(duration: 0.18), value: find.shown)
            .animation(.easeOut, value: find.returnOffer != nil)
        }
    }
}

/// After closing the find bar somewhere else in the paper: a way back, for a few seconds.
struct FindReturnNote: View {
    @ObservedObject var find: PaperFind
    var body: some View {
        if find.returnOffer != nil {
            Button { find.goBack() } label: {
                Label("Back to where you were", systemImage: "arrow.uturn.backward")
                    .font(.footnote.weight(.medium))
                    .padding(.horizontal, 14).padding(.vertical, 8)
                    .background(.regularMaterial, in: Capsule())
            }
            .buttonStyle(.plain)
            .padding(.top, 10)
            .transition(.move(edge: .top).combined(with: .opacity))
        }
    }
}

// MARK: - Mac: keys and the Edit menu

struct PaperFindKey: FocusedValueKey { typealias Value = PaperFind }
extension FocusedValues {
    var paperFind: PaperFind? {
        get { self[PaperFindKey.self] }
        set { self[PaperFindKey.self] = newValue }
    }
}

#if os(macOS)
/// A Mac reader window's side of finding: its keys, the Edit menu's commands, the view being
/// searched (`backend` follows the reading view / PDF switch), and the keys back to the page
/// (`webView`: arrow keys turn pages) when the bar closes.
struct ReaderFindHooks: ViewModifier {
    @ObservedObject var find: PaperFind
    var backend: FindBackend?
    weak var webView: WKWebView?
    func body(content: Content) -> some View {
        content
            .background(ReaderFindKeys(find: find))
            .focusedSceneValue(\.paperFind, find)
            .onChange(of: backend.map(ObjectIdentifier.init)) { find.use(backend) }
            .onChange(of: find.shown) { _, shown in
                if !shown, let v = webView { v.window?.makeFirstResponder(v) }
            }
    }
}

/// Find commands for the Edit menu; enabled (with their keys) in a reader window.
struct PaperFindCommands: Commands {
    @FocusedValue(\.paperFind) private var find
    var body: some Commands {
        CommandGroup(after: .textEditing) {
            Divider()
            Button("Find in Paper…") { find?.open() }
                .keyboardShortcut(find != nil ? KeyboardShortcut("f", modifiers: .command) : nil)
                .disabled(find == nil)
            Button("Find Next in Paper") { find?.next() }
                .keyboardShortcut(find != nil ? KeyboardShortcut("g", modifiers: .command) : nil)
                .disabled(find == nil)
            Button("Find Previous in Paper") { find?.previous() }
                .keyboardShortcut(find != nil ? KeyboardShortcut("g", modifiers: [.command, .shift]) : nil)
                .disabled(find == nil)
        }
    }
}

/// ⌘F, ⌘G and ⇧⌘G in a reader window, and Return, ⇧Return and Escape in its find field. Caught
/// before the window handles them (the web view and the system's Find menu would take them),
/// as ReaderUndoKeys does for undo.
struct ReaderFindKeys: NSViewRepresentable {
    let find: PaperFind

    func makeNSView(context: Context) -> KeyView { let v = KeyView(); v.find = find; return v }
    func updateNSView(_ v: KeyView, context: Context) { v.find = find }

    final class KeyView: NSView {
        weak var find: PaperFind?
        private var monitor: Any?

        override func hitTest(_ point: NSPoint) -> NSView? { nil }

        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            if let monitor { NSEvent.removeMonitor(monitor); self.monitor = nil }
            guard window != nil else { return }
            monitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] e in
                guard let self, let win = self.window, e.window === win, let find = self.find else { return e }
                let mods = e.modifierFlags.intersection([.command, .shift, .option, .control])
                let key = e.charactersIgnoringModifiers?.lowercased()
                return MainActor.assumeIsolated {
                    if mods == .command, key == "f" { find.open(); return nil }
                    if key == "g", mods == .command || mods == [.command, .shift], find.shown || !find.query.isEmpty {
                        if mods == .command { find.next() } else { find.previous() }
                        return nil
                    }
                    guard find.shown, find.fieldFocused else { return e }
                    if e.keyCode == 36 || e.keyCode == 76 {          // Return, Enter
                        guard mods.isEmpty || mods == .shift else { return e }
                        if mods == .shift { find.previous() } else { find.next() }
                        return nil
                    }
                    if e.keyCode == 53, mods.isEmpty { find.close(); return nil }    // Escape
                    return e
                }
            }
        }

        deinit { if let monitor { NSEvent.removeMonitor(monitor) } }
    }
}
#endif

#if DEBUG
extension PaperFind {
    /// Simulator test: -findTest <query> searches the open paper (-findPDF: in the PDF) once it has
    /// settled, then steps forward and back, logging the match and where the reader is; -findClose
    /// closes the bar at the end (the highlights must still be drawn).
    func selfTest(reader: @escaping @MainActor () async -> String, annotations: @escaping @MainActor () async -> String) async {
        let args = ProcessInfo.processInfo.arguments
        guard let i = args.firstIndex(of: "-findTest"), args.indices.contains(i + 1) else { return }
        func log(_ s: String) { print("FIND " + s); fflush(stdout) }
        try? await Task.sleep(for: .seconds(7))     // past the restored place's hold
        log("before: \(await reader()) \(await annotations())")
        quiet = true; query = args[i + 1]; quiet = false
        shown = true
        start = await backend?.findPlace()
        await search(go: true)
        try? await Task.sleep(for: .milliseconds(600))
        log("find '\(query)': \(result.index + 1) of \(result.count) | \(await reader())")
        for k in 0..<3 {
            await step(true); try? await Task.sleep(for: .milliseconds(600))
            log("next \(k + 1): \(result.index + 1) of \(result.count) | \(await reader())")
        }
        for k in 0..<2 {
            await step(false); try? await Task.sleep(for: .milliseconds(600))
            log("prev \(k + 1): \(result.index + 1) of \(result.count) | \(await reader())")
        }
        if args.contains("-findWrap") {
            for _ in 0..<(result.index + 1) { await step(false) }
            try? await Task.sleep(for: .milliseconds(600))
            log("wrapped back: \(result.index + 1) of \(result.count) | \(await reader())")
        }
        if args.contains("-findClose") {
            try? await Task.sleep(for: .seconds(3))
            close()
            try? await Task.sleep(for: .milliseconds(800))
            log("closed: \(await reader()) \(await annotations()) offer=\(returnOffer != nil)")
        }
    }
}
#endif
