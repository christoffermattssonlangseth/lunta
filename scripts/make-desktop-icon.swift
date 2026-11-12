// Makes the desktop app's 1024 px icon from the Mac app's artwork, on the macOS icon grid:
// the artwork clipped to an 824 × 824 rounded rectangle centred in a transparent 1024 canvas,
// with a subtle drop shadow. (Xcode masks asset-catalog icons itself; the Tauri bundles use the
// PNG / .icns / .ico as they are, so the shape has to be in the image.)
//
//   swift scripts/make-desktop-icon.swift
//   cd desktop && npx tauri icon src-tauri/icons/app-icon-1024.png -o src-tauri/icons
//
// (then delete the generated android/, ios/ and Square*/StoreLogo files, which aren't used)

import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers

let repo = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent()
let input = repo.appendingPathComponent("apple/Assets.xcassets/AppIcon.appiconset/ios-1024.png")
let output = repo.appendingPathComponent("desktop/src-tauri/icons/app-icon-1024.png")

guard let src = CGImageSourceCreateWithURL(input as CFURL, nil),
      let art = CGImageSourceCreateImageAtIndex(src, 0, nil) else { fatalError("can't read \(input.path)") }

let canvas = 1024.0, tile = 824.0, radius = 185.0   // Apple's macOS icon grid
let origin = (canvas - tile) / 2
let ctx = CGContext(data: nil, width: Int(canvas), height: Int(canvas), bitsPerComponent: 8, bytesPerRow: 0,
                    space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
let rect = CGRect(x: origin, y: origin, width: tile, height: tile)
let shape = CGPath(roundedRect: rect, cornerWidth: radius, cornerHeight: radius, transform: nil)

// shadow: drawn by filling the tile once with a shadow set (CoreGraphics' y axis points up)
ctx.saveGState()
ctx.setShadow(offset: CGSize(width: 0, height: -10), blur: 28, color: CGColor(gray: 0, alpha: 0.32))
ctx.addPath(shape)
ctx.setFillColor(CGColor(gray: 0.1, alpha: 1))
ctx.fillPath()
ctx.restoreGState()

// the artwork, scaled into the tile and clipped to it
ctx.saveGState()
ctx.addPath(shape)
ctx.clip()
ctx.draw(art, in: rect)
ctx.restoreGState()

let img = ctx.makeImage()!
let dest = CGImageDestinationCreateWithURL(output as CFURL, UTType.png.identifier as CFString, 1, nil)!
CGImageDestinationAddImage(dest, img, nil)
guard CGImageDestinationFinalize(dest) else { fatalError("can't write \(output.path)") }
print("wrote \(output.path)")
