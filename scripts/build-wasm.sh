#!/bin/bash
# Baut den Renderer als WebAssembly für den Editor.
# Ausgabe: apps/desktop/src/lib/wasm/ (nicht eingecheckt)
#
# Voraussetzungen:
#   rustup target add wasm32-unknown-unknown
#   cargo install wasm-bindgen-cli --version <wie in crates/nabshot-render-wasm/Cargo.toml>
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/apps/desktop/src/lib/wasm"
PROFILE="${WASM_PROFILE:-release}"

cd "$ROOT"
cargo build -p nabshot-render-wasm --target wasm32-unknown-unknown --profile "$PROFILE"
wasm-bindgen \
  --target web \
  --out-dir "$OUT" \
  --out-name nabshot_render \
  "target/wasm32-unknown-unknown/$PROFILE/nabshot_render_wasm.wasm"
echo "WASM-Renderer → $OUT"
