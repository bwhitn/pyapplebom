from typing import Any

class BomParseError(Exception): ...

def parse_bom_bytes(
    data: bytes,
    *,
    include_blocks: bool = True,
    include_raw_block_bytes: bool = False,
) -> dict[str, Any]: ...

def parse_bom_file(
    path: str,
    *,
    include_blocks: bool = True,
    include_raw_block_bytes: bool = False,
) -> dict[str, Any]: ...

__version__: str
