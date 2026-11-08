# Build and release

The Rust toolchain is pinned in `rust-toolchain.toml`. `make build` runs a development build. `make release` builds a stripped release for `TARGET`; the default is the host target, with Linux defaulting to a statically linked musl binary. `make install` copies the release binary to `~/.local/bin/e` and ad-hoc signs it on macOS.

`make lint` runs Clippy across the workspace without modifying source files (`cargo clippy --workspace --all-targets -- --deny warnings`). For a locked check across all features, run `cargo clippy --locked --all-targets --all-features -- --deny warnings`.

## hi-lite dependency

Cargo resolves dependencies including `laputa-hi-lite = "0.2"` through the `stuff` registry replacing crates.io in `.cargo/config.toml`. Changes in the adjacent [hi-lite repository](https://github.com/laputa-systems/hi-lite) do not affect this app until a compatible version is published and the dependency is updated. For local work, apply a Cargo `[patch.crates-io]` override pointing `laputa-hi-lite` at the checkout; its package version must satisfy `Cargo.toml`.

## Release workflow

The manual GitHub Actions release workflow builds macOS arm64 and Linux x86_64/aarch64 binaries, tests them, and publishes a pre-release. Before packaging, it rejects Linux binaries with an ELF interpreter or shared-library dependency. `Dockerfile` supplies the Linux musl build environment. The CI workflow runs Clippy and tests on pushes to `main` and on pull requests.
