Prefix: EDIT
Next: 9

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
