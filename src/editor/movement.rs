//! Cursor movement, selection extension, and scrolling.

use super::*;

impl Editor {
    pub(super) fn scroll_up(&mut self) {
        if self.viewport.scroll_line == 0 && self.viewport.scroll_wrap == 0 {
            return;
        }
        let gw = if self.line_numbers_visible {
            gutter_width(self.document.buffer.line_count())
        } else {
            0
        };
        let text_cols = self.viewport.text_cols(gw);
        // Scroll back by SCROLL_LINES screen rows
        let mut remaining = SCROLL_LINES;
        while remaining > 0 {
            if self.viewport.scroll_wrap > 0 {
                let step = remaining.min(self.viewport.scroll_wrap);
                self.viewport.scroll_wrap -= step;
                remaining -= step;
            } else if self.viewport.scroll_line > 0 {
                self.viewport.scroll_line -= 1;
                let dw = self
                    .document
                    .buffer
                    .display_column_at(self.viewport.scroll_line, usize::MAX);
                let wraps = crate::viewport::wrapped_rows(dw, text_cols);
                remaining -= 1;
                if remaining > 0 && wraps > 1 {
                    let step = remaining.min(wraps - 1);
                    self.viewport.scroll_wrap = (wraps - 1).saturating_sub(step);
                    remaining -= step;
                }
            } else {
                break;
            }
        }
        // Move cursor into viewport if it scrolled past the bottom
        self.clamp_cursor_to_viewport(gw, text_cols);
    }

    pub(super) fn scroll_down(&mut self) {
        let line_count = self.document.buffer.line_count();
        if self.viewport.scroll_line >= line_count.saturating_sub(1) {
            return;
        }
        let gw = if self.line_numbers_visible {
            gutter_width(line_count)
        } else {
            0
        };
        let text_cols = self.viewport.text_cols(gw);
        // Scroll forward by SCROLL_LINES screen rows
        let mut remaining = SCROLL_LINES;
        while remaining > 0 && self.viewport.scroll_line < line_count.saturating_sub(1) {
            let dw = self
                .document
                .buffer
                .display_column_at(self.viewport.scroll_line, usize::MAX);
            let wraps = crate::viewport::wrapped_rows(dw, text_cols);
            let remaining_in_line = wraps.saturating_sub(self.viewport.scroll_wrap);
            if remaining < remaining_in_line {
                self.viewport.scroll_wrap += remaining;
                remaining = 0;
            } else {
                remaining -= remaining_in_line;
                self.viewport.scroll_line += 1;
                self.viewport.scroll_wrap = 0;
            }
        }
        // Move cursor into viewport if it scrolled past the top
        self.clamp_cursor_to_viewport(gw, text_cols);
    }

    /// After a mouse-wheel scroll, move the cursor so it stays within the visible area.
    pub(super) fn clamp_cursor_to_viewport(&mut self, _gw: usize, text_cols: usize) {
        let text_rows = self.viewport.text_rows();
        if text_rows == 0 || text_cols == 0 {
            return;
        }
        let line_count = self.document.buffer.line_count();
        let cursor = self.cursor();
        let cursor_dcol = self.cursor_display_col();
        let cursor_wrap = cursor_dcol / text_cols;

        // Check if cursor is above viewport
        if cursor.line < self.viewport.scroll_line
            || (cursor.line == self.viewport.scroll_line && cursor_wrap < self.viewport.scroll_wrap)
        {
            // Snap cursor to the first visible position
            // The first visible char column is scroll_wrap * text_cols
            let first_dcol = self.viewport.scroll_wrap * text_cols;
            let character_column = self
                .document
                .buffer
                .character_column_from_display(self.viewport.scroll_line, first_dcol);
            let line_len = self
                .document
                .buffer
                .line_character_count(self.viewport.scroll_line);
            self.set_cursor(TextPosition::new(
                self.viewport.scroll_line,
                character_column.min(line_len),
            ));
            return;
        }

        // Check if cursor is below viewport — walk screen rows to find last visible position
        let mut screen_row = 0usize;
        let mut line_idx = self.viewport.scroll_line;
        let mut last_visible_line = self.viewport.scroll_line;
        let mut last_visible_wrap = self.viewport.scroll_wrap;

        while screen_row < text_rows && line_idx < line_count {
            let dw = self.document.buffer.display_column_at(line_idx, usize::MAX);
            let total = crate::viewport::wrapped_rows(dw, text_cols);
            let start_w = if line_idx == self.viewport.scroll_line {
                self.viewport.scroll_wrap
            } else {
                0
            };
            for w in start_w..total {
                if screen_row >= text_rows {
                    break;
                }
                last_visible_line = line_idx;
                last_visible_wrap = w;
                screen_row += 1;
            }
            line_idx += 1;
        }

        if cursor.line > last_visible_line
            || (cursor.line == last_visible_line && cursor_wrap > last_visible_wrap)
        {
            // Snap cursor to last visible wrap row
            let target_dcol = last_visible_wrap * text_cols;
            let character_column = self
                .document
                .buffer
                .character_column_from_display(last_visible_line, target_dcol);
            let line_len = self.document.buffer.line_character_count(last_visible_line);
            self.set_cursor(TextPosition::new(
                last_visible_line,
                character_column.min(line_len),
            ));
        }
    }

    // -- selection helpers --------------------------------------------------

    pub(super) fn delete_selection(&mut self) {
        if !self.has_selection() {
            return;
        }
        let (start, end) = self.selection().ordered();
        self.document.seal_undo();
        let pos = self.document.delete_range(start, end);
        self.document.seal_undo();
        self.set_cursor(pos);
    }

    pub(super) fn clear_selection(&mut self) {
        self.set_cursor(self.cursor());
    }

    pub(super) fn select_all(&mut self) {
        let line_count = self.document.buffer.line_count();
        let last_line = line_count.saturating_sub(1);
        let last_col = self.document.buffer.line_character_count(last_line);
        self.set_selection(Selection {
            anchor: TextPosition::zero(),
            cursor: TextPosition::new(last_line, last_col),
        });
    }

    pub(super) fn select_above(&mut self) {
        self.set_selection(Selection {
            anchor: self.cursor(),
            cursor: TextPosition::zero(),
        });
        self.set_desired_col(None);
    }

    pub(super) fn select_below(&mut self) {
        let last_line = self.document.buffer.line_count().saturating_sub(1);
        let last_col = self.document.buffer.line_character_count(last_line);
        self.set_selection(Selection {
            anchor: self.cursor(),
            cursor: TextPosition::new(last_line, last_col),
        });
        self.set_desired_col(None);
    }

    // -- movement (no selection) --------------------------------------------

    pub(super) fn move_up_impl(&mut self, extend: bool) {
        if self.carets.is_multicursor() {
            self.mutate_carets(true, |document, _, caret| {
                let cursor = caret.selection.cursor;
                if cursor.line == 0 {
                    return;
                }
                let target_col = caret.desired_column.unwrap_or(cursor.column);
                caret.desired_column = Some(target_col);
                let new_line = cursor.line - 1;
                let line_len = document.buffer.line_character_count(new_line);
                let pos = TextPosition::new(new_line, target_col.min(line_len));
                if extend {
                    caret.selection.cursor = pos;
                } else {
                    caret.selection = Selection::caret(pos);
                }
            });
            return;
        }
        if self.cursor().line > 0 {
            let target_col = self.desired_column().unwrap_or(self.cursor().column);
            self.set_desired_col(Some(target_col));
            let new_line = self.cursor().line - 1;
            let line_len = self.document.buffer.line_character_count(new_line);
            self.move_cursor(
                TextPosition::new(new_line, target_col.min(line_len)),
                extend,
            );
        }
    }

    pub(super) fn move_up(&mut self) {
        self.move_up_impl(false);
    }

    pub(super) fn move_down_impl(&mut self, extend: bool) {
        if self.carets.is_multicursor() {
            self.mutate_carets(true, |document, _, caret| {
                let cursor = caret.selection.cursor;
                if cursor.line + 1 >= document.buffer.line_count() {
                    return;
                }
                let target_col = caret.desired_column.unwrap_or(cursor.column);
                caret.desired_column = Some(target_col);
                let new_line = cursor.line + 1;
                let line_len = document.buffer.line_character_count(new_line);
                let pos = TextPosition::new(new_line, target_col.min(line_len));
                if extend {
                    caret.selection.cursor = pos;
                } else {
                    caret.selection = Selection::caret(pos);
                }
            });
            return;
        }
        let line_count = self.document.buffer.line_count();
        if self.cursor().line + 1 < line_count {
            let target_col = self.desired_column().unwrap_or(self.cursor().column);
            self.set_desired_col(Some(target_col));
            let new_line = self.cursor().line + 1;
            let line_len = self.document.buffer.line_character_count(new_line);
            self.move_cursor(
                TextPosition::new(new_line, target_col.min(line_len)),
                extend,
            );
        }
    }

    pub(super) fn move_down(&mut self) {
        self.move_down_impl(false);
    }

    pub(super) fn indent_snap_left(&mut self, line: usize, column: usize) -> usize {
        Self::indent_snap_left_for(&self.document, line, column)
    }

    pub(super) fn indent_snap_right(&mut self, line: usize, column: usize) -> usize {
        Self::indent_snap_right_for(&self.document, line, column)
    }

    pub(super) fn move_left_impl(&mut self, extend: bool) {
        if self.carets.is_multicursor() {
            self.mutate_carets(true, |document, _, caret| {
                if !extend && !caret.selection.is_empty() {
                    let (start, _) = caret.selection.ordered();
                    caret.selection = Selection::caret(start);
                    return;
                }
                let cursor = caret.selection.cursor;
                if cursor.column > 0 {
                    let new_col = Self::indent_snap_left_for(document, cursor.line, cursor.column);
                    let pos = TextPosition::new(cursor.line, new_col);
                    if extend {
                        caret.selection.cursor = pos;
                    } else {
                        caret.selection = Selection::caret(pos);
                    }
                } else if cursor.line > 0 {
                    let prev_len = document.buffer.line_character_count(cursor.line - 1);
                    let pos = TextPosition::new(cursor.line - 1, prev_len);
                    if extend {
                        caret.selection.cursor = pos;
                    } else {
                        caret.selection = Selection::caret(pos);
                    }
                }
            });
            return;
        }
        if !extend && self.has_selection() {
            let (start, _) = self.selection().ordered();
            self.set_cursor(start);
            return;
        }
        let c = self.cursor();
        if c.column > 0 {
            let new_col = self.indent_snap_left(c.line, c.column);
            self.move_cursor(TextPosition::new(c.line, new_col), extend);
        } else if c.line > 0 {
            let prev_len = self.document.buffer.line_character_count(c.line - 1);
            self.move_cursor(TextPosition::new(c.line - 1, prev_len), extend);
        }
    }

    pub(super) fn move_left(&mut self) {
        self.set_desired_col(None);
        self.move_left_impl(false);
    }

    pub(super) fn move_right_impl(&mut self, extend: bool) {
        if self.carets.is_multicursor() {
            self.mutate_carets(true, |document, _, caret| {
                if !extend && !caret.selection.is_empty() {
                    let (_, end) = caret.selection.ordered();
                    caret.selection = Selection::caret(end);
                    return;
                }
                let cursor = caret.selection.cursor;
                let line_len = document.buffer.line_character_count(cursor.line);
                if cursor.column < line_len {
                    let new_col = Self::indent_snap_right_for(document, cursor.line, cursor.column);
                    let pos = TextPosition::new(cursor.line, new_col);
                    if extend {
                        caret.selection.cursor = pos;
                    } else {
                        caret.selection = Selection::caret(pos);
                    }
                } else if cursor.line + 1 < document.buffer.line_count() {
                    let pos = TextPosition::new(cursor.line + 1, 0);
                    if extend {
                        caret.selection.cursor = pos;
                    } else {
                        caret.selection = Selection::caret(pos);
                    }
                }
            });
            return;
        }
        if !extend && self.has_selection() {
            let (_, end) = self.selection().ordered();
            self.set_cursor(end);
            return;
        }
        let c = self.cursor();
        let line_len = self.document.buffer.line_character_count(c.line);
        if c.column < line_len {
            let new_col = self.indent_snap_right(c.line, c.column);
            self.move_cursor(TextPosition::new(c.line, new_col), extend);
        } else if c.line + 1 < self.document.buffer.line_count() {
            self.move_cursor(TextPosition::new(c.line + 1, 0), extend);
        }
    }

    pub(super) fn move_right(&mut self) {
        self.set_desired_col(None);
        self.move_right_impl(false);
    }

    /// If the cursor is on a bracket character, return the matching bracket position.
    pub(super) fn bracket_jump_target(&mut self) -> Option<TextPosition> {
        let mut scratch = std::mem::take(&mut self.line_text_scratch);
        let result = Self::bracket_jump_target_at(&self.document, &mut scratch, self.cursor());
        self.line_text_scratch = scratch;
        result
    }

    pub(super) fn word_left(&mut self) {
        self.set_desired_col(None);
        if self.carets.is_multicursor() {
            self.mutate_carets(true, |document, scratch, caret| {
                caret.desired_column = None;
                if !caret.selection.is_empty() {
                    let (start, _) = caret.selection.ordered();
                    caret.selection = Selection::caret(start);
                    return;
                }
                let cursor = caret.selection.cursor;
                if let Some(target) = Self::bracket_jump_target_at(document, scratch, cursor) {
                    caret.selection = Selection::caret(target);
                    return;
                }
                if cursor.column == 0 {
                    if cursor.line > 0 {
                        let prev_len = document.buffer.line_character_count(cursor.line - 1);
                        caret.selection =
                            Selection::caret(TextPosition::new(cursor.line - 1, prev_len));
                    }
                    return;
                }
                document.buffer.line_text_into(cursor.line, scratch);
                let boundary = prev_word_boundary(scratch, cursor.column);
                caret.selection = Selection::caret(TextPosition::new(cursor.line, boundary));
            });
            return;
        }
        if self.has_selection() {
            let (start, _) = self.selection().ordered();
            self.set_cursor(start);
            return;
        }
        if let Some(target) = self.bracket_jump_target() {
            self.set_cursor(target);
            return;
        }
        let c = self.cursor();
        if c.column == 0 {
            if c.line > 0 {
                let prev_len = self.document.buffer.line_character_count(c.line - 1);
                self.set_cursor(TextPosition::new(c.line - 1, prev_len));
            }
            return;
        }
        self.document
            .buffer
            .line_text_into(c.line, &mut self.line_text_scratch);
        let boundary = prev_word_boundary(&self.line_text_scratch, c.column);
        self.set_cursor(TextPosition::new(c.line, boundary));
    }

    pub(super) fn word_right(&mut self) {
        self.set_desired_col(None);
        if self.carets.is_multicursor() {
            self.mutate_carets(true, |document, scratch, caret| {
                caret.desired_column = None;
                if !caret.selection.is_empty() {
                    let (_, end) = caret.selection.ordered();
                    caret.selection = Selection::caret(end);
                    return;
                }
                let cursor = caret.selection.cursor;
                if let Some(target) = Self::bracket_jump_target_at(document, scratch, cursor) {
                    caret.selection = Selection::caret(target);
                    return;
                }
                let line_len = document.buffer.line_character_count(cursor.line);
                if cursor.column >= line_len {
                    if cursor.line + 1 < document.buffer.line_count() {
                        caret.selection = Selection::caret(TextPosition::new(cursor.line + 1, 0));
                    }
                    return;
                }
                document.buffer.line_text_into(cursor.line, scratch);
                let boundary = next_word_boundary(scratch, cursor.column);
                caret.selection = Selection::caret(TextPosition::new(cursor.line, boundary));
            });
            return;
        }
        if self.has_selection() {
            let (_, end) = self.selection().ordered();
            self.set_cursor(end);
            return;
        }
        if let Some(target) = self.bracket_jump_target() {
            self.set_cursor(target);
            return;
        }
        let c = self.cursor();
        let line_len = self.document.buffer.line_character_count(c.line);
        if c.column >= line_len {
            if c.line + 1 < self.document.buffer.line_count() {
                self.set_cursor(TextPosition::new(c.line + 1, 0));
            }
            return;
        }
        self.document
            .buffer
            .line_text_into(c.line, &mut self.line_text_scratch);
        let boundary = next_word_boundary(&self.line_text_scratch, c.column);
        self.set_cursor(TextPosition::new(c.line, boundary));
    }

    pub(super) fn word_left_extend(&mut self) {
        if self.carets.is_multicursor() {
            self.mutate_carets(true, |document, scratch, caret| {
                caret.desired_column = None;
                let cursor = caret.selection.cursor;
                if let Some(target) = Self::bracket_jump_target_at(document, scratch, cursor) {
                    caret.selection.cursor = target;
                    return;
                }
                if cursor.column == 0 {
                    if cursor.line > 0 {
                        let prev_len = document.buffer.line_character_count(cursor.line - 1);
                        caret.selection.cursor = TextPosition::new(cursor.line - 1, prev_len);
                    }
                    return;
                }
                document.buffer.line_text_into(cursor.line, scratch);
                let boundary = prev_word_boundary(scratch, cursor.column);
                caret.selection.cursor = TextPosition::new(cursor.line, boundary);
            });
            return;
        }
        if let Some(target) = self.bracket_jump_target() {
            self.carets.primary_mut().selection.cursor = target;
            return;
        }
        let c = self.cursor();
        if c.column == 0 {
            if c.line > 0 {
                let prev_len = self.document.buffer.line_character_count(c.line - 1);
                self.carets.primary_mut().selection.cursor =
                    TextPosition::new(c.line - 1, prev_len);
            }
            return;
        }
        self.document
            .buffer
            .line_text_into(c.line, &mut self.line_text_scratch);
        let boundary = prev_word_boundary(&self.line_text_scratch, c.column);
        self.carets.primary_mut().selection.cursor = TextPosition::new(c.line, boundary);
    }

    pub(super) fn word_right_extend(&mut self) {
        if self.carets.is_multicursor() {
            self.mutate_carets(true, |document, scratch, caret| {
                caret.desired_column = None;
                let cursor = caret.selection.cursor;
                if let Some(target) = Self::bracket_jump_target_at(document, scratch, cursor) {
                    caret.selection.cursor = target;
                    return;
                }
                let line_len = document.buffer.line_character_count(cursor.line);
                if cursor.column >= line_len {
                    if cursor.line + 1 < document.buffer.line_count() {
                        caret.selection.cursor = TextPosition::new(cursor.line + 1, 0);
                    }
                    return;
                }
                document.buffer.line_text_into(cursor.line, scratch);
                let boundary = next_word_boundary(scratch, cursor.column);
                caret.selection.cursor = TextPosition::new(cursor.line, boundary);
            });
            return;
        }
        if let Some(target) = self.bracket_jump_target() {
            self.carets.primary_mut().selection.cursor = target;
            return;
        }
        let c = self.cursor();
        let line_len = self.document.buffer.line_character_count(c.line);
        if c.column >= line_len {
            if c.line + 1 < self.document.buffer.line_count() {
                self.carets.primary_mut().selection.cursor = TextPosition::new(c.line + 1, 0);
            }
            return;
        }
        self.document
            .buffer
            .line_text_into(c.line, &mut self.line_text_scratch);
        let boundary = next_word_boundary(&self.line_text_scratch, c.column);
        self.carets.primary_mut().selection.cursor = TextPosition::new(c.line, boundary);
    }

    pub(super) fn move_home(&mut self) {
        self.set_desired_col(None);
        if self.carets.is_multicursor() {
            self.mutate_carets(true, |_, _, caret| {
                caret.desired_column = None;
                let pos = TextPosition::new(caret.selection.cursor.line, 0);
                caret.selection = Selection::caret(pos);
            });
            return;
        }
        self.set_cursor(TextPosition::new(self.cursor().line, 0));
    }

    pub(super) fn move_end(&mut self) {
        self.set_desired_col(None);
        if self.carets.is_multicursor() {
            self.mutate_carets(true, |document, _, caret| {
                caret.desired_column = None;
                let cursor = caret.selection.cursor;
                let len = document.buffer.line_character_count(cursor.line);
                caret.selection = Selection::caret(TextPosition::new(cursor.line, len));
            });
            return;
        }
        let c = self.cursor();
        let len = self.document.buffer.line_character_count(c.line);
        self.set_cursor(TextPosition::new(c.line, len));
    }

    pub(super) fn page_up(&mut self) {
        if self.carets.is_multicursor() {
            let rows = self.viewport.text_rows();
            self.mutate_carets(true, |document, _, caret| {
                let cursor = caret.selection.cursor;
                let target_col = caret.desired_column.unwrap_or(cursor.column);
                caret.desired_column = Some(target_col);
                let new_line = cursor.line.saturating_sub(rows);
                let line_len = document.buffer.line_character_count(new_line);
                caret.selection =
                    Selection::caret(TextPosition::new(new_line, target_col.min(line_len)));
            });
            return;
        }
        let rows = self.viewport.text_rows();
        let target_col = self.desired_column().unwrap_or(self.cursor().column);
        self.set_desired_col(Some(target_col));
        let new_line = self.cursor().line.saturating_sub(rows);
        let line_len = self.document.buffer.line_character_count(new_line);
        self.set_cursor_preserving_desired_col(TextPosition::new(
            new_line,
            target_col.min(line_len),
        ));
    }

    pub(super) fn page_down(&mut self) {
        if self.carets.is_multicursor() {
            let rows = self.viewport.text_rows();
            self.mutate_carets(true, |document, _, caret| {
                let cursor = caret.selection.cursor;
                let target_col = caret.desired_column.unwrap_or(cursor.column);
                caret.desired_column = Some(target_col);
                let new_line =
                    (cursor.line + rows).min(document.buffer.line_count().saturating_sub(1));
                let line_len = document.buffer.line_character_count(new_line);
                caret.selection =
                    Selection::caret(TextPosition::new(new_line, target_col.min(line_len)));
            });
            return;
        }
        let rows = self.viewport.text_rows();
        let line_count = self.document.buffer.line_count();
        let target_col = self.desired_column().unwrap_or(self.cursor().column);
        self.set_desired_col(Some(target_col));
        let new_line = (self.cursor().line + rows).min(line_count.saturating_sub(1));
        let line_len = self.document.buffer.line_character_count(new_line);
        self.set_cursor_preserving_desired_col(TextPosition::new(
            new_line,
            target_col.min(line_len),
        ));
    }

    // -- movement (extend selection) ----------------------------------------

    pub(super) fn move_up_extend(&mut self) {
        self.move_up_impl(true);
    }

    pub(super) fn move_down_extend(&mut self) {
        self.move_down_impl(true);
    }

    pub(super) fn move_left_extend(&mut self) {
        self.move_left_impl(true);
    }

    pub(super) fn move_right_extend(&mut self) {
        self.move_right_impl(true);
    }
}
