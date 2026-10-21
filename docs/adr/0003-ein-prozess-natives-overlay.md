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
  gezeichnet mit tiny-skia über `softbuffer` (`grabmark-overlay`). Das eingefrorene
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

### Nachtrag (Windows): GDI mit geänderten Rechtecken, physische Pixel
- **Fenster:** pro Monitor ein randloses `WS_POPUP` mit `WS_EX_TOPMOST |
  WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_LAYERED` – die vorherige App
  behält den Fokus. Esc kommt über ein für die Dauer des Overlays
  registriertes Tastenkürzel (`RegisterHotKey`), weil ein nie aktives Fenster
  keine Tastendrücke bekommt. Alle eigenen Fenster setzen
  `WDA_EXCLUDEFROMCAPTURE` und sind in keiner Aufnahme zu sehen; der Editor
  wird für die Dauer einer Aufnahme genauso ausgeblendet.
- **Bereich:** Das Overlay erscheint sofort mit Deckkraft 1/255 (unsichtbar,
  nimmt aber Klicks an) und wird deckend, sobald das eingefrorene Bild da ist.
  Es liegt einmal hell und einmal abgedunkelt als DIB vor (eine Kopie beim
  Einfrieren). Pro Mausbewegung werden nur die geänderten Rechtecke
  (Fadenkreuz-Streifen, Lupe, Rand der Auswahl) im Hintergrundpuffer neu
  zusammengesetzt und per `BitBlt` auf den Bildschirm kopiert – nie der ganze
  Monitor. Direct2D/DirectComposition wäre schneller, lohnt den Aufwand aber
  nicht, solange die Messwerte im Budget bleiben.
- **Fenster-Modus:** ohne eingefrorenes Bild; eine Ebene mit Alpha pro Pixel
  (`UpdateLayeredWindowIndirect`, nur geänderte Rechtecke) dunkelt den
  laufenden Bildschirm ab und hebt das Fenster blau hervor.
- **Koordinaten:** Grabmark ist „per monitor aware v2“ und rechnet unter
  Windows in physischen Pixeln des virtuellen Desktops. Mit gemischten
  Skalierungen gibt es dort kein durchgehendes logisches System;
  `Display::scale_factor` ist 1.0, die Skalierung des Monitors (z. B. 1.5)
  steckt in `Frame::scale_factor` und landet als DPI in PNG und
  Zwischenablage (96 dpi × Skalierung).

## Folgen
- Leerlauf ca. 60–120 MB RAM (Webview).
- Unter Wayland bestimmt das Portal die Latenz (100–500 ms).
