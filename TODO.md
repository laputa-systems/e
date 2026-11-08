Prefix: EDIT
Next: 9

### EDIT-8 · P3 · Persistent state saving runs blocking filesystem metadata checks across 10,000 historical files

- **Area:** Performance
- **Where:** `src/file_io.rs:652-663`, `src/file_io.rs:689-694`, `src/file_io.rs:975-978`
- **Problem:** In `save_undo_history_to` and `save_cursor_position_to`, `collect_kept_entries` and `collect_cursor_entries` run while holding an exclusive `flock` on `undo.bin.lock` / `cursor.bin.lock`. These functions iterate over up to `MAX_ENTRIES` (10,000) entries and execute synchronous `fs::metadata(path)` and `path.exists()` calls across all historical file paths to prune deleted or modified files. If historical files live on disconnected NFS/SSHFS mounts or sleeping disks, saving any local file blocks on remote filesystem timeouts while holding the user state lock.
- **Recommendation:** Prune entries lazily on load, or only prune when the entry count exceeds `MAX_ENTRIES`, or bound the check to a small recent subset rather than scanning up to 10,000 files on every save.
- **Fix with:** durable-state
- **Effort:** S
