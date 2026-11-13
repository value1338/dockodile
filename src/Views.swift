import AppKit
import SwiftUI

private let accent = Color(red: 1.00, green: 0.36, blue: 0.16)

// MARK: Language

/// German and English, chosen on first launch and in the menu. Until then the system language decides.
enum Lang: String {
    case de, en

    static var current: Lang {
        get {
            if let saved = UserDefaults.standard.string(forKey: "language"), let lang = Lang(rawValue: saved) {
                return lang
            }
            return Locale.preferredLanguages.first?.hasPrefix("de") == true ? .de : .en
        }
        set { UserDefaults.standard.set(newValue.rawValue, forKey: "language") }
    }
}

/// ponytail: two languages side by side in the code. Move to string catalogs when a third one comes.
func t(_ german: String, _ english: String) -> String {
    Lang.current == .de ? german : english
}

/// Lets open views redraw when the language changes. Only ever touched on the main thread.
final class LanguageState: ObservableObject {
    static let shared = LanguageState()
    @Published var current = Lang.current {
        didSet { Lang.current = current }
    }
}

private struct HintLabel: View {
    @ObservedObject private var language = LanguageState.shared

    var body: some View {
        Text(t("Loslassen zum Tabben", "Release to tab"))
            .font(.system(size: 13, weight: .semibold))
            .foregroundStyle(.white)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(accent.opacity(0.88), in: RoundedRectangle(cornerRadius: 12, style: .continuous))
            .padding(3)
    }
}

/// Lets a click act on the first try even though the bar never becomes the key window.
final class FirstClickHost<Content: View>: NSHostingView<Content> {
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
}

final class BarPanel: NSPanel {
    private weak var group: TabGroup?

    init(group: TabGroup) {
        super.init(contentRect: .zero, styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: false)
        self.group = group
        isOpaque = false
        backgroundColor = .clear
        hasShadow = false
        hidesOnDeactivate = false
        isReleasedWhenClosed = false
        // Transient: Mission Control leaves the bar out instead of showing it as a window of its own.
        collectionBehavior = [.transient, .ignoresCycle, .fullScreenNone]
        contentView = FirstClickHost(rootView: TabBar(group: group))
    }

    override var canBecomeKey: Bool { false }
    override var canBecomeMain: Bool { false }

    /// Any click on the bar, on a tab or beside them, brings the whole group forward first, the way a
    /// click on a title bar does. Done here and not in the tab views, so the free part of the bar counts too.
    override func sendEvent(_ event: NSEvent) {
        if event.type == .leftMouseDown, let group { Hub.shared.bringForward(group) }
        super.sendEvent(event)
    }
}

/// Shown while a window hovers over a spot where letting go would tab it.
final class HintPanel: NSPanel {
    init() {
        super.init(contentRect: .zero, styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: false)
        isOpaque = false
        backgroundColor = .clear
        hasShadow = false
        level = .statusBar
        ignoresMouseEvents = true
        hidesOnDeactivate = false
        isReleasedWhenClosed = false
        contentView = NSHostingView(rootView:
            HintLabel())
    }

    func show(over strip: CGRect) {
        setFrame(flipped(strip), display: true)
        orderFrontRegardless()
    }
}

/// Frosted glass that shows what lies behind the bar, in light and dark alike.
private struct Glass: NSViewRepresentable {
    func makeNSView(context: Context) -> NSVisualEffectView {
        let view = NSVisualEffectView()
        view.material = .sidebar
        view.blendingMode = .behindWindow
        // Dockodile is never the active app, and inactive glass would turn flat grey.
        view.state = .active
        return view
    }

    func updateNSView(_ view: NSVisualEffectView, context: Context) {}
}

struct TabBar: View {
    @ObservedObject var group: TabGroup

    var body: some View {
        GeometryReader { geometry in
            let count = max(1, group.members.count + group.starting.count)
            let width = min(230, (geometry.size.width - 12 - 34 - 34 - 50) / CGFloat(count))
            HStack(spacing: 0) {
                ForEach(Array(group.members.enumerated()), id: \.element.id) { index, member in
                    ForEach(group.starting.filter { $0.at == index }) { GhostCell(ghost: $0, width: width) }
                    TabCell(group: group, member: member, index: index, width: width)
                }
                ForEach(group.starting.filter { $0.at >= group.members.count }) { GhostCell(ghost: $0, width: width) }
                PlusButton(group: group)
                // The free part of the bar: drag it to move the group, double click it to fill the screen.
                Color.clear
                    .contentShape(Rectangle())
                    .onTapGesture(count: 2) { Hub.shared.zoom(group) }
                BarButton(symbol: "minus", label: t("Gruppe minimieren", "Minimize group")) {
                    Hub.shared.minimize(group)
                }
                BarButton(symbol: "arrow.up.left.and.arrow.down.right",
                          label: t("Bildschirm füllen oder alte Größe", "Fill screen or restore size")) {
                    Hub.shared.zoom(group)
                }
                CloseButton(group: group)
            }
            .padding(.horizontal, 6)
            // The tabs keep to the visible bar. Below it the panel continues behind the window.
            .frame(height: barHeight)
        }
        .background(Glass())
        .clipShape(UnevenRoundedRectangle(topLeadingRadius: 14, topTrailingRadius: 14, style: .continuous))
        .overlay(alignment: .topLeading) { ResizeCorner(group: group, left: true) }
        .overlay(alignment: .topTrailing) { ResizeCorner(group: group, left: false) }
        // The bar's window carries the group's apps in its title, so the group reads as one object.
        .onChange(of: group.members.map(\.appName), initial: true) { _, names in
            group.panel?.title = "Dockodile: " + names.joined(separator: ", ")
        }
        .contentShape(Rectangle())
        // The free part of the bar carries the whole group. Tabs keep their own drag for sorting and pulling out.
        .gesture(
            DragGesture(minimumDistance: 3, coordinateSpace: .global)
                .onChanged { _ in Hub.shared.dragBar(group) }
                .onEnded { _ in Hub.shared.endBarDrag(group) }
        )
    }
}

/// What the plus offers: open windows that are in no group yet, and from two typed letters on every
/// installed app. Picking an app starts it and tabs its window into the group.
@MainActor
final class AddModel: ObservableObject {
    enum Kind {
        case window(Member)
        case app(String)
        case save
    }
    struct Row: Identifiable {
        let id: String
        let icon: NSImage
        let title: String
        let kind: Kind
    }

    @Published var query = "" { didSet { if query != oldValue { refresh() } } }
    @Published var rows: [Row] = []
    @Published var selected = 0
    private weak var group: TabGroup?
    private var members: [Member] = []
    private var apps: [(name: String, file: String, path: String)] = []

    var height: CGFloat { 41 + CGFloat(max(1, rows.count)) * 28 + 8 }

    func start(_ group: TabGroup) {
        self.group = group
        members = Hub.shared.candidates()
        apps = Self.installed()
        query = ""
        refresh()
    }

    /// Every app in the Applications folders, one folder level deep, with its shown and its file name.
    private static func installed() -> [(name: String, file: String, path: String)] {
        let files = FileManager.default
        var result: [(name: String, file: String, path: String)] = []
        func add(_ path: String) {
            let file = ((path as NSString).lastPathComponent as NSString).deletingPathExtension
            let name = files.displayName(atPath: path).replacingOccurrences(of: ".app", with: "")
            result.append((name, file, path))
        }
        func scan(_ folder: String, deeper: Bool) {
            for item in (try? files.contentsOfDirectory(atPath: folder)) ?? [] where !item.hasPrefix(".") {
                let path = folder + "/" + item
                if item.hasSuffix(".app") { add(path) } else if deeper { scan(path, deeper: false) }
            }
        }
        scan("/Applications", deeper: true)
        scan("/System/Applications", deeper: true)
        scan(NSHomeDirectory() + "/Applications", deeper: true)
        add("/System/Library/CoreServices/Finder.app")
        return result
    }

    private func refresh() {
        let text = query.trimmingCharacters(in: .whitespaces)
        let searching = text.count >= 2
        var found: [Row] = []
        for member in members {
            let title = member.title.isEmpty ? member.appName : member.appName + " · " + member.title
            guard !searching || title.localizedCaseInsensitiveContains(text) else { continue }
            found.append(Row(id: "w\(member.id)", icon: member.icon ?? NSImage(), title: title, kind: .window(member)))
        }
        if searching {
            let lower = text.lowercased()
            let hits = apps
                .filter { $0.name.localizedCaseInsensitiveContains(text) || $0.file.localizedCaseInsensitiveContains(text) }
                .sorted { first, second in
                    let a = first.name.lowercased().hasPrefix(lower)
                    let b = second.name.lowercased().hasPrefix(lower)
                    if a != b { return a }
                    return first.name.localizedCaseInsensitiveCompare(second.name) == .orderedAscending
                }
            // ponytail: eight hits are enough for a list you narrow by typing.
            for app in hits.prefix(8) {
                found.append(Row(id: "a" + app.path, icon: NSWorkspace.shared.icon(forFile: app.path),
                                 title: app.name, kind: .app(app.path)))
            }
        } else {
            let icon = NSImage(systemSymbolName: "square.and.arrow.down", accessibilityDescription: nil) ?? NSImage()
            found.append(Row(id: "save", icon: icon,
                             title: t("Diese Gruppe als Paket sichern …", "Save this group as a package …"), kind: .save))
        }
        rows = found
        selected = 0
        AddPanel.shared.fit()
    }

    func step(_ by: Int) {
        guard !rows.isEmpty else { return }
        selected = min(max(selected + by, 0), rows.count - 1)
    }

    func pick(_ index: Int) {
        guard rows.indices.contains(index), let group else { return }
        let kind = rows[index].kind
        AddPanel.shared.orderOut(nil)
        switch kind {
        case .window(let member): Hub.shared.add(member, to: group)
        case .app(let path): Hub.shared.launch(path, into: group)
        case .save: savePackage(of: group)
        }
    }
}

/// The list behind the plus. A panel of its own instead of a menu, so that it can take typing while
/// another app stays the active one.
final class AddPanel: NSPanel {
    static let shared = AddPanel()
    let model = AddModel()
    private var anchor = CGPoint.zero

    init() {
        super.init(contentRect: .zero, styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: false)
        isOpaque = false
        backgroundColor = .clear
        hasShadow = true
        level = .popUpMenu
        hidesOnDeactivate = false
        isReleasedWhenClosed = false
        collectionBehavior = [.transient, .ignoresCycle]
    }

    override var canBecomeKey: Bool { true }

    /// A click anywhere else closes the list, as with a menu. Losing key covers other apps; the monitors
    /// cover Dockodile's own tab bars, which never take key and so would not close it.
    override func resignKey() {
        super.resignKey()
        orderOut(nil)
    }

    private var monitors: [Any] = []

    override func orderOut(_ sender: Any?) {
        super.orderOut(sender)
        monitors.forEach(NSEvent.removeMonitor)
        monitors = []
    }

    func dismissOutside() {
        guard isVisible, !frame.contains(NSEvent.mouseLocation) else { return }
        orderOut(nil)
    }

    func show(for group: TabGroup) {
        anchor = NSEvent.mouseLocation
        contentView = NSHostingView(rootView: AddList(model: model))
        model.start(group)
        makeKeyAndOrderFront(nil)
        guard monitors.isEmpty else { return }
        let clicks: NSEvent.EventTypeMask = [.leftMouseDown, .rightMouseDown]
        if let local = NSEvent.addLocalMonitorForEvents(matching: clicks, handler: { event in
            DispatchQueue.main.async { AddPanel.shared.dismissOutside() }
            return event
        }) { monitors.append(local) }
        if let global = NSEvent.addGlobalMonitorForEvents(matching: clicks, handler: { _ in
            DispatchQueue.main.async { AddPanel.shared.dismissOutside() }
        }) { monitors.append(global) }
    }

    /// Grows downwards from where the plus was clicked.
    func fit() {
        let height = model.height
        let width: CGFloat = 400
        // Below the pointer, and pushed back inside when the plus sits near a screen edge.
        var x = anchor.x
        var y = anchor.y - 12 - height
        if let screen = NSScreen.screens.first(where: { $0.frame.contains(anchor) })?.visibleFrame ?? NSScreen.main?.visibleFrame {
            x = max(screen.minX + 8, min(x, screen.maxX - width - 8))
            y = max(screen.minY + 8, min(y, screen.maxY - height - 8))
        }
        setFrame(NSRect(x: x, y: y, width: width, height: height), display: true)
    }
}

private struct AddList: View {
    @ObservedObject var model: AddModel
    @FocusState private var focused: Bool

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 8) {
                Image(systemName: "magnifyingglass").foregroundStyle(.secondary)
                TextField(t("Programm suchen …", "Search apps …"), text: $model.query)
                    .textFieldStyle(.plain)
                    .font(.system(size: 14))
                    .focused($focused)
                    .onSubmit { model.pick(model.selected) }
                    .onKeyPress(.downArrow) { model.step(1); return .handled }
                    .onKeyPress(.upArrow) { model.step(-1); return .handled }
                    .onKeyPress(.escape) { AddPanel.shared.orderOut(nil); return .handled }
            }
            .padding(.horizontal, 12)
            .frame(height: 40)
            Divider()
            if model.rows.isEmpty {
                Text(t("Nichts gefunden", "Nothing found"))
                    .font(.system(size: 13))
                    .foregroundStyle(.secondary)
                    .frame(height: 28)
            }
            ForEach(Array(model.rows.enumerated()), id: \.element.id) { index, row in
                let chosen = index == model.selected
                HStack(spacing: 8) {
                    Image(nsImage: row.icon).resizable().frame(width: 16, height: 16)
                    Text(row.title).font(.system(size: 13)).lineLimit(1)
                    Spacer(minLength: 0)
                }
                .padding(.horizontal, 8)
                .frame(height: 28)
                .foregroundStyle(chosen ? Color.white : Color.primary)
                .background(chosen ? Color.accentColor.opacity(0.85) : Color.clear,
                            in: RoundedRectangle(cornerRadius: 6, style: .continuous))
                .padding(.horizontal, 5)
                .contentShape(Rectangle())
                .onHover { if $0 { model.selected = index } }
                .onTapGesture { model.pick(index) }
            }
            Spacer(minLength: 0)
        }
        .frame(width: 400)
        .background(Glass())
        .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
        .onAppear { focused = true }
    }
}

/// The bar is the group's top edge, so its two top corners resize the group like a window's corners do.
private struct ResizeCorner: View {
    let group: TabGroup
    let left: Bool
    @State private var inside = false

    var body: some View {
        Color.clear
            .frame(width: 11, height: 11)
            .contentShape(Rectangle())
            .onHover { hovering in
                guard hovering != inside else { return }
                inside = hovering
                if hovering {
                    if #available(macOS 15.0, *) {
                        NSCursor.frameResize(position: left ? .topLeft : .topRight, directions: .all).push()
                    } else {
                        NSCursor.crosshair.push()
                    }
                } else {
                    NSCursor.pop()
                }
            }
            .gesture(
                DragGesture(minimumDistance: 1, coordinateSpace: .global)
                    .onChanged { _ in Hub.shared.resizeBar(group, left: left) }
                    .onEnded { _ in Hub.shared.endResize(group) }
            )
    }
}

/// The X at the right end of the bar closes the whole group, after asking how.
/// A small round button at the right end of the bar, beside the close button.
private struct BarButton: View {
    let symbol: String
    let label: String
    let action: () -> Void
    @State private var hovering = false

    var body: some View {
        Button(action: action) {
            Image(systemName: symbol)
                .font(.system(size: 10, weight: .semibold))
                .foregroundStyle(.secondary)
                .frame(width: 25, height: 25)
                .background(Color.primary.opacity(hovering ? 0.12 : 0), in: Circle())
                .contentShape(Circle())
        }
        .buttonStyle(.plain)
        .onHover { hovering = $0 }
        .help(label)
        .accessibilityLabel(label)
    }
}

private struct CloseButton: View {
    let group: TabGroup
    @State private var hovering = false

    var body: some View {
        Button {
            Hub.shared.close(group)
        } label: {
            Image(systemName: "xmark")
                .font(.system(size: 10, weight: .semibold))
                .foregroundStyle(.secondary)
                .frame(width: 25, height: 25)
                .background(Color.primary.opacity(hovering ? 0.12 : 0), in: Circle())
                .contentShape(Circle())
        }
        .buttonStyle(.plain)
        // Clear of the resize corner.
        .padding(.trailing, 3)
        .onHover { hovering = $0 }
        .help(t("Tab-Gruppe schließen", "Close tab group"))
        .accessibilityLabel(t("Tab-Gruppe schließen", "Close tab group"))
    }
}

private struct PlusButton: View {
    let group: TabGroup
    @State private var hovering = false

    var body: some View {
        Button {
            AddPanel.shared.show(for: group)
        } label: {
            Image(systemName: "plus")
                .font(.system(size: 11, weight: .semibold))
                .foregroundStyle(.secondary)
                .frame(width: 25, height: 25)
                .background(Color.primary.opacity(hovering ? 0.12 : 0), in: Circle())
                .contentShape(Circle())
        }
        .buttonStyle(.plain)
        .padding(.leading, 4)
        .onHover { hovering = $0 }
        .help(t("Fenster hinzufügen", "Add a window"))
        .accessibilityLabel(t("Fenster oder Programm hinzufügen", "Add a window or app"))
    }
}

/// The tab of an app that is still starting. It pulses until the app's window is there.
private struct GhostCell: View {
    let ghost: Ghost
    let width: CGFloat
    @State private var dim = false

    var body: some View {
        HStack(spacing: 7) {
            Image(nsImage: NSWorkspace.shared.icon(forFile: ghost.path)).resizable().frame(width: 17, height: 17)
            Text(ghost.name).font(.system(size: 12)).foregroundStyle(.secondary).lineLimit(1)
            Spacer(minLength: 0)
        }
        .padding(.horizontal, 9)
        .frame(width: max(40, width - 3), height: 27)
        .background {
            RoundedRectangle(cornerRadius: 9, style: .continuous)
                .strokeBorder(Color.primary.opacity(0.18), style: StrokeStyle(lineWidth: 1, dash: [4, 3]))
        }
        .padding(.horizontal, 1.5)
        .opacity(dim ? 0.3 : 0.9)
        .onAppear {
            withAnimation(.easeInOut(duration: 0.8).repeatForever(autoreverses: true)) { dim = true }
        }
        .help(t("\(ghost.name) startet", "\(ghost.name) is starting"))
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(t("\(ghost.name) startet", "\(ghost.name) is starting"))
    }
}

private struct TabCell: View {
    @ObservedObject var group: TabGroup
    let member: Member
    let index: Int
    let width: CGFloat

    @GestureState private var pull = CGSize.zero
    /// The slot this tab sat in when its drag began. While it is set, the tab follows the pointer.
    @State private var dragStart: Int?
    @Environment(\.colorScheme) private var scheme
    @ObservedObject private var language = LanguageState.shared
    @State private var hovering = false

    /// Where the dragged tab is drawn, relative to its slot. The slot itself moves each time the tab
    /// changes places, so that shift is taken back out and the tab stays under the pointer.
    private var slide: CGFloat {
        guard let start = dragStart else { return 0 }
        return pull.width - CGFloat(index - start) * width
    }

    private var label: String {
        var parts = [member.appName]
        if !member.title.isEmpty, member.title != member.appName { parts.append(member.title) }
        parts.append(t("Tab \(index + 1) von \(group.members.count)", "tab \(index + 1) of \(group.members.count)"))
        if !member.badge.isEmpty { parts.append(t("\(member.badge) ungelesen", "\(member.badge) unread")) }
        return parts.joined(separator: ", ")
    }

    var body: some View {
        let active = index == group.active
        HStack(spacing: 7) {
            if let icon = member.icon {
                Image(nsImage: icon).resizable().frame(width: 17, height: 17)
            }
            Text(member.title.isEmpty ? member.appName : member.title)
                .font(.system(size: 12, weight: active ? .semibold : .regular))
                .foregroundStyle(active ? Color.primary : Color.secondary)
                .lineLimit(1)
                .truncationMode(.tail)
            Spacer(minLength: 0)
            if !member.badge.isEmpty {
                // The same count the app shows on its Dock icon.
                Text(member.badge)
                    .font(.system(size: 10, weight: .bold, design: .rounded))
                    .foregroundStyle(.white)
                    .lineLimit(1)
                    .padding(.horizontal, 5)
                    .frame(minWidth: 17, minHeight: 17)
                    .background(LinearGradient(colors: [Color(red: 1.00, green: 0.36, blue: 0.33),
                                                        Color(red: 0.87, green: 0.13, blue: 0.16)],
                                               startPoint: .top, endPoint: .bottom), in: Capsule())
                    .shadow(color: Color.red.opacity(0.35), radius: 2, x: 0, y: 1)
                    .accessibilityLabel(t("\(member.badge) ungelesen", "\(member.badge) unread"))
            }
            if hovering || active {
                Button {
                    Hub.shared.detach(group, member.id)
                } label: {
                    Image(systemName: "xmark")
                        .font(.system(size: 8, weight: .bold))
                        .frame(width: 16, height: 16)
                        .background(Color.primary.opacity(0.08), in: Circle())
                }
                .buttonStyle(.plain)
                .help(t("Aus der Gruppe lösen", "Remove from group"))
            }
        }
        .padding(.horizontal, 9)
        .frame(width: max(40, width - 3), height: 27)
        .background {
            let shape = RoundedRectangle(cornerRadius: 9, style: .continuous)
            let dark = scheme == .dark
            if active {
                // A raised glass key: lighter at the top, a bright upper edge, a soft shadow below.
                shape
                    .fill(LinearGradient(colors: [Color.white.opacity(dark ? 0.24 : 0.92),
                                                  Color.white.opacity(dark ? 0.10 : 0.58)],
                                         startPoint: .top, endPoint: .bottom))
                    .overlay {
                        shape.strokeBorder(LinearGradient(colors: [Color.white.opacity(dark ? 0.38 : 1),
                                                                   Color.black.opacity(dark ? 0.30 : 0.10)],
                                                          startPoint: .top, endPoint: .bottom), lineWidth: 1)
                    }
                    .shadow(color: Color.black.opacity(dark ? 0.40 : 0.16), radius: 3, x: 0, y: 1.5)
            } else {
                // Resting tabs: a faint fill and a hairline, so each one reads as a tab without competing
                // with the raised active one.
                shape
                    .fill(Color.primary.opacity(hovering ? 0.09 : 0.03))
                    .overlay {
                        shape.strokeBorder(Color.primary.opacity(dark ? 0.16 : 0.11), lineWidth: 1)
                    }
            }
        }
        .padding(.horizontal, 1.5)
        .contentShape(Rectangle())
        .offset(x: slide)
        .zIndex(dragStart == nil ? 0 : 1)
        .onHover { hovering = $0 }
        .onTapGesture { Hub.shared.activate(group, index) }
        // For VoiceOver and for AI agents that read the accessibility tree instead of pixels: one
        // named, selectable button per tab.
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(label)
        .accessibilityAddTraits(active ? [.isButton, .isSelected] : .isButton)
        .accessibilityAction { Hub.shared.activate(group, index) }
        .accessibilityAction(named: Text(t("Aus der Gruppe lösen", "Remove from group"))) {
            Hub.shared.detach(group, member.id)
        }
        .gesture(
            // Measured against the bar, not the tab: the tab itself moves while it is dragged, and a
            // distance measured from a moving tab swings back and forth.
            DragGesture(minimumDistance: 8, coordinateSpace: .global)
                .updating($pull) { value, state, _ in state = value.translation }
                .onChanged { value in
                    // Pulled out of the bar, like a Chrome tab: the window comes loose at once and
                    // hangs on the pointer until the button is released.
                    // What counts is where the pointer is, not how far it wandered: inside the bar the
                    // tab is being sorted, however unsteady the hand; clearly above or below it, it leaves.
                    if value.location.y < -14 || value.location.y > barHeight + 14 {
                        Hub.shared.beginPull(group, member.id)
                        return
                    }
                    // Sideways: the tab rides along and its neighbours make room as soon as it has
                    // crossed half of one of them.
                    let start = dragStart ?? index
                    if dragStart == nil { dragStart = index }
                    let steps = Int((value.translation.width / max(1, width)).rounded())
                    let target = min(max(0, start + steps), group.members.count - 1)
                    if target != index {
                        withAnimation(.easeOut(duration: 0.16)) { Hub.shared.move(group, from: index, to: target) }
                    }
                }
                .onEnded { _ in
                    withAnimation(.easeOut(duration: 0.16)) { dragStart = nil }
                }
        )
        .help(member.appName + (member.title.isEmpty ? "" : " · " + member.title))
        .contextMenu {
            Button(t("Diese Gruppe als Paket sichern …", "Save this group as a package …")) { savePackage(of: group) }
            Button(t("Tab aus der Gruppe lösen", "Remove tab from group")) { Hub.shared.detach(group, member.id) }
        }
    }
}

@MainActor
final class RecorderState: ObservableObject {
    @Published var name = ""
    @Published var current = ""
    @Published var message = ""
    @Published var warning = false
}

/// The small window that waits for a key combination for one package.
struct Recorder: View {
    @ObservedObject var state: RecorderState
    let remove: () -> Void
    let cancel: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text(t("Kürzel für „\(state.name)“", "Shortcut for “\(state.name)”")).font(.system(size: 18, weight: .bold))
            Text(t("Aktuell: \(state.current)", "Current: \(state.current)")).foregroundStyle(.secondary)
            Label(state.message, systemImage: state.warning ? "hand.raised.fill" : "keyboard")
                .foregroundStyle(state.warning ? Color.orange : Color.primary)
                .fixedSize(horizontal: false, vertical: true)
                .padding(12)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(Color.primary.opacity(0.05), in: RoundedRectangle(cornerRadius: 12, style: .continuous))
            HStack {
                Button(t("Kürzel entfernen", "Remove shortcut"), action: remove)
                Spacer()
                Button(t("Abbrechen", "Cancel"), action: cancel)
            }
        }
        .padding(22)
        .frame(width: 460)
    }
}

/// First launch and the menu's "Anleitung": what Dockodile needs and how to use it.
struct Welcome: View {
    @ObservedObject var state: AccessState
    @ObservedObject private var language = LanguageState.shared
    let openSettings: () -> Void
    let close: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack(spacing: 12) {
                if let logo = NSImage(named: "Logo") {
                    Image(nsImage: logo)
                        .resizable()
                        .interpolation(.high)
                        .frame(width: 60, height: 60)
                } else {
                    Image(systemName: "rectangle.stack.fill")
                        .font(.system(size: 22, weight: .semibold))
                        .foregroundStyle(.white)
                        .frame(width: 48, height: 48)
                        .background(accent, in: RoundedRectangle(cornerRadius: 14, style: .continuous))
                }
                VStack(alignment: .leading, spacing: 2) {
                    Text("Dockodile").font(.system(size: 24, weight: .bold))
                    Text(t("Fenster verschiedener Programme als Tabs", "Windows of different apps as tabs")).foregroundStyle(.secondary)
                }
                Spacer()
                Picker("Sprache / Language", selection: $language.current) {
                    Text("Deutsch").tag(Lang.de)
                    Text("English").tag(Lang.en)
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .fixedSize()
            }
            VStack(alignment: .leading, spacing: 8) {
                Label(t("Zieh ein Fenster an seiner Titelleiste auf den oberen Rand eines anderen und lass los.", "Drag a window by its title bar onto the top edge of another one and let go."),
                      systemImage: "arrow.down.right.and.arrow.up.left")
                Label(t("Klick auf einen Tab wechselt das Programm. Tabs lassen sich seitlich umsortieren.", "Click a tab to switch apps. Drag tabs sideways to reorder them."),
                      systemImage: "hand.tap")
                Label(t("Zieh einen Tab nach unten heraus oder klick auf das Kreuz, um ein Fenster zu lösen.", "Pull a tab down and out, or click its cross, to release a window."),
                      systemImage: "arrow.up.forward.square")
            }
            .font(.callout)
            .fixedSize(horizontal: false, vertical: true)
            .padding(14)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Color.primary.opacity(0.05), in: RoundedRectangle(cornerRadius: 14, style: .continuous))
            if state.trusted {
                Label(t("Bedienungshilfen-Zugriff ist aktiv. Dockodile läuft oben in der Menüleiste.", "Accessibility access is active. Dockodile lives in the menu bar."),
                      systemImage: "checkmark.circle.fill")
                    .foregroundStyle(.green)
            } else {
                Text(t("Dockodile bewegt fremde Fenster. Dafür verlangt macOS den Zugriff „Bedienungshilfen“. Schalte Dockodile in den Systemeinstellungen ein, die App merkt das von selbst.", "Dockodile moves windows of other apps. macOS requires Accessibility access for that. Switch Dockodile on in System Settings; the app notices by itself."))
                    .font(.callout)
                    .fixedSize(horizontal: false, vertical: true)
                HStack(spacing: 8) {
                    ProgressView().controlSize(.small)
                    Text(t("Warte auf die Freigabe …", "Waiting for access …")).foregroundStyle(.secondary)
                }
            }
            HStack {
                Spacer()
                if !state.trusted {
                    Button(t("Systemeinstellungen öffnen", "Open System Settings"), action: openSettings)
                }
                Button(state.trusted ? t("Los geht's", "Let's go") : t("Später", "Later"), action: close)
                    .keyboardShortcut(.defaultAction)
            }
        }
        .padding(24)
        .frame(width: 520)
    }
}
