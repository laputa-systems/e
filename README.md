# e

my text editor

shares quite a bit of DNA with `micro` but has been radically pared down to intuitive essentials

you probably don't want to use this

## Build and test

- `make build` / `make test` — dev build and the unit plus PTY e2e suites (`cargo test --test e2e` for e2e only).
- `make release` — pinned-nightly static release build; `make install` copies it to `~/usr/bin`.
- `make lint` — clippy with `--deny warnings`; it does not modify files.
- `make bench` — benchmarks.
- `.github/workflows/ci.yml` runs clippy and `cargo test` on push to `main` and on pull requests.
- `.github/workflows/release.yml` is manual (`workflow_dispatch`) and builds, tests, and pre-releases macOS arm64 and Linux musl binaries.
- Release builds use the published `laputa-hi-lite` from crates.io. To try a sibling checkout locally without touching `Cargo.toml`, pass `--config 'patch.crates-io.laputa-hi-lite.path="../hi-lite"'` to cargo; the sibling version must satisfy the `Cargo.toml` requirement (bump it first if the sibling is ahead).
- The XSH vocabulary generator and its freshness test live in `hi-lite` (`make gen-xsh`).

## Files

- `~/.config/e/keybindings.ini` — Ctrl-key action overrides.
- `~/.config/e/undo.bin`, `cursor.bin` — persistent undo history and last cursor position per file.
- `~/.config/e/locks/` — one lock per open file, held with `flock` so a crashed editor's lock is never stale.

State files are readable only by you, and `HOME` must be set.

Syntax highlighting comes from the sibling `hi-lite` crate; see `AGENTS.md` for the architecture and invariants.
