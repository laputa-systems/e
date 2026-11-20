Prefix: EDIT
Next: 12
### EDIT-9 · P3 · The save symlink test fails when TMPDIR is relative

- **Area:** test isolation
- **Where:** src/file_io.rs:1155, src/file_io.rs:1161, src/file_io.rs:1589
- **Problem:** With TMPDIR=.work/tmp, test_write_keeps_mode_and_follows_symlink fails with ENOENT: its symlink target includes the relative temporary directory and is interpreted relative to the link's parent. This prevents a full test run with the relative scratch path used to avoid the long-path status clipping recorded in DSLP-6.
- **Recommendation:** Build the symlink fixture from an absolute target or a target relative to the link's parent, and check that the fixture points to the file before exercising save.
- **Fix with:** test-audit
- **Effort:** S

### EDIT-10 · P3 · Establish representative editor performance and attribution evidence

- **Area:** benchmarks
- **Where:** benches/bench.rs, benches/baseline.json, docs/benchmarks.md
- **Problem:** Component workflows lack end-to-end startup and terminal latency, correctness and completed-work oracles, representative fixture diversity, host-gated interleaved comparisons, and process-wide CPU, memory and I/O attribution. The checked-in baseline names differ from the present harness and no gate compares it. No performance regression is demonstrated.
- **Recommendation:** Add a PTY startup and edit journey with output checks; establish matched interleaved baselines and a host gate, and document separate sampling, allocation and syscall profiling for attribution.
- **Fix with:** bench-pyramid
- **Effort:** L

### EDIT-11 · P3 · The split-escape PTY test is timing-sensitive on hosted macOS

- **Area:** tests
- **Where:** tests/e2e/movement.rs:4 (split_escape_sequence_moves_right_before_typing)
- **Problem:** The DSLP-8 regression writes ESC, sleeps 20 ms, then writes `[CX`. On hosted macOS CI (run 44649416152, PR #39) it saved `[CXabc` instead of `aXbc`: the editor flushed ESC on its 100 ms timeout, so the runner delayed the second write past the window. Locally and on #38's CI it passed. The test asserts a wall-clock race, so a loaded runner can fail it without a product defect.
- **Recommendation:** Make the fragment boundary deterministic without a wall-clock race, for example by waiting until the editor has consumed the lone ESC before the second write, or by exercising the reader with a controllable timeout below the PTY layer; keep a test that fails on the pre-DSLP-8 read-boundary flush.
- **Fix with:** deslop
- **Effort:** S

