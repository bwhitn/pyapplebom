#!/usr/bin/env python3
"""Reproducible release-mode benchmarks for pyapplebom.

The driver starts one worker process per scenario so peak RSS is comparable
between scenarios.  The corpus is generated deterministically and includes
valid path trees, all optional path sections, block projection, and malformed
inputs which exercise both fatal and non-fatal validation paths.
"""

from __future__ import annotations

import argparse
import gc
import json
import os
import platform
import resource
import statistics
import struct
import subprocess
import sys
import time
import tracemalloc
from pathlib import Path
from typing import Any, NamedTuple

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_PATH_COUNT = 4_000
DEFAULT_ITERATIONS = 15
DEFAULT_WARMUPS = 3


class Corpus(NamedTuple):
    data: bytes
    paths_blocks: tuple[int, ...]
    file_record_block: int


class Scenario(NamedTuple):
    name: str
    corpus: str
    include_blocks: bool
    include_raw_block_bytes: bool
    expected: str


SCENARIOS = (
    Scenario("valid_paths", "paths", False, False, "valid"),
    Scenario("valid_optional_sections", "optional", False, False, "valid"),
    Scenario("valid_blocks", "paths", True, False, "valid"),
    Scenario("valid_raw_blocks", "paths", True, True, "valid"),
    Scenario("hostile_block_count", "block_count", False, False, "fatal"),
    Scenario("hostile_link_length", "link_length", False, False, "nonfatal"),
    Scenario("hostile_paths_cycle", "cycle", False, False, "nonfatal"),
)


def _path_record(path_type: int, mode: int) -> bytes:
    return struct.pack(
        ">BBHHIIIIBII",
        path_type,
        0,
        0,
        mode,
        501,
        20,
        1_700_000_000,
        123,
        0,
        0x12345678,
        0,
    )


def _variable_index(variables: list[tuple[str, int]]) -> bytes:
    entries = []
    for name, block_index in variables:
        encoded = name.encode("ascii")
        entries.append(struct.pack(">IB", block_index, len(encoded)) + encoded)
    return struct.pack(">I", len(entries)) + b"".join(entries)


def build_corpus(path_count: int, *, optional_sections: bool) -> Corpus:
    """Build a shallow, valid BOM without relying on platform BOM tooling."""
    if not 1 <= path_count <= 65_535:
        raise ValueError("path_count must fit the BOM paths-block u16 field")

    blocks = [b"", struct.pack(">III", 1, path_count, 0)]
    directory_record = len(blocks)
    blocks.append(_path_record(2, 0o755))
    file_record = len(blocks)
    blocks.append(_path_record(1, 0o644))

    path_entries: list[bytes] = []
    for index in range(path_count):
        path_info_index = len(blocks)
        file_index = path_info_index + 1
        record_index = directory_record if index == 0 else file_record
        blocks.append(struct.pack(">II", index + 1, record_index))

        name = b"." if index == 0 else f"file-{index:05d}".encode("ascii")
        parent_id = 0 if index == 0 else 1
        blocks.append(struct.pack(">I", parent_id) + name + b"\0")
        path_entries.append(struct.pack(">II", path_info_index, file_index))

    section_names = ("Paths", "HLIndex", "Size64", "VIndex") if optional_sections else ("Paths",)
    variables: list[tuple[str, int]] = [("BomInfo", 1)]
    paths_blocks: list[int] = []
    entries_data = b"".join(path_entries)

    for name in section_names:
        tree_index = len(blocks)
        blocks.append(b"")
        paths_index = len(blocks)
        paths_blocks.append(paths_index)
        blocks.append(struct.pack(">HHII", 1, path_count, 0, 0) + entries_data)
        blocks[tree_index] = b"tree" + struct.pack(">IIIIB", 1, paths_index, 4_096, path_count, 0)

        if name == "VIndex":
            vindex = len(blocks)
            blocks.append(struct.pack(">IIBI", 0, tree_index, 0, 0))
            variables.append((name, vindex))
        else:
            variables.append((name, tree_index))

    vars_data = _variable_index(variables)
    blocks_index_length = 4 + len(blocks) * 8
    vars_offset = 32 + blocks_index_length
    payload_offset = vars_offset + len(vars_data)

    block_entries = []
    offset = payload_offset
    for block in blocks:
        block_entries.append(struct.pack(">II", offset, len(block)))
        offset += len(block)

    blocks_data = struct.pack(">I", len(blocks)) + b"".join(block_entries)
    header = b"BOMStore" + struct.pack(
        ">IIIIII",
        1,
        len(blocks) - 1,
        32,
        len(blocks_data),
        vars_offset,
        len(vars_data),
    )
    return Corpus(
        header + blocks_data + vars_data + b"".join(blocks),
        tuple(paths_blocks),
        file_record,
    )


def _block_offset(data: bytes | bytearray, block_index: int) -> int:
    blocks_index_offset = struct.unpack_from(">I", data, 16)[0]
    return struct.unpack_from(">I", data, blocks_index_offset + 4 + block_index * 8)[0]


def scenario_data(name: str, path_count: int) -> bytes:
    if name == "paths":
        return build_corpus(path_count, optional_sections=False).data
    if name == "optional":
        return build_corpus(path_count, optional_sections=True).data
    if name == "block_count":
        paths = build_corpus(path_count, optional_sections=False)
        data = bytearray(paths.data)
        blocks_index_offset = struct.unpack_from(">I", data, 16)[0]
        struct.pack_into(">I", data, blocks_index_offset, (1 << 32) - 1)
        return bytes(data)
    if name == "link_length":
        optional = build_corpus(path_count, optional_sections=True)
        data = bytearray(optional.data)
        record_offset = _block_offset(data, optional.file_record_block)
        data[record_offset] = 3
        struct.pack_into(">I", data, record_offset + 27, (1 << 32) - 1)
        return bytes(data)
    if name == "cycle":
        optional = build_corpus(path_count, optional_sections=True)
        data = bytearray(optional.data)
        for paths_block in optional.paths_blocks:
            paths_offset = _block_offset(data, paths_block)
            struct.pack_into(">I", data, paths_offset + 4, paths_block)
        return bytes(data)
    raise ValueError(f"unknown corpus {name!r}")


def _run_parser(scenario: Scenario, data: bytes) -> Any:
    import pyapplebom

    try:
        document = pyapplebom.parse_bom_bytes(
            data,
            include_blocks=scenario.include_blocks,
            include_raw_block_bytes=scenario.include_raw_block_bytes,
            max_input_bytes=len(data),
        )
    except pyapplebom.BomParseError as error:
        if scenario.expected != "fatal":
            raise AssertionError(f"{scenario.name} unexpectedly failed: {error}") from error
        return str(error)

    if scenario.expected == "fatal":
        raise AssertionError(f"{scenario.name} unexpectedly parsed")
    if scenario.expected == "nonfatal" and not isinstance(document["parse_errors"], dict):
        raise AssertionError(f"{scenario.name} did not report a partial-result parse error")
    return document


def _percentile(values: list[int], percentile: float) -> float:
    ordered = sorted(values)
    position = (len(ordered) - 1) * percentile
    lower = int(position)
    upper = min(lower + 1, len(ordered) - 1)
    fraction = position - lower
    return ordered[lower] * (1 - fraction) + ordered[upper] * fraction


def _deep_size(value: Any) -> int:
    seen: set[int] = set()

    def visit(item: Any) -> int:
        identity = id(item)
        if identity in seen:
            return 0
        seen.add(identity)
        size = sys.getsizeof(item)
        if isinstance(item, dict):
            return size + sum(visit(key) + visit(child) for key, child in item.items())
        if isinstance(item, (list, tuple)):
            return size + sum(visit(child) for child in item)
        return size

    return visit(value)


def _string_bytes(value: Any) -> int:
    if isinstance(value, str):
        return len(value.encode("utf-8"))
    if isinstance(value, dict):
        return sum(_string_bytes(key) + _string_bytes(child) for key, child in value.items())
    if isinstance(value, (list, tuple)):
        return sum(_string_bytes(child) for child in value)
    return 0


def _peak_rss_bytes() -> int:
    peak = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
    # Linux reports KiB; macOS and the BSDs report bytes.
    return int(peak * 1024 if sys.platform.startswith("linux") else peak)


def benchmark_worker(
    scenario: Scenario, path_count: int, iterations: int, warmups: int
) -> dict[str, Any]:
    data = scenario_data(scenario.corpus, path_count)
    for _ in range(warmups):
        result = _run_parser(scenario, data)
        del result
    gc.collect()

    wall_samples: list[int] = []
    cpu_samples: list[int] = []
    checksum = 0
    for _ in range(iterations):
        wall_start = time.perf_counter_ns()
        cpu_start = time.process_time_ns()
        result = _run_parser(scenario, data)
        cpu_samples.append(time.process_time_ns() - cpu_start)
        wall_samples.append(time.perf_counter_ns() - wall_start)
        checksum ^= len(result)
        del result
    gc.collect()

    tracemalloc.start()
    traced_result = _run_parser(scenario, data)
    traced_current, traced_peak = tracemalloc.get_traced_memory()
    snapshot = tracemalloc.take_snapshot()
    tracemalloc.stop()

    retained_allocations = sum(stat.count for stat in snapshot.statistics("filename"))
    result = {
        "name": scenario.name,
        "corpus": scenario.corpus,
        "expected": scenario.expected,
        "include_blocks": scenario.include_blocks,
        "include_raw_block_bytes": scenario.include_raw_block_bytes,
        "input_bytes": len(data),
        "iterations": iterations,
        "warmups": warmups,
        "wall_ns_median": int(statistics.median(wall_samples)),
        "wall_ns_min": min(wall_samples),
        "wall_ns_p95": int(_percentile(wall_samples, 0.95)),
        "cpu_ns_median": int(statistics.median(cpu_samples)),
        "cpu_ns_min": min(cpu_samples),
        "cpu_ns_p95": int(_percentile(cpu_samples, 0.95)),
        "peak_rss_bytes": _peak_rss_bytes(),
        "traced_current_bytes": traced_current,
        "traced_peak_bytes": traced_peak,
        "traced_retained_allocations": retained_allocations,
        "projected_deep_bytes": _deep_size(traced_result),
        "projected_string_bytes": _string_bytes(traced_result),
        "checksum": checksum,
    }
    del traced_result
    return result


def _command_output(command: list[str]) -> str:
    try:
        return subprocess.run(  # noqa: S603 - callers provide fixed diagnostic commands.
            command,
            cwd=ROOT,
            check=True,
            capture_output=True,
            text=True,
        ).stdout.strip()
    except (OSError, subprocess.CalledProcessError) as error:
        return f"unavailable: {error}"


def benchmark_driver(args: argparse.Namespace) -> dict[str, Any]:
    import pyapplebom._native as native

    selected = [
        scenario for scenario in SCENARIOS if not args.scenario or scenario.name in args.scenario
    ]
    unknown = set(args.scenario) - {scenario.name for scenario in SCENARIOS}
    if unknown:
        raise ValueError(f"unknown scenarios: {', '.join(sorted(unknown))}")

    results = []
    for scenario in selected:
        command = [
            sys.executable,
            str(Path(__file__).resolve()),
            "--worker",
            scenario.name,
            "--paths",
            str(args.paths),
            "--iterations",
            str(args.iterations),
            "--warmups",
            str(args.warmups),
        ]
        completed = subprocess.run(  # noqa: S603 - executes this benchmark with validated options.
            command, cwd=ROOT, check=True, capture_output=True, text=True
        )
        results.append(json.loads(completed.stdout))

    native_path = Path(native.__file__).resolve()
    report = {
        "schema_version": 1,
        "label": args.label,
        "revision": _command_output(["git", "rev-parse", "HEAD"]),
        "dirty": bool(_command_output(["git", "status", "--short"])),
        "environment": {
            "platform": platform.platform(),
            "machine": platform.machine(),
            "processor": platform.processor(),
            "python": sys.version,
            "rustc": _command_output(["rustc", "--version", "--verbose"]),
            "cargo": _command_output(["cargo", "--version"]),
            "native_path": str(native_path),
            "native_bytes": os.path.getsize(native_path),
        },
        "path_count": args.paths,
        "results": results,
    }
    return report


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--label", default="local", help="label stored in the report")
    parser.add_argument("--output", type=Path, help="write JSON here instead of stdout")
    parser.add_argument("--paths", type=int, default=DEFAULT_PATH_COUNT)
    parser.add_argument("--iterations", type=int, default=DEFAULT_ITERATIONS)
    parser.add_argument("--warmups", type=int, default=DEFAULT_WARMUPS)
    parser.add_argument("--scenario", action="append", default=[], help="run only this scenario")
    parser.add_argument(
        "--worker", choices=[scenario.name for scenario in SCENARIOS], help=argparse.SUPPRESS
    )
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    if args.iterations <= 0 or args.warmups < 0:
        raise ValueError("iterations must be positive and warmups must be non-negative")

    if args.worker:
        scenario = next(item for item in SCENARIOS if item.name == args.worker)
        report = benchmark_worker(scenario, args.paths, args.iterations, args.warmups)
    else:
        report = benchmark_driver(args)

    rendered = json.dumps(report, indent=2, sort_keys=True) + "\n"
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(rendered, encoding="utf-8")
    else:
        sys.stdout.write(rendered)


if __name__ == "__main__":
    main()
