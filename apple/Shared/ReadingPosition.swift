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

    #if os(macOS)
    static let deviceLabel = "Mac"
    #else
    static let deviceLabel = "iPhone"
    #endif

    private func folder(_ root: URL) -> URL { root.appendingPathComponent("positions", isDirectory: true) }

    func save(_ paper: String, offset: Int, root: URL) {
        if mine == nil || self.root != root { mine = load(folder(root).appendingPathComponent("\(LibraryModel.deviceID).json")); self.root = root }
        if let old = mine?[paper], abs(old.offset - offset) < 40 { return }
        mine?[paper] = ReadingSpot(offset: offset, at: Date(), device: Self.deviceLabel)
        let dir = folder(root)
        let data = try? JSONEncoder().encode(mine ?? [:])
        Task.detached(priority: .utility) {
            try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
            try? data?.write(to: dir.appendingPathComponent("\(LibraryModel.deviceID).json"), options: .atomic)
        }
    }

    /// The most recent position from any device.
    func latest(_ paper: String, root: URL) async -> ReadingSpot? {
        let dir = folder(root)
        return await Task.detached(priority: .userInitiated) { () -> ReadingSpot? in
            let fm = FileManager.default
            guard let files = try? fm.contentsOfDirectory(at: dir, includingPropertiesForKeys: nil) else { return nil }
            var best: ReadingSpot?
            for f in files {
                var url = f
                if f.lastPathComponent.hasPrefix(".") && f.pathExtension == "icloud" {
                    // not downloaded yet: ask for it and use it next time
                    url = CloudLibrary.realURL(forPlaceholder: f.path)
                    CloudLibrary.startDownload(url)
                    continue
                }
                guard url.pathExtension == "json" else { continue }
                CloudLibrary.startDownload(url)    // refresh for next time
                if let s = Self.read(url)[paper], s.at > (best?.at ?? .distantPast) { best = s }
            }
            return best
        }.value
    }

    private func load(_ url: URL) -> [String: ReadingSpot] { Self.read(url) }

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
      window.marginsReadingOffset = function(){
        var ns = nodes(), pos = 0;
        // pages: the exact character at the start of the page, so the place survives new page sizes
        var o = window.marginsPageOrigin && window.marginsPageOrigin();
        var c = o && document.caretRangeFromPoint ? document.caretRangeFromPoint(o.x, o.y) : null;
        if (c && c.startContainer.nodeType === 3) {
          for (var j = 0; j < ns.length; j++) { if (ns[j] === c.startContainer) return pos + c.startOffset; pos += ns[j].nodeValue.length; }
          pos = 0;
        }
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
      window.marginsScrollToOffset = function(off){
        var ns = nodes(), pos = 0;
        for (var i = 0; i < ns.length; i++) {
          var len = ns[i].nodeValue.length;
          if (pos + len > off) {
            var el = ns[i].parentElement; if (!el) return false;
            if (window.marginsPaged && window.marginsPaged()) {
              var r = document.createRange(), k = Math.max(0, Math.min(len - 1, off - pos));
              r.setStart(ns[i], k); r.setEnd(ns[i], Math.min(len, k + 1));
              var rs = r.getClientRects();
              return window.marginsShowRect(rs.length ? rs[0] : el.getBoundingClientRect(), false);
            }
            var top = el.getBoundingClientRect().top + window.scrollY - 60;
            window.scrollTo({ top: Math.max(0, top), behavior: 'instant' });
            return true;
          }
          pos += len;
        }
        return false;
      };
    })();
    """#
}

extension ReaderController {
    func readingOffset() async -> Int? {
        (try? await webView?.evaluateJavaScript("marginsReadingOffset()")) as? Int
    }
    func scroll(toOffset o: Int) {
        webView?.evaluateJavaScript("marginsScrollToOffset(\(o))")
    }
}

import SwiftUI

/// Restores and keeps saving the reading position of one paper (used by both readers).
@MainActor
final class PositionKeeper: ObservableObject {
    @Published var note: String?
    private var restored = false
    private var saver: Task<Void, Never>?

    func pageReady(_ paper: String, model: LibraryModel, controller: ReaderController) {
        guard !restored else { return }
        restored = true
        Task {
            if let spot = await ReadingPositions.shared.latest(paper, root: model.libraryURL), spot.offset > 300 {
                try? await Task.sleep(for: .milliseconds(350))   // let figures and tables settle first
                controller.scroll(toOffset: spot.offset)
                note = spot.device == ReadingPositions.deviceLabel
                    ? "Continuing where you left off"
                    : "Continuing where you left off on your \(spot.device)"
                try? await Task.sleep(for: .seconds(3))
                withAnimation { note = nil }
            }
        }
        saver?.cancel()
        saver = Task { [weak controller] in
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(6))
                guard let controller, let o = await controller.readingOffset() else { continue }
                ReadingPositions.shared.save(paper, offset: o, root: model.libraryURL)
            }
        }
    }

    func stop(_ paper: String, model: LibraryModel, controller: ReaderController) {
        saver?.cancel()
        Task { if let o = await controller.readingOffset() { ReadingPositions.shared.save(paper, offset: o, root: model.libraryURL) } }
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
