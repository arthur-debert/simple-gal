#!/usr/bin/env bash
set -euo pipefail

# Simple Gal build script
# Used both locally and in CI
# Content root and output dir default from config.toml / CLI defaults

echo "==> Building simple-gal CLI"
cargo build --release

echo "==> Running full build pipeline"
./target/release/simple-gal build "$@"

echo "==> Done"
