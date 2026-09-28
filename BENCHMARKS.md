# Benchmarks

Run `scripts/benchmark.sh`. It uses fixed 10,000-row assignment and C++ declaration
inputs, asserts the edit counts, and reports elapsed time without timing assertions.

## Baseline: 2026-09-24

- Command: `scripts/benchmark.sh`
- Assignment input: fixed 10,000-row document; 1,000 whitespace edits planned in
  27.46 ms.
- C++ input: fixed 10,000-row alternating simple-declaration document; 5,000
  whitespace edits planned in 42.93 ms.
- Environment: x86_64, AMD Ryzen 7 7800X3D (8 cores / 16 threads), 60 GiB RAM.
- I/O, network, and contention: none during the measured planner invocation.

These test-profile measurements validate deterministic behavior and establish a
baseline. They are not a performance-improvement claim.

## Latest validation: 2026-09-25

- Command: `scripts/benchmark.sh`
- Assignment input: fixed 10,000-row document; 1,000 whitespace edits planned in
  26.66 ms.
- C++ input: fixed 10,000-row alternating simple-declaration document; 5,000
  whitespace edits planned in 42.96 ms.
- Environment: x86_64, AMD Ryzen 7 7800X3D (8 cores / 16 threads), 60 GiB RAM.
- I/O, network, and contention: none during the measured planner invocation.

These are validation measurements after the C++ recognition change, not a claim
of an improvement over the baseline.

## Structural-spacing validation: 2026-09-25

- Command: `scripts/benchmark.sh`
- Assignment input: fixed 10,000-row document; one composed document edit planned
  in 47.17 ms.
- C++ input: fixed 10,000-row alternating simple-declaration document; one composed
  document edit planned in 70.00 ms.
- Environment: x86_64, AMD Ryzen 7 7800X3D (8 cores / 16 threads), 60 GiB RAM.
- I/O, network, and contention: none during the measured planner invocation.

The structural pass scans every line and the composed-output model copies the changed
document once. These measurements document that trade-off; they are not a performance
improvement claim.

## Structural-layout validation: 2026-09-25

- Command: `scripts/benchmark.sh`
- Assignment input: fixed 10,000-row document; one composed document edit planned
  in 99.41 ms.
- C++ input: fixed 10,000-row alternating simple-declaration document; one composed
  document edit planned in 123.66 ms.
- Environment: x86_64, AMD Ryzen 7 7800X3D (8 cores / 16 threads), 60 GiB RAM.
- I/O, network, and contention: none during the measured built-in planner
  invocation. Native formatter process time is deliberately excluded because its
  optional availability and project configuration are external to this fixed fixture.

The additional lexical layout and continuation scans increase built-in planning work.
These measurements record that cost and do not claim an improvement over prior runs.

## Profile-layout validation: 2026-09-25

- Command: `scripts/benchmark.sh`
- Assignment input: fixed 10,000-row document; one composed document edit planned
  in 100.97 ms.
- C++ input: fixed 10,000-row alternating simple-declaration document; one composed
  document edit planned in 167.42 ms.
- Environment: x86_64, AMD Ryzen 7 7800X3D (8 cores / 16 threads), 60 GiB RAM.
- I/O, network, and contention: none during the measured in-memory planner invocation.

The C++ profile now performs declaration-category recognition in addition to lexical
layout. These validation measurements record its cost and make no performance-improvement claim.

## Designated-initializer validation: 2026-09-25

- Command: `scripts/benchmark.sh`
- Assignment input: fixed 10,000-row document; one composed document edit planned
  in 99.60 ms.
- C++ input: fixed 10,000-row alternating simple-declaration document; one composed
  document edit planned in 179.36 ms.
- Environment: x86_64, AMD Ryzen 7 7800X3D (8 cores / 16 threads), 60 GiB RAM.
- I/O, network, and contention: none during the measured in-memory planner invocation.

The multiline-designator and declaration-run checks add linear scans. These are
validation measurements, not a performance-improvement claim.

## Aggregate-section validation: 2026-09-25

- Command: `scripts/benchmark.sh`
- Assignment input: fixed 10,000-row document; one composed document edit planned
  in 121.37 ms.
- C++ input: fixed 10,000-row alternating simple-declaration document; one composed
  document edit planned in 247.20 ms.
- Environment: x86_64, AMD Ryzen 7 7800X3D (8 cores / 16 threads), 60 GiB RAM.
- I/O, network, and contention: none during the measured in-memory planner invocation.

Aggregate-boundary and wrapped-initializer recognition add linear scans. These are
validation measurements, not a performance-improvement claim.

## Signature-layout validation: 2026-09-25

- Command: `scripts/benchmark.sh`
- Assignment input: fixed 10,000-row document; one composed document edit planned
  in 260.48 ms.
- C++ input: fixed 10,000-row alternating simple-declaration document; one composed
  document edit planned in 497.03 ms.
- Environment: x86_64, AMD Ryzen 7 7800X3D (8 cores / 16 threads), 60 GiB RAM.
- I/O, network, and contention: none during the measured in-memory planner invocation.

These are validation measurements after the signature-layout change, not a
performance-improvement claim.

## Qualifier-alignment validation: 2026-09-25

- Command: `scripts/benchmark.sh`
- Assignment input: fixed 10,000-row document; one composed document edit planned
  in 101.83 ms.
- C++ input: fixed 10,000-row alternating simple-declaration document; one composed
  document edit planned in 187.00 ms.
- Environment: x86_64, AMD Ryzen 7 7800X3D (8 cores / 16 threads), 60 GiB RAM.
- I/O, network, and contention: none during the measured in-memory planner invocation.

These are validation measurements after qualifier-aware parameter alignment, not a
performance-improvement claim.

## Constructor-initializer validation: 2026-09-25

- Command: `scripts/benchmark.sh`
- Assignment input: fixed 10,000-row document; one composed document edit planned
  in 97.31 ms.
- C++ input: fixed 10,000-row alternating simple-declaration document; one composed
  document edit planned in 184.99 ms.
- Environment: x86_64, AMD Ryzen 7 7800X3D (8 cores / 16 threads), 60 GiB RAM.
- I/O, network, and contention: none during the measured in-memory planner invocation.

These are validation measurements after C++ constructor-initializer recognition,
not a performance-improvement claim.

## Rust structural-profile validation: 2026-09-25

- Command: `scripts/benchmark.sh`
- Assignment input: fixed 10,000-row document; one composed document edit planned
  in 113.06 ms.
- C++ input: fixed 10,000-row alternating simple-declaration document; one composed
  document edit planned in 186.42 ms.
- Environment: x86_64, AMD Ryzen 7 7800X3D (8 cores / 16 threads), 60 GiB RAM.
- I/O, network, and contention: none during the measured in-memory planner invocation.

These measurements validate Rust lifetime and structural-boundary recognition;
they are not a performance-improvement claim.

## Attached-gap validation: 2026-09-25

- Command: `scripts/benchmark.sh`
- Assignment input: fixed 10,000-row document; one composed document edit planned
  in 122.99 ms.
- C++ input: fixed 10,000-row alternating simple-declaration document; one composed
  document edit planned in 198.67 ms.
- Environment: x86_64, AMD Ryzen 7 7800X3D (8 cores / 16 threads), 60 GiB RAM.
- I/O, network, and contention: none during the measured in-memory planner invocation.

These measurements validate stale attached-gap removal; they are not a
performance-improvement claim.

## Print-label validation: 2026-09-25

- Command: `scripts/benchmark.sh`
- Assignment input: fixed 10,000-row document; one composed document edit planned
  in 129.21 ms.
- C++ input: fixed 10,000-row alternating simple-declaration document; one composed
  document edit planned in 204.16 ms.
- Environment: x86_64, AMD Ryzen 7 7800X3D (8 cores / 16 threads), 60 GiB RAM.
- I/O, network, and contention: none during the measured in-memory planner invocation.

The additional static output-literal scan is linear in document size. These are
validation measurements, not a performance-improvement claim.

## Rust lifetime-field validation: 2026-09-27

- Command: `scripts/benchmark.sh`
- Assignment input: fixed 10,000-row document; one composed document edit planned
  in 123.34 ms.
- C++ input: fixed 10,000-row alternating simple-declaration document; one composed
  document edit planned in 200.34 ms.
- Environment: x86_64, AMD Ryzen 7 7800X3D (8 cores / 16 threads), 60 GiB RAM.
- I/O, network, and contention: none during the measured in-memory planner invocation.

The lifetime recognizer is a single linear lexical check. These are validation
measurements, not a performance-improvement claim.

## Python keyword-call validation: 2026-09-27

- Command: `scripts/benchmark.sh`
- Assignment input: fixed 10,000-row document; one composed document edit planned
  in 408.54 ms.
- C++ input: fixed 10,000-row alternating simple-declaration document; one composed
  document edit planned in 464.50 ms.
- Environment: x86_64, AMD Ryzen 7 7800X3D (8 cores / 16 threads), 60 GiB RAM.
- I/O, network, and contention: none during the measured in-memory planner invocation.

The keyword-call candidate scan visits parenthesis-bearing Python rows. These are
validation measurements, not a performance-improvement claim.

## Release-automation validation: 2026-09-27

- Command: `scripts/benchmark.sh`
- Assignment input: fixed 10,000-row document; one composed document edit planned
  in 140.77 ms.
- C++ input: fixed 10,000-row alternating simple-declaration document; one composed
  document edit planned in 213.61 ms.
- Environment: x86_64, AMD Ryzen 7 7800X3D (8 cores / 16 threads), 60 GiB RAM.
- I/O, network, and contention: none during the measured in-memory planner invocation.
- macOS compilation is performed by the GitHub Actions matrix after the repository
  is pushed.
- The release workflow changes packaging only. It does not alter the in-memory
  planner, its fixed benchmark inputs, or formatter runtime boundaries.

## Python multiline-suite validation: 2026-09-29

- Command: `scripts/benchmark.sh`
- Assignment input: fixed 10,000-row document; one composed document edit planned
  in 125.55 ms.
- C++ input: fixed 10,000-row alternating simple-declaration document; one composed
  document edit planned in 199.21 ms.
- Environment: x86_64, AMD Ryzen 7 7800X3D (8 cores / 16 threads), 60 GiB RAM.
- I/O, network, and contention: none during the measured in-memory planner invocation.

The multiline Python suite scanner adds only linear lexical state. These validation
measurements document its current cost and do not claim an improvement.
