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
      function locate(text, a){
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
          var map = [], flat = '';
          for (var k = 0; k < text.length; k++) {
            var c = text[k];
            if (/\s/.test(c)) { if (flat.length && flat[flat.length - 1] === ' ') continue; c = ' '; }
            map.push(k); flat += c;
          }
          var j = flat.indexOf(q); if (j < 0) return null;
          return [map[j], map[j + q.length - 1] + 1];
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
        var list = JSON.parse(json), missing = [];
        list.sort(function(x, y){ return (y.start || 0) - (x.start || 0); });   // last first keeps earlier offsets valid
        for (var i = 0; i < list.length; i++) {
          var nodes = textNodes(), text = fullText(nodes), at = locate(text, list[i]);
          if (!at || !wrap(nodes, at[0], at[1], list[i])) missing.push(list[i].id);
        }
        return JSON.stringify(missing);
      };
      window.luntaShowAnnotation = function(id){
        var m = document.querySelector('mark[data-ann="' + id + '"]'); if (!m) return false;
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
            if id == "__ready__" { self.controller?.onPageReady?() } else { self.controller?.onAnnotation?(id) }
        }
    }
}

extension ReaderController {
    func pageSelection() async -> PageSelection? {
        guard let json = try? await webView?.evaluateJavaScript("luntaSelection()") as? String,
              let data = json.data(using: .utf8) else { return nil }
        return try? JSONDecoder().decode(PageSelection.self, from: data)
    }

    /// Draws the paper's highlights into the page (replacing what's there).
    func showAnnotations(_ list: [Annotation]) {
        let items: [[String: Any]] = list.filter { $0.location == "epub" }.map { a in
            ["id": a.id, "quote": a.quote, "prefix": a.prefix, "suffix": a.suffix, "color": a.color, "note": a.note,
             "start": Int(a.position) ?? 0]
        }
        guard let data = try? JSONSerialization.data(withJSONObject: items), let json = String(data: data, encoding: .utf8) else { return }
        let arg = json.replacingOccurrences(of: "\\", with: "\\\\").replacingOccurrences(of: "'", with: "\\'")
            .replacingOccurrences(of: "\n", with: "\\n").replacingOccurrences(of: "\u{2028}", with: "\\u2028").replacingOccurrences(of: "\u{2029}", with: "\\u2029")
        webView?.evaluateJavaScript("luntaApplyAnnotations('\(arg)')")
    }

    func showAnnotation(_ id: String) {
        let safe = id.filter { $0.isLetter || $0.isNumber || $0 == "-" }
        webView?.evaluateJavaScript("luntaShowAnnotation('\(safe)')")
    }

    func clearSelection() {
        webView?.evaluateJavaScript("window.getSelection().removeAllRanges()")
    }
}

extension LibraryModel {
    func annotations(_ paper: String) -> [Annotation] { lib.annotations(paper: paper) }

    @discardableResult
    func addHighlight(_ paper: String, _ s: PageSelection, color: HighlightColor = .yellow, note: String = "") -> Annotation? {
        let a = Annotation(id: "", paperId: paper, kind: note.isEmpty ? .highlight : .note, color: color.rawValue,
                           location: "epub", position: String(s.start), quote: s.quote, prefix: s.prefix, suffix: s.suffix,
                           note: note, created: 0, modified: 0)
        do { let saved = try lib.saveAnnotation(a: a); reload(); return saved }
        catch { errorMessage = error.localizedDescription; return nil }
    }

    func saveAnnotation(_ a: Annotation) {
        var a = a
        a.kind = a.note.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? .highlight : .note
        do { _ = try lib.saveAnnotation(a: a) } catch { errorMessage = error.localizedDescription }
        reload()
    }

    func deleteAnnotation(_ a: Annotation) {
        try? lib.deleteAnnotation(paper: a.paperId, id: a.id)
        reload()
    }
}

// MARK: - Editing one highlight

struct AnnotationEditor: View {
    @EnvironmentObject var model: LibraryModel
    @Environment(\.dismiss) private var dismiss
    @State var annotation: Annotation
    var onDone: () -> Void = {}

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
                    TextField("Add a note", text: $annotation.note, axis: .vertical).lineLimit(3...10)
                }
                Section {
                    Button("Copy Quote") { copyText(annotation.quote) }
                    Button("Delete Highlight", role: .destructive) {
                        model.deleteAnnotation(annotation); onDone(); dismiss()
                    }
                }
            }
            .formStyle(.grouped)
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
    let paperID: String
    var onShow: (Annotation) -> Void
    var onEdit: (Annotation) -> Void

    var body: some View {
        let list = model.annotations(paperID).sorted { (Int($0.position) ?? 0) < (Int($1.position) ?? 0) }
        List {
            if list.isEmpty {
                ContentUnavailableView("No highlights yet", systemImage: "highlighter",
                                       description: Text("Select text in the reading view, then choose Highlight or Add Note."))
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
                        }
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .contextMenu {
                    Button("Edit…") { onEdit(a) }
                    Button("Copy Quote") { copyText(a.quote) }
                    Button("Delete", role: .destructive) { model.deleteAnnotation(a) }
                }
                .swipeActions {
                    Button("Delete", role: .destructive) { model.deleteAnnotation(a) }
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
