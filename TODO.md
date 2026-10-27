Prefix: EDIT
Next: 4

### EDIT-3 · P2 · Sudo save overwrites without a modification-time check

- **Area:** Data
- **Where:** `src/editor/file_session.rs:209`, `src/editor/file_session.rs:251`, `src/file_io.rs:73`
- **Problem:** `write_file` refuses the write when the file's modification time differs from `expected_mtime`. `save_to` calls that check, and on `PermissionDenied` opens the sudo password prompt. `save_file_sudo` then runs `sudo -n tee` with the cleaned bytes and never reads the modification time again. The password prompt is long enough for the root-owned file to change. `tee` truncates in place, so the newer contents are replaced by the buffer from before the prompt. This was not executed under sudo. The missing check is the whole of `save_file_sudo`.
- **Recommendation:** After sudo authentication, stat the target and return the same "changed on disk" failure `write_file` returns when the time differs from the time captured before the prompt. Do not run `tee` in that case.
- **Fix with:** durable-state
- **Effort:** S
