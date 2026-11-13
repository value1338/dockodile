import CoreGraphics
import Foundation

@main
struct PackagePositionTests {
    static func main() {
        let laptop = PackageDisplay(id: "laptop", visibleFrame: CGRect(x: 0, y: 25, width: 1280, height: 775))
        let external = PackageDisplay(id: "external", visibleFrame: CGRect(x: 1280, y: -100, width: 1600, height: 900))
        let saved = CGRect(x: 1400, y: 80, width: 900, height: 650)
        let position = PackagePosition.capture(saved, on: [laptop, external])!
        precondition(position.displayID == "external")
        precondition(position.offset == CGPoint(x: 120, y: 180))
        let data = try! JSONEncoder().encode(position)
        let decoded = try! JSONDecoder().decode(PackagePosition.self, from: data)

        let moved = PackageDisplay(id: "external", visibleFrame: CGRect(x: -1600, y: 10, width: 1600, height: 900))
        let other = PackageDisplay(id: "other", visibleFrame: external.visibleFrame)
        precondition(PackagePosition.restore(saved, position: decoded, on: [laptop, moved, other], fallback: laptop.visibleFrame)
            == CGRect(x: -1480, y: 190, width: 900, height: 650))

        // Do not reuse the disconnected display's global coordinates on another external screen.
        let centered = CGRect(x: 190, y: 87.5, width: 900, height: 650)
        precondition(PackagePosition.restore(saved, position: decoded, on: [laptop, other], fallback: laptop.visibleFrame) == centered)
        precondition(PackagePosition.restore(saved, position: decoded, on: [laptop], fallback: laptop.visibleFrame) == centered)

        let smaller = PackageDisplay(id: "external", visibleFrame: CGRect(x: -800, y: 20, width: 800, height: 600))
        precondition(PackagePosition.restore(saved, position: decoded, on: [smaller], fallback: smaller.visibleFrame) == smaller.visibleFrame)

        // Packages saved before 0.3.2 still use their existing coordinates and visible fallback.
        precondition(PackagePosition.restore(saved, position: nil, on: [laptop, external], fallback: laptop.visibleFrame) == saved)
        precondition(PackagePosition.restore(saved, position: nil, on: [laptop], fallback: laptop.visibleFrame) == centered)
        precondition(PackagePosition.restore(saved, position: nil, on: [], fallback: nil) == saved)
        print("PASS package display and placement")
    }
}
