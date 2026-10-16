# Testanleitung (macOS)

Stand: Nacht vom 16. auf den 17. Oktober 2026. Alles ist für **macOS**
umgesetzt; Windows und Linux bauen in der CI, nehmen aber noch nicht auf
(siehe „Noch offen“).

## Vorbereitung

```bash
cd ~/04_Software/FMShot/apps/desktop
npm install
npm run tauri build -- --bundles app
open ../../target/release/bundle/macos/FMShot.app
```

FMShot erscheint als Symbol in der Menüleiste (kein Dock-Symbol, solange der
Editor zu ist). Beim ersten Aufnehmen fragt macOS nach **Bildschirmaufnahme** –
erlauben und FMShot über das Menüleisten-Symbol → *FMShot beenden* schliessen
und neu öffnen.

> Solange das eigene Zertifikat nicht eingerichtet ist
> (`./scripts/make-signing-cert.sh`), fragt macOS nach **jedem neuen Build**
> erneut nach der Freigabe.

## Tastenkürzel

| Aktion            | Kürzel |
|-------------------|--------|
| Bereich           | ⇧⌘2    |
| Vollbild          | ⇧⌘1    |
| Fenster           | ⌥⌘3    |
| Scroll-Aufnahme   | ⌥⌘4 (nochmals drücken = Fertig) |
| Text erfassen     | ⌥⌘5    |
| Galerie / Editor  | ⌥⌘G    |
| Einstellungen     | ⌘, (im Editor) |

Alle Aufnahmemodi gibt es auch im Menüleisten-Symbol.

## Checkliste

### 1. Bereich (⇧⌘2)
- [ ] Overlay erscheint sofort; **Fadenkreuz über den ganzen Bildschirm**
- [ ] Lupe neben dem Zeiger, Pixelmasse beim Ziehen
- [ ] Esc oder Rechtsklick bricht ab
- [ ] Nach dem Loslassen: **Vorschau an der Maus** (Kopieren · Speichern ·
      Bearbeiten · Text), Bild liegt schon in der Zwischenablage
- [ ] Vorschau-Bild in Mail/Slack/Finder **ziehen**
- [ ] Klick aufs Vorschau-Bild öffnet den Editor
- [ ] Einfügen in Vorschau/Keynote: Retina-Bild in **normaler Grösse**

### 2. Fenster (⌥⌘3) und Vollbild (⇧⌘1)
- [ ] Fenster unter der Maus wird blau hervorgehoben, Klick nimmt es auf
      (runde Ecken durchsichtig)
- [ ] Vollbild nimmt den Monitor auf, auf dem die Maus ist

### 3. Text erfassen (⌥⌘5)
- [ ] Bereich mit Text wählen → Hinweis „Text kopiert (n Zeichen)“
- [ ] Umlaute stimmen

### 4. Scroll-Aufnahme (⌥⌘4)
- [ ] Bereich wählen (z. B. Inhalt einer Webseite), dann langsam scrollen
- [ ] Leiste zeigt die Höhe; „Fertig“ oder erneut ⌥⌘4 beendet
- [ ] Ergebnis öffnet im Editor, ohne doppelte Zeilen
- [ ] Einstellungen → Allgemein → Scroll-Aufnahme „FMShot scrollt
      automatisch“: fragt einmal nach **Bedienungshilfen**, scrollt dann selbst

### 5. Editor
- [ ] Werkzeuge: Pfeil (A), Linie (L), Rechteck (R), Ellipse (O),
      Freihand (P), Text (T), Sprechblase (B), Nummern (N), Marker (H),
      Spotlight (S), Lupe (M), Weichzeichnen (U), Verpixeln (X),
      Zuschneiden (C), Hintergrund (G), Auswählen (V)
- [ ] Griffe zum Ändern, Shift = gerade Linien / Quadrate
- [ ] Text direkt tippen, Umlaute, Esc beendet
- [ ] ⌘Z / ⇧⌘Z, ⌘C kopiert das Bild (mit Auswahl: die Ebene), ⌘D dupliziert
- [ ] Bild aus dem Finder hineinziehen, ⌘V fügt Bild aus der
      Zwischenablage ein, Galerie-Bild auf die Leinwand ziehen
- [ ] Bild-Ebene auswählen, Zuschneiden-Werkzeug → nur das Bild wird
      zugeschnitten
- [ ] Hintergrund (G): Verlauf wählen → Rand, runde Ecken, Schatten
- [ ] „Text kopieren“ (T-Symbol oben) markiert erkannte Zeilen, Klick
      kopiert eine Zeile
- [ ] „Auto-Schwärzen“ verpixelt E-Mail-Adressen, IBANs, Telefonnummern
- [ ] Ebenen-Panel: Sichtbarkeit, Sperre, Reihenfolge
- [ ] PNG / JPG exportieren, „Kopieren“
- [ ] Zoom: Pinch / ⌘-Mausrad, ⌘0 einpassen, ⌘1 = 100 %; Leertaste + Ziehen
      verschiebt

### 6. Galerie
- [ ] Neue Aufnahmen erscheinen unten; Rechtsklick: Stern, Im Finder
      zeigen, Löschen
- [ ] **Suche** findet Aufnahmen nach Text darin (z. B. ein Wort aus einer
      Fehlermeldung)
- [ ] Ablage: `~/Library/Application Support/ch.forellmedia.fmshot/Library/2026/10/`
      – alles flach im Monatsordner

### 7. Einstellungen (⌘, oder Menüleiste → Einstellungen …)
- [ ] Nach der Aufnahme: Vorschau / Editor / nur kopieren
- [ ] Tastenkürzel neu belegen (Feld anklicken, Kombination drücken)
- [ ] Auto-Export einschalten, Ordner wählen → jede Aufnahme landet dort,
      Änderungen im Editor aktualisieren die Datei
- [ ] Sprache Englisch → Oberfläche und Menüleiste wechseln
- [ ] Info → Messwerte (Taste → Overlay in ms), „Fehler melden …“ öffnet
      ein vorausgefülltes GitHub-Issue

## Noch offen
- **Windows und Linux:** Aufnahme, Overlay, Texterkennung, Vorschau und
  Zwischenablage-Feinheiten fehlen noch (App startet, Aufnahme meldet
  „noch nicht umgesetzt“).
- Signatur-/Updater-Secrets auf GitHub (Issue #2) – danach erster Release.
- Eigenes Menüleisten-Symbol (noch das App-Symbol).
- Emoji im Text-Werkzeug (Schrift Inter hat keine Emoji).
- Stil-Vorlagen und Ausrichtungshilfen im Editor.
