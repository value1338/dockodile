import AppKit
import Security

/// Updates for buyers. The app asks dockodile.de for the newest version number. With the key of
/// the purchase, stored through `dockodile://activate?token=…`, it downloads the DMG itself, checks
/// that it carries the same Developer ID as the running app, swaps the bundle and starts again.
/// Self-built copies have no Developer ID and no key: they are sent to the update page instead.
@MainActor
final class Updater {
    static let shared = Updater()
    static var page: URL {
        var url = URLComponents(string: "https://dockodile.de/update.html")!
        url.queryItems = [URLQueryItem(name: "os", value: "mac"),
                          URLQueryItem(name: "from", value: Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String)]
        if let expected = Updater.shared.available { url.queryItems?.append(URLQueryItem(name: "min_version", value: expected)) }
        return url.url!
    }
    private static let server = "https://dockodile.de"

    /// Newest version on the server, set only while it is newer than this app.
    private(set) var available: String?
    private var timer: Timer?
    private var busy = false

    var current: String {
        Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "0"
    }

    /// On: look once a day and install. Off: nothing goes to the network unless asked from the menu.
    var automatic: Bool {
        get { UserDefaults.standard.object(forKey: "autoUpdate") as? Bool ?? true }
        set {
            UserDefaults.standard.set(newValue, forKey: "autoUpdate")
            schedule()
        }
    }

    private var token: String? { UserDefaults.standard.string(forKey: "updateToken") }
    var activated: Bool { token != nil }

    func start() { schedule() }

    private func schedule() {
        timer?.invalidate()
        timer = nil
        guard automatic else { return }
        check(manual: false)
        timer = Timer.scheduledTimer(withTimeInterval: 24 * 60 * 60, repeats: true) { _ in
            DispatchQueue.main.async { self.check(manual: false) }
        }
    }

    // MARK: Looking

    func check(manual: Bool) {
        guard !busy else { return }
        var request = URLRequest(url: URL(string: Updater.server + "/api/version")!)
        request.cachePolicy = .reloadIgnoringLocalCacheData
        request.timeoutInterval = 20
        URLSession.shared.dataTask(with: request) { data, _, _ in
            let json = data.flatMap { try? JSONSerialization.jsonObject(with: $0) } as? [String: Any]
            let version = json?["version"] as? String
            DispatchQueue.main.async { self.found(version, manual: manual) }
        }.resume()
    }

    private func found(_ version: String?, manual: Bool) {
        guard let version, version.range(of: "^[0-9]+(\\.[0-9]+)*$", options: .regularExpression) != nil else {
            if manual {
                tell(t("Die Suche nach Updates hat nicht geklappt.", "Checking for updates failed."),
                     t("Prüfe die Internetverbindung und versuch es später noch einmal.",
                       "Check your internet connection and try again later."))
            }
            return
        }
        guard version.compare(current, options: .numeric) == .orderedDescending else {
            available = nil
            if manual {
                tell(t("Dockodile ist aktuell.", "Dockodile is up to date."),
                     t("Version \(current) ist die neueste.", "Version \(current) is the newest."))
            }
            return
        }
        available = version
        if !manual {
            let defaults = UserDefaults.standard
            // Without asking only while no group is open that a restart would take apart, and only
            // once per version: if the swap fails, the next start asks instead of trying forever.
            if canInstall, Hub.shared.groups.isEmpty, defaults.string(forKey: "updateTried") != version {
                defaults.set(version, forKey: "updateTried")
                install()
                return
            }
            // Offered once per version. After that it waits in the menu.
            if defaults.string(forKey: "updateOffered") == version { return }
            defaults.set(version, forKey: "updateOffered")
        }
        offer()
    }

    /// The update dialog. Also behind the menu item.
    func offer() {
        guard let version = available else { return }
        let direct = canInstall
        let alert = NSAlert()
        alert.messageText = t("Dockodile \(version) ist da.", "Dockodile \(version) is available.")
        if direct {
            alert.informativeText = t(
                "Du nutzt Version \(current). Dockodile lädt das Update, ersetzt sich selbst und startet neu. Offene Gruppen werden dabei aufgelöst.",
                "You are using version \(current). Dockodile downloads the update, replaces itself and starts again. Open groups are dissolved.")
            alert.addButton(withTitle: t("Jetzt installieren", "Install now"))
        } else {
            alert.informativeText = t(
                "Du nutzt Version \(current). Lade die neue DMG über die Update-Seite. Beende Dockodile und ersetze die vorhandene Dockodile.app in Programme. Wähle Ersetzen, nicht Beide behalten. Starte nur die ersetzte App aus Programme.",
                "You are using version \(current). Download the new DMG from the update page. Quit Dockodile and replace the existing Dockodile.app in Applications. Choose Replace, not Keep Both. Launch only the replaced app from Applications.")
            alert.addButton(withTitle: t("Update-Seite öffnen", "Open update page"))
        }
        alert.addButton(withTitle: t("Später", "Later"))
        NSApp.activate(ignoringOtherApps: true)
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        if direct { install() } else { NSWorkspace.shared.open(Updater.page) }
    }

    // MARK: Key of the purchase

    /// `dockodile://activate?token=…` from the purchase page or the update mail.
    func activate(_ token: String) -> Bool {
        guard token.range(of: "^[A-Za-z0-9_-]{43}$", options: .regularExpression) != nil else { return false }
        if let old = self.token, old != token {
            // Any web page can open this link, so an existing key is never replaced silently.
            let alert = NSAlert()
            alert.messageText = t("Freischaltung ersetzen?", "Replace the unlock key?")
            alert.informativeText = t(
                "Dockodile ist schon für Updates freigeschaltet. Ersetze den Schlüssel nur, wenn du den Link selbst gerade angefordert hast.",
                "Dockodile is already unlocked for updates. Replace the key only if you just asked for this link yourself.")
            alert.addButton(withTitle: t("Ersetzen", "Replace"))
            alert.addButton(withTitle: t("Abbrechen", "Cancel"))
            NSApp.activate(ignoringOtherApps: true)
            guard alert.runModal() == .alertFirstButtonReturn else { return true }
        }
        UserDefaults.standard.set(token, forKey: "updateToken")
        tell(t("Updates sind freigeschaltet.", "Updates are unlocked."),
             t("Dockodile lädt neue Versionen ab jetzt selbst.", "From now on Dockodile downloads new versions itself."))
        check(manual: false)
        return true
    }

    // MARK: Installing

    private var canInstall: Bool {
        let bundle = Bundle.main.bundleURL.resolvingSymlinksInPath()
        let path = bundle.path
        let files = FileManager.default
        let parent = bundle.deletingLastPathComponent().path
        let installFolders = ["/Applications", files.homeDirectoryForCurrentUser.appendingPathComponent("Applications").path]
        // Never update a copy in Downloads, a mounted DMG or another temporary location.
        return installFolders.contains(parent) && token != nil && Updater.team(of: Bundle.main.bundleURL) != nil && !path.contains("/AppTranslocation/")
            && files.isWritableFile(atPath: path) && files.isWritableFile(atPath: (path as NSString).deletingLastPathComponent)
    }

    private func install() {
        guard !busy else { return }
        guard canInstall, let expected = available, let token, let team = Updater.team(of: Bundle.main.bundleURL) else { failed(); return }
        busy = true
        var request = URLRequest(url: URL(string: Updater.server + "/api/app/update")!)
        request.httpMethod = "POST"
        request.setValue("application/x-www-form-urlencoded", forHTTPHeaderField: "Content-Type")
        request.httpBody = Data("token=\(token)&platform=mac&current_version=\(current)&min_version=\(expected)".utf8)
        URLSession.shared.downloadTask(with: request) { file, response, _ in
            let status = (response as? HTTPURLResponse)?.statusCode ?? 0
            // The downloaded file is removed when this closure returns, so it moves first.
            let dmg = FileManager.default.temporaryDirectory.appendingPathComponent("Dockodile-\(UUID().uuidString).dmg")
            var moved = false
            if status == 200, let file {
                moved = (try? FileManager.default.moveItem(at: file, to: dmg)) != nil
            }
            let ready = moved
            DispatchQueue.main.async {
                self.busy = false
                if ready {
                    self.swap(dmg, team: team, expected: expected)
                } else if status == 403 {
                    self.tell(t("Die Freischaltung gilt nicht mehr.", "The unlock key is no longer valid."),
                              t("Fordere auf der Update-Seite einen neuen Link an.", "Ask for a new link on the update page."))
                    NSWorkspace.shared.open(Updater.page)
                } else {
                    self.failed()
                }
            }
        }.resume()
    }

    private func swap(_ dmg: URL, team: String, expected: String) {
        let files = FileManager.default
        let mount = files.temporaryDirectory.appendingPathComponent("Dockodile-Update-\(UUID().uuidString)")
        try? files.createDirectory(at: mount, withIntermediateDirectories: true)
        // ponytail: hdiutil and the signature check block the main thread for about a second. Move
        // them to a background queue if the DMG ever grows well beyond a few megabytes.
        guard Updater.run("/usr/bin/hdiutil", ["attach", dmg.path, "-nobrowse", "-readonly", "-noautoopen", "-mountpoint", mount.path]) else {
            try? files.removeItem(at: dmg)
            failed()
            return
        }
        let fresh = mount.appendingPathComponent("Dockodile.app")
        // A filename is not a version guarantee. Check the actual signed bundle, identity and version.
        let freshBundle = Bundle(url: fresh)
        let freshVersion = freshBundle?.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? ""
        guard freshBundle?.bundleIdentifier == Bundle.main.bundleIdentifier,
              freshVersion.range(of: "^[0-9]+(\\.[0-9]+)*$", options: .regularExpression) != nil,
              freshVersion.compare(current, options: .numeric) == .orderedDescending,
              freshVersion.compare(expected, options: .numeric) != .orderedAscending,
              Updater.signed(fresh, by: team) else {
            _ = Updater.run("/usr/bin/hdiutil", ["detach", mount.path, "-quiet"])
            try? files.removeItem(at: dmg)
            failed()
            return
        }
        // Prepare and verify a complete copy on the destination volume before stopping this app.
        let target = Bundle.main.bundleURL.resolvingSymlinksInPath()
        let parent = target.deletingLastPathComponent()
        let staged = parent.appendingPathComponent(".Dockodile-update-\(UUID().uuidString).app")
        let backup = parent.appendingPathComponent(".Dockodile-rollback-\(UUID().uuidString).app")
        let copied = Updater.run("/usr/bin/ditto", [fresh.path, staged.path])
        let stagedBundle = Bundle(url: staged)
        let stagedVersion = stagedBundle?.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String
        let checked = copied && Updater.signed(staged, by: team)
            && stagedBundle?.bundleIdentifier == Bundle.main.bundleIdentifier
            && stagedVersion == freshVersion
        _ = Updater.run("/usr/bin/hdiutil", ["detach", mount.path, "-quiet"])
        try? files.removeItem(at: dmg)
        try? files.removeItem(at: mount)
        guard checked else {
            try? files.removeItem(at: staged)
            failed()
            return
        }
        // Replace precisely the installed bundle. On failure restore that same path; no second app.
        let script = """
        [ -d "$TARGET" ] && [ -d "$STAGED" ] && [ ! -e "$BACKUP" ] || exit 1
        attempts=0
        while kill -0 "$OLDPID" 2>/dev/null; do
          attempts=$((attempts + 1))
          if [ "$attempts" -ge 150 ]; then rm -rf "$STAGED"; exit 1; fi
          sleep 0.2
        done
        if ! mv "$TARGET" "$BACKUP"; then
          rm -rf "$STAGED"
          open "$TARGET"
          exit 1
        fi
        if ! mv "$STAGED" "$TARGET"; then
          mv "$BACKUP" "$TARGET"
          rm -rf "$STAGED"
          open "$TARGET"
          exit 1
        fi
        if ! open "$TARGET"; then
          rm -rf "$TARGET"
          mv "$BACKUP" "$TARGET"
          open "$TARGET"
          exit 1
        fi
        rm -rf "$BACKUP"
        """
        let task = Process()
        task.executableURL = URL(fileURLWithPath: "/bin/sh")
        task.arguments = ["-c", script]
        task.environment = [
            "PATH": "/usr/bin:/bin:/usr/sbin:/sbin",
            "OLDPID": String(ProcessInfo.processInfo.processIdentifier),
            "TARGET": target.path,
            "STAGED": staged.path,
            "BACKUP": backup.path,
        ]
        do {
            try task.run()
        } catch {
            try? files.removeItem(at: staged)
            failed()
            return
        }
        NSApp.terminate(nil)
    }

    private func failed() {
        let alert = NSAlert()
        alert.messageText = t("Das Update konnte nicht installiert werden.", "The update could not be installed.")
        alert.informativeText = t(
            "Dockodile läuft unverändert weiter. Lade die neue DMG über die Update-Seite. Beende Dockodile und ersetze die vorhandene Dockodile.app im Ordner Programme. Wähle im Finder Ersetzen, nicht Beide behalten. Starte danach die App aus Programme, nicht aus dem Download oder der DMG.",
            "Dockodile keeps running unchanged. Download the new DMG from the update page. Quit Dockodile and replace the existing Dockodile.app in Applications. In Finder choose Replace, not Keep Both. Then launch the app from Applications, not Downloads or the DMG.")
        alert.addButton(withTitle: t("Neu herunterladen", "Download again"))
        alert.addButton(withTitle: t("Später", "Later"))
        NSApp.activate(ignoringOtherApps: true)
        if alert.runModal() == .alertFirstButtonReturn { NSWorkspace.shared.open(Updater.page) }
    }

    private func tell(_ title: String, _ text: String) {
        let alert = NSAlert()
        alert.messageText = title
        alert.informativeText = text
        NSApp.activate(ignoringOtherApps: true)
        _ = alert.runModal()
    }

    private static func run(_ tool: String, _ arguments: [String]) -> Bool {
        let task = Process()
        task.executableURL = URL(fileURLWithPath: tool)
        task.arguments = arguments
        task.standardOutput = FileHandle.nullDevice
        task.standardError = FileHandle.nullDevice
        do {
            try task.run()
        } catch {
            return false
        }
        task.waitUntilExit()
        return task.terminationStatus == 0
    }

    // MARK: Signature

    /// Team of the Developer ID that signed the bundle. Nil for unsigned and self-signed builds.
    private static func team(of bundle: URL) -> String? {
        var code: SecStaticCode?
        guard SecStaticCodeCreateWithPath(bundle as CFURL, [], &code) == errSecSuccess, let code else { return nil }
        var info: CFDictionary?
        guard SecCodeCopySigningInformation(code, SecCSFlags(rawValue: kSecCSSigningInformation), &info) == errSecSuccess,
              let entries = info as? [String: Any] else { return nil }
        return entries[kSecCodeInfoTeamIdentifier as String] as? String
    }

    /// True when the bundle is intact and signed with an Apple-issued certificate of that team.
    private static func signed(_ bundle: URL, by team: String) -> Bool {
        guard team.range(of: "^[A-Z0-9]+$", options: .regularExpression) != nil else { return false }
        var code: SecStaticCode?
        guard SecStaticCodeCreateWithPath(bundle as CFURL, [], &code) == errSecSuccess, let code else { return false }
        var requirement: SecRequirement?
        let rule = "anchor apple generic and certificate leaf[subject.OU] = \"\(team)\""
        guard SecRequirementCreateWithString(rule as CFString, [], &requirement) == errSecSuccess, let requirement else { return false }
        let flags = SecCSFlags(rawValue: kSecCSCheckAllArchitectures | kSecCSStrictValidate)
        return SecStaticCodeCheckValidity(code, flags, requirement) == errSecSuccess
    }
}
