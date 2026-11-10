#!/bin/bash
# Doppelklick: baut Dockodile.app, schreibt build.log und startet die App.
# Beim ersten Mal bietet das Skript an, ein eigenes Signaturzertifikat anzulegen.
cd "$(dirname "$0")"
mkdir -p .build
# The Swift module cache remembers the folder it was made in. After the project folder was moved
# or renamed it only produces errors, so it is thrown away and built anew.
if [[ "$(cat .build/ort 2>/dev/null)" != "$PWD" ]]; then
  find .build -mindepth 1 -maxdepth 1 ! -name ohne-signatur -exec rm -rf {} +
  pwd > .build/ort
fi

# --- Signatur -------------------------------------------------------------------------------
# macOS merkt sich den Bedienungshilfen-Zugriff anhand der Signatur. Ohne festes Zertifikat
# bekommt jeder Build eine neue Wegwerf-Signatur, und der Zugriff muss jedes Mal neu erteilt werden.
own="Dockodile Lokal"

# German for German systems, English for everyone else.
case "$(defaults read -g AppleLanguages 2>/dev/null | sed -n 2p)" in *de*) german=1 ;; *) german= ;; esac
say() { if [[ -n $german ]]; then echo "$1"; else echo "$2"; fi; }
keychain="$HOME/Library/Keychains/login.keychain-db"

valid() {
  security find-identity -v -p codesigning 2>/dev/null | grep -q "\"$1\""
}

# Creates a self-signed code signing certificate in the login keychain. It never leaves this Mac.
create_identity() {
  local work
  work=$(mktemp -d) || return 1
  cat > "$work/cfg" <<CFG
[req]
distinguished_name = dn
x509_extensions = ext
prompt = no
[dn]
CN = $own
[ext]
basicConstraints = critical,CA:false
keyUsage = critical,digitalSignature
extendedKeyUsage = critical,codeSigning
CFG
  # /usr/bin/openssl on purpose: its .p12 files are the kind the keychain can read.
  /usr/bin/openssl req -x509 -newkey rsa:2048 -nodes -days 3650 -config "$work/cfg" \
      -keyout "$work/key.pem" -out "$work/cert.pem" 2>/dev/null &&
    /usr/bin/openssl pkcs12 -export -inkey "$work/key.pem" -in "$work/cert.pem" \
      -out "$work/id.p12" -passout pass:lokal -name "$own" &&
    security import "$work/id.p12" -k "$keychain" -P lokal -T /usr/bin/codesign >/dev/null &&
    say "macOS fragt jetzt nach deinem Passwort oder Touch ID, damit das Zertifikat für Signaturen gilt." \
        "macOS now asks for your password or Touch ID, so the certificate counts for signing." &&
    security add-trusted-cert -r trustRoot -p codeSign -k "$keychain" "$work/cert.pem"
  local status=$?
  rm -rf "$work"
  [[ $status == 0 ]] && valid "$own" || return 1
  # A grant given to an earlier throwaway build would only get in the way.
  tccutil reset Accessibility de.lakecode.dockodile >/dev/null 2>&1
  return 0
}

identity=${DOCKODILE_SIGN:-}
if [[ -z $identity ]]; then
  if valid "$own"; then
    identity=$own
  elif valid "Lake Code Lokal"; then
    identity="Lake Code Lokal"
  elif [[ -t 0 && ! -e .build/ohne-signatur ]]; then
    if [[ -n $german ]]; then
      cat <<TEXT

Dockodile braucht den Zugriff "Bedienungshilfen". macOS merkt ihn sich anhand der Signatur der App.
Ohne eigenes Zertifikat musst du den Zugriff nach jedem Build neu erteilen.

Das Skript kann einmalig ein Zertifikat "$own" in deinem Anmelde-Schlüsselbund anlegen.
Es ist selbst signiert, gilt nur auf diesem Mac und wird nirgends hochgeladen.
Entfernen kannst du es jederzeit in der Schlüsselbundverwaltung.

TEXT
      read -r -p "Zertifikat jetzt anlegen? [j/N] " answer
    else
      cat <<TEXT

Dockodile needs "Accessibility" access. macOS remembers it by the app's signature.
Without a certificate of your own you have to grant access again after every build.

This script can create a certificate "$own" in your login keychain, once.
It is self-signed, valid on this Mac only and is not uploaded anywhere.
You can remove it at any time in Keychain Access.

TEXT
      read -r -p "Create the certificate now? [y/N] " answer
    fi
    if [[ $answer == [jJyY]* ]]; then
      if create_identity; then
        identity=$own
        say "Zertifikat angelegt. Fragt macOS beim Signieren nach dem Schlüssel, wähle \"Immer erlauben\"." \
            "Certificate created. If macOS asks about the key while signing, choose \"Always Allow\"."
      else
        say "Das hat nicht geklappt. Der Build läuft mit Wegwerf-Signatur weiter." \
            "That did not work. The build continues with a throwaway signature."
      fi
    else
      # Remember the answer. Delete .build/ohne-signatur to be asked again.
      touch .build/ohne-signatur
    fi
  fi
fi
identity=${identity:--}

# --- Build ----------------------------------------------------------------------------------
{
  pkill -x Dockodile && sleep 1   # Dockodile hands its windows back before it quits
  pkill -x Klette && sleep 1      # the same app under its former name
  rm -rf "$PWD/Dockodile.app" &&
  mkdir -p Dockodile.app/Contents/MacOS Dockodile.app/Contents/Resources &&
  cp Info.plist Dockodile.app/Contents/Info.plist &&
  cp Icon/Dockodile.icns Dockodile.app/Contents/Resources/AppIcon.icns &&
  cp Icon/MenuBar.png Icon/MenuBar@2x.png Icon/Logo.png Icon/Logo@2x.png Dockodile.app/Contents/Resources/ &&
  xcrun --sdk macosx swiftc -swift-version 5 -O -target "$(uname -m)-apple-macos14.0" \
    -module-cache-path "$PWD/.build" src/*.swift -o Dockodile.app/Contents/MacOS/Dockodile &&
  codesign --force --sign "$identity" Dockodile.app &&
  codesign --verify --strict Dockodile.app &&
  echo "signiert mit: $identity"
  echo "exit=$?"
} > build.log 2>&1
tail -n 3 build.log
if grep -q "^exit=0" build.log; then
  open "$PWD/Dockodile.app"
else
  say "Der Build ist fehlgeschlagen. Einzelheiten stehen in build.log." \
      "The build failed. See build.log for details."
fi
