# Microsoft-Store-Paket

Die Website-Ausgabe und die Store-Ausgabe werden aus demselben Windows-Quelltext gebaut. Das Cargo-Feature `microsoft-store` schaltet den kaufgebundenen Web-Updater ab. Der Update-Menüpunkt öffnet stattdessen die Downloads und Updates im Microsoft Store.

## Paket bauen

`build-store.sh AUSGABEORDNER` baut auf Linux mit dem GNU-Windows-Ziel, prüft das Binary und erzeugt ein unsigniertes MSIX zur Einreichung. Vorher Rust, das Ziel `x86_64-pc-windows-gnu`, MinGW und das offizielle [Microsoft MSIX SDK](https://github.com/microsoft/msix-packaging) mit aktivierter Pack-Unterstützung einrichten. `MAKEMSIX` auf dessen ausführbares Werkzeug setzen. Ohne diese Variable wird `~/.local/lib/dockodile-msix/bin/makemsix` verwendet.

```sh
rustup target add x86_64-pc-windows-gnu
MAKEMSIX=/pfad/zum/makemsix ./build-store.sh /pfad/zur/ausgabe
```

Das Skript überschreibt keine vorhandene Paketdatei. Paketversion und Dateiname folgen der Version in `Cargo.toml`. Die bestehende Store-Identität in `msix/AppxManifest.xml` bleibt erhalten. Für eine andere Store-App müssen Name und Publisher zum eigenen Partner-Center-Eintrag passen.

Der Store-Build 0.4.1.0 wurde am 13. November 2026 eingereicht. Cross-Build, sieben Tests der Einstellungslogik, Prüfung auf fehlende Web-Updater-Endpunkte und Paketvalidierung sind bestanden. Eine vollständige Installation und Aktualisierung unter Windows wurde für diesen Build noch nicht praktisch geprüft.
