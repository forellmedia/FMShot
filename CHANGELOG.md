# Changelog

## Unveröffentlicht

**Verbessert**
- Nach der Aufnahme öffnet sich standardmässig der Editor (bisher die
  Vorschau). Wer die Einstellung schon geändert hat, behält seine Wahl.
- Tastenkürzel lassen sich mit ✕ ganz entfernen; die Funktion bleibt im
  Menü. „Pause / Weiter“ zeigt, dass dasselbe Kürzel die Aufnahme fortsetzt.
- Tray: Das gewählte Ziel (Bereich, Fenster, Vollbild) ist auch unter
  Windows und Linux deutlich markiert, lange Kürzel unter dem Auslöser
  brechen um statt abgeschnitten zu werden.

**Behoben**
- Ein Screenshot per Tastenkürzel konnte grau werden oder den eben
  versteckten Editor zeigen (macOS). Jetzt kommen nur Nabshot-Fenster ins
  Bild, die gerade zu sehen sind.
- Nach der Aufnahme öffnet sich der Editor auch, wenn gerade die Galerie
  offen ist. Bis die neue Aufnahme da ist, zeigt er „Neue Aufnahme wird
  geladen …“ statt des vorigen Bildes.
- ⌘H blendet Editor und Einstellungen aus, das Dock-Symbol bleibt; ein
  Klick darauf holt sie zurück. Der Editor blitzt beim nächsten Screenshot
  nicht mehr auf (macOS).

## 0.2.0-beta.3

**Neu**
- **Oberfläche im Stil „Glas“** (ADR 0014): Leisten als schwebende Kapseln,
  Bildliste als Spalte rechts; auf dem Mac durchscheinend.
- **Speicher aufräumen:** Aufnahmen werden standardmässig 90 Tage
  aufbewahrt, Bibliothek und Export-Ordner haben je eine Warngrenze. Eine
  Anzeige unten zeigt den Platz, ein Klick öffnet das Aufräumen.
- **Rechtsklick in der Galerie:** Öffnen, Stern, im Finder / Explorer
  zeigen, Löschen. Im Editor neu **„In Galerie zeigen“**.
- **Im Finder / Explorer zeigen** öffnet die Datei im Export-Ordner, wenn
  Aufnahmen zusätzlich als Datei gespeichert werden; bei Videos das Video.
- **Video-Editor:** Ton mit Wellenform und Lautstärke-Punkten, Ein- und
  Ausblenden, Bildraten 24–60. Die Zeitleiste hat eine feste Höhe, die sich
  am oberen Rand ziehen lässt; Spuren lassen sich ein- und ausblenden.
- Ausführliches Log mit einstellbarer Aufbewahrung (Einstellungen → Info).

**Verbessert**
- **Windows:** Nabshot wird für alle Benutzer nach „Programme\Nabshot“
  installiert statt unter AppData. Eine frühere Installation unter AppData
  (auch unter dem Namen Grabmark) wird dabei entfernt; Aufnahmen und
  Einstellungen bleiben, ebenso „Beim Anmelden starten“.
- Zeichnen im Video-Editor ist auch mit vielen Pfeilen flüssig.
- Videos werden deutlich schneller erstellt und lassen sich mit Rückfrage
  abbrechen; eine abgebrochene Aufnahme lässt sich wiederherstellen.
- Tray: Einstellungen stehen in der Fusszeile, oben nur noch Bild,
  Bildschirm und Webcam.
- Galerie: Schnellfilter, die nicht in eine Zeile passen, stehen hinter
  „Mehr…“.
- „Im Finder zeigen“ heisst unter Windows „Im Explorer zeigen“ und unter
  Linux „Im Dateimanager zeigen“.

## 0.2.0-beta.2

**Neu**
- **Neuer Name: Nabshot** (vorher Grabmark, ADR 0015). Aufnahmen,
  Einstellungen und Logs werden beim ersten Start übernommen; macOS fragt
  die Freigaben einmal neu ab.
- **Video aufnehmen** (macOS, ADR 0011): Bereich, Fenster oder Vollbild mit
  Ton (⇧⌘6), Countdown, Aufnahmeleiste mit Pause (⇧⌘9), Stopp und Pegel.
  - Webcam als Blase, auf Wunsch nur der Kopf mit weichem Rand und
    unscharfem oder entferntem Hintergrund
  - Nur Webcam aufnehmen in 16:9, 9:16 oder 1:1 (⇧⌘7)
  - **Live zeichnen** während der Aufnahme (⇧⌘8): Pfeil, Linie, Rahmen,
    Kreis, Freihand, Nummer, Textmarker. Verschwindet nach ein paar
    Sekunden oder mit Esc. In den Einstellungen wählbar, ob Zeichnen nach
    einer Markierung an bleibt.
  - Eigener Mauszeiger, Klicks als Kreis, Vergrössern (auch automatisch
    aus den Klicks)
- **Video-Editor** in der gewohnten Leinwand: Schneiden, Trimmen, Webcam je
  Abschnitt, Lautstärke, Pfeile und Text mit Zeitbereich. Die Zeitleiste hat
  eine Zeile je Werkzeug; Markierungen und Klicks lassen sich dort wählen,
  verschieben, verlängern und löschen. Ausgabe als MP4, GIF oder Einzelbild.
- **Einstellungen → Video:** Auflösung, Codec (H.264/HEVC), Qualität und
  Bilder pro Sekunde für das fertige Video.
- **Tray-Fenster mit Tabs** Bild · Bildschirm · Webcam · Einstellungen, dazu
  „Beim Anmelden starten“ und **„Editor öffnen“**.
- **Speichern als …** (⇧⌘S bzw. Ctrl+Shift+S) fragt nach dem Format (PNG
  oder JPG).
- **Galerie:** Ein Tag-Symbol neben dem Stern zeigt die Stichwörter einer
  Aufnahme; ein Klick auf ein Stichwort sucht danach.

**Verbessert**
- ⌘C und ⌘S (Ctrl+C / Ctrl+S) kopieren bzw. speichern das ganze Bild auch
  direkt nach dem Zeichnen, ohne vorher Esc. Eine markierte Ebene lässt sich
  danach in Nabshot trotzdem mit ⌘V einfügen.
- Das Rechtsklick-Menü der Leiste unten im Editor wird am rechten Rand nicht
  mehr abgeschnitten.
- Fehlende Freigaben öffnen direkt die Systemeinstellungen.
- Abhängigkeiten mit Sicherheitsmeldungen aktualisiert (rustls-webpki,
  webpki-root-certs).

**Hinweise**
- Video-Aufnahme gibt es vorerst nur auf dem Mac. Unter Windows und Linux
  funktionieren Screenshots, Editor und Galerie wie bisher.

## 0.2.0-beta.1

**Neu**
- **Suche nach Bildinhalt:** Die Galerie findet Aufnahmen auch über das, was
  darauf zu sehen ist, etwa „Wald“, „Hund“, „Code“ oder „Diagramm“. Die
  Erkennung läuft im Hintergrund auf dem Gerät, nichts verlässt den Computer
  (ADR 0010).
  - Bis zu drei Stichwörter je Aufnahme
  - Schnellfilter mit Anzahl, zum Beispiel Personen, Natur oder Code
  - Jede Kachel zeigt, warum sie gefunden wurde
- **Galerie im Vollbild** (⌥⌘G bzw. Ctrl+Alt+G):
  - Suche, Zeitraum, Art, Sortierung und Favoriten
  - Kacheln nach Tagen gruppiert
- **Editor:**
  - Linienarten, gebogene Pfeile mit Spitze gefüllt/offen in S/M/L
  - Text als Kasten mit Hintergrund und Ausrichtung
  - Nummern mit Startwert
  - Spotlight mit runden Ecken und weichem Rand, Lupe mit Griff
  - „Unkenntlich machen“ (weichzeichnen oder verpixeln)
- **Kopieren und Speichern** als Symbole in der Kopfleiste. Das Format
  (PNG/JPEG) wählt man in den Einstellungen.
- **Scroll- und Text-Aufnahme** lassen sich direkt aus dem Editor starten.

**Verbessert**
- ⌘Q schliesst nur den Editor. Beendet wird Nabshot über das Menü.
- Beim ersten Screenshot erscheint nur noch Apples Freigabe-Dialog.
- Vorlagen sind wieder wählbar, die Kopfleiste läuft nicht mehr über den
  Rand, und Farbe und Stärke gezeichneter Elemente lassen sich wieder
  ändern.
- Das Bild lässt sich nicht mehr aus dem Fenster schieben, und es flackert
  beim Zeichnen nicht mehr.
- Für die macOS-Kürzel ⇧⌘3/4/5 erkennt Nabshot, ob sie noch belegt sind, und
  öffnet auf Knopfdruck die passenden Systemeinstellungen.

**Hinweise**
- Das Paket ist durch das Bildmodell rund 400 MB gross.
- Intel-Macs werden nicht mehr unterstützt (ADR 0009).
- Windows und Linux sind in dieser Beta noch nicht ausführlich getestet.

## 0.1.0

Erste Version.
