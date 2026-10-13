# Architecture

e edits one file at a time. The ownership chain is `main.rs` → `Editor` → `Document` → `GapBuffer`: startup and file safety live in `main.rs`; `Editor` owns the event loop and interaction state; `Document` owns text plus undo and dirty state; `GapBuffer` stores the bytes and line metadata.

## Source map

| Topic | Source |
|---|---|
| CLI, startup, and piped input | `src/main.rs` |
| Editor state and command dispatch | `src/editor.rs` and `src/editor/` |
| Terminal input and restoration | `src/input.rs`, `src/terminal.rs`, `src/signal.rs` |
| File I/O and persistent state | `src/file_io.rs`, `src/editor/file_session.rs` |
| Text storage and document edits | `src/buffer.rs`, `src/document.rs`, `src/operation.rs` |
| Carets and screen layout | `src/selection.rs`, `src/viewport.rs` |
| Rendering and search | `src/render.rs`, `src/find.rs` |
| Unit tests | `src/editor/tests.rs` and module-local test modules |
| PTY E2E tests | `tests/e2e/` |
| Benchmarks | `benches/bench.rs` |

The syntax highlighter is the `laputa-hi-lite` dependency. Cargo resolves it from crates.io; see [build and release](build-and-release.md) for local dependency work.
