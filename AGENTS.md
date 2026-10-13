`e` is a single-file terminal editor in Rust for macOS and Linux.

Build with `make build`; run unit and E2E tests with `cargo test --locked` (`cargo test --test e2e` runs E2E only); check with `cargo clippy --locked --all-targets --all-features -- --deny warnings`.

## Invariants

- `GapBuffer` keeps `line_starts` and `line_ascii` valid and parallel; byte offsets are internal and `TextPosition::column` is a character index.
- All text mutations go through `Document`; batch edits use sorted half-open byte ranges in reverse order and form one undo group.
- `CaretSet` is never empty and its `primary` index always names a caret.
- `Viewport` remains safe for empty, narrow, and huge files; resizing preserves a logical content anchor.
- `Renderer` invalidates syntax from buffer version and dirty-line state, compares cached rows, and reuses its scratch storage.
- Find is single-line; invalid UTF-8 lines do not match, and replace-all refuses ranges containing invalid UTF-8.
- Saving strips trailing ASCII spaces and tabs, ensures a final newline, and refuses a file whose modification time changed since load.
- Persistent state requires `HOME` and uses owner-only directories and files.

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
