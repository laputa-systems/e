//! Text-changing commands: typing, newline, deletion, indentation, comment
//! toggling, dedent, undo/redo, and whitespace conversions. Each handles the
//! single-caret and multi-caret cases.

use super::*;

impl Editor {
    // -- editing ------------------------------------------------------------

    pub(super) fn insert_char(&mut self, c: char) {
        let lang_name = self.document.detect_language().map(|l| l.name());
        if self.carets.is_multicursor() {
            let mut planned = Vec::with_capacity(self.carets.len());
            let mut char_buf = [0u8; 4];
            let encoded = c.encode_utf8(&mut char_buf);
            let encoded_bytes = encoded.as_bytes();

            for caret in self.carets.iter().copied() {
                let selection = caret.selection;
                if !selection.is_empty() {
                    let (start, end) = selection.ordered();
                    let start_offset = self.offset_for_pos(start);
                    let deleted = self.document.text_in_range(start, end);
                    let end_offset = start_offset + deleted.len();
                    if let Some(close) = auto_close_char(c, lang_name) {
                        let mut insert =
                            Vec::with_capacity(deleted.len() + encoded_bytes.len() + 1);
                        insert.extend_from_slice(encoded_bytes);
                        insert.extend_from_slice(&deleted);
                        insert.push(close as u8);
                        planned.push(PlannedCaretEdit {
                            start_byte: start_offset,
                            end_byte: end_offset,
                            inserted_bytes: insert,
                            deleted_bytes: deleted,
                            anchor_after_byte: start_offset + encoded_bytes.len(),
                            cursor_after_byte: start_offset
                                + encoded_bytes.len()
                                + (end_offset - start_offset),
                        });
                    } else {
                        planned.push(PlannedCaretEdit {
                            start_byte: start_offset,
                            end_byte: end_offset,
                            inserted_bytes: encoded_bytes.to_vec(),
                            deleted_bytes: deleted,
                            anchor_after_byte: start_offset + encoded_bytes.len(),
                            cursor_after_byte: start_offset + encoded_bytes.len(),
                        });
                    }
                    continue;
                }

                let pos = selection.cursor;
                let offset = self.offset_for_pos(pos);

                if is_close_char(c) {
                    let ls = self.document.buffer.line_start(pos.line);
                    let le = self.document.buffer.line_end(pos.line);
                    if ls + pos.column < le
                        && self.document.buffer.byte_at(ls + pos.column) == c as u8
                    {
                        planned.push(PlannedCaretEdit {
                            start_byte: offset,
                            end_byte: offset,
                            inserted_bytes: Vec::new(),
                            deleted_bytes: Vec::new(),
                            anchor_after_byte: offset + 1,
                            cursor_after_byte: offset + 1,
                        });
                        continue;
                    }
                }

                if let Some(close) = auto_close_char(c, lang_name) {
                    let ls = self.document.buffer.line_start(pos.line);
                    let le = self.document.buffer.line_end(pos.line);
                    let next = if ls + pos.column < le {
                        self.document.buffer.byte_at(ls + pos.column)
                    } else {
                        b'\n'
                    };
                    let next_is_boundary = next == b' '
                        || next == b'\t'
                        || next == b'\n'
                        || is_close_char(next as char);
                    if next_is_boundary {
                        let mut insert = Vec::with_capacity(encoded_bytes.len() + 1);
                        insert.extend_from_slice(encoded_bytes);
                        insert.push(close as u8);
                        planned.push(PlannedCaretEdit {
                            start_byte: offset,
                            end_byte: offset,
                            inserted_bytes: insert,
                            deleted_bytes: Vec::new(),
                            anchor_after_byte: offset + encoded_bytes.len(),
                            cursor_after_byte: offset + encoded_bytes.len(),
                        });
                        continue;
                    }
                }

                planned.push(PlannedCaretEdit {
                    start_byte: offset,
                    end_byte: offset,
                    inserted_bytes: encoded_bytes.to_vec(),
                    deleted_bytes: Vec::new(),
                    anchor_after_byte: offset + encoded_bytes.len(),
                    cursor_after_byte: offset + encoded_bytes.len(),
                });
            }

            self.apply_multi_caret_edits(planned);
            return;
        }

        if self.has_selection() {
            // Wrap selection with matching pairs
            if let Some(close) = auto_close_char(c, lang_name) {
                let (start, end) = self.selection().ordered();
                let text = self.document.text_in_range(start, end);
                let mut wrapped = vec![c as u8];
                wrapped.extend_from_slice(&text);
                wrapped.push(close as u8);
                self.document.begin_undo_group();
                self.document.delete_range(start, end);
                let after = self.document.insert(start.line, start.column, &wrapped);
                self.document.end_undo_group();
                // Select the inner text (between the pair chars)
                self.set_selection(Selection {
                    anchor: TextPosition::new(start.line, start.column + 1),
                    cursor: TextPosition::new(after.line, after.column - 1),
                });
                return;
            }
            self.delete_selection();
        }

        // Skip over closing char if it's already the next character.
        // close chars are ASCII so byte_at(line_start + column) == the char at column.
        if is_close_char(c) {
            let line = self.cursor().line;
            let column = self.cursor().column;
            let ls = self.document.buffer.line_start(line);
            let le = self.document.buffer.line_end(line);
            if ls + column < le && self.document.buffer.byte_at(ls + column) == c as u8 {
                self.set_cursor(TextPosition::new(line, column + 1));
                return;
            }
        }

        let mut char_buf = [0u8; 4];
        let s = c.encode_utf8(&mut char_buf);

        // Auto-close pairs: insert open+close on a stack buffer, no heap alloc.
        if let Some(close) = auto_close_char(c, lang_name) {
            let line = self.cursor().line;
            let column = self.cursor().column;
            let ls = self.document.buffer.line_start(line);
            let le = self.document.buffer.line_end(line);
            // Treat end-of-line (\n or past end) as a boundary.
            let next = if ls + column < le {
                self.document.buffer.byte_at(ls + column)
            } else {
                b'\n'
            };
            let next_is_boundary =
                next == b' ' || next == b'\t' || next == b'\n' || is_close_char(next as char);
            if next_is_boundary {
                if c == close {
                    // Symmetric-pair fence heuristic: a run of three quote-like
                    // characters at a boundary almost always means a fenced
                    // block or a triple-quoted string (``` ```, """ """,
                    // ''' ''') rather than an inline pair. If the cursor is
                    // already preceded by two of the same character, close with
                    // three more so the caret lands inside. Asymmetric pairs
                    // ((, [, {) are excluded because a run of identical open
                    // chars is never a meaningful delimiter.
                    let mut n = 0;
                    while column > n
                        && self.document.buffer.byte_at(ls + column - 1 - n) == c as u8
                    {
                        n += 1;
                    }
                    if n >= 2 {
                        let fence = [c as u8; 4];
                        let pos = self.document.insert(line, column, &fence);
                        // Caret sits after the third opening character.
                        self.set_cursor(TextPosition::new(pos.line, pos.column - 3));
                        return;
                    }
                }
                // Stack-allocate the pair: open char (1–4 bytes) + close char (1 byte).
                let cb = s.as_bytes();
                let mut pair = [0u8; 5];
                pair[..cb.len()].copy_from_slice(cb);
                pair[cb.len()] = close as u8;
                let pos = self.document.insert(line, column, &pair[..cb.len() + 1]);
                // Place cursor between the pair
                self.set_cursor(TextPosition::new(pos.line, pos.column - 1));
                return;
            }
        }

        let pos = self
            .document
            .insert(self.cursor().line, self.cursor().column, s.as_bytes());
        self.set_cursor(pos);
    }

    pub(super) fn insert_tab(&mut self) {
        if self.carets.is_multicursor() {
            if self.carets.iter().any(|caret| !caret.selection.is_empty()) {
                self.indent_caret_lines();
                return;
            }
            let bytes: &[u8] = if self.use_tab_indent() { b"\t" } else { b"  " };
            let mut planned = Vec::with_capacity(self.carets.len());
            for caret in self.carets.iter().copied() {
                let selection = caret.selection;
                let (start, end) = selection.ordered();
                let start_offset = self.offset_for_pos(start);
                let deleted = if selection.is_empty() {
                    Vec::new()
                } else {
                    self.document.text_in_range(start, end)
                };
                let end_offset = start_offset + deleted.len();
                planned.push(PlannedCaretEdit {
                    start_byte: start_offset,
                    end_byte: end_offset,
                    inserted_bytes: bytes.to_vec(),
                    deleted_bytes: deleted,
                    anchor_after_byte: start_offset + bytes.len(),
                    cursor_after_byte: start_offset + bytes.len(),
                });
            }
            self.apply_multi_caret_edits(planned);
            return;
        }

        if self.has_selection() {
            self.indent_selection();
            return;
        }
        let bytes: &[u8] = if self.use_tab_indent() { b"\t" } else { b"  " };
        let pos = self
            .document
            .insert(self.cursor().line, self.cursor().column, bytes);
        self.set_cursor(pos);
    }

    pub(super) fn indent_caret_lines(&mut self) {
        let use_tab = self.use_tab_indent();
        let indent_bytes: &[u8] = if use_tab { b"\t" } else { b"  " };
        let mut text_edits = Vec::new();

        for line in self.targeted_lines_for_carets() {
            let text = self.document.buffer.line_text(line);
            let is_blank = text.iter().all(|&b| b == b' ' || b == b'\t');
            if is_blank {
                continue;
            }
            text_edits.push(TextEdit {
                start_byte: self.document.buffer.line_start(line),
                end_byte: self.document.buffer.line_start(line),
                inserted_bytes: indent_bytes.to_vec(),
                deleted_bytes: Vec::new(),
            });
        }

        self.apply_text_edits_preserving_carets(text_edits);
    }

    pub(super) fn indent_selection(&mut self) {
        let (s, e) = self.selection().ordered();
        let end_line = if e.column == 0 && e.line > s.line {
            e.line - 1
        } else {
            e.line
        };
        let start_line = s.line;

        let use_tab = self.use_tab_indent();
        let indent_bytes: &[u8] = if use_tab { b"\t" } else { b"  " };
        let indent_char_len = if use_tab { 1 } else { 2 };

        // Pre-read line data to avoid O(n²) cache rebuilds
        let lines: Vec<(Vec<u8>, usize)> = (start_line..=end_line)
            .map(|i| {
                (
                    self.document.buffer.line_text(i),
                    self.document.buffer.line_start(i),
                )
            })
            .collect();

        let cursor_pos = self.cursor();
        self.document.begin_undo_group();
        let anchor_line = self.selection().anchor.line;
        let cursor_line = self.selection().cursor.line;
        let mut anchor_added = 0usize;
        let mut cursor_added = 0usize;
        for (idx, (text, line_offset)) in lines.iter().enumerate().rev() {
            let is_blank = text.iter().all(|&b| b == b' ' || b == b'\t');
            if is_blank {
                continue;
            }
            self.document
                .insert_at_byte(*line_offset, indent_bytes, cursor_pos, cursor_pos);
            let line_idx = start_line + idx;
            if line_idx == cursor_line {
                cursor_added = indent_char_len;
            }
            if line_idx == anchor_line {
                anchor_added = indent_char_len;
            }
        }
        self.document.end_undo_group();

        // Preserve the selection so the user can indent multiple times.
        self.carets.primary_mut().selection.cursor.column += cursor_added;
        self.carets.primary_mut().selection.anchor.column += anchor_added;
    }

    pub(super) fn insert_newline(&mut self) {
        if self.carets.is_multicursor() {
            let mut planned = Vec::with_capacity(self.carets.len());
            for caret in self.carets.iter().copied() {
                let selection = caret.selection;
                let (start, end) = selection.ordered();
                let base = if selection.is_empty() {
                    selection.cursor
                } else {
                    start
                };
                let start_offset = self.offset_for_pos(start);
                let deleted = if selection.is_empty() {
                    Vec::new()
                } else {
                    self.document.text_in_range(start, end)
                };
                let end_offset = start_offset + deleted.len();

                self.document
                    .buffer
                    .line_text_into(base.line, &mut self.line_text_scratch);
                let indent: Vec<u8> = self
                    .line_text_scratch
                    .iter()
                    .take_while(|&&b| b == b' ' || b == b'\t')
                    .copied()
                    .collect();

                if selection.is_empty() && base.column > 0 {
                    let ls = self.document.buffer.line_start(base.line);
                    let le = self.document.buffer.line_end(base.line);
                    let prev = self.document.buffer.byte_at(ls + base.column - 1);
                    let close_opt = match prev {
                        b'(' => Some(b')'),
                        b'[' => Some(b']'),
                        b'{' => Some(b'}'),
                        _ => None,
                    };
                    if let Some(close) = close_opt
                        && ls + base.column < le
                        && self.document.buffer.byte_at(ls + base.column) == close
                    {
                        let extra: &[u8] = if self.use_tab_indent() { b"\t" } else { b"  " };
                        let mut insert = vec![b'\n'];
                        insert.extend_from_slice(&indent);
                        insert.extend_from_slice(extra);
                        let cursor_offset = start_offset + insert.len();
                        insert.push(b'\n');
                        insert.extend_from_slice(&indent);
                        planned.push(PlannedCaretEdit {
                            start_byte: start_offset,
                            end_byte: end_offset,
                            inserted_bytes: insert,
                            deleted_bytes: deleted,
                            anchor_after_byte: cursor_offset,
                            cursor_after_byte: cursor_offset,
                        });
                        continue;
                    }
                }

                let mut insert = vec![b'\n'];
                insert.extend_from_slice(&indent);
                let cursor_offset = start_offset + insert.len();
                planned.push(PlannedCaretEdit {
                    start_byte: start_offset,
                    end_byte: end_offset,
                    inserted_bytes: insert,
                    deleted_bytes: deleted,
                    anchor_after_byte: cursor_offset,
                    cursor_after_byte: cursor_offset,
                });
            }
            self.apply_multi_caret_edits(planned);
            return;
        }

        if self.has_selection() {
            self.delete_selection();
        }
        let c = self.cursor();
        self.document
            .buffer
            .line_text_into(c.line, &mut self.line_text_scratch);
        let indent: Vec<u8> = self
            .line_text_scratch
            .iter()
            .take_while(|&&b| b == b' ' || b == b'\t')
            .copied()
            .collect();

        // Between bracket pairs ({|}, (|), [|]): split with extra indent level.
        if c.column > 0 {
            let ls = self.document.buffer.line_start(c.line);
            let le = self.document.buffer.line_end(c.line);
            let prev = self.document.buffer.byte_at(ls + c.column - 1);
            let close_opt = match prev {
                b'(' => Some(b')'),
                b'[' => Some(b']'),
                b'{' => Some(b'}'),
                _ => None,
            };
            if let Some(close) = close_opt
                && ls + c.column < le
                && self.document.buffer.byte_at(ls + c.column) == close
            {
                let extra: &[u8] = if self.use_tab_indent() { b"\t" } else { b"  " };
                let mut split = vec![b'\n'];
                split.extend_from_slice(&indent);
                split.extend_from_slice(extra);
                let cursor_col = indent.len() + extra.len();
                split.push(b'\n');
                split.extend_from_slice(&indent);
                self.document.seal_undo();
                self.document.insert(c.line, c.column, &split);
                self.document.seal_undo();
                self.set_cursor(TextPosition::new(c.line + 1, cursor_col));
                return;
            }
        }

        let mut newline = vec![b'\n'];
        newline.extend_from_slice(&indent);

        self.document.seal_undo();
        let pos = self
            .document
            .insert(self.cursor().line, self.cursor().column, &newline);
        self.document.seal_undo();
        self.set_cursor(pos);
    }

    pub(super) fn backspace(&mut self) {
        if self.carets.is_multicursor() {
            let mut planned = Vec::with_capacity(self.carets.len());
            for caret in self.carets.iter().copied() {
                let selection = caret.selection;
                if !selection.is_empty() {
                    let (start, end) = selection.ordered();
                    let start_offset = self.offset_for_pos(start);
                    let deleted = self.document.text_in_range(start, end);
                    planned.push(PlannedCaretEdit {
                        start_byte: start_offset,
                        end_byte: start_offset + deleted.len(),
                        inserted_bytes: Vec::new(),
                        deleted_bytes: deleted,
                        anchor_after_byte: start_offset,
                        cursor_after_byte: start_offset,
                    });
                    continue;
                }

                let c = selection.cursor;
                if c.column > 0 {
                    let ls = self.document.buffer.line_start(c.line);
                    let le = self.document.buffer.line_end(c.line);
                    let mut leading_ws = 0;
                    while ls + leading_ws < le {
                        match self.document.buffer.byte_at(ls + leading_ws) {
                            b' ' | b'\t' => leading_ws += 1,
                            _ => break,
                        }
                    }

                    if c.column <= leading_ws && c.column >= 2 {
                        let all_spaces =
                            (0..c.column).all(|i| self.document.buffer.byte_at(ls + i) == b' ');
                        if all_spaces && c.column.is_multiple_of(2) {
                            let start = TextPosition::new(c.line, c.column - 2);
                            let end = TextPosition::new(c.line, c.column);
                            let start_offset = self.offset_for_pos(start);
                            let deleted = self.document.text_in_range(start, end);
                            planned.push(PlannedCaretEdit {
                                start_byte: start_offset,
                                end_byte: start_offset + deleted.len(),
                                inserted_bytes: Vec::new(),
                                deleted_bytes: deleted,
                                anchor_after_byte: start_offset,
                                cursor_after_byte: start_offset,
                            });
                            continue;
                        }
                    }

                    let prev = self.document.buffer.byte_at(ls + c.column - 1);
                    if ls + c.column < le {
                        let next = self.document.buffer.byte_at(ls + c.column);
                        let lang_name = self.document.detect_language().map(|l| l.name());
                        if auto_close_char(prev as char, lang_name) == Some(next as char) {
                            let start = TextPosition::new(c.line, c.column - 1);
                            let end = TextPosition::new(c.line, c.column + 1);
                            let start_offset = self.offset_for_pos(start);
                            let deleted = self.document.text_in_range(start, end);
                            planned.push(PlannedCaretEdit {
                                start_byte: start_offset,
                                end_byte: start_offset + deleted.len(),
                                inserted_bytes: Vec::new(),
                                deleted_bytes: deleted,
                                anchor_after_byte: start_offset,
                                cursor_after_byte: start_offset,
                            });
                            continue;
                        }
                    }

                    let start = TextPosition::new(c.line, c.column - 1);
                    let end = TextPosition::new(c.line, c.column);
                    let start_offset = self.offset_for_pos(start);
                    let deleted = self.document.text_in_range(start, end);
                    planned.push(PlannedCaretEdit {
                        start_byte: start_offset,
                        end_byte: start_offset + deleted.len(),
                        inserted_bytes: Vec::new(),
                        deleted_bytes: deleted,
                        anchor_after_byte: start_offset,
                        cursor_after_byte: start_offset,
                    });
                } else if c.line > 0 {
                    let prev_len = self.document.buffer.line_character_count(c.line - 1);
                    let start = TextPosition::new(c.line - 1, prev_len);
                    let end = TextPosition::new(c.line, 0);
                    let start_offset = self.offset_for_pos(start);
                    let deleted = self.document.text_in_range(start, end);
                    planned.push(PlannedCaretEdit {
                        start_byte: start_offset,
                        end_byte: start_offset + deleted.len(),
                        inserted_bytes: Vec::new(),
                        deleted_bytes: deleted,
                        anchor_after_byte: start_offset,
                        cursor_after_byte: start_offset,
                    });
                } else {
                    let offset = self.offset_for_pos(c);
                    planned.push(PlannedCaretEdit {
                        start_byte: offset,
                        end_byte: offset,
                        inserted_bytes: Vec::new(),
                        deleted_bytes: Vec::new(),
                        anchor_after_byte: offset,
                        cursor_after_byte: offset,
                    });
                }
            }
            self.apply_multi_caret_edits(planned);
            return;
        }

        if self.has_selection() {
            self.delete_selection();
            return;
        }
        let c = self.cursor();
        if c.column > 0 {
            let ls = self.document.buffer.line_start(c.line);
            let le = self.document.buffer.line_end(c.line);
            // Count leading whitespace (ASCII: byte offset == char offset here).
            let mut leading_ws = 0;
            while ls + leading_ws < le {
                match self.document.buffer.byte_at(ls + leading_ws) {
                    b' ' | b'\t' => leading_ws += 1,
                    _ => break,
                }
            }

            // Smart 2-space dedent
            if c.column <= leading_ws && c.column >= 2 {
                let all_spaces =
                    (0..c.column).all(|i| self.document.buffer.byte_at(ls + i) == b' ');
                if all_spaces && c.column.is_multiple_of(2) {
                    let end = TextPosition::new(c.line, c.column);
                    let start = TextPosition::new(c.line, c.column - 2);
                    self.document.delete_range(start, end);
                    self.set_cursor(start);
                    return;
                }
            }

            // Delete matching auto-close pair if cursor is between them.
            let prev = self.document.buffer.byte_at(ls + c.column - 1);
            if ls + c.column < le {
                let next = self.document.buffer.byte_at(ls + c.column);
                let lang_name = self.document.detect_language().map(|l| l.name());
                if auto_close_char(prev as char, lang_name) == Some(next as char) {
                    let start = TextPosition::new(c.line, c.column - 1);
                    let end = TextPosition::new(c.line, c.column + 1);
                    self.document.delete_range(start, end);
                    self.set_cursor(start);
                    return;
                }
            }

            let start = TextPosition::new(c.line, c.column - 1);
            let end = TextPosition::new(c.line, c.column);
            self.document.delete_range(start, end);
            self.set_cursor(start);
        } else if c.line > 0 {
            let prev_len = self.document.buffer.line_character_count(c.line - 1);
            let start = TextPosition::new(c.line - 1, prev_len);
            let end = TextPosition::new(c.line, 0);
            self.document.delete_range(start, end);
            self.set_cursor(start);
        }
    }

    pub(super) fn ctrl_backspace(&mut self) {
        if self.carets.is_multicursor() {
            let mut planned = Vec::with_capacity(self.carets.len());
            for caret in self.carets.iter().copied() {
                let selection = caret.selection;
                if !selection.is_empty() {
                    let (start, end) = selection.ordered();
                    let start_offset = self.offset_for_pos(start);
                    let deleted = self.document.text_in_range(start, end);
                    planned.push(PlannedCaretEdit {
                        start_byte: start_offset,
                        end_byte: start_offset + deleted.len(),
                        inserted_bytes: Vec::new(),
                        deleted_bytes: deleted,
                        anchor_after_byte: start_offset,
                        cursor_after_byte: start_offset,
                    });
                    continue;
                }

                let c = selection.cursor;
                if c.column == 0 && c.line == 0 {
                    let offset = self.offset_for_pos(c);
                    planned.push(PlannedCaretEdit {
                        start_byte: offset,
                        end_byte: offset,
                        inserted_bytes: Vec::new(),
                        deleted_bytes: Vec::new(),
                        anchor_after_byte: offset,
                        cursor_after_byte: offset,
                    });
                    continue;
                }
                if c.column == 0 {
                    let prev_len = self.document.buffer.line_character_count(c.line - 1);
                    let start = TextPosition::new(c.line - 1, prev_len);
                    let end = TextPosition::new(c.line, 0);
                    let start_offset = self.offset_for_pos(start);
                    let deleted = self.document.text_in_range(start, end);
                    planned.push(PlannedCaretEdit {
                        start_byte: start_offset,
                        end_byte: start_offset + deleted.len(),
                        inserted_bytes: Vec::new(),
                        deleted_bytes: deleted,
                        anchor_after_byte: start_offset,
                        cursor_after_byte: start_offset,
                    });
                    continue;
                }

                self.document
                    .buffer
                    .line_text_into(c.line, &mut self.line_text_scratch);
                let boundary = prev_word_boundary(&self.line_text_scratch, c.column);
                let start = TextPosition::new(c.line, boundary);
                let end = TextPosition::new(c.line, c.column);
                let start_offset = self.offset_for_pos(start);
                let deleted = self.document.text_in_range(start, end);
                planned.push(PlannedCaretEdit {
                    start_byte: start_offset,
                    end_byte: start_offset + deleted.len(),
                    inserted_bytes: Vec::new(),
                    deleted_bytes: deleted,
                    anchor_after_byte: start_offset,
                    cursor_after_byte: start_offset,
                });
            }
            self.apply_multi_caret_edits(planned);
            return;
        }

        if self.has_selection() {
            self.delete_selection();
            return;
        }
        let c = self.cursor();
        if c.column == 0 && c.line == 0 {
            return;
        }
        if c.column == 0 {
            let prev_len = self.document.buffer.line_character_count(c.line - 1);
            let start = TextPosition::new(c.line - 1, prev_len);
            let end = TextPosition::new(c.line, 0);
            self.document.seal_undo();
            self.document.delete_range(start, end);
            self.document.seal_undo();
            self.set_cursor(start);
            return;
        }
        self.document
            .buffer
            .line_text_into(c.line, &mut self.line_text_scratch);
        let boundary = prev_word_boundary(&self.line_text_scratch, c.column);
        let start = TextPosition::new(c.line, boundary);
        let end = TextPosition::new(c.line, c.column);
        self.document.seal_undo();
        self.document.delete_range(start, end);
        self.document.seal_undo();
        self.set_cursor(start);
    }

    pub(super) fn delete_forward(&mut self) {
        if self.carets.is_multicursor() {
            let mut planned = Vec::with_capacity(self.carets.len());
            for caret in self.carets.iter().copied() {
                let selection = caret.selection;
                if !selection.is_empty() {
                    let (start, end) = selection.ordered();
                    let start_offset = self.offset_for_pos(start);
                    let deleted = self.document.text_in_range(start, end);
                    planned.push(PlannedCaretEdit {
                        start_byte: start_offset,
                        end_byte: start_offset + deleted.len(),
                        inserted_bytes: Vec::new(),
                        deleted_bytes: deleted,
                        anchor_after_byte: start_offset,
                        cursor_after_byte: start_offset,
                    });
                    continue;
                }

                let c = selection.cursor;
                let line_len = self.document.buffer.line_character_count(c.line);
                if c.column < line_len {
                    let start = TextPosition::new(c.line, c.column);
                    let end = TextPosition::new(c.line, c.column + 1);
                    let start_offset = self.offset_for_pos(start);
                    let deleted = self.document.text_in_range(start, end);
                    planned.push(PlannedCaretEdit {
                        start_byte: start_offset,
                        end_byte: start_offset + deleted.len(),
                        inserted_bytes: Vec::new(),
                        deleted_bytes: deleted,
                        anchor_after_byte: start_offset,
                        cursor_after_byte: start_offset,
                    });
                } else if c.line + 1 < self.document.buffer.line_count() {
                    let start = TextPosition::new(c.line, c.column);
                    let end = TextPosition::new(c.line + 1, 0);
                    let start_offset = self.offset_for_pos(start);
                    let deleted = self.document.text_in_range(start, end);
                    planned.push(PlannedCaretEdit {
                        start_byte: start_offset,
                        end_byte: start_offset + deleted.len(),
                        inserted_bytes: Vec::new(),
                        deleted_bytes: deleted,
                        anchor_after_byte: start_offset,
                        cursor_after_byte: start_offset,
                    });
                } else {
                    let offset = self.offset_for_pos(c);
                    planned.push(PlannedCaretEdit {
                        start_byte: offset,
                        end_byte: offset,
                        inserted_bytes: Vec::new(),
                        deleted_bytes: Vec::new(),
                        anchor_after_byte: offset,
                        cursor_after_byte: offset,
                    });
                }
            }
            self.apply_multi_caret_edits(planned);
            return;
        }

        if self.has_selection() {
            self.delete_selection();
            return;
        }
        let c = self.cursor();
        let line_len = self.document.buffer.line_character_count(c.line);
        if c.column < line_len {
            self.document.delete_range(
                TextPosition::new(c.line, c.column),
                TextPosition::new(c.line, c.column + 1),
            );
        } else if c.line + 1 < self.document.buffer.line_count() {
            self.document.delete_range(
                TextPosition::new(c.line, c.column),
                TextPosition::new(c.line + 1, 0),
            );
        }
    }

    pub(super) fn duplicate_line(&mut self) {
        let c = self.cursor();
        self.document
            .buffer
            .line_text_into(c.line, &mut self.line_text_scratch);
        let mut new_content = vec![b'\n'];
        new_content.extend_from_slice(&self.line_text_scratch);
        let line_character_count = self.document.buffer.line_character_count(c.line);
        self.document.seal_undo();
        self.document
            .insert(c.line, line_character_count, &new_content);
        self.document.seal_undo();
        self.set_cursor(TextPosition::new(c.line + 1, c.column));
    }

    // -- commenting ---------------------------------------------------------

    pub(super) fn toggle_comment(&mut self) {
        self.comment_impl(None);
    }

    pub(super) fn set_comment(&mut self, on: bool) {
        self.comment_impl(Some(on));
    }

    /// `force`: None = toggle, Some(true) = comment, Some(false) = uncomment.
    pub(super) fn comment_impl(&mut self, force: Option<bool>) {
        let comment = match self.document.detect_language() {
            Some(lang) => lang.comment(),
            None => {
                self.set_status("No language detected for commenting".to_string());
                return;
            }
        };

        if self.carets.is_multicursor() {
            let prefix = format!("{} ", comment);
            let targeted_lines = self.targeted_lines_for_carets();
            let lines: Vec<(Vec<u8>, usize)> = targeted_lines
                .iter()
                .map(|&line| {
                    (
                        self.document.buffer.line_text(line),
                        self.document.buffer.line_start(line),
                    )
                })
                .collect();

            let all_commented = lines.iter().all(|(text, _)| {
                let trimmed = text.iter().position(|&b| b != b' ' && b != b'\t');
                match trimmed {
                    Some(pos) => text[pos..].starts_with(prefix.as_bytes()),
                    None => true,
                }
            });

            let do_uncomment = match force {
                Some(true) => false,
                Some(false) => true,
                None => all_commented,
            };

            let mut text_edits = Vec::new();
            if do_uncomment {
                for (text, line_offset) in &lines {
                    let indent_pos = text
                        .iter()
                        .position(|&b| b != b' ' && b != b'\t')
                        .unwrap_or(text.len());
                    if text[indent_pos..].starts_with(prefix.as_bytes()) {
                        text_edits.push(TextEdit {
                            start_byte: line_offset + indent_pos,
                            end_byte: line_offset + indent_pos + prefix.len(),
                            inserted_bytes: Vec::new(),
                            deleted_bytes: prefix.as_bytes().to_vec(),
                        });
                    }
                }
            } else {
                let min_indent = lines
                    .iter()
                    .filter_map(|(text, _)| text.iter().position(|&b| b != b' ' && b != b'\t'))
                    .min()
                    .unwrap_or(0);
                for (text, line_offset) in &lines {
                    let is_blank = text.iter().all(|&b| b == b' ' || b == b'\t');
                    if is_blank {
                        continue;
                    }
                    let indent_pos = text
                        .iter()
                        .position(|&b| b != b' ' && b != b'\t')
                        .unwrap_or(text.len());
                    if text[indent_pos..].starts_with(prefix.as_bytes()) {
                        continue;
                    }
                    text_edits.push(TextEdit {
                        start_byte: line_offset + min_indent,
                        end_byte: line_offset + min_indent,
                        inserted_bytes: prefix.as_bytes().to_vec(),
                        deleted_bytes: Vec::new(),
                    });
                }
            }

            self.apply_text_edits_preserving_carets(text_edits);
            return;
        }

        // Determine line range: selection or current line
        let (start_line, end_line) = if !self.has_selection() {
            (self.cursor().line, self.cursor().line)
        } else {
            let (s, e) = self.selection().ordered();
            let end = if e.column == 0 && e.line > s.line {
                e.line - 1
            } else {
                e.line
            };
            (s.line, end)
        };

        let prefix = format!("{} ", comment);

        // Pre-read all line data and byte offsets to avoid O(n²) cache rebuilds.
        // Each insert/delete invalidates the line-start cache; reading it back
        // triggers a full rebuild. By collecting everything up front we rebuild
        // the cache exactly once.
        let lines: Vec<(Vec<u8>, usize)> = (start_line..=end_line)
            .map(|i| {
                let text = self.document.buffer.line_text(i);
                let offset = self.document.buffer.line_start(i);
                (text, offset)
            })
            .collect();

        // Check if all lines are already commented
        let all_commented = lines.iter().all(|(text, _)| {
            let trimmed = text.iter().position(|&b| b != b' ' && b != b'\t');
            match trimmed {
                Some(pos) => text[pos..].starts_with(prefix.as_bytes()),
                None => true, // empty/whitespace-only lines count as commented
            }
        });

        let do_uncomment = match force {
            Some(true) => false,   // comment on → never uncomment
            Some(false) => true,   // comment off → always uncomment
            None => all_commented, // toggle
        };

        let cursor_pos = self.cursor();
        self.document.begin_undo_group();
        if do_uncomment {
            // Uncomment: remove first occurrence of "comment " from each line
            for (text, line_offset) in lines.iter().rev() {
                let indent_pos = text
                    .iter()
                    .position(|&b| b != b' ' && b != b'\t')
                    .unwrap_or(text.len());
                if text[indent_pos..].starts_with(prefix.as_bytes()) {
                    self.document.delete_at_byte(
                        line_offset + indent_pos,
                        prefix.len(),
                        cursor_pos,
                        cursor_pos,
                    );
                }
            }
        } else {
            // Comment: find minimum indent, insert comment prefix at that indent
            let min_indent = lines
                .iter()
                .filter_map(|(text, _)| text.iter().position(|&b| b != b' ' && b != b'\t'))
                .min()
                .unwrap_or(0);
            for (text, line_offset) in lines.iter().rev() {
                let is_blank = text.iter().all(|&b| b == b' ' || b == b'\t');
                if is_blank {
                    continue;
                }
                // Skip lines that are already commented
                let indent_pos = text
                    .iter()
                    .position(|&b| b != b' ' && b != b'\t')
                    .unwrap_or(text.len());
                if text[indent_pos..].starts_with(prefix.as_bytes()) {
                    continue;
                }
                self.document.insert_at_byte(
                    line_offset + min_indent,
                    prefix.as_bytes(),
                    cursor_pos,
                    cursor_pos,
                );
            }
        }
        self.document.end_undo_group();
    }

    // -- dedent -------------------------------------------------------------

    pub(super) fn dedent(&mut self) {
        if self.carets.is_multicursor() {
            let mut text_edits = Vec::new();
            for line in self.targeted_lines_for_carets() {
                let text = self.document.buffer.line_text(line);
                let line_offset = self.document.buffer.line_start(line);
                if text.starts_with(b"\t") {
                    text_edits.push(TextEdit {
                        start_byte: line_offset,
                        end_byte: line_offset + 1,
                        inserted_bytes: Vec::new(),
                        deleted_bytes: vec![b'\t'],
                    });
                } else if text.starts_with(b"  ") {
                    text_edits.push(TextEdit {
                        start_byte: line_offset,
                        end_byte: line_offset + 2,
                        inserted_bytes: Vec::new(),
                        deleted_bytes: b"  ".to_vec(),
                    });
                }
            }
            self.apply_text_edits_preserving_carets(text_edits);
            return;
        }
        let (start_line, end_line) = if !self.has_selection() {
            (self.cursor().line, self.cursor().line)
        } else {
            let (s, e) = self.selection().ordered();
            let end = if e.column == 0 && e.line > s.line {
                e.line - 1
            } else {
                e.line
            };
            (s.line, end)
        };

        // Pre-read line data to avoid O(n²) cache rebuilds
        let lines: Vec<(Vec<u8>, usize)> = (start_line..=end_line)
            .map(|i| {
                (
                    self.document.buffer.line_text(i),
                    self.document.buffer.line_start(i),
                )
            })
            .collect();

        let cursor_pos = self.cursor();
        self.document.begin_undo_group();
        let anchor_line = self.selection().anchor.line;
        let cursor_line = self.selection().cursor.line;
        let mut anchor_removed = 0usize;
        let mut cursor_removed = 0usize;
        for (idx, (text, line_offset)) in lines.iter().enumerate().rev() {
            let removed = if text.starts_with(b"\t") {
                self.document
                    .delete_at_byte(*line_offset, 1, cursor_pos, cursor_pos);
                1
            } else if text.starts_with(b"  ") {
                self.document
                    .delete_at_byte(*line_offset, 2, cursor_pos, cursor_pos);
                2
            } else {
                0
            };
            let line_idx = start_line + idx;
            if line_idx == cursor_line {
                cursor_removed = removed;
            }
            if line_idx == anchor_line {
                anchor_removed = removed;
            }
        }
        self.document.end_undo_group();

        // Preserve the selection so the user can dedent multiple times.
        self.carets.primary_mut().selection.cursor.column =
            self.cursor().column.saturating_sub(cursor_removed);
        self.carets.primary_mut().selection.anchor.column = self
            .selection()
            .anchor
            .column
            .saturating_sub(anchor_removed);
    }


    // -- undo/redo ----------------------------------------------------------

    pub(super) fn undo(&mut self) {
        if let Some(carets) = self.document.undo() {
            self.restore_carets(carets);
        }
    }

    pub(super) fn redo(&mut self) {
        if let Some(carets) = self.document.redo() {
            self.restore_carets(carets);
        }
    }

    // -- file I/O -----------------------------------------------------------

    pub(super) fn strip_trailing_whitespace(&mut self) {
        let line_count = self.document.buffer.line_count();
        self.document.seal_undo();
        for line_idx in (0..line_count).rev() {
            self.document
                .buffer
                .line_text_into(line_idx, &mut self.line_text_scratch);
            let trimmed_len = self
                .line_text_scratch
                .iter()
                .rposition(|&b| b != b' ' && b != b'\t')
                .map(|i| i + 1)
                .unwrap_or(0);
            let char_len = crate::buffer::character_count(&self.line_text_scratch);
            let trim_char_len =
                crate::buffer::character_count(&self.line_text_scratch[..trimmed_len]);
            if trim_char_len < char_len {
                self.document.delete_range(
                    TextPosition::new(line_idx, trim_char_len),
                    TextPosition::new(line_idx, char_len),
                );
            }
        }
        self.document.seal_undo();
        // Adjust cursor if past end of line
        let c = self.cursor();
        let line_len = self.document.buffer.line_character_count(c.line);
        if c.column > line_len {
            self.set_cursor(TextPosition::new(c.line, line_len));
        }
    }

    pub(super) fn tabs_to_spaces(&mut self) {
        let line_count = self.document.buffer.line_count();
        self.document.seal_undo();
        for line_idx in 0..line_count {
            self.document
                .buffer
                .line_text_into(line_idx, &mut self.line_text_scratch);
            if !self.line_text_scratch.contains(&b'\t') {
                continue;
            }
            let mut new_text = Vec::with_capacity(self.line_text_scratch.len() * 2);
            for &b in &self.line_text_scratch {
                if b == b'\t' {
                    new_text.extend_from_slice(b"  ");
                } else {
                    new_text.push(b);
                }
            }
            let char_len = crate::buffer::character_count(&self.line_text_scratch);
            self.document.delete_range(
                TextPosition::new(line_idx, 0),
                TextPosition::new(line_idx, char_len),
            );
            self.document.insert(line_idx, 0, &new_text);
        }
        self.document.seal_undo();
        let c = self.cursor();
        let line_len = self.document.buffer.line_character_count(c.line);
        if c.column > line_len {
            self.set_cursor(TextPosition::new(c.line, line_len));
        }
        self.set_status("Converted tabs to spaces".to_string());
    }

    pub(super) fn spaces_to_tabs(&mut self) {
        let line_count = self.document.buffer.line_count();
        self.document.seal_undo();
        for line_idx in 0..line_count {
            self.document
                .buffer
                .line_text_into(line_idx, &mut self.line_text_scratch);
            // Only convert leading whitespace (indentation)
            let mut new_text = Vec::with_capacity(self.line_text_scratch.len());
            let mut i = 0;
            while i < self.line_text_scratch.len() {
                if self.line_text_scratch[i] == b'\t' {
                    new_text.push(b'\t');
                    i += 1;
                } else if i + 1 < self.line_text_scratch.len()
                    && self.line_text_scratch[i] == b' '
                    && self.line_text_scratch[i + 1] == b' '
                {
                    new_text.push(b'\t');
                    i += 2;
                } else {
                    // End of leading whitespace (or lone space): copy rest verbatim
                    new_text.extend_from_slice(&self.line_text_scratch[i..]);
                    break;
                }
            }
            if new_text[..] == self.line_text_scratch[..] {
                continue;
            }
            let char_len = crate::buffer::character_count(&self.line_text_scratch);
            self.document.delete_range(
                TextPosition::new(line_idx, 0),
                TextPosition::new(line_idx, char_len),
            );
            self.document.insert(line_idx, 0, &new_text);
        }
        self.document.seal_undo();
        let c = self.cursor();
        let line_len = self.document.buffer.line_character_count(c.line);
        if c.column > line_len {
            self.set_cursor(TextPosition::new(c.line, line_len));
        }
        self.set_status("Converted spaces to tabs".to_string());
    }
}
