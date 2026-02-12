"""Python bindings for parsing Apple BOM files."""

from __future__ import annotations

from os import PathLike
from typing import Any

from ._native import BomParseError, __version__, parse_bom_bytes as _parse_bom_bytes
from ._native import parse_bom_file as _parse_bom_file

__all__ = [
    "BomParseError",
    "__version__",
    "parse_bom",
    "parse_bom_bytes",
    "parse_bom_file",
]


def parse_bom(
    data: bytes | bytearray | memoryview,
    *,
    include_blocks: bool = True,
    include_raw_block_bytes: bool = False,
) -> dict[str, Any]:
    """Parse Apple BOM data from a bytes-like object."""
    if isinstance(data, memoryview):
        data = data.tobytes()
    elif isinstance(data, bytearray):
        data = bytes(data)

    if not isinstance(data, bytes):
        raise TypeError("data must be bytes, bytearray, or memoryview")

    return _parse_bom_bytes(
        data,
        include_blocks=include_blocks,
        include_raw_block_bytes=include_raw_block_bytes,
    )


def parse_bom_bytes(
    data: bytes | bytearray | memoryview,
    *,
    include_blocks: bool = True,
    include_raw_block_bytes: bool = False,
) -> dict[str, Any]:
    """Alias for :func:`parse_bom`."""
    return parse_bom(
        data,
        include_blocks=include_blocks,
        include_raw_block_bytes=include_raw_block_bytes,
    )


def parse_bom_file(
    path: str | PathLike[str],
    *,
    include_blocks: bool = True,
    include_raw_block_bytes: bool = False,
) -> dict[str, Any]:
    """Parse an Apple BOM from a filesystem path."""
    return _parse_bom_file(
        str(path),
        include_blocks=include_blocks,
        include_raw_block_bytes=include_raw_block_bytes,
    )
