# Build, test, benchmark, and install

## Prerequisites

- Rust via rustup.
- `wasm32-wasip2`: `rustup target add wasm32-wasip2`.
- Stock Zed for the local dev-extension install.

## Deterministic commands

Run from the repository root:

```bash
scripts/build.sh
scripts/run.sh
scripts/test.sh
scripts/benchmark.sh
```

`scripts/build.sh` builds the local LSP helper and validates the extension's WASM
build. `scripts/run.sh` runs the helper with `--stdio`.

## GitHub release builds

`.github/workflows/release.yml` runs the deterministic tests and Linux/WASM build
on every pull request and push to `main`. It then creates helper archives and
SHA-256 checksums for Linux x86_64, macOS Apple Silicon, and macOS Intel. Pushing a
tag beginning with `v` creates or updates the matching GitHub Release with those
archives and the extension WASM artifact.

```bash
git tag -a v0.1.2 -m 'v0.1.2'
git push origin v0.1.2
```

On macOS, download the archive matching `uname -m` (`arm64` for Apple Silicon,
`x86_64` for Intel), verify its adjacent checksum, extract it, and install the
`zed-vertical-align-lsp` executable on the PATH visible to Zed. The archive keeps
the executable bit.

## Local Zed installation

1. Run `scripts/build.sh`.
2. Run `scripts/install-helper.sh --bin-dir <directory-visible-in-Zed-PATH>`.
3. Register the dev extension either with Zed's `zed: install dev extension`
   command, or deterministically on Linux with:

   ```bash
   scripts/install-zed-dev-extension.sh ~/.local/share/zed/extensions/installed
   ```

4. Restart Zed (or run `zed: rebuild dev extension`) after changing this project.
5. Add the settings documented in README.md.
6. Use Zed's Format Document or Format Selection command.

The script refuses to replace a normal extension or a different dev extension.
The extension never downloads binaries and does not alter a worktree.
