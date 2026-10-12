/// Viewport: tracks scroll offsets and maps cursor position to screen coordinates.
#[derive(Clone)]
pub struct Viewport {
    /// First visible line (0-indexed logical line).
    pub scroll_line: usize,
    /// Which wrapped sub-row of `scroll_line` is at top of screen (0 = first row).
    pub scroll_wrap: usize,
    /// Terminal width in columns.
    pub width: u16,
    /// Terminal height in rows.
    pub height: u16,
}

/// Logical content position kept at the center of the viewport during layout changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewportAnchor {
    pub line: usize,
    pub display_col: usize,
}

/// How many screen rows a line of given display width occupies (minimum 1).
pub fn wrapped_rows(display_width: usize, text_cols: usize) -> usize {
    if text_cols == 0 || display_width == 0 {
        return 1;
    }
    display_width.div_ceil(text_cols)
}

impl Viewport {
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            scroll_line: 0,
            scroll_wrap: 0,
            width,
            height,
        }
    }

    /// Number of lines available for text (total height minus status bar and command line).
    pub fn text_rows(&self) -> usize {
        (self.height as usize).saturating_sub(2)
    }

    /// Width available for text after the gutter.
    pub fn text_cols(&self, gutter_width: usize) -> usize {
        (self.width as usize).saturating_sub(gutter_width)
    }

    /// Return the logical content position currently shown at the viewport center.
    pub fn center_anchor(
        &self,
        line_count: usize,
        gutter_width: usize,
        line_display_width: &mut dyn FnMut(usize) -> usize,
    ) -> Option<ViewportAnchor> {
        let rows = self.text_rows();
        let text_cols = self.text_cols(gutter_width);
        if rows == 0 || text_cols == 0 || line_count == 0 {
            return None;
        }

        let mut line = self.scroll_line.min(line_count - 1);
        let mut wrap = self.scroll_wrap;
        let mut display_col = wrap.saturating_mul(text_cols);
        let target_row = rows / 2;

        for _ in 0..target_row {
            let wraps = wrapped_rows(line_display_width(line), text_cols);
            if wrap + 1 < wraps {
                wrap += 1;
                display_col = display_col.saturating_add(text_cols);
            } else if line + 1 < line_count {
                line += 1;
                wrap = 0;
                display_col = 0;
            } else {
                break;
            }
        }

        Some(ViewportAnchor { line, display_col })
    }

    /// Recenter the viewport on a logical content position after a layout change.
    pub fn center_on_anchor(
        &mut self,
        anchor: ViewportAnchor,
        line_count: usize,
        gutter_width: usize,
        line_display_width: &mut dyn FnMut(usize) -> usize,
    ) {
        let rows = self.text_rows();
        let text_cols = self.text_cols(gutter_width);
        if rows == 0 || text_cols == 0 || line_count == 0 {
            return;
        }

        let line = anchor.line.min(line_count - 1);
        let wraps = wrapped_rows(line_display_width(line), text_cols);
        let mut start_line = line;
        let mut start_wrap = (anchor.display_col / text_cols).min(wraps - 1);
        let mut rows_to_walk = rows / 2;

        while rows_to_walk > 0 {
            if start_wrap > 0 {
                start_wrap -= 1;
            } else if start_line > 0 {
                start_line -= 1;
                start_wrap = wrapped_rows(line_display_width(start_line), text_cols) - 1;
            } else {
                break;
            }
            rows_to_walk -= 1;
        }

        self.scroll_line = start_line;
        self.scroll_wrap = start_wrap;
    }

    /// Adjust scroll so that the cursor line/column is visible, with soft-wrap.
    ///
    /// `line_display_width` returns the total display width for a given line index.
    pub fn ensure_cursor_visible(
        &mut self,
        cursor_line: usize,
        cursor_col: usize,
        gutter_width: usize,
        line_display_width: &mut dyn FnMut(usize) -> usize,
    ) {
        let rows = self.text_rows();
        if rows == 0 {
            return;
        }
        let text_cols = self.text_cols(gutter_width);
        if text_cols == 0 {
            return;
        }

        // Which wrapped sub-row the cursor is on within its line
        let cursor_wrap = cursor_col / text_cols;

        // Cursor above viewport?
        if cursor_line < self.scroll_line
            || (cursor_line == self.scroll_line && cursor_wrap < self.scroll_wrap)
        {
            self.scroll_line = cursor_line;
            self.scroll_wrap = cursor_wrap;
            return;
        }

        // Fast path: when the cursor is far off-screen (more than 2x viewport heights
        // away), skip the O(distance) forward scan and bottom-align the cursor row by
        // walking back from the cursor line, which costs O(rows) even with soft wrap.
        // The slow path is still used for small overscrolls so minimal-scroll
        // semantics are exact.
        if cursor_line.saturating_sub(self.scroll_line) > rows * 2 {
            let cursor_rows = cursor_wrap + 1;
            if cursor_rows >= rows {
                self.scroll_line = cursor_line;
                self.scroll_wrap = cursor_rows - rows;
                return;
            }
            let mut remaining = rows - cursor_rows;
            let mut start_line = cursor_line;
            let mut start_wrap = 0;
            while remaining > 0 && start_line > 0 {
                start_line -= 1;
                let wraps = wrapped_rows(line_display_width(start_line), text_cols);
                if wraps <= remaining {
                    remaining -= wraps;
                    start_wrap = 0;
                } else {
                    start_wrap = wraps - remaining;
                    remaining = 0;
                }
            }
            self.scroll_line = start_line;
            self.scroll_wrap = start_wrap;
            return;
        }

        // Count screen rows from (scroll_line, scroll_wrap) to (cursor_line, cursor_wrap) inclusive
        let mut screen_rows = 0usize;

        if cursor_line == self.scroll_line {
            // Same line: just the difference in wrap rows + 1
            screen_rows = cursor_wrap - self.scroll_wrap + 1;
        } else {
            // Remaining rows of scroll_line
            let first_line_wraps = wrapped_rows(line_display_width(self.scroll_line), text_cols);
            screen_rows += first_line_wraps.saturating_sub(self.scroll_wrap);

            // Full lines between scroll_line+1 and cursor_line-1
            for line in (self.scroll_line + 1)..cursor_line {
                screen_rows += wrapped_rows(line_display_width(line), text_cols);
            }

            // Rows needed on cursor_line (through cursor_wrap, inclusive)
            screen_rows += cursor_wrap + 1;
        }

        // If cursor is beyond bottom of viewport, scroll forward
        if screen_rows > rows {
            let overshoot = screen_rows - rows;
            self.scroll_forward(overshoot, text_cols, line_display_width);
        }
    }

    /// Scroll forward by `n` screen rows.
    fn scroll_forward(
        &mut self,
        mut n: usize,
        text_cols: usize,
        line_display_width: &mut dyn FnMut(usize) -> usize,
    ) {
        while n > 0 {
            let wraps = wrapped_rows(line_display_width(self.scroll_line), text_cols);
            let remaining_in_line = wraps.saturating_sub(self.scroll_wrap);
            if n < remaining_in_line {
                self.scroll_wrap += n;
                return;
            }
            n -= remaining_in_line;
            self.scroll_line += 1;
            self.scroll_wrap = 0;
        }
    }

    /// Center the viewport vertically on the given line.
    pub fn center_on_line(
        &mut self,
        line: usize,
        line_display_width: &mut dyn FnMut(usize) -> usize,
        gutter_width: usize,
    ) {
        let rows = self.text_rows();
        if rows == 0 {
            return;
        }
        let text_cols = self.text_cols(gutter_width);
        if text_cols == 0 {
            return;
        }

        // Walk backwards from `line` accumulating screen rows until we've used ~half
        let half = rows / 2;
        let mut accum = 0usize;
        let mut start_line = line;
        let mut start_wrap = 0usize;

        // Include wraps of the target line itself (up to the midpoint)
        let target_wraps = wrapped_rows(line_display_width(line), text_cols);
        // Put the first row of the target line roughly at center
        accum += 1; // at least 1 row for the target line's first row

        if accum < half && start_line > 0 {
            // Walk backwards through preceding lines
            let mut remaining = half - accum;
            let mut l = line;
            while l > 0 && remaining > 0 {
                l -= 1;
                let w = wrapped_rows(line_display_width(l), text_cols);
                if w <= remaining {
                    remaining -= w;
                    start_line = l;
                    start_wrap = 0;
                } else {
                    // Partial: start at a sub-row within this line
                    start_line = l;
                    start_wrap = w - remaining;
                    remaining = 0;
                }
            }
            if remaining > 0 {
                start_line = 0;
                start_wrap = 0;
            }
        } else {
            // Target line wraps enough to fill half the screen on its own
            let _ = target_wraps; // target line starts at top
            start_line = line;
            start_wrap = 0;
        }

        self.scroll_line = start_line;
        self.scroll_wrap = start_wrap;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trivial_width(_line: usize) -> usize {
        0
    }

    // -- new / basic accessors ------------------------------------------------

    #[test]
    fn test_new() {
        let v = Viewport::new(80, 24);
        assert_eq!(v.scroll_line, 0);
        assert_eq!(v.scroll_wrap, 0);
        assert_eq!(v.width, 80);
        assert_eq!(v.height, 24);
    }

    #[test]
    fn test_text_rows() {
        let v = Viewport::new(80, 24);
        assert_eq!(v.text_rows(), 22); // 24 - 2 (status bar + command line)
    }

    #[test]
    fn test_text_rows_small_terminal() {
        let v = Viewport::new(80, 3);
        assert_eq!(v.text_rows(), 1);
    }

    #[test]
    fn test_text_rows_minimum() {
        let v = Viewport::new(80, 2);
        assert_eq!(v.text_rows(), 0);
    }

    #[test]
    fn test_text_rows_very_small() {
        let v = Viewport::new(80, 1);
        assert_eq!(v.text_rows(), 0); // saturating_sub
    }

    #[test]
    fn test_text_cols() {
        let v = Viewport::new(80, 24);
        assert_eq!(v.text_cols(5), 75); // 80 - 5 gutter
    }

    #[test]
    fn test_text_cols_no_gutter() {
        let v = Viewport::new(80, 24);
        assert_eq!(v.text_cols(0), 80);
    }

    #[test]
    fn test_text_cols_large_gutter() {
        let v = Viewport::new(10, 24);
        assert_eq!(v.text_cols(15), 0); // saturating_sub
    }

    #[test]
    fn test_center_anchor_preserves_logical_display_position() {
        let mut v = Viewport::new(14, 7); // text_cols=10, text_rows=5
        v.scroll_line = 3;
        let mut widths = |_line: usize| 25;
        let anchor = v.center_anchor(20, 4, &mut widths).unwrap();
        assert_eq!(
            anchor,
            ViewportAnchor {
                line: 3,
                display_col: 20
            }
        );

        v.width = 24; // text_cols=20
        v.height = 9; // text_rows=7
        v.center_on_anchor(anchor, 20, 4, &mut widths);

        let mut resized_widths = |_line: usize| 25;
        assert_eq!(v.center_anchor(20, 4, &mut resized_widths), Some(anchor));
    }

    #[test]
    fn test_center_anchor_handles_empty_layout() {
        let v = Viewport::new(80, 2);
        let mut widths = |_line: usize| 10;
        assert_eq!(v.center_anchor(1, 4, &mut widths), None);
    }

    // -- wrapped_rows ---------------------------------------------------------

    #[test]
    fn test_wrapped_rows_short_line() {
        assert_eq!(wrapped_rows(10, 80), 1);
    }

    #[test]
    fn test_wrapped_rows_exact_fit() {
        assert_eq!(wrapped_rows(80, 80), 1);
    }

    #[test]
    fn test_wrapped_rows_overflow() {
        assert_eq!(wrapped_rows(81, 80), 2);
    }

    #[test]
    fn test_wrapped_rows_double() {
        assert_eq!(wrapped_rows(160, 80), 2);
    }

    #[test]
    fn test_wrapped_rows_empty() {
        assert_eq!(wrapped_rows(0, 80), 1);
    }

    #[test]
    fn test_wrapped_rows_zero_cols() {
        assert_eq!(wrapped_rows(100, 0), 1);
    }

    // -- ensure_cursor_visible -----------------------------------------------

    #[test]
    fn test_ensure_visible_cursor_already_visible() {
        let mut v = Viewport::new(80, 24);
        v.ensure_cursor_visible(10, 5, 4, &mut trivial_width);
        assert_eq!(v.scroll_line, 0); // no scroll needed, line 10 is within 0..22
    }

    #[test]
    fn test_ensure_visible_scrolls_down() {
        let mut v = Viewport::new(80, 24);
        v.ensure_cursor_visible(30, 0, 4, &mut trivial_width);
        // Cursor at line 30, text_rows=22, scroll_line should be 9
        assert_eq!(v.scroll_line, 9);
    }

    #[test]
    fn test_ensure_visible_scrolls_up() {
        let mut v = Viewport::new(80, 24);
        v.scroll_line = 50;
        v.ensure_cursor_visible(10, 0, 4, &mut trivial_width);
        assert_eq!(v.scroll_line, 10);
    }

    #[test]
    fn test_ensure_visible_zero_rows() {
        let mut v = Viewport::new(80, 2); // text_rows = 0
        v.ensure_cursor_visible(10, 0, 4, &mut trivial_width);
        assert_eq!(v.scroll_line, 0);
    }

    #[test]
    fn test_ensure_visible_zero_text_cols() {
        let mut v = Viewport::new(5, 24);
        v.ensure_cursor_visible(0, 100, 10, &mut trivial_width);
        // text_cols = 0, early return
        assert_eq!(v.scroll_line, 0);
    }

    #[test]
    fn test_ensure_visible_cursor_at_last_row() {
        let mut v = Viewport::new(80, 24);
        // text_rows=22, cursor at line 21 should not scroll
        v.ensure_cursor_visible(21, 0, 4, &mut trivial_width);
        assert_eq!(v.scroll_line, 0);

        // cursor at line 22 should scroll by 1
        v.ensure_cursor_visible(22, 0, 4, &mut trivial_width);
        assert_eq!(v.scroll_line, 1);
    }

    #[test]
    fn test_ensure_visible_at_line_zero() {
        let mut v = Viewport::new(80, 24);
        v.scroll_line = 10;
        v.ensure_cursor_visible(0, 0, 4, &mut trivial_width);
        assert_eq!(v.scroll_line, 0);
    }

    #[test]
    fn test_ensure_visible_wrapped_line() {
        // 10 cols text area, line 0 is 25 chars wide → 3 wrapped rows
        let mut v = Viewport::new(14, 7); // text_rows=5, text_cols=10 (14-4 gutter)
        let mut widths = |_line: usize| -> usize { 25 };
        // cursor at column 22 → wrap row 2 (0-indexed)
        v.ensure_cursor_visible(0, 22, 4, &mut widths);
        assert_eq!(v.scroll_line, 0);
        // cursor wrap=2, scroll_wrap=0, screen_rows=3 ≤ 5, no scroll needed
        assert_eq!(v.scroll_wrap, 0);
    }

    #[test]
    fn test_ensure_visible_wrapped_scrolls_down() {
        // 10 cols text area, terminal 5 text rows
        let mut v = Viewport::new(14, 7); // text_rows=5, text_cols=10
        // Line 0 is 60 chars → 6 wrapped rows. Cursor at column 55 → wrap 5
        let mut widths = |_line: usize| -> usize { 60 };
        v.ensure_cursor_visible(0, 55, 4, &mut widths);
        // Need wrap 5 visible, 5 rows available → scroll_wrap should be 1
        assert_eq!(v.scroll_line, 0);
        assert_eq!(v.scroll_wrap, 1);
    }

    #[test]
    fn far_jump_accounts_for_soft_wrap() {
        // 200-column lines at 76 text columns wrap to 3 rows each.
        let mut v = Viewport::new(80, 24);
        let mut widths = |_line: usize| -> usize { 200 };
        v.ensure_cursor_visible(100, 0, 4, &mut widths);

        let text_cols = v.text_cols(4);
        let mut rows = wrapped_rows(200, text_cols) - v.scroll_wrap;
        for _ in (v.scroll_line + 1)..100 {
            rows += wrapped_rows(200, text_cols);
        }
        rows += 1; // cursor sits on the first wrapped row of line 100
        assert!(v.scroll_line <= 100);
        assert!(rows <= v.text_rows(), "cursor row {rows} is past the text area");
        // Bottom-aligned: scrolling one row less would lose the cursor.
        assert!(rows + 1 > v.text_rows() || v.scroll_line == 0 && v.scroll_wrap == 0);
    }

    #[test]
    fn far_jump_cursor_line_taller_than_viewport() {
        let mut v = Viewport::new(14, 6); // 4 text rows, 10 text columns
        let mut widths = |_line: usize| -> usize { 100 };
        v.ensure_cursor_visible(50, 95, 4, &mut widths);
        assert_eq!((v.scroll_line, v.scroll_wrap), (50, 9 + 1 - 4));
    }

    // -- center_on_line -------------------------------------------------------

    #[test]
    fn test_center_on_line_middle() {
        let mut v = Viewport::new(80, 24);
        let mut widths = |_: usize| -> usize { 10 }; // short lines
        v.center_on_line(50, &mut widths, 4);
        // scroll_line should be roughly 50 - 11 = 39
        assert!(v.scroll_line <= 50);
        assert!(v.scroll_line + v.text_rows() > 50);
    }

    #[test]
    fn test_center_on_line_near_start() {
        let mut v = Viewport::new(80, 24);
        let mut widths = |_: usize| -> usize { 10 };
        v.center_on_line(3, &mut widths, 4);
        assert_eq!(v.scroll_line, 0);
    }

    #[test]
    fn test_center_on_line_zero_rows() {
        let mut v = Viewport::new(80, 2); // text_rows = 0
        let mut widths = |_: usize| -> usize { 10 };
        v.center_on_line(10, &mut widths, 4);
        assert_eq!(v.scroll_line, 0);
    }

    #[test]
    fn test_center_on_line_zero_text_cols() {
        let mut v = Viewport::new(4, 24); // text_cols = 0 with gutter 4
        let mut widths = |_: usize| -> usize { 10 };
        v.center_on_line(10, &mut widths, 4);
        assert_eq!(v.scroll_line, 0);
    }

    #[test]
    fn test_center_on_line_wrapped_target() {
        // Target line is very wide → wraps, center_on_line should still work
        let mut v = Viewport::new(14, 7); // text_rows=5, text_cols=10
        let mut widths = |line: usize| -> usize { if line == 50 { 100 } else { 10 } };
        v.center_on_line(50, &mut widths, 4);
        assert!(v.scroll_line <= 50);
    }

    // -- scroll_forward -------------------------------------------------------

    #[test]
    fn test_scroll_forward_multi_line() {
        let mut v = Viewport::new(80, 24);
        let mut widths = |_: usize| -> usize { 10 };
        v.scroll_forward(3, 76, &mut widths);
        assert_eq!(v.scroll_line, 3);
        assert_eq!(v.scroll_wrap, 0);
    }

    #[test]
    fn test_scroll_forward_wrapping() {
        let mut v = Viewport::new(14, 24); // text_cols = 10
        // Line 0 has 25 display cols → 3 wraps
        let mut widths = |_: usize| -> usize { 25 };
        v.scroll_forward(2, 10, &mut widths);
        assert_eq!(v.scroll_line, 0);
        assert_eq!(v.scroll_wrap, 2);
    }
    #[test]
    fn test_center_on_line_partial_wrap() {
        // Triggers the partial wrap branch (lines 158-161) in center_on_line
        let mut v = Viewport::new(14, 14); // text_rows=12, text_cols=10 (with gutter 4)
        // half = 6, target = 5 with width 10 → 1 wrap → remaining = 5
        // Lines 4..1 each have 1 wrap → remaining = 1
        // Line 0 has 100 display cols → 10 wraps. w=10 > remaining=1 → partial
        let mut widths = |line: usize| -> usize { if line == 0 { 100 } else { 10 } };
        v.center_on_line(5, &mut widths, 4);
        // start_line should be 0 with start_wrap = 10 - 1 = 9
        assert_eq!(v.scroll_line, 0);
        assert_eq!(v.scroll_wrap, 9);
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// wrapped_rows never returns 0 and never panics.
        #[test]
        fn wrapped_rows_always_at_least_one(
            display_width in 0usize..10_000,
            text_cols in 0usize..10_000,
        ) {
            let rows = wrapped_rows(display_width, text_cols);
            prop_assert!(rows >= 1);
        }

        /// ensure_cursor_visible never panics on arbitrary inputs.
        #[test]
        fn ensure_cursor_visible_no_panic(
            width in 1u16..500,
            height in 1u16..200,
            cursor_line in 0usize..5000,
            cursor_col in 0usize..5000,
            gutter in 0usize..20,
            scroll_line in 0usize..5000,
            scroll_wrap in 0usize..50,
        ) {
            let mut v = Viewport::new(width, height);
            v.scroll_line = scroll_line;
            v.scroll_wrap = scroll_wrap;

            // Use a constant-width line function to avoid unbounded callbacks.
            let mut widths = |_line: usize| -> usize { 40 };
            v.ensure_cursor_visible(cursor_line, cursor_col, gutter, &mut widths);

            // After call, scroll state must be sane.
            // If text_rows > 0, the cursor should now be reachable.
            // At minimum, no panic occurred.
        }

        /// center_on_line never panics on arbitrary inputs.
        #[test]
        fn center_on_line_no_panic(
            width in 1u16..500,
            height in 1u16..200,
            line in 0usize..5000,
            gutter in 0usize..20,
        ) {
            let mut v = Viewport::new(width, height);
            let mut widths = |_line: usize| -> usize { 40 };
            v.center_on_line(line, &mut widths, gutter);
        }
    }
}
