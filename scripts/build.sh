#!/usr/bin/env bash
set -euo pipefail

# LightTable build script
# Used both locally and in CI

ROOT_DIR="${1:-images}"
OUTPUT_DIR="${2:-dist}"

echo "==> Building lighttable CLI"
cargo build --release

echo "==> Running full build pipeline"
./target/release/lighttable build "$ROOT_DIR" --output "$OUTPUT_DIR"

echo "==> Done"
