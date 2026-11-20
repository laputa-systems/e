Prefix: DSLP
Next: 11

Deslop campaign backlog ([plan](https://github.com/laputa-systems/stuff/blob/main/docs/campaigns/deslop.md), tier B). One agent works this file with the `deslop` skill: take the lowest-numbered open ticket whose area does not start with `Needs owner ·`. After DSLP-5, work the open P1 and P2 findings, then fill in `## Report` below.

## Report

DSLP-7 closed: atomic replacement copies each listed extended attribute before
rename. A failed list, read, or set does not prevent the save; the save status
warns about attributes that could not be preserved, or an unavailable list.
Hard-linked files retain their existing in-place save behavior, and symlinks
continue to be followed.

Bench audit (2026-11-19, DSLP-10): the harness measures wall time and
allocator counters within the benchmark process, with no checked performance
gate. Ranked P3 because no regression is demonstrated: (1) no PTY startup or
end-to-end input-to-screen journey, correctness oracle or completed-work check;
(2) no matched interleaved reference, host gate or valid budget, and the old
macOS baseline contains names absent from the current harness; (3) no complete
CPU, memory and I/O attribution or pinned representative fixture corpus.
Sampling the process with Instruments/perf, heap profiling and separate
strace/dtruss runs are the cheapest attribution starting points; Linux marker
support exists, but instrumented timings are not comparable to normal runs.
The allocator measures in-process allocations, not whole-process RSS or child
processes. Rustybench reports samples, but this driver neither filters host load
nor claims a percentile or regression verdict. The baseline records three
measured rounds after one warmup, without an interleaved candidate.

Coverage map: file read/write measure component I/O, open-and-render measures
loading, buffer creation and highlighting of Rust or generated 1/10 MiB Python;
interactive typing/newline measure edits and cached rendering; multicursor,
paste and batch undo exercise buffer/document edits; scroll, full frames and
cursor move exercise viewport rendering and syntax/selection; forward and regex
search, find trace and replace-all exercise search; command palette trace
exercises goto and rendering. These use embedded source or synthetic Python,
black-box outputs and unwrap errors, not output correctness assertions. They do
not measure process startup, terminal transport, responsiveness distribution
or filesystem cold-cache behavior. The 10 MiB case is Scenario, remaining
workflows Change; Edit tests the driver. Full is explicit and never run here.
Preflight cannot prove registry availability or absence of unrelated processes;
Cargo handles stale builds. Each run records exclusions and a no-verdict warning.


Dead code (DSLP-5): on macOS ARM64, rustc 1.102.0-nightly checked
all targets and features with `-W dead_code -W unreachable_pub`; the same
check cross-compiled for aarch64 Linux musl. The base had 50 unreachable
public items in the private PTY harness. Its remaining exports now have
crate visibility and are used by `tests/e2e` scenarios. Removing the harness
suppression exposed an unused `Key::CtrlShiftLeft` encoding and
`TestEditor::quit_saving`; neither had a caller in source, tests, benchmarks,
docs, build or package references. They were removed from the test adapter,
not the editor: input parsing and editor dispatch still handle that key.
`make test` exercises the consumer suite. No production behavior, CLI,
format, dependency or hi-lite integration changed.

The two buffer dead-code allowances were unnecessary: public `len` and
`is_empty` are exercised by buffer unit tests, including the length property.
The remaining test-only adapters have callers: `from_text` in buffer and
render tests, `status_left` in editor tests, clipboard fixtures in clipboard
and editor tests, and the sudo override in the file-session editor test.
Other module-local helpers are checked by rustc with test targets enabled.
The `signal` module's `allow(unused)` is retained; editor startup and terminal
cleanup call it. No dead-code allowlist remains.

cargo-machete 0.10.1 reports `laputa-hi-lite` as unused because its library
is imported as `hi_lite`: renderer, document, editor and benchmark use it.
It is retained. Debloat review retained the document/undo and line-index
layers (editing invariants), terminal and PTY adapters (lifecycle and fixture
isolation), syntax dependency, and benchmark tracing (Linux workload). The
cut above is test-only; no additional structural cut was established. Linux
was cross-checked, not executed; external library consumers were not surveyed.

Ground at `4bc0854`, 2026-11-19. Both hosted default jobs ran
`make test CI_TEST_MODE=hosted` and passed all 874 cases, including
163 PTY cases. The hosted all-feature Clippy and the fleet
`gha-test-workflow` rule passed.
[Hosted default](https://github.com/laputa-systems/e/actions/runs/44477817612)

On macOS ARM64, the empty-target run took 429.80 s and the warm
repeat 8.58 s with the deep absolute worktree scratch path.
Each reproduced the same 14 PTY failures. With the shorter
absolute campaign `.work/tmp` as TMPDIR, the warm command passed
all 874 cases in 7.68 s. The same binary's focused filename case
fails with the deep path and passes with the short path.
`make lint` passed in 249.72 s. These are observations recorded
under shared host load. DSLP-6 tracks the user-visible clipping.

The benchmark workload covers realistic editor operations.
The separate release, nested tmux, and pinned LLVM qualification
remain in their owning workflows. Fleet musl lint reports two
existing LLVM pin findings.

Diagnose reviewed save and persistence, undo, terminal lifecycle, input,
startup, search, rendering and their boundary tests; DSLP-7 and DSLP-8
record reproduced defects, and DSLP-9 records a test fixture failure. Linux,
fatal-signal recovery, privileged saves, crash windows and non-UTF-8
filenames were not exercised in this pass.

Types (DSLP-3): measured on macOS ARM64 with rustc 1.102.0-nightly
and Clippy 0.1.101, all targets and features, 23,779 first-party Rust
lines. Counts below are raw diagnostics / unique message and location;
duplicates include separate target builds. Each candidate was run alone
with `cargo clippy --locked --all-targets --all-features -- -W <lint>`.

- Adopted `unwrap_used` only in production `find` code: 396 / 386
  repository-wide; seven unique production library locations, including two
  in search and five buffer invariant assertions. The scoped lint rejects
  the two former search unwraps; retaining the compiled regex removes them.
  Its base negative witness fails on both lines. Existing search tests cover
  matches, indexing and invalid UTF-8. The blanket lint is rejected: its five
  buffer false positives exceed one per 5,000 lines; test and benchmark
  unwraps are also intentional.
- Rejected `unsafe_code` forbid: 16 / 7; terminal, signal and rendering
  operations require or currently justify unsafe, with no proven defect.
- Rejected `wildcard_enum_match_arm`: 47 / 37; intentional fallback arms
  include partial command tests, input mappings and modal key handling.
- Rejected `match_wildcard_for_single_variants`: 2 / 1; guarded mouse
  alternatives leave modifier combinations for the wildcard, so it is not a
  single-variant defect.
- Rejected `cast_possible_truncation`: 57 / 33; many conversions are bounded
  or in tests. The persisted 32-bit cursor and undo coordinates deserve a
  large-file reproduction; their reachable impact is unverified.
- Rejected `cast_sign_loss`: 17 / 12; bounded edit offsets and nonnegative
  test generators account for findings, with no demonstrated defect.
- Rejected `expect_used`: 24 / 22; two production renderer assertions follow
  the language/highlighter setter invariant, and the rest are PTY harness
  assertions. No trust was deleted by adopting it.
- Rejected `redundant_clone`: 4 / 2; clone suggestions do not establish a
  type defect or delete an escape hatch.
- Rejected `needless_pass_by_value`: 9 / 6; ownership and callback interfaces
  explain the reported cases, with no type defect or deleted escape hatch.

The prescribed escape-hatch pattern counted 426 occurrences before and 424
after across `src`, `tests`, and `benches`. The search change preserves the
regex result and existing viewport refresh; no user-visible contract or file
format changed. The 32-bit persisted coordinate lead above remains unverified.

Tests (DSLP-4, in progress): the contract map and tier costs are in AGENTS.md.
An open reader now guards single-link atomic replacement; the whitespace save
case waits for bytes different from its original fixture, and argument errors
assert exit 1 and stderr. In-place overwrite, skipped whitespace stripping,
and a success exit on bad arguments each made the respective test fail.
On macOS ARM64, cargo-mutants 27.1.0 selected only the `meta.nlink() == 1`
comparison in `write_file` (`--file src/file_io.rs --re
'^src/file_io.rs:85:21:' --baseline run --gitignore true --timeout 30
--build-timeout 210 --jobs 1 --jobserver-tasks 2 -- --lib`). One mutant was
caught, none missed, timed out, or unviable; the existing hard-link and new
open-reader assertions both failed. The trial used 291.71 s wall, one runner;
the killed mutant is evidence, not an additional behavior. The same mutant on
base 8523438 was already caught by the hard-link assertion, with 206.71 s wall
on one runner; total trial cost was 498.42 s, with no survivors or exclusions.
The open-reader fault independently demonstrates the newly protected behavior.
The known split-input defect remains DSLP-8. Fatal signal
recovery and crash/power-loss durability were not exercised.
`make coverage` exited 2 with the same 13 deep-path PTY failures as the base;
the subsequent partial report showed 83.79% lines and 63.44% branches, but
terminal.rs reported 0%, so subprocess coverage is not established. The test
time gate is deferred: `perf-gate-ready` reported a busy host. No retained mutation
lane is added: one is required only where the tools are mirrored, and
cargo-mutants is not on the crate mirror. DSLP-4 closes on the contract map,
the three fault-proven boundary tests and the bounded single-link trial.
The PTY test `find_live_search_highlights` asserts the prompt text rather than
the highlight; it is retained for that assertion. `test_render_with_find_matches`
checks the highlight rendering, while a PTY highlight assertion remains absent.

## Tickets









### DSLP-8 · P2 · A split arrow-key sequence inserts its suffix into the file

- **Area:** terminal input
- **Where:** src/editor.rs:273, src/editor.rs:284, src/input.rs:161, src/input.rs:192
- **Problem:** A PTY session opened abc with a final newline, sent ESC separately, waited 300 ms, then sent [C followed by X and save. The editor saved [CXabc instead of moving right and saving aXbc. The input thread flushes ESC at the end of each read burst, so a terminal or remote link that fragments a key sequence turns its suffix into document text.
- **Recommendation:** Disambiguate a standalone Escape using a bounded timeout that permits fragmented terminal sequences, and test split keys and standalone Escape through a PTY.
- **Fix with:** root-cause
- **Effort:** M


### DSLP-9 · P3 · The save symlink test fails when TMPDIR is relative

- **Area:** test isolation
- **Where:** src/file_io.rs:1155, src/file_io.rs:1161, src/file_io.rs:1589
- **Problem:** With TMPDIR=.work/tmp, test_write_keeps_mode_and_follows_symlink fails with ENOENT: its symlink target includes the relative temporary directory and is interpreted relative to the link's parent. This prevents a full test run with the relative scratch path used to avoid the long-path status clipping recorded in DSLP-6.
- **Recommendation:** Build the symlink fixture from an absolute target or a target relative to the link's parent, and check that the fixture points to the file before exercising save.
- **Fix with:** test-audit
- **Effort:** S


### DSLP-10 · P3 · Establish representative editor performance and attribution evidence

- **Area:** benchmarks
- **Where:** benches/bench.rs, benches/baseline.json, docs/benchmarks.md
- **Problem:** Component workflows lack end-to-end startup and terminal latency, correctness and completed-work oracles, representative fixture diversity, host-gated interleaved comparisons, and process-wide CPU, memory and I/O attribution. The checked-in baseline names differ from the present harness and no gate compares it. No performance regression is demonstrated.
- **Recommendation:** Add a PTY startup and edit journey with output checks; establish matched interleaved baselines and a host gate, and document separate sampling, allocation and syscall profiling for attribution.
- **Fix with:** bench-pyramid
- **Effort:** L

