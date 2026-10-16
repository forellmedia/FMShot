---
name: Release-Test
about: Checkliste für den manuellen Test einer Version auf einer Plattform
title: "Release-Test vX.Y.Z – <Plattform>"
labels: release-test
---

**Version:** vX.Y.Z
**Plattform / Desktop:** (z. B. Windows 11 ARM in UTM, Ubuntu 24.04 GNOME Wayland)
**Monitore / Skalierung:** (z. B. 1 × 2560×1440 @ 150 %)

### Installation
- [ ] Installiert und startet, Tray-Symbol sichtbar
- [ ] Updater meldet neue Version (wenn vorhanden)

### Aufnahme
- [ ] Bereich (Tastenkürzel) – Overlay erscheint sofort, Lupe und Masse stimmen
- [ ] Vollbild
- [ ] Fenster
- [ ] Scroll manuell
- [ ] Scroll automatisch (nicht unter Wayland)
- [ ] Text erfassen (OCR) – Umlaute korrekt
- [ ] Mehrere Monitore / unterschiedliche Skalierung

### Nach der Aufnahme
- [ ] Vorschau an der Maus, stiehlt keinen Fokus
- [ ] Drag & Drop aus der Vorschau in eine andere App
- [ ] Zwischenablage enthält das Bild
- [ ] Einstellung „Editor öffnen“ / „Nur kopieren“ wirkt

### Editor & Galerie
- [ ] Alle Werkzeuge zeichnen, Rückgängig/Wiederholen
- [ ] Bild hineinziehen, skalieren, zuschneiden
- [ ] Export PNG/JPG sieht aus wie im Editor
- [ ] Galerie-Suche findet Text aus Screenshots
- [ ] Auto-Export schreibt Datei in den Ordner

### Auffälligkeiten
(Geschwindigkeit, Flackern, falsche Farben …)
