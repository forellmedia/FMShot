# CLAUDE.md – Arbeitsregeln für FMShot

FMShot ist ein schnelles, plattformübergreifendes Screenshot-Tool (Rust + Tauri v2
+ Svelte 5). Produktumfang: [docs/produkt.md](docs/produkt.md).
Architektur-Entscheidungen: [docs/adr/](docs/adr/). Vor grösseren Änderungen die
passenden ADRs lesen; neue Grundsatzentscheidungen als neue ADR festhalten.

## Aufbau

```
crates/
  fmshot-core      Dokumentmodell (Ebenen, doc.json), gemeinsame Typen – kein I/O
  fmshot-render    Renderer (tiny-skia), nativ + WASM
  fmshot-capture   Bildschirmaufnahme (Einzelbild + Stream), je Plattform
  fmshot-stitch    Scroll-Bilder zusammensetzen – plattformunabhängig
  fmshot-ocr       Trait OcrEngine + Vision / Windows.Media.Ocr / Tesseract
  fmshot-vision    Bildinhalt: Bild/Suchtext → Vektor (SigLIP 2, ONNX) – plattformunabhängig
  fmshot-library   Bibliothek: Dateien, SQLite-Index, Suche, Auto-Export
  fmshot-clipboard Zwischenablage (macOS: PNG mit Retina-Auflösung)
  fmshot-hotkeys   globale Tastenkombinationen
  fmshot-overlay   natives Auswahl-Overlay (macOS: NSPanel + Core Animation)
  fmshot-cli       Kommandozeile `fmshot`
  fmshot-bench     Geschwindigkeitsmessungen mit Budgets (läuft in der CI)
apps/desktop/      Tauri-App: src-tauri/ (Rust) + src/ (Svelte)
```

## Regeln

1. **Logik gehört in `crates/`.** `apps/desktop/src-tauri` verdrahtet nur
   (Commands, Fenster, Tray). Das Svelte-Frontend zeichnet nur Bedienelemente –
   die Leinwand zeichnet `fmshot-render`.
2. **Plattformcode nur in `capture`, `clipboard`, `ocr`, `hotkeys`, `overlay`**, jeweils hinter
   einem Trait, Implementierungen in `src/<plattform>.rs` per `#[cfg(target_os)]`.
   Alle anderen Crates sind plattformunabhängig und ohne Bildschirm testbar.
3. **Abhängigkeitsrichtung:** `core` hängt von nichts Internem ab; `render`,
   `stitch`, `library` nur von `core`. Keine Zyklen, keine Tauri-Abhängigkeit in
   `crates/`.
4. **Rust↔TS-Typen** werden mit `tauri-specta` erzeugt, nie von Hand gepflegt.
   Neue Commands in `specta_builder()` (src-tauri/src/lib.rs) eintragen, dann
   `cargo test -p fmshot-desktop` – die CI prüft, ob `bindings.ts` aktuell ist.
5. **Lizenzen:** Keine Copyleft-Abhängigkeiten (GPL/AGPL/LGPL). `cargo deny check`
   muss grün sein.
6. **Geschwindigkeit ist eine Funktion.** Budgets in docs/produkt.md. Nichts
   Blockierendes im Aufnahmepfad; Datei-I/O und Kodierung im Hintergrund.
7. **Texte der Oberfläche** nie fest im Code – immer über die Übersetzungsdateien
   (de + en).
8. **Keine Telemetrie**, keine Netzwerkzugriffe ausser Updater-Prüfung.

## Stil
- Bezeichner Englisch, Kommentare und Dokumentation Deutsch.
- `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`.
- Fehler: `thiserror` in Bibliotheks-Crates, `anyhow` nur in Apps/CLI.
- Tests neben dem Code (`#[cfg(test)]`); Snapshot-Tests des Renderers unter
  `crates/fmshot-render/tests/`.

## Befehle

```bash
cargo test --workspace                 # alle Rust-Tests
./scripts/fetch-model.sh               # Bildmodell nach models/ (ADR 0010, braucht uv)
cargo test -p fmshot-vision --release -- --ignored  # Bildinhalt mit echtem Modell
cargo clippy --workspace --all-targets -- -D warnings
cargo deny check                       # Lizenzen/Advisories
cargo test -p fmshot-desktop           # erzeugt apps/desktop/src/lib/bindings.ts
cargo run --profile bench -p fmshot-bench  # Geschwindigkeit messen (Budgets)
cd apps/desktop && npm test             # Frontend-Tests (Vitest)
cd apps/desktop && npm run tauri dev   # App starten
cd apps/desktop && npm run dev         # nur Frontend im Browser (Beispieldaten)
```

## Ablauf
- Arbeit pro Meilenstein-Issue auf GitHub.
- **Zurzeit Phase „Apple Silicon“ (ADR 0009):** Nach jeder Änderung
  `./scripts/build-local.sh` (baut, installiert nach /Applications, startet).
  Der Nutzer testet; committen und pushen erst nach seiner Freigabe.
  Windows/Linux müssen kompilieren, Funktionen dürfen dort vorerst fehlen.
  Kein Intel-Mac.
- Keine fremden Pull Requests annehmen.
- App-ID `ch.forellmedia.fmshot` nie ändern (macOS knüpft Berechtigungen daran).
