#!/usr/bin/env bash
# Baut FMShot für Apple Silicon, beendet die laufende App, installiert nach
# /Applications und startet sie (ADR 0009). `--no-install` baut nur.
set -euo pipefail

cd "$(dirname "$0")/.."
ROOT=$(pwd)
APP="$ROOT/target/aarch64-apple-darwin/release/bundle/macos/FMShot.app"

# Gleiches Zertifikat wie die installierte App, sonst verlangt macOS die
# Bildschirmaufnahme-Freigabe neu (ADR 0008).
export APPLE_SIGNING_IDENTITY="${APPLE_SIGNING_IDENTITY:-FMShot Local Signing}"

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
if pgrep -xq fmshot-desktop; then
  osascript -e 'quit app "FMShot"' || true
  for _ in $(seq 1 20); do pgrep -xq fmshot-desktop || break; sleep 0.25; done
  pkill -x fmshot-desktop || true
fi

rm -rf /Applications/FMShot.app
ditto "$APP" /Applications/FMShot.app
open /Applications/FMShot.app
echo "Installiert und gestartet: /Applications/FMShot.app"
