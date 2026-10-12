//! Edit planning shared by single- and multi-caret commands: translating
//! carets to byte offsets, applying sorted `TextEdit` batches, and mapping caret
//! positions through them.

use crate::document::TextEdit;
use crate::selection::{Selection, TextPosition};

use super::Editor;

/// One caret's edit expressed in byte offsets, before the batch is applied.
pub(super) struct PlannedCaretEdit {
    pub(super) start_byte: usize,
    pub(super) end_byte: usize,
    pub(super) inserted_bytes: Vec<u8>,
    pub(super) deleted_bytes: Vec<u8>,
    pub(super) anchor_after_byte: usize,
    pub(super) cursor_after_byte: usize,
}

impl Editor {
    pub(super) fn offset_for_pos(&self, pos: TextPosition) -> usize {
        self.document
            .buffer
            .position_to_byte_offset(pos.line, pos.column)
    }

    pub(super) fn map_offset_after_edits(offset: usize, edits: &[TextEdit]) -> usize {
        let mut delta: isize = 0;
        for edit in edits {
            let mapped_start = (edit.start_byte as isize + delta) as usize;
            if offset < edit.start_byte {
                break;
            }
            if offset <= edit.end_byte {
                return mapped_start + edit.inserted_bytes.len();
            }
            delta +=
                edit.inserted_bytes.len() as isize - (edit.end_byte - edit.start_byte) as isize;
        }
        (offset as isize + delta) as usize
    }

    pub(super) fn apply_text_edits_with_offsets(
        &mut self,
        text_edits: Vec<TextEdit>,
        final_offsets: Vec<(usize, usize)>,
    ) {
        let before = self.carets.snapshot();
        let after =
            self.document
                .apply_batch(&text_edits, &before, &final_offsets, self.carets.primary_index());
        self.restore_carets(after);
    }

    pub(super) fn apply_text_edits_preserving_carets(&mut self, mut text_edits: Vec<TextEdit>) {
        if text_edits.is_empty() {
            return;
        }
        text_edits.sort_by_key(|edit| edit.start_byte);
        let before = self.carets.snapshot();
        let final_offsets: Vec<(usize, usize)> = before
            .selections
            .iter()
            .map(|selection| {
                let anchor = Self::map_offset_after_edits(
                    self.offset_for_pos(selection.anchor),
                    &text_edits,
                );
                let cursor = Self::map_offset_after_edits(
                    self.offset_for_pos(selection.cursor),
                    &text_edits,
                );
                (anchor, cursor)
            })
            .collect();
        self.apply_text_edits_with_offsets(text_edits, final_offsets);
    }

    pub(super) fn apply_multi_caret_edits(&mut self, planned: Vec<PlannedCaretEdit>) {
        let mut final_offsets = Vec::with_capacity(planned.len());
        let mut delta: isize = 0;
        let mut text_edits = Vec::with_capacity(planned.len());

        for edit in planned {
            final_offsets.push((
                (edit.anchor_after_byte as isize + delta) as usize,
                (edit.cursor_after_byte as isize + delta) as usize,
            ));
            delta +=
                edit.inserted_bytes.len() as isize - (edit.end_byte - edit.start_byte) as isize;
            text_edits.push(TextEdit {
                start_byte: edit.start_byte,
                end_byte: edit.end_byte,
                inserted_bytes: edit.inserted_bytes,
                deleted_bytes: edit.deleted_bytes,
            });
        }

        self.apply_text_edits_with_offsets(text_edits, final_offsets);
    }

    pub(super) fn line_range_for_selection(selection: Selection) -> (usize, usize) {
        let (start, end) = selection.ordered();
        let end_line = if !selection.is_empty() && end.column == 0 && end.line > start.line {
            end.line - 1
        } else {
            end.line
        };
        (start.line, end_line)
    }

    pub(super) fn targeted_lines_for_carets(&self) -> Vec<usize> {
        let mut lines = std::collections::BTreeSet::new();
        for caret in self.carets.iter() {
            let (start_line, end_line) = Self::line_range_for_selection(caret.selection);
            for line in start_line..=end_line {
                lines.insert(line);
            }
        }
        lines.into_iter().collect()
    }
}
