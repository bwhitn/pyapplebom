# pyapplebom

`pyapplebom` is a Python library for parsing Apple Bill of Materials (BOM) files (commonly found inside `.pkg` installers).

It uses the Rust crate [`apple-bom`](https://crates.io/crates/apple-bom) for parsing and exposes rich metadata to Python.

## Features

- Full BOM file parsing through Rust `apple-bom`
- High-level path metadata (`paths`, `hl_index`, `size64`, `vindex`)
- Low-level metadata:
  - BOM header
  - blocks index
  - variables index
  - typed block decoding (`BomInfo`, `Tree`, `Paths`, `PathRecord`, etc.)
- Optional raw block bytes (hex encoded)
- Tested interface with a real BOM fixture
- Cross-platform design for Linux, macOS, and Windows

## Installation

### From PyPI

```bash
pip install pyapplebom
```

### From source

Prerequisites:

- Python 3.8+
- Rust toolchain (stable)
- `pip`

Install:

```bash
python -m venv .venv
source .venv/bin/activate
pip install maturin
pip install -e .
```

On Windows PowerShell, activate with `.venv\\Scripts\\Activate.ps1`.

## Quick Start

```python
from pathlib import Path
import pyapplebom

bom = pyapplebom.parse_bom_file("/path/to/Bom")

print(bom["header"]["magic"])        # BOMStore
print(len(bom["paths"]))              # Number of parsed paths in the Paths tree
print(bom["paths"][0]["path"])       # e.g. "."
print(bom["paths"][0]["symbolic_mode"])  # e.g. drwxr-xr-x
```

## API

### `parse_bom(data, *, include_blocks=True, include_raw_block_bytes=False, max_input_bytes=134217728, max_paths=250000)`

Parse BOM content from bytes-like input.

### `parse_bom_bytes(data, *, include_blocks=True, include_raw_block_bytes=False, max_input_bytes=134217728, max_paths=250000)`

Alias of `parse_bom`.

### `parse_bom_file(path, *, include_blocks=True, include_raw_block_bytes=False, max_input_bytes=134217728, max_paths=250000)`

Parse BOM content from a file path.

All parsing functions also accept these keyword-only resource limits:

- `max_input_bytes=134217728`: maximum input size (128 MiB)
- `max_paths=250000`: maximum declared or traversed paths per optional path section

The defaults are exported as `pyapplebom.DEFAULT_MAX_INPUT_BYTES` and `pyapplebom.DEFAULT_MAX_PATHS`. Increase them explicitly only when parsing a trusted BOM that legitimately exceeds a default.

### Exceptions

- `pyapplebom.BomParseError`: Raised for BOM parsing errors.

## Security Model

BOM files are treated as untrusted binary input. Before invoking the upstream parser, `pyapplebom` validates the file magic, index and block ranges, count-to-size relationships, path references, traversal cycles, expanded path data, and configured resource limits. Unexpected upstream Rust panics are contained and converted into parse errors.

Invalid top-level layout and resource amplification raise `BomParseError`. An invalid optional section such as `Paths` is returned as `None` with details in `parse_errors`, allowing safe metadata from other sections to remain available. Resource limits bound in-process work; they are not an operating-system sandbox for adversarial parsing.

## Return Structure

Each parse call returns a dictionary with these keys:

- `format`: Always `"apple-bom"`
- `byte_length`: Input size in bytes
- `source_path`: Included for `parse_bom_file`
- `header`: BOM header metadata
- `blocks_index`: Index metadata (`count` and block entries)
- `variables`: BOM variables (`BomInfo`, `Paths`, `HLIndex`, `VIndex`, `Size64` when present)
- `bom_info`: Parsed BomInfo metadata, or `None`
- `paths`: Parsed paths list, or `None`
- `hl_index`: Parsed hard link index paths, or `None`
- `size64`: Parsed Size64 paths, or `None`
- `vindex`: Parsed VIndex paths, or `None`
- `blocks`: Parsed block list (typed metadata) when `include_blocks=True`, else `None`
- `parse_errors`: Optional parse errors for non-fatal sections, or `None`

### Path entry fields

Path entries in `paths`/`hl_index`/`size64`/`vindex` include:

- `path`, `path_type`, `path_type_raw`
- `file_mode`, `symbolic_mode`
- `user_id`, `group_id`
- `mtime`, `mtime_iso8601`
- `size`, `crc32`, `link_name`

## Testing

Run tests:

```bash
python -m venv .venv
source .venv/bin/activate
python -m pip install -e '.[dev]'
python -m pytest --cov=pyapplebom --cov-report=term-missing
```

Run the full local quality and security suite:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-targets --all-features --locked
python -m ruff check .
python -m ruff format --check .
python -m mypy
zizmor --offline --strict-collection --min-severity=medium .
python -m pip_audit --local --strict --progress-spinner=off
cargo audit --deny warnings --file Cargo.lock
```

CI additionally measures `src/validation.rs` with `cargo-llvm-cov` and requires at least 80% line coverage. Python branch coverage is required to remain at 100%.

## Build and Publish (manual PyPI workflow)

Build wheels and source distribution:

```bash
python -m venv .venv
source .venv/bin/activate
pip install maturin
maturin build --release --locked --out dist
maturin sdist --out dist
```

Artifacts are placed in `dist/`.

Publish manually with your preferred process (for example `twine upload`) after validating test/build outputs.

## GitHub Tag Release Automation

This repo includes a release workflow at `.github/workflows/release.yml` that:

- builds wheels for:
  - Linux `x86_64` and `aarch64`
  - Windows `x86_64` and `aarch64`
  - macOS `x86_64` and `aarch64`
- builds an sdist
- verifies the tag version matches both `pyproject.toml` and `Cargo.toml`
- publishes to PyPI on tags matching `v*` (for example `v0.1.0`)

## Compatibility Notes

- Uses `pyo3` with `abi3` (`abi3-py38`) for broad CPython binary compatibility.
- CI runs dependency-free installed-wheel smoke tests on Python 3.8 and the full test suite on a maintained Python release.
- No platform-specific runtime logic is required for parsing.
- Build targets are suitable for Windows, Linux, and macOS when compiled on those platforms.

## License

MIT
