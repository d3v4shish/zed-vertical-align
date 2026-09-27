# Zed Vertical Align extension

- [x] Publish reproducible GitHub release artifacts.
  - Contract: GitHub Actions validates Linux/WASM, packages Linux x86_64 and both
    native macOS helper architectures with SHA-256 checksums, and publishes all
    assets when a `v*` tag is pushed.
  - Validation: workflow syntax inspection plus local workspace tests, build, and
    benchmark; the first pushed version tag is the cloud macOS compilation check.

- [ ] Reflow multiline Python keyword calls.
  - Contract: an existing multiline Python call with two or more top-level `name=value` arguments keeps its first argument after `(`, aligns later argument names and `=` separators below it, and retains a trailing comma.
  - Validation: direct and nested `ProcessStats` snapshots, malformed-call guards, idempotence, range formatting, workspace tests, build, and benchmark are complete. Visible `dsa3.py` validation remains after the helper restart.

- [ ] Recognize Rust lifetimes followed by a type.
  - Contract: a field such as `label: &'static str` remains code, so it and all following fields participate in their local colon-alignment group.
  - Validation: exact `MemoryRegion` lifetime-field snapshot, idempotence, workspace tests, build, and benchmark are complete. Visible `dsa3.rs` validation remains after the helper restart.

- [ ] Align static labels in standard output calls.
  - Contract: contiguous runs of two or more supported C, C++, Rust, Go, Python, JavaScript, or TypeScript output calls align `:`, `=`, or `|` inside safe static label literals; each delimiter kind remains an independent group.
  - Validation: deterministic standard-output and diagnostic-output snapshots for every supported language, C++ stream coverage, delimiter isolation, ambiguous-string guards, idempotence, range formatting, workspace tests, build, and benchmark are complete. Visible `dsa3.rs` validation remains after the helper restart.

- [ ] Fix stale attribute and operator-chain spacing.
  - Contract: Rust attributes remain attached to their item; stale blank lines inside verified operator-led expression chains are removed in C, C++, Rust, Go, JavaScript, and TypeScript; ordinary blank-separated statements remain independent.
  - Validation: deterministic `CpuFlags`, `checksum`, per-language operator-chain, intentional-gap, and idempotence snapshots; workspace tests, build, benchmark, and a visible `dsa3.rs` check.

- [ ] Format C++ constructor initializer lists.
  - Contract: constructor lists with two or more top-level `member(...)` initializers keep the first member after `:`, align later members beneath it, indent the list one level below the constructor header, and preserve the Allman body brace at the header indentation.
  - Validation: inline and already-multiline `WorkerRegistry` fixtures, nested-call and single-member guards, idempotence, workspace tests, build, benchmark, and a visible `dsa3.cpp` check.

- [x] Align C/C++ parameter qualifiers without separating references from names.
  - Contract: compatible trailing qualifiers such as `const` align vertically, while `&name` and `*name` remain contiguous declarators.
  - Validation: focused C++ reference-parameter snapshot, workspace tests, build, benchmark, and a visible `dsa3.cpp` check.

- [x] Place the first C/C++ function parameter on its declaration line.
  - Contract: signatures with two or more parameters keep the first parameter after `(`, align every later parameter below it, and keep the Allman opening brace on its own line.
  - Validation: focused C++ signature snapshot, workspace tests, build, benchmark, and a visible `dsa3.cpp` check.

- [x] Correct C++ aggregate sections, wrapped initializers, and short declaration groups.
  - Contract: each completed designated aggregate receives one surrounding separator; a split simple initializer is restored to one line; automatic type separators require at least three declarations in each adjacent run, and stale single separators around smaller mixed-type runs are removed.
  - Validation: focused core regression snapshot of the dsa3.cpp patterns, workspace tests, build, benchmark, and a visible Zed check.

- [x] Add language-specific vertical-layout profiles for C, C++, Rust, Go, and Python.
  - Contract: C/C++ preserve Allman records/functions, type-partitioned declaration groups, bit-field/designator/assignment alignment, aggregate layout, and C++ stream chains without LLVM reflow; Rust, Go, and Python apply equivalent syntax-safe local alignment while retaining their native structural conventions.
  - Validation: deterministic per-language snapshots, idempotence checks, no-native-C++ formatting test, full workspace tests, build, and benchmark scripts.
- [x] Configure C++ format-on-save to use the vertical-layout helper as the sole document formatter.
  - Contract: C++ formatting receives the editor's original blank-line groups and cannot be flattened by `clang-format` or an automatic formatter before alignment.
  - Validation: inspect the generated C++ formatter configuration and run the dsa3-inspired fixture through the installed helper.

- [x] Add hybrid native and built-in structural layout for every supported language.
  - Contract: Format Document uses an available native formatter, then deterministic indentation, declaration layout, structural gaps, and local alignment; missing or failing tools use the built-in formatter.
  - Validation: language fixtures, native-command selection tests, LSP edits, build, test, and benchmark scripts.
- [x] Lock C++ class layout and all-block spacing regressions.
  - Contract: templates and type declarations remain separated, access labels indent four spaces with members at eight, parameter lists with two or more entries expand vertically, and each finished block receives exactly one separator unless attached.
  - Validation: dsa3-inspired fixture, nested control-flow fixture, range-formatting fixture, and visible Zed test.
- [x] Normalize structural blank lines for all supported languages.
  - Contract: keep exactly one separator after each finished code block and import group; remove separators between ordinary statements and attached continuations.
  - Validation: deterministic brace-language, Python, import, continuation, initializer, and range-formatting fixtures.
- [x] Validate and document the structural-spacing pass.
  - Contract: one Format Document operation applies structural spacing, reflow, and alignment without requiring a second run.
  - Validation: workspace tests, deterministic benchmarks, shell checks, and updated architecture/performance records.
- [x] Register the dev extension and configure Zed format-on-save for the supported languages.
  - Contract: Format Document and saving a C++ buffer invoke the normal formatter followed by `zed-vertical-align`.
  - Validation: inspect Zed's extension registration and settings, then check Zed.log after a format request.
- [x] Extend C++ formatting for the dsa3.cpp fixture.
  - Contract: template member declarations, multi-line constructor parameters, initializer lists, and stream value columns are independently aligned after normal C++ formatting.
  - Validation: deterministic core fixtures, full workspace tests, and a formatting request against dsa3.cpp.
- [x] Implement the pure planner for document and range formatting.
  - Contract: each compatible block has its own alignment column.
  - Validation: deterministic assignment, declaration, comment, string, and tab fixtures.
- [x] Implement the formatting-only LSP helper and WebAssembly launcher.
  - Contract: expose document/range formatting for the supported code languages only.
  - Validation: LSP capability and edit conversion tests.
- [x] Document deterministic build, local installation, configuration, and benchmarks.
  - Contract: a clean checkout can build, test, benchmark, and install the dev helper.
  - Validation: all scripts run without mutable external input.
