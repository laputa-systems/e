# e

my text editor

shares quite a bit of DNA with `micro` but has been radically pared down to intuitive essentials

you probably don't want to use this

## Build and test

- `make build` / `make test` — dev build and the unit plus PTY e2e suites (`cargo test --test e2e` for e2e only).
- `make release` — pinned-nightly static release build; `make install` copies it to `~/usr/bin`.
- `cargo bench --bench bench` — benchmarks.
- `.github/workflows/release.yml` is manual (`workflow_dispatch`) and builds, tests, and pre-releases macOS arm64 and Linux musl binaries. Nothing runs on push or pull request.

## Files

- `~/.config/e/keybindings.ini` — Ctrl-key action overrides.
- `~/.config/e/undo.bin`, `cursor.bin` — persistent undo history and last cursor position per file.
- `~/.config/e/locks/` — one lock per open file.

Syntax highlighting comes from the sibling `hi-lite` crate; see `AGENTS.md` for the architecture and invariants.
