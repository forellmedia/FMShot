# 0008 – Repo, Lizenz, Signatur, Verteilung

**Status:** angenommen

## Entscheidung
- Öffentliches Repo `forellmedia/Nabshot`, **keine Lizenz-Datei** (alle Rechte
  vorbehalten). Eine Open-Source-Lizenz kann später ergänzt werden.
- **Keine fremden Pull Requests** vorerst; Issues willkommen.
- `cargo-deny` blockiert Abhängigkeiten mit Copyleft-Lizenzen (GPL/AGPL/LGPL),
  damit ein späterer Verkauf möglich bleibt.
- Spätere Pro-Funktionen (Cloud, Video) kommen in eigene Crates.
- **macOS-Signatur** mit selbst erstelltem Zertifikat „Nabshot Local Signing“
  (wie Faster2Bed), stabile Identität über Updates → Bildschirmaufnahme-Freigabe
  bleibt erhalten. Keine Notarisierung bis zum Verkauf.
- Windows-Signatur erst vor dem Verkauf.
- Releases und **Tauri-Updater** über GitHub Releases desselben Repos.
- App-ID: `ch.forellmedia.fmshot`.
- Keine Telemetrie.
