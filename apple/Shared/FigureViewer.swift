import SwiftUI

/// A figure from the reading view, full size: pinch or scroll to zoom, double-tap to zoom in and out.
struct FigureViewer: View {
    let figure: ReaderFigure
    @Environment(\.dismiss) private var dismiss
    @State private var showCaption = true

    var body: some View {
        ZStack(alignment: .bottom) {
            Color.black.ignoresSafeArea()
            ZoomableImage(url: figure.url).ignoresSafeArea()
            if showCaption, !figure.caption.isEmpty {
                ScrollView {
                    Text(figure.caption).font(.callout).foregroundStyle(.white).frame(maxWidth: .infinity, alignment: .leading).padding(14)
                }
                .frame(maxHeight: 160)
                .background(.black.opacity(0.6))
            }
        }
        .overlay(alignment: .topTrailing) {
            HStack(spacing: 10) {
                if !figure.caption.isEmpty {
                    Button { showCaption.toggle() } label: { Image(systemName: showCaption ? "text.bubble.fill" : "text.bubble") }
                }
                Button { dismiss() } label: { Image(systemName: "xmark.circle.fill") }
                    .keyboardShortcut(.cancelAction)
            }
            .font(.title2)
            .foregroundStyle(.white.opacity(0.9))
            .buttonStyle(.plain)
            .padding(16)
        }
        #if os(macOS)
        .frame(minWidth: 900, idealWidth: 1200, minHeight: 650, idealHeight: 850)
        #endif
    }
}

#if os(iOS)
import UIKit

struct ZoomableImage: UIViewRepresentable {
    let url: URL

    func makeUIView(context: Context) -> UIScrollView {
        let sv = UIScrollView()
        sv.delegate = context.coordinator
        sv.maximumZoomScale = 8
        sv.minimumZoomScale = 1
        sv.showsVerticalScrollIndicator = false
        sv.showsHorizontalScrollIndicator = false
        sv.backgroundColor = .black
        let iv = UIImageView(image: UIImage(contentsOfFile: url.path))
        iv.contentMode = .scaleAspectFit
        iv.backgroundColor = .white
        iv.translatesAutoresizingMaskIntoConstraints = true
        sv.addSubview(iv)
        context.coordinator.imageView = iv
        let tap = UITapGestureRecognizer(target: context.coordinator, action: #selector(Coordinator.doubleTap(_:)))
        tap.numberOfTapsRequired = 2
        sv.addGestureRecognizer(tap)
        return sv
    }

    func updateUIView(_ sv: UIScrollView, context: Context) {
        DispatchQueue.main.async { context.coordinator.layout(sv) }
    }

    func makeCoordinator() -> Coordinator { Coordinator() }

    final class Coordinator: NSObject, UIScrollViewDelegate {
        var imageView: UIImageView?
        private var laidOutFor: CGSize = .zero

        func layout(_ sv: UIScrollView) {
            guard let iv = imageView, let img = iv.image, sv.bounds.size != laidOutFor, sv.bounds.width > 0 else { return }
            laidOutFor = sv.bounds.size
            sv.zoomScale = 1
            let s = min(sv.bounds.width / img.size.width, sv.bounds.height / img.size.height)
            iv.frame = CGRect(x: 0, y: 0, width: img.size.width * s, height: img.size.height * s)
            sv.contentSize = iv.frame.size
            center(sv)
        }

        func viewForZooming(in sv: UIScrollView) -> UIView? { imageView }
        func scrollViewDidZoom(_ sv: UIScrollView) { center(sv) }

        private func center(_ sv: UIScrollView) {
            guard let iv = imageView else { return }
            let dx = max(0, (sv.bounds.width - iv.frame.width) / 2)
            let dy = max(0, (sv.bounds.height - iv.frame.height) / 2)
            sv.contentInset = UIEdgeInsets(top: dy, left: dx, bottom: dy, right: dx)
        }

        @objc func doubleTap(_ g: UITapGestureRecognizer) {
            guard let sv = g.view as? UIScrollView, let iv = imageView else { return }
            if sv.zoomScale > 1.01 {
                sv.setZoomScale(1, animated: true)
            } else {
                let p = g.location(in: iv)
                let size = CGSize(width: sv.bounds.width / 3, height: sv.bounds.height / 3)
                sv.zoom(to: CGRect(x: p.x - size.width / 2, y: p.y - size.height / 2, width: size.width, height: size.height), animated: true)
            }
        }
    }
}
#else
import AppKit

struct ZoomableImage: NSViewRepresentable {
    let url: URL

    func makeNSView(context: Context) -> NSScrollView {
        let sv = NSScrollView()
        sv.allowsMagnification = true
        sv.maxMagnification = 8
        sv.minMagnification = 0.2
        sv.hasVerticalScroller = true
        sv.hasHorizontalScroller = true
        sv.backgroundColor = .black
        sv.drawsBackground = true
        let iv = NSImageView()
        iv.image = NSImage(contentsOf: url)
        iv.imageScaling = .scaleProportionallyUpOrDown
        iv.wantsLayer = true
        iv.layer?.backgroundColor = NSColor.white.cgColor
        sv.documentView = iv
        let click = NSClickGestureRecognizer(target: context.coordinator, action: #selector(Coordinator.doubleClick(_:)))
        click.numberOfClicksRequired = 2
        sv.addGestureRecognizer(click)
        context.coordinator.scrollView = sv
        return sv
    }

    func updateNSView(_ sv: NSScrollView, context: Context) {
        DispatchQueue.main.async { context.coordinator.fit() }
    }

    func makeCoordinator() -> Coordinator { Coordinator() }

    final class Coordinator: NSObject {
        weak var scrollView: NSScrollView?
        private var fittedFor: CGSize = .zero

        func fit() {
            guard let sv = scrollView, let iv = sv.documentView as? NSImageView, let img = iv.image else { return }
            let box = sv.contentSize
            guard box.width > 0, box != fittedFor else { return }
            fittedFor = box
            let s = min(box.width / img.size.width, box.height / img.size.height)
            iv.frame = CGRect(x: 0, y: 0, width: img.size.width * s, height: img.size.height * s)
            sv.magnification = 1
        }

        @objc func doubleClick(_ g: NSClickGestureRecognizer) {
            guard let sv = scrollView else { return }
            let p = g.location(in: sv.documentView)
            sv.setMagnification(sv.magnification > 1.01 ? 1 : 3, centeredAt: p)
        }
    }
}
#endif
