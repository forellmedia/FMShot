#!/bin/bash
# Erzeugt einmalig den Schlüssel, mit dem Updates signiert werden (Tauri-Updater).
# Die App prüft jedes Update gegen den öffentlichen Schlüssel in
# apps/desktop/src-tauri/tauri.conf.json (plugins.updater.pubkey).
#
#   ./scripts/make-updater-key.sh
#
# Ergebnis in ~/.fmshot-signing/ (nur für dich lesbar):
#   updater.key           privater Schlüssel  → GitHub-Secret TAURI_SIGNING_PRIVATE_KEY
#   updater.key.pub       öffentlicher Schlüssel → tauri.conf.json
#   updater-password.txt  Passwort            → GitHub-Secret TAURI_SIGNING_PRIVATE_KEY_PASSWORD
#
# WICHTIG: Geht updater.key verloren, können installierte Versionen keine
# Updates mehr annehmen – gut aufbewahren (Passwort-Manager).
set -euo pipefail

OUT="$HOME/.fmshot-signing"
KEY="$OUT/updater.key"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

if [[ -f "$KEY" ]]; then
  echo "Updater-Schlüssel existiert schon: $KEY – nichts zu tun."
  exit 0
fi

mkdir -p "$OUT"
chmod 700 "$OUT"
PASS="$(/usr/bin/openssl rand -hex 24)"
printf '%s' "$PASS" > "$OUT/updater-password.txt"
chmod 600 "$OUT/updater-password.txt"

(cd "$ROOT/apps/desktop" && npx tauri signer generate --ci -p "$PASS" -w "$KEY" >/dev/null)
chmod 600 "$KEY"

cat <<MSG
Updater-Schlüssel erzeugt in $OUT.

Öffentlicher Schlüssel (gehört nach tauri.conf.json → plugins.updater.pubkey):
$(cat "$KEY.pub")

GitHub-Secrets setzen:

  gh secret set TAURI_SIGNING_PRIVATE_KEY -R forellmedia/FMShot < "$KEY"
  gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD -R forellmedia/FMShot < "$OUT/updater-password.txt"
MSG
