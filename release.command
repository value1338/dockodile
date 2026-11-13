#!/bin/bash
# macOS only. Uses existing Developer ID and a Keychain notarytool profile.
set -Eeuo pipefail
cd "$(dirname "$0")"
[[ $(uname -s) == Darwin ]] || { echo 'Dieses Skript benötigt macOS.' >&2; exit 2; }
for tool in xcrun codesign security hdiutil ditto lipo spctl shasum python3; do
  command -v "$tool" >/dev/null || { echo "Fehlt: $tool" >&2; exit 2; }
done
version=$(/usr/libexec/PlistBuddy -c 'Print CFBundleShortVersionString' Info.plist)
build=$(/usr/libexec/PlistBuddy -c 'Print CFBundleVersion' Info.plist)
identifier=$(/usr/libexec/PlistBuddy -c 'Print CFBundleIdentifier' Info.plist)
[[ $version == 0.3.2 && $build == 5 && $identifier == de.lakecode.dockodile ]] || { echo 'Unerwartete Version/Bundle-ID. Export abgebrochen.' >&2; exit 2; }
profile=${DOCKODILE_NOTARY:-dockodile-notary}
expected_team=9Q5D64SJUN
identity=${DOCKODILE_SIGN:-}
if [[ -z $identity ]]; then
  identity=$(security find-identity -v -p codesigning | sed -n 's/.*"\(Developer ID Application: .* (9Q5D64SJUN)\)"$/\1/p' | head -1)
fi
[[ -n $identity ]] || { echo 'Developer-ID-Application-Zertifikat für Team 9Q5D64SJUN fehlt im Schlüsselbund.' >&2; exit 2; }
xcrun --find swiftc >/dev/null
xcrun --find notarytool >/dev/null
if ! xcrun notarytool history --keychain-profile "$profile" >/dev/null 2>&1; then
  echo "Notarisierungsprofil '$profile' fehlt oder ist nicht nutzbar." >&2
  echo 'Richte es interaktiv mit xcrun notarytool store-credentials ein. Keine Passwörter in Chat, Quellcode oder Logs.' >&2
  exit 2
fi
mkdir -p "release/$version"
out=$(mktemp -d "$PWD/release/$version/$(date -u +%Y%m%dT%H%M%SZ)-XXXXXX")
work=$(mktemp -d "${TMPDIR:-/tmp}/dockodile-release.XXXXXX")
stage='';mount='';mounted=0
cleanup() {
  if [[ $mounted == 1 ]]; then hdiutil detach "$mount" -quiet || true; fi
  [[ -z $stage ]] || rm -rf "$stage"
  rm -rf "$work"
}
trap cleanup EXIT
trap 'rc=$?; echo "FEHLER: Export unvollständig (exit=$rc). Belege erhalten in: $out" >&2; exit "$rc"' ERR
exec > >(tee "$out/release.log") 2>&1
printf 'Version: %s (Build %s)\nAusgabe: %s\n' "$version" "$build" "$out"
app="$out/Dockodile.app"
dmg="$out/Dockodile-$version.dmg"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp Info.plist "$app/Contents/Info.plist"
cp Icon/Dockodile.icns "$app/Contents/Resources/AppIcon.icns"
cp Icon/MenuBar.png Icon/MenuBar@2x.png Icon/Logo.png Icon/Logo@2x.png "$app/Contents/Resources/"
shasum -a 256 Info.plist src/*.swift Icon/*.png Icon/*.icns tests/test_package_position.swift tests/test_update_swap.py release.command > "$out/SOURCE-SHA256SUMS.txt"
python3 tests/test_update_swap.py
xcrun --sdk macosx swiftc -module-cache-path "$work/cache-position-test" -parse-as-library \
  src/PackagePosition.swift tests/test_package_position.swift -o "$work/test-package-position"
"$work/test-package-position"
for arch in arm64 x86_64; do
  xcrun --sdk macosx swiftc -swift-version 5 -O -target "$arch-apple-macos14.0" \
    -module-cache-path "$work/cache-$arch" src/*.swift -o "$work/Dockodile-$arch"
done
lipo -create "$work/Dockodile-arm64" "$work/Dockodile-x86_64" -output "$app/Contents/MacOS/Dockodile"
lipo -verify_arch arm64 "$app/Contents/MacOS/Dockodile"
lipo -verify_arch x86_64 "$app/Contents/MacOS/Dockodile"
shasum -a 256 -c "$out/SOURCE-SHA256SUMS.txt"
codesign --force --options runtime --timestamp --sign "$identity" "$app"
codesign --verify --deep --strict --verbose=2 "$app"
codesign -d --verbose=4 "$app" 2> "$out/codesign-app.txt"
grep -q "^TeamIdentifier=$expected_team$" "$out/codesign-app.txt"
notarize() {
  local file=$1 report=$2 status id
  xcrun notarytool submit "$file" --keychain-profile "$profile" --wait --output-format json > "$report"
  status=$(/usr/bin/plutil -extract status raw -o - "$report")
  if [[ $status != Accepted ]]; then
    id=$(/usr/bin/plutil -extract id raw -o - "$report")
    xcrun notarytool log "$id" --keychain-profile "$profile" "${report%.json}-details.json" || true
    echo "Apple-Notarisierung nicht akzeptiert: $status" >&2
    return 1
  fi
}
# Notarize and staple the app before placing it into the distribution image.
ditto -c -k --keepParent "$app" "$work/app-notary.zip"
notarize "$work/app-notary.zip" "$out/notary-app.json"
xcrun stapler staple "$app"
xcrun stapler validate "$app"
spctl --assess --type execute --verbose=2 "$app"
stage=$(mktemp -d "${TMPDIR:-/tmp}/dockodile-image.XXXXXX")
ditto "$app" "$stage/Dockodile.app"
ln -s /Applications "$stage/Applications"
hdiutil create -volname Dockodile -srcfolder "$stage" -format UDZO "$dmg"
codesign --sign "$identity" --timestamp "$dmg"
codesign --verify --verbose=2 "$dmg"
notarize "$dmg" "$out/notary-dmg.json"
xcrun stapler staple "$dmg"
xcrun stapler validate "$dmg"
hdiutil verify "$dmg"
spctl --assess --type open --context context:primary-signature --verbose=2 "$dmg"
# Inspect the ACTUAL image, rather than trusting its filename or the staging app.
mount="$work/mounted-image";mkdir "$mount"
hdiutil attach "$dmg" -nobrowse -readonly -noautoopen -mountpoint "$mount"
mounted=1
inside="$mount/Dockodile.app"
[[ $(/usr/libexec/PlistBuddy -c 'Print CFBundleShortVersionString' "$inside/Contents/Info.plist") == "$version" ]]
[[ $(/usr/libexec/PlistBuddy -c 'Print CFBundleVersion' "$inside/Contents/Info.plist") == "$build" ]]
[[ $(/usr/libexec/PlistBuddy -c 'Print CFBundleIdentifier' "$inside/Contents/Info.plist") == "$identifier" ]]
lipo -verify_arch arm64 "$inside/Contents/MacOS/Dockodile"
lipo -verify_arch x86_64 "$inside/Contents/MacOS/Dockodile"
codesign --verify --deep --strict --verbose=2 "$inside"
xcrun stapler validate "$inside"
spctl --assess --type execute --verbose=2 "$inside"
[[ $(readlink "$mount/Applications") == /Applications ]]
hdiutil detach "$mount" -quiet;mounted=0
# A final ZIP preserves bundle metadata and the stapled ticket for return to the workstation.
ditto -c -k --keepParent "$app" "$out/Dockodile-$version.app.zip"
(cd "$out" && shasum -a 256 "Dockodile-$version.dmg" "Dockodile-$version.app.zip" > SHA256SUMS.txt)
cat > "$out/PRUEFBERICHT.txt" <<REPORT
Dockodile $version, Build $build, Bundle-ID $identifier
Apple Silicon und Intel gemeinsam gebaut (Universal Binary).
Developer-ID-Team $expected_team, Hardened Runtime, Zeitstempel, Signaturprüfung erfolgreich.
App und DMG von Apple mit Accepted bestätigt, Tickets angeheftet und validiert.
Gatekeeper und tatsächlicher DMG-Inhalt auf Version, Build, Bundle-ID, Signatur und beide Architekturen geprüft.
Vier isolierte Austausch-Helper-Tests bestanden; keine installierte Nutzer-App verändert.
Die Bildschirmposition wurde mit simulierten Monitoranordnungen getestet (Wiederherstellen, Umordnen, Abstecken, kleinerer Bildschirm und alte Koordinaten).
Nicht automatisch geprüft: vollständiges GUI-Verhalten, Accessibility-Gruppierung und kompletter In-App-Updateablauf auf einem echten Mac; Intel-Laufzeit ebenfalls separat prüfen.
Kein Upload nach Netcup, kein Einschalten des Verkaufs.
REPORT
printf '\nFERTIG: %s\n' "$dmg"
echo 'Prüfbericht, Notarisierungsbelege und SHA256SUMS.txt liegen im selben Ausgabeordner.'
