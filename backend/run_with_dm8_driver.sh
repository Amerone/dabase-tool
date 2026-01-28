#!/usr/bin/env bash
set -euo pipefail

# Run the backend with the bundled DM8 ODBC driver dependencies on the library path.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
DRIVER_DIR="${ROOT_DIR}/drivers/dm8"

if [ ! -d "${DRIVER_DIR}" ]; then
  echo "Driver directory not found: ${DRIVER_DIR}" >&2
  exit 1
fi

export LD_LIBRARY_PATH="${DRIVER_DIR}:${LD_LIBRARY_PATH:-}"
export DM8_DRIVER_PATH="${DRIVER_DIR}/libdodbc.so"

cd "${ROOT_DIR}/backend"
cargo run "$@"
