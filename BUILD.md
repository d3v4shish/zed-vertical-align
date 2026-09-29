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

`.github/workflows/release.yml` runs formatting, strict Clippy, workspace tests,
and the Linux/WASM build on every pull request and push to `main`. It then builds
and smoke-tests the native LSP helper on Linux x86_64, Windows x86_64, macOS Apple
Silicon, and macOS Intel. Pushing a tag beginning with `v` creates or updates the
matching GitHub Release with checksummed helper archives and an installable
extension bundle.

```bash
git tag -a v0.1.3 -m 'v0.1.3'
git push origin v0.1.3
```

Download the helper archive matching the host, verify its adjacent checksum, and
install the contained executable on the PATH visible to Zed. The macOS archive
matches `uname -m` (`arm64` for Apple Silicon, `x86_64` for Intel); Windows uses a
ZIP archive and `Get-FileHash -Algorithm SHA256`; Linux and macOS use `shasum -a
256 -c <checksum-file>`. The archive keeps the executable bit on Linux and macOS.

The release also contains `zed-vertical-align-extension.tar.gz`. Extract it and
select its `vertical-align-lsp` directory in `zed: install dev extension`. This
works before the extension is listed in Zed's public Extension Gallery.

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

On Windows, build and install the helper with PowerShell:

```powershell
.\scripts\install-helper.ps1 -BinDir "$env:LOCALAPPDATA\Programs\ZedVerticalAlign"
```

Add that directory to the user PATH, restart Zed, then install the extension bundle
and add the settings from README.md.
