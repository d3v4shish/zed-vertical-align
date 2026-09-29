# Architecture

The root crate is a Zed WebAssembly extension. It registers one formatter-only
language server for Python, C, C++, Rust, Go, JavaScript, and TypeScript and starts
`zed-vertical-align-lsp` from Zed's PATH.

`vertical-align-core` is a pure synchronous planner. It first rejects documents
with unmatched parentheses, brackets, or braces. It then applies a lexical,
language-aware structural layout pass, then partitions compatible rows into
independent blocks and creates whitespace/reflow edits. The C-like pass tracks
braces, multiline delimiters, C++ templates, and class access sections; Python
tracks suites and attached continuations. C++ access labels are indented one level
inside their class and member declarations another level inside the active section.

Rows containing multiline Python strings, Go raw strings, Rust raw strings,
JavaScript template strings, C/C++ raw strings, and block comments are preserved
verbatim during structural layout and spacing. After layout and before alignment,
the planner performs a language-aware
structural-spacing pass.
It tracks brace blocks for C, C++, Rust, Go, JavaScript, and TypeScript; it tracks
indentation suites for Python. The pass adds one blank separator only when a finished
executable block or import group has none; it preserves existing blank-line runs,
attached continuations, and expression/initializer braces. C and C++ use an Allman
profile that expands compact and partially multiline designated initializers,
separates completed aggregates, restores split simple initializers, separates
declaration/mutation categories, inserts type separators only between adjacent
declaration runs of three or more lines, removes stale single separators around
short mixed-type runs, aligns bit fields, and splits C++ stream heads. C and C++
signature reflow keeps the first parameter on the header and aligns each later
parameter beneath it. For C++ constructors with two or more unambiguous
`member(...)` initializers, it emits a colon-led initializer list one indentation
level below the header, with subsequent members aligned beneath the first, while
leaving the Allman body brace at header indentation. Its qualifier pass aligns
trailing `const` and `volatile` tokens without detaching `&name` or `*name`. Rust,
Go, and Python retain their native structural syntax while receiving field, assignment, and
annotation alignment. Rust attributes stay attached to their item, lifetimes are recognized as
code rather than character literals even when a type follows (such as `&'static str`), and operator-led expression chains do not form structural
boundaries; stale blank lines in those attached forms are removed across the supported
brace languages. Its output is then reflowed and aligned in memory; the LSP
receives one composed edit so the passes cannot overlap.
Python tracks parenthesized continuations during structural layout, then separately
reflows existing multiline calls whose top-level entries are simple keyword arguments.
The call pass keeps its first argument after `(`, aligns later names and their `=`
separators locally, preserves trailing commas, and rejects positional, spread,
commented, raw, multiline, and malformed argument forms.
The structural scanner also defers Python suite creation for a multiline `def`,
`class`, or control-flow header until its closing delimiter line ends in `:`. It
tracks whether a suite body was inferred from malformed indentation, preventing
that recovery behavior from swallowing a normally indented nested suite.
After ordinary code alignment, a separate literal-content pass recognizes only standard
output and diagnostic APIs. It aligns safe static labels on `:`, `=`, or `|` in contiguous
local groups for C, C++, Rust, Go, Python, JavaScript, and TypeScript. C++ stream-head
continuations retain their logical output grouping. The pass deliberately edits spaces inside
the output literal, and rejects dynamic, raw, multiline, escaped-delimiter, URL-like, and
custom-wrapper forms.
`zed-vertical-align-lsp` stores open documents in memory. Format Document and Format
Selection both call the pure core directly, so original spacing is never flattened by
an external formatter. Both paths return one composed LSP edit.

Formatting requests are asynchronous at the LSP boundary. The core has no shared
state, storage, network, process, or file-system boundary. The LSP passes Zed's tab
size and `insert_spaces` setting into the core. The LSP document map is
keyed by URI and protected by an async mutex.

The release boundary is GitHub Actions. It runs deterministic Linux tests and a
WASM build, then compiles and smoke-tests the same native LSP source on Linux
x86_64, Windows x86_64, macOS arm64, and macOS x86_64 runners. Each helper is
packaged with a SHA-256 checksum; version tags publish those archives and an
installable WebAssembly extension bundle as GitHub Release assets.
The extension and helper make no network calls at runtime.
