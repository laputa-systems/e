//! Copy, cut, and paste, including per-caret fragments for multi-caret edits.

use super::*;

impl Editor {
    // -- clipboard ----------------------------------------------------------

    /// Copy the selection(s) to the clipboard. Returns false when the system
    /// clipboard could not be written, so `cut` keeps the text instead of
    /// deleting something that was not copied.
    pub(super) fn copy(&mut self) -> bool {
        if self.carets.is_multicursor() {
            let fragments: Vec<String> = self
                .carets
                .iter()
                .filter_map(|caret| {
                    if caret.selection.is_empty() {
                        return None;
                    }
                    let (start, end) = caret.selection.ordered();
                    let text = self.document.text_in_range(start, end);
                    Some(String::from_utf8_lossy(&text).to_string())
                })
                .collect();
            if fragments.is_empty() {
                return true;
            }
            let copied = if fragments.len() == self.carets.len() {
                self.clipboard.copy_multi(&fragments)
            } else {
                self.clipboard.copy(&fragments.join("\n"))
            };
            return self.report_clipboard(copied);
        }

        if !self.has_selection() {
            return true;
        }
        let (start, end) = self.selection().ordered();
        let text = self.document.text_in_range(start, end);
        let s = String::from_utf8_lossy(&text).to_string();
        let copied = self.clipboard.copy(&s);
        self.report_clipboard(copied)
    }

    /// Show a clipboard failure in the status line. Returns true on success.
    pub(super) fn report_clipboard(&mut self, result: Result<(), String>) -> bool {
        match result {
            Ok(()) => true,
            Err(e) => {
                self.set_status(format!("Clipboard error: {e}"));
                false
            }
        }
    }

    pub(super) fn cut(&mut self) {
        if self.carets.is_multicursor() {
            let fragments: Vec<String> = self
                .carets
                .iter()
                .filter_map(|caret| {
                    if caret.selection.is_empty() {
                        return None;
                    }
                    let (start, end) = caret.selection.ordered();
                    let text = self.document.text_in_range(start, end);
                    Some(String::from_utf8_lossy(&text).to_string())
                })
                .collect();
            if fragments.is_empty() {
                return;
            }
            let copied = if fragments.len() == self.carets.len() {
                self.clipboard.copy_multi(&fragments)
            } else {
                self.clipboard.copy(&fragments.join("\n"))
            };
            if !self.report_clipboard(copied) {
                return;
            }
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

        if !self.has_selection() {
            return;
        }
        if self.copy() {
            self.delete_selection();
        }
    }

    pub(super) fn paste(&mut self) {
        let contents = match self.clipboard.paste_contents() {
            Ok(contents) => contents,
            Err(e) => {
                self.set_status(format!("Clipboard error: {e}"));
                return;
            }
        };
        if self.carets.is_multicursor() {
            let fragment_texts = contents
                .fragments
                .filter(|fragments| fragments.len() == self.carets.len());
            let mut planned = Vec::with_capacity(self.carets.len());
            let carets = self.carets.as_slice().to_vec();
            for (idx, caret) in carets.into_iter().enumerate() {
                let text = fragment_texts
                    .as_ref()
                    .map(|fragments| fragments[idx].as_str())
                    .unwrap_or(contents.text.as_str());
                let selection = caret.selection;
                let (start, end) = selection.ordered();
                let mut insert_pos = if selection.is_empty() {
                    selection.cursor
                } else {
                    start
                };
                let mut start_offset = self.offset_for_pos(start);
                let mut deleted = if selection.is_empty() {
                    Vec::new()
                } else {
                    self.document.text_in_range(start, end)
                };

                if selection.is_empty() && text.contains('\n') {
                    self.document
                        .buffer
                        .line_text_into(insert_pos.line, &mut self.line_text_scratch);
                    let is_blank = self
                        .line_text_scratch
                        .iter()
                        .all(|&b| b == b' ' || b == b'\t');
                    if is_blank {
                        let char_len = crate::buffer::character_count(&self.line_text_scratch);
                        if char_len > 0 {
                            deleted = self.document.text_in_range(
                                TextPosition::new(insert_pos.line, 0),
                                TextPosition::new(insert_pos.line, char_len),
                            );
                            insert_pos = TextPosition::new(insert_pos.line, 0);
                            start_offset = self.offset_for_pos(insert_pos);
                        }
                    }
                }

                let insert_text = self.reindent_paste_for_cursor(text, insert_pos);
                let insert_len = insert_text.len();
                planned.push(PlannedCaretEdit {
                    start_byte: start_offset,
                    end_byte: start_offset + deleted.len(),
                    inserted_bytes: insert_text.into_bytes(),
                    deleted_bytes: deleted,
                    anchor_after_byte: start_offset + insert_len,
                    cursor_after_byte: start_offset + insert_len,
                });
            }
            self.apply_multi_caret_edits(planned);
            return;
        }

        self.paste_text(&contents.text);
    }

    pub(super) fn paste_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if self.has_selection() {
            self.delete_selection();
        }
        // Multi-line paste: clear any auto-indent on a blank line and anchor at
        // column 0 so the pasted text's own indentation is used as-is.
        // Indentation is a copy-time property, not a paste-time one.
        if text.contains('\n') {
            let c = self.cursor();
            self.document
                .buffer
                .line_text_into(c.line, &mut self.line_text_scratch);
            let is_blank = self
                .line_text_scratch
                .iter()
                .all(|&b| b == b' ' || b == b'\t');
            if is_blank {
                let char_len = crate::buffer::character_count(&self.line_text_scratch);
                if char_len > 0 {
                    self.document.delete_range(
                        TextPosition::new(c.line, 0),
                        TextPosition::new(c.line, char_len),
                    );
                }
                self.set_cursor(TextPosition::new(c.line, 0));
            }
        }
        let text = self.reindent_paste(text);
        self.document.seal_undo();
        let pos = self
            .document
            .insert(self.cursor().line, self.cursor().column, text.as_bytes());
        self.document.seal_undo();
        self.set_cursor(pos);
    }

    /// Re-indent pasted multi-line text to match the current cursor's indent level.
    ///
    /// Continuation lines (lines[1..]) keep their indentation relative to lines[0]:
    /// each line's new indent = cursor_col + original_indent_of_that_line.
    ///
    /// For multi-line paste on a blank line, `paste_text` already resets the cursor
    /// to column 0, so this function is effectively a no-op in that case.
    pub(super) fn reindent_paste(&mut self, text: &str) -> String {
        self.reindent_paste_for_cursor(text, self.cursor())
    }

    pub(super) fn reindent_paste_for_cursor(&mut self, text: &str, cursor: TextPosition) -> String {
        let lines: Vec<&str> = text.split('\n').collect();
        if lines.len() < 2 {
            return text.to_string();
        }

        // Get the indent of the current line at cursor
        self.document
            .buffer
            .line_text_into(cursor.line, &mut self.line_text_scratch);
        let cur_indent: usize = self
            .line_text_scratch
            .iter()
            .take_while(|&&b| b == b' ' || b == b'\t')
            .count();

        // If lines[0] has content it lands at cursor.column; otherwise the pasted
        // block starts on a new line relative to the current line's indent.
        let target_base = if !lines[0].trim().is_empty() {
            cursor.column
        } else {
            cur_indent
        };

        // Pasting at column 0: the text already carries correct indentation.
        if target_base == 0 {
            return text.to_string();
        }

        let mut result = String::with_capacity(text.len());
        result.push_str(lines[0]);
        for line in &lines[1..] {
            result.push('\n');
            if line.trim().is_empty() {
                result.push_str(line);
            } else {
                // Preserve relative indentation: new position = cursor column + original indent.
                let ik = line.len() - line.trim_start().len();
                let new_indent = target_base + ik;
                for _ in 0..new_indent {
                    result.push(' ');
                }
                result.push_str(line.trim_start());
            }
        }
        result
    }
}
