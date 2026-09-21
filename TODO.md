# pyapplebom TODO

## Current

- [ ] Finish the Rust 1.98.1 release and downstream ALES handoff — **The `0.1.2` release candidate passes every
  local gate; hosted CI, publication, and the downstream handoff remain**:
  - [x] Push modernization candidate `4c014a5914edc940ef531abbc935f4c82c64aef0` to `origin/master` with the
    reproducible benchmark record and locally passing Rust, Python, security, coverage, audit, wheel, and sdist gates.
  - [x] Apply Ruff's Markdown code-block formatting to `README.md` and rerun both `python -m ruff check .` and
    `python -m ruff format --check .`. GitHub Actions run `35600677189`, job `106335761177`, reached a runner and
    failed only because the example at `README.md:59` would be reformatted; this is a real gate failure, not the
    account Actions-capacity rejection.
  - [ ] Rerun the entire CI job after that fix, not only Ruff. The failed job stopped before mypy, zizmor, pip-audit,
    and the Python branch-coverage suite. Require those steps plus Rust formatting/Clippy/tests/native coverage,
    cargo-audit, all legacy-Python wheel smokes, and the Rust 1.83 MSRV lane to pass on one final commit. The owner
    restored $20 of Actions capacity on 2026-09-21; make and validate the formatting fix locally first, then run only
    the required hosted workflow rather than spending the limited budget on redundant reruns.
  - [x] Select package version `0.1.2`, greater than the already published/tagged `0.1.1`, update `Cargo.toml`,
    `pyproject.toml`, and the root package entry in `Cargo.lock` together, and rerun the release workflow's version/tag
    consistency check. Do not overwrite or relabel the existing `v0.1.1` artifacts.
  - [ ] Create the matching immutable tag only after CI is green, then let the protected `pypi` environment publish
    the ABI3 wheels and sdist built from that tag. Verify artifact names, hashes, licenses, imports, and the published
    version before treating the release as available.
  - [ ] Update ALES from `pyapplebom==0.1.1` to the new exact release, refresh `poetry.lock`, and pass BOM analyzer,
    output/ObjectRules, image, SBOM/license, runtime-pruning, and authorized-corpus acceptance.

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
  - [x] Run the complete locally available security, coverage, Rust/Python, audit, documentation, wheel, and release
    gates and record reproducible before/after results. Hosted CI correction and immutable publication are tracked in
    Current above and are intentionally not claimed complete here.
