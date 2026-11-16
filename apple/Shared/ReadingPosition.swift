import Foundation
import WebKit

// Where you stopped reading, synced between devices. Kept out of the op log (it changes all the
// time): each device writes its own small file, positions/<device>.json, in the library folder,
// and a reader opens at the newest position from any device. A position is a character offset
// into the page's text, which is the same on every device whatever the screen size.

struct ReadingSpot: Codable, Equatable {
    var offset: Int
    var at: Date
    var device: String       // "Mac" or "iPhone", for the "continued from" note
}

@MainActor
final class ReadingPositions {
    static let shared = ReadingPositions()
    private var mine: [String: ReadingSpot]?
    private var root: URL?
    /// When the place a reader opened at was saved (by any device): a save near that place must
    /// still go out, with a fresh time, or the older place would keep winning on the other device.
    private var restoredAt: [String: Date] = [:]
    /// Writes one after another, so an older snapshot never lands after a newer one.
    private nonisolated static let writer = DispatchQueue(label: "lunta.positions", qos: .utility)

    #if os(macOS)
    static let deviceLabel = "Mac"
    #else
    static let deviceLabel = "iPhone"
    #endif

    private func folder(_ root: URL) -> URL { root.appendingPathComponent("positions", isDirectory: true) }

    private func own(_ root: URL) -> [String: ReadingSpot] {
        if mine == nil || self.root != root { mine = Self.read(folder(root).appendingPathComponent("\(LibraryModel.deviceID).json")); self.root = root }
        return mine ?? [:]
    }

    /// Saves the place. Small moves are skipped (it saves every few seconds) unless `force`
    /// (closing the paper, leaving the app); `wait` writes it before returning (the app may be
    /// about to be closed).
    func save(_ paper: String, offset: Int, root: URL, force: Bool = false, wait: Bool = false) {
        var all = own(root)
        if !force, let old = all[paper], abs(old.offset - offset) < 40, old.at >= (restoredAt[paper] ?? .distantPast) { return }
        if let old = all[paper], old.offset == offset, old.at >= (restoredAt[paper] ?? .distantPast), Date().timeIntervalSince(old.at) < 2 { return }
        posLog("save \(offset)\(force ? " (forced)" : "")")
        all[paper] = ReadingSpot(offset: offset, at: Date(), device: Self.deviceLabel)
        mine = all
        let dir = folder(root)
        let data = try? JSONEncoder().encode(all)
        let write = {
            try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
            try? data?.write(to: dir.appendingPathComponent("\(LibraryModel.deviceID).json"), options: .atomic)
        }
        if wait { Self.writer.sync(execute: write) } else { Self.writer.async(execute: write) }
    }

    /// The reader went to this place (saved at `spot.at`).
    func restored(_ paper: String, _ spot: ReadingSpot) { restoredAt[paper] = spot.at }

    /// The most recent position from any device.
    func latest(_ paper: String, root: URL) async -> ReadingSpot? {
        let dir = folder(root)
        let ownName = "\(LibraryModel.deviceID).json"
        let fromFiles = await Task.detached(priority: .userInitiated) { () -> ReadingSpot? in
            let fm = FileManager.default
            guard let files = try? fm.contentsOfDirectory(at: dir, includingPropertiesForKeys: nil) else { return nil }
            // other devices' files: placeholders not downloaded yet, or local copies that iCloud
            // knows are out of date (the other device read further). Ask for all of them at once
            // and wait for them together, briefly.
            let urls = Set(files.compactMap { f -> URL? in
                let u = f.pathExtension == "icloud" && f.lastPathComponent.hasPrefix(".") ? CloudLibrary.realURL(forPlaceholder: f.path) : f
                return u.pathExtension == "json" ? u : nil
            }).sorted { $0.path < $1.path }
            let others = urls.filter { $0.lastPathComponent != ownName }
            others.forEach(CloudLibrary.startDownload)
            let deadline = Date().addingTimeInterval(2)
            while Date() < deadline, others.contains(where: { !Self.isCurrent($0) }) {
                try? await Task.sleep(for: .milliseconds(150))
            }
            var best: ReadingSpot?
            for u in urls where CloudLibrary.isDownloaded(u) {     // (a dataless file would block the read)
                if let s = Self.read(u)[paper], s.at > (best?.at ?? .distantPast) { best = s }
            }
            return best
        }.value
        // this device's own place, which may still be on its way to the file
        if let m = own(root)[paper], m.at > (fromFiles?.at ?? .distantPast) { return m }
        return fromFiles
    }

    /// True when the file is here and is iCloud's newest version (or isn't in iCloud).
    nonisolated private static func isCurrent(_ url: URL) -> Bool {
        var u = URL(fileURLWithPath: url.path)
        u.removeAllCachedResourceValues()
        guard FileManager.default.fileExists(atPath: u.path) else { return false }
        guard let v = try? u.resourceValues(forKeys: [.isUbiquitousItemKey, .ubiquitousItemDownloadingStatusKey]),
              v.isUbiquitousItem == true else { return true }
        return v.ubiquitousItemDownloadingStatus == .current
    }

    nonisolated static func read(_ url: URL) -> [String: ReadingSpot] {
        guard let d = try? Data(contentsOf: url) else { return [:] }
        return (try? JSONDecoder().decode([String: ReadingSpot].self, from: d)) ?? [:]
    }
}

extension ReaderAssets {
    static let positionJS = #"""
    (function(){
      function nodes(){
        var out = [], w = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT, null), n;
        while ((n = w.nextNode())) out.push(n);
        return out;
      }
      // character offset of the first text visible near the top of the screen
      window.luntaReadingOffset = function(){
        var ns = nodes(), pos = 0;
        // pages: the exact character at the start of the page, so the place survives new page sizes
        // scroll: the character in the first line under the top edge (the middle of the line, clear of margins)
        // (a figure or the gap between paragraphs at that point: a little lower, then off-centre,
        // before falling back to the start of the first visible paragraph, which can be far above)
        var origin = window.luntaPageOrigin && window.luntaPageOrigin();
        var w = window.innerWidth, points = origin ? [origin] : [];
        if (!origin) [64, 84, 110, 140, 180, 230].forEach(function(y){ [0.5, 0.3, 0.7].forEach(function(x){ points.push({ x: w * x, y: y }); }); });
        for (var p = 0; p < points.length && document.caretRangeFromPoint; p++) {
          var c = document.caretRangeFromPoint(points[p].x, points[p].y);
          if (!c || c.startContainer.nodeType !== 3 || !c.startContainer.nodeValue.trim()) continue;
          pos = 0;
          for (var j = 0; j < ns.length; j++) { if (ns[j] === c.startContainer) return pos + c.startOffset; pos += ns[j].nodeValue.length; }
        }
        pos = 0;
        for (var i = 0; i < ns.length; i++) {
          var n = ns[i], len = n.nodeValue.length;
          if (n.nodeValue.trim()) {
            var r = document.createRange(); r.selectNodeContents(n);
            var b = r.getBoundingClientRect();
            if (b.height > 0 && b.bottom > 40 && b.right > 1 && b.left < window.innerWidth) return pos;
          }
          pos += len;
        }
        return pos;
      };
      window.luntaScrollToOffset = function(off){
        var ns = nodes(), pos = 0;
        for (var i = 0; i < ns.length; i++) {
          var len = ns[i].nodeValue.length;
          if (pos + len > off) {
            var el = ns[i].parentElement; if (!el) return false;
            if (window.luntaPaged && window.luntaPaged()) {
              var r = document.createRange(), k = Math.max(0, Math.min(len - 1, off - pos));
              r.setStart(ns[i], k); r.setEnd(ns[i], Math.min(len, k + 1));
              var rs = r.getClientRects();
              return window.luntaShowRect(rs.length ? rs[0] : el.getBoundingClientRect(), false);
            }
            // the line holding that exact character, just under the top edge
            var cr = document.createRange(), ck = Math.max(0, Math.min(len - 1, off - pos));
            cr.setStart(ns[i], ck); cr.setEnd(ns[i], Math.min(len, ck + 1));
            var crs = cr.getClientRects(), box = crs.length ? crs[0] : el.getBoundingClientRect();
            var top = box.top + window.scrollY - 60;
            window.scrollTo({ top: Math.max(0, top), behavior: 'instant' });
            return true;
          }
          pos += len;
        }
        return false;
      };
      // goes to the offset and keeps it there while figures and tables load and push the text down
      // (and, in pages, while the pages are laid out again for the real screen size), until you
      // touch or scroll (or after 6 s)
      var held = null;
      window.luntaHoldOffset = function(off){
        if (window.luntaHold) window.luntaHold();
        held = off;
        var ok = window.luntaScrollToOffset(off);
        // pages: a new layout goes back to this place, not to the page shown before
        if (window.luntaKeepPlace) window.luntaKeepPlace(off);
        var stopped = false, again = function(){ if (!stopped) { window.luntaScrollToOffset(off); if (window.luntaKeepPlace) window.luntaKeepPlace(off); } };
        var ro = window.ResizeObserver ? new ResizeObserver(again) : null;
        if (ro) ro.observe(document.body);
        // pages grow sideways inside a body of fixed size, which ResizeObserver doesn't see
        var root = document.documentElement, size = root.scrollWidth + 'x' + root.scrollHeight;
        var poll = setInterval(function(){ var s = root.scrollWidth + 'x' + root.scrollHeight; if (s !== size) { size = s; again(); } }, 200);
        var imgs = Array.prototype.slice.call(document.images);
        imgs.forEach(function(i){ if (!i.complete) i.addEventListener('load', again); });
        var events = ['touchstart', 'wheel', 'keydown', 'mousedown'];
        var stop = function(){
          if (stopped) return; stopped = true; held = null;
          if (ro) ro.disconnect();
          clearInterval(poll);
          imgs.forEach(function(i){ i.removeEventListener('load', again); });
          events.forEach(function(e){ window.removeEventListener(e, stop, true); });
          if (window.luntaHold === stop) window.luntaHold = null;
        };
        events.forEach(function(e){ window.addEventListener(e, stop, true); });
        setTimeout(stop, 6000);
        window.luntaHold = stop;
        return ok;
      };
      // the place to save: while holding, the place being held (the page may not have settled);
      // in pages, the place the pages are kept at (the page's first word would creep back);
      // otherwise the first text on the screen
      window.luntaPlace = function(){
        if (held != null) return held;
        var a = window.luntaPageAnchor && window.luntaPageAnchor();
        return a != null ? a : window.luntaReadingOffset();
      };
    })();
    """#
}

extension ReaderController {
    func readingOffset() async -> Int? {
        (try? await webView?.evaluateJavaScript("luntaPlace()")) as? Int
    }
    func scroll(toOffset o: Int) {
        webView?.evaluateJavaScript("luntaHoldOffset(\(o))")
    }
}

import SwiftUI
#if os(iOS)
import UIKit
#else
import AppKit
#endif

/// Restores and keeps saving the reading position of one paper (used by both readers).
@MainActor
final class PositionKeeper: ObservableObject {
    @Published var note: String?
    private var started = false
    /// Saving starts only once the page is at the saved place (or there was none): before that
    /// the page shows its top, and saving that would wipe the place on every device.
    private var ready = false
    /// The place last measured (after the restore), saved as is when the reader closes or the app
    /// is left: by then the web view may be gone or too slow to answer.
    private var last: Int?
    private var paper: String?
    private var root: URL?
    private weak var controller: ReaderController?
    private var saver: Task<Void, Never>?
    private var quitting: NSObjectProtocol?
    /// Called once the page is at its place (restored, or there was none to restore).
    var onSettled: (() -> Void)?

    init() {
        #if os(macOS)
        quitting = NotificationCenter.default.addObserver(forName: NSApplication.willTerminateNotification, object: nil, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.saveLast(wait: true) }
        }
        #endif
    }

    deinit { if let quitting { NotificationCenter.default.removeObserver(quitting) } }

    func pageReady(_ paper: String, model: LibraryModel, controller: ReaderController) {
        self.paper = paper; self.root = model.libraryURL; self.controller = controller
        let again = started
        started = true
        ready = false
        saver?.cancel()
        saver = Task { [weak self, weak controller] in
            // the page loaded again (its web process was closed): back to the place it was at
            if again, let o = self?.last {
                try? await Task.sleep(for: .milliseconds(350))
                guard !Task.isCancelled else { return }
                controller?.scroll(toOffset: o)
            } else if let spot = await ReadingPositions.shared.latest(paper, root: model.libraryURL) {
                guard !Task.isCancelled else { return }
                ReadingPositions.shared.restored(paper, spot)
                if spot.offset > 300 {
                    try? await Task.sleep(for: .milliseconds(350))   // let figures and tables settle first
                    guard !Task.isCancelled else { return }
                    posLog("restore \(spot.offset) from \(spot.device)")
                    controller?.scroll(toOffset: spot.offset)
                    self?.show(spot.device == ReadingPositions.deviceLabel
                               ? "Continuing where you left off"
                               : "Continuing where you left off on your \(spot.device)")
                }
                self?.last = spot.offset
            }
            guard !Task.isCancelled else { return }
            self?.ready = true
            self?.onSettled?()
            await self?.keepSaving()
        }
    }

    /// The place is measured every second (kept for closing and leaving the app) and saved every third.
    private func keepSaving() async {
        var tick = 0
        while !Task.isCancelled {
            try? await Task.sleep(for: .seconds(1))
            guard !Task.isCancelled, let paper, let root, let o = await controller?.readingOffset(), !Task.isCancelled else { continue }
            last = o
            tick += 1
            if tick % 3 == 0 { ReadingPositions.shared.save(paper, offset: o, root: root) }
        }
    }

    /// The reader is showing again after `stop` without loading the page again (a full-screen
    /// figure covered it): carry on saving from where it was.
    func resume() {
        guard started, saver == nil, last != nil else { return }
        ready = true
        saver = Task { [weak self] in await self?.keepSaving() }
    }

    private func show(_ text: String) {
        note = text
        Task { try? await Task.sleep(for: .seconds(3)); withAnimation { note = nil } }
    }

    /// The last place measured, saved right away (`wait`: written before returning).
    private func saveLast(wait: Bool = false) {
        guard ready, let paper, let root, let last else { return }
        ReadingPositions.shared.save(paper, offset: last, root: root, force: true, wait: wait)
    }

    /// Measures the place once more and saves it (the web view is held on to until it answers).
    private func saveFresh(then done: @escaping () -> Void = {}) {
        guard ready, let paper, let root, let v = controller?.webView else { done(); return }
        Task {
            if let o = (try? await v.evaluateJavaScript("luntaPlace()")) as? Int, self.ready || self.saver == nil {
                self.last = o
                ReadingPositions.shared.save(paper, offset: o, root: root, force: true)
            }
            done()
        }
    }

    /// Now (leaving the app: iOS may close it in the background).
    func saveNow(_ paper: String, model: LibraryModel, controller: ReaderController) {
        posLog("leaving the app: ready=\(ready) last=\(last.map(String.init) ?? "-")")
        saveLast(wait: true)
        #if os(iOS)
        let app = UIApplication.shared
        var task = UIBackgroundTaskIdentifier.invalid
        task = app.beginBackgroundTask(withName: "Save reading place") { app.endBackgroundTask(task); task = .invalid }
        saveFresh { if task != .invalid { app.endBackgroundTask(task); task = .invalid } }
        #else
        saveFresh()
        #endif
    }

    func stop(_ paper: String, model: LibraryModel, controller: ReaderController) {
        saver?.cancel()
        saveLast()
        saveFresh()
        saver = nil
        ready = false
    }
}

struct ResumeNote: View {
    let text: String?
    var body: some View {
        if let text {
            Label(text, systemImage: "bookmark.fill")
                .font(.footnote.weight(.medium))
                .padding(.horizontal, 14).padding(.vertical, 8)
                .background(.regularMaterial, in: Capsule())
                .padding(.top, 10)
                .transition(.move(edge: .top).combined(with: .opacity))
        }
    }
}

func posLog(_ s: String) {
    #if DEBUG
    if ProcessInfo.processInfo.arguments.contains("-posTrace") { print("POS keeper: " + s); fflush(stdout) }
    #endif
}
