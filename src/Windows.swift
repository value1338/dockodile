import AppKit
import ApplicationServices

// Private but long-stable: the window server id behind an accessibility window.
// Without it a window could only be told apart by its frame, and grouped windows share one.
@_silgen_name("_AXUIElementGetWindow")
@discardableResult
func _AXUIElementGetWindow(_ element: AXUIElement, _ id: UnsafeMutablePointer<CGWindowID>) -> AXError

/// Accessibility and CGWindow coordinates start at the top left of the main screen, AppKit at the bottom left.
func flipped(_ rect: CGRect) -> CGRect {
    let height = NSScreen.screens.first?.frame.height ?? 0
    return CGRect(x: rect.minX, y: height - rect.maxY, width: rect.width, height: rect.height)
}

func flipped(_ point: CGPoint) -> CGPoint {
    CGPoint(x: point.x, y: (NSScreen.screens.first?.frame.height ?? 0) - point.y)
}

extension AXUIElement {
    func raw(_ attribute: String) -> CFTypeRef? {
        var value: CFTypeRef?
        guard AXUIElementCopyAttributeValue(self, attribute as CFString, &value) == .success else { return nil }
        return value
    }

    private func element(_ attribute: String) -> AXUIElement? {
        guard let value = raw(attribute), CFGetTypeID(value) == AXUIElementGetTypeID() else { return nil }
        return (value as! AXUIElement)
    }

    var windowID: CGWindowID? {
        var id: CGWindowID = 0
        return _AXUIElementGetWindow(self, &id) == .success && id != 0 ? id : nil
    }

    var pid: pid_t? {
        var pid: pid_t = 0
        return AXUIElementGetPid(self, &pid) == .success ? pid : nil
    }

    var title: String { raw(kAXTitleAttribute) as? String ?? "" }
    var isStandardWindow: Bool { raw(kAXSubroleAttribute) as? String == kAXStandardWindowSubrole }
    var isMinimized: Bool { raw(kAXMinimizedAttribute) as? Bool ?? false }
    var windows: [AXUIElement] { raw(kAXWindowsAttribute) as? [AXUIElement] ?? [] }
    var children: [AXUIElement] { raw(kAXChildrenAttribute) as? [AXUIElement] ?? [] }
    var focusedWindow: AXUIElement? { element(kAXFocusedWindowAttribute) }

    /// The window an element belongs to, or the element itself when it already is one.
    var enclosingWindow: AXUIElement? {
        if raw(kAXRoleAttribute) as? String == kAXWindowRole { return self }
        return element(kAXWindowAttribute)
    }

    var frame: CGRect? {
        guard let position = raw(kAXPositionAttribute), let size = raw(kAXSizeAttribute),
              CFGetTypeID(position) == AXValueGetTypeID(), CFGetTypeID(size) == AXValueGetTypeID() else { return nil }
        var origin = CGPoint.zero
        var extent = CGSize.zero
        guard AXValueGetValue(position as! AXValue, .cgPoint, &origin),
              AXValueGetValue(size as! AXValue, .cgSize, &extent) else { return nil }
        return CGRect(origin: origin, size: extent)
    }

    func setFrame(_ rect: CGRect) {
        var origin = rect.origin
        var extent = rect.size
        guard let position = AXValueCreate(.cgPoint, &origin), let size = AXValueCreate(.cgSize, &extent) else { return }
        // Size, position, size: a window moving to a smaller screen may refuse the size until it has arrived.
        AXUIElementSetAttributeValue(self, kAXSizeAttribute as CFString, size)
        AXUIElementSetAttributeValue(self, kAXPositionAttribute as CFString, position)
        AXUIElementSetAttributeValue(self, kAXSizeAttribute as CFString, size)
    }

    func setPosition(_ point: CGPoint) {
        var origin = point
        guard let position = AXValueCreate(.cgPoint, &origin) else { return }
        AXUIElementSetAttributeValue(self, kAXPositionAttribute as CFString, position)
    }

    /// Brings this window to the front, and its app with it.
    func focus() {
        AXUIElementSetAttributeValue(self, kAXMainAttribute as CFString, kCFBooleanTrue)
        AXUIElementPerformAction(self, kAXRaiseAction as CFString)
        if let pid {
            AXUIElementSetAttributeValue(AXUIElementCreateApplication(pid), kAXFrontmostAttribute as CFString,
                                         kCFBooleanTrue)
            // Asked a second way as well: some apps answer only one of the two.
            NSRunningApplication(processIdentifier: pid)?.activate(options: [])
        }
    }

    static func window(at point: CGPoint) -> AXUIElement? {
        var element: AXUIElement?
        let system = AXUIElementCreateSystemWide()
        guard AXUIElementCopyElementAtPosition(system, Float(point.x), Float(point.y), &element) == .success else {
            return nil
        }
        return element?.enclosingWindow
    }

    static func window(pid: pid_t, id: CGWindowID) -> AXUIElement? {
        AXUIElementCreateApplication(pid).windows.first { $0.windowID == id }
    }
}
