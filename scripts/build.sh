#!/usr/bin/env bash
set -euo pipefail

# LightTable build script
# Used both locally and in CI
# Content root and output dir default from config.toml / CLI defaults

echo "==> Building lighttable CLI"
cargo build --release

echo "==> Running full build pipeline"
./target/release/lighttable build "$@"

echo "==> Done"
