#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 2 || $1 != "--bin-dir" ]]; then
    echo "usage: scripts/install-helper.sh --bin-dir <directory-on-Zed-PATH>" >&2
    exit 2
fi

cd "$(dirname "$0")/.."
cargo build --release -p zed-vertical-align-lsp
install -Dm755 target/release/zed-vertical-align-lsp "$2/zed-vertical-align-lsp"
