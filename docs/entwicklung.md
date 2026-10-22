# Entwicklung

## Voraussetzungen
- Rust (stable, über [rustup](https://rustup.rs)) – `rust-toolchain.toml` legt den Rest fest.
- Node.js 22+ und npm.
- macOS: Xcode Command Line Tools.
- Linux: Pakete aus [`scripts/ci-linux-deps.sh`](../scripts/ci-linux-deps.sh).
- Windows: Visual Studio Build Tools (C++), WebView2 (bei Windows 11 dabei).

## Häufige Befehle

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo deny check
cargo run --profile bench -p nabshot-bench   # Geschwindigkeit mit Budgets
cd apps/desktop && npm install && npm run tauri dev
```

Lokal bauen und installieren (Apple Silicon, mit eigenem Zertifikat, ADR 0009):

```bash
./scripts/build-local.sh            # bauen, laufendes Nabshot beenden, nach /Applications, starten
./scripts/build-local.sh --no-install   # nur bauen
```

## Einmalige Einrichtung: Mac-Signatur

Damit macOS die Freigabe „Bildschirmaufnahme“ über Updates hinweg behält, wird
mit einem dauerhaften, selbst erstellten Zertifikat signiert (ADR 0008).

1. `./scripts/make-signing-cert.sh` ausführen (fragt nach dem Mac-Passwort).
2. Die zwei `gh secret set …`-Befehle ausführen, die das Skript am Ende ausgibt.
3. `~/.nabshot-signing/` sicher aufbewahren (z. B. Passwort-Manager).

Ohne Secrets signiert die CI ad-hoc – die App läuft, aber macOS fragt nach
jedem Update neu nach der Bildschirmaufnahme-Freigabe.

## Einmalige Einrichtung: Updater-Schlüssel

Updates werden mit einem eigenen Schlüssel signiert; die App prüft sie gegen den
öffentlichen Schlüssel in `tauri.conf.json` (`plugins.updater.pubkey`).

1. `./scripts/make-updater-key.sh` (schon erledigt – Schlüssel liegt in
   `~/.nabshot-signing/updater.key`).
2. Secrets setzen:
   ```bash
   gh secret set TAURI_SIGNING_PRIVATE_KEY -R forellmedia/Nabshot < ~/.nabshot-signing/updater.key
   gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD -R forellmedia/Nabshot < ~/.nabshot-signing/updater-password.txt
   ```
3. **Geht `updater.key` verloren, nehmen installierte Versionen keine Updates
   mehr an.** Zusammen mit dem Zertifikat sichern.

Ohne diese Secrets schlägt die Release-Pipeline fehl.

Die App sucht beim Start und danach alle 6 Stunden nach Updates
(`latest.json` am neuesten GitHub-Release). Unter Linux kann nur das AppImage
sich selbst aktualisieren, das .deb nicht.

## Release erstellen

1. Version in `Cargo.toml` (workspace), `apps/desktop/package.json` und
   `apps/desktop/src-tauri/tauri.conf.json` erhöhen.
2. `git tag v0.1.0 && git push origin v0.1.0`
3. Die Release-Pipeline baut macOS (arm64), Windows (x64, ARM64) und Linux
   (x86_64, aarch64) und legt einen **Release-Entwurf** an.
4. Entwurf auf GitHub prüfen und veröffentlichen.

## CI
- **Prüfungen** (Linux): fmt, clippy, Lizenzen/Advisories (`cargo deny`),
  WASM-Build des Renderers, Frontend-Typprüfung und -Build.
- **Tests** auf macOS arm64, Windows x64/ARM64, Linux x86_64/aarch64.
- **Geschwindigkeit** auf macOS arm64, Windows x64, Linux x86_64:
  `nabshot-bench` misst Speichern + Vorschaubild, Editor öffnen, Zeichnen,
  Export, Scroll-Zusammensetzen und Suche. Ein Wert über Budget lässt den Lauf
  fehlschlagen; die Tabelle steht in der Zusammenfassung des Laufs.

## Manuelle Tests
- macOS nativ, Windows und Linux in den UTM-VMs (ARM64).
- In VMs ohne echte Grafikkarte verhalten sich manche Aufnahme-Schnittstellen
  anders (z. B. Desktop Duplication) – Stichproben auf echter Hardware.
