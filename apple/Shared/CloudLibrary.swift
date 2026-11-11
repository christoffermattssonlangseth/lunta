import Foundation

/// Where the library lives: the app's iCloud Drive container when iCloud is available, otherwise a
/// local folder. The layout is sync-safe (see core/src/store.rs), so both devices can write to it.
enum CloudLibrary {
    static let containerID = "iCloud.com.cmlangseth.margins"

    static var localURL: URL {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("Margins/Library", isDirectory: true)
    }

    /// Resolves the library folder. Blocks on the first iCloud call, so run it off the main thread.
    /// On first use with iCloud, an existing local library is copied into iCloud (the local copy is kept).
    static func resolve() -> (url: URL, inCloud: Bool) {
        guard let container = FileManager.default.url(forUbiquityContainerIdentifier: containerID) else {
            return (localURL, false)
        }
        let lib = container.appendingPathComponent("Documents/Library", isDirectory: true)
        migrateIfNeeded(from: localURL, to: lib)
        return (lib, true)
    }

    private static func hasLogs(_ lib: URL) -> Bool {
        let ops = (try? FileManager.default.contentsOfDirectory(atPath: lib.appendingPathComponent("ops").path)) ?? []
        return ops.contains { $0.hasSuffix(".jsonl") || $0.hasSuffix(".jsonl.icloud") }
    }

    private static func migrateIfNeeded(from local: URL, to cloud: URL) {
        let fm = FileManager.default
        guard hasLogs(local), !hasLogs(cloud) else { return }
        for sub in ["ops", "papers"] {
            let src = local.appendingPathComponent(sub)
            let dst = cloud.appendingPathComponent(sub)
            try? fm.createDirectory(at: dst, withIntermediateDirectories: true)
            for name in (try? fm.contentsOfDirectory(atPath: src.path)) ?? [] {
                let to = dst.appendingPathComponent(name)
                if !fm.fileExists(atPath: to.path) {
                    try? fm.copyItem(at: src.appendingPathComponent(name), to: to)
                }
            }
        }
    }

    /// The real file behind an iCloud placeholder path (".name.icloud" → "name").
    static func realURL(forPlaceholder path: String) -> URL {
        let u = URL(fileURLWithPath: path)
        var name = u.lastPathComponent
        if name.hasPrefix(".") { name.removeFirst() }
        if name.hasSuffix(".icloud") { name.removeLast(".icloud".count) }
        return u.deletingLastPathComponent().appendingPathComponent(name)
    }

    static func startDownload(_ url: URL) {
        try? FileManager.default.startDownloadingUbiquitousItem(at: url)
    }

    /// Asks iCloud for the file and waits until it is on this device.
    static func download(_ url: URL, timeout: TimeInterval = 180) async -> Bool {
        if FileManager.default.fileExists(atPath: url.path) { return true }
        startDownload(url)
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            try? await Task.sleep(nanoseconds: 400_000_000)
            if FileManager.default.fileExists(atPath: url.path) { return true }
        }
        return false
    }
}

/// Reader styling and scripts shared by the Mac and iPhone reading views.
enum ReaderAssets {
    static let css = """
    html { font-size: 112%; scroll-behavior: smooth; -webkit-text-size-adjust: 100%; }
    body { max-width: 42em; margin: 2.2em auto !important; padding: 0 2em 6em; font-family: "Iowan Old Style", Georgia, serif;
           line-height: 1.6; color: #1d1d1f; background: #fffdf8; }
    @media (max-width: 640px) { html { font-size: 106%; } body { margin-top: 1em !important; padding: 0 1.1em 5em; } }
    @media (prefers-color-scheme: dark) { body { color: #e6e3dc; background: #1c1b19; } a { color: #9ec9d8; } img { opacity: .92; } }
    h1, h2, h3 { font-family: -apple-system, "Helvetica Neue", sans-serif; scroll-margin-top: 1em; }
    h1 { margin-top: 2.4em; padding-top: 1.2em; border-top: 1px solid rgba(127,127,127,.25); }
    .titlepage h1 { border-top: none; padding-top: 0; margin-top: .6em; }
    figure img, .table-wrap img { border-radius: 4px; }
    .section-start { scroll-margin-top: 1em; }
    .narr-current { background: rgba(242,180,90,.30); border-radius: 6px; box-shadow: 0 0 0 5px rgba(242,180,90,.30);
                    transition: background .25s; }
    @media (prefers-color-scheme: dark) { .narr-current { background: rgba(242,180,90,.22); box-shadow: 0 0 0 5px rgba(242,180,90,.22); } }
    body.listening p, body.listening li, body.listening h1, body.listening h2, body.listening h3, body.listening figcaption { cursor: pointer; }
    """

    /// Injects `css` into the page.
    static var styleScript: String {
        let c = css.replacingOccurrences(of: "`", with: "")
        return "var s=document.createElement('style');s.textContent=`\(c)`;document.documentElement.appendChild(s);"
    }

    /// Reports the id of the heading currently at the top of the screen to the "pos" handler.
    static let trackingJS = """
    (function(){
      var t = null;
      function report(){
        var cur = null, hs = document.querySelectorAll('h1[id],h2[id],h3[id],.section-start');
        for (var i = 0; i < hs.length; i++) { if (hs[i].getBoundingClientRect().top < 140) cur = hs[i].id; else break; }
        if (cur) window.webkit.messageHandlers.pos.postMessage(cur);
      }
      window.addEventListener('scroll', function(){ clearTimeout(t); t = setTimeout(report, 90); });
    })();
    """

    /// Narration helpers: list readable units in reading order, highlight one, report taps.
    static let narrationJS = #"""
    (function(){
      window.marginsNarration = function(includeCaptions){
        var out = [], n = 0;
        var skip = '.references, ol.refs, .refs, .notes, table, .affiliations, .frontmatter, .license, .doi, .journal, nav, .supp';
        var els = document.querySelectorAll('h1, h2, h3, h4, p, li, figcaption, .titlepage .authors');
        for (var i = 0; i < els.length; i++) {
          var el = els[i];
          if (el.closest(skip)) continue;
          if (!includeCaptions && (el.closest('figure') || el.classList.contains('caption'))) continue;
          var tag = el.tagName.toUpperCase();   // XHTML pages report lowercase names
          if (tag === 'LI' && el.querySelector('p')) continue;
          var c = el.cloneNode(true);
          c.querySelectorAll('sup, .fn, .eqlabel, script, style, img').forEach(function(x){ x.remove(); });
          c.querySelectorAll('math').forEach(function(m){ m.replaceWith(document.createTextNode(' [equation] ')); });
          var t = (c.textContent || '').replace(/\s+/g, ' ').trim();
          if (t.length < 2) continue;
          // footnote-style lines from PDFs: affiliations, equal-contribution notes, venue lines
          if (/^[∗*†‡§¶]/.test(t) || /^(Work (was )?performed|Correspond|E-?mail|Present address|\d+(st|nd|rd|th) (Conference|International))/i.test(t)) continue;
          if (!el.id) el.id = 'narr-' + (n++);
          var kind = /^H\d$/.test(tag) ? 'heading' : (el.closest('figure') ? 'caption' : 'text');
          out.push({ id: el.id, text: t, kind: kind });
        }
        return JSON.stringify(out);
      };
      window.marginsHighlight = function(id){
        document.querySelectorAll('.narr-current').forEach(function(e){ e.classList.remove('narr-current'); });
        var e = document.getElementById(id);
        if (e) { e.classList.add('narr-current'); e.scrollIntoView({ behavior: 'smooth', block: 'center' }); }
      };
      window.marginsListening = function(on){
        document.body.classList.toggle('listening', !!on);
        if (!on) document.querySelectorAll('.narr-current').forEach(function(e){ e.classList.remove('narr-current'); });
      };
      window.marginsFirstVisible = function(){
        var els = document.querySelectorAll('[id^="narr-"], h1[id], h2[id], h3[id], p[id], li[id]');
        for (var i = 0; i < els.length; i++) { var r = els[i].getBoundingClientRect(); if (r.bottom > 60) return els[i].id; }
        return '';
      };
      document.addEventListener('click', function(ev){
        if (!document.body.classList.contains('listening')) return;
        var el = ev.target.closest('p, li, h1, h2, h3, h4, figcaption');
        if (el && el.id && !ev.target.closest('a')) window.webkit.messageHandlers.narr.postMessage(el.id);
      });
    })();
    """#

    static func scrollJS(_ anchor: String) -> String {
        let safe = anchor.replacingOccurrences(of: "\\", with: "").replacingOccurrences(of: "'", with: "\\'")
        return "var e=document.getElementById('\(safe)'); if(e) e.scrollIntoView({behavior:'smooth', block:'start'});"
    }
}
