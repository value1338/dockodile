import CoreGraphics
import Foundation

struct PackageDisplay {
    let id: String?
    let visibleFrame: CGRect
}

/// The saved monitor and the group's offset from its usable top-left corner, in AX coordinates.
struct PackagePosition: Codable {
    let displayID: String
    let offset: CGPoint

    static func capture(_ frame: CGRect, on displays: [PackageDisplay]) -> Self? {
        guard let display = bestDisplay(for: frame, in: displays),
              overlap(frame, display.visibleFrame) > 0, let id = display.id else { return nil }
        return Self(displayID: id, offset: CGPoint(x: frame.minX - display.visibleFrame.minX,
                                                   y: frame.minY - display.visibleFrame.minY))
    }

    static func restore(_ frame: CGRect, position: Self?, on displays: [PackageDisplay], fallback: CGRect?) -> CGRect {
        if let position {
            if let display = displays.first(where: { $0.id == position.displayID }) {
                let bounds = display.visibleFrame
                let origin = CGPoint(x: bounds.minX + position.offset.x, y: bounds.minY + position.offset.y)
                return fit(frame.size, in: bounds, at: origin)
            }
        } else if let display = bestDisplay(for: frame, in: displays),
                  overlap(frame, display.visibleFrame) >= 0.3 * frame.width * frame.height {
            // Older packages have only global coordinates. Preserve their original fallback behavior.
            return fit(frame.size, in: display.visibleFrame, at: frame.origin)
        }
        guard let bounds = fallback ?? displays.first?.visibleFrame else { return frame }
        return fit(frame.size, in: bounds, at: nil)
    }

    private static func bestDisplay(for frame: CGRect, in displays: [PackageDisplay]) -> PackageDisplay? {
        displays.max { overlap(frame, $0.visibleFrame) < overlap(frame, $1.visibleFrame) }
    }

    private static func overlap(_ frame: CGRect, _ bounds: CGRect) -> CGFloat {
        let shared = frame.intersection(bounds)
        return shared.isNull ? 0 : shared.width * shared.height
    }

    private static func fit(_ size: CGSize, in bounds: CGRect, at origin: CGPoint?) -> CGRect {
        let width = min(max(size.width, 320), bounds.width)
        let height = min(max(size.height, 240), bounds.height)
        let x = origin.map { min(max($0.x, bounds.minX), bounds.maxX - width) } ?? (bounds.midX - width / 2)
        let y = origin.map { min(max($0.y, bounds.minY), bounds.maxY - height) } ?? (bounds.midY - height / 2)
        return CGRect(x: x, y: y, width: width, height: height)
    }
}
