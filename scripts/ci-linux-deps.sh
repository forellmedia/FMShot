#!/bin/bash
# Systempakete, die Tauri unter Linux zum Bauen braucht (CI).
set -euo pipefail
sudo apt-get update
sudo apt-get install -y --no-install-recommends \
  libwebkit2gtk-4.1-dev \
  libgtk-3-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev \
  libxdo-dev \
  libssl-dev \
  patchelf \
  file
