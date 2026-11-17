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
      window.luntaSelection = function(){
        var sel = window.getSelection(); if (!sel || sel.rangeCount === 0 || sel.isCollapsed) return '';
        var r = sel.getRangeAt(0), nodes = textNodes();
        var s = r.startContainer.nodeType === 3 ? offsetOf(nodes, r.startContainer, r.startOffset) : -1;
        var e = r.endContainer.nodeType === 3 ? offsetOf(nodes, r.endContainer, r.endOffset) : -1;
        var text = fullText(nodes);
        if (s < 0 || e < 0) { var q0 = sel.toString(); s = text.indexOf(q0); e = s + q0.length; if (s < 0) return ''; }
        var quote = text.slice(s, e);
        if (!quote.trim()) return '';
        return JSON.stringify({ quote: quote, prefix: text.slice(Math.max(0, s - 40), s), suffix: text.slice(e, e + 40), start: s });
      };
      function unwrapAll(){
        document.querySelectorAll('mark.m-hl').forEach(function(m){
          var p = m.parentNode; while (m.firstChild) p.insertBefore(m.firstChild, m); p.removeChild(m); p.normalize();
        });
      }
      var lcache = { text: null };
      function locate(text, a){
        if (!a.quote || !a.quote.trim()) return null;   // a note without a passage: nothing to mark (and indexOf('') never ends)
        var best = -1, bestScore = -1, i = text.indexOf(a.quote);
        while (i >= 0) {
          var score = 0;
          if (a.prefix && text.slice(Math.max(0, i - a.prefix.length), i) === a.prefix) score += 2;
          if (a.suffix && text.slice(i + a.quote.length, i + a.quote.length + a.suffix.length) === a.suffix) score += 2;
          score -= Math.min(1, Math.abs(i - (a.start || 0)) / 100000);
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
        if (document.body.classList.contains('listening')) return;
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
        var s = window.getSelection(); if (!s || s.isCollapsed || s.toString().trim().length < 2) return;
        var j = window.luntaSelection(); if (!j) return;
        // coloured here and now; the app saves it and redraws (which replaces this mark with the real one)
        try {
          var o = JSON.parse(j);
          if (o.start >= 0) wrap(textNodes(), o.start, o.start + o.quote.length, { id: '__pending', color: window.luntaHLColor || 'yellow' });
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
        return JSON.stringify({ quote: text.slice(s, e), prefix: text.slice(Math.max(0, s - 40), s), suffix: text.slice(e, e + 40), start: s });
      }
      function caretOffset(nodes, x, y){
        var r = document.caretRangeFromPoint && document.caretRangeFromPoint(x, y);
        if (!r) return -1;
        var n = r.startContainer, o = r.startOffset;
        if (n.nodeType === 3) return offsetOf(nodes, n, o);
        // between blocks or beside an image: the first text after that point
        var el = n.childNodes[o] || n, pos = 0;
        for (var i = 0; i < nodes.length; i++) {
          if (el === nodes[i] || (el.compareDocumentPosition(nodes[i]) & 4)) return pos;
          pos += nodes[i].nodeValue.length;
        }
        return -1;
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
        try { var o = JSON.parse(j); wrap(textNodes(), o.start, o.start + o.quote.length, { id: '__pending', color: color || 'yellow' }); } catch (x) {}
        return j;
      };
      // A new highlight from s to e: the highlights already drawn that it overlaps or touches, and
      // the passage covering all of them (so they become one highlight instead of one replacing
      // or stacking on another). {ids: [...], quote, prefix, suffix, start}
      window.luntaMergeRange = function(s, e){
        var nodes = textNodes(), text = fullText(nodes), spans = {};
        document.querySelectorAll('mark.m-hl[data-ann]').forEach(function(m){
          var id = m.getAttribute('data-ann'); if (!id || id === '__pending') return;
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
            else { self.controller?.onAnnotation?(id) }
        }
    }
}

extension ReaderController {
    /// The highlights already in the page that a new one over `sel` overlaps or touches, and the
    /// passage covering them all (nil: none, or the page couldn't tell).
    func mergeRange(_ sel: PageSelection) async -> (ids: [String], passage: PageSelection)? {
        let end = sel.start + sel.quote.utf16.count
        guard sel.start >= 0,
              let json = try? await webView?.evaluateJavaScript("window.luntaMergeRange ? luntaMergeRange(\(sel.start), \(end)) : ''") as? String,
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
    func showAnnotations(_ list: [Annotation]) {
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
            var missing = Set<String>()
            if let s = result as? String, let d = s.data(using: .utf8), let ids = try? JSONDecoder().decode([String].self, from: d) {
                missing = Set(ids)
            }
            if missing != self.missingAnnotations { self.missingAnnotations = missing }
        }
    }

    func showAnnotation(_ id: String) {
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
        a.kind = a.note.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? .highlight : .note
        do { _ = try lib.saveAnnotation(a: a) } catch { errorMessage = error.readable }
        reload()
    }

    /// Deletes a highlight; with an undo manager, undo puts it back as it was (same id, colour, note).
    func deleteAnnotation(_ a: Annotation, undo: UndoManager? = nil) {
        try? lib.deleteAnnotation(paper: a.paperId, id: a.id)
        reload()
        undo?.registerUndo(withTarget: self) { m in MainActor.assumeIsolated { m.restoreAnnotation(a, undo: undo) } }
        undo?.setActionName(a.note.isEmpty ? "Highlight" : "Note")
    }

    /// Puts back a deleted highlight (undo of a delete, or redo of a highlight).
    func restoreAnnotation(_ a: Annotation, undo: UndoManager? = nil) {
        do { _ = try lib.saveAnnotation(a: a) } catch { errorMessage = error.readable; return }
        reload()
        undo?.registerUndo(withTarget: self) { m in MainActor.assumeIsolated { m.deleteAnnotation(a, undo: undo) } }
        undo?.setActionName(a.note.isEmpty ? "Highlight" : "Note")
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
    var onDone: () -> Void = {}
    @FocusState private var noteFocused: Bool
    /// Set just before a note made with the Pencil opens here: the note field gets the focus,
    /// ready for handwriting.
    static var focusNext: Date?

    var body: some View {
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
        let list = model.annotations(paperID).sorted { (Int($0.position) ?? 0) < (Int($1.position) ?? 0) }
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
                .buttonStyle(.plain)
                .contextMenu {
                    Button("Edit…") { onEdit(a) }
                    Button("Copy Quote") { copyText(a.quote) }
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

    static func markdown(_ list: [Annotation], _ p: Paper?) -> String {
        var out = "## \(p?.meta.title ?? "Highlights")\n\n"
        for a in list {
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
        builder.insertChild(UIMenu(title: "", options: .displayInline, children: [hl, note, colors]), atStartOfMenu: .root)
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
