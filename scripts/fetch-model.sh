#!/usr/bin/env bash
# Stellt das Bildmodell SigLIP 2 (ADR 0010) unter models/siglip2/ bereit:
# lädt die fp32-Originale (geprüft per SHA-256) nach models/.cache/ und wandelt
# sie mit scripts/model/convert.py in die kleinere, mitgelieferte Fassung.
# Ist alles schon da und richtig, passiert nichts. Braucht `uv`.
set -euo pipefail

cd "$(dirname "$0")/.."
OUT=models/siglip2
CACHE=models/.cache
REPO=onnx-community/siglip2-base-patch16-224-ONNX
REV=ba1f3b0843f24bc5417d38e19c37b287d719b2f4

# Originale: Pfad im Repo, SHA-256
SOURCES=(
  "onnx/vision_model.onnx c0573e3f4140c3a7c4e9cc5912bd6b26a033b46a6a8e8af26cbea262b163bcad"
  "onnx/text_model.onnx baf12d941beabafafb14f7b4adb38dc15be18681b964a84410ec53d9d65e6293"
  "tokenizer.json cb9140fae3ac5122c972d37adf83e1248471a38147ad76f8215c8872c6fd8322"
)
# Ergebnis der Umwandlung (deterministisch), SHA-256
RESULTS=(
  "vision.onnx 95f1226c95ffe5a244b07b41105792e619b859fd9a1fc13f439ad7488e295e25"
  "text.onnx 75a9cc417690972fb431562509887cc24a78b5eb9da3da6a5495a62d7fecf906"
  "tokenizer.json cb9140fae3ac5122c972d37adf83e1248471a38147ad76f8215c8872c6fd8322"
)

sha() {
  if command -v sha256sum >/dev/null; then sha256sum "$1"; else shasum -a 256 "$1"; fi | cut -d' ' -f1
}

ready() {
  for entry in "${RESULTS[@]}"; do
    read -r name sum <<<"$entry"
    [[ -f "$OUT/$name" && "$(sha "$OUT/$name")" == "$sum" ]] || return 1
  done
}

if ready; then
  echo "Modell bereit: $OUT"
  exit 0
fi

mkdir -p "$OUT" "$CACHE"
for entry in "${SOURCES[@]}"; do
  read -r src sum <<<"$entry"
  file="$CACHE/$(basename "$src")"
  [[ -f "$file" && "$(sha "$file")" == "$sum" ]] && continue
  echo "Lade $src …"
  curl -fsSL --retry 3 -o "$file.part" "https://huggingface.co/$REPO/resolve/$REV/$src"
  if [[ "$(sha "$file.part")" != "$sum" ]]; then
    echo "Prüfsumme falsch: $src" >&2
    rm -f "$file.part"
    exit 1
  fi
  mv "$file.part" "$file"
done

echo "Wandle Modell um …"
uv run --quiet --no-project --python 3.14 \
  --with onnxruntime==1.31.0 --with onnx==1.23.2 --with onnx-ir==1.0.0 \
  --with numpy==2.5.4 --with ml-dtypes==0.6.0 --with protobuf==7.36.2 \
  python -I scripts/model/convert.py "$CACHE" "$OUT"
cp "$CACHE/tokenizer.json" "$OUT/tokenizer.json"
rm -f "$CACHE/text_gather8.onnx"

if ! ready; then
  echo "Umgewandeltes Modell hat andere Prüfsummen als erwartet:" >&2
  for entry in "${RESULTS[@]}"; do
    read -r name _ <<<"$entry"
    echo "  $name $(sha "$OUT/$name")" >&2
  done
  exit 1
fi
echo "Modell bereit: $OUT"
