# Find and replace

`FindState` owns the pattern, compiled regex, current match, and viewport match cache. Live find updates highlights lazily for the visible window plus lookahead. Navigation scans forward or backward on demand.

Viewport matches are cached by buffer version and viewport geometry. Multi-line regex matches are unsupported. Invalid UTF-8 lines do not match and are counted by `skipped_invalid_lines()`; replace-all refuses a range containing invalid UTF-8.
