# 0001 – Tauri v2 + Svelte 5 als Oberfläche

**Status:** angenommen

## Kontext
Ein Snagit-artiger Editor ist viel UI-Arbeit (Panels, Galerie, Einstellungen,
Texteingabe mit IME). Reines Rust-GUI (egui/iced) wäre langsamer zu bauen und
weniger poliert; Flutter (wie RustDesk) hat schwache Multi-Window-Unterstützung.

## Entscheidung
- **Tauri v2** als App-Hülle, Frontend in **Svelte 5 + TypeScript + Vite**,
  plain CSS mit Design-Tokens (kein Tailwind).
- Rust↔TS-Typen werden mit `tauri-specta` aus Rust erzeugt, nie von Hand gepflegt.
- **Regel:** Sämtliche Logik liegt in Rust-Crates unter `crates/`, die nichts
  von Tauri oder der Webview wissen. Das Frontend zeichnet nur Bedienelemente.

## Folgen
- Rust + TypeScript im Projekt.
- Unter Linux ist WebKitGTK die Schwachstelle – deshalb zeichnet die Webview
  weder die Leinwand (ADR 0002) noch das Auswahl-Overlay (ADR 0003).
