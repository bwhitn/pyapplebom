from __future__ import annotations

from pathlib import Path

import pytest

import pyapplebom

FIXTURE = Path(__file__).parent / "fixtures" / "python-applications.bom"


def test_parse_bom_file_has_expected_top_level_metadata() -> None:
    doc = pyapplebom.parse_bom_file(FIXTURE)

    assert doc["format"] == "apple-bom"
    assert doc["byte_length"] > 0
    assert doc["header"]["magic"] == "BOMStore"
    assert doc["blocks_index"]["count"] >= 1
    assert any(variable["name"] == "Paths" for variable in doc["variables"])
    assert isinstance(doc["paths"], list)


def test_parse_bom_file_contains_known_path_metadata() -> None:
    doc = pyapplebom.parse_bom_file(FIXTURE, include_blocks=False)
    root = next(path for path in doc["paths"] if path["path"] == ".")
    readme = next(path for path in doc["paths"] if path["path"] == "./Python 3.9/ReadMe.rtf")

    assert root["path_type"] == "directory"
    assert root["symbolic_mode"] == "drwxr-xr-x"
    assert readme["path_type"] == "file"
    assert readme["symbolic_mode"] == "-rw-r--r--"
    assert readme["size"] > 0


def test_parse_bom_bytes_matches_parse_bom_file_path_count() -> None:
    data = FIXTURE.read_bytes()

    from_file = pyapplebom.parse_bom_file(FIXTURE, include_blocks=False)
    from_bytes = pyapplebom.parse_bom_bytes(data, include_blocks=False)

    assert len(from_file["paths"]) == len(from_bytes["paths"])


def test_include_raw_block_bytes_exposes_raw_hex() -> None:
    doc = pyapplebom.parse_bom_file(
        FIXTURE,
        include_blocks=True,
        include_raw_block_bytes=True,
    )

    assert isinstance(doc["blocks"], list)
    assert any(block["kind"] == "Tree" for block in doc["blocks"])
    assert all("raw_hex" in block for block in doc["blocks"])


def test_parse_invalid_data_raises() -> None:
    with pytest.raises(pyapplebom.BomParseError):
        pyapplebom.parse_bom_bytes(b"this is not a bom file")
