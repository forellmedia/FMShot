# 0003 – Ein Prozess im Tray, natives Auswahl-Overlay

**Status:** angenommen

## Kontext
Ziel: Tastenkombination → Overlay sichtbar in < 50 ms. Eine Webview kalt zu
starten dauert 300–800 ms; transparente Vollbild-Webviews sind unter Linux und
bei gemischtem HiDPI unzuverlässig.

## Entscheidung
- **Ein Prozess**, dauerhaft im Tray. Editor wird beim ersten Öffnen erzeugt und
  danach versteckt statt geschlossen.
- Das **Auswahl-Overlay** ist ein natives Fenster pro Monitor ohne Webview,
  gezeichnet mit tiny-skia über `softbuffer` (`fmshot-overlay`). Das eingefrorene
  Bild bleibt im Speicher, nichts wird kopiert. Fensterebenen pro Plattform
  nativ (macOS: über Menüleiste und Vollbild-Spaces).
- Das Vorschau-Popup darf den Fokus nicht stehlen (macOS: nicht aktivierendes
  `NSPanel`, Windows: `WS_EX_NOACTIVATE`).

## Folgen
- Leerlauf ca. 60–120 MB RAM (Webview).
- Unter Wayland bestimmt das Portal die Latenz (100–500 ms).
