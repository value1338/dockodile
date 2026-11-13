# Dockodile für Windows 11 – Version 0.4.1

Tabs für alle Programme unter Windows. Geschrieben in Rust, eine einzelne
`dockodile.exe` (ca. 500 KB), keine Installation, keine Zusatzprogramme nötig.

**Stand:** Auf Linux gebaut und dort unter Wine getestet. Version 0.2.0 lief auf einem echten
Windows 11; die neuen Funktionen von 0.3.0 liefen dort noch nie. Dafür ist die Testliste unten.

## Neu in 0.4.1

- **Plus öffnet zuverlässig.** Ein Klick auf das Plus gab zuerst dem Fenster der Gruppe die Tastatur;
  das Fenster konnte sie sich kurz danach zurückholen und die Auswahlliste damit sofort wieder
  schließen. Jetzt kommt die Gruppe nur nach vorn, die Liste behält die Tastatur.
- **Fenster dockt beim ersten Loslassen an.** Änderte sich beim Ziehen die Größe des Fensters, etwa
  weil es vorher maximiert oder angedockt war oder auf einen Monitor mit anderer Skalierung
  wechselte, hielt Dockodile das für ein Vergrößern und nahm es nicht auf. Beides sind vermutete
  Ursachen, auf echtem Windows noch nicht bestätigt.

## Neu in 0.4.0

- **Zweireihige Tabs.** Rechtsklick auf einen Tab oder auf die Leiste, dann „Zweireihige Tabs“. Die
  Einstellung gilt für alle Gruppen und bleibt gespeichert. Die zweite Reihe erscheint erst, wenn die
  Tabs in einer Reihe schmaler als etwa 150 Pixel würden; bis dahin bleibt alles einreihig. Auf einer
  1200 Pixel breiten Gruppe passen so 6 Tabs in die erste Reihe, auf voller Full-HD-Breite 11. Das
  Plus steht hinter dem letzten Tab, Minimieren, Maximieren und Schließen bleiben oben rechts. Tabs
  lassen sich auch zwischen den Reihen umsortieren. Unter Wine mit fünf Fenstern geprüft.

## Neu in 0.3.4

- **Menüs auf dem richtigen Bildschirm.** Bei mehreren Monitoren mit unterschiedlicher Skalierung konnte
  das Menü des Symbols auf dem falschen Bildschirm erscheinen und keine Klicks annehmen. Vermutete
  Ursache behoben; mit mehreren Monitoren ließ sich das hier nicht nachstellen.

## Neu in 0.3.3

- Im Menü des Symbols steht nicht mehr „(Vorschau)“.

## Neu in 0.3.2

- **Herausgezogener Tab folgt der Maus flüssig.** Bisher bewegte sich ein Fenster, das als Tab aus
  der Leiste gezogen wurde, nur ruckelnd mit, wie mit 30 Bildern pro Sekunde. Jetzt wird es einmal
  pro Bildschirmbild nachgeführt. Unter Wine geprüft, dass das Herausziehen weiter funktioniert; ob
  es auf echtem Windows flüssig aussieht, lässt sich nur dort beurteilen.

## Neu in 0.3.1

- **Kein Konflikt mehr mit Tabs von Chrome, Edge, Explorer, Terminal und Editor.** Ziehst du einen
  Tab eines Programms auf ein anderes Fenster desselben Programms, macht Dockodile daraus keine
  Gruppe mehr, das Programm übernimmt den Tab wie gewohnt. Soll Dockodile zwei Fenster desselben
  Programms doch gruppieren: beim Loslassen **Umschalt** halten, oder das Plus in der Leiste nutzen.
  Auf eine bestehende Tab-Leiste von Dockodile lässt sich jedes Fenster weiter ziehen.

## Neu in 0.3.0

- **Minimieren und Maximieren** als Symbole rechts in der Tab-Leiste, neben dem Kreuz.
- **Klick auf die Leiste holt die Gruppe nach vorn**, egal wohin: freie Fläche, Plus, Maximieren,
  Kreuz, Rechtsklick. Bisher ging das nur über einen Programm-Tab.
- **Updates in der App:** Dockodile sucht beim Start und einmal am Tag nach einer neuen Version,
  lädt sie mit dem Schlüssel deines Kaufs, ersetzt sich selbst und startet neu.

## Neu in 0.2.0

- **Behoben:** Beim Ziehen einer Gruppe blieben die Fenster der anderen Tabs stehen. Jetzt ziehen
  alle Fenster der Gruppe sofort mit, an der Tab-Leiste wie an der Titelleiste.
- **Pakete:** Gruppe unter einem Namen sichern und per Klick, Kürzel oder Link wieder öffnen.
- **Kürzel:** Jedes Paket bekommt ein systemweites Kürzel, frei änderbar.
- **Programme suchen:** Das Plus öffnet eine Liste mit Suchfeld; ab zwei Buchstaben erscheinen
  die Programme aus dem Startmenü.
- **Ungelesen-Zähler:** rote Zahl am Tab.
- **Größe ändern an der Leiste:** obere Kante und die beiden oberen Ecken der Tab-Leiste.
- **Steuerung per Link:** `dockodile://open?paket=…` und `dockodile://tab?app=…`.
- **Englisch:** Sprache im Menü am Krokodil.

## 1. Starten

1. Läuft noch die alte Version: Rechtsklick aufs Krokodil → „Beenden“.
2. `dockodile.exe` auf den Windows-PC kopieren, z. B. nach `C:\Tools\Dockodile\`.
3. Doppelklick. Windows zeigt „Der Computer wurde durch Windows geschützt“, weil die Datei nicht
   signiert ist: **Weitere Informationen → Trotzdem ausführen**.
4. Unten rechts im Infobereich erscheint das Krokodil (eventuell hinter dem Pfeil `^`).
   Ein Klick darauf öffnet das Menü: Pakete, „Alle Gruppen auflösen“, „Mit Windows starten“,
   „Sprache“, „Beenden“.

Meldet der Virenscanner die Datei: Das passiert bei unsignierten, frisch gebauten Programmen
häufig. Für den Test eine Ausnahme setzen oder die Datei selbst bauen (Abschnitt 10).

## 2. Bedienung

| Aktion | So geht es |
|---|---|
| Fenster zu Tabs machen | Fenster an der Titelleiste auf den oberen Rand eines anderen ziehen, bei „Loslassen zum Tabben“ loslassen |
| Fenster zur Gruppe hinzufügen | Auf die Tab-Leiste ziehen, oder Plus in der Leiste |
| Programm starten und anhängen | Plus, mindestens zwei Buchstaben tippen, Programm wählen |
| Tab wechseln | Tab anklicken, oder per Alt-Tab / Taskleiste zum Programm wechseln |
| Tabs umsortieren | Tab seitlich ziehen |
| Tab lösen | Tab nach unten oder oben herausziehen, Kreuz im Tab, oder Rechtsklick auf den Tab |
| Gruppe verschieben | Freien Teil der Tab-Leiste ziehen, oder das Fenster an seiner Titelleiste |
| Größe ändern | Fenster am Rand ziehen, oder obere Kante / obere Ecken der Tab-Leiste |
| Bildschirm füllen | Maximieren-Symbol oder Doppelklick auf den freien Teil der Leiste, nochmal stellt zurück |
| Gruppe minimieren | Minimieren-Symbol in der Leiste |
| Gruppe nach vorn holen | Irgendwo auf die Leiste klicken |
| Gruppe auflösen | Kreuz ganz rechts in der Leiste |
| Gruppe als Paket sichern | Plus oder Rechtsklick auf Tab/Leiste → „Diese Gruppe als Paket sichern …“, Namen tippen, Enter |
| Paket öffnen | Menü am Krokodil, das Kürzel des Pakets, oder ein Link |
| Kürzel ändern, Paket löschen | Menü am Krokodil → Paket |

Im Suchfenster: Pfeiltasten wählen, Enter bestätigt, Esc schließt.

## 3. Pakete und Kürzel

Ein Paket merkt sich die Programme einer Gruppe, ihre Reihenfolge sowie Platz und Größe. Der
Inhalt der Fenster wird nicht gespeichert.

Beim Öffnen sammelt Dockodile die offenen Fenster der Programme ein, startet fehlende Programme
und wartet bis zu 30 Sekunden auf ihre Fenster. Ist das Paket schon offen, kommt seine Gruppe nach
vorn. Ist der Monitor von damals nicht angeschlossen, öffnet sich die Gruppe auf dem Hauptbildschirm.

Neue Pakete bekommen das erste freie Kürzel aus **Alt+Umschalt+1 bis 9**. Dockodile lehnt ein
Kürzel ab, wenn

- es schon zu einem anderen Paket gehört,
- Windows oder ein anderes Programm es schon belegt,
- weder Strg noch Alt enthalten ist,
- es nur aus Strg+Alt besteht. Das ist für Windows dasselbe wie AltGr und würde auf deutschen
  Tastaturen Zeichen wie `{`, `[` und `@` blockieren. Deshalb weicht die Vorgabe vom Mac ab.

Pakete und Sprache liegen in `%APPDATA%\Dockodile\dockodile.txt`.

## 4. Updates

Ein Kauf auf dockodile.de gilt für Mac und Windows. Damit Dockodile Updates selbst installiert,
verknüpfst du es einmal mit deinem Kauf: Auf der Seite nach dem Kauf oder auf
`dockodile.de/update.html` (Link per E-Mail) „In Dockodile freischalten“ klicken, während
Dockodile läuft.

Danach fragt Dockodile beim Start und einmal am Tag nach der neuesten Version. Ist keine Gruppe
offen, installiert es sie ohne zu fragen, sonst meldet es sich mit einer Sprechblase. Im Menü am
Krokodil unter „Updates“: jetzt suchen, automatisch an/aus, freischalten.

Beim Installieren lädt Dockodile die neue `dockodile.exe` in seinen Ordner, benennt die laufende
Datei in `dockodile.old.exe` um, setzt die neue an ihre Stelle und startet neu. Die alte Datei
löscht es beim nächsten Start. Liegt Dockodile in einem Ordner ohne Schreibrecht, etwa unter
„Programme“, geht das nicht; dann schickt es dich auf die Update-Seite. Deshalb: eigener Ordner
wie `C:\Tools\Dockodile\`.

Die Datei kommt nur über HTTPS von dockodile.de. Weil sie noch nicht signiert ist, prüft
Dockodile außer der Verbindung nur, dass es eine Windows-Programmdatei ist.

## 5. Steuerung per Link

```
start "" "dockodile://open?paket=Büro"
start "" "dockodile://tab?app=Editor"
```

`open` öffnet ein Paket oder holt seine Gruppe nach vorn. `tab` zeigt den Tab, dessen Programm oder
Titel den Namen enthält. Findet Dockodile nichts, ertönt der Warnton. Die Links gehen auch über
Win+R, aus Skripten und aus dem Browser. Dockodile trägt sich dafür beim Start als Programm für
`dockodile://` ein (nur für dein Benutzerkonto, unter `HKCU\Software\Classes\dockodile`).

## 6. Testliste

Bitte der Reihe nach durchgehen und notieren, was nicht klappt. Zum Testen reichen Editor
(Notepad), Explorer und ein Browser.

**Neu in 0.3.1**
- [ ] Chrome-Tab aus einem Chrome-Fenster ziehen und in ein anderes Chrome-Fenster einhängen: kein gelber Balken, Chrome übernimmt den Tab
- [ ] Dasselbe mit Umschalt gehalten: gelber Balken, Dockodile gruppiert die Fenster
- [ ] Editor auf Chrome ziehen (verschiedene Programme): gruppiert wie bisher

**Neu in 0.3.0**
- [ ] Minimieren-Symbol: Gruppe verschwindet, über die Taskleiste zurückholen: Leiste ist wieder da
- [ ] Maximieren-Symbol füllt den Bildschirm, das Symbol wechselt, zweiter Klick stellt zurück
- [ ] Fährt die Maus über Minimieren, Maximieren, Kreuz: Knopf wird hell, das Kreuz rot
- [ ] Anderes Fenster halb über die Gruppe legen, freie Fläche der Leiste anklicken: Gruppe kommt nach vorn
- [ ] Dasselbe mit Plus und Rechtsklick auf die Leiste
- [ ] Menü am Krokodil → Updates → „Nach Updates suchen“: „Dockodile ist aktuell“ (solange auf dem Server nichts Neueres liegt)

**Der in 0.2.0 behobene Fehler**
- [ ] Gruppe aus drei Fenstern am freien Teil der Leiste ziehen: kein Fenster bleibt stehen
- [ ] Dieselbe Gruppe an der Titelleiste des aktiven Fensters ziehen: kein Fenster bleibt stehen
- [ ] Danach jeden Tab anklicken: alle Fenster liegen am neuen Platz
- [ ] Ziehen fühlt sich mit vier oder mehr Tabs noch flüssig an

**Grundfunktion** (in 0.1.0 schon vorhanden)
- [ ] Editor auf den oberen Rand des Explorers ziehen: gelber Balken, Loslassen bildet die Gruppe
- [ ] Tab anklicken wechselt das Fenster, Tab seitlich ziehen sortiert um
- [ ] Tab herausziehen und wieder andocken, Kreuz im Tab löst das Fenster
- [ ] Minimieren und über die Taskleiste zurückholen, Maximieren, Doppelklick auf die Leiste
- [ ] Kreuz rechts → „Gruppe auflösen“: Fenster liegen gestaffelt, alle erreichbar

**Suchfenster am Plus**
- [ ] Plus zeigt die übrigen offenen Fenster, Klick oder Enter hängt eins an
- [ ] „rech“ tippen: „Starten: Rechner“ erscheint, Auswahl startet ihn und hängt ihn an
- [ ] Esc oder Klick daneben schließt das Suchfenster

**Größe**
- [ ] Obere Kante der Leiste ziehen ändert die Höhe, die Ecken auch die Breite
- [ ] Danach haben alle Tabs die neue Größe

**Pakete**
- [ ] Rechtsklick auf Tab → „Diese Gruppe als Paket sichern …“, Name „Test“, Enter: Meldung mit Kürzel
- [ ] Gruppe auflösen, Alt+Umschalt+1: Gruppe steht wieder am alten Platz, Tabs in alter Reihenfolge
- [ ] Ein Programm des Pakets schließen, Paket öffnen: das Programm startet und kommt als Tab dazu
- [ ] Paket öffnen, während es offen ist: Gruppe kommt nach vorn
- [ ] Menü am Krokodil → Paket → „Kürzel ändern …“: Strg+Alt+K wird abgelehnt, Strg+Umschalt+K angenommen
- [ ] Gruppe ändern, sichern, „Aktualisieren: Test“ wählen: Paket ist aktualisiert
- [ ] Paket löschen: fragt nach, Kürzel ist danach frei

**Sonstiges**
- [ ] Browser-Tab mit „(3) …“ im Titel (z. B. WhatsApp Web, ein Postfach): rote 3 am Tab
- [ ] Win+R, `dockodile://open?paket=Test`: Paket öffnet sich
- [ ] Win+R, `dockodile://tab?app=Editor`: der Tab kommt nach vorn
- [ ] Sprache auf English und zurück
- [ ] Zweiter Monitor und Skalierung 125 % / 150 %: Leiste scharf, Gruppe lässt sich hinüberziehen

## 7. Fehler melden

Mit Protokoll starten: Win+R, dann

```
C:\Tools\Dockodile\dockodile.exe --log
```

Dockodile schreibt dann `dockodile.log` in denselben Ordner (Ordner muss beschreibbar sein).
Darin steht, welches Fenster bewegt wurde, welches Ziel erkannt wurde, wann Gruppen entstehen und
was beim Öffnen eines Pakets passiert. Fenstertitel stehen im Klartext drin. Für einen
Fehlerbericht bitte schicken:

1. was du gemacht hast und was passiert ist,
2. die `dockodile.log`,
3. bei Darstellungsfehlern einen Screenshot.

## 8. Bekannte Grenzen dieser Version

- **Ungelesen-Zähler nur aus dem Fenstertitel.** Windows lässt kein Programm die Zahl am
  Taskleisten-Symbol eines anderen lesen. Dockodile zeigt deshalb die Zahl, die ein Programm vorn
  in seinen Titel schreibt, etwa „(3) WhatsApp“. Das tun Browser und viele Messenger, Outlook und
  Teams als Programm nicht.
- **Pakete mit Store-Apps** (Rechner, Einstellungen, neues Outlook): Dockodile erkennt deren
  Fenster nur am Titel und kann sie nicht selbst starten. Sind sie offen, kommen sie mit.
- **Programme mit Startbildschirm** können beim Öffnen eines Pakets kurz mit dem falschen Fenster
  auftauchen.
- **Startet das Plus ein Programm, das schon läuft**, und es öffnet kein neues Fenster, hängt sich
  nichts an. Dann das Fenster aus der Liste wählen.
- **Startende Programme** erscheinen erst als Tab, wenn ihr Fenster da ist, nicht vorab als
  pulsierender Platzhalter wie auf dem Mac.
- **Ziehen wird nicht bei jedem Programm erkannt.** Programme mit ganz eigener Titelleiste melden
  Windows das Verschieben nicht immer. Klappt das Andocken per Ziehen nicht: Plus benutzen.
- **Programme mit Administratorrechten** (z. B. Task-Manager) lassen sich nicht bewegen, solange
  Dockodile ohne Administratorrechte läuft.
- **Fenster mit fester Größe** passen sich nicht an die Gruppe an.
- **Durchsichtige Fenster** (z. B. Windows Terminal mit Acryl) zeigen die Tabs dahinter.
- **Hängt ein Programm**, lässt Dockodile sein Fenster stehen, bis es wieder antwortet.
- **Virtuelle Desktops** und **Vollbild-Programme** sind nicht berücksichtigt.
- Alt-Tab und Taskleiste zeigen weiter jedes Programm einzeln. Das ist Absicht.

Anders als auf dem Mac werden wartende Fenster nicht an den Bildschirmrand geschoben: Alle Fenster
einer Gruppe liegen deckungsgleich übereinander, das aktive oben. Stürzt Dockodile ab, bleibt
deshalb jedes Fenster erreichbar.

## 9. Was gegenüber der Mac-Version noch fehlt

- Signatur (ohne sie warnt SmartScreen)
- Auszeichnung der Tab-Leiste für Bedienungshilfen

## 10. Selbst bauen

Auf Windows:

1. Rust installieren: https://rustup.rs (fragt nach den „Visual Studio Build Tools“ und richtet sie ein).
2. Im Ordner `Quelltext`: `cargo build --release`
3. Ergebnis: `target\release\dockodile.exe`

Auf Linux (so ist die beiliegende Datei entstanden):

```
sudo apt install gcc-mingw-w64-x86-64
rustup target add x86_64-pc-windows-gnu
cargo build --release --target x86_64-pc-windows-gnu
```

Der Code steht in `src/main.rs` (Fenster, Leiste, Menüs) und `src/store.rs` (Pakete, Links,
Kürzelnamen). `rustc --test src/store.rs` prüft den zweiten Teil auf jedem System.


## Microsoft-Store-Build

Seit dem Store-Paket 0.4.1 wird mit dem Cargo-Feature `microsoft-store` gebaut. Diese Ausgabe bezieht Updates ausschließlich über den Microsoft Store. Der Web-Kaufschlüssel und der EXE-Selbstersatz gehören nur zur direkten Website-Ausgabe. `build-store.sh AUSGABEORDNER` baut und prüft die Store-Ausgabe und erzeugt ein neues MSIX mit der bestehenden Store-Identität. Einzelheiten und Prüfgrenzen: [MSIX.md](MSIX.md).
