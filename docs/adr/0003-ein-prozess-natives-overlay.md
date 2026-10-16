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

### Nachtrag (Meilenstein 2): macOS ohne softbuffer
Auf macOS zeichnet das Overlay nicht mit tiny-skia/softbuffer, sondern mit
**Core-Animation-Ebenen** in einem nicht aktivierenden `NSPanel`: Das
eingefrorene Bild liegt einmal als Textur auf der GPU, beim Ziehen ändern sich
nur Pfade (Abdunklung, Rahmen) und Positionen (Lupe, Grössenanzeige).
softbuffer kopiert bei jeder Mausbewegung den ganzen Puffer – bei 5K-Monitoren
rund 60 MB pro Bild und damit zu langsam. Das Bild wird ohne Kopie an Core
Graphics übergeben (`CGDataProvider` auf dem `Arc<RgbaImage>`).
Windows und Linux entscheiden wir bei ihrer Umsetzung nach demselben Massstab.

## Folgen
- Leerlauf ca. 60–120 MB RAM (Webview).
- Unter Wayland bestimmt das Portal die Latenz (100–500 ms).
