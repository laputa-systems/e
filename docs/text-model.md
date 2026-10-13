# Text model

## GapBuffer

`GapBuffer` stores UTF-8 bytes around a movable gap. Logical byte offsets skip the gap. `line_starts` and `line_ascii` stay valid and parallel; a trailing newline terminates the last line rather than adding an empty loaded line. Invalid UTF-8 is preserved as bytes.

`TextPosition` stores a zero-based logical line and character column. Conversion methods map it to byte offsets. Tabs occupy two display columns. ASCII lines use a byte-for-character fast path, and loading a file transfers its byte vector into the buffer.

Every edit advances the buffer version and marks the earliest line whose syntax state may be stale. `take_dirty_line()` returns and clears that line index for the renderer.

## Document and undo

`Document` owns the `GapBuffer`, undo stack, dirty state, and optional file path. Mutations go through `Document` so the undo history and dirty state remain aligned.

`TextEdit` uses a half-open byte range `[start_byte, end_byte)`, plus inserted and deleted bytes. `Document::apply_batch()` consumes sorted edits in reverse order and records one undo group. Undo and redo operate through callbacks rather than an intermediate operation list.

An `UndoOperation` inserts or deletes retained bytes at a byte offset. Undo and cursor state can be persisted; see [files](files.md).

## Selections and carets

`Selection` has a fixed `anchor` and a moving `cursor`; `ordered()` returns its start and end. `CaretSet` contains one or more carets and a `primary` index that controls viewport focus and status behavior. Its constructors reject empty sets and invalid primary indices.

Each `Caret` has a sticky `desired_column` for vertical and page movement. Other movement resets it.
