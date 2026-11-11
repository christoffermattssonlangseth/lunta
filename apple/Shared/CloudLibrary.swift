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

    /// Waits until the library's change logs are on this device (they are what lists the papers).
    /// Reports progress text; returns when all logs are downloaded or none are expected yet.
    static func prepareLogs(_ lib: URL, report: @escaping @Sendable (String) -> Void) async {
        let fm = FileManager.default
        let ops = lib.appendingPathComponent("ops")
        try? fm.createDirectory(at: ops, withIntermediateDirectories: true)
        // a brand-new device may not have the folder listing yet: give iCloud a moment to deliver it
        var waited = 0.0
        while true {
            let names = (try? fm.contentsOfDirectory(atPath: ops.path)) ?? []
            let logs = names.filter { $0.hasSuffix(".jsonl") || $0.hasSuffix(".jsonl.icloud") }
                .map { $0.hasSuffix(".icloud") ? realURL(forPlaceholder: ops.appendingPathComponent($0).path) : ops.appendingPathComponent($0) }
            if logs.isEmpty {
                if waited > 20 { return }   // nothing in iCloud yet (new library): open empty
                report("Looking for your library in iCloud…")
            } else {
                let missing = logs.filter { !isDownloaded($0) }
                if missing.isEmpty { return }
                missing.forEach(startDownload)
                report("Getting your library from iCloud… (\(logs.count - missing.count) of \(logs.count))")
            }
            try? await Task.sleep(nanoseconds: 500_000_000)
            waited += 0.5
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

    /// True when the file's contents are on this device. iCloud Drive keeps not-yet-downloaded
    /// files as "dataless" entries that look normal but block when read.
    static func isDownloaded(_ url: URL) -> Bool {
        // a fresh URL every time: Foundation caches resource values on a URL, so polling the same
        // one never notices that the download finished
        var u = URL(fileURLWithPath: url.path)
        u.removeAllCachedResourceValues()
        guard FileManager.default.fileExists(atPath: u.path) else { return false }
        guard let v = try? u.resourceValues(forKeys: [.isUbiquitousItemKey, .ubiquitousItemDownloadingStatusKey]),
              v.isUbiquitousItem == true else { return true }
        // .downloaded = a local copy that may not be the newest version; library files never change
        return v.ubiquitousItemDownloadingStatus == .current || v.ubiquitousItemDownloadingStatus == .downloaded
    }

    static func startDownload(_ url: URL) {
        try? FileManager.default.startDownloadingUbiquitousItem(at: URL(fileURLWithPath: url.path))
    }

    /// Asks iCloud for the file and waits until it is on this device.
    static func download(_ url: URL, timeout: TimeInterval = 180) async -> Bool {
        if isDownloaded(url) { return true }
        startDownload(url)
        // a coordinated read makes iOS fetch the file and only returns once it's readable
        let path = url.path
        let coordinated = Task.detached(priority: .userInitiated) { () -> Bool in
            var err: NSError?
            var ok = false
            NSFileCoordinator().coordinate(readingItemAt: URL(fileURLWithPath: path), options: [], error: &err) { u in
                ok = FileManager.default.isReadableFile(atPath: u.path)
            }
            return ok && err == nil
        }
        let deadline = Date().addingTimeInterval(timeout)
        let done = Flag()
        Task { if await coordinated.value { done.set() } }
        while Date() < deadline {
            try? await Task.sleep(nanoseconds: 300_000_000)
            if done.isSet || isDownloaded(url) { return true }
        }
        return false
    }
}

private final class Flag: @unchecked Sendable {
    private let lock = NSLock()
    private var value = false
    func set() { lock.lock(); value = true; lock.unlock() }
    var isSet: Bool { lock.lock(); defer { lock.unlock() }; return value }
}

/// Reader styling and scripts shared by the Mac and iPhone reading views.
enum ReaderAssets {
    static let css = """
    html { font-size: 112%; scroll-behavior: smooth; -webkit-text-size-adjust: 100%; }
    body { max-width: 42em; margin: 2.2em auto !important; padding: 0 2em 6em; font-family: "Iowan Old Style", Georgia, serif;
           line-height: 1.6; color: #1d1d1f; background: #fffdf8; }
    @media (max-width: 640px) { html { font-size: 122%; } body { margin-top: 1em !important; padding: 0 1.05em 5em; line-height: 1.55; } }
    @media (prefers-color-scheme: dark) { body { color: #e6e3dc; background: #1c1b19; } a { color: #9ec9d8; } img { opacity: .92; } }
    h1, h2, h3 { font-family: -apple-system, "Helvetica Neue", sans-serif; scroll-margin-top: 1em; }
    h1 { margin-top: 2.4em; padding-top: 1.2em; border-top: 1px solid rgba(127,127,127,.25); }
    .titlepage h1 { border-top: none; padding-top: 0; margin-top: .6em; }
    figure img, .table-wrap img { border-radius: 4px; }
    img { max-width: 100%; height: auto; cursor: zoom-in; }
    /* nothing may stick out sideways: long links and words wrap, wide things scroll inside their own box */
    html, body { overflow-x: hidden; max-width: 100vw; }
    p, li, a, td, th, figcaption, h1, h2, h3, h4, div { overflow-wrap: anywhere; }
    svg, video, iframe, object, embed, canvas { max-width: 100%; height: auto; }
    pre { white-space: pre-wrap; }
    .m-scroll { overflow-x: auto; -webkit-overflow-scrolling: touch; max-width: 100%; margin: 1em 0; }
    .m-scroll > table { margin: 0; }
    table { border-collapse: collapse; }
    .section-start { scroll-margin-top: 1em; }
    .narr-current { background: rgba(242,180,90,.30); border-radius: 6px; box-shadow: 0 0 0 5px rgba(242,180,90,.30);
                    transition: background .25s; }
    @media (prefers-color-scheme: dark) { .narr-current { background: rgba(242,180,90,.22); box-shadow: 0 0 0 5px rgba(242,180,90,.22); } }
    body.listening p, body.listening li, body.listening h1, body.listening h2, body.listening h3, body.listening figcaption { cursor: pointer; }
    """

    /// Tapping a figure (or any image) asks the app to show it full screen, zoomable.
    static let figureJS = """
    document.addEventListener('click', function(e){
      var img = e.target.closest && e.target.closest('img'); if (!img) return;
      var fig = img.closest('figure, .fig, .table-wrap');
      var cap = fig ? fig.querySelector('figcaption, .caption') : null;
      try { window.webkit.messageHandlers.fig.postMessage({ src: img.src, caption: cap ? cap.innerText : (img.alt || '') }); } catch (x) { return; }
      e.preventDefault(); e.stopPropagation();
    }, true);
    """

    /// Finds anything wider than the page (tables, equations, code, fixed-width boxes) and contains it,
    /// so the page itself never becomes wider than the screen. Runs again as images load.
    static let overflowJS = """
    (function(){
      function fix(){
        // in page-turning mode the limit is one column, wherever on the page it sits
        var paged = window.marginsPaged && window.marginsPaged();
        var W = paged ? document.body.getBoundingClientRect().width : document.documentElement.clientWidth;
        if (paged) { var cs = getComputedStyle(document.body); W = parseFloat(cs.columnWidth) || W; }
        var all = document.body ? document.body.getElementsByTagName('*') : [];
        for (var i = 0; i < all.length; i++) {
          var el = all[i];
          if (el.closest('.m-scroll')) continue;
          var r = el.getBoundingClientRect();
          if ((paged ? r.width <= W + 1 : r.right <= W + 1) && el.scrollWidth <= el.clientWidth + 1) continue;
          var tag = el.tagName.toLowerCase();
          if (tag === 'table' || tag === 'pre' || tag === 'math' || el.classList.contains('table-wrap') || el.classList.contains('disp-formula')) {
            var box = document.createElement('div'); box.className = 'm-scroll';
            el.parentNode.insertBefore(box, el); box.appendChild(el);
          } else if (r.width > W) {
            el.style.maxWidth = '100%'; el.style.boxSizing = 'border-box';
            if (el.scrollWidth > el.clientWidth + 1) el.style.overflowX = 'auto';
          }
        }
      }
      window.marginsFixOverflow = fix;
      fix();
      window.addEventListener('load', fix);
      setTimeout(fix, 600);
    })();
    """

    /// Injects `css` into the page.
    static var styleScript: String {
        let c = css.replacingOccurrences(of: "`", with: "")
        // the books don't declare a mobile viewport; without one the iPhone lays the page out 980 px wide and shrinks it
        return "var s=document.createElement('style');s.textContent=`\(c)`;document.documentElement.appendChild(s);"
            + "if(!document.querySelector('meta[name=viewport]')){var m=document.createElement('meta');m.name='viewport';m.content='width=device-width, initial-scale=1, minimum-scale=1, viewport-fit=cover';(document.head||document.documentElement).appendChild(m);}"
            + figureJS + overflowJS
    }

    /// Reports the id of the heading currently at the top of the screen to the "pos" handler.
    static let trackingJS = """
    (function(){
      var t = null;
      function report(){
        var cur = null, hs = document.querySelectorAll('h1[id],h2[id],h3[id],.section-start');
        for (var i = 0; i < hs.length; i++) { var r = hs[i].getBoundingClientRect(); if (window.marginsPaged && window.marginsPaged() ? r.left < window.innerWidth - 1 : r.top < 140) cur = hs[i].id; else break; }
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
        for (var i = 0; i < els.length; i++) { var r = els[i].getBoundingClientRect(); if (r.bottom > 60 && r.right > 0) return els[i].id; }
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
