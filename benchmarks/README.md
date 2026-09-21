# Performance benchmarks

`benchmark.py` generates deterministic BOM containers and measures the complete public Python
projection path in a fresh process per scenario. The corpus covers a normal path tree, every
optional path section, typed block and raw-hex projection, a fatal hostile block count, and two
malformed optional sections which must return safe partial results.

Build the extension in release mode and run the suite with the pinned primary toolchain:

```bash
maturin develop --release --locked
python benchmarks/benchmark.py --label local --output /tmp/pyapplebom-benchmark.json
```

The report includes median/min/p95 wall and CPU time, fresh-process peak RSS, Python-traced peak and
retained allocations, projected deep size, projected string bytes (the copy-sensitive payload),
native-library size, exact tool versions, and the Git revision. `tracemalloc` is run separately from
the timing loop so its instrumentation does not distort latency. RSS includes the interpreter and
loaded extension, while traced allocations cover Python-managed projection objects rather than the
Rust allocator; compare results on the same host and toolchain.

The default 4,000-path corpus fits the BOM format's 16-bit per-block count and is large enough to
expose repeated traversal and path-materialization costs. Use `--paths`, `--iterations`, and
`--warmups` to change the workload. Repeat `--scenario NAME` to select individual scenarios.
