# pyapplebom TODO

## Current

No current items.

## Completed 2026-09-21

- [x] Finish the Rust 1.98.1 release and downstream ALES handoff — **Completed**:
  - [x] Correct the README formatting failure, select version 0.1.2, and pass the complete local gate set plus the
    final hosted [CI](https://github.com/bwhitn/pyapplebom/actions/runs/35623948152) and
    [dependency-security](https://github.com/bwhitn/pyapplebom/actions/runs/35623948083) workflows at immutable
    revision `b677efe1dee37e7610efc7096286770b6ad3cbf5`, including Rust 1.83 MSRV, Rust 1.98.1, Python 3.8 legacy smoke,
    native coverage, branch coverage, audits, and every supported wheel platform.
  - [x] Create immutable tag `v0.1.2` only after CI is green. The protected
    [trusted-publishing workflow](https://github.com/bwhitn/pyapplebom/actions/runs/35623948163) publishes six ABI3
    wheels and the sdist to PyPI; every downloaded public artifact matches its workflow hash, retains the required
    license/SBOM or source files, imports successfully, and reports version 0.1.2.
  - [x] Update ALES to exact public `pyapplebom==0.1.2`, refresh all seven locked artifact hashes, retain the
    license/SBOM before and after pruning, and pass its focused and complete host tests plus locked-down test and
    operational images. The benign BOM emits byte-identical ALES and ObjectRules output versus 0.1.1, its normalized
    parser output is unchanged, and the 90-iteration in-image median improves 42.03%. No standalone BOM exists in the
    140-file authorized corpus, so the public benign upstream fixture is the applicable end-to-end replay.
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
  - [x] Run the complete locally available security, coverage, Rust/Python, audit, documentation, wheel, and release
    gates and record reproducible before/after results. Hosted CI correction and immutable publication are tracked in
    Current above and are intentionally not claimed complete here.
