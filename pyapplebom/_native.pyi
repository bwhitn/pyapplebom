from typing import Any

class BomParseError(Exception): ...

def parse_bom_bytes(
    data: bytes,
    *,
    include_blocks: bool = True,
    include_raw_block_bytes: bool = False,
    max_input_bytes: int = 134217728,
    max_paths: int = 250000,
) -> dict[str, Any]: ...
def parse_bom_file(
    path: str,
    *,
    include_blocks: bool = True,
    include_raw_block_bytes: bool = False,
    max_input_bytes: int = 134217728,
    max_paths: int = 250000,
) -> dict[str, Any]: ...

DEFAULT_MAX_INPUT_BYTES: int
DEFAULT_MAX_PATHS: int
__version__: str
