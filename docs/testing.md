# Testing

Run all unit and integration tests with `cargo test --locked`. Run only the PTY E2E suite with `cargo test --test e2e`. `make test-ci` selects the existing release binary through `E_TEST_BINARY` and runs tests against it.

## PTY harness

`tests/e2e/harness.rs` is an editor-specific adapter over `laputa-ptytest`. It owns editor keys, fixtures, recordings, and cell/style assertions. The shared crate owns PTY spawn, nonblocking I/O, terminal parsing, and cleanup.

The harness uses a hermetic environment: ASCII on Linux and `C.UTF-8` on macOS. It selects the `xterm-minimal-v1` protocol profile and captures initial terminal state. Every observed normal editor exit must restore the applicable terminal modes. Forced cleanup in `Drop` is a safety net, not a restoration check.

Use screen or state barriers to wait for redraws rather than adding delays. The shared crate writes failure bundles under `target/ptytest-failures/`.

## Nested tmux over SSH

Run `tests/repro/sigwinch-ssh/run.sh` to resize `e` through local tmux, loopback SSH, and nested tmux. The harness loads the vendored [tmux config](../tests/repro/sigwinch-ssh/tmux.conf) in both tmux servers and starts a temporary key-only SSH server on localhost.

The script builds the debug binary unless you set `E_TEST_BINARY`. It needs `tmux`, `ssh`, `ssh-keygen`, `sshd`, and `python3`. Set `SIGWINCH_KEEP_TMP=1` to retain generated keys and logs under `.work/tmp`.
