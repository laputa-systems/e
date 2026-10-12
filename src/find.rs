use crate::buffer::{self, GapBuffer};
use crate::selection::TextPosition;
use crate::viewport::Viewport;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ViewportMatchCache {
    buffer_version: u64,
    scroll_line: usize,
    scroll_wrap: usize,
    width: u16,
    height: u16,
}

/// Converts increasing byte offsets within one line to character columns,
/// counting each stretch of text once instead of from the line start.
struct ColumnCursor {
    byte: usize,
    column: usize,
}

impl ColumnCursor {
    fn new() -> Self {
        Self { byte: 0, column: 0 }
    }

    /// `byte_offset` must not be smaller than any offset passed before.
    fn column_at(&mut self, line: &[u8], byte_offset: usize) -> usize {
        self.column += buffer::character_count(&line[self.byte..byte_offset]);
        self.byte = byte_offset;
        self.column
    }
}

pub struct FindState {
    pub pattern: String,
    /// Viewport-only matches, reused until the buffer or viewport changes.
    pub matches: Vec<(TextPosition, TextPosition)>,
    /// Compiled regex cached across keystrokes.
    pub re: Option<regex_lite::Regex>,
    /// The currently-navigated match (start, end).
    pub current: Option<(TextPosition, TextPosition)>,
    /// 1-based index of the current match within the file.
    pub current_index: usize,
    /// True while browsing find results with up/down arrows.
    pub active: bool,
    /// Total match count across the entire file.
    pub total_count: usize,
    total_count_known: bool,
    /// Lines that are not valid UTF-8 and so cannot match. Counted over the
    /// whole file when the total match count is computed; zero until then.
    skipped_invalid_lines: usize,
    /// Cursor position captured when find mode is opened; search anchors here.
    pub search_start: TextPosition,
    viewport_cache: Option<ViewportMatchCache>,
}

impl Default for FindState {
    fn default() -> Self {
        Self::new()
    }
}

impl FindState {
    pub fn new() -> Self {
        FindState {
            pattern: String::new(),
            matches: Vec::new(),
            re: None,
            current: None,
            current_index: 0,
            active: false,
            total_count: 0,
            total_count_known: false,
            skipped_invalid_lines: 0,
            search_start: TextPosition::zero(),
            viewport_cache: None,
        }
    }

    /// Number of lines skipped because they are not valid UTF-8, as of the
    /// last whole-file count (`update_highlights`). Reset on every new search.
    pub fn skipped_invalid_lines(&self) -> usize {
        self.skipped_invalid_lines
    }

    /// Clear all find state (matches, regex, current match).
    pub fn clear(&mut self) {
        self.matches.clear();
        self.re = None;
        self.current = None;
        self.current_index = 0;
        self.total_count = 0;
        self.total_count_known = false;
        self.skipped_invalid_lines = 0;
        self.viewport_cache = None;
    }

    /// Update find highlights for a new pattern. Scans viewport and picks
    /// the first match at or after `self.search_start`.
    pub fn update_highlights(&mut self, pattern: &str, buffer: &GapBuffer, viewport: &Viewport) {
        self.update_highlights_impl(pattern, buffer, viewport, true);
    }

    /// Update find highlights without counting every match in the file.
    /// Used while the find command buffer is changing; the exact count is
    /// computed when the pattern is submitted for browsing.
    pub fn update_highlights_lazy(
        &mut self,
        pattern: &str,
        buffer: &GapBuffer,
        viewport: &Viewport,
    ) {
        self.update_highlights_impl(pattern, buffer, viewport, false);
    }

    fn update_highlights_impl(
        &mut self,
        pattern: &str,
        buffer: &GapBuffer,
        viewport: &Viewport,
        count_total: bool,
    ) {
        self.matches.clear();
        self.current = None;
        self.total_count = 0;
        self.total_count_known = false;
        self.skipped_invalid_lines = 0;
        self.viewport_cache = None;
        self.pattern = pattern.to_string();
        if pattern.is_empty() {
            self.re = None;
            return;
        }

        let re = match Self::compile_regex(pattern) {
            Some(r) => r,
            None => {
                self.re = None;
                return;
            }
        };
        self.re = Some(re);

        if count_total {
            let (total, skipped) = Self::count_all_matches(self.re.as_ref().unwrap(), buffer);
            self.total_count = total;
            self.skipped_invalid_lines = skipped;
            self.total_count_known = true;
        }
        self.refresh_viewport_matches(buffer, viewport);

        // Land on the first match at or after the search-start position.
        if let Some(re) = self.re.as_ref() {
            self.current = Self::search_forward(buffer, re, self.search_start);
        }
        if let Some((start, _)) = self.current {
            if count_total {
                self.current_index = Self::match_index(self.re.as_ref().unwrap(), buffer, start);
            }
        } else if !count_total {
            self.total_count_known = true;
        }
    }

    /// Scan visible lines only when the buffer version or viewport geometry changed.
    pub fn refresh_viewport_matches(&mut self, buffer: &GapBuffer, viewport: &Viewport) {
        let cache = ViewportMatchCache {
            buffer_version: buffer.version(),
            scroll_line: viewport.scroll_line,
            scroll_wrap: viewport.scroll_wrap,
            width: viewport.width,
            height: viewport.height,
        };
        if self.viewport_cache == Some(cache) {
            return;
        }
        self.matches.clear();
        let Some(re) = self.re.as_ref() else {
            self.viewport_cache = Some(cache);
            return;
        };
        let line_count = buffer.line_count();
        let viewport_end = (viewport.scroll_line + viewport.text_rows() + 4).min(line_count);
        let mut line_buf = Vec::new();
        for line_idx in viewport.scroll_line..viewport_end {
            buffer.line_text_into(line_idx, &mut line_buf);
            let Ok(text) = std::str::from_utf8(&line_buf) else {
                continue;
            };
            let mut columns = ColumnCursor::new();
            for m in re.find_iter(text) {
                let start_col = columns.column_at(&line_buf, m.start());
                let end_col = columns.column_at(&line_buf, m.end());
                self.matches.push((
                    TextPosition::new(line_idx, start_col),
                    TextPosition::new(line_idx, end_col),
                ));
            }
        }
        self.viewport_cache = Some(cache);
    }

    /// Return the 1-based index of the match starting at `pos` in the file.
    fn match_index(re: &regex_lite::Regex, buffer: &GapBuffer, pos: TextPosition) -> usize {
        let mut index = 0;
        let mut line_buf = Vec::new();
        for line_idx in 0..=pos.line {
            buffer.line_text_into(line_idx, &mut line_buf);
            let Ok(text) = std::str::from_utf8(&line_buf) else {
                continue;
            };
            if line_idx != pos.line {
                index += re.find_iter(text).count();
                continue;
            }
            let mut columns = ColumnCursor::new();
            for m in re.find_iter(text) {
                if columns.column_at(&line_buf, m.start()) >= pos.column {
                    return index + 1;
                }
                index += 1;
            }
        }
        // Fallback — pos is on the last match of its line
        index
    }

    /// Count all matches in the entire file, and the lines skipped because
    /// they are not valid UTF-8.
    fn count_all_matches(re: &regex_lite::Regex, buffer: &GapBuffer) -> (usize, usize) {
        let line_count = buffer.line_count();
        let mut count = 0;
        let mut skipped = 0;
        let mut line_buf = Vec::new();
        for line_idx in 0..line_count {
            buffer.line_text_into(line_idx, &mut line_buf);
            let Ok(text) = std::str::from_utf8(&line_buf) else {
                skipped += 1;
                continue;
            };
            count += re.find_iter(text).count();
        }
        (count, skipped)
    }

    /// Search forward from `from` (inclusive), wrapping around the file.
    pub fn search_forward(
        buffer: &GapBuffer,
        re: &regex_lite::Regex,
        from: TextPosition,
    ) -> Option<(TextPosition, TextPosition)> {
        let line_count = buffer.line_count();
        let mut line_buf = Vec::new();
        for pass in 0..2 {
            // The wrap pass includes the starting line: pass 0 only saw its text
            // from `from.column`, so a match before the column is found here.
            let (start, end) = if pass == 0 {
                (from.line, line_count)
            } else {
                (0, (from.line + 1).min(line_count))
            };
            for line_idx in start..end {
                buffer.line_text_into(line_idx, &mut line_buf);
                let Ok(text) = std::str::from_utf8(&line_buf) else {
                    continue;
                };
                // On the starting line, search from the byte offset of from.column
                let byte_start = if pass == 0 && line_idx == from.line {
                    buffer::character_column_to_byte_offset(&line_buf, from.column)
                } else {
                    0
                };
                if let Some(m) = re.find_at(text, byte_start) {
                    let mut columns = ColumnCursor::new();
                    let start_col = columns.column_at(&line_buf, m.start());
                    let end_col = columns.column_at(&line_buf, m.end());
                    return Some((
                        TextPosition::new(line_idx, start_col),
                        TextPosition::new(line_idx, end_col),
                    ));
                }
            }
        }
        None
    }

    /// Search backward from `from` (exclusive), wrapping around the file.
    pub fn search_backward(
        buffer: &GapBuffer,
        re: &regex_lite::Regex,
        from: TextPosition,
    ) -> Option<(TextPosition, TextPosition)> {
        let line_count = buffer.line_count();
        let mut line_buf = Vec::new();
        for pass in 0..2 {
            // The wrap pass starts at the cursor line: earlier lines were already
            // searched in pass 0, and the cursor line may hold matches after the
            // cursor that pass 0 excluded.
            let (start, end) = if pass == 0 {
                (0, (from.line + 1).min(line_count))
            } else {
                (from.line.min(line_count - 1), line_count)
            };
            for line_idx in (start..end).rev() {
                buffer.line_text_into(line_idx, &mut line_buf);
                let Ok(text) = std::str::from_utf8(&line_buf) else {
                    continue;
                };
                // Walk re.find_at() to find the last match on this line
                let mut best: Option<(TextPosition, TextPosition)> = None;
                let mut at = 0;
                let mut columns = ColumnCursor::new();
                while let Some(m) = re.find_at(text, at) {
                    let start_col = columns.column_at(&line_buf, m.start());
                    let end_col = columns.column_at(&line_buf, m.end());
                    if pass == 0 && line_idx == from.line && end_col >= from.column {
                        break;
                    }
                    best = Some((
                        TextPosition::new(line_idx, start_col),
                        TextPosition::new(line_idx, end_col),
                    ));
                    // Advance past this match; an empty match steps over one whole
                    // character so `at` never lands inside a UTF-8 sequence.
                    at = if m.end() > m.start() {
                        m.end()
                    } else {
                        m.end() + text[m.end()..].chars().next().map_or(1, char::len_utf8)
                    };
                    if at >= text.len() {
                        break;
                    }
                }
                if best.is_some() {
                    return best;
                }
            }
        }
        None
    }

    /// Navigate to the next match. Returns the match position if found.
    pub fn find_next(
        &mut self,
        buffer: &GapBuffer,
        cursor: TextPosition,
    ) -> Option<(TextPosition, TextPosition)> {
        let re = self.re.as_ref()?;
        let result = Self::search_forward(buffer, re, cursor);
        if let Some(m) = result {
            self.current = Some(m);
            self.current_index = Self::match_index(re, buffer, m.0);
        }
        result
    }

    /// Navigate to the previous match. Returns the match position if found.
    pub fn find_prev(
        &mut self,
        buffer: &GapBuffer,
        cursor: TextPosition,
    ) -> Option<(TextPosition, TextPosition)> {
        let re = self.re.as_ref()?;
        let result = Self::search_backward(buffer, re, cursor);
        if let Some(m) = result {
            self.current = Some(m);
            self.current_index = Self::match_index(re, buffer, m.0);
        }
        result
    }

    /// Exit find mode. Returns the current match (if any) for selection.
    pub fn exit(&mut self) -> Option<(TextPosition, TextPosition)> {
        let current = self.current;
        self.active = false;
        self.matches.clear();
        self.re = None;
        self.current = None;
        self.current_index = 0;
        self.total_count = 0;
        self.total_count_known = false;
        self.skipped_invalid_lines = 0;
        self.viewport_cache = None;
        current
    }

    /// Format the find status text.
    pub fn status_text(&self) -> String {
        if !self.total_count_known {
            return format!("Find: {}", self.pattern);
        }
        if self.total_count == 0 {
            return format!("Find: {} (no matches)", self.pattern);
        }
        if self.current_index > 0 {
            format!(
                "Find: {} (match {} of {})",
                self.pattern, self.current_index, self.total_count
            )
        } else {
            format!(
                "Find: {} ({} match{})",
                self.pattern,
                self.total_count,
                if self.total_count == 1 { "" } else { "es" }
            )
        }
    }

    /// Compile a regex with smart-case: case-insensitive if pattern is all lowercase.
    fn compile_regex(pattern: &str) -> Option<regex_lite::Regex> {
        let case_insensitive = pattern.chars().all(|c| !c.is_uppercase());
        let result = if case_insensitive {
            regex_lite::RegexBuilder::new(pattern)
                .case_insensitive(true)
                .build()
        } else {
            regex_lite::Regex::new(pattern)
        };
        result.ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewport_matches_refresh_after_buffer_version_changes() {
        let mut buffer = GapBuffer::from_bytes(b"alpha\nbeta".to_vec());
        let viewport = Viewport::new(80, 24);
        let mut find = FindState::new();
        find.update_highlights_lazy("alpha", &buffer, &viewport);
        assert_eq!(find.matches.len(), 1);

        find.refresh_viewport_matches(&buffer, &viewport);
        buffer.insert(5, b" alpha");
        find.refresh_viewport_matches(&buffer, &viewport);

        assert_eq!(find.matches.len(), 2);
    }

    fn regex(pattern: &str) -> regex_lite::Regex {
        FindState::compile_regex(pattern).unwrap()
    }

    fn span(line: usize, start: usize, end: usize) -> Option<(TextPosition, TextPosition)> {
        Some((TextPosition::new(line, start), TextPosition::new(line, end)))
    }

    #[test]
    fn forward_wraps_to_single_match_on_cursor_line() {
        let buffer = GapBuffer::from_bytes(b"foo bar".to_vec());
        let found = FindState::search_forward(&buffer, &regex("foo"), TextPosition::new(0, 5));
        assert_eq!(found, span(0, 0, 3));
    }

    #[test]
    fn forward_prefers_later_lines_over_earlier_match_on_cursor_line() {
        let buffer = GapBuffer::from_bytes(b"foo foo\nbar\nfoo".to_vec());
        let re = regex("foo");
        assert_eq!(
            FindState::search_forward(&buffer, &re, TextPosition::new(0, 5)),
            span(2, 0, 3)
        );
        // From the last line's only match the search wraps to the first line.
        assert_eq!(
            FindState::search_forward(&buffer, &re, TextPosition::new(2, 1)),
            span(0, 0, 3)
        );
        // Within a line it returns the next match, not the first one.
        assert_eq!(
            FindState::search_forward(&buffer, &re, TextPosition::new(0, 1)),
            span(0, 4, 7)
        );
    }

    #[test]
    fn forward_returns_none_without_matches() {
        let buffer = GapBuffer::from_bytes(b"foo bar".to_vec());
        assert_eq!(
            FindState::search_forward(&buffer, &regex("zzz"), TextPosition::new(0, 5)),
            None
        );
    }

    #[test]
    fn backward_wraps_to_single_match_on_cursor_line() {
        let buffer = GapBuffer::from_bytes(b"bar foo".to_vec());
        let re = regex("foo");
        assert_eq!(
            FindState::search_backward(&buffer, &re, TextPosition::new(0, 2)),
            span(0, 4, 7)
        );
        // At the start of the only match, wrapping lands on it again.
        assert_eq!(
            FindState::search_backward(&buffer, &re, TextPosition::new(0, 4)),
            span(0, 4, 7)
        );
    }

    #[test]
    fn backward_prefers_earlier_lines_and_last_match_on_cursor_line() {
        let buffer = GapBuffer::from_bytes(b"foo\nfoo foo bar\nfoo".to_vec());
        let re = regex("foo");
        assert_eq!(
            FindState::search_backward(&buffer, &re, TextPosition::new(1, 5)),
            span(1, 0, 3)
        );
        assert_eq!(
            FindState::search_backward(&buffer, &re, TextPosition::new(1, 0)),
            span(0, 0, 3)
        );
        assert_eq!(
            FindState::search_backward(&buffer, &re, TextPosition::new(0, 0)),
            span(2, 0, 3)
        );
    }

    #[test]
    fn columns_count_characters_not_bytes() {
        let buffer = GapBuffer::from_bytes("é foo é foo".as_bytes().to_vec());
        let found = FindState::search_forward(&buffer, &regex("foo"), TextPosition::new(0, 5));
        assert_eq!(found, span(0, 8, 11));
        let mut find = FindState::new();
        find.update_highlights("foo", &buffer, &Viewport::new(80, 24));
        assert_eq!(find.matches, vec![span(0, 2, 5).unwrap(), span(0, 8, 11).unwrap()]);
    }

    #[test]
    fn skipped_invalid_lines_are_counted_and_reset_per_search() {
        let buffer = GapBuffer::from_bytes(b"foo\n\xff foo\nfoo\n\xfe\nbar".to_vec());
        let viewport = Viewport::new(80, 24);
        let mut find = FindState::new();
        find.update_highlights("foo", &buffer, &viewport);
        assert_eq!(find.total_count, 2);
        assert_eq!(find.skipped_invalid_lines(), 2);

        find.update_highlights_lazy("foo", &buffer, &viewport);
        assert_eq!(find.skipped_invalid_lines(), 0);

        find.update_highlights("foo", &buffer, &viewport);
        find.clear();
        assert_eq!(find.skipped_invalid_lines(), 0);
    }

    #[test]
    fn search_backward_steps_over_whole_characters_on_empty_matches() {
        // An empty-matching pattern on non-ASCII text must not resume the
        // scan inside a multi-byte character.
        let buffer = GapBuffer::from_bytes("éé".as_bytes().to_vec());
        let found = FindState::search_backward(&buffer, &regex("x*"), TextPosition::new(0, 2));
        assert!(found.is_some());
    }
}
