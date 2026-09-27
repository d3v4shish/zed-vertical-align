# Zed Vertical Align

A Zed extension that formats and vertically aligns independent code blocks
through Zed's standard formatting commands. GitHub Actions builds native helpers
for Linux x86_64, macOS Apple Silicon, and macOS Intel.

## Why this exists

Stock Zed extensions cannot add editor actions or directly edit the active buffer.
This project provides a small Language Server Protocol formatter instead, so
Format Document, Format Selection, and format-on-save can apply alignment without
patching Zed.

## Demo

```cpp
int a;
float b ;
```

becomes:

```cpp
int   a;
float b;
```

The built-in structural formatter fixes indentation across the supported languages.
Major blank-line groups are preserved; a missing separator after a completed code
block or import group is added once. C and C++ also remove stale single separators
between short mixed-type declaration groups. Attached continuations such as `else`
and `catch`, Rust attributes and their item, and verified operator-led expression
chains remain contiguous. Comments, indentation
changes, and separator kinds create independent alignment blocks. They never share
one file-wide alignment column.

## What is interesting technically

The WebAssembly extension only launches a local helper. The helper speaks LSP over
stdio and returns one deterministic structural-layout and alignment edit. It does
not invoke a native formatter before alignment or write to the worktree. Release
tags publish one helper archive per supported operating system and CPU architecture.

## Architecture

See [ARCHITECTURE.md](ARCHITECTURE.md).

## How it works

Install the helper on the PATH visible to Zed, install this directory as a dev
extension, then configure Zed to use `zed-vertical-align` as its formatter. See
[BUILD.md](BUILD.md).

### Zed settings

Install this directory with `zed: install dev extension`, then configure each
supported language to use the vertical-layout helper as its only formatter:

```json
{
  "languages": {
    "Python": {
      "language_servers": ["...", "zed-vertical-align"],
      "formatter": [{ "language_server": { "name": "zed-vertical-align" } }],
      "format_on_save": "on"
    },
    "C": {
      "language_servers": ["...", "zed-vertical-align"],
      "formatter": [{ "language_server": { "name": "zed-vertical-align" } }],
      "format_on_save": "on"
    },
    "C++": {
      "language_servers": ["...", "zed-vertical-align"],
      "formatter": [{ "language_server": { "name": "zed-vertical-align" } }],
      "format_on_save": "on"
    },
    "Rust": {
      "language_servers": ["...", "zed-vertical-align"],
      "formatter": [{ "language_server": { "name": "zed-vertical-align" } }],
      "format_on_save": "on"
    },
    "Go": {
      "language_servers": ["...", "zed-vertical-align"],
      "formatter": [{ "language_server": { "name": "zed-vertical-align" } }],
      "format_on_save": "on"
    },
    "JavaScript": {
      "language_servers": ["...", "zed-vertical-align"],
      "formatter": [{ "language_server": { "name": "zed-vertical-align" } }],
      "format_on_save": "on"
    },
    "TypeScript": {
      "language_servers": ["...", "zed-vertical-align"],
      "formatter": [{ "language_server": { "name": "zed-vertical-align" } }],
      "format_on_save": "on"
    }
  }
}
```

Use Zed's **Format Document** command to format every independent block in the
file, or **Format Selection** to change only blocks intersecting the selection.

## Performance / Benchmarks

See [BENCHMARKS.md](BENCHMARKS.md) and [HOTSPOTS.md](HOTSPOTS.md).

## Build and run

```bash
scripts/build.sh
scripts/test.sh
scripts/benchmark.sh
```

`scripts/run.sh` starts the LSP helper in stdio mode for protocol debugging.

## Experiments

The helper deliberately supports only Python, C, C++, Rust, Go, JavaScript, and
TypeScript. Markdown is excluded because prose with colons is not reliably code.

## Design decisions

The helper is the sole configured formatter, so it receives the original blank-line
groups. C and C++ use Allman record/function braces, type-aware declaration and
assignment sections, bit-field/designator alignment, expanded multiline designated
initializers with a gap around each completed aggregate, C++ stream chains, and
constructor initializer lists. A C++ constructor with two or more `member(...)`
initializers keeps its first member after the colon, aligns later members under it,
and leaves its Allman body brace at the constructor indentation.
Automatic type separators require at least three declarations on each side; shorter
mixed-type runs remain one contiguous logical group. Rust, Go, and Python retain their native
structural conventions while aligning compatible fields, assignments, and Python
annotations locally. Rust attributes stay attached to their item, lifetime annotations
remain code rather than character literals even when a type follows, and operator-led expression chains stay
contiguous even when an existing stale blank line separates their rows. Functions,
methods, constructors, and lambdas with two or more
parameters expand vertically and align locally. C and C++ keep the first parameter
on the declaration line and align the remaining parameters beneath it. C++ templates
stay separate from type declarations; trailing `const`/`volatile` qualifiers align
locally while `&name` and `*name` stay contiguous. Access labels indent four spaces
and their members eight.
Existing multiline Python calls with two or more simple keyword arguments keep the
first argument after `(`, align later argument names beneath it, and use one aligned
` = ` column. Compact, positional, spread, commented, and malformed calls are left alone.
Contiguous standard output and diagnostic calls also align simple static labels on
`:`, `=`, or `|` inside their string literals. This intentionally changes rendered
output spacing; dynamic, raw, multiline, URL-like, and custom-wrapper strings are left alone.

## Limitations

Local development requires Rust, the `wasm32-wasip2` target, and a helper
executable on the PATH seen by Zed. GitHub Releases provide helpers for Linux
x86_64 and macOS (Apple Silicon and Intel); Windows is not released yet. The
extension preserves tokens and therefore does not repair syntax errors such as
`RE#include`. It cannot provide custom alignment keybindings.

## Related projects

The companion patched-Zed prototype lives in `~/Workspace/Temp/ZedVerticalAlign`.

## Article

Not planned.

## Status / Roadmap

See [TODO.md](TODO.md). Is there anything you want added to this README outline?
