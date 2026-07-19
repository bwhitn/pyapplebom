"""Python bindings for parsing Apple BOM files."""

from __future__ import annotations

from os import PathLike
from typing import Any

from ._native import (
    DEFAULT_MAX_INPUT_BYTES,
    DEFAULT_MAX_PATHS,
    BomParseError,
    __version__,
)
from ._native import parse_bom_bytes as _parse_bom_bytes
from ._native import parse_bom_file as _parse_bom_file

__all__ = [
    "DEFAULT_MAX_INPUT_BYTES",
    "DEFAULT_MAX_PATHS",
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
    max_input_bytes: int = DEFAULT_MAX_INPUT_BYTES,
    max_paths: int = DEFAULT_MAX_PATHS,
) -> dict[str, Any]:
    """Parse Apple BOM data from a bytes-like object within resource limits."""
    if not isinstance(data, (bytes, bytearray, memoryview)):
        raise TypeError("data must be bytes, bytearray, or memoryview")
    if type(max_input_bytes) is not int:
        raise TypeError("max_input_bytes must be an integer")
    if type(max_paths) is not int:
        raise TypeError("max_paths must be an integer")
    if max_input_bytes <= 0:
        raise ValueError("max_input_bytes must be greater than zero")
    if max_paths <= 0:
        raise ValueError("max_paths must be greater than zero")

    view = memoryview(data)
    input_size = view.nbytes
    if input_size > max_input_bytes:
        raise BomParseError(
            f"BOM input is {input_size} bytes, exceeding max_input_bytes={max_input_bytes}"
        )

    if not isinstance(data, bytes):
        data = view.tobytes()

    return _parse_bom_bytes(
        data,
        include_blocks=include_blocks,
        include_raw_block_bytes=include_raw_block_bytes,
        max_input_bytes=max_input_bytes,
        max_paths=max_paths,
    )


def parse_bom_bytes(
    data: bytes | bytearray | memoryview,
    *,
    include_blocks: bool = True,
    include_raw_block_bytes: bool = False,
    max_input_bytes: int = DEFAULT_MAX_INPUT_BYTES,
    max_paths: int = DEFAULT_MAX_PATHS,
) -> dict[str, Any]:
    """Alias for :func:`parse_bom`."""
    return parse_bom(
        data,
        include_blocks=include_blocks,
        include_raw_block_bytes=include_raw_block_bytes,
        max_input_bytes=max_input_bytes,
        max_paths=max_paths,
    )


def parse_bom_file(
    path: str | PathLike[str],
    *,
    include_blocks: bool = True,
    include_raw_block_bytes: bool = False,
    max_input_bytes: int = DEFAULT_MAX_INPUT_BYTES,
    max_paths: int = DEFAULT_MAX_PATHS,
) -> dict[str, Any]:
    """Parse an Apple BOM from a filesystem path within resource limits."""
    return _parse_bom_file(
        str(path),
        include_blocks=include_blocks,
        include_raw_block_bytes=include_raw_block_bytes,
        max_input_bytes=max_input_bytes,
        max_paths=max_paths,
    )
