#!/usr/bin/env bash
# build_linux.sh
# Build DM8 Export Tool Linux packages (AppImage + .deb)
# Runtime: Ubuntu 22.04+ or Debian 12+ (WSL2 or native Linux)
# Output: src-tauri/target/release/bundle/{appimage,deb}/

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# ── 1. Check required tools ──────────────────────────────────────────────
check_cmd() {
    if ! command -v "$1" &>/dev/null; then
        echo "Error: '$1' not found. Install: sudo apt-get install $2"
        exit 1
    fi
}

check_cmd cargo    "cargo (via rustup.rs)"
check_cmd node     "nodejs npm"
check_cmd npm      "nodejs npm"
check_cmd pkg-config "pkg-config"

if ! dpkg -s unixodbc-dev &>/dev/null 2>&1; then
    echo "Installing unixodbc-dev..."
    sudo apt-get install -y unixodbc-dev
fi

# AppImage tool (Tauri downloads it automatically if missing)
if ! command -v appimagetool &>/dev/null; then
    echo "Note: appimagetool not found — Tauri will try to download it automatically."
fi

# ── 2. Install frontend dependencies (includes @tauri-apps/cli) ─────────
echo ""
echo "Installing frontend dependencies..."
npm install --prefix frontend

# ── 3. Set LD_LIBRARY_PATH for bundled DM8 driver ───────────────────────
DRIVER_DIR="$SCRIPT_DIR/drivers/dm8"
if [ -f "$DRIVER_DIR/libdodbc.so" ]; then
    export LD_LIBRARY_PATH="$DRIVER_DIR:${LD_LIBRARY_PATH:-}"
    export DM8_DRIVER_PATH="$DRIVER_DIR/libdodbc.so"
    echo "DM8 driver: $DM8_DRIVER_PATH"
else
    echo "Warning: drivers/dm8/libdodbc.so not found. Connection tests will fail."
fi

# ── 4. Build frontend ────────────────────────────────────────────────────
echo ""
echo "Building frontend..."
npm run build --prefix frontend

# ── 5. Run Tauri build from project root (where src-tauri/ lives) ────────
echo ""
echo "Starting Tauri AppImage + deb build..."
node "frontend/node_modules/@tauri-apps/cli/tauri.js" build --bundles appimage,deb
BUILD_EXIT=$?

if [ $BUILD_EXIT -ne 0 ]; then
    echo "Error: Tauri build failed (exit $BUILD_EXIT)"
    exit $BUILD_EXIT
fi

# ── 6. Report output ────────────────────────────────────────────────────
echo ""
echo "Build successful! Packages:"
BUNDLE_BASE="$SCRIPT_DIR/src-tauri/target/release/bundle"
for dir in "$BUNDLE_BASE/appimage" "$BUNDLE_BASE/deb"; do
    [ -d "$dir" ] && ls "$dir"/*.{AppImage,deb} 2>/dev/null | sed 's/^/  /'
done
echo ""
echo "Install deb on Ubuntu/Debian:"
echo "  sudo dpkg -i <package>.deb && sudo apt-get install -f"
