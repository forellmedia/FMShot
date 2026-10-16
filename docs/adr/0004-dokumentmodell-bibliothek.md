# 0004 – Dokumentmodell und Bibliothek

**Status:** angenommen

## Entscheidung
### Dokument
- **Alles ist eine Ebene** (Screenshot, Bild, Pfeil, Text …) mit Transformation,
  Zuschnitt und Deckkraft. Der Screenshot ist die unterste Ebene (gesperrt).
- **Zeichenfläche** (Artboard) startet in Screenshot-Grösse, ist vergrösserbar,
  „An Inhalt anpassen“. Export = Bereich der Zeichenfläche.
- Nicht-destruktiv: `original.png` wird nie verändert; Ebenen stehen in
  `doc.json` (versioniertes Schema). Erst beim Export wird flachgerechnet.
- Hineingezogene Bilder werden nach `assets/<hash>.png` kopiert (Dokument bleibt
  vollständig).

### Bibliothek
```
Library/
  index.sqlite            Metadaten, Tags, OCR-Volltext (FTS5)
  2026/10/<id>/
    original.png
    doc.json
    thumb.webp
    assets/<hash>.png
```
- Jede Aufnahme landet **immer** sofort dort (Absturzsicherheit).
- Nach der Aufnahme bleibt das Bild im Speicher; PNG wird im Hintergrund mit
  schneller Kompression geschrieben.
- **Auto-Export** (Einstellung) schreibt zusätzlich PNG/JPG in einen Ordner,
  aktualisiert bei Änderungen.
- **Aufbewahrung**: Standard „nie löschen“, optional „nach N Tagen, ausser ⭐“.
