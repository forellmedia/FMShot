# Testanleitung

Stand: 17. Oktober 2026. Alles ist für **macOS** umgesetzt und getestet.
**Windows** ist umgesetzt und baut in der CI, muss aber noch von Hand in der
VM geprüft werden ([Abschnitt Windows](#windows-utm-vm-windows-11-arm)).
Linux baut in der CI, nimmt aber noch nicht auf (siehe „Noch offen“).

# macOS

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
- [ ] Emoji im Text (⌃⌘Leertaste): farbig, auch ❤️, 👍🏽, 🇨🇭; im Export gleich
- [ ] Drehen: runder Griff über der Ebene ziehen (⇧ = 15°-Schritte) oder
      Gradzahl oben eingeben; „Gerade“ setzt zurück; gedrehter Text bleibt
      bearbeitbar (Doppelklick); Griffe ändern die Grösse, die Gegenseite
      bleibt stehen
- [ ] Mehrfachauswahl: ⇧-Klick oder Rahmen aufziehen (Auswählen-Werkzeug),
      ⌘A; gemeinsam verschieben (Maus/Pfeiltasten), drehen, löschen
- [ ] Gruppen: ⌘G gruppiert, ⇧⌘G löst auf; Klick wählt die ganze Gruppe,
      ⌘-Klick eine einzelne Ebene darin; ⌘C/⌘V und ⌘D kopieren die Gruppe;
      Ebenen-Panel zeigt „Gruppe n“ mit Sichtbar/Sperre/Reihenfolge
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

# Windows (UTM-VM, Windows 11 ARM)

Was neu ist: Aufnahme über GDI, eigenes Overlay (randlose Fenster, die nie
den Fokus nehmen), Texterkennung mit Windows.Media.Ocr, Zwischenablage mit
PNG + DIB (inkl. DPI), Vorschau mit Drag & Drop. Bisher nur gebaut und mit
Unit-Tests geprüft – **noch nie auf einem echten Desktop gelaufen.**

## Vorbereitung (einmalig in der VM)

1. **Visual Studio 2022 Build Tools** mit „Desktopentwicklung mit C++“; unter
   „Einzelne Komponenten“ zusätzlich *MSVC … ARM64/ARM64EC-Buildtools*.
2. **Rust** über <https://rustup.rs> (wählt `aarch64-pc-windows-msvc`), dann
   ```bash
   rustup target add wasm32-unknown-unknown
   cargo install wasm-bindgen-cli --version 0.2.129
   ```
3. **Node.js 22** (ARM64) und **Git für Windows**. Alle Befehle unten in
   **Git Bash** ausführen (das WASM-Skript braucht `bash`).
4. Repository in der VM klonen (nicht im geteilten Ordner bauen – zu langsam).

```bash
cd ~/FMShot/apps/desktop
npm install
npm run tauri build -- --bundles nsis
```

Installer: `target/release/bundle/nsis/FMShot_0.1.0_arm64-setup.exe`.
Schneller zum Ausprobieren: `npm run tauri dev`.

FMShot erscheint im Infobereich der Taskleiste (ggf. unter dem Pfeil ^).
Windows fragt nicht nach einer Freigabe für Bildschirmaufnahme.

> **Druck-Taste:** Windows 11 öffnet damit das Snipping Tool. Für FMShot
> unter *Einstellungen → Barrierefreiheit → Tastatur* „Mit der Druck-Taste
> Bildschirmausschnitt öffnen“ ausschalten (danach FMShot neu starten).

## Tastenkürzel

| Aktion            | Kürzel |
|-------------------|--------|
| Bereich           | Strg+Umschalt+2 oder Druck |
| Vollbild          | Strg+Umschalt+1 |
| Fenster           | Strg+Umschalt+3 |
| Scroll-Aufnahme   | Strg+Umschalt+4 (nochmals drücken = Fertig) |
| Text erfassen     | Strg+Umschalt+5 |
| Galerie / Editor  | Strg+Alt+G |

## Checkliste

### W1. Bereich (Strg+Umschalt+2)
- [ ] Overlay erscheint sofort: Bildschirm leicht abgedunkelt, eingefroren,
      **Fadenkreuz über den ganzen Monitor**
- [ ] Die App darunter **behält den Fokus** (Titelleiste bleibt aktiv,
      Taskleiste wechselt nicht)
- [ ] Lupe neben dem Zeiger (Pixel scharf, rotes Kästchen = Pixel unter der
      Maus), Pixelmasse beim Ziehen; Ziehen in jede Richtung
- [ ] Nur klicken (ohne Ziehen) lässt das Overlay offen
- [ ] **Esc** und **Rechtsklick** brechen ab; danach funktioniert Esc in
      anderen Programmen wieder normal
- [ ] Ziehen ruckelt nicht (auch bei grossen Bereichen)
- [ ] Nach dem Loslassen: **Vorschau an der Maus** (Kopieren · Speichern ·
      Bearbeiten · Text), nimmt keinen Fokus, verschwindet nach ~6 s (bleibt,
      solange die Maus darauf ist)
- [ ] Vorschau-Bild in den **Explorer**, in Outlook/Mail oder einen Browser
      ziehen → Datei kommt an (wird kopiert, nicht verschoben)
- [ ] Klick aufs Vorschau-Bild öffnet den Editor; ✕ schliesst

### W2. Zwischenablage
- [ ] Einfügen in **Paint** (Bild vollständig, Farben stimmen)
- [ ] Einfügen in **Word**/WordPad/Outlook: bei *Anzeige → Skalierung 150 %*
      in **normaler Grösse**, nicht 1,5-fach
- [ ] „Text erfassen“ → Einfügen im Editor: Zeilenumbrüche stimmen

### W3. Fenster (Strg+Umschalt+3) und Vollbild (Strg+Umschalt+1)
- [ ] Bildschirm wird abgedunkelt, Fenster unter der Maus **blau
      hervorgehoben** (laufender Bildschirm, nicht eingefroren)
- [ ] Klick nimmt das Fenster auf – auch wenn ein anderes Fenster es teilweise
      verdeckt; runde Ecken (Windows 11) durchsichtig; kein Schatten/Rand
- [ ] Browser-Fenster (Edge) ist nicht schwarz
- [ ] Vollbild nimmt den Monitor auf, auf dem die Maus ist
- [ ] Ein offener FMShot-Editor ist **nicht** in der Aufnahme zu sehen

### W4. Text erfassen (Strg+Umschalt+5)
- [ ] Bereich mit Text → Hinweis „Text kopiert (n Zeichen)“ unten in der Mitte
- [ ] Umlaute stimmen; auch ein einzelnes kleines Wort wird erkannt
- [ ] Fehlt die Texterkennung: Deutsch unter *Zeit und Sprache → Sprache*
      hinzufügen (oder PowerShell als Admin:
      `Add-WindowsCapability -Online -Name "Language.OCR~~~de-DE~0.0.1.0"`)

### W5. Scroll-Aufnahme (Strg+Umschalt+4)
- [ ] Roter Rahmen um den Bereich, Leiste darunter; Mausrad und Klicks gehen
      durch den Rahmen an die Seite
- [ ] Langsam scrollen → Leiste zeigt die Höhe; „Fertig“ oder erneut
      Strg+Umschalt+4 beendet; Ergebnis ohne doppelte Zeilen, ohne roten Rahmen
- [ ] Einstellung „FMShot scrollt automatisch“: scrollt ohne Rückfrage selbst
      (Edge, Explorer, Einstellungen-App)

### W6. Skalierung und Monitore
- [ ] *Anzeige → Skalierung* auf 125 % / 150 %: Auswahl hat die richtige
      Pixelgrösse, Lupe/Schrift/Leisten sind entsprechend grösser
- [ ] Mit zwei Monitoren (falls die VM das kann): Overlay auf beiden,
      Fadenkreuz wechselt mit der Maus, Ziehen bleibt auf dem Start-Monitor

### W7. Rest
- [ ] Editor, Galerie und Einstellungen öffnen und zeigen die Aufnahmen
- [ ] Ablage: `%APPDATA%\ch.forellmedia.fmshot\Library\2026\10\`
- [ ] Info → Messwerte: „Taste → Overlay sichtbar“ unter 50 ms,
      „Taste → Bildschirm eingefroren“ notieren

# Noch offen
- **Linux:** Aufnahme, Overlay, Texterkennung, Vorschau und
  Zwischenablage-Feinheiten fehlen noch (App startet, Aufnahme meldet
  „noch nicht umgesetzt“).
- **Windows:** Bildstrom über Desktop Duplication / Windows Graphics Capture
  (erst mit der Video-Aufnahme nötig). Exportierte PNG-Dateien tragen 72 dpi
  × Skalierung (macOS-Basis) – unter Windows wären 96 dpi richtig.
- Signatur-/Updater-Secrets auf GitHub (Issue #2) – danach erster Release.
- Eigenes Menüleisten-Symbol (noch das App-Symbol).
