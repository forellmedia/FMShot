#!/bin/bash
# CI: Importiert das Zertifikat „Nabshot Local Signing“ in einen temporären
# Schlüsselbund und vertraut ihm für Code-Signing. Danach signiert der
# Tauri-Bundler damit (APPLE_SIGNING_IDENTITY).
#
# Erwartet die Umgebungsvariablen CERT_P12_BASE64 und CERT_PASSWORD
# (GitHub-Secrets MACOS_CERT_P12_BASE64 / MACOS_CERT_PASSWORD).
# Fehlen sie, wird ad-hoc signiert – dann vergisst macOS die
# Bildschirmaufnahme-Freigabe bei jedem Update.
set -euo pipefail

NAME="Nabshot Local Signing"

if [[ -z "${CERT_P12_BASE64:-}" || -z "${CERT_PASSWORD:-}" ]]; then
  echo "::warning::Keine Mac-Signatur-Secrets – Build wird ad-hoc signiert."
  echo "APPLE_SIGNING_IDENTITY=-" >> "$GITHUB_ENV"
  exit 0
fi

# LibreSSL des Systems: versteht die PKCS#12-Datei aus make-signing-cert.sh.
OPENSSL=/usr/bin/openssl
WORK="$RUNNER_TEMP/nabshot-signing"
KEYCHAIN="$WORK/nabshot.keychain-db"
KC_PASS="$("$OPENSSL" rand -hex 16)"
mkdir -p "$WORK"

echo "$CERT_P12_BASE64" | base64 --decode > "$WORK/identity.p12"
"$OPENSSL" pkcs12 -in "$WORK/identity.p12" -passin "pass:$CERT_PASSWORD" \
  -clcerts -nokeys -out "$WORK/cert.pem"

security create-keychain -p "$KC_PASS" "$KEYCHAIN"
security set-keychain-settings -lut 21600 "$KEYCHAIN"
security unlock-keychain -p "$KC_PASS" "$KEYCHAIN"
security import "$WORK/identity.p12" -k "$KEYCHAIN" -f pkcs12 \
  -P "$CERT_PASSWORD" -T /usr/bin/codesign
security set-key-partition-list -S apple-tool:,apple: -s -k "$KC_PASS" "$KEYCHAIN" >/dev/null
# Temporären Schlüsselbund vorne in die Suchliste.
# shellcheck disable=SC2046
security list-keychains -d user -s "$KEYCHAIN" $(security list-keychains -d user | tr -d '"')

# Selbstsigniert → explizit vertrauen, sonst gilt die Identität als ungültig.
sudo security add-trusted-cert -d -r trustRoot -p codeSign \
  -k /Library/Keychains/System.keychain "$WORK/cert.pem"

security find-identity -v -p codesigning "$KEYCHAIN" | grep "$NAME"
echo "APPLE_SIGNING_IDENTITY=$NAME" >> "$GITHUB_ENV"
rm -f "$WORK/identity.p12"
