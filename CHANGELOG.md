# Changelog

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
- ⌘Q schliesst nur den Editor. Beendet wird FMShot über das Menü.
- Beim ersten Screenshot erscheint nur noch Apples Freigabe-Dialog.
- Vorlagen sind wieder wählbar, die Kopfleiste läuft nicht mehr über den
  Rand, und Farbe und Stärke gezeichneter Elemente lassen sich wieder
  ändern.
- Das Bild lässt sich nicht mehr aus dem Fenster schieben, und es flackert
  beim Zeichnen nicht mehr.
- Für die macOS-Kürzel ⇧⌘3/4/5 erkennt FMShot, ob sie noch belegt sind, und
  öffnet auf Knopfdruck die passenden Systemeinstellungen.

**Hinweise**
- Das Paket ist durch das Bildmodell rund 400 MB gross.
- Intel-Macs werden nicht mehr unterstützt (ADR 0009).
- Windows und Linux sind in dieser Beta noch nicht ausführlich getestet.

## 0.1.0

Erste Version.
