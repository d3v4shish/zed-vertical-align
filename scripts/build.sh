#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
cargo build --release -p zed-vertical-align-lsp
cargo build --release --target wasm32-wasip2 -p zed-vertical-align-extension

# This is the location Zed loads for a local dev extension. Keep the generated
# artifact out of version control while making `scripts/build.sh` sufficient for
# the deterministic symlink-based local installation below.
ln -sfn target/wasm32-wasip2/release/zed_vertical_align_extension.wasm extension.wasm
