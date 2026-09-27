#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
cargo test -p vertical-align-core benchmark --lib -- --ignored --nocapture
