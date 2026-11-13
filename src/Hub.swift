import AppKit
import ApplicationServices
import ColorSync
import SwiftUI

let barHeight: CGFloat = 36
/// How far the bar's panel reaches down behind the window. The window's rounded top corners then sit
/// on the bar's glass instead of showing whatever lies behind, and bar and window read as one piece.
let tuck: CGFloat = 22
/// How far below a window's top edge a drop still counts as "onto the title bar".
let dropZone: CGFloat = 46

/// One app of a saved package. The title only helps to pick the right window when an app has several.
struct PackageApp: Codable {
    let bundleID: String
    let path: String
    let title: String
}

/// A system wide keyboard shortcut: Carbon key code and modifiers, plus how to show it.
struct HotKey: Codable, Equatable {
    let code: UInt32
    let modifiers: UInt32
    /// For display, such as ⌃⌥1.
    let label: String
    /// The plain key when it is a single character, so the menu can draw it natively. Empty otherwise.
    let character: String
}

/// A named set of apps that Dockodile opens and tabs together on request.
struct Package: Codable {
    let name: String
    let apps: [PackageApp]
    /// Where the group sat when it was saved: bar and window together, in accessibility coordinates.
    var frame: CGRect?
    var hotKey: HotKey?
    var position: PackagePosition? = nil

    static func load() -> [Package] {
        guard let data = UserDefaults.standard.data(forKey: "packages"),
              let packages = try? JSONDecoder().decode([Package].self, from: data) else { return [] }
        return packages
    }

    static func save(_ packages: [Package]) {
        if let data = try? JSONEncoder().encode(packages) { UserDefaults.standard.set(data, forKey: "packages") }
    }
}

struct Member: Identifiable {
    let id: CGWindowID
    let window: AXUIElement
    let pid: pid_t
    let appName: String
    let icon: NSImage?
    /// Path of the app bundle, to find the app's tile in the Dock.
    let bundlePath: String
    var title: String
    /// What the app shows on its Dock icon, such as the number of unread messages. Empty when nothing.
    var badge = ""

    init?(_ window: AXUIElement) {
        guard let id = window.windowID, let pid = window.pid else { return nil }
        let app = NSRunningApplication(processIdentifier: pid)
        self.id = id
        self.window = window
        self.pid = pid
        appName = app?.localizedName ?? "App"
        icon = app?.icon
        bundlePath = app?.bundleURL?.standardizedFileURL.path ?? ""
        title = window.title
    }
}

/// A tab for an app of a package that is still starting. `at` is the member index it is drawn before.
struct Ghost: Identifiable, Equatable {
    let id: Int
    let at: Int
    let name: String
    let path: String
}

/// Windows that share one frame, with a tab bar on top.
@MainActor
final class TabGroup: ObservableObject {
    @Published var members: [Member]
    @Published var active: Int
    /// True while the hidden tabs wait off screen, which they only do while the group is moved or resized.
    var parked = false
    /// Apps of the package that are still starting, shown as pulsing tabs.
    @Published var starting: [Ghost] = []
    /// Bar and window together, in accessibility coordinates.
    var frame: CGRect
    private(set) var panel: BarPanel!
    /// True while the user carries the group by its tab bar; window notifications are echoes then.
    var moving = false
    /// The frame before a double click on the bar filled the screen, to go back to.
    var restore: CGRect?
    /// The package this group was opened from or last saved as, offered first when saving again.
    var package: String?

    init(frame: CGRect, members: [Member], active: Int) {
        self.frame = frame
        self.members = members
        self.active = active
        panel = BarPanel(group: self)
    }

    var bar: CGRect { CGRect(x: frame.minX, y: frame.minY, width: frame.width, height: barHeight) }
    /// The bar plus the strip tucked behind the window.
    var panelFrame: CGRect { CGRect(x: frame.minX, y: frame.minY, width: frame.width, height: barHeight + tuck) }
    var content: CGRect {
        CGRect(x: frame.minX, y: frame.minY + barHeight, width: frame.width,
               height: max(120, frame.height - barHeight))
    }
}

/// Moves windows off the main thread and skips positions that are already outdated.
/// Asking another app to move is a round trip that can take longer than a frame; done on the main
/// thread it made the tab bar stutter.
final class Mover: @unchecked Sendable {
    private let queue = DispatchQueue(label: "de.lakecode.dockodile.mover", qos: .userInteractive)
    private let lock = NSLock()
    private var pending: (windows: [AXUIElement], origin: CGPoint, size: CGSize?, then: (@Sendable () -> Void)?)?
    private var running = false

    /// With a size the windows are resized as well. `then` runs on the mover's queue once they are in place.
    func move(_ windows: [AXUIElement], to origin: CGPoint, size: CGSize? = nil,
              then: (@Sendable () -> Void)? = nil) {
        lock.lock()
        pending = (windows, origin, size, then)
        let start = !running
        running = true
        lock.unlock()
        guard start else { return }
        queue.async {
            while true {
                self.lock.lock()
                guard let job = self.pending else {
                    self.running = false
                    self.lock.unlock()
                    return
                }
                self.pending = nil
                self.lock.unlock()
                for window in job.windows {
                    if let size = job.size {
                        window.setFrame(CGRect(origin: job.origin, size: size))
                    } else {
                        window.setPosition(job.origin)
                    }
                }
                job.then?()
            }
        }
    }
}

private let observerCallback: AXObserverCallback = { _, element, notification, _ in
    let name = notification as String
    DispatchQueue.main.async { Hub.shared.handle(element, name) }
}

/// Watches window drags, builds groups and keeps every group's windows and tab bar together.
@MainActor
final class Hub {
    static let shared = Hub()
    private(set) var groups: [TabGroup] = []
    private var observers: [pid_t: AXObserver] = [:]
    private var monitors: [Any] = []
    private var timer: Timer?
    private let hint = HintPanel()
    private var lastDragCheck = Date.distantPast
    private var pollCount = 0

    private struct Drag {
        let window: AXUIElement
        let id: CGWindowID
        let start: CGRect
        var moving = false
    }
    private var drag: Drag?
    private var barDrag: (mouse: CGPoint, frame: CGRect)?
    private var barResize: (mouse: CGPoint, frame: CGRect)?
    /// Counts finished bar drags, so a late answer from the mover cannot move the bar back.
    private var barGeneration = 0
    private let mover = Mover()

    /// The user holds the visible window of a group by its own title bar.
    private struct OwnDrag {
        let group: TabGroup
        let mouse: CGPoint
        let frame: CGRect
        let size: CGSize
        var moving = false
    }
    private var ownDrag: OwnDrag?

    /// A window that was pulled out of a tab bar and still hangs on the pointer.
    private struct Pull {
        let member: Member
        weak var origin: TabGroup?
        var ticks = 0
    }
    private var pull: Pull?
    private var pullTimer: Timer?
    /// Where the pointer holds a pulled window, measured from its top left corner.
    private let grip = CGPoint(x: 120, y: 14)

    private enum Target {
        case group(TabGroup)
        case window(AXUIElement, CGRect)

        @MainActor var strip: CGRect {
            switch self {
            case .group(let group): return group.bar
            case .window(_, let frame):
                return CGRect(x: frame.minX, y: frame.minY, width: frame.width, height: dropZone)
            }
        }
    }

    // MARK: Lifecycle

    func start() {
        guard monitors.isEmpty else { return }
        AXUIElementSetMessagingTimeout(AXUIElementCreateSystemWide(), 0.3)
        rescue()
        let down = NSEvent.addGlobalMonitorForEvents(matching: .leftMouseDown) { _ in Hub.shared.mouseDown() }
        let dragged = NSEvent.addGlobalMonitorForEvents(matching: .leftMouseDragged) { _ in Hub.shared.mouseDragged() }
        let up = NSEvent.addGlobalMonitorForEvents(matching: .leftMouseUp) { _ in Hub.shared.mouseUp() }
        monitors = [down, dragged, up].compactMap { $0 }
        NSWorkspace.shared.notificationCenter.addObserver(
            forName: NSWorkspace.didActivateApplicationNotification, object: nil, queue: .main
        ) { _ in
            DispatchQueue.main.async { Hub.shared.syncFocus() }
        }
        timer = Timer.scheduledTimer(withTimeInterval: 0.5, repeats: true) { _ in
            DispatchQueue.main.async { Hub.shared.poll() }
        }
    }

    // MARK: Parking hidden tabs

    /// Hidden tabs lie behind the visible one, at the same frame. Only while the group is moved or resized
    /// do they wait just outside the picture: at the bottom right corner of the rightmost screen, with one
    /// point still showing, because macOS keeps no window entirely off screen. The group then has a single
    /// window to move and nothing is left standing behind it.
    /// They must not stay out there: Mission Control shrinks every window to a speck as soon as some
    /// windows sit almost entirely off screen.
    private var parkPoint: CGPoint {
        let screen = NSScreen.screens.max { $0.frame.maxX < $1.frame.maxX }
        let frame = flipped(screen?.frame ?? .zero)
        return CGPoint(x: frame.maxX - 1, y: frame.maxY - 1)
    }

    private func isParked(_ window: AXUIElement) -> Bool {
        guard let origin = window.frame?.origin else { return true }
        let point = parkPoint
        return origin.x > point.x - 60 && origin.y > point.y - 60
    }

    private func park(_ group: TabGroup) {
        guard !group.parked else { return }
        group.parked = true
        let point = parkPoint
        for (index, member) in group.members.enumerated() where index != group.active {
            member.window.setPosition(point)
        }
    }

    /// Back behind the visible window, once the group has come to rest.
    private func stack(_ group: TabGroup) {
        group.parked = false
        for (index, member) in group.members.enumerated() where index != group.active {
            if member.window.frame != group.content { member.window.setFrame(group.content) }
        }
    }

    /// Windows a crashed or killed Dockodile left parked come back to the middle of the main screen.
    private func rescue() {
        guard let screen = NSScreen.main else { return }
        let home = flipped(screen.visibleFrame)
        for app in NSWorkspace.shared.runningApplications where app.activationPolicy == .regular {
            for window in AXUIElementCreateApplication(app.processIdentifier).windows
            where window.isStandardWindow && isParked(window) {
                window.setPosition(CGPoint(x: home.minX + 80, y: home.minY + 60))
            }
        }
    }

    /// Gives every window its full frame back and removes all tab bars.
    func dissolveAll() {
        for group in groups { dissolve(group) }
    }

    /// Gives every window of the group back as a window of its own, where the group was.
    func dissolve(_ group: TabGroup) {
        guard groups.contains(where: { $0 === group }) else { return }
        for member in group.members {
            unobserve(member)
            member.window.setFrame(group.frame)
        }
        group.panel.orderOut(nil)
        groups.removeAll { $0 === group }
        // Whatever was still starting for this group must not build it anew.
        opening = opening.filter { $0.value.home !== group }
    }

    /// The X at the end of the bar: asks whether to only dissolve the group or to quit its apps as well.
    func close(_ group: TabGroup) {
        let pids = Set(group.members.map(\.pid))
        let names = Set(group.members.map(\.appName)).sorted().joined(separator: ", ")
        let alert = NSAlert()
        alert.messageText = t("Tab-Gruppe schließen?", "Close tab group?")
        alert.informativeText = t("Enthält \(names). Beim Beenden schließen die Programme ganz, auch Fenster außerhalb der Gruppe. Ungesichertes fragen sie selbst ab.",
                                  "Contains \(names). Quitting closes the apps entirely, including windows outside the group. They ask about unsaved work themselves.")
        alert.addButton(withTitle: t("Nur Gruppe auflösen", "Only dissolve group"))
        alert.addButton(withTitle: t("Programme beenden", "Quit apps"))
        alert.addButton(withTitle: t("Abbrechen", "Cancel"))
        NSApp.activate(ignoringOtherApps: true)
        let answer = alert.runModal()
        guard answer != .alertThirdButtonReturn else { return }
        dissolve(group)
        guard answer == .alertSecondButtonReturn else { return }
        for pid in pids {
            // The Finder is part of the system and stays; its window is simply free again.
            guard let app = NSRunningApplication(processIdentifier: pid),
                  app.bundleIdentifier != "com.apple.finder" else { continue }
            app.terminate()
        }
    }

    // MARK: Dragging one window onto another

    private func mouseDown() {
        drag = nil
        ownDrag = nil
        let mouse = NSEvent.mouseLocation
        guard let window = AXUIElement.window(at: flipped(mouse)), let id = window.windowID,
              let frame = window.frame, window.pid != getpid() else { return }
        if let group = group(of: id) {
            if group.members.indices.contains(group.active), group.members[group.active].id == id {
                ownDrag = OwnDrag(group: group, mouse: mouse, frame: group.frame, size: frame.size)
            }
            return
        }
        guard window.isStandardWindow else { return }
        drag = Drag(window: window, id: id, start: frame)
    }

    private func mouseDragged() {
        if let own = ownDrag {
            // A dragged window follows the pointer exactly, so the pointer says where the group is.
            // That is smoother than waiting for the app to report each step.
            guard own.moving else { return }
            let mouse = NSEvent.mouseLocation
            own.group.frame.origin = CGPoint(x: own.frame.minX + mouse.x - own.mouse.x,
                                             y: own.frame.minY - (mouse.y - own.mouse.y))
            own.group.panel.setFrame(flipped(own.group.panelFrame), display: true)
            return
        }
        guard var current = drag, Date().timeIntervalSince(lastDragCheck) > 0.05 else { return }
        lastDragCheck = Date()
        if !current.moving {
            // Moved without changing size: the window is being carried by its title bar.
            guard let frame = current.window.frame, frame.size == current.start.size,
                  frame.origin != current.start.origin else { return }
            current.moving = true
            drag = current
        }
        if let target = target(at: flipped(NSEvent.mouseLocation), excluding: current.id, from: current.window.pid) {
            hint.show(over: target.strip)
        } else {
            hint.orderOut(nil)
        }
    }

    private func mouseUp() {
        hint.orderOut(nil)
        if let own = ownDrag {
            ownDrag = nil
            if own.moving { settle(own.group) }
            return
        }
        guard let current = drag else { return }
        drag = nil
        guard current.moving, let target = target(at: flipped(NSEvent.mouseLocation), excluding: current.id, from: current.window.pid),
              let dropped = Member(current.window) else { return }
        // Let the app finish its own drag before the window is moved again.
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.08) {
            DispatchQueue.main.async { Hub.shared.join(dropped, target) }
        }
    }

    /// What lies visibly under the pointer, ignoring the window being dragged: a group's tab bar or another
    /// window's top edge. A window of the program `from` belongs to is no target: browsers, the Finder and
    /// terminals have tabs of their own up there, and dragging one of their tabs onto another of their
    /// windows must stay theirs. Holding Shift docks anyway; a group's tab bar always takes the drop.
    private func target(at point: CGPoint, excluding dragged: CGWindowID, from owner: pid_t? = nil) -> Target? {
        let options: CGWindowListOption = [.optionOnScreenOnly, .excludeDesktopElements]
        let list = CGWindowListCopyWindowInfo(options, kCGNullWindowID) as? [[String: Any]] ?? []
        for info in list {
            guard (info[kCGWindowLayer as String] as? NSNumber)?.intValue == 0,
                  let number = (info[kCGWindowNumber as String] as? NSNumber)?.uint32Value, number != dragged,
                  let pid = (info[kCGWindowOwnerPID as String] as? NSNumber)?.int32Value,
                  let bounds = info[kCGWindowBounds as String] as? NSDictionary,
                  let frame = CGRect(dictionaryRepresentation: bounds as CFDictionary),
                  frame.contains(point) else { continue }
            // The list runs front to back, so this is the frontmost other window under the pointer and it
            // alone decides. Whatever lies behind it is covered and must not react through it.
            if pid == getpid() {
                // One of Dockodile's own windows: a tab bar takes the drop, anything else blocks it.
                guard let group = groups.first(where: { $0.panel.windowNumber == Int(number) }) else { return nil }
                return .group(group)
            }
            // Only its top edge accepts a drop.
            guard point.y - frame.minY <= dropZone else { return nil }
            if let group = group(of: number) { return .group(group) }
            if let owner, owner == pid, !NSEvent.modifierFlags.contains(.shift) { return nil }
            guard let window = AXUIElement.window(pid: pid, id: number), window.isStandardWindow,
                  let exact = window.frame else { return nil }
            return .window(window, exact)
        }
        return nil
    }

    private func join(_ dropped: Member, _ target: Target) {
        guard group(of: dropped.id) == nil else { return }
        switch target {
        case .group(let group):
            guard groups.contains(where: { $0 === group }) else { return }
            group.members.append(dropped)
            observe(dropped)
            activate(group, group.members.count - 1)
        case .window(let window, let frame):
            guard let host = Member(window), host.id != dropped.id, group(of: host.id) == nil else { return }
            let group = TabGroup(frame: frame, members: [host, dropped], active: 1)
            groups.append(group)
            observe(host)
            observe(dropped)
            activate(group, 1)
        }
    }

    // MARK: Groups

    private func group(of id: CGWindowID) -> TabGroup? {
        groups.first { $0.members.contains { $0.id == id } }
    }

    private func placeBar(_ group: TabGroup) {
        group.panel.setFrame(flipped(group.panelFrame), display: true)
        guard group.members.indices.contains(group.active) else { return }
        // Directly below its window in the stacking order: other windows cover both alike, and the
        // window lies on top of the tucked strip.
        group.panel.order(.below, relativeTo: Int(group.members[group.active].id))
    }

    /// Raises the group's visible window and its app, with the bar on top of it.
    func bringForward(_ group: TabGroup) {
        guard group.members.indices.contains(group.active) else { return }
        group.members[group.active].window.focus()
        placeBar(group)
    }

    func activate(_ group: TabGroup, _ index: Int) {
        guard group.members.indices.contains(index) else { return }
        group.active = index
        let member = group.members[index]
        member.window.setFrame(group.content)
        member.window.focus()
        placeBar(group)
        stack(group)
    }

    func move(_ group: TabGroup, from: Int, to: Int) {
        guard from != to, group.members.indices.contains(from), group.members.indices.contains(to) else { return }
        let activeID = group.members[group.active].id
        let member = group.members.remove(at: from)
        group.members.insert(member, at: to)
        group.active = group.members.firstIndex { $0.id == activeID } ?? 0
    }

    /// Takes a window out of its group. `at` is where its top left corner should land.
    func detach(_ group: TabGroup, _ id: CGWindowID, at origin: CGPoint? = nil) {
        guard let index = group.members.firstIndex(where: { $0.id == id }) else { return }
        let member = group.members[index]
        let place = origin ?? CGPoint(x: group.frame.minX + 36, y: group.frame.minY + 36)
        remove(group, index)
        member.window.setFrame(CGRect(origin: place, size: group.frame.size))
        member.window.focus()
    }

    private func remove(_ group: TabGroup, _ index: Int) {
        let activeID = group.members.indices.contains(group.active) ? group.members[group.active].id : 0
        unobserve(group.members[index])
        group.members.remove(at: index)
        if group.members.count < 2 {
            // One window is no group: it gets the whole frame back.
            if let last = group.members.first {
                unobserve(last)
                last.window.setFrame(group.frame)
            }
            group.members = []
            group.starting = []
            group.panel.orderOut(nil)
            groups.removeAll { $0 === group }
            // Whatever was still starting for this group must not build it anew.
            opening = opening.filter { $0.value.home !== group }
            return
        }
        let next = group.members.firstIndex { $0.id == activeID } ?? min(index, group.members.count - 1)
        activate(group, next)
    }

    /// The minus in the bar: the shown window goes to the Dock and the bar hides with it. Clicking the
    /// window in the Dock brings both back, see the regular check that follows minimized windows.
    func minimize(_ group: TabGroup) {
        guard group.members.indices.contains(group.active) else { return }
        group.members[group.active].window.minimize()
        group.panel.orderOut(nil)
    }

    /// Double click on the free part of the bar: fill the screen the group is on, or return to the size before.
    func zoom(_ group: TabGroup) {
        let center = flipped(CGPoint(x: group.frame.midX, y: group.frame.midY))
        guard let screen = NSScreen.screens.first(where: { $0.frame.contains(center) }) ?? NSScreen.main else {
            return
        }
        // visibleFrame leaves out the menu bar and the Dock.
        let full = flipped(screen.visibleFrame)
        if near(group.frame, full), let before = group.restore {
            group.frame = before
            group.restore = nil
        } else {
            group.restore = group.frame
            group.frame = full
        }
        group.moving = true
        park(group)
        if group.members.indices.contains(group.active) {
            group.members[group.active].window.setFrame(group.content)
        }
        placeBar(group)
        settle(group)
    }

    // MARK: Pulling a tab out

    func beginPull(_ group: TabGroup, _ id: CGWindowID) {
        guard pull == nil, let index = group.members.firstIndex(where: { $0.id == id }) else { return }
        let member = group.members[index]
        let size = group.frame.size
        let mouse = flipped(NSEvent.mouseLocation)
        remove(group, index)
        member.window.setFrame(CGRect(origin: CGPoint(x: mouse.x - grip.x, y: mouse.y - grip.y), size: size))
        member.window.focus()
        pull = Pull(member: member, origin: group)
        // A timer, not mouse events: the tab that started the drag no longer exists, and with it its gesture.
        let timer = Timer(timeInterval: 1.0 / 60, repeats: true) { _ in
            DispatchQueue.main.async { Hub.shared.pullTick() }
        }
        RunLoop.main.add(timer, forMode: .common)
        pullTimer = timer
    }

    private func pullTarget(_ current: Pull, at mouse: CGPoint) -> Target? {
        guard let found = target(at: mouse, excluding: current.member.id) else { return nil }
        // Right after the pull the pointer still hovers over the old group. Only its bar takes the tab back.
        if case .group(let group) = found, group === current.origin, !group.bar.contains(mouse) { return nil }
        return found
    }

    private func pullTick() {
        guard var current = pull else { return }
        let mouse = flipped(NSEvent.mouseLocation)
        if NSEvent.pressedMouseButtons & 1 == 0 {
            pullTimer?.invalidate()
            pullTimer = nil
            pull = nil
            hint.orderOut(nil)
            if let target = pullTarget(current, at: mouse) { join(current.member, target) }
            return
        }
        mover.move([current.member.window], to: CGPoint(x: mouse.x - grip.x, y: mouse.y - grip.y))
        current.ticks += 1
        pull = current
        if current.ticks % 6 == 0 {
            if let target = pullTarget(current, at: mouse) {
                hint.show(over: target.strip)
            } else {
                hint.orderOut(nil)
            }
        }
    }

    // MARK: Carrying a group by its tab bar

    /// Called for every mouse movement while the empty part of a tab bar is dragged.
    func dragBar(_ group: TabGroup) {
        // A tab that was just pulled out leaves the bar's own drag gesture as the next in line for the
        // same mouse movement. That drag belongs to the loose window, not to the group.
        guard pull == nil else { return }
        let mouse = NSEvent.mouseLocation
        if barDrag == nil {
            barDrag = (mouse, group.frame)
            group.moving = true
            park(group)
        }
        guard let start = barDrag else { return }
        var y = start.frame.minY - (mouse.y - start.mouse.y)
        if NSScreen.screens.count == 1, let screen = NSScreen.screens.first {
            // Windows cannot go under the menu bar, so the bar must not either.
            y = max(screen.frame.height - screen.visibleFrame.maxY, y)
        }
        group.frame.origin = CGPoint(x: start.frame.minX + mouse.x - start.mouse.x, y: y)
        guard group.members.indices.contains(group.active), let panel = group.panel else { return }
        // Bar and window move as one. The bar takes each position only once the window has it, so the two
        // never drift apart, however quickly or slowly the app answers.
        let bar = flipped(group.panelFrame)
        let generation = barGeneration
        mover.move([group.members[group.active].window], to: group.content.origin) {
            DispatchQueue.main.async {
                if Hub.shared.barGeneration == generation { panel.setFrame(bar, display: true) }
            }
        }
    }

    // MARK: Resizing a group by the bar's top corners

    /// Called for every mouse movement while a top corner of the bar is dragged. The bottom edge and the
    /// opposite side stay where they are, as with any window.
    func resizeBar(_ group: TabGroup, left: Bool) {
        guard pull == nil else { return }
        let mouse = NSEvent.mouseLocation
        if barResize == nil {
            barResize = (mouse, group.frame)
            group.moving = true
            park(group)
        }
        guard let start = barResize else { return }
        let minWidth: CGFloat = 360
        let minHeight: CGFloat = 240
        var top = min(start.frame.minY - (mouse.y - start.mouse.y), start.frame.maxY - minHeight)
        if NSScreen.screens.count == 1, let screen = NSScreen.screens.first {
            top = max(screen.frame.height - screen.visibleFrame.maxY, top)
        }
        var frame = start.frame
        frame.origin.y = top
        frame.size.height = start.frame.maxY - top
        let across = mouse.x - start.mouse.x
        if left {
            let edge = min(start.frame.minX + across, start.frame.maxX - minWidth)
            frame.origin.x = edge
            frame.size.width = start.frame.maxX - edge
        } else {
            frame.size.width = max(minWidth, start.frame.width + across)
        }
        group.frame = frame
        guard group.members.indices.contains(group.active), let panel = group.panel else { return }
        let bar = flipped(group.panelFrame)
        let generation = barGeneration
        mover.move([group.members[group.active].window], to: group.content.origin, size: group.content.size) {
            DispatchQueue.main.async {
                if Hub.shared.barGeneration == generation { panel.setFrame(bar, display: true) }
            }
        }
    }

    func endResize(_ group: TabGroup) {
        guard barResize != nil else { return }
        barResize = nil
        barGeneration += 1
        // The app has the last word on its size; settle takes what it actually accepted.
        settle(group)
    }

    /// After a drag: take the visible window's real frame, keep the bar below the menu bar, align everyone.
    private func settle(_ group: TabGroup) {
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.1) {
            DispatchQueue.main.async {
                defer { group.moving = false }
                guard Hub.shared.groups.contains(where: { $0 === group }),
                      group.members.indices.contains(group.active) else { return }
                if let actual = group.members[group.active].window.frame {
                    group.frame = CGRect(x: actual.minX, y: actual.minY - barHeight, width: actual.width,
                                         height: actual.height + barHeight)
                }
                if NSScreen.screens.count == 1, let screen = NSScreen.screens.first {
                    group.frame.origin.y = max(screen.frame.height - screen.visibleFrame.maxY, group.frame.minY)
                }
                group.members[group.active].window.setFrame(group.content)
                Hub.shared.placeBar(group)
                Hub.shared.stack(group)
            }
        }
    }

    func endBarDrag(_ group: TabGroup) {
        guard barDrag != nil else { return }
        barDrag = nil
        barGeneration += 1
        if group.members.indices.contains(group.active) {
            let window = group.members[group.active].window
            window.setFrame(group.content)
            // Also the last word for the mover, in case an older position is still on its way.
            mover.move([window], to: group.content.origin)
        }
        placeBar(group)
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.4) {
            DispatchQueue.main.async {
                group.moving = false
                if Hub.shared.groups.contains(where: { $0 === group }) { Hub.shared.stack(group) }
            }
        }
    }

    // MARK: Following the windows

    private func observe(_ member: Member) {
        var observer = observers[member.pid]
        if observer == nil {
            guard AXObserverCreate(member.pid, observerCallback, &observer) == .success, let created = observer else {
                return
            }
            CFRunLoopAddSource(CFRunLoopGetMain(), AXObserverGetRunLoopSource(created), .defaultMode)
            observers[member.pid] = created
        }
        guard let observer else { return }
        for name in [kAXMovedNotification, kAXResizedNotification, kAXUIElementDestroyedNotification] {
            AXObserverAddNotification(observer, member.window, name as CFString, nil)
        }
    }

    private func unobserve(_ member: Member) {
        guard let observer = observers[member.pid] else { return }
        for name in [kAXMovedNotification, kAXResizedNotification, kAXUIElementDestroyedNotification] {
            AXObserverRemoveNotification(observer, member.window, name as CFString)
        }
    }

    func handle(_ element: AXUIElement, _ notification: String) {
        for group in groups {
            guard let index = group.members.firstIndex(where: { CFEqual($0.window, element) }) else { continue }
            if notification == kAXUIElementDestroyedNotification {
                if let next = sibling(of: group.members[index], in: group), replace(group, index, with: next) {
                    placeBar(group)
                } else {
                    remove(group, index)
                }
            } else if let own = ownDrag, own.group === group, index == group.active {
                if !own.moving {
                    if notification == kAXMovedNotification, element.frame?.size == own.size {
                        // Same size, new place: a title bar drag. From here the pointer drives the group.
                        ownDrag?.moving = true
                        group.moving = true
                        park(group)
                    } else {
                        ownDrag = nil
                    }
                }
            } else if !group.moving, index == group.active, let content = element.frame, content != group.content {
                // The user moved or resized the visible window: the bar follows. The hidden tabs get out
                // of the way until the mouse is released, so none peeks out from behind a smaller window.
                if NSEvent.pressedMouseButtons != 0 { park(group) }
                group.frame = CGRect(x: content.minX, y: content.minY - barHeight, width: content.width,
                                     height: content.height + barHeight)
                placeBar(group)
            }
            return
        }
    }

    /// The user reached a grouped window some other way, such as Cmd-Tab: make its tab the active one.
    func syncFocus() {
        guard let app = NSWorkspace.shared.frontmostApplication,
              let window = AXUIElementCreateApplication(app.processIdentifier).focusedWindow,
              let id = window.windowID else { return }
        if group(of: id) == nil, window.isStandardWindow, let frame = window.frame {
            // The app switched to another of its own tabs inside the grouped window.
            for group in groups where group.members.indices.contains(group.active) {
                if group.members[group.active].pid == app.processIdentifier, near(frame, group.content) {
                    replace(group, group.active, with: window)
                    break
                }
            }
        }
        guard let group = group(of: id), let index = group.members.firstIndex(where: { $0.id == id }) else { return }
        if index != group.active {
            // A parked tab was brought forward, by Cmd-Tab or Mission Control.
            activate(group, index)
        } else {
            placeBar(group)
        }
    }

    // MARK: Apps with their own tabs

    private func near(_ a: CGRect, _ b: CGRect) -> Bool {
        abs(a.minX - b.minX) < 3 && abs(a.minY - b.minY) < 3 && abs(a.width - b.width) < 3
            && abs(a.height - b.height) < 3
    }

    /// Terminal, Finder and others keep every one of their own tabs as a separate window in the same place.
    /// When the grouped one closes or steps back, its neighbour takes over the slot instead of the group falling apart.
    private func sibling(of member: Member, in group: TabGroup) -> AXUIElement? {
        AXUIElementCreateApplication(member.pid).windows.first { window in
            guard let id = window.windowID, id != member.id, self.group(of: id) == nil, window.isStandardWindow,
                  let frame = window.frame else { return false }
            return near(frame, group.content) || isParked(window)
        }
    }

    @discardableResult
    private func replace(_ group: TabGroup, _ index: Int, with window: AXUIElement) -> Bool {
        guard let fresh = Member(window) else { return false }
        unobserve(group.members[index])
        group.members[index] = fresh
        observe(fresh)
        return true
    }

    // MARK: Packages

    private struct Opening {
        let package: Package
        var found: [Member?]
        var launched = Set<String>()
        var tries = 0
        /// The package's group when it is already open and only lacks some of its tabs.
        var home: TabGroup?
        /// A single app added through the plus: it goes to the end and the tab order stays as it is.
        var append = false
        /// Steps in a row with every window in place. Starting apps swap or drop windows for a moment.
        var settled = 0
        /// Apps whose only windows are tabs of this very group already or belong to a package that is
        /// opening right now. A window exists once, so these are left out.
        var skipped = Set<Int>()
    }
    /// One entry per package that is being opened, by package name. Packages open independently of each
    /// other; a second call for the same package while it is still starting does nothing.
    private var opening: [String: Opening] = [:]

    func describe(_ group: TabGroup) -> [PackageApp] {
        group.members.compactMap { member in
            guard let app = NSRunningApplication(processIdentifier: member.pid), let id = app.bundleIdentifier,
                  let url = app.bundleURL else { return nil }
            return PackageApp(bundleID: id, path: url.path, title: member.title)
        }
    }

    /// Starts what is not running yet, waits for the windows and tabs them together.
    func open(_ package: Package) {
        // Already open: bring that group forward instead of starting everything a second time.
        func bundle(_ member: Member) -> String? {
            NSRunningApplication(processIdentifier: member.pid)?.bundleIdentifier
        }
        let wanted = Set(package.apps.map(\.bundleID))
        let home = groups.first { $0.package == package.name }
            ?? groups.first { wanted.isSubset(of: Set($0.members.compactMap(bundle))) }
        // The group that is already there comes forward at once, whatever is still missing or starting.
        if let home { activate(home, home.active) }
        guard opening[package.name] == nil else { return }
        var found = [Member?](repeating: nil, count: package.apps.count)
        if let home {
            // Tabs that are still there count as found, so only the closed ones get opened again.
            var present = home.members
            for (index, entry) in package.apps.enumerated() {
                if let at = present.firstIndex(where: { bundle($0) == entry.bundleID }) {
                    found[index] = present.remove(at: at)
                }
            }
        }
        let state = Opening(package: package, found: found, home: home)
        arrange(state)
        if !found.contains(where: { $0 == nil }) { return }
        opening[package.name] = state
        openStep(package.name)
    }

    private func openStep(_ name: String) {
        guard var state = opening[name] else { return }
        // Windows another package has already picked are off limits as well.
        if state.home != nil {
            // A window that joined and fell out again while its app was still starting is looked for anew.
            for index in state.found.indices {
                if let member = state.found[index], group(of: member.id) == nil { state.found[index] = nil }
            }
            opening[name] = state
        }
        var taken = Set(opening.values.flatMap { $0.found.compactMap { $0?.id } })
        for (index, entry) in state.package.apps.enumerated()
        where state.found[index] == nil && !state.skipped.contains(index) {
            var free: [AXUIElement] = []
            // Windows that are tabs of another group. The saved package leads: it takes them over.
            var borrowed: [AXUIElement] = []
            var elsewhere = false
            for app in NSRunningApplication.runningApplications(withBundleIdentifier: entry.bundleID) {
                let element = AXUIElementCreateApplication(app.processIdentifier)
                // An app that is still starting does not answer; do not wait for it and freeze the bar.
                AXUIElementSetMessagingTimeout(element, 0.3)
                for window in element.windows {
                    guard let id = window.windowID, window.isStandardWindow else { continue }
                    let owner = group(of: id)
                    if taken.contains(id) || (owner != nil && owner === state.home) {
                        elsewhere = true
                    } else if owner != nil {
                        borrowed.append(window)
                    } else {
                        free.append(window)
                    }
                }
            }
            // The window with the remembered title first, otherwise any free one of that app.
            let match = free.first { !entry.title.isEmpty && $0.title == entry.title } ?? free.first
                ?? borrowed.first { !entry.title.isEmpty && $0.title == entry.title } ?? borrowed.first
            if let match, let member = Member(match) {
                if let owner = group(of: member.id), let at = owner.members.firstIndex(where: { $0.id == member.id }) {
                    remove(owner, at)
                }
                state.found[index] = member
                taken.insert(member.id)
            } else if elsewhere {
                // Running, and every window it has is taken. Opening the app again would give no new window.
                state.skipped.insert(index)
            } else if !state.launched.contains(entry.bundleID) {
                // Not running, or running without a window: opening the app gives us one.
                state.launched.insert(entry.bundleID)
                NSWorkspace.shared.openApplication(at: URL(fileURLWithPath: entry.path),
                                                   configuration: NSWorkspace.OpenConfiguration())
            }
        }
        state.tries += 1
        let complete = !state.found.indices.contains { state.found[$0] == nil && !state.skipped.contains($0) }
        // ponytail: fixed wait of thirty seconds for slow starters; whatever is there by then gets grouped.
        state.settled = complete ? state.settled + 1 : 0
        // Complete means: everything has been in place for three seconds.
        let done = state.settled >= 6 || state.tries >= 60
        if state.home.map({ home in !groups.contains { $0 === home } }) ?? true {
            // No group yet: build it as soon as two windows are there, the rest shows as starting.
            let free = state.found.compactMap { $0 }.filter { group(of: $0.id) == nil }
            if done || free.count > 1 {
                assemble(free, of: state.package)
                state.home = free.first.flatMap { group(of: $0.id) }
            }
        }
        arrange(state)
        if done {
            opening[name] = nil
            state.home?.starting = []
            return
        }
        opening[name] = state
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) {
            DispatchQueue.main.async { Hub.shared.openStep(name) }
        }
    }

    /// Brings the package's group in line with what has been found so far: arrived windows join, the
    /// tabs take the saved order, and what is still missing shows as a pulsing tab in its place.
    private func arrange(_ state: Opening) {
        guard let home = state.home, groups.contains(where: { $0 === home }) else { return }
        let activeID = home.members.indices.contains(home.active) ? home.members[home.active].id : nil
        var joined = false
        for member in state.found.compactMap({ $0 }) where group(of: member.id) == nil {
            join(member, .group(home))
            joined = true
        }
        let wanted = state.found.compactMap { $0?.id }
        let ordered = state.append ? home.members : wanted.compactMap { id in home.members.first { $0.id == id } }
            + home.members.filter { !wanted.contains($0.id) }
        if ordered.map(\.id) != home.members.map(\.id) { home.members = ordered }
        var ghosts: [Ghost] = []
        var before = 0
        for (index, entry) in state.package.apps.enumerated() {
            if let member = state.found[index] {
                if home.members.contains(where: { $0.id == member.id }) { before += 1 }
            } else if !state.skipped.contains(index) {
                let name = FileManager.default.displayName(atPath: entry.path).replacingOccurrences(of: ".app", with: "")
                ghosts.append(Ghost(id: index, at: state.append ? home.members.count : before, name: name, path: entry.path))
            }
        }
        if ghosts != home.starting { home.starting = ghosts }
        // The tab that was showing stays the one that shows.
        guard !state.append, let activeID,
              let index = home.members.firstIndex(where: { $0.id == activeID }) else { return }
        if joined { activate(home, index) } else if home.active != index { home.active = index }
    }

    private var packageDisplays: [PackageDisplay] {
        NSScreen.screens.map { screen in
            let number = screen.deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as? NSNumber
            let id = number.flatMap { CGDisplayCreateUUIDFromDisplayID($0.uint32Value)?.takeRetainedValue() }
                .map { CFUUIDCreateString(nil, $0) as String }
            return PackageDisplay(id: id, visibleFrame: flipped(screen.visibleFrame))
        }
    }

    func packagePosition(for frame: CGRect) -> PackagePosition? {
        PackagePosition.capture(frame, on: packageDisplays)
    }

    private func assemble(_ members: [Member], of package: Package) {
        guard members.count > 1, let current = members[0].window.frame else {
            NSSound.beep()
            return
        }
        let frame = PackagePosition.restore(package.frame ?? current, position: package.position,
                                            on: packageDisplays, fallback: NSScreen.main.map { flipped($0.visibleFrame) })
        join(members[1], .window(members[0].window, frame))
        guard let group = group(of: members[0].id) else { return }
        group.package = package.name
        for member in members.dropFirst(2) { join(member, .group(group)) }
        activate(group, 0)
    }

    // MARK: Adding windows from the plus menu

    /// Visible windows of other apps that are in no group yet, frontmost first.
    func candidates() -> [Member] {
        let options: CGWindowListOption = [.optionOnScreenOnly, .excludeDesktopElements]
        let list = CGWindowListCopyWindowInfo(options, kCGNullWindowID) as? [[String: Any]] ?? []
        var windows: [pid_t: [AXUIElement]] = [:]
        var result: [Member] = []
        for info in list {
            guard (info[kCGWindowLayer as String] as? NSNumber)?.intValue == 0,
                  let number = (info[kCGWindowNumber as String] as? NSNumber)?.uint32Value,
                  let pid = (info[kCGWindowOwnerPID as String] as? NSNumber)?.int32Value, pid != getpid(),
                  group(of: number) == nil, !result.contains(where: { $0.id == number }) else { continue }
            if windows[pid] == nil { windows[pid] = AXUIElementCreateApplication(pid).windows }
            guard let window = windows[pid]?.first(where: { $0.windowID == number }), window.isStandardWindow,
                  let member = Member(window) else { continue }
            result.append(member)
        }
        return result
    }

    /// Shows the tab whose app or title contains the name. For `dockodile://tab?app=…`.
    func showTab(_ name: String) -> Bool {
        for group in groups {
            let hit = group.members.firstIndex { $0.appName.localizedCaseInsensitiveContains(name) }
                ?? group.members.firstIndex { $0.title.localizedCaseInsensitiveContains(name) }
            if let hit {
                activate(group, hit)
                return true
            }
        }
        return false
    }

    func add(_ member: Member, to group: TabGroup) {
        join(member, .group(group))
    }

    /// Starts an app picked in the plus list and tabs its window into the group. Runs through the same
    /// steps as a package, so the app shows as a starting tab and cannot be started twice.
    func launch(_ path: String, into group: TabGroup) {
        let key = "+" + path
        guard opening[key] == nil, let id = Bundle(path: path)?.bundleIdentifier else { return }
        let package = Package(name: key, apps: [PackageApp(bundleID: id, path: path, title: "")],
                              frame: nil, hotKey: nil)
        let state = Opening(package: package, found: [nil], home: group, append: true)
        arrange(state)
        opening[key] = state
        openStep(key)
    }

    /// The labels apps put on their Dock icons, by app bundle path. Read from the Dock itself, which is
    /// the one place where every app reports unread counts in the same way.
    private func dockBadges() -> [String: String] {
        guard let dock = NSRunningApplication.runningApplications(withBundleIdentifier: "com.apple.dock").first else {
            return [:]
        }
        var badges: [String: String] = [:]
        for list in AXUIElementCreateApplication(dock.processIdentifier).children {
            for tile in list.children {
                guard let label = tile.raw("AXStatusLabel") as? String, !label.isEmpty,
                      let url = tile.raw(kAXURLAttribute) as? URL else { continue }
                badges[url.standardizedFileURL.path] = label
            }
        }
        return badges
    }

    private func poll() {
        pollCount += 1
        if pollCount % 4 == 0, !groups.isEmpty {
            let badges = dockBadges()
            for group in groups {
                for index in group.members.indices {
                    let badge = badges[group.members[index].bundlePath] ?? ""
                    if group.members[index].badge != badge { group.members[index].badge = badge }
                }
            }
        }
        for group in groups {
            // Windows that vanished without a notification.
            while let gone = group.members.firstIndex(where: { $0.window.isGone }) {
                if let next = sibling(of: group.members[gone], in: group), replace(group, gone, with: next) {
                    continue
                }
                remove(group, gone)
                if group.members.isEmpty { break }
            }
            guard group.members.indices.contains(group.active) else { continue }
            for index in group.members.indices {
                let title = group.members[index].window.title
                if title != group.members[index].title { group.members[index].title = title }
            }
            let active = group.members[group.active]
            let hidden = active.window.isMinimized
                || NSRunningApplication(processIdentifier: active.pid)?.isHidden == true
            if hidden {
                group.panel.orderOut(nil)
                continue
            }
            // Put the group back together after anything that pulled it apart, such as Mission Control.
            // Not while the user is dragging: that would fight the mouse.
            guard !group.moving, NSEvent.pressedMouseButtons == 0 else { continue }
            if let actual = active.window.frame, actual != group.content {
                group.frame = CGRect(x: actual.minX, y: actual.minY - barHeight, width: actual.width,
                                     height: actual.height + barHeight)
            }
            stack(group)
            placeBar(group)
        }
        syncFocus()
    }
}
