#!/usr/bin/env bash
# Baut Nabshot für Apple Silicon, beendet die laufende App, installiert nach
# /Applications und startet sie (ADR 0009). `--no-install` baut nur.
set -euo pipefail

cd "$(dirname "$0")/.."
ROOT=$(pwd)
APP="$ROOT/target/aarch64-apple-darwin/release/bundle/macos/Nabshot.app"

# Gleiches Zertifikat wie die installierte App, sonst verlangt macOS die
# Bildschirmaufnahme-Freigabe neu (ADR 0008).
export APPLE_SIGNING_IDENTITY="${APPLE_SIGNING_IDENTITY:-Nabshot Local Signing}"

# Bildmodell für die Suche nach Bildinhalt (ADR 0010).
./scripts/fetch-model.sh

# Update-Pakete brauchen den Updater-Schlüssel und sind lokal unnötig.
(cd apps/desktop && npm run tauri build -- \
  --target aarch64-apple-darwin --bundles app \
  --config src-tauri/tauri.models.conf.json \
  --config '{"bundle":{"createUpdaterArtifacts":false}}')

codesign --verify --deep --strict "$APP"
echo "Gebaut: $APP"

[[ "${1:-}" == "--no-install" ]] && exit 0

# Laufende App regulär beenden (das Beenden über macOS umgeht ⌘Q).
if pgrep -xq nabshot-desktop; then
  osascript -e 'quit app "Nabshot"' || true
  for _ in $(seq 1 20); do pgrep -xq nabshot-desktop || break; sleep 0.25; done
  pkill -x nabshot-desktop || true
fi

# Früherer Name (ADR 0015): alte App beenden und entfernen, sonst laufen zwei.
if pgrep -xq grabmark-desktop; then
  osascript -e 'quit app "Grabmark"' || true
  for _ in $(seq 1 20); do pgrep -xq grabmark-desktop || break; sleep 0.25; done
  pkill -x grabmark-desktop || true
fi
rm -rf /Applications/Grabmark.app

rm -rf /Applications/Nabshot.app
ditto "$APP" /Applications/Nabshot.app
open /Applications/Nabshot.app
echo "Installiert und gestartet: /Applications/Nabshot.app"
