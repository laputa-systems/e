`e` is a single-file terminal editor in Rust for macOS and Linux.

Build with `make build`; run unit and E2E tests with `cargo test --locked` (`cargo test --test e2e` runs E2E only); check with `cargo clippy --locked --all-targets --all-features -- --deny warnings`.

The roadmap, [docs/roadmap.md](docs/roadmap.md), holds all current and future work; its Status is the resume point.

## Invariants

- `GapBuffer` keeps `line_starts` and `line_ascii` valid and parallel; byte offsets are internal and `TextPosition::column` is a character index.
- All text mutations go through `Document`; batch edits use sorted half-open byte ranges in reverse order and form one undo group.
- `CaretSet` is never empty and its `primary` index always names a caret.
- `Viewport` remains safe for empty, narrow, and huge files; resizing preserves a logical content anchor.
- `Renderer` invalidates syntax from buffer version and dirty-line state, compares cached rows, and reuses its scratch storage.
- Find is single-line; invalid UTF-8 lines do not match, and replace-all refuses ranges containing invalid UTF-8.
- Saving strips trailing ASCII spaces and tabs, ensures a final newline, and refuses a file whose modification time changed since load.
- Persistent state requires `HOME` and uses owner-only directories and files.

## Verification

`make clean` removes the checkout's ordinary `./target` directory. It refuses a file or symlink at that path.

- Edit: `cargo test --locked <test-name>` selects a focused case; cost varies.
  Change: `cargo test --locked --test e2e <test-name>` selects a PTY scenario.
  Full: `make test CI_TEST_MODE=hosted` runs locked unit and PTY tests; a warm
  short-path run took 7.68 s. Deep absolute TMPDIR, one test thread: base and
  candidate PTY runs took 51.14 s and 50.22 s, each with 13 DSLP-6 failures.
  `make test-ci` selects the release binary through `E_TEST_BINARY`.
- `make lint` took 2m 01s under load; hosted Clippy also enables all features.
  Two single-mutant trials took 498.42 s combined on one runner.
- Save: `file_io::tests::test_write_replaces_single_link_without_changing_open_reader`,
  `test_write_keeps_mode_and_follows_symlink`, `test_write_hard_linked_file_stays_linked`,
  `tests/e2e/file_fidelity.rs`, `save_quit.rs`, and `external_changes.rs` cover
  replacement, links, mode, bytes, cleanup, and conflicts. Xattrs: DSLP-7.
- Undo: `tests/e2e/undo.rs`, `persistent_undo.rs`, and
  `document::tests::test_apply_batch_records_one_group_and_undoes_all_edits`.
  Terminal restoration: `tests/e2e/terminal_lifecycle.rs` covers exit and four
  signals; fatal signals remain unverified.
- Input: `input::tests::test_utf8_ground_decodes_to_one_char`, `test_arrows`,
  and `test_paste`; split escape sequences: DSLP-8. Search: `find::tests`,
  `tests/e2e/find_replace.rs`, and `test_replace_all_refuses_invalid_utf8`.
- CLI: `tests/help.rs` covers help and argument exit codes;
  `tests/e2e/startup_inputs.rs` covers stdin and invalid file exit paths.
- `make bench` runs editor workflows; see `benches/baseline.json` and
  [benchmarks](docs/benchmarks.md). Nested tmux and SSH: [testing](docs/testing.md).

## Topic map

- Ownership and source layout: [architecture](docs/architecture.md)
- Buffer, document, edits, undo, selections, and carets: [text model](docs/text-model.md)
- Viewport and rendering: [rendering](docs/rendering.md)
- Find and replace: [search](docs/search.md)
- Terminal lifecycle, input, and editor loop: [runtime](docs/runtime.md)
- File safety, saving, locks, and persisted state: [files](docs/files.md)
- Build, local dependency work, and release: [build and release](docs/build-and-release.md)
- Unit and PTY E2E workflow: [testing](docs/testing.md)
- Benchmarks: [benchmarks](docs/benchmarks.md)
