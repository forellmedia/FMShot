#!/bin/bash
# Legt einmalig ein selbstsigniertes Code-Signing-Zertifikat
# »FMShot Local Signing« an – lokal im Anmelde-Schlüsselbund und als Datei für
# die CI (ADR 0008, gleiches Verfahren wie bei Faster2Bed).
#
# Wozu: Ad-hoc signiert ändert sich die Signatur bei jedem Build, und macOS
# vergisst die Freigabe „Bildschirmaufnahme“. Mit einem dauerhaften Zertifikat
# bleibt sie über Updates erhalten – lokal und bei Freunden.
#
# Verändert den Schlüsselbund – bewusst von Hand starten:
#   ./scripts/make-signing-cert.sh
#
# Was passiert:
#   1. openssl erzeugt Schlüssel und Zertifikat (RSA 2048, 10 Jahre gültig,
#      Extended Key Usage „Code Signing“).
#   2. `security import` legt beides als Identität in den Anmelde-Schlüsselbund.
#   3. `security add-trusted-cert` vertraut dem Zertifikat für Code-Signing
#      (macOS fragt nach dem Anmeldepasswort).
#   4. Die PKCS#12-Datei und ihr Passwort bleiben in ~/.fmshot-signing/
#      (nur für dich lesbar) – daraus werden die GitHub-Secrets gesetzt.
#
# Rückgängig: In der Schlüsselbundverwaltung unter „Anmeldung → Meine
# Zertifikate“ den Eintrag »FMShot Local Signing« löschen und
# ~/.fmshot-signing/ entfernen.
set -euo pipefail

NAME="FMShot Local Signing"
KEYCHAIN="$HOME/Library/Keychains/login.keychain-db"
OUT="$HOME/.fmshot-signing"
# Bewusst das openssl des Systems (LibreSSL): Dessen PKCS#12 versteht
# `security import`. Ein OpenSSL 3 aus Homebrew bräuchte dafür `-legacy`.
OPENSSL=/usr/bin/openssl

if security find-certificate -c "$NAME" "$KEYCHAIN" >/dev/null 2>&1; then
  echo "Zertifikat »${NAME}« ist schon im Schlüsselbund – nichts zu tun."
  echo "Ein zweites mit gleichem Namen würde die Signatur mehrdeutig machen."
  exit 0
fi

mkdir -p "$OUT"
chmod 700 "$OUT"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

cat > "$TMP/cert.cnf" <<CNF
[req]
distinguished_name = dn
x509_extensions    = ext
prompt             = no

[dn]
CN = $NAME
O  = forellmedia.ch

[ext]
basicConstraints     = critical, CA:false
keyUsage             = critical, digitalSignature
extendedKeyUsage     = critical, codeSigning
subjectKeyIdentifier = hash
CNF

"$OPENSSL" req -x509 -newkey rsa:2048 -nodes -days 3650 \
  -config "$TMP/cert.cnf" -keyout "$TMP/key.pem" -out "$TMP/cert.pem" 2>/dev/null

PASS="$("$OPENSSL" rand -hex 16)"
"$OPENSSL" pkcs12 -export -name "$NAME" \
  -inkey "$TMP/key.pem" -in "$TMP/cert.pem" \
  -out "$OUT/identity.p12" -passout "pass:$PASS"
printf '%s' "$PASS" > "$OUT/password.txt"
chmod 600 "$OUT/identity.p12" "$OUT/password.txt"

security import "$OUT/identity.p12" -k "$KEYCHAIN" -f pkcs12 \
  -P "$PASS" -T /usr/bin/codesign

echo "Vertrauen für Code-Signing setzen – macOS fragt nach dem Anmeldepasswort …"
security add-trusted-cert -r trustRoot -p codeSign -k "$KEYCHAIN" "$TMP/cert.pem"

echo
security find-identity -v -p codesigning | grep "$NAME" || {
  echo "Identität »${NAME}« taucht nicht als gültig auf – siehe Fehlermeldungen oben." >&2
  exit 1
}
cat <<MSG

Fertig. Jetzt die GitHub-Secrets für die CI setzen:

  base64 -i "$OUT/identity.p12" | gh secret set MACOS_CERT_P12_BASE64 -R forellmedia/FMShot
  gh secret set MACOS_CERT_PASSWORD -R forellmedia/FMShot < "$OUT/password.txt"

Lokal signiert 'npm run tauri build' damit, wenn APPLE_SIGNING_IDENTITY gesetzt ist:

  APPLE_SIGNING_IDENTITY="$NAME" npm run tauri build

~/.fmshot-signing/ gut aufbewahren (z. B. Passwort-Manager) – ohne diese Datei
bekommen künftige Builds eine neue Identität und alle müssen die
Bildschirmaufnahme-Freigabe neu erteilen.
MSG
