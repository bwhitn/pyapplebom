from __future__ import annotations

from pathlib import Path

import pytest

import pyapplebom

FIXTURE = Path(__file__).parent / "fixtures" / "python-applications.bom"
FIXTURE_DATA = FIXTURE.read_bytes()


class _MisleadingBytearray(bytearray):
    def __len__(self) -> int:
        return 0

    def __bytes__(self) -> bytes:
        raise AssertionError("the parser must copy through the validated buffer view")


def test_parse_bom_file_has_expected_top_level_metadata() -> None:
    doc = pyapplebom.parse_bom_file(FIXTURE)

    assert doc["format"] == "apple-bom"
    assert doc["byte_length"] > 0
    assert doc["source_path"] == str(FIXTURE)
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
    from_file = pyapplebom.parse_bom_file(FIXTURE, include_blocks=False)
    from_bytes = pyapplebom.parse_bom_bytes(FIXTURE_DATA, include_blocks=False)

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


def test_parse_bom_accepts_supported_bytes_like_inputs() -> None:
    inputs: tuple[bytes | bytearray | memoryview, ...] = (
        FIXTURE_DATA,
        bytearray(FIXTURE_DATA),
        memoryview(FIXTURE_DATA),
    )

    for data in inputs:
        doc = pyapplebom.parse_bom(data, include_blocks=False)
        assert doc["header"]["magic"] == "BOMStore"


def test_parse_bom_rejects_non_bytes_like_input() -> None:
    with pytest.raises(TypeError, match="data must be bytes"):
        pyapplebom.parse_bom("not bytes")  # type: ignore[arg-type]


def test_include_blocks_false_returns_none() -> None:
    doc = pyapplebom.parse_bom_bytes(
        FIXTURE_DATA,
        include_blocks=False,
        include_raw_block_bytes=True,
    )

    assert doc["blocks"] is None


def test_public_metadata_and_limits_are_exposed() -> None:
    assert pyapplebom.__version__ == "0.1.0"
    assert pyapplebom.DEFAULT_MAX_INPUT_BYTES == 128 * 1024 * 1024
    assert pyapplebom.DEFAULT_MAX_PATHS == 250_000


def test_parse_bom_file_missing_path_raises_os_error(tmp_path: Path) -> None:
    with pytest.raises(OSError, match="failed opening"):
        pyapplebom.parse_bom_file(tmp_path / "missing.bom")


def test_max_input_bytes_must_be_positive() -> None:
    with pytest.raises(ValueError, match="max_input_bytes must be greater than zero"):
        pyapplebom.parse_bom_bytes(FIXTURE_DATA, max_input_bytes=0)


def test_max_paths_must_be_positive() -> None:
    with pytest.raises(ValueError, match="max_paths must be greater than zero"):
        pyapplebom.parse_bom_bytes(FIXTURE_DATA, max_paths=0)


def test_resource_limits_must_be_integers() -> None:
    with pytest.raises(TypeError, match="max_input_bytes must be an integer"):
        pyapplebom.parse_bom_bytes(FIXTURE_DATA, max_input_bytes=float("inf"))  # type: ignore[arg-type]
    with pytest.raises(TypeError, match="max_paths must be an integer"):
        pyapplebom.parse_bom_bytes(FIXTURE_DATA, max_paths=1.5)  # type: ignore[arg-type]


def test_input_size_limit_applies_to_bytes_and_files() -> None:
    limit = len(FIXTURE_DATA) - 1

    inputs: tuple[bytes | bytearray | memoryview, ...] = (
        FIXTURE_DATA,
        bytearray(FIXTURE_DATA),
        memoryview(FIXTURE_DATA),
    )
    for data in inputs:
        with pytest.raises(pyapplebom.BomParseError, match="max_input_bytes"):
            pyapplebom.parse_bom_bytes(data, max_input_bytes=limit)
    with pytest.raises(pyapplebom.BomParseError, match="max_input_bytes"):
        pyapplebom.parse_bom_bytes(_MisleadingBytearray(FIXTURE_DATA), max_input_bytes=limit)
    with pytest.raises(pyapplebom.BomParseError, match="max_input_bytes"):
        pyapplebom.parse_bom_file(FIXTURE, max_input_bytes=limit)


def test_path_limit_is_reported_as_a_nonfatal_section_error() -> None:
    doc = pyapplebom.parse_bom_bytes(
        FIXTURE_DATA,
        include_blocks=False,
        max_paths=1,
    )

    assert doc["paths"] is None
    assert "max_paths" in doc["parse_errors"]["paths"]
