import SwiftUI
import WebKit
import LuntaCore

// Highlights and notes in the reading view. Each one is anchored by its quote plus a little
// text before and after (and its character offset as a tie-breaker), so it finds its place
// again on any device even if the page's element ids differ. Stored in the core's op log, so
// they sync through iCloud and agents (MCP) can read them.

enum HighlightColor: String, CaseIterable, Identifiable {
    case yellow, green, blue, pink
    var id: String { rawValue }
    var color: Color {
        switch self {
        case .yellow: return Color(red: 0.98, green: 0.83, blue: 0.30)
        case .green: return Color(red: 0.55, green: 0.82, blue: 0.48)
        case .blue: return Color(red: 0.48, green: 0.72, blue: 0.95)
        case .pink: return Color(red: 0.96, green: 0.56, blue: 0.70)
        }
    }
    var name: String { rawValue.capitalized }
}

/// What the user selected in the page.
struct PageSelection: Codable {
    let quote: String
    let prefix: String
    let suffix: String
    let start: Int
}

extension ReaderAssets {
    static let annotationCSS = """
    mark.m-hl { color: inherit; border-radius: 2px; padding: 0 1px; cursor: pointer; }
    mark.m-yellow { background: rgba(250,212,77,.45); }
    mark.m-green { background: rgba(140,209,122,.45); }
    mark.m-blue { background: rgba(122,184,242,.42); }
    mark.m-pink { background: rgba(245,143,179,.42); }
    mark.m-note { border-bottom: 2px dotted rgba(200,120,40,.9); }
    html.m-hlmode[data-hlcolor="yellow"] ::selection { background: rgba(250,212,77,.55); }
    html.m-hlmode[data-hlcolor="green"] ::selection { background: rgba(140,209,122,.55); }
    html.m-hlmode[data-hlcolor="blue"] ::selection { background: rgba(122,184,242,.5); }
    html.m-hlmode[data-hlcolor="pink"] ::selection { background: rgba(245,143,179,.5); }
    mark.m-hl.m-flash { box-shadow: 0 0 0 4px rgba(242,180,90,.55); }
    @media (prefers-color-scheme: dark) {
      mark.m-yellow { background: rgba(250,212,77,.30); } mark.m-green { background: rgba(140,209,122,.30); }
      mark.m-blue { background: rgba(122,184,242,.30); } mark.m-pink { background: rgba(245,143,179,.30); }
    }
    """

    static let annotationJS = #"""
    (function(){
      var st = document.createElement('style'); st.textContent = `CSS`; document.documentElement.appendChild(st);
      // A book read chapter by chapter (ChapterReader.swift) shows one part of its reading view per
      // page; the part says where its text starts in the whole view's text. Offsets the app gets
      // and gives (a highlight's start) are in the whole view's text, so they are the same in the
      // whole book on the Mac and in one chapter on the iPhone. 0 for a paper.
      function base(){
        var m = document.querySelector('meta[name="lunta-text-base"]');
        return m ? (parseInt(m.getAttribute('content'), 10) || 0) : 0;
      }
      function textNodes(){
        var out = [], w = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT, { acceptNode: function(n){
          var p = n.parentNode && n.parentNode.nodeName.toLowerCase();
          return (p === 'script' || p === 'style') ? NodeFilter.FILTER_REJECT : NodeFilter.FILTER_ACCEPT; } });
        var n; while ((n = w.nextNode())) out.push(n);
        return out;
      }
      function fullText(nodes){ var s = ''; for (var i = 0; i < nodes.length; i++) s += nodes[i].nodeValue; return s; }
      function offsetOf(nodes, node, off){
        var pos = 0;
        for (var i = 0; i < nodes.length; i++) { if (nodes[i] === node) return pos + off; pos += nodes[i].nodeValue.length; }
        return -1;
      }
      // a point in the page (a range's start or end) as an offset in the page text: in a text node,
      // or between elements (a selection that ends at a paragraph's or a figure's edge): the first
      // text after that point
      function pointOffset(nodes, node, off){
        if (node.nodeType === 3) return offsetOf(nodes, node, off);
        var ref = node.childNodes[off], pos = 0;
        for (var i = 0; i < nodes.length; i++) {
          var n = nodes[i];
          if (ref ? (n === ref || (ref.compareDocumentPosition(n) & 4))
                  : ((node.compareDocumentPosition(n) & 20) === 4)) return pos;   // after the node, not in it
          pos += n.nodeValue.length;
        }
        return pos;
      }
      window.luntaSelection = function(){
        var sel = window.getSelection(); if (!sel || sel.rangeCount === 0 || sel.isCollapsed) return '';
        var r = sel.getRangeAt(0), nodes = textNodes();
        var s = pointOffset(nodes, r.startContainer, r.startOffset);
        var e = pointOffset(nodes, r.endContainer, r.endOffset);
        var text = fullText(nodes);
        if (s < 0 || e < 0) { var q0 = sel.toString(); s = text.indexOf(q0); e = s + q0.length; if (s < 0) return ''; }
        var quote = text.slice(s, e);
        if (!quote.trim()) return '';
        return JSON.stringify({ quote: quote, prefix: text.slice(Math.max(0, s - 40), s), suffix: text.slice(e, e + 40), start: s + base() });
      };
      function unwrapAll(){
        document.querySelectorAll('mark.m-hl').forEach(function(m){
          var p = m.parentNode; while (m.firstChild) p.insertBefore(m.firstChild, m); p.removeChild(m); p.normalize();
        });
      }
      var lcache = { text: null };
      function locate(text, a){
        if (!a.quote || !a.quote.trim()) return null;   // a note without a passage: nothing to mark (and indexOf('') never ends)
        var best = -1, bestScore = -1, i = text.indexOf(a.quote), hint = (a.start || 0) - base();
        while (i >= 0) {
          var score = 0;
          if (a.prefix && text.slice(Math.max(0, i - a.prefix.length), i) === a.prefix) score += 2;
          if (a.suffix && text.slice(i + a.quote.length, i + a.quote.length + a.suffix.length) === a.suffix) score += 2;
          score -= Math.min(1, Math.abs(i - hint) / 100000);
          if (score > bestScore) { bestScore = score; best = i; }
          i = text.indexOf(a.quote, i + 1);
        }
        if (best < 0) {
          // whitespace may differ between devices: match ignoring it
          var squash = function(s){ return s.replace(/\s+/g, ' '); };
          var q = squash(a.quote).trim(); if (q.length < 4) return null;
          // the page text's squashed and dense forms are built once per text (all highlights share them)
          if (lcache.text !== text) lcache = { text: text };
          if (!lcache.flat) {
            var map = [], flat = '';
            for (var k = 0; k < text.length; k++) {
              var c = text[k];
              if (/\s/.test(c)) { if (flat.length && flat[flat.length - 1] === ' ') continue; c = ' '; }
              map.push(k); flat += c;
            }
            lcache.flat = flat; lcache.map = map;
          }
          var map = lcache.map, flat = lcache.flat;
          var j = flat.indexOf(q);
          if (j >= 0) return [map[j], map[j + q.length - 1] + 1];
          // made in the PDF view: spacing ("(Fig. 2)" vs "( Fig. 2 )"), dashes, quotes and µ differ;
          // match with all of that left out
          var foldc = function(c){
            if (c === '\u00B5') return '\u03BC';
            if (/[\u2018\u2019]/.test(c)) return "'";
            if (/[\u201C\u201D]/.test(c)) return '"';
            if (/[\u2010-\u2014\u2212]/.test(c)) return '-';
            return c.toLowerCase();
          };
          // quote marks are left out too: a PDF's ‘‘x’’ is the web's “x”
          var skip = /[\s'"\u2018\u2019\u201A\u201B\u201C\u201D\u201E\u201F\u00B4\u0060\u00AB\u00BB]/;
          var dq = ''; for (var u = 0; u < a.quote.length; u++) { if (!skip.test(a.quote[u])) dq += foldc(a.quote[u]); }
          if (dq.length < 4) return null;
          if (!lcache.dense) {
            var dm = [], dn = [];
            for (var k2 = 0; k2 < text.length; k2++) { if (!skip.test(text[k2])) { dm.push(k2); dn.push(foldc(text[k2])); } }
            lcache.dense = dn.join(''); lcache.dmap = dm;
          }
          var dmap = lcache.dmap, dense = lcache.dense;
          var d = dense.indexOf(dq);
          var whole = dq;
          if (d < 0 && dq.length > 140) { dq = dq.slice(0, 100); d = dense.indexOf(dq); }
          if (d < 0) {
            dq = whole;
            // the text changed inside the passage (e.g. a PDF that ran two columns together, now
            // rebuilt from a better source): its longest surviving start and end, close together
            var pre = -1, preLen = 0, suf = -1, sufLen = 0;
            for (var L = Math.min(dq.length - 1, 120); L >= 30; L -= 5) { var x = dense.indexOf(dq.slice(0, L)); if (x >= 0) { pre = x; preLen = L; break; } }
            if (pre < 0) return null;
            for (var L2 = Math.min(dq.length - 1, 120); L2 >= 30; L2 -= 5) { var y = dense.indexOf(dq.slice(dq.length - L2), pre + preLen); if (y >= 0) { suf = y; sufLen = L2; break; } }
            if (suf < 0 || suf + sufLen - pre > dq.length * 2) return null;
            return [dmap[pre], dmap[suf + sufLen - 1] + 1];
          }
          return [dmap[d], dmap[d + dq.length - 1] + 1];
        }
        return [best, best + a.quote.length];
      }
      function wrap(nodes, s, e, a){
        var pos = 0, first = null;
        for (var i = 0; i < nodes.length; i++) {
          var n = nodes[i], len = n.nodeValue.length, ns = pos, ne = pos + len; pos = ne;
          if (ne <= s || ns >= e) continue;
          var from = Math.max(0, s - ns), to = Math.min(len, e - ns);
          if (!n.nodeValue.slice(from, to).trim()) continue;
          var target = n;
          if (from > 0) target = target.splitText(from);
          if (to - from < target.nodeValue.length) target.splitText(to - from);
          var m = document.createElement('mark');
          m.className = 'm-hl m-' + (a.color || 'yellow') + (a.note ? ' m-note' : '');
          m.setAttribute('data-ann', a.id);
          if (a.note) m.title = a.note;
          target.parentNode.insertBefore(m, target); m.appendChild(target);
          if (!first) first = m;
        }
        return first;
      }
      window.luntaApplyAnnotations = function(json){
        unwrapAll();
        var list = JSON.parse(json), missing = [], found = [];
        // marks add no text, so every highlight is found in the same page text (built once) …
        var text = fullText(textNodes());
        for (var i = 0; i < list.length; i++) {
          var at = locate(text, list[i]);
          if (at) found.push([at[0], at[1], list[i]]); else missing.push(list[i].id);
        }
        // … then drawn last first (splitting text nodes keeps the earlier offsets valid)
        found.sort(function(x, y){ return y[0] - x[0]; });
        for (var j = 0; j < found.length; j++) {
          if (!wrap(textNodes(), found[j][0], found[j][1], found[j][2])) missing.push(found[j][2].id);
        }
        return JSON.stringify(missing);
      };
      window.luntaShowAnnotation = function(id){
        var m = document.querySelector('mark[data-ann="' + id + '"]'); if (!m) return false;
        if (window.luntaMarkJump) window.luntaMarkJump();
        m.scrollIntoView({ behavior: 'smooth', block: 'center' });
        document.querySelectorAll('mark[data-ann="' + id + '"]').forEach(function(x){
          x.classList.add('m-flash'); setTimeout(function(){ x.classList.remove('m-flash'); }, 1800); });
        return true;
      };
      document.addEventListener('click', function(ev){
        if (ext || document.body.classList.contains('listening')) return;   // extending: the tap ends the passage (below)
        var sel = window.getSelection(); if (sel && !sel.isCollapsed) return;
        var m = ev.target.closest && ev.target.closest('mark.m-hl'); if (!m) return;
        try { window.webkit.messageHandlers.ann.postMessage(m.getAttribute('data-ann')); } catch (x) {}
        ev.preventDefault(); ev.stopPropagation();
      }, true);
      // highlight mode: a finished selection is highlighted at once (the app decides the colour).
      // Mac: when the mouse is released. iPhone: when the selection has stopped changing for a
      // moment (handles can be dragged, so there is no "done" event).
      window.luntaHLMode = false;
      var hlTimer = null;
      function hlFire(){
        if (!window.luntaHLMode || document.body.classList.contains('listening')) return;
        // a selection handle still being dragged (the app says so in pages, PageTurn.swift): not finished yet
        if (window.luntaDragging) { clearTimeout(hlTimer); hlTimer = setTimeout(hlFire, 300); return; }
        var s = window.getSelection(); if (!s || s.isCollapsed || s.toString().trim().length < 2) return;
        var j = window.luntaSelection(); if (!j) return;
        // coloured here and now; the app saves it and redraws (which replaces this mark with the real one)
        try {
          var o = JSON.parse(j);
          var s0 = o.start - base();
          if (s0 >= 0) wrap(textNodes(), s0, s0 + o.quote.length, { id: '__pending', color: window.luntaHLColor || 'yellow' });
        } catch (x) {}
        s.removeAllRanges();
        try { window.webkit.messageHandlers.ann.postMessage('__hl__:' + j); } catch (x) {}
      }
      // Apple Pencil (iPad). A stroke along the text highlights from the word under its start to
      // the word under its end; points are in the viewport (page pixels). The result is what
      // luntaSelection gives for a selection of that text.
      function selectionJSON(text, s, e){
        while (s < e && /\s/.test(text[s])) s++;
        while (e > s && /\s/.test(text[e - 1])) e--;
        if (e <= s) return '';
        return JSON.stringify({ quote: text.slice(s, e), prefix: text.slice(Math.max(0, s - 40), s), suffix: text.slice(e, e + 40), start: s + base() });
      }
      // the page text from s to e (offsets in this page's text) as a selection of it: the figure viewer's
      // caption (FigureViewer.swift) gives the passage selected there so
      window.luntaPassage = function(s, e){ return selectionJSON(fullText(textNodes()), s, e); };
      function caretOffset(nodes, x, y){
        var r = document.caretRangeFromPoint && document.caretRangeFromPoint(x, y);
        if (!r) return -1;
        // between blocks or beside an image: the first text after that point
        return pointOffset(nodes, r.startContainer, r.startOffset);
      }
      var wordChar = /[\p{L}\p{N}\p{M}_'’\-]/u;
      window.luntaSelectionBetween = function(x1, y1, x2, y2){
        var nodes = textNodes(), text = fullText(nodes);
        var a = caretOffset(nodes, x1, y1), b = caretOffset(nodes, x2, y2);
        if (a < 0 || b < 0) return '';
        var s = Math.min(a, b), e = Math.max(a, b);
        // whole words at both ends
        if (s < text.length && wordChar.test(text[s])) { while (s > 0 && wordChar.test(text[s - 1])) s--; }
        if (e > 0 && wordChar.test(text[e - 1])) { while (e < text.length && wordChar.test(text[e])) e++; }
        return selectionJSON(text, s, e);
      };
      // the stroke's passage coloured at once (as in highlight mode); the app saves it and redraws
      window.luntaPencilHighlight = function(x1, y1, x2, y2, color){
        if (document.body.classList.contains('listening')) return '';
        var j = window.luntaSelectionBetween(x1, y1, x2, y2); if (!j) return '';
        try { var o = JSON.parse(j), s0 = o.start - base(); wrap(textNodes(), s0, s0 + o.quote.length, { id: '__pending', color: color || 'yellow' }); } catch (x) {}
        return j;
      };
      // A new highlight from s to e: the highlights already drawn that it overlaps or touches, and
      // the passage covering all of them (so they become one highlight instead of one replacing
      // or stacking on another). {ids: [...], quote, prefix, suffix, start}
      // (`exclude`: the highlight being extended, which takes the new passage as it is, so it can also shrink)
      window.luntaMergeRange = function(s, e, exclude){
        s -= base(); e -= base();
        var nodes = textNodes(), text = fullText(nodes), spans = {};
        document.querySelectorAll('mark.m-hl[data-ann]').forEach(function(m){
          var id = m.getAttribute('data-ann'); if (!id || id === '__pending' || id === exclude) return;
          var t = m.firstChild; if (!t || t.nodeType !== 3) return;
          var a = offsetOf(nodes, t, 0); if (a < 0) return;
          var b = a + t.nodeValue.length, sp = spans[id];
          spans[id] = sp ? [Math.min(sp[0], a), Math.max(sp[1], b)] : [a, b];
        });
        var us = s, ue = e, ids = [], grew = true;
        while (grew) {
          grew = false;
          for (var id in spans) {
            if (ids.indexOf(id) >= 0) continue;
            var sp = spans[id];
            // overlapping, or touching (only whitespace between)
            var gapOK = (sp[1] >= us && sp[0] <= ue) ||
                        (sp[1] < us && !text.slice(sp[1], us).trim()) || (sp[0] > ue && !text.slice(ue, sp[0]).trim());
            if (gapOK) { ids.push(id); us = Math.min(us, sp[0]); ue = Math.max(ue, sp[1]); grew = true; }
          }
        }
        var j = selectionJSON(text, us, ue); if (!j) return '';
        var o = JSON.parse(j); o.ids = ids;
        return JSON.stringify(o);
      };
      // a note on the paragraph at a point: anchored to its first sentence (or ~120 characters)
      window.luntaParagraphAt = function(x, y){
        var el = document.elementFromPoint(x, y);
        var block = el && el.closest && el.closest('p, li, blockquote, figcaption, dd, td, h1, h2, h3, h4, h5, h6');
        if (!block) return '';
        var nodes = textNodes(), text = fullText(nodes), s = -1, e = -1, pos = 0;
        for (var i = 0; i < nodes.length; i++) {
          var len = nodes[i].nodeValue.length;
          if (block.contains(nodes[i])) { if (s < 0) s = pos; e = pos + len; }
          pos += len;
        }
        if (s < 0) return '';
        var para = text.slice(s, e), lead = para.length - para.replace(/^\s+/, '').length;
        s += lead; para = para.slice(lead);
        var m = para.match(/^[\s\S]{20,}?[.!?](?=\s|$)/), q;
        if (m && m[0].length <= 160) q = m[0];
        else { q = para.slice(0, 120); if (para.length > 120) { var cut = q.lastIndexOf(' '); if (cut > 60) q = q.slice(0, cut); } }
        return selectionJSON(text, s, s + q.length);
      };

      // Extend… (a highlight's editor) and Highlight to… (the selection menu): the passage — a
      // highlight, or the selection — runs on to the word tapped next, on this page or another
      // one (turned meanwhile: a swipe in pages, scrolling otherwise), so a passage that runs over
      // the page's end becomes one highlight. Tapping before its start moves the start instead;
      // inside it, the end comes back to that word. The app shows the hint, and saves what the
      // page sends ('__extend__:' + the passage, as luntaSelection gives it, with the highlight's id).
      var ext = null;
      // where a highlight is in the page text (all its marks: it may run over paragraphs)
      function annSpan(nodes, id){
        var pos = 0, s = -1, e = -1;
        for (var i = 0; i < nodes.length; i++) {
          var len = nodes[i].nodeValue.length;
          for (var p = nodes[i].parentNode; p && p.nodeType === 1; p = p.parentNode) {
            if (p.getAttribute('data-ann') === id) { if (s < 0) s = pos; e = pos + len; break; }
          }
          pos += len;
        }
        return s < 0 ? null : [s, e];
      }
      window.luntaExtendStart = function(id, color){
        var nodes = textNodes(), span = null;
        if (id) span = annSpan(nodes, id);
        else {
          var j = window.luntaSelection();
          if (j) { var o = JSON.parse(j), s0 = o.start - base(); if (s0 >= 0) span = [s0, s0 + o.quote.length]; }
        }
        if (!span) return false;
        ext = { id: id || '', s: span[0], e: span[1] };
        if (!id) {
          // the selection goes (its menu would cover the page); its passage stays coloured meanwhile
          var sl = window.getSelection(); if (sl) sl.removeAllRanges();
          wrap(nodes, span[0], span[1], { id: '__pending', color: color || 'yellow' });
        }
        document.documentElement.classList.add('m-extending');
        return true;
      };
      window.luntaExtending = function(){ return !!ext; };
      window.luntaExtendCancel = function(){ ext = null; document.documentElement.classList.remove('m-extending'); };
      // the passage when the text at offset `a` is tapped (whole words)
      window.luntaExtendToOffset = function(a){
        if (!ext || a < 0) return '';
        var text = fullText(textNodes()), s = ext.s, e = ext.e;
        if (a >= ext.s) { e = a; while (e < text.length && wordChar.test(text[e])) e++; }
        else { s = a; while (s > 0 && wordChar.test(text[s - 1])) s--; }
        var j = selectionJSON(text, s, e); if (!j) return '';
        var o = JSON.parse(j); o.id = ext.id;
        return JSON.stringify(o);
      };
      window.luntaExtendTo = function(x, y){ return ext ? window.luntaExtendToOffset(caretOffset(textNodes(), x, y)) : ''; };
      // a selection handle dragged (the app says so in pages): a drag that became a pending
      // Highlight to… at the page's edge ends when the finger lifts, which is no tap on the end word
      var dragEndAt = 0;
      window.luntaSetDragging = function(on){ if (window.luntaDragging && !on) dragEndAt = Date.now(); window.luntaDragging = on; };
      // the tap (before links, citations, figures, page turns and the bars take it)
      window.addEventListener('click', function(ev){
        if (!ext) return;
        ev.preventDefault(); ev.stopImmediatePropagation();
        if (window.luntaDragging || Date.now() - dragEndAt < 400) return;
        var j = window.luntaExtendTo(ev.clientX, ev.clientY); if (!j) return;
        window.luntaExtendCancel();
        try { window.webkit.messageHandlers.ann.postMessage('__extend__:' + j); } catch (x) {}
      }, true);
      if ('ontouchstart' in window) {
        document.addEventListener('selectionchange', function(){
          if (!window.luntaHLMode) return;
          clearTimeout(hlTimer); hlTimer = setTimeout(hlFire, 500);
        });
      } else {
        document.addEventListener('mouseup', function(){ if (window.luntaHLMode) setTimeout(hlFire, 0); });
      }
      try { window.webkit.messageHandlers.ann.postMessage('__ready__'); } catch (x) {}
    })();
    """#.replacingOccurrences(of: "CSS", with: annotationCSS.replacingOccurrences(of: "`", with: ""))
}

/// Receives "ann" messages (a tapped highlight's id) from the page.
final class AnnotationMessageHandler: NSObject, WKScriptMessageHandler {
    weak var controller: ReaderController?
    func userContentController(_ c: WKUserContentController, didReceive m: WKScriptMessage) {
        guard let id = m.body as? String else { return }
        Task { @MainActor in
            if id == "__ready__" { self.controller?.onPageReady?() }
            else if id.hasPrefix("__hl__:"), let d = id.dropFirst(7).data(using: .utf8) {
                // highlight mode: the page already coloured the passage; this is what to save
                self.controller?.onSelectionDone?(try? JSONDecoder().decode(PageSelection.self, from: d))
            }
            else if id.hasPrefix("__extendstart__:") {
                // a selection dragged over the page's edge became a pending Highlight to… (PageTurn.swift)
                self.controller?.onExtendBegan?(HighlightColor(rawValue: String(id.dropFirst(16))) ?? .yellow)
            }
            else if id.hasPrefix("__extend__:"), let d = id.dropFirst(11).data(using: .utf8),
                    let sel = try? JSONDecoder().decode(PageSelection.self, from: d) {
                // Extend… / Highlight to…: the word that was tapped ends the passage
                self.controller?.onExtendDone?(sel)
            }
            else { self.controller?.onAnnotation?(id) }
        }
    }
}

extension ReaderController {
    /// The highlights already in the page that a new one over `sel` overlaps or touches, and the
    /// passage covering them all (nil: none, or the page couldn't tell).
    /// `excluding`: a highlight being extended (it isn't merged in, so it can also get shorter).
    func mergeRange(_ sel: PageSelection, excluding: String? = nil) async -> (ids: [String], passage: PageSelection)? {
        let end = sel.start + sel.quote.utf16.count
        let skip = (excluding ?? "").filter { $0.isLetter || $0.isNumber || $0 == "-" }
        guard sel.start >= 0,
              let json = try? await webView?.evaluateJavaScript("window.luntaMergeRange ? luntaMergeRange(\(sel.start), \(end), '\(skip)') : ''") as? String,
              let data = json.data(using: .utf8),
              let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let ids = obj["ids"] as? [String], !ids.isEmpty,
              let passage = try? JSONDecoder().decode(PageSelection.self, from: data) else { return nil }
        return (ids, passage)
    }

    func pageSelection() async -> PageSelection? {
        // a passage the Pencil marked (PencilReader.swift) stands in for the selection, once
        if let (sel, at) = pencilSelection {
            pencilSelection = nil
            if Date().timeIntervalSince(at) < 5 { return sel }
        }
        guard let json = try? await webView?.evaluateJavaScript("luntaSelection()") as? String,
              let data = json.data(using: .utf8) else { return nil }
        return try? JSONDecoder().decode(PageSelection.self, from: data)
    }

    /// Draws the paper's highlights into the page (replacing what's there): those made here and
    /// those made in the PDF, found by their text (a PDF highlight's position is a PDF offset,
    /// so it gives no hint here).
    func showAnnotations(_ all: [Annotation]) {
        // regions marked on figures aren't in the text: their figures' cards show them (FigureMarks.swift)
        showFigureMarkBadges(all)
        // a book read chapter by chapter: the highlights in the part shown; those found in no part
        // are missing (ChapterReader.split)
        var list = all.filter { !$0.isFigureMark }, unplaced = Set<String>()
        if let ch = chapters { (list, unplaced) = ch.split(all) }
        let items: [[String: Any]] = list.map { a in
            ["id": a.id, "quote": a.quote, "prefix": a.prefix, "suffix": a.suffix, "color": a.color, "note": a.note,
             "start": a.location == "epub" ? Int(a.position) ?? 0 : 0]
        }
        guard let data = try? JSONSerialization.data(withJSONObject: items), let json = String(data: data, encoding: .utf8) else { return }
        let arg = json.replacingOccurrences(of: "\\", with: "\\\\").replacingOccurrences(of: "'", with: "\\'")
            .replacingOccurrences(of: "\n", with: "\\n").replacingOccurrences(of: "\u{2028}", with: "\\u2028").replacingOccurrences(of: "\u{2029}", with: "\\u2029")
        // the page answers with the ids it couldn't place in this text
        webView?.evaluateJavaScript("luntaApplyAnnotations('\(arg)')") { [weak self] result, _ in
            guard let self else { return }
            var missing = unplaced
            if let s = result as? String, let d = s.data(using: .utf8), let ids = try? JSONDecoder().decode([String].self, from: d) {
                missing.formUnion(ids)
            }
            if missing != self.missingAnnotations { self.missingAnnotations = missing }
            self.annotationsShown += 1
        }
    }

    /// Tells the citation card which papers are in the library (DOIs, "arxiv:<id>"), so it says
    /// "In library" rather than "Add to Lunta" for those.
    func setLibraryIDs(_ ids: [String]) {
        guard let data = try? JSONSerialization.data(withJSONObject: ids), let json = String(data: data, encoding: .utf8) else { return }
        webView?.evaluateJavaScript("window.luntaSetLibrary && luntaSetLibrary(\(json))")
    }

    func showAnnotation(_ id: String) {
        // a region marked on a figure: the figure viewer, at the mark
        if let model, model.annotations(paperID).contains(where: { $0.id == id && $0.isFigureMark }) {
            showFigureMark(id)
            return
        }
        // a book read chapter by chapter: in its part (opened first)
        if let ch = chapters, let k = ch.part(ofAnnotation: id), k != ch.current {
            ch.go(k, then: .annotation(id))
            return
        }
        let safe = id.filter { $0.isLetter || $0.isNumber || $0 == "-" }
        webView?.evaluateJavaScript("luntaShowAnnotation('\(safe)')")
    }

    /// Highlight mode on or off in the page (call again after the page reloads).
    func setHighlightMode(_ on: Bool, color: HighlightColor = .yellow) {
        webView?.evaluateJavaScript("""
            window.luntaHLMode = \(on); window.luntaHLColor = '\(color.rawValue)';
            document.documentElement.classList.toggle('m-hlmode', \(on));
            document.documentElement.setAttribute('data-hlcolor', '\(color.rawValue)');
            """)
    }

    func clearSelection() {
        webView?.evaluateJavaScript("window.getSelection().removeAllRanges()")
    }

    /// Extend… (a highlight, `id`) or Highlight to… (the selection, `id` nil): the next tap in the
    /// page ends the passage, and `onExtendDone` gets it. False: there was nothing to extend.
    func startExtend(id: String?, color: HighlightColor = .yellow) async -> Bool {
        let safe = (id ?? "").filter { $0.isLetter || $0.isNumber || $0 == "-" }
        let ok = try? await webView?.evaluateJavaScript("window.luntaExtendStart ? luntaExtendStart('\(safe)', '\(color.rawValue)') : false") as? Bool
        return ok == true
    }

    func endExtend() {
        webView?.evaluateJavaScript("window.luntaExtendCancel && luntaExtendCancel()")
    }
}

extension LibraryModel {
    func annotations(_ paper: String) -> [Annotation] { lib.annotations(paper: paper) }

    @discardableResult
    func addHighlight(_ paper: String, _ s: PageSelection, color: HighlightColor = .yellow, note: String = "",
                      location: String = "epub", undo: UndoManager? = nil) -> Annotation? {
        let a = Annotation(id: "", paperId: paper, kind: note.isEmpty ? .highlight : .note, color: color.rawValue,
                           location: location, position: String(s.start), quote: s.quote, prefix: s.prefix, suffix: s.suffix,
                           note: note, created: 0, modified: 0)
        do {
            let saved = try lib.saveAnnotation(a: a)
            reload()
            undo?.registerUndo(withTarget: self) { m in MainActor.assumeIsolated { m.deleteAnnotation(saved, undo: undo) } }
            undo?.setActionName(note.isEmpty ? "Highlight" : "Note")
            return saved
        }
        catch { errorMessage = error.readable; return nil }
    }

    /// Adds a highlight in the reading view. One that overlaps (or touches) highlights already there
    /// becomes one highlight with them, covering the whole passage: the first of them is stretched
    /// (keeping its id), the others are folded in, their notes kept; the new colour wins. Undo
    /// puts them back as they were.
    func addHighlightMerging(_ paper: String, _ sel: PageSelection, color: HighlightColor = .yellow,
                             controller: ReaderController, undo: UndoManager? = nil) async -> Annotation? {
        guard let (ids, passage) = await controller.mergeRange(sel) else { return addHighlight(paper, sel, color: color, undo: undo) }
        let olds = annotations(paper).filter { ids.contains($0.id) && $0.location == "epub" }
        guard var merged = olds.first else { return addHighlight(paper, sel, color: color, undo: undo) }
        let notes = olds.map { $0.note.trimmingCharacters(in: .whitespacesAndNewlines) }.filter { !$0.isEmpty }
        merged.quote = passage.quote
        merged.prefix = passage.prefix
        merged.suffix = passage.suffix
        merged.position = String(passage.start)
        merged.color = color.rawValue
        merged.note = notes.joined(separator: "\n\n")
        merged.kind = merged.note.isEmpty ? .highlight : .note
        do {
            let saved = try lib.saveAnnotation(a: merged)
            for o in olds.dropFirst() { try? lib.deleteAnnotation(paper: o.paperId, id: o.id) }
            reload()
            registerMergeUndo(merged: saved, originals: olds, undo: undo)
            return saved
        } catch { errorMessage = error.readable; return nil }
    }

    /// Extend… : the highlight `id` now covers `sel` (from its start to the word tapped, over pages
    /// and paragraphs), still one highlight with its id, colour and note. Highlights the new
    /// passage overlaps or touches are folded into it, as when highlighting over them.
    func extendHighlight(_ paper: String, id: String, to sel: PageSelection,
                         controller: ReaderController, undo: UndoManager? = nil) async -> Annotation? {
        guard let original = annotations(paper).first(where: { $0.id == id }) else { return nil }
        var olds = [original], passage = sel
        if let (ids, p) = await controller.mergeRange(sel, excluding: id) {
            passage = p
            olds += annotations(paper).filter { ids.contains($0.id) && $0.location == "epub" && $0.id != id }
        }
        var merged = original
        merged.quote = passage.quote
        merged.prefix = passage.prefix
        merged.suffix = passage.suffix
        merged.position = String(passage.start)
        merged.location = "epub"
        merged.note = olds.map { $0.note.trimmingCharacters(in: .whitespacesAndNewlines) }.filter { !$0.isEmpty }.joined(separator: "\n\n")
        merged.kind = merged.note.isEmpty ? .highlight : .note
        do {
            let saved = try lib.saveAnnotation(a: merged)
            for o in olds.dropFirst() { try? lib.deleteAnnotation(paper: o.paperId, id: o.id) }
            reload()
            registerMergeUndo(merged: saved, originals: olds, undo: undo)
            return saved
        } catch { errorMessage = error.readable; return nil }
    }

    private func registerMergeUndo(merged: Annotation, originals: [Annotation], undo: UndoManager?) {
        undo?.registerUndo(withTarget: self) { m in MainActor.assumeIsolated {
            // undo: the highlights as they were (the first keeps the merged one's id)
            for o in originals { _ = try? m.lib.saveAnnotation(a: o) }
            m.reload()
            undo?.registerUndo(withTarget: m) { m in MainActor.assumeIsolated {
                _ = try? m.lib.saveAnnotation(a: merged)
                for o in originals.dropFirst() { try? m.lib.deleteAnnotation(paper: o.paperId, id: o.id) }
                m.reload()
                m.registerMergeUndo(merged: merged, originals: originals, undo: undo)
            } }
            undo?.setActionName("Highlight")
        } }
        undo?.setActionName("Highlight")
    }

    func saveAnnotation(_ a: Annotation) {
        var a = a
        if !a.isFigureMark { a.kind = a.note.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? .highlight : .note }
        do { _ = try lib.saveAnnotation(a: a) } catch { errorMessage = error.readable }
        reload()
    }

    /// Deletes a highlight; with an undo manager, undo puts it back as it was (same id, colour, note).
    func deleteAnnotation(_ a: Annotation, undo: UndoManager? = nil) {
        try? lib.deleteAnnotation(paper: a.paperId, id: a.id)
        reload()
        undo?.registerUndo(withTarget: self) { m in MainActor.assumeIsolated { m.restoreAnnotation(a, undo: undo) } }
        undo?.setActionName(a.isFigureMark ? "Figure Mark" : a.note.isEmpty ? "Highlight" : "Note")
    }

    /// Puts back a deleted highlight (undo of a delete, or redo of a highlight).
    func restoreAnnotation(_ a: Annotation, undo: UndoManager? = nil) {
        do { _ = try lib.saveAnnotation(a: a) } catch { errorMessage = error.readable; return }
        reload()
        undo?.registerUndo(withTarget: self) { m in MainActor.assumeIsolated { m.deleteAnnotation(a, undo: undo) } }
        undo?.setActionName(a.isFigureMark ? "Figure Mark" : a.note.isEmpty ? "Highlight" : "Note")
    }
}

/// The undo manager for highlights and notes in a reader (its own on the Mac: see ReaderUndoKeys).
private struct HighlightUndoKey: EnvironmentKey { static let defaultValue: UndoManager? = nil }
extension EnvironmentValues {
    var highlightUndo: UndoManager? {
        get { self[HighlightUndoKey.self] }
        set { self[HighlightUndoKey.self] = newValue }
    }
}

// MARK: - Editing one highlight

struct AnnotationEditor: View {
    @EnvironmentObject var model: LibraryModel
    @Environment(\.dismiss) private var dismiss
    @Environment(\.highlightUndo) private var undo
    @State var annotation: Annotation
    /// Extend… : the reading view takes over (the next tap there ends the highlight); nil: not offered.
    var onExtend: (() -> Void)? = nil
    var onDone: () -> Void = {}
    @FocusState private var noteFocused: Bool
    /// Set just before a note made with the Pencil opens here: the note field gets the focus,
    /// ready for handwriting.
    static var focusNext: Date?

    var body: some View {
        // a region marked on a figure has its own editor (FigureMarks.swift)
        if annotation.isFigureMark { FigureMarkEditor(annotation: annotation, onDone: onDone) } else { highlightEditor }
    }

    private var highlightEditor: some View {
        NavigationStack {
            Form {
                Section {
                    Text("“\(annotation.quote.trimmingCharacters(in: .whitespacesAndNewlines))”")
                        .font(.callout).italic().lineLimit(8)
                }
                Section("Color") {
                    HStack(spacing: 14) {
                        ForEach(HighlightColor.allCases) { c in
                            Button { annotation.color = c.rawValue } label: {
                                Circle().fill(c.color).frame(width: 28, height: 28)
                                    .overlay(Circle().stroke(Color.primary, lineWidth: annotation.color == c.rawValue ? 2.5 : 0))
                            }
                            .buttonStyle(.plain)
                            .accessibilityLabel(c.name)
                        }
                    }
                    .padding(.vertical, 4)
                }
                Section("Note") {
                    TextField("Add a note", text: $annotation.note, axis: .vertical)
                        .focused($noteFocused)
                        #if os(iOS)
                        // room to write by hand (Scribble turns it into text) on iPad
                        .lineLimit(UIDevice.current.userInterfaceIdiom == .pad ? 6...16 : 3...10)
                        .frame(minHeight: UIDevice.current.userInterfaceIdiom == .pad ? 150 : nil, alignment: .topLeading)
                        #else
                        .lineLimit(3...10)
                        #endif
                }
                Section {
                    if let onExtend {
                        Button("Extend…") {
                            // a colour or note changed here first is kept
                            if let cur = model.annotations(annotation.paperId).first(where: { $0.id == annotation.id }),
                               cur.color != annotation.color || cur.note != annotation.note { model.saveAnnotation(annotation) }
                            dismiss(); onExtend()
                        }
                    }
                    Button("Copy Quote") { copyText(annotation.quote) }
                    Button("Delete Highlight", role: .destructive) {
                        model.deleteAnnotation(annotation, undo: undo); onDone(); dismiss()
                    }
                }
            }
            .formStyle(.grouped)
            .onAppear {
                guard let t = Self.focusNext else { return }
                Self.focusNext = nil
                guard Date().timeIntervalSince(t) < 5 else { return }
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.6) { noteFocused = true }
            }
            .navigationTitle(annotation.note.isEmpty ? "Highlight" : "Note")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Save") { model.saveAnnotation(annotation); onDone(); dismiss() }
                }
            }
        }
        #if os(macOS)
        .frame(minWidth: 400, minHeight: 420)
        #endif
    }
}

extension Annotation: @retroactive Identifiable {}

// MARK: - All highlights and notes of a paper

struct AnnotationsList: View {
    @EnvironmentObject var model: LibraryModel
    @Environment(\.highlightUndo) private var undo
    let paperID: String
    /// Highlights the reading view couldn't find in its current text (ReaderController.missingAnnotations).
    var missing: Set<String> = []
    var onShow: (Annotation) -> Void
    var onEdit: (Annotation) -> Void

    var body: some View {
        let list = model.annotations(paperID).sorted { Self.order($0) < Self.order($1) }
        let lost = list.filter { missing.contains($0.id) }.count
        List {
            if list.isEmpty {
                ContentUnavailableView("No highlights yet", systemImage: "highlighter",
                                       description: Text("Select text in the reading view, then choose Highlight or Add Note."))
            }
            if lost > 0 {
                Label(lost == 1 ? "1 highlight isn’t in this version of the reading view. It is kept, with its quote and note."
                                : "\(lost) highlights aren’t in this version of the reading view. They are kept, with their quotes and notes.",
                      systemImage: "exclamationmark.triangle")
                    .font(.footnote).foregroundStyle(.secondary)
            }
            ForEach(list) { a in
                Button { onShow(a) } label: {
                    if a.isFigureMark { markRow(a) } else {
                    HStack(alignment: .top, spacing: 10) {
                        RoundedRectangle(cornerRadius: 2).fill((HighlightColor(rawValue: a.color) ?? .yellow).color).frame(width: 4)
                        VStack(alignment: .leading, spacing: 4) {
                            Text(a.quote.trimmingCharacters(in: .whitespacesAndNewlines)).font(.callout).lineLimit(4)
                            if !a.note.isEmpty {
                                Label(a.note, systemImage: "note.text").font(.footnote).foregroundStyle(.secondary)
                            }
                            if missing.contains(a.id) {
                                Label("Not found in this reading view", systemImage: "eye.slash")
                                    .font(.caption).foregroundStyle(.orange)
                            }
                        }
                    }
                    .contentShape(Rectangle())
                    }
                }
                .buttonStyle(.plain)
                .contextMenu {
                    Button("Edit…") { onEdit(a) }
                    if !a.isFigureMark { Button("Copy Quote") { copyText(a.quote) } }
                    Button("Delete", role: .destructive) { model.deleteAnnotation(a, undo: undo) }
                }
                .swipeActions {
                    Button("Delete", role: .destructive) { model.deleteAnnotation(a, undo: undo) }
                    Button("Edit") { onEdit(a) }
                }
            }
            if !list.isEmpty {
                Button("Copy All as Markdown") { copyText(Self.markdown(list, model.paper(paperID))) }
            }
        }
    }

    /// A figure mark: the region cropped from its image, "Figure 3 · region", its note.
    private func markRow(_ a: Annotation) -> some View {
        HStack(alignment: .top, spacing: 10) {
            MarkThumbnail(annotation: a)
            VStack(alignment: .leading, spacing: 4) {
                Text(figureMarkTitle(a)).font(.callout.weight(.medium))
                if !a.note.isEmpty {
                    Label(a.note, systemImage: "note.text").font(.footnote).foregroundStyle(.secondary).lineLimit(4)
                } else if !a.suffix.isEmpty {
                    Text(a.suffix).font(.footnote).foregroundStyle(.secondary).lineLimit(2)
                }
            }
            Spacer(minLength: 0)
        }
        .contentShape(Rectangle())
    }

    /// Where a highlight is in the paper (a mark: where its figure is), to list them in the paper's order.
    static func order(_ a: Annotation) -> Int {
        if a.isFigureMark { return Int(figureRegion(annotation: a)?.start ?? 0) }
        return Int(a.position) ?? 0
    }

    static func markdown(_ list: [Annotation], _ p: Paper?) -> String {
        var out = "## \(p?.meta.title ?? "Highlights")\n\n"
        for a in list {
            if a.isFigureMark {
                // the region as words: its figure, and the start of the caption
                out += "> **\(figureMarkTitle(a))**" + (a.suffix.isEmpty ? "" : " — \(a.suffix)") + "\n"
                if !a.note.isEmpty { out += "\n\(a.note)\n" }
                out += "\n"
                continue
            }
            out += "> \(a.quote.trimmingCharacters(in: .whitespacesAndNewlines))\n"
            if !a.note.isEmpty { out += "\n\(a.note)\n" }
            out += "\n"
        }
        return out
    }
}

// MARK: - "Highlight" and "Add Note" in the text selection menu

enum ReaderMenuAction { case highlight(HighlightColor), note }

#if os(macOS)
final class ReaderWebView: WKWebView {
    weak var controller: ReaderController?

    override func willOpenMenu(_ menu: NSMenu, with event: NSEvent) {
        super.willOpenMenu(menu, with: event)
        guard controller?.onMenu != nil else { return }
        let hl = NSMenuItem(title: "Highlight", action: #selector(highlightYellow), keyEquivalent: "")
        hl.target = self
        let sub = NSMenu()
        for c in HighlightColor.allCases {
            let i = NSMenuItem(title: c.name, action: #selector(highlightColor(_:)), keyEquivalent: "")
            i.target = self; i.representedObject = c.rawValue
            sub.addItem(i)
        }
        let colors = NSMenuItem(title: "Highlight in Color", action: nil, keyEquivalent: "")
        colors.submenu = sub
        let note = NSMenuItem(title: "Add Note…", action: #selector(addNote), keyEquivalent: "")
        note.target = self
        menu.insertItem(.separator(), at: 0)
        menu.insertItem(note, at: 0)
        menu.insertItem(colors, at: 0)
        menu.insertItem(hl, at: 0)
    }

    @objc private func highlightYellow() { controller?.onMenu?(.highlight(.yellow)) }
    @objc private func highlightColor(_ sender: NSMenuItem) {
        controller?.onMenu?(.highlight(HighlightColor(rawValue: sender.representedObject as? String ?? "") ?? .yellow))
    }
    @objc private func addNote() { controller?.onMenu?(.note) }
}
#else
final class ReaderWebView: WKWebView {
    weak var controller: ReaderController?
    /// Apple Pencil highlighting and notes (iPad).
    private var pencil: PencilReader?

    override func didMoveToWindow() {
        super.didMoveToWindow()
        if window != nil, pencil == nil, UIDevice.current.userInterfaceIdiom == .pad { pencil = PencilReader(self) }
    }

    override func buildMenu(with builder: UIMenuBuilder) {
        super.buildMenu(with: builder)
        guard controller?.onMenu != nil else { return }
        let hl = UIAction(title: "Highlight", image: UIImage(systemName: "highlighter")) { [weak self] _ in
            self?.controller?.onMenu?(.highlight(.yellow))
        }
        let note = UIAction(title: "Add Note", image: UIImage(systemName: "note.text.badge.plus")) { [weak self] _ in
            self?.controller?.onMenu?(.note)
        }
        let colors = UIMenu(title: "Color", image: UIImage(systemName: "paintpalette"), children: HighlightColor.allCases.map { c in
            UIAction(title: c.name) { [weak self] _ in self?.controller?.onMenu?(.highlight(c)) }
        })
        var items: [UIMenuElement] = [hl, note, colors]
        // a passage running over the page's end: highlight from here to the word tapped next
        if controller?.onHighlightTo != nil {
            items.append(UIAction(title: "Highlight to…", image: UIImage(systemName: "arrow.right.to.line")) { [weak self] _ in
                self?.controller?.onHighlightTo?()
            })
        }
        builder.insertChild(UIMenu(title: "", options: .displayInline, children: items), atStartOfMenu: .root)
    }
}
#endif

extension WKWebViewConfiguration {
    /// Adds the highlight script and its message handler.
    func addAnnotations(_ controller: ReaderController) {
        userContentController.addUserScript(WKUserScript(source: ReaderAssets.annotationJS, injectionTime: .atDocumentEnd, forMainFrameOnly: true))
        userContentController.addUserScript(WKUserScript(source: ReaderAssets.positionJS, injectionTime: .atDocumentEnd, forMainFrameOnly: true))
        let h = AnnotationMessageHandler()
        h.controller = controller
        userContentController.add(h, name: "ann")
    }
}

/// Highlight mode: every selection is highlighted as soon as it's made, in the chosen colour, so
/// marking up a paper doesn't need a menu each time. A selection that overlaps highlights already
/// there (including one just made, then extended) joins them into one highlight.
@MainActor
final class AutoHighlighter {
    func apply(_ sel: PageSelection, paper: String, color: HighlightColor, model: LibraryModel,
               controller: ReaderController, undo: UndoManager?) async {
        _ = await model.addHighlightMerging(paper, sel, color: color, controller: controller, undo: undo)
    }
}
