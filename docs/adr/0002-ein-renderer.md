# 0002 – Ein Renderer in Rust, nativ und als WASM

**Status:** angenommen

## Kontext
Dasselbe Dokument muss im Editor, beim Export, beim Auto-Export (ohne Fenster),
als Galerie-Vorschau und über die CLI identisch aussehen.

## Entscheidung
- `grabmark-render` (tiny-skia + cosmic-text, mitgelieferte Schriften) zeichnet
  Dokumente. Nativ für Export/Auto-Export/Vorschau/CLI, als **WASM** im Editor.
- Im Editor werden unveränderte Ebenen als Bitmap gecacht; beim Ziehen wird nur
  die aktive Ebene neu gezeichnet.
- Griffe, Hilfslinien und Text-Cursor sind HTML/SVG darüber. Beim Text-Tippen
  liegt ein echtes Textfeld über der Leinwand (IME, Umlaute, Emoji); nach
  Bestätigung zeichnet Rust.
- Qualitätssicherung: Snapshot-Tests (Dokument → PNG → Vergleich mit Referenz).

## Folgen
- Eine einzige Wahrheit, wie ein Dokument aussieht.
- Renderer liegt hinter einem Trait; späterer Wechsel auf GPU (vello/wgpu) ohne
  Änderung am Dokumentmodell möglich.
