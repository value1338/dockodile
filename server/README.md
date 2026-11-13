# Webdienst: Kauf- und Update-Logik

Diese Klassen enthalten die Kaufprüfung, Downloadberechtigung, Vertragsbestätigung und Update-Mails. Betriebszugänge, Produktionskonfiguration und Bestelldaten sind nicht enthalten.

Die Tests verwenden ausschließlich eine SQLite-Datenbank im Arbeitsspeicher und künstliche Bestelldaten. Benötigt werden PHP ab 8.1 mit PDO/SQLite.

```sh
php server/tests/update.php
php server/tests/confirmation.php
```

Der Update-Link wird nach einer erfolgreichen Übergabe an den Mailtransport erneuert. Wird die Übergabe abgelehnt, bleiben die Laufzeit des bisherigen Links und das Wiederholungsfenster unverändert. Eine angenommene Mail ist noch kein Beleg für den Eingang im Postfach.
