Prefix: DSLP
Next: 7

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

## Tickets


### DSLP-2 · P2 · Diagnose: ultrareview into this backlog

- **Area:** deslop · diagnose
- **Where:** whole repository
- **Problem:** No review has ranked this repository's risks for the deslop pass.
- **Recommendation:** Run the deslop Diagnose step. Accept: the diff touches only docs, comments, and this file, and every finding names its Fix with.
- **Fix with:** deslop
- **Effort:** S

### DSLP-3 · P2 · Types: strict profile and typing findings

- **Area:** deslop · types
- **Where:** type checker and lint configuration; Rust sources
- **Problem:** The strict-profile candidates in the deslop skill's tools reference have not been measured here.
- **Recommendation:** Run the deslop Types step. Accept: clean at the adopted profile, escape hatches not increased, witnesses recorded.
- **Fix with:** deslop
- **Effort:** M

### DSLP-4 · P2 · Tests: coverage, boundary gaps, mutation

- **Area:** deslop · tests
- **Where:** test suites and CI
- **Problem:** Coverage and mutation results are unmeasured; no deslop mutation lane exists.
- **Recommendation:** Run the deslop Tests step. Accept: no undispositioned survivor in critical code, the test cost budget holds, and the lane is green where the tools are mirrored.
- **Fix with:** deslop
- **Effort:** M

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

