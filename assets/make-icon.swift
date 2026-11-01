// Превращает квадратную картинку в иконку macOS: скруглённая плашка с полями и тенью.
//   swift make-icon.swift icon-source.png out.png
import AppKit

let input = NSImage(contentsOfFile: CommandLine.arguments[1])!
let canvas: CGFloat = 1024
let plate: CGFloat = 824
let inset = (canvas - plate) / 2
let rect = NSRect(x: inset, y: inset + 8, width: plate, height: plate)

let bitmap = NSBitmapImageRep(
    bitmapDataPlanes: nil, pixelsWide: Int(canvas), pixelsHigh: Int(canvas),
    bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
    colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!

NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: bitmap)
let path = NSBezierPath(roundedRect: rect, xRadius: 186, yRadius: 186)

NSGraphicsContext.saveGraphicsState()
let shadow = NSShadow()
shadow.shadowColor = NSColor.black.withAlphaComponent(0.4)
shadow.shadowBlurRadius = 24
shadow.shadowOffset = NSSize(width: 0, height: -10)
shadow.set()
NSColor.black.setFill()
path.fill()
NSGraphicsContext.restoreGraphicsState()

NSGraphicsContext.saveGraphicsState()
path.addClip()
// Символ на исходнике мелковат для маленьких размеров, поэтому берём центр картинки
let side = input.size.width * 0.78
let crop = NSRect(x: (input.size.width - side) / 2, y: (input.size.height - side) / 2, width: side, height: side)
input.draw(in: rect, from: crop, operation: .sourceOver, fraction: 1)
NSGraphicsContext.restoreGraphicsState()
NSGraphicsContext.current = nil

try! bitmap.representation(using: .png, properties: [:])!
    .write(to: URL(fileURLWithPath: CommandLine.arguments[2]))
