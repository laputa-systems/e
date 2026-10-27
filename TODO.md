Prefix: EDIT
Next: 4

### EDIT-1 · P1 · Non-ASCII keystrokes duplicate the line

- **Area:** Data
- **Where:** `src/input.rs:192`, `src/editor.rs:866`, `src/editor.rs:875`, `src/keybind.rs:68`
- **Problem:** `InputParser::ground_key` maps only bytes `0x20..=0x7E` to `Key::Char`. Every other ground byte, including UTF-8 (`0xC3 0xA9` for `é`), `0x00`, and the wildcard arm, becomes `Key::Null`. `Editor::handle_key` treats `Key::Null` as duplicate-line and does not consult `KeybindingTable`. Ctrl+J is byte `0x0A`, so it is also `Key::Null`, not `Key::Ctrl('j')`. The default `ctrl+j = duplicateline` binding is never hit, and a `keybindings.ini` override of `ctrl+j` does not change the key. Typing `é` into `target/debug/e` on a buffer `hi\n` saved `hi\nhi\nhi\n` (one duplicate per byte). ASCII `X` still inserted. Rebinding `ctrl+j` to `quit` left the process running.
- **Recommendation:** Decode UTF-8 in `InputParser` into `Key::Char`. Map `0x0A` to `Key::Ctrl('j')` and delete the `Key::Null` duplicate-line arm so duplicate-line comes only from the binding table. Emit no event for a byte that is not a key. In the command buffer, submit on `Key::Char('\n')` only; Enter is already that key.
- **Fix with:** none
- **Effort:** S

### EDIT-2 · P1 · One undo after replace-all drops the original text

- **Area:** Data
- **Where:** `src/editor.rs:1185`, `src/document.rs:112`, `src/operation.rs:221`
- **Problem:** `Editor::replace_all` seals the undo stack and calls `Document::replace_range_with_deleted`, which records a `Delete` and then an `Insert` through `UndoStack::record`. A kind change splits those into two groups, so replace-all is not one undo step. The first undo reverses only the insert and leaves the range deleted. On `foo bar foo\n`, `replaceall foo XXX`, one Ctrl+Z, and save, `target/debug/e` wrote `\n`. A second undo restored `foo bar foo\n`. Saving or quitting after the natural single undo persists the emptied range; trailing-whitespace cleanup then makes the buffer differ from the file, so `persist_undo_history` does not keep the rest of the stack.
- **Recommendation:** Record the delete and insert as one group. `Document::begin_undo_group` / `end_undo_group` around `replace_range_with_deleted`, or a single `record_group`, matches `apply_batch`. Add a test that one undo restores the pre-replace bytes.
- **Fix with:** none
- **Effort:** S

### EDIT-3 · P2 · Sudo save overwrites without a modification-time check

- **Area:** Data
- **Where:** `src/editor/file_session.rs:209`, `src/editor/file_session.rs:251`, `src/file_io.rs:73`
- **Problem:** `write_file` refuses the write when the file's modification time differs from `expected_mtime`. `save_to` calls that check, and on `PermissionDenied` opens the sudo password prompt. `save_file_sudo` then runs `sudo -n tee` with the cleaned bytes and never reads the modification time again. The password prompt is long enough for the root-owned file to change. `tee` truncates in place, so the newer contents are replaced by the buffer from before the prompt. This was not executed under sudo. The missing check is the whole of `save_file_sudo`.
- **Recommendation:** After sudo authentication, stat the target and return the same "changed on disk" failure `write_file` returns when the time differs from the time captured before the prompt. Do not run `tee` in that case.
- **Fix with:** durable-state
- **Effort:** S
