# Testing

Run all unit and integration tests with `cargo test --locked`. Run only the PTY E2E suite with `cargo test --test e2e`. `make test-ci` selects the existing release binary through `E_TEST_BINARY` and runs tests against it.

## PTY harness

`tests/e2e/harness.rs` is an editor-specific adapter over `laputa-ptytest`. It owns editor keys, fixtures, recordings, and cell/style assertions. The shared crate owns PTY spawn, nonblocking I/O, terminal parsing, and cleanup.

The harness uses a hermetic environment: ASCII on Linux and `C.UTF-8` on macOS. It selects the `xterm-minimal-v1` protocol profile and captures initial terminal state. Every observed normal editor exit must restore the applicable terminal modes. Forced cleanup in `Drop` is a safety net, not a restoration check.

Use screen or state barriers to wait for redraws rather than adding delays. The shared crate writes failure bundles under `target/ptytest-failures/`.
