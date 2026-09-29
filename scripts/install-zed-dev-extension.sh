#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -ne 1 ]; then
  echo "usage: $0 <zed-extensions-installed-directory>" >&2
  exit 2
fi

project_dir="$(cd "$(dirname "$0")/.." && pwd)"
installed_dir="$(cd "$1" && pwd)"
target="$installed_dir/vertical-align-lsp"
legacy_target="$installed_dir/zed-vertical-align-lsp"

if [ -e "$target" ] && [ ! -L "$target" ]; then
  echo "refusing to replace non-dev extension: $target" >&2
  exit 1
fi
if [ -L "$target" ] && [ "$(readlink -f "$target")" != "$project_dir" ]; then
  echo "refusing to replace another dev extension: $target" >&2
  exit 1
fi

if [ -L "$legacy_target" ] && [ "$(readlink -f "$legacy_target")" = "$project_dir" ]; then
  rm "$legacy_target"
fi

ln -sfn "$project_dir" "$target"
echo "registered Zed dev extension: $target -> $project_dir"
