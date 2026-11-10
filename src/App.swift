import AppKit
import ApplicationServices
import Carbon.HIToolbox
import ServiceManagement
import SwiftUI

@MainActor
final class AccessState: ObservableObject {
    @Published var trusted = AXIsProcessTrusted()
}

@main
@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate, NSMenuDelegate {
    private var status: NSStatusItem?
    private var welcome: NSWindow?
    private let access = AccessState()
    private var waiting: Timer?
    private var termination: DispatchSourceSignal?
    private var recorder: NSWindow?
    private var keyMonitor: Any?
    private let recording = RecorderState()

    static func main() {
        let app = NSApplication.shared
        let delegate = AppDelegate()
        app.delegate = delegate
        app.setActivationPolicy(.accessory)
        app.run()
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        // Packages saved while the app was still called Klette.
        if UserDefaults.standard.data(forKey: "packages") == nil,
           let old = UserDefaults(suiteName: "de.lakecode.klette")?.data(forKey: "packages") {
            UserDefaults.standard.set(old, forKey: "packages")
        }
        // The build script ends Dockodile with a plain kill. Turn that into a regular quit,
        // so parked windows are handed back before the process goes.
        signal(SIGTERM, SIG_IGN)
        let source = DispatchSource.makeSignalSource(signal: SIGTERM, queue: .main)
        source.setEventHandler { DispatchQueue.main.async { NSApp.terminate(nil) } }
        source.resume()
        termination = source
        let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
        if let image = NSImage(named: "MenuBar") {
            // A template image: macOS colours it to suit a light or dark menu bar.
            image.isTemplate = true
            image.accessibilityDescription = "Dockodile"
            item.button?.image = image
        } else {
            item.button?.image = NSImage(systemSymbolName: "rectangle.stack", accessibilityDescription: "Dockodile")
        }
        // The menu is filled each time it opens, see menuNeedsUpdate.
        let menu = NSMenu()
        menu.autoenablesItems = false
        menu.delegate = self
        item.menu = menu
        status = item

        HotKeys.shared.update()
        if access.trusted {
            Hub.shared.start()
            if !UserDefaults.standard.bool(forKey: "welcomed") { showWelcome() }
        } else {
            // Asks macOS to list Dockodile under Accessibility; the user still has to switch it on.
            let prompt = kAXTrustedCheckOptionPrompt.takeUnretainedValue() as String
            AXIsProcessTrustedWithOptions([prompt: true] as CFDictionary)
            showWelcome()
            waiting = Timer.scheduledTimer(withTimeInterval: 1, repeats: true) { _ in
                DispatchQueue.main.async { self.checkAccess() }
            }
        }
    }

    /// Control without clicking, for agents, Raycast and Shortcuts:
    /// `dockodile://open?paket=Name` opens a package, `dockodile://tab?app=Mail` shows a tab.
    func application(_ application: NSApplication, open urls: [URL]) {
        for url in urls where url.scheme == "dockodile" {
            let items = URLComponents(url: url, resolvingAgainstBaseURL: false)?.queryItems ?? []
            func value(_ names: String...) -> String? { items.first { names.contains($0.name) }?.value }
            var done = false
            if url.host == "open", let name = value("paket", "package", "name"),
               let package = Package.load().first(where: { $0.name.caseInsensitiveCompare(name) == .orderedSame }) {
                Hub.shared.open(package)
                done = true
            } else if url.host == "tab", let name = value("app", "name"), !name.isEmpty {
                done = Hub.shared.showTab(name)
            }
            if !done { NSSound.beep() }
        }
    }

    func applicationWillTerminate(_ notification: Notification) {
        Hub.shared.dissolveAll()
    }

    private func checkAccess() {
        guard AXIsProcessTrusted() else { return }
        waiting?.invalidate()
        waiting = nil
        access.trusted = true
        Hub.shared.start()
    }

    @objc private func showWelcome() {
        UserDefaults.standard.set(true, forKey: "welcomed")
        if welcome == nil {
            let window = NSWindow(contentRect: .zero, styleMask: [.titled, .closable, .fullSizeContentView],
                                  backing: .buffered, defer: false)
            window.titlebarAppearsTransparent = true
            window.title = ""
            window.isReleasedWhenClosed = false
            window.contentView = NSHostingView(rootView: Welcome(
                state: access,
                openSettings: {
                    let url = URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")!
                    NSWorkspace.shared.open(url)
                },
                close: { [weak window] in window?.close() }))
            window.setContentSize(window.contentView?.fittingSize ?? NSSize(width: 520, height: 380))
            window.center()
            welcome = window
        }
        NSApp.activate(ignoringOtherApps: true)
        welcome?.makeKeyAndOrderFront(nil)
    }

    func menuNeedsUpdate(_ menu: NSMenu) {
        menu.removeAllItems()
        @discardableResult
        func add(_ title: String, _ action: Selector?, in parent: NSMenu, enabled: Bool = true) -> NSMenuItem {
            let item = parent.addItem(withTitle: title, action: action, keyEquivalent: "")
            item.target = self
            item.isEnabled = enabled && action != nil
            return item
        }
        let packages = Package.load()
        if !packages.isEmpty {
            add(t("Pakete", "Packages"), nil, in: menu)
            for (index, package) in packages.enumerated() {
                let item = add(package.name, #selector(openPackage(_:)), in: menu, enabled: access.trusted)
                item.tag = index
                item.indentationLevel = 1
                if let key = package.hotKey {
                    // Shown here, registered system wide in HotKeys.
                    if key.character.isEmpty {
                        item.title += "   " + key.label
                    } else {
                        item.keyEquivalent = key.character
                        var mask: NSEvent.ModifierFlags = []
                        if key.modifiers & UInt32(controlKey) != 0 { mask.insert(.control) }
                        if key.modifiers & UInt32(optionKey) != 0 { mask.insert(.option) }
                        if key.modifiers & UInt32(shiftKey) != 0 { mask.insert(.shift) }
                        if key.modifiers & UInt32(cmdKey) != 0 { mask.insert(.command) }
                        item.keyEquivalentModifierMask = mask
                    }
                }
                item.toolTip = package.apps
                    .map { (($0.path as NSString).lastPathComponent as NSString).deletingPathExtension }
                    .joined(separator: ", ")
            }
        } else {
            add(t("Pakete sicherst du am Plus einer Tab-Leiste", "Save packages from the plus on a tab bar"), nil, in: menu)
        }
        if !packages.isEmpty {
            let keys = add(t("Kürzel ändern", "Change shortcut"), nil, in: menu)
            keys.isEnabled = true
            let choices = NSMenu()
            choices.autoenablesItems = false
            for (index, package) in packages.enumerated() {
                let label = package.hotKey?.label ?? t("kein Kürzel", "no shortcut")
                add(package.name + "   " + label, #selector(changeKey(_:)), in: choices).tag = index
            }
            keys.submenu = choices
            let remove = add(t("Paket löschen", "Delete package"), nil, in: menu)
            remove.isEnabled = true
            let submenu = NSMenu()
            submenu.autoenablesItems = false
            for (index, package) in packages.enumerated() {
                add(package.name, #selector(deletePackage(_:)), in: submenu).tag = index
            }
            remove.submenu = submenu
        }
        menu.addItem(.separator())
        add(t("Anleitung …", "How it works …"), #selector(showWelcome), in: menu)
        add(t("Alle Gruppen auflösen", "Dissolve all groups"), #selector(dissolve), in: menu, enabled: !Hub.shared.groups.isEmpty)
        menu.addItem(.separator())
        let language = add(t("Sprache", "Language"), nil, in: menu)
        language.isEnabled = true
        let languages = NSMenu()
        languages.autoenablesItems = false
        for (index, entry) in [("Deutsch", Lang.de), ("English", Lang.en)].enumerated() {
            let item = add(entry.0, #selector(setLanguage(_:)), in: languages)
            item.tag = index
            item.state = Lang.current == entry.1 ? .on : .off
        }
        language.submenu = languages
        add(t("Beim Anmelden starten", "Start at login"), #selector(toggleLogin), in: menu).state =
            SMAppService.mainApp.status == .enabled ? .on : .off
        menu.addItem(.separator())
        add(t("Dockodile beenden", "Quit Dockodile"), #selector(quit), in: menu)
    }

    // MARK: Packages

    @objc private func openPackage(_ item: NSMenuItem) {
        let packages = Package.load()
        guard packages.indices.contains(item.tag) else { return }
        Hub.shared.open(packages[item.tag])
    }

    // MARK: Recording a shortcut

    @objc private func changeKey(_ item: NSMenuItem) {
        let packages = Package.load()
        guard packages.indices.contains(item.tag), keyMonitor == nil else { return }
        recording.name = packages[item.tag].name
        recording.current = packages[item.tag].hotKey?.label ?? t("kein Kürzel", "no shortcut")
        recording.message = t("Drück jetzt die gewünschte Tastenkombination. Esc bricht ab.", "Press the key combination you want. Esc cancels.")
        recording.warning = false
        if recorder == nil {
            // No close button: the window ends through its own buttons, so the key monitor is always removed.
            let window = NSWindow(contentRect: .zero, styleMask: [.titled, .fullSizeContentView],
                                  backing: .buffered, defer: false)
            window.titlebarAppearsTransparent = true
            window.isReleasedWhenClosed = false
            window.contentView = NSHostingView(rootView: Recorder(
                state: recording,
                remove: { [weak self] in self?.store(nil) },
                cancel: { [weak self] in self?.endRecording() }))
            window.setContentSize(window.contentView?.fittingSize ?? NSSize(width: 460, height: 200))
            window.center()
            recorder = window
        }
        // While recording, Dockodile's own shortcuts are off, or pressing one would open its package.
        HotKeys.shared.suspend()
        keyMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
            self?.record(event)
            return nil
        }
        NSApp.activate(ignoringOtherApps: true)
        recorder?.makeKeyAndOrderFront(nil)
    }

    private func record(_ event: NSEvent) {
        if event.keyCode == UInt16(kVK_Escape) {
            endRecording()
            return
        }
        var modifiers: UInt32 = 0
        var label = ""
        if event.modifierFlags.contains(.control) { modifiers |= UInt32(controlKey); label += "⌃" }
        if event.modifierFlags.contains(.option) { modifiers |= UInt32(optionKey); label += "⌥" }
        if event.modifierFlags.contains(.shift) { modifiers |= UInt32(shiftKey); label += "⇧" }
        if event.modifierFlags.contains(.command) { modifiers |= UInt32(cmdKey); label += "⌘" }
        let named = [36: "↩", 48: "⇥", 49: t("Leertaste", "Space"), 51: "⌫", 123: "←", 124: "→", 125: "↓", 126: "↑",
                     122: "F1", 120: "F2", 99: "F3", 118: "F4", 96: "F5", 97: "F6", 98: "F7", 100: "F8",
                     101: "F9", 109: "F10", 103: "F11", 111: "F12"]
        var character = ""
        if let name = named[Int(event.keyCode)] {
            label += name
        } else if let plain = event.charactersIgnoringModifiers, plain.count == 1 {
            label += plain.uppercased()
            character = plain.lowercased()
        } else {
            label += t("Taste \(event.keyCode)", "Key \(event.keyCode)")
        }
        let key = HotKey(code: UInt32(event.keyCode), modifiers: modifiers, label: label, character: character)
        if let problem = HotKeys.shared.conflict(key, for: recording.name) {
            recording.message = problem
            recording.warning = true
            return
        }
        store(key)
    }

    private func store(_ key: HotKey?) {
        var packages = Package.load()
        if let index = packages.firstIndex(where: { $0.name == recording.name }) {
            packages[index].hotKey = key
            Package.save(packages)
        }
        endRecording()
    }

    private func endRecording() {
        if let keyMonitor { NSEvent.removeMonitor(keyMonitor) }
        keyMonitor = nil
        recorder?.orderOut(nil)
        HotKeys.shared.update()
    }

    @objc private func deletePackage(_ item: NSMenuItem) {
        var packages = Package.load()
        guard packages.indices.contains(item.tag) else { return }
        packages.remove(at: item.tag)
        Package.save(packages)
        HotKeys.shared.update()
    }

    /// Registers or removes Dockodile as a login item. macOS lists it under General, Login Items as well.
    @objc private func toggleLogin() {
        let service = SMAppService.mainApp
        do {
            if service.status == .enabled {
                try service.unregister()
            } else {
                try service.register()
            }
        } catch {
            let alert = NSAlert()
            alert.messageText = t("Der Autostart ließ sich nicht ändern", "Could not change the login item")
            alert.informativeText = error.localizedDescription
                + t("\n\nDu kannst Dockodile auch von Hand eintragen: Systemeinstellungen, Allgemein, Anmeldeobjekte.", "\n\nYou can add Dockodile by hand as well: System Settings, General, Login Items.")
            NSApp.activate(ignoringOtherApps: true)
            alert.runModal()
        }
    }

    @objc private func setLanguage(_ item: NSMenuItem) {
        LanguageState.shared.current = item.tag == 0 ? .de : .en
    }

    @objc private func dissolve() {
        Hub.shared.dissolveAll()
    }

    @objc private func quit() {
        NSApp.terminate(nil)
    }
}

/// Switches the name field off while an existing package is chosen in the save dialog.
@MainActor
private final class SaveChoice: NSObject {
    weak var field: NSTextField?

    @objc func changed(_ sender: NSPopUpButton) {
        field?.isEnabled = sender.indexOfSelectedItem == 0
    }
}

/// Saves the group's apps and its place: as a new package under a name, or over an existing one.
/// Window content is not stored.
@MainActor
func savePackage(of group: TabGroup) {
    let apps = Hub.shared.describe(group)
    guard apps.count > 1 else { return }
    var packages = Package.load()
    let names = apps.map { (($0.path as NSString).lastPathComponent as NSString).deletingPathExtension }

    let alert = NSAlert()
    alert.messageText = t("Gruppe als Paket sichern", "Save group as package")
    alert.informativeText = t("Enthält ", "Contains ") + names.joined(separator: ", ") + "."
    let field = NSTextField(frame: NSRect(x: 0, y: 0, width: 300, height: 24))
    field.placeholderString = t("Name für das neue Paket", "Name for the new package")
    let popup = NSPopUpButton(frame: NSRect(x: 0, y: 34, width: 300, height: 26))
    popup.addItem(withTitle: t("Neues Paket anlegen", "Create new package"))
    for package in packages { popup.addItem(withTitle: t("„\(package.name)“ aktualisieren", "Update “\(package.name)”")) }

    // Offer the package this group came from, otherwise the one that shares the most apps with it.
    let wanted = Set(apps.map(\.bundleID))
    var likely = packages.firstIndex { $0.name == group.package }
    if likely == nil {
        let shared = packages.map { Set($0.apps.map(\.bundleID)).intersection(wanted).count }
        if let most = shared.max(), most > 0 { likely = shared.firstIndex(of: most) }
    }
    popup.selectItem(at: likely.map { $0 + 1 } ?? 0)
    let choice = SaveChoice()
    choice.field = field
    popup.target = choice
    popup.action = #selector(SaveChoice.changed(_:))
    field.isEnabled = popup.indexOfSelectedItem == 0

    let box = NSView(frame: NSRect(x: 0, y: 0, width: 300, height: packages.isEmpty ? 24 : 60))
    if !packages.isEmpty { box.addSubview(popup) }
    box.addSubview(field)
    alert.accessoryView = box
    alert.addButton(withTitle: t("Sichern", "Save"))
    alert.addButton(withTitle: t("Abbrechen", "Cancel"))
    let first: NSView = field.isEnabled ? field : popup
    alert.window.initialFirstResponder = first
    NSApp.activate(ignoringOtherApps: true)
    let answer = withExtendedLifetime(choice) { alert.runModal() }
    guard answer == .alertFirstButtonReturn else { return }

    let picked = packages.isEmpty ? 0 : popup.indexOfSelectedItem
    if picked > 0 {
        // Same name, same shortcut, same place in the menu. Only apps and position are replaced.
        let old = packages[picked - 1]
        packages[picked - 1] = Package(name: old.name, apps: apps, frame: group.frame, hotKey: old.hotKey)
        group.package = old.name
    } else {
        let name = field.stringValue.trimmingCharacters(in: .whitespaces)
        guard !name.isEmpty else { return }
        if let same = packages.firstIndex(where: { $0.name == name }) {
            packages[same] = Package(name: name, apps: apps, frame: group.frame, hotKey: packages[same].hotKey)
        } else {
            packages.append(Package(name: name, apps: apps, frame: group.frame,
                                    hotKey: HotKeys.shared.suggestion(for: name)))
        }
        group.package = name
    }
    Package.save(packages)
    HotKeys.shared.update()
}

private let hotKeyHandler: EventHandlerUPP = { _, event, _ in
    var key = EventHotKeyID()
    GetEventParameter(event, EventParamName(kEventParamDirectObject), EventParamType(typeEventHotKeyID), nil,
                      MemoryLayout<EventHotKeyID>.size, nil, &key)
    let index = Int(key.id)
    DispatchQueue.main.async {
        let packages = Package.load()
        if packages.indices.contains(index) { Hub.shared.open(packages[index]) }
    }
    return noErr
}

/// Registers each package's shortcut system wide and judges whether a new one is free.
@MainActor
final class HotKeys {
    static let shared = HotKeys()
    private let signature = OSType(0x4B4C_5454)
    private var registered: [EventHotKeyRef] = []
    private var installed = false

    /// Drops every shortcut, for the time a new one is being recorded.
    func suspend() {
        for key in registered { UnregisterEventHotKey(key) }
        registered = []
    }

    func update() {
        suspend()
        if !installed {
            var spec = EventTypeSpec(eventClass: OSType(kEventClassKeyboard), eventKind: UInt32(kEventHotKeyPressed))
            InstallEventHandler(GetApplicationEventTarget(), hotKeyHandler, 1, &spec, nil, nil)
            installed = true
        }
        for (index, package) in Package.load().enumerated() {
            guard let key = package.hotKey else { continue }
            var reference: EventHotKeyRef?
            let id = EventHotKeyID(signature: signature, id: UInt32(index))
            if RegisterEventHotKey(key.code, key.modifiers, id, GetApplicationEventTarget(), 0, &reference) == noErr,
               let reference {
                registered.append(reference)
            }
        }
    }

    /// Shortcuts macOS itself has switched on, such as Spotlight or Mission Control.
    private func systemUses(_ key: HotKey) -> Bool {
        var array: Unmanaged<CFArray>?
        guard CopySymbolicHotKeys(&array) == noErr,
              let list = array?.takeRetainedValue() as? [[String: Any]] else { return false }
        return list.contains { entry in
            (entry[kHISymbolicHotKeyEnabled as String] as? Bool) == true
                && (entry[kHISymbolicHotKeyCode as String] as? NSNumber)?.uint32Value == key.code
                && (entry[kHISymbolicHotKeyModifiers as String] as? NSNumber)?.uint32Value == key.modifiers
        }
    }

    /// Why this combination cannot be used, or nil when it is free as far as macOS lets an app see.
    /// Call while suspended, so Dockodile's own registrations do not get in the way.
    /// ponytail: macOS has no list of other apps' shortcuts. A trial registration catches those it refuses;
    /// a tool that listens for keys by other means stays invisible.
    func conflict(_ key: HotKey, for name: String) -> String? {
        if key.modifiers & UInt32(controlKey | optionKey) == 0 {
            return t("Nimm Control oder Option dazu. Ohne eine der beiden würde das Kürzel Befehle der Programme selbst überschreiben.", "Add Control or Option. Without one of them the shortcut would override commands of the apps themselves.")
        }
        let twin = Package.load().first {
            $0.name != name && $0.hotKey?.code == key.code && $0.hotKey?.modifiers == key.modifiers
        }
        if let twin {
            return t("Halt, stopp: Dieses Kürzel gehört schon zum Paket „\(twin.name)“.", "Hold on: this shortcut already belongs to the package “\(twin.name)”.")
        }
        if systemUses(key) {
            return t("Halt, stopp: Dieses Kürzel ist von macOS belegt. Du findest es in den Systemeinstellungen unter Tastatur, Tastaturkurzbefehle.", "Hold on: macOS uses this shortcut. You will find it in System Settings under Keyboard, Keyboard Shortcuts.")
        }
        var reference: EventHotKeyRef?
        let status = RegisterEventHotKey(key.code, key.modifiers, EventHotKeyID(signature: signature, id: 9_999),
                                         GetApplicationEventTarget(), 0, &reference)
        if let reference { UnregisterEventHotKey(reference) }
        if status != noErr {
            return t("Halt, stopp: Dieses Kürzel ist schon vergeben, von macOS oder einem anderen Programm.", "Hold on: this shortcut is already taken, by macOS or another app.")
        }
        return nil
    }

    /// Control-Option-1 to 9, the first that is still free, as a starting point for a new package.
    func suggestion(for name: String) -> HotKey? {
        suspend()
        defer { update() }
        let digits = [kVK_ANSI_1, kVK_ANSI_2, kVK_ANSI_3, kVK_ANSI_4, kVK_ANSI_5, kVK_ANSI_6, kVK_ANSI_7,
                      kVK_ANSI_8, kVK_ANSI_9]
        for (index, code) in digits.enumerated() {
            let key = HotKey(code: UInt32(code), modifiers: UInt32(controlKey | optionKey),
                             label: "⌃⌥\(index + 1)", character: "\(index + 1)")
            if conflict(key, for: name) == nil { return key }
        }
        return nil
    }
}
