# FMShot – Produktbeschreibung

Schnelles, plattformübergreifendes Screenshot-Tool mit Editor und Galerie im Stil
von Snagit. Zuerst für den Eigengebrauch und Freunde, später kommerziell –
Schwerpunkt macOS, wo gute, günstige Screenshot-Tools fehlen.

## Umfang v0.1

### Aufnahme
- **Bereich**, **Vollbild**, **Fenster** – mehrere Monitore, HiDPI, Bildschirm wird
  beim Auslösen eingefroren.
- **Scroll-Aufnahme**, in den Einstellungen umschaltbar:
  - *Manuell* (Standard): Nutzer scrollt selbst, FMShot setzt den Bildstrom live
    zusammen. Funktioniert überall, auch unter Wayland.
  - *Automatisch*: FMShot scrollt per simulierter Eingabe. Unter Wayland
    ausgegraut; macOS fragt die Bedienungshilfen-Berechtigung erst beim Einschalten.
- **Text erfassen (OCR)**: Bereich wählen → Text direkt in die Zwischenablage.

### Nach der Aufnahme
Einstellbar (global, pro Aufnahmemodus überschreibbar):
- **A** Editor öffnen
- **B** nur Zwischenablage
- **C** Schnellzugriff-Vorschau (Standard) – erscheint an der Mausposition
  (Alternative: feste Ecke), stiehlt keinen Fokus, Drag & Drop in andere Apps,
  Knöpfe *Kopieren · Speichern · Bearbeiten · Text kopieren · ✕*.
  Das Bild liegt zusätzlich sofort in der Zwischenablage.

### Editor
- Snagit-artiger Aufbau: Eigenschaftenleiste oben, Werkzeuge links, Leinwand,
  Ebenen-Panel rechts (einklappbar), **Galerie-Leiste** unten mit Suche.
- Werkzeuge: Pfeil, Linie, Rechteck, Ellipse, Freihand · Text, Sprechblase,
  Nummern-Schritte · Textmarker, Spotlight, Lupe · Weichzeichnen, Verpixeln,
  Auto-Schwärzen (OCR) · Zuschneiden, Grösse ändern, Hintergrund/Rahmen.
- **Bild-Ebenen**: Bilder aus Galerie, Datei oder Zwischenablage hineinziehen;
  skalieren, drehen, nicht-destruktiv zuschneiden, Deckkraft, Reihenfolge.
- **Freie Leinwand** mit Zeichenfläche (Artboard); alles ist eine Ebene, auch der
  Screenshot (standardmässig gesperrt). Export = Bereich der Zeichenfläche.
- Markierungen bleiben dauerhaft änderbar (nicht-destruktiv).
- Ein-Tasten-Kürzel pro Werkzeug, Stil-Vorlagen, unbegrenztes Rückgängig.

### Galerie & Speicherung
- Jede Aufnahme landet sofort in der internen **Bibliothek** (SQLite-Index).
- **Volltextsuche** über OCR-Text aller Aufnahmen.
- **Auto-Export** (Einstellung): zusätzlich PNG/JPG in frei wählbaren Ordner,
  Dateinamen-Muster, wird bei Änderungen aktualisiert.
- **Aufbewahrung**: z. B. „nach 30 Tagen löschen, ausser ⭐“. Standard: nie löschen.

### Sonstiges
- Tray-App, globale Tastenkombinationen (alle konfigurierbar, Konflikterkennung).
- Kommandozeile `fmshot capture region|screen|window|scroll|text`.
- Oberfläche Deutsch + Englisch (folgt System, umschaltbar).
- Eingebauter Updater (GitHub Releases).
- **Keine Telemetrie.** Logs lokal; „Fehler melden“ öffnet vorausgefülltes
  GitHub-Issue ohne Screenshots.

### Nicht in v0.1
Video-Aufnahme, Cloud-Teilen, Lizenzschlüssel, Apple-Notarisierung,
Windows-Signatur, Flatpak.

## Standard-Tastenkombinationen

| Aktion          | macOS  | Windows / Linux          |
|-----------------|--------|--------------------------|
| Vollbild        | ⌘⇧1    | Ctrl+Shift+1             |
| Bereich         | ⌘⇧2    | Ctrl+Shift+2 / `Druck`   |
| Fenster         | ⌘⇧3*   | Ctrl+Shift+3             |
| Scroll          | ⌘⇧4*   | Ctrl+Shift+4             |
| Text (OCR)      | ⌘⇧5*   | Ctrl+Shift+5             |
| Galerie öffnen  | ⌘⇧G    | Ctrl+Shift+G             |

\* Vom System belegt. Erster Start bietet an, die Systemkürzel zu ersetzen
(mit Anleitung); sonst Ausweichkürzel ⌘⌥3/4/5.

## Geschwindigkeitsziele

| Messpunkt                              | Ziel        |
|----------------------------------------|-------------|
| Tastenkombination → Overlay sichtbar   | < 50 ms     |
| Auswahl losgelassen → Vorschau         | < 150 ms    |
| Editor öffnen (warm)                   | < 200 ms    |
| Leerlauf RAM / CPU                     | < 120 MB / ~0 % |
| Download-Grösse                        | < 25 MB     |

Unter Wayland gilt „so schnell wie das Portal erlaubt“.
Die App misst diese Werte selbst (Debug-Panel); Benchmarks in der CI warnen bei
Verschlechterung.

## Plattformen

| Plattform | Mindestversion               | Architekturen        |
|-----------|------------------------------|----------------------|
| macOS     | 14 Sonoma                    | Universal (arm64 + x86_64) |
| Windows   | 10 22H2, 11                  | x64, ARM64           |
| Linux     | Ubuntu 22.04+, Fedora 39+    | x86_64, aarch64      |

Linux offiziell: GNOME (Wayland), KDE Plasma (Wayland), X11. Andere Umgebungen
ohne Garantie, Kommandozeile als Weg für eigene Tastenkürzel.
Pakete: AppImage, .deb.

## Meilensteine

1. **Fundament** – Workspace, CI (3 OS × 2 Architekturen), Signatur, Release, Updater.
2. **Durchstich** – Tray → Tastenkürzel → Bereich (natives Overlay) → Zwischenablage + Bibliothek.
3. **Editor-Kern** – Dokumentmodell, WASM-Renderer, Grundwerkzeuge, Galerie-Leiste.
4. **Aufnahmemodi** – Vollbild, Fenster, Scroll (manuell, dann automatisch), OCR.
5. **Editor komplett** – Ebenen/Bilder, Weichzeichnen, Auto-Schwärzen, Hintergrund …
6. **Feinschliff** – Vorschau-Popup, Einstellungen, i18n, Auto-Export, Suche → **v0.1**.

Jedes Feature zuerst macOS, dann Windows, dann Linux. Die CI baut immer alle.

## Entwicklung & Tests
- Code schreibt hauptsächlich Claude; Jeremy reviewt und testet.
- Manuelle Tests: macOS nativ, Windows und Linux in UTM-VMs (ARM64), Freunde
  stichprobenartig auf echter Hardware. Checkliste als Issue-Vorlage.
- Vorerst keine fremden Pull Requests; Issues willkommen.
