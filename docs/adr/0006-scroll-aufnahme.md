# 0006 – Scroll-Aufnahme: manuell und automatisch

**Status:** angenommen

## Entscheidung
- Einstellung mit zwei Modi; **manuell** ist Standard.
- Manuell: Bildstrom (macOS ScreenCaptureKit, Windows Desktop Duplication /
  Windows Graphics Capture, Linux PipeWire bzw. X11) + Live-Zusammensetzen.
- Automatisch: simulierte Scroll-Eingaben (`enigo`); unter Wayland nicht
  verfügbar, macOS fragt Bedienungshilfen-Berechtigung erst beim Einschalten.
- Das Zusammensetzen liegt plattformunabhängig in `nabshot-stitch` (Zeilen-Hashes
  / Kreuzkorrelation, fixe Kopf-/Fusszeilen) und wird mit gespeicherten
  Bildfolgen getestet.

### Nachtrag (Windows)
Der Bildstrom ist vorerst `BitBlt` des Ausschnitts im Takt der Scroll-Schleife
(wenige Millisekunden pro Bild). Das läuft auch in VMs mit einfachem
Grafiktreiber, wo Desktop Duplication oft fehlt. Desktop Duplication bzw.
Windows Graphics Capture folgen mit der Video-Aufnahme. Automatisch gescrollt
wird mit `SendInput` (Mausrad, 120 Einheiten ≈ 100 logische Pixel); Windows
verlangt dafür keine Freigabe.

## Folgen
Die Bildstrom-Backends sind gleichzeitig die Grundlage der späteren Video-Aufnahme.
