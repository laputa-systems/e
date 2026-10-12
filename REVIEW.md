# Review: e

Scope: the `e` crate (src, tests, benches, Makefile, Dockerfile, release workflow, docs); `../hi-lite` read only for the dependency boundary. Checks run: `cargo check --all-targets` passes. Lane repros (scratch crate under `.work/tmp`, not in the repo) proved R1, R2, R6 and R7 below; the repo's own test suite and e2e suite were not run. Not covered: `../hi-lite` internals, clipboard behaviour on Linux, the `unsafe` in `signal.rs` beyond a read (it looked correct), and multi-caret unicode edit commands in `editor.rs` beyond sampling.

## Summary

The core text model is carefully built and fast, but the save and persistence paths are the weak side: saving rewrites file bytes, persisted undo history can disagree with the buffer, and nothing restores the terminal or lock when the process dies abnormally, which `panic = "abort"` makes more likely. Most other findings share one root: there is no safety net, since CI runs only inside a manual release workflow, the benchmark gate is not wired, and the test suite is mostly unit tests coupled to `Editor` internals. Cleanup of `editor.rs` (7.6k lines, boolean-flag modes) would lower the cost of fixing the rest.

## Findings

### R1 · P1 · Save rewrites file contents: invalid UTF-8, CRLF and NBSP are changed

- **Area:** Data
- **Where:** `src/file_io.rs:15-42`, `src/editor.rs:3586`
- **Problem:** `write_file` decodes with `String::from_utf8_lossy`, then `trim_end` on each line. Repro: `write_file(b"a\xff\xfeb\r\nline2\xc2\xa0\r\n")` wrote `a`, two U+FFFD, `b`, `\n`, `line2`, `\n`. Invalid bytes become U+FFFD, every CRLF becomes LF, and trailing NBSP is stripped. This contradicts the documented invariant that invalid UTF-8 is preserved, and the binary-file prompt invites exactly these files. Anyone editing a CRLF, Latin-1 or binary file loses data on every save, silently.
- **Recommendation:** Strip trailing ASCII space and tab at byte level, keep `\r` before `\n`, never decode. Add a round-trip test with `\xff` and CRLF first.
- **Fix with:** durable-state
- **Effort:** S

### R2 · P1 · Persisted undo history can disagree with the file and panic on the next open

- **Area:** Data
- **Where:** `src/editor.rs:776-779`, `src/editor.rs:3586-3591`, `src/file_io.rs:397-458`, `src/file_io.rs:508-520`, `src/buffer.rs:162`
- **Problem:** Two paths store undo history that does not match the file: a save writes `clean_for_write(buffer)` but stores byte offsets of the uncleaned buffer, and quit-then-`n` stores history containing discarded edits. The entry is validated on load only by mtime, which `cp -p`, rsync and git can reproduce. Repro: file `abc   \nxyz`, insert `Q` at the end, save, store history, reopen, undo: `assertion failed: pos + count <= self.len_logical()` in `buffer.rs:162`, which aborts in release and loses unsaved work. When offsets happen to fit, undo edits the wrong bytes.
- **Recommendation:** Persist history only when the buffer bytes equal what is on disk, so skip it after a cleaned save and on the discard path. Store the file length in the entry, bump `UNDO_VERSION`, and reject a restore whose length differs.
- **Fix with:** durable-state
- **Effort:** S

### R3 · P1 · Terminal and lock file are not restored on panic, signal, or early error

- **Area:** Operability
- **Where:** `src/editor.rs:191-199`, `src/editor.rs:251`, `src/editor.rs:276-282`, `src/main.rs:123-134`, `src/signal.rs`, `Cargo.toml` (`panic = "abort"`)
- **Problem:** Raw mode, alternate screen, mouse and paste modes are set at `editor.rs:191-199` and undone only by straight-line code at `276-282`; any `?` between them (for example the draw at line 251) skips it. `rg` finds no panic hook, no `Drop` guard, and no SIGTERM or SIGHUP handler; only SIGWINCH is installed. A release panic aborts with the terminal left raw; `kill` or an ssh drop does the same. `release_lock` runs only after `run()` returns, so the lock stays and the next launch asks "already open in another instance". Traced from code, not reproduced by killing a live process.
- **Recommendation:** Add a `TerminalGuard` whose `Drop` restores termios and the reset sequences, a panic hook that does the same before abort, and SIGTERM/SIGHUP handlers that set an atomic flag polled like `take_sigwinch`. Release the lock through the same path (see R10).
- **Fix with:** lifecycle-concurrency
- **Effort:** M

### R4 · P2 · Save truncates in place, never fsyncs, and does not recheck the file on disk

- **Area:** Data
- **Where:** `src/file_io.rs:16-22`, `src/editor.rs:3586`
- **Problem:** `File::create` truncates the target, then `write_all` and `flush` with no `sync_all` and no temp-and-rename. A crash, ENOSPC or EIO mid-write leaves a truncated file; power loss can leave zero length. No mtime check happens at save time, so an external edit made after the last check is overwritten. Trace only. Current behaviour keeps symlink-follow, mode and owner, and a fix must keep that.
- **Recommendation:** Resolve symlinks, write a temp file beside the target, copy mode and owner, `sync_all`, rename, fsync the directory, and compare mtime just before writing.
- **Fix with:** os-boundaries
- **Effort:** M

### R5 · P2 · Cancelling a save prompt during quit leaves `quit_pending` set; a failed save-as keeps the bad path

- **Area:** Types
- **Where:** `src/editor.rs:765-787`, `src/editor.rs:956-994`, `src/editor.rs:3564-3606`
- **Problem:** Modal state is four independent flags (`quit_pending`, `reload_pending`, `command_buffer.active`, `find.active`). Trace: dirty unnamed buffer, Ctrl-Q, `y` opens the save-as prompt, Esc takes the Cancel arm, which never clears `quit_pending`. The next key reaches the quit branch at line 765 first; `n` runs `save_undo_if_named` and exits without saving or confirming. The SudoSave Cancel arm has the same gap. Also the `Prompt` Submit arm sets `file_path` before the save, so a failed save leaves the document pointing at the wrong path. Traced from code, not run; an existing test at `editor.rs:4738` asserts the flag stays true until a save completes.
- **Recommendation:** Clear `quit_pending` in the Cancel arms and set `file_path` only after `write_file` succeeds. Then replace the flags with one `Modal` enum.
- **Fix with:** make-invalid-states-unrepresentable
- **Effort:** S for the fix, M for the enum

### R6 · P2 · Replace-all rewrites invalid UTF-8; find skips lines that contain it

- **Area:** Data
- **Where:** `src/editor.rs:1232-1249`, `src/document.rs:93`, `src/find.rs:153`, `src/find.rs:174`, `src/find.rs:196`, `src/find.rs:220`, `src/find.rs:258`
- **Problem:** `replace_all` reads the range with `from_utf8_lossy` and writes the result back, so one match anywhere turns every invalid byte in the range into U+FFFD, including far from any match. Find does `let Ok(text) = from_utf8(..) else { continue }`, so those lines never match and nothing says so. Found by code reading; not run.
- **Recommendation:** Refuse replace-all with a status error when the range is not valid UTF-8, and report skipped lines in find.
- **Fix with:** error-model
- **Effort:** S

### R7 · P2 · Find-next misses a wrapped match on the cursor's own line

- **Area:** Types
- **Where:** `src/find.rs:212-238`
- **Problem:** Pass 0 searches `from.line` from `from.column`; pass 1 covers `0..from.line`, which excludes `from.line`. Repro: buffer `"foo bar"`, pattern `foo`, from `(0,5)` returns `None` although a match exists, so find-next reports no result.
- **Recommendation:** Make pass 1 `0..=from.line` and add a test for the single-match and same-line cases.
- **Fix with:** none
- **Effort:** S

### R8 · P2 · Local undo database, cursor database, and sudo staging file are world-readable; `HOME`-unset falls back to `/tmp`

- **Area:** Security
- **Where:** `src/file_io.rs:46`, `src/file_io.rs:138-141`, `src/file_io.rs:428-434`, `src/file_io.rs:541-544`, `src/editor.rs:3608-3625`
- **Problem:** With umask 022, `undo.bin` came out `-rw-r--r--` and `~/.config/e` is 0755. `undo.bin` holds the inserted and deleted text of every edited file, including deleted secrets. `start_sudo_save` writes the whole buffer to `/tmp/e_sudo_<pid>` with `fs::write`: it follows symlinks, is mode 0644, and stays readable while the password prompt is open. A swap before `sudo cp` is unproven. If `HOME` is unset, all state moves to `/tmp/.config/e`, a shared location.
- **Recommendation:** Create the directory 0700 and the files 0600, and error out instead of using `/tmp` when `HOME` is unset. Send sudo-save content on stdin to `sudo tee`, or use `O_EXCL|O_NOFOLLOW` 0600 in a private directory.
- **Fix with:** os-boundaries
- **Effort:** M

### R9 · P2 · Nothing verifies changes before a release: no push or PR CI, no clippy gate, and the one freshness test cannot fail

- **Area:** Build
- **Where:** `.github/workflows/release.yml:3-5`, `Makefile:27-31`, `Makefile:35`, `tests/xsh_lang.rs:30`
- **Problem:** The only workflow is `workflow_dispatch`. Tests run only against the release binary inside that workflow, and `make lint` runs clippy with `--fix --allow-dirty`, so it mutates and is not a gate. `tests/xsh_lang.rs` returns early with an `eprintln!` when the XSH registry checkout is absent, so it is green anywhere but the author's machine. It also checks `../hi-lite`, which the editor does not build against (R12). A regression is found only when someone cuts a release.
- **Recommendation:** Add a push and PR workflow running `cargo test` and `cargo clippy -- --deny warnings` without `--fix`. Move the xsh freshness test and generator into hi-lite, or mark the test `#[ignore]` so the skip is visible.
- **Fix with:** none
- **Effort:** S

### R10 · P3 · Lock files race, go stale, and conflate errors

- **Area:** Concurrency
- **Where:** `src/file_io.rs:88-108`, `src/main.rs:105-129`
- **Problem:** `acquire_lock` does `exists()` then `File::create`, so two starters can both pass. The file carries no pid or `flock`, so a stale lock after SIGKILL or abort cannot be told from a live one. "Delete lock and open anyway" unlinks a lock a live instance holds. Any `Err`, including an unwritable `~/.config`, prints the "already open in another instance" prompt. `main` reads the file before taking the lock.
- **Recommendation:** `create_new`, hold an exclusive `flock` for the process lifetime, write the pid, return a typed `Held | Io` error, and lock before reading.
- **Fix with:** os-boundaries
- **Effort:** M

### R11 · P3 · `undo.bin` rewrite is non-atomic, a version bump destroys data, and limits differ between save and load

- **Area:** Data
- **Where:** `src/file_io.rs:455`, `src/file_io.rs:594`, `src/file_io.rs:360-390`, `src/file_io.rs:135-136`, `src/file_io.rs:313-318`, `src/file_io.rs:393-395`
- **Problem:** `fs::write` truncates first, so a crash loses every file's history. A writer that sees a version mismatch overwrites the database, so two binary versions destroy each other's data. Save writes unbounded history while load rejects more than 100,000 groups and drops entries past 10,000. Each save rewrites the whole database. `flock` serialization across editors is correct. Failures are swallowed by design and never shown.
- **Recommendation:** Write via temp and rename under the same lock, cap on save with the load limits, refuse to overwrite a database of another version, and show a status on failure.
- **Fix with:** durable-state
- **Effort:** M

### R12 · P3 · The editor builds against crates.io `laputa-hi-lite` 0.2.0 while tooling and docs target the sibling 0.3.0

- **Area:** Dependencies
- **Where:** `Cargo.toml:6`, `Cargo.lock:185-188`, `examples/gen_xsh.rs`, `tests/xsh_lang.rs`
- **Problem:** `../hi-lite` is 0.3.0 and the lockfile resolves 0.2.0 from the registry, so edits to the sibling reach the editor only after a publish and bump. The XSH generator and its freshness test live in the editor repo but rewrite a file in the other repo, and `syn` and `proc-macro2` are dev-dependencies solely for that.
- **Recommendation:** Pick one source: a `path` dependency during development, or move the generator and test into hi-lite and drop `syn` and `proc-macro2` here.
- **Fix with:** single-source-of-truth
- **Effort:** S

### R13 · P3 · The benchmark gate is not wired, and some recorded benchmarks look empty

- **Area:** Build
- **Where:** `Makefile:15`, `Makefile:18`, `benches/baseline.json`, `benches/bench.rs`
- **Problem:** `.PHONY` lists `bench`, `bench-hi-lite` and `bench-syscalls`, and `RUSTYBENCH` is set, but no recipe uses any of them. `benches/baseline.json` is one macOS aarch64 recording (27 benchmarks). Nothing compares against it. Several entries have medians of 0 to 19 ns (`viewport_resize_unanchored_100k`, `viewport_resize_anchored_100k`, `find_refresh_viewport_cached`), which suggests the work is optimised away; unproven, not run. The `gifs` recipe prints `???` for a mangled arrow.
- **Recommendation:** Add a real `bench` recipe or delete the dead entries, and wrap benchmark inputs and outputs in `black_box`.
- **Fix with:** benchmark-checklist
- **Effort:** S

### R14 · P3 · Tests are mostly unit tests coupled to `Editor` internals, and critical runtime paths have no e2e scenario

- **Area:** Tests
- **Where:** `src/editor.rs:3711-7650` (about 330 tests), `tests/e2e/`, `tests/e2e/harness.rs:289-297`, `tests/e2e/undo.rs:42`
- **Problem:** About 760 unit tests against 135 in `tests/`. The editor unit tests build an `Editor` literal (duplicated at `editor.rs:3740`) and assert on private fields, so they break on refactors that change no behaviour. There is no e2e scenario for piped stdin, resize while running, terminal restore, the external-modification reload prompt, persistent undo across sessions, or invalid UTF-8 and CRLF files, which is where R1, R2, R3 and R6 live. `wait()` panics after a fixed 100 ms, and three tests sleep 1.1 s for mtime granularity.
- **Recommendation:** Add the missing e2e scenarios first, then remove unit tests they make redundant. Keep unit tests for pure logic.
- **Fix with:** renovate-tests
- **Effort:** L

### R15 · P3 · `editor.rs` is one 3.7k-line production unit plus a 3.9k-line test module

- **Area:** Architecture
- **Where:** `src/editor.rs:93-122`, `src/editor.rs:3711`
- **Problem:** One struct holds the event loop, key dispatch, caret editing, commands, save, sudo and reload flows. Every change recompiles and reviews one file, save and reload flows cannot be tested without a real filesystem, and R5 is a symptom.
- **Recommendation:** Split along existing seams: save, sudo and reload into their own module, multi-caret edits out, tests into a submodule. Build tests through `Editor::new`.
- **Fix with:** api-surface-and-module-boundaries
- **Effort:** L

### R16 · P3 · Hot paths do removable work: multi-caret edits are quadratic and every edit shifts the whole line index

- **Area:** Performance
- **Where:** `src/document.rs:302-325`, `src/operation.rs:110-137`, `src/buffer.rs:222-224`, `src/buffer.rs:294-296`, `src/render.rs:346-361`, `src/find.rs:156-163`
- **Problem:** Scratch measurements (order of magnitude; single runs, no baseline) show 2.5k, 5k, 10k and 20k carets taking 11, 38, 134 and 534 ms, because `apply_batch` clones the caret snapshots per edit. 1,000 inserts at the top of a 1M-line buffer took 522 ms because each edit adds an offset to every later `line_starts` entry. `update_user_types` scans every line on each edit for any language once a `type` line exists, though it applies only to XSH. Find computes character counts from line start per match, so 200k matches on a 200 kB line took 426 ms. Undo has no cap on groups or bytes.
- **Recommendation:** Record snapshots once per group, apply sorted edits in one pass over `line_starts`, gate the type scan on the XSH language, and count characters incrementally in find.
- **Fix with:** performance-by-mechanism
- **Effort:** M

### R17 · P3 · Viewport far-jump ignores soft wrap and can leave the cursor off screen

- **Area:** Types
- **Where:** `src/viewport.rs:154-159`
- **Problem:** When the cursor is more than `2*rows` below `scroll_line`, `scroll_line` is set assuming one row per line. In a scratch run (80x24, 200-column lines, cursor on line 100) it chose `scroll_line=79`, needing 66 rows against 22 available, so the cursor renders past the text area. Whether a later call corrects it is unproven.
- **Recommendation:** Walk back from the cursor line summing `wrapped_rows` until the rows are filled, which stays O(rows).
- **Fix with:** performance-by-mechanism
- **Effort:** S

### R18 · P3 · Release image downloads are only partly verified

- **Area:** Build
- **Where:** `Dockerfile:1`, `Dockerfile:19-20`, `Dockerfile:37-38`, `Dockerfile:80-81`
- **Problem:** `rustup-init` is fetched with `ADD` and executed with no checksum. The base image is a tag, not a digest. LLVM and mold archives are checksummed but come from personal and org forks of prebuilt releases, so those forks are in the trust path of every shipped Linux binary. Workflow actions are pinned by SHA, which is good.
- **Recommendation:** Pin `rustup-init` by sha256 and the base image by digest, and note in the Dockerfile where the prebuilt toolchains are built.
- **Fix with:** none
- **Effort:** S

### R19 · P4 · Core invariants rely on caller discipline, and a few symbols are dead

- **Area:** Dead code
- **Where:** `src/document.rs:9-14`, `src/selection.rs:107-110`, `src/document.rs:221-258`, `src/viewport.rs:269-319`, `src/buffer.rs:171-179`
- **Problem:** `Document.buffer` is public with public mutators, so "all mutations go through `Document`" is not enforced. `CaretSet.carets` and `primary` are public and `primary()` indexes directly, so an empty set aborts a release build. `insert_at_byte_with_carets` and `delete_at_byte_with_carets` have no callers, and `buffer_to_screen` is used only by its own tests.
- **Recommendation:** Delete the dead symbols, then make `CaretSet` fields private behind a non-empty constructor.
- **Fix with:** dead-code-archaeology
- **Effort:** S

### R20 · P4 · Startup, config and clipboard failures are swallowed

- **Area:** Errors
- **Where:** `src/main.rs:85-94`, `src/keybind.rs:80-100`, `src/clipboard.rs:125-152`, `src/signal.rs:34`, `src/signal.rs:66`
- **Problem:** Piped stdin is read in full with no cap and discarded when a file argument is given. After that, a declined confirmation reads EOF and exits 0 with no message. A malformed `keybindings.ini` line is dropped silently. A failed clipboard read looks like an empty clipboard. FIFOs and `/dev/zero` as the file argument are not rejected. Signal registration uses `let _ =` with no reason.
- **Recommendation:** Reject non-regular files, exit non-zero with a message when stdin is non-interactive, and report config and clipboard errors in the status line.
- **Fix with:** error-model
- **Effort:** S

## Open decisions

- **hi-lite source (R12):** a `path` dependency in dev (fast iteration, but the published editor must be built against a released version) or the registry version (current, slow to iterate). Recommendation: registry for release builds, a `[patch]` override for local work, and move the XSH generator into hi-lite.

## Strengths to keep

- `Document::apply_batch` and `UndoStack` callbacks keep undo history and dirty state aligned through one mutation path.
- `GapBuffer` keeps the ASCII fast path and the incremental line index behind precise position types, with proptest coverage.
- The persistent undo database is `flock`-serialized across editors, validates its header, and bounds what it will load.
- The release workflow pins every action by SHA, runs with empty default permissions, and verifies static or dynamic linkage per target.
