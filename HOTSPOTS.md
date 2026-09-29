# Hotspots

The built-in planner is expected to be linear in document size plus generated edits.
Its main costs are lexical scanning (including C++ template/parenthesis matching,
continuation depth, brace/class-section tracking, multiline designated-initializer
state, aggregate-boundary detection, declaration-run classification, signature
reflow/qualifier parsing, C++ constructor-initializer collection, Rust lifetime
recognition, attached-gap classification, static output-literal recognition, Python keyword-call
recognition, multiline-header suite tracking, Python-suite tracking, multiline-literal
exclusion, and unmatched-delimiter rejection), logical-column
computation, UTF-16 position conversion, and copying the composed
formatted document. The profile formatter has no process, I/O, or network boundary,
so Format Document latency is determined by the in-memory planner and never blocks
Zed's GUI thread. There is no intended cross-document contention.

Release builds execute outside the editor on isolated GitHub runners. macOS helper
and Windows helper compilation are CI-time costs only and cannot affect formatter
latency or introduce runtime network activity.
