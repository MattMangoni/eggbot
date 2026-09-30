// Draws the app icon (the eggbot egg on a dark macOS tile) to the PNG path given as the first argument.
import AppKit

let size = 1024.0
let ctx = CGContext(data: nil, width: Int(size), height: Int(size), bitsPerComponent: 8, bytesPerRow: 0, space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
// y grows downward, like egg.rs
ctx.translateBy(x: 0, y: size)
ctx.scaleBy(x: 1, y: -1)

func gray(_ v: Double, _ a: Double = 1) -> CGColor { CGColor(srgbRed: v, green: v, blue: v, alpha: a) }

// the macOS icon grid: an 824pt tile with a soft drop shadow (shadow offsets ignore the flip)
let tile = CGPath(roundedRect: CGRect(x: 100, y: 100, width: 824, height: 824), cornerWidth: 185, cornerHeight: 185, transform: nil)
ctx.saveGState()
ctx.setShadow(offset: CGSize(width: 0, height: -10), blur: 28, color: gray(0, 0.35))
ctx.addPath(tile)
ctx.setFillColor(gray(0.09))
ctx.fillPath()
ctx.restoreGState()
ctx.saveGState()
ctx.addPath(tile)
ctx.clip()
let shade = CGGradient(colorsSpace: nil, colors: [gray(0.17), gray(0.07)] as CFArray, locations: [0, 1])!
ctx.drawLinearGradient(shade, start: CGPoint(x: 0, y: 100), end: CGPoint(x: 0, y: 924), options: [])
ctx.restoreGState()

// the egg from egg.rs, in unit coordinates (x 0..1, y 0..1.3)
let w = 400.0
let (ox, oy) = ((size - w) / 2, (size - w * 1.3) / 2 + 12)
func at(_ x: Double, _ y: Double) -> CGPoint { CGPoint(x: ox + x * w, y: oy + y * w) }
let egg = CGMutablePath()
egg.move(to: at(0.5, 0))
egg.addCurve(to: at(1, 0.82), control1: at(0.76, 0), control2: at(1, 0.4))
egg.addCurve(to: at(0.5, 1.3), control1: at(1, 1.12), control2: at(0.8, 1.3))
egg.addCurve(to: at(0, 0.82), control1: at(0.2, 1.3), control2: at(0, 1.12))
egg.addCurve(to: at(0.5, 0), control1: at(0, 0.4), control2: at(0.24, 0))
egg.closeSubpath()
ctx.addPath(egg)
ctx.setFillColor(CGColor(srgbRed: 0.96, green: 0.94, blue: 0.90, alpha: 1))
ctx.fillPath()

for x in [0.36, 0.64] {
    let c = at(x, 0.8)
    let (rx, ry) = (w * 0.06, w * 0.078)
    ctx.addEllipse(in: CGRect(x: c.x - rx, y: c.y - ry, width: rx * 2, height: ry * 2))
}
ctx.setFillColor(gray(0.09))
ctx.fillPath()

let rep = NSBitmapImageRep(cgImage: ctx.makeImage()!)
try! rep.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: CommandLine.arguments[1]))
