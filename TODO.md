Prefix: EDIT
Next: 9

### EDIT-5 · P1 · Line-level editing and word navigation confuse character indices with byte offsets on UTF-8 text

- **Area:** Data
- **Where:** `src/editor/editing.rs:60-61`, `src/editor/editing.rs:78-79`, `src/editor/editing.rs:146`, `src/editor/editing.rs:162-163`, `src/editor/editing.rs:181`, `src/editor/editing.rs:358-367`, `src/editor/editing.rs:422-431`, `src/editor/editing.rs:511-513`, `src/editor/editing.rs:605-607`, `src/editor/editing.rs:684`, `src/editor/editing.rs:723`, `src/editor/movement.rs:410`, `src/editor/movement.rs:435`, `src/editor/movement.rs:508`, `src/editor/movement.rs:529`, `src/editor.rs:1267-1280`, `src/selection.rs:291-321`
- **Problem:** `TextPosition::column` is contractually a character index, but multiple line-level editing routines add `column` directly to `line_start` (`ls + column`, `ls + c.column`), or treat raw byte offsets returned by `prev_word_boundary` and `next_word_boundary` as character indices. On lines with multibyte UTF-8 characters:
1. `prev_word_boundary` and `next_word_boundary` take character columns as byte offsets into line bytes and return byte offsets that callers store into `TextPosition::column`. In `"🦀 word"`, cursor at end of line (col 6) jumps to col 5 (between 'r' and 'd') on `word_left`, and `ctrl_backspace` deletes incorrect text slices.
2. `select_word_at` treats `pos.column` as a byte index into line bytes and stores raw byte boundaries into `TextPosition::new(pos.line, start/end)`, selecting invalid ranges.
3. Autoclose skip (`is_close_char`), pair detection (`auto_close_char`), bracket splitting on Enter (`insert_newline`), and pair deletion on Backspace (`backspace`) index raw buffer bytes at `ls + column`. When preceded by multibyte characters, `byte_at(ls + column)` inspects interior continuation bytes or earlier characters. On `"é)"`, typing `)` with the cursor at the end of the line (col 2) reads byte 2 (`')'`) and advances the cursor to column 3 into thin air without inserting anything.
- **Recommendation:** Use `position_to_byte_offset` or `offset_for_pos` rather than `ls + column` for byte-level inspection. Make `prev_word_boundary`, `next_word_boundary`, and `select_word_at` character-aware (decoding UTF-8 characters or converting between byte offsets and character columns via `GapBuffer`).
- **Fix with:** none
- **Effort:** M
- **Design:** `docs/rfc/EDIT-5-utf8-column-boundaries.md`

### EDIT-6 · P2 · Terminal resize is delayed by up to 500ms and completely blocked during active input

- **Area:** Concurrency
- **Where:** `src/editor.rs:321-341`, `src/signal.rs:226-228`
- **Problem:** `Editor::event_loop` only checks `crate::signal::take_sigwinch()` inside the `Err(mpsc::RecvTimeoutError::Timeout)` arm of `rx.recv_timeout(Duration::from_millis(500))`. The SIGWINCH handler sets an atomic flag but does not wake the channel. As a result:
1. Resizing the terminal while idle does not update the viewport or redraw until the 500ms timeout expires, causing noticeable resize lag.
2. If input events arrive during or after resize (typing, movement, mouse events), `recv_timeout` returns `Ok(ev)` and `rx.try_recv()` drains the queue. The loop calls `draw(&mut stdout)?` without calling `take_sigwinch()`, repeatedly rendering with stale viewport dimensions until input stops for 500ms.
- **Recommendation:** Check `crate::signal::take_sigwinch()` and call `resize_view` at the top of the event loop before `self.draw(&mut stdout)?`, as well as immediately after handling events. To eliminate the 500ms idle delay, wake the event loop on SIGWINCH (e.g. by sending a signal event through a channel or pipe).
- **Fix with:** lifecycle-concurrency
- **Effort:** S

### EDIT-7 · P2 · Saving to a dangling symlink unconditionally fails with AlreadyExists

- **Area:** Untrusted input, OS
- **Where:** `src/file_io.rs:64-68`, `src/file_io.rs:94-103`
- **Problem:** When saving to a dangling symlink, `fs::canonicalize(path)` returns `NotFound`, but `fs::symlink_metadata(path).is_ok()` detects the symlink. Line 67 calls `write_new_file(path, data)`. `write_new_file` opens `path` with `OpenOptions::new().write(true).create_new(true).open(path)`. Under POSIX (`open(2)` with `O_CREAT | O_EXCL`), opening a symbolic link (even if dangling) is required to fail with `EEXIST` (`std::io::ErrorKind::AlreadyExists`). Saving an unnamed buffer or saving under a new name targeting a dangling symlink always fails with `File exists`, preventing the file from being written through the symlink.
- **Recommendation:** Resolve the target of the dangling symlink using `fs::read_link(path)`. If relative, join it to `path.parent()`. Call `write_new_file` on the resolved target path rather than on the symlink itself, creating the destination file while preserving the symlink.
- **Fix with:** os-boundaries
- **Effort:** S

### EDIT-8 · P3 · Persistent state saving runs blocking filesystem metadata checks across 10,000 historical files

- **Area:** Performance
- **Where:** `src/file_io.rs:652-663`, `src/file_io.rs:689-694`, `src/file_io.rs:975-978`
- **Problem:** In `save_undo_history_to` and `save_cursor_position_to`, `collect_kept_entries` and `collect_cursor_entries` run while holding an exclusive `flock` on `undo.bin.lock` / `cursor.bin.lock`. These functions iterate over up to `MAX_ENTRIES` (10,000) entries and execute synchronous `fs::metadata(path)` and `path.exists()` calls across all historical file paths to prune deleted or modified files. If historical files live on disconnected NFS/SSHFS mounts or sleeping disks, saving any local file blocks on remote filesystem timeouts while holding the user state lock.
- **Recommendation:** Prune entries lazily on load, or only prune when the entry count exceeds `MAX_ENTRIES`, or bound the check to a small recent subset rather than scanning up to 10,000 files on every save.
- **Fix with:** durable-state
- **Effort:** S
