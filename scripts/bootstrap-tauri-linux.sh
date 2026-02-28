#!/usr/bin/env bash
set -euo pipefail

# Bootstrap Tauri 2.x build dependencies on Debian/Ubuntu.
# Run with: sudo bash scripts/bootstrap-tauri-linux.sh
#
# For Ubuntu 25.10+, first run: sudo bash scripts/fix-apt-sources.sh

echo "Installing Tauri 2.x Linux build dependencies..."

apt-get update -qq || echo "Warning: apt-get update had errors (likely a broken PPA), continuing anyway..."

apt-get install -y --no-install-recommends \
    build-essential \
    pkg-config \
    libwebkit2gtk-4.1-dev \
    libsoup-3.0-dev \
    libgtk-3-dev \
    librsvg2-dev \
    patchelf

echo ""
echo "Done. You can now build with:"
echo "  cargo install tauri-cli --version '^2'"
echo "  cd src-tauri && cargo tauri build"
