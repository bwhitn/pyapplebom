# pyapplebom TODO

## Completed 2026-09-21

- [x] Adopt Rust 1.98.1 and optimize measured BOM parsing/projection hot paths — **Completed** ([results](benchmarks/results/2026-09-21-rust-1.98.1.md)):
  - [x] Capture release-mode baselines for generated valid and hostile BOMs covering block validation, path trees,
    optional sections, malformed partial results, and Python projection. Record wall time, CPU, peak memory,
    allocations/copies, and wheel/native size.
  - [x] Make Rust 1.98.1 the reproducible primary CI, release, and local toolchain. Declare an evidence-backed Cargo
    MSRV separately and retain Python 3.8 plus `abi3-py38` compatibility unless all published support surfaces change
    together.
  - [x] Profile duplicate container validation, block lookup, root/linked path traversal, cycle detection, path
    materialization, owned byte/string copies, and Python dictionary/list creation. Use current stable APIs and data
    structures where measurement shows fewer passes or allocations without bypassing prevalidation.
  - [x] Preserve checked ranges, allocation/count/depth limits, `unsafe_code = "forbid"`, parse-error semantics,
    public typing, and 100% Python wrapper coverage. Performance changes must not move hostile data into an unchecked
    upstream parser path.
  - [x] Run all repository security, coverage, Rust/Python, audit, documentation, wheel, and release gates. Record
    reproducible before/after results and publish only an immutable revision with no compatibility regression.
