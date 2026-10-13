# Files and persistence

## Open and save

The editor reads file bytes unchanged. Before saving, it strips trailing ASCII spaces and tabs from each line and ensures a final newline. It preserves other bytes, including invalid UTF-8 and CRLF line endings.

Saving is refused if the file modification time differs from the value recorded at load or the last successful save. The normal path writes a synced temporary file and renames it over the target, then syncs the containing directory. If rename cannot preserve hard-link or ownership behavior, the file is written in place. A permission failure can prompt for a sudo password. Save-as changes the document path only after the write succeeds.

External file changes prompt before reload. Dirty quit prompts before saving or discarding changes.

## Locks and user state

`HOME` is required; there is no shared temporary-directory fallback. State lives under `~/.config/e`, with directories mode `0700` and files mode `0600`.

- `keybindings.ini` stores Ctrl-key overrides.
- `undo.bin` stores persistent undo history when the buffer matches the file on disk. It is restored only when the file modification time and byte length match.
- `cursor.bin` stores the last cursor position per file.
- The database files use `.bin.lock` sidecars to serialize access while the database is replaced by rename.
- `locks/<encoded_path>.elock` holds a per-file `flock` for the editor process lifetime and records its PID. A lock left after a crash is not held.

## Clipboard

Clipboard integration uses platform-specific system tools when available and keeps an internal clipboard as a fallback.
