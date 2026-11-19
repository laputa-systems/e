Prefix: DSLP
Next: 10

Deslop campaign backlog ([plan](https://github.com/laputa-systems/stuff/blob/main/docs/campaigns/deslop.md), tier B). One agent works this file with the `deslop` skill: take the lowest-numbered open ticket whose area does not start with `Needs owner ·`. After DSLP-5, work the open P1 and P2 findings, then fill in `## Report` below.

## Report

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
The known xattr and split-input defects remain DSLP-7 and DSLP-8. Fatal signal
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





### DSLP-5 · P2 · Dead code: tools and debloat

- **Area:** deslop · dead code
- **Where:** whole repository
- **Problem:** No dead-code scan has run.
- **Recommendation:** Run the deslop Dead code step. Accept: tools clean or allowlisted with reasons; walk-backs filed as Needs owner tickets.
- **Fix with:** deslop
- **Effort:** M


### DSLP-6 · P2 · Long paths hide the filename and edit state in the status bar

- **Area:** terminal rendering
- **Where:** src/editor.rs:661, src/render.rs:893, tests/e2e/startup.rs:37
- **Problem:** At 80 columns the renderer consumes the available width with the front of the full path. A deep scratch checkout reproduces the missing filename, language and dirty marker in 14 ordinary PTY cases. At revision 4bc0854 the same filename case fails with absolute TMPDIR="$PWD/.work/tmp" and passes with TMPDIR=.work/tmp.
- **Recommendation:** Preserve the basename and edit state within the available terminal columns. Verify the rendered row and resize behavior with long path, dirty and named-language fixtures.
- **Fix with:** root-cause
- **Effort:** S


### DSLP-7 · P2 · Saving an existing file drops its extended attributes

- **Area:** file saving
- **Where:** src/file_io.rs:136, src/file_io.rs:151, src/file_io.rs:158, tests/e2e/file_fidelity.rs:4
- **Problem:** On macOS, a PTY session opened a single-link file with com.example.e-diagnose=important metadata, inserted X and saved successfully. The file contained Xoriginal followed by a newline, but xattr no longer listed the attribute. The atomic replacement copies owner and mode but replaces the inode without copying extended attributes; user metadata is silently lost on an ordinary save.
- **Recommendation:** Preserve relevant extended attributes and other supported metadata when replacing a file, and add a filesystem-backed editor save regression covering their contents and failure behavior.
- **Fix with:** os-boundaries
- **Effort:** M


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

