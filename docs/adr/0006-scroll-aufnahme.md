# 0006 – Scroll-Aufnahme: manuell und automatisch

**Status:** angenommen

## Entscheidung
- Einstellung mit zwei Modi; **manuell** ist Standard.
- Manuell: Bildstrom (macOS ScreenCaptureKit, Windows Desktop Duplication /
  Windows Graphics Capture, Linux PipeWire bzw. X11) + Live-Zusammensetzen.
- Automatisch: simulierte Scroll-Eingaben (`enigo`); unter Wayland nicht
  verfügbar, macOS fragt Bedienungshilfen-Berechtigung erst beim Einschalten.
- Das Zusammensetzen liegt plattformunabhängig in `fmshot-stitch` (Zeilen-Hashes
  / Kreuzkorrelation, fixe Kopf-/Fusszeilen) und wird mit gespeicherten
  Bildfolgen getestet.

## Folgen
Die Bildstrom-Backends sind gleichzeitig die Grundlage der späteren Video-Aufnahme.
