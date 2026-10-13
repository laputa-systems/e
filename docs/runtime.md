# Runtime

`Editor::run()` owns a `TerminalGuard` for raw mode and the alternate screen. The guard restores terminal state on normal exit, drop, and panic. A terminal input thread feeds queued events; the loop drains event bursts and polls resize and termination flags. SIGTERM and SIGHUP exit with an error after terminal restoration.

Command input is modal through `CommandBuffer`; ordinary editing operates on `CaretSet`. Editor confirmations for dirty quit, quit-after-save, and reload use a single `Modal` state. Mouse, resize, and key dispatch are handled by the editor.

The CLI accepts at most one file argument. With no file, it opens an unnamed buffer; when stdin is piped without a file argument, it reads stdin as the document before entering the editor. A named path must be a regular file or a new file. Existing files larger than 5 MB and files detected as likely binary require confirmation. Piped input is capped at 256 MiB.
