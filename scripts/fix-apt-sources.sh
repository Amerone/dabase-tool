#!/usr/bin/env bash
set -euo pipefail

# Fix broken apt sources on Ubuntu 25.10 (questing).
# Removes stale Focal (20.04) entries and disables the broken mc3man PPA.
# Run with: sudo bash scripts/fix-apt-sources.sh

echo "Fixing apt sources for Ubuntu 25.10..."

# 1) Disable the broken mc3man PPA
PPA_FILE="/etc/apt/sources.list.d/mc3man-ubuntu-mpv-tests-questing.sources"
if [ -f "$PPA_FILE" ]; then
    echo "Disabling broken PPA: $PPA_FILE"
    mv "$PPA_FILE" "${PPA_FILE}.disabled"
fi

# 2) Remove stale Focal entries from ubuntu.sources
SOURCES_FILE="/etc/apt/sources.list.d/ubuntu.sources"
if [ -f "$SOURCES_FILE" ] && grep -q "focal" "$SOURCES_FILE"; then
    echo "Removing Focal (20.04) entries from $SOURCES_FILE"
    cp "$SOURCES_FILE" "${SOURCES_FILE}.bak"
    # Keep only non-Focal stanzas (DEB822 format: stanzas separated by blank lines)
    python3 -c "
import re, sys
with open('$SOURCES_FILE') as f:
    content = f.read()
# Split into stanzas (separated by blank lines)
stanzas = re.split(r'\n\n+', content.strip())
kept = [s for s in stanzas if 'focal' not in s.lower()]
with open('$SOURCES_FILE', 'w') as f:
    f.write('\n\n'.join(kept) + '\n')
print(f'Kept {len(kept)}/{len(stanzas)} stanzas')
"
fi

# 3) Clean and update
echo "Running apt-get update..."
apt-get update -qq

echo ""
echo "Done. apt sources are clean."
echo "You can now run: sudo bash scripts/bootstrap-tauri-linux.sh"
