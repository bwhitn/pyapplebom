from __future__ import annotations

from pathlib import Path

import pytest

import pyapplebom

FIXTURE = Path(__file__).parent / "fixtures" / "python-applications.bom"
FIXTURE_DATA = FIXTURE.read_bytes()


def _read_u16(data: bytes | bytearray, offset: int) -> int:
    return int.from_bytes(data[offset : offset + 2], "big")


def _read_u32(data: bytes | bytearray, offset: int) -> int:
    return int.from_bytes(data[offset : offset + 4], "big")


def _write_u32(data: bytearray, offset: int, value: int) -> None:
    data[offset : offset + 4] = value.to_bytes(4, "big")


def _block_entries(data: bytes | bytearray) -> list[tuple[int, int]]:
    index_offset = _read_u32(data, 16)
    count = _read_u32(data, index_offset)
    return [
        (
            _read_u32(data, index_offset + 4 + index * 8),
            _read_u32(data, index_offset + 8 + index * 8),
        )
        for index in range(count)
    ]


def _variables(data: bytes | bytearray) -> dict[str, int]:
    index_offset = _read_u32(data, 24)
    count = _read_u32(data, index_offset)
    offset = index_offset + 4
    result: dict[str, int] = {}

    for _ in range(count):
        block_index = _read_u32(data, offset)
        name_length = data[offset + 4]
        name = bytes(data[offset + 5 : offset + 5 + name_length]).decode()
        result[name] = block_index
        offset += 5 + name_length

    return result


def _first_info_paths_block(data: bytes | bytearray, tree_index: int) -> int:
    blocks = _block_entries(data)
    tree_offset, _ = blocks[tree_index]
    paths_index = _read_u32(data, tree_offset + 8)

    while True:
        paths_offset, _ = blocks[paths_index]
        if _read_u16(data, paths_offset) != 0:
            return paths_index
        paths_index = _read_u32(data, paths_offset + 12)


def test_truncated_headers_raise_parse_error() -> None:
    for size in (0, 1, 7, 8, 31):
        with pytest.raises(pyapplebom.BomParseError):
            pyapplebom.parse_bom_bytes(FIXTURE_DATA[:size])


def test_invalid_magic_is_rejected() -> None:
    data = bytearray(FIXTURE_DATA)
    data[:8] = b"NOTABOM!"

    with pytest.raises(pyapplebom.BomParseError, match="magic"):
        pyapplebom.parse_bom_bytes(data)


def test_out_of_bounds_index_range_is_rejected() -> None:
    data = bytearray(FIXTURE_DATA)
    _write_u32(data, 16, len(data))

    with pytest.raises(pyapplebom.BomParseError, match="blocks index range"):
        pyapplebom.parse_bom_bytes(data)


def test_attacker_controlled_block_count_is_bounded_before_allocation() -> None:
    data = bytearray(FIXTURE_DATA)
    blocks_offset = _read_u32(data, 16)
    _write_u32(data, blocks_offset, (1 << 32) - 1)

    with pytest.raises(pyapplebom.BomParseError, match="blocks index contains"):
        pyapplebom.parse_bom_bytes(data)


def test_attacker_controlled_variable_count_is_bounded_before_allocation() -> None:
    data = bytearray(FIXTURE_DATA)
    variables_offset = _read_u32(data, 24)
    _write_u32(data, variables_offset, (1 << 32) - 1)

    with pytest.raises(pyapplebom.BomParseError, match="variables index contains"):
        pyapplebom.parse_bom_bytes(data)


def test_out_of_bounds_block_data_is_rejected() -> None:
    data = bytearray(FIXTURE_DATA)
    blocks_offset = _read_u32(data, 16)
    first_real_entry = blocks_offset + 4 + 8
    _write_u32(data, first_real_entry, len(data))
    _write_u32(data, first_real_entry + 4, 1)

    with pytest.raises(pyapplebom.BomParseError, match="block data range"):
        pyapplebom.parse_bom_bytes(data)


def test_oversized_tree_path_count_becomes_nonfatal_parse_error() -> None:
    data = bytearray(FIXTURE_DATA)
    blocks = _block_entries(data)
    paths_tree_index = _variables(data)["Paths"]
    paths_tree_offset, _ = blocks[paths_tree_index]
    _write_u32(data, paths_tree_offset + 16, (1 << 32) - 1)

    doc = pyapplebom.parse_bom_bytes(data, include_blocks=False)

    assert doc["paths"] is None
    assert "max_paths" in doc["parse_errors"]["paths"]


def test_cyclic_paths_chain_becomes_nonfatal_parse_error() -> None:
    data = bytearray(FIXTURE_DATA)
    paths_tree_index = _variables(data)["Paths"]
    paths_index = _first_info_paths_block(data, paths_tree_index)
    paths_offset, _ = _block_entries(data)[paths_index]
    _write_u32(data, paths_offset + 4, paths_index)

    doc = pyapplebom.parse_bom_bytes(data, include_blocks=False)

    assert doc["paths"] is None
    assert "cycle detected" in doc["parse_errors"]["paths"]


def test_oversized_bom_info_count_does_not_reach_upstream_allocation() -> None:
    data = bytearray(FIXTURE_DATA)
    bom_info_index = _variables(data)["BomInfo"]
    bom_info_offset, _ = _block_entries(data)[bom_info_index]
    _write_u32(data, bom_info_offset + 8, (1 << 32) - 1)

    doc = pyapplebom.parse_bom_bytes(data)

    assert doc["bom_info"] is None
    assert "invalid entry count" in doc["parse_errors"]["bom_info"]


def test_oversized_link_name_does_not_trigger_unchecked_slice() -> None:
    data = bytearray(FIXTURE_DATA)
    blocks = _block_entries(data)
    paths_tree_index = _variables(data)["Paths"]
    paths_index = _first_info_paths_block(data, paths_tree_index)
    paths_offset, _ = blocks[paths_index]
    path_info_index = _read_u32(data, paths_offset + 12)
    path_info_offset, _ = blocks[path_info_index]
    record_index = _read_u32(data, path_info_offset + 4)
    record_offset, _ = blocks[record_index]
    data[record_offset] = 3
    _write_u32(data, record_offset + 27, (1 << 32) - 1)

    doc = pyapplebom.parse_bom_bytes(data)

    assert doc["paths"] is None
    assert "path record block" in doc["parse_errors"]["paths"]


def test_overlapping_blocks_cannot_amplify_block_output_without_bound() -> None:
    data = bytearray(FIXTURE_DATA)
    blocks_offset = _read_u32(data, 16)
    block_count = _read_u32(data, blocks_offset)

    for index in range(block_count):
        entry_offset = blocks_offset + 4 + index * 8
        _write_u32(data, entry_offset, 0)
        _write_u32(data, entry_offset + 4, len(data))

    with pytest.raises(pyapplebom.BomParseError, match="block output would process"):
        pyapplebom.parse_bom_bytes(data, max_input_bytes=len(data))


def test_deterministic_single_byte_mutations_do_not_reach_native_panics() -> None:
    sample_count = 64

    for sample in range(sample_count):
        data = bytearray(FIXTURE_DATA)
        offset = sample * (len(data) - 1) // (sample_count - 1)
        data[offset] ^= 1 << (sample % 8)

        try:
            document = pyapplebom.parse_bom_bytes(data, max_input_bytes=len(data))
        except pyapplebom.BomParseError as error:
            outcome = str(error)
        else:
            outcome = repr(document)

        assert "panicked" not in outcome
