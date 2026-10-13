# Build and release

The Rust toolchain is pinned in `rust-toolchain.toml`. `make build` runs a development build. `make release` builds a stripped static release for `TARGET`; the default is the host target, with Linux defaulting to musl. `make release-dynamic` builds the dynamic Linux variant. `make install` copies the release binary to `~/usr/bin/e` and ad-hoc signs it on macOS.

The `make lint` target passes `--fix` to Clippy and can modify source files. For a read-only check, run `cargo clippy --locked --all-targets --all-features -- --deny warnings`.

## hi-lite dependency

Cargo resolves `laputa-hi-lite = "0.2"` from crates.io. Changes in the adjacent [hi-lite repository](https://github.com/laputa-systems/hi-lite) do not affect this app until a compatible version is published and the dependency is updated. For local work, apply a Cargo `[patch.crates-io]` override pointing `laputa-hi-lite` at the checkout; its package version must satisfy `Cargo.toml`. The XSH vocabulary generator and freshness test are also owned by hi-lite.

## Release workflow

The manual GitHub Actions release workflow builds macOS arm64 and Linux x86_64/aarch64 binaries, tests them, and publishes a pre-release. Linux produces dynamic and static binaries. `Dockerfile` supplies the Linux musl build environment. The CI workflow runs Clippy and tests on pushes to `main` and on pull requests.
