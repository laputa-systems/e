# Viewport and rendering

`Viewport` owns scroll offsets, terminal dimensions, and soft-wrap mapping. `scroll_line` and `scroll_wrap` identify the first visible wrapped row. Cursor visibility must remain safe for empty, narrow, and very large files.

Before a layout change, `center_anchor()` captures the logical content at the viewport center. `center_on_anchor()` restores it after the new layout is known.

`Renderer` uses the `GapBuffer` version and dirty-line index to invalidate syntax state. It compares rendered rows before emitting ANSI output. Persistent caches stay on the renderer, and its scratch buffers are reused between frames.
