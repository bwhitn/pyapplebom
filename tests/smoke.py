"""Dependency-free smoke test for an installed pyapplebom wheel."""

from __future__ import annotations

from pathlib import Path

import pyapplebom

FIXTURE = Path(__file__).parent / "fixtures" / "python-applications.bom"


def main() -> None:
    document = pyapplebom.parse_bom_file(FIXTURE, include_blocks=False)

    assert document["header"]["magic"] == "BOMStore"
    assert document["source_path"] == str(FIXTURE)
    assert document["paths"]


if __name__ == "__main__":
    main()
