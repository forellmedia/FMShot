# 0014 – Oberfläche im Stil „Glas“

**Status:** angenommen

## Kontext
Die Oberfläche wirkte nach der ersten Überarbeitung (Phosphor-Symbole, neues
Tray-Fenster) noch nicht modern. Aus drei ausgearbeiteten Entwürfen (Glas,
Sucher, Studio) hat der Nutzer im Oktober 2026 „Glas“ gewählt, angelehnt an
Apples Liquid Glass (macOS 26): durchscheinende Flächen, schwebende
Leisten, weiche Tiefe. Die Bildliste sitzt dabei rechts als Spalte, mit
fester Suche oben.

## Entscheidung
- **Fenster unter macOS durchsichtig mit Vibrancy:** Haupt-, Einstellungs-
  und Tray-Fenster sind transparent und bekommen einen Glas-Hintergrund des
  Systems (`windowEffects`: Sidebar bzw. Popover). Die Titelleiste liegt über
  dem Inhalt (`titleBarStyle: Overlay`), Kopfleisten ziehen das Fenster
  (`data-tauri-drag-region`).
- Dafür braucht Tauri `macOSPrivateApi` (Feature `macos-private-api`). Für den
  App Store wäre das ein Ausschlussgrund – Nabshot wird dort nicht
  angeboten (ADR 0008), darum annehmbar.
- Die Fenstereinstellungen nur für macOS stehen in
  `src-tauri/tauri.macos.conf.json` bzw. hinter `#[cfg(target_os = "macos")]`.
  Unter Windows und Linux bleibt alles deckend: Die Glas-Tokens
  (`--glass`, `--glass-strong`, `--glass-edge`, `--glass-shadow`) fallen ohne
  die Klasse `glass` auf die normalen Flächenfarben zurück.
- **Gestaltung:** Leisten und Werkzeuge als Kapseln, Leinwand und Bildliste
  als abgerundete Flächen; Symbole aus Phosphor „Light“ (feine Linie, MIT).
  Blau zeigt Gewähltes, Grün eingeschaltete Schalter, Rot nur Aufnehmen und
  Löschen.

## Folgen
- Inhalte, die durchscheinen, hängen vom Schreibtisch dahinter ab; Texte
  liegen deshalb immer auf einer Glasfläche mit genug Deckkraft, nie direkt
  auf dem Fensterhintergrund.
- Im Browser (`npm run dev`) gibt es keine Vibrancy; dort zeigt ein
  Ersatz-Hintergrund (`glass-preview`), wie das Glas wirkt.
