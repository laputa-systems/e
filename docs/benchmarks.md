# Benchmarks

`make bench` runs Edit by default. `make bench-check` checks prerequisites;
`make bench-list` lists every name, tier, area and command. Pass options through
`BENCH_ARGS`, for example:

```
make bench BENCH_ARGS='--tier Change --area render --skip render_full_frame_plain --dry-run'
make bench-check BENCH_ARGS='--tier Change --name render_full_frame_rust_syntax'
make bench BENCH_ARGS='--tier Change --name render_full_frame_rust_syntax'
```

Names, areas and skips are exact and repeatable; selections intersect and the
tier is a ceiling. Unknown or empty selections fail. Edit runs the driver
self-test (estimated seconds). Change runs individual rustybench workflows
(estimated minutes including compilation). Scenario adds the 10 MiB case
(estimated minutes); it is a single harness run, not an interleaved comparison.
Full is manual-only: explicitly use `--tier Full --name all` for the unfiltered
suite. `make bench-all` preserves the original `cargo bench --bench bench` path
and is also manual-only. Do not use Full as a validation step.

Python 3, the pinned Rust toolchain, Cargo dependencies, source fixtures and
512 MiB free disk are required for measured workloads. Cargo rebuilds artifacts
when sources change; its normal dependency resolution may need registry access.
Fixtures are embedded checkout source or generated Python, not external data.
The preflight checks the selected tools, files and disk; it does not test registry
availability or identify unrelated running processes. Host load is informational.
Dry runs do not create output. Runs write logs and a summary to a dated directory
under `.work/bench/`, including exclusions, revision, dirty state and exit codes.
The timing has no host gate or comparison budget: completion is not a performance
verdict. Rustybench measures wall time and allocator activity in its process;
results do not measure an editor process tree or peak RSS. `SYSCALL_TRACE=1`
enables Linux prctl markers for separate syscall tracing; it is instrumented and
should not be compared to an uninstrumented timing run.

`benches/baseline.json` is an older macOS arm64 baseline; its names differ from
current workloads and no target or CI gate compares it. See DSLP-10 for the
missing journey, comparison and attribution evidence.
