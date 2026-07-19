# Repository guidance

This file applies to the entire repository.

## Project shape

- `src/lib.rs` is the PyO3 boundary and Python-object serializer.
- `src/validation.rs` validates untrusted BOM structure and resource usage before any potentially unsafe `apple-bom` parser path is called.
- `pyapplebom/__init__.py` is the public Python API; `_native.pyi` must stay synchronized with the native signatures.
- `tests/test_api.py` covers supported behavior, `tests/test_security.py` covers malformed-input regressions, and `tests/smoke.py` provides dependency-free installed-wheel coverage for legacy Python.
- The extension uses PyO3 `abi3-py38`; retain Python 3.8 compatibility unless the package metadata, CI matrix, documentation, and release targets are changed together.

## Security invariants

Treat every BOM byte, offset, count, path relationship, and file path as attacker-controlled.

- Validate the container before calling `ParsedBom::parse`.
- Bound input size, block count/output, path count, expanded path data, and traversal depth.
- Use checked arithmetic and checked slices for format offsets and lengths, including the BOM format's 32-bit offset width.
- Detect cycles in root and linked path-block traversal before calling upstream tree traversal.
- Do not rely on `catch_unwind` as input validation; it is only a final containment layer.
- Do not call upstream block parsers with attacker-controlled allocation counts until their required serialized size has been checked against the containing block.
- Keep `unsafe_code = "forbid"`. Do not add `unsafe` without an explicit design and security review.
- New or changed resource-limit arguments must be propagated through the Rust functions, Python wrappers, stub, README, and regression tests.
- Do not add pytest older than 9.0.3 for Python 3.8/3.9; no patched pytest release supports those EOL interpreters. Keep their wheel validation dependency-free.

## Development setup

```bash
python3 -m venv .venv
source .venv/bin/activate
python -m pip install -e '.[dev]'
```

Use `python` after activating the environment. On systems where it is not activated, use `.venv/bin/python` (or the Windows equivalent).

## Required checks

Run all of these before handing off changes:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-targets --all-features --locked
python -m ruff check .
python -m ruff format --check .
python -m mypy
zizmor --offline --strict-collection --min-severity=medium .
python -m pip_audit --requirement <(python -m pip freeze --all --exclude-editable) --no-deps --strict --progress-spinner=off
python -m pytest --cov=pyapplebom --cov-report=term-missing
cargo audit --deny warnings --file Cargo.lock
maturin build --release --locked --out dist
```

Python coverage is configured to require 100% branch coverage for the Python wrapper. CI also requires at least 80% line coverage for `src/validation.rs` using `cargo-llvm-cov`. Rust changes should include focused unit tests; parser/security changes also need Python integration regressions that exercise the built extension.

## Change discipline

- Keep Rust formatted with `rustfmt` and Python formatted with Ruff.
- Preserve the `Cargo.lock` file and use `--locked` in reproducible build/test commands.
- Prefer precise public types in `_native.pyi`; avoid broadening the public API to `Any` beyond the heterogeneous parsed document return value.
- Keep malformed optional sections nonfatal through `parse_errors` when safe to do so. Invalid top-level layout, limit violations, or unsafe block amplification should raise `BomParseError`.
- Do not commit `.venv`, `target`, `dist`, coverage output, caches, or compiled extension files.
- Release workflow edits must retain least-privilege permissions, tag/version verification, and trusted publishing through the protected `pypi` environment.
