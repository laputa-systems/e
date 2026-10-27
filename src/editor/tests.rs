use super::*;
use crate::input::{Key, MouseButton, MouseEvent};
use crate::selection::Caret;

fn ed(text: &str) -> Editor {
    ed_impl(text, None)
}

fn ed_named(text: &str, name: &str) -> Editor {
    ed_impl(text, Some(name.to_string()))
}

fn ed_impl(text: &str, file_path: Option<String>) -> Editor {
    let file_modification_time = file_path
        .as_deref()
        .and_then(|name| crate::file_io::file_modification_time(std::path::Path::new(name)));
    Editor::from_parts(
        Document::new(text.as_bytes().to_vec(), file_path),
        TextPosition::zero(),
        Viewport::new(80, 24),
        Clipboard::internal_only(),
        KeybindingTable::with_defaults(),
        false,
        file_modification_time,
    )
}

impl Editor {
    fn test_text(&self) -> String {
        String::from_utf8_lossy(&self.document.buffer.contents()).to_string()
    }
}

fn selection(e: &Editor) -> Selection {
    e.selection()
}

fn set_sel(e: &mut Editor, selection: Selection) {
    e.set_selection(selection);
}


#[test]
fn test_move_up_down_with_desired_col_stickiness() {
    let mut e = ed("short\nlonger line here\nhi");
    e.set_cursor(TextPosition::new(1, 15));
    e.move_up();
    assert_eq!(e.cursor(), TextPosition::new(0, 5));
    e.move_down();
    assert_eq!(e.cursor(), TextPosition::new(1, 15));
    e.move_down();
    assert_eq!(e.cursor(), TextPosition::new(2, 2));
}

#[test]
fn test_move_up_at_top() {
    let mut e = ed("hello");
    e.move_up();
    assert_eq!(e.cursor(), TextPosition::new(0, 0));
}

#[test]
fn test_move_down_at_bottom() {
    let mut e = ed("hello");
    e.move_down();
    assert_eq!(e.cursor(), TextPosition::new(0, 0));
}

#[test]
fn test_move_left_wraps_to_prev_line() {
    let mut e = ed("abc\ndef");
    e.set_cursor(TextPosition::new(1, 0));
    e.move_left();
    assert_eq!(e.cursor(), TextPosition::new(0, 3));
}

#[test]
fn test_move_right_wraps_to_next_line() {
    let mut e = ed("abc\ndef");
    e.set_cursor(TextPosition::new(0, 3));
    e.move_right();
    assert_eq!(e.cursor(), TextPosition::new(1, 0));
}

#[test]
fn test_move_left_collapses_selection() {
    let mut e = ed("hello world");
    set_sel(
        &mut e,
        Selection {
            anchor: TextPosition::new(0, 2),
            cursor: TextPosition::new(0, 7),
        },
    );
    e.move_left();
    assert_eq!(e.cursor(), TextPosition::new(0, 2));
    assert!(selection(&e).is_empty());
}

#[test]
fn test_move_right_collapses_selection() {
    let mut e = ed("hello world");
    set_sel(
        &mut e,
        Selection {
            anchor: TextPosition::new(0, 2),
            cursor: TextPosition::new(0, 7),
        },
    );
    e.move_right();
    assert_eq!(e.cursor(), TextPosition::new(0, 7));
    assert!(selection(&e).is_empty());
}

#[test]
fn test_home_end() {
    let mut e = ed("hello world");
    e.set_cursor(TextPosition::new(0, 5));
    e.move_home();
    assert_eq!(e.cursor(), TextPosition::new(0, 0));
    e.move_end();
    assert_eq!(e.cursor(), TextPosition::new(0, 11));
}

#[test]
fn test_page_up_down() {
    let text = (0..50)
        .map(|i| format!("line {}", i))
        .collect::<Vec<_>>()
        .join("\n");
    let mut e = ed(&text);
    e.set_cursor(TextPosition::new(25, 0));
    e.page_up();
    assert_eq!(e.cursor().line, 3);
    e.page_down();
    assert_eq!(e.cursor().line, 25);
}

#[test]
fn test_indent_snap_left_right() {
    let mut e = ed("    hello");
    e.set_cursor(TextPosition::new(0, 4));
    e.move_left();
    assert_eq!(e.cursor().column, 2);
    e.move_right();
    assert_eq!(e.cursor().column, 4);
}

#[test]
fn test_move_left_at_origin() {
    let mut e = ed("hello");
    e.move_left();
    assert_eq!(e.cursor(), TextPosition::new(0, 0));
}

#[test]
fn test_move_right_at_end_of_last_line() {
    let mut e = ed("hello");
    e.set_cursor(TextPosition::new(0, 5));
    e.move_right();
    assert_eq!(e.cursor(), TextPosition::new(0, 5));
}


#[test]
fn test_shift_arrow_extends_selection() {
    let mut e = ed("hello");
    e.move_right_extend();
    e.move_right_extend();
    assert_eq!(selection(&e).anchor, TextPosition::new(0, 0));
    assert_eq!(selection(&e).cursor, TextPosition::new(0, 2));
    assert!(!selection(&e).is_empty());
}

#[test]
fn test_select_all() {
    let mut e = ed("hello\nworld");
    e.select_all();
    let (start, end) = selection(&e).ordered();
    assert_eq!(start, TextPosition::new(0, 0));
    assert_eq!(end, TextPosition::new(1, 5));
}

#[test]
fn test_select_word_at() {
    let mut e = ed("hello world");
    e.select_word_at(TextPosition::new(0, 7));
    let (start, end) = selection(&e).ordered();
    assert_eq!(start, TextPosition::new(0, 6));
    assert_eq!(end, TextPosition::new(0, 11));
}

#[test]
fn test_select_line_at() {
    let mut e = ed("hello\nworld\nfoo");
    e.select_line_at(1);
    let (start, end) = selection(&e).ordered();
    assert_eq!(start, TextPosition::new(1, 0));
    assert_eq!(end, TextPosition::new(2, 0));
}

#[test]
fn test_select_line_at_last_line() {
    let mut e = ed("hello\nworld");
    e.select_line_at(1);
    let (start, end) = selection(&e).ordered();
    assert_eq!(start, TextPosition::new(1, 0));
    assert_eq!(end, TextPosition::new(1, 5));
}

#[test]
fn test_select_above_below() {
    let mut e = ed("hello\nworld\nfoo");
    e.set_cursor(TextPosition::new(1, 3));
    e.select_above();
    assert_eq!(selection(&e).cursor, TextPosition::new(0, 0));
    assert_eq!(selection(&e).anchor, TextPosition::new(1, 3));

    e.set_cursor(TextPosition::new(1, 3));
    e.select_below();
    assert_eq!(selection(&e).cursor, TextPosition::new(2, 3));
    assert_eq!(selection(&e).anchor, TextPosition::new(1, 3));
}

#[test]
fn test_delete_selection() {
    let mut e = ed("hello world");
    set_sel(
        &mut e,
        Selection {
            anchor: TextPosition::new(0, 5),
            cursor: TextPosition::new(0, 11),
        },
    );
    e.delete_selection();
    assert_eq!(e.test_text(), "hello");
}

#[test]
fn test_clear_selection() {
    let mut e = ed("hello");
    set_sel(
        &mut e,
        Selection {
            anchor: TextPosition::new(0, 0),
            cursor: TextPosition::new(0, 5),
        },
    );
    e.clear_selection();
    assert!(selection(&e).is_empty());
    assert_eq!(e.cursor(), TextPosition::new(0, 5));
}

#[test]
fn test_shift_up_down_extend() {
    let mut e = ed("aaa\nbbb\nccc");
    e.set_cursor(TextPosition::new(1, 1));
    e.move_up_extend();
    assert_eq!(selection(&e).anchor, TextPosition::new(1, 1));
    assert_eq!(selection(&e).cursor, TextPosition::new(0, 1));
    e.move_down_extend();
    assert_eq!(selection(&e).cursor, TextPosition::new(1, 1));
    e.move_down_extend();
    assert_eq!(selection(&e).cursor, TextPosition::new(2, 1));
}

#[test]
fn test_shift_left_right_extend() {
    let mut e = ed("hello");
    e.set_cursor(TextPosition::new(0, 2));
    e.move_left_extend();
    assert_eq!(selection(&e).cursor, TextPosition::new(0, 1));
    e.move_right_extend();
    assert_eq!(selection(&e).cursor, TextPosition::new(0, 2));
}


#[test]
fn test_insert_char() {
    let mut e = ed("hllo");
    e.set_cursor(TextPosition::new(0, 1));
    e.insert_char('e');
    assert_eq!(e.test_text(), "hello");
    assert_eq!(e.cursor(), TextPosition::new(0, 2));
}

#[test]
fn test_insert_char_replaces_selection() {
    let mut e = ed("hello world");
    set_sel(
        &mut e,
        Selection {
            anchor: TextPosition::new(0, 5),
            cursor: TextPosition::new(0, 11),
        },
    );
    e.insert_char('!');
    assert_eq!(e.test_text(), "hello!");
}

#[test]
fn test_insert_tab_spaces() {
    let mut e = ed_named("hello", "test.rs");
    e.insert_tab();
    assert_eq!(e.test_text(), "  hello");
}

#[test]
fn test_insert_tab_actual_tab_for_c_file() {
    let mut e = ed_named("hello", "test.c");
    e.insert_tab();
    assert_eq!(e.test_text(), "\thello");
}

#[test]
fn test_tab_indents_selection() {
    let mut e = ed_named("aaa\nbbb\nccc", "test.rs");
    set_sel(
        &mut e,
        Selection {
            anchor: TextPosition::new(0, 0),
            cursor: TextPosition::new(2, 3),
        },
    );
    e.insert_tab();
    assert_eq!(e.test_text(), "  aaa\n  bbb\n  ccc");
}

#[test]
fn test_tab_indents_selection_preserves_selection() {
    let mut e = ed_named("aaa\nbbb\nccc", "test.rs");
    set_sel(
        &mut e,
        Selection {
            anchor: TextPosition::new(0, 0),
            cursor: TextPosition::new(2, 3),
        },
    );
    e.insert_tab();
    assert!(!selection(&e).is_empty());
    assert_eq!(selection(&e).anchor, TextPosition::new(0, 2));
    assert_eq!(selection(&e).cursor, TextPosition::new(2, 5));
}

#[test]
fn test_insert_newline_with_auto_indent() {
    let mut e = ed("  hello");
    e.set_cursor(TextPosition::new(0, 7));
    e.insert_newline();
    assert_eq!(e.test_text(), "  hello\n  ");
    assert_eq!(e.cursor(), TextPosition::new(1, 2));
}

#[test]
fn test_insert_newline_replaces_selection() {
    let mut e = ed("hello world");
    set_sel(
        &mut e,
        Selection {
            anchor: TextPosition::new(0, 5),
            cursor: TextPosition::new(0, 11),
        },
    );
    e.insert_newline();
    assert_eq!(e.test_text(), "hello\n");
}

#[test]
fn test_insert_newline_between_braces() {
    let mut e = ed("{}");
    e.set_cursor(TextPosition::new(0, 1));
    e.insert_newline();
    assert_eq!(e.test_text(), "{\n  \n}");
    assert_eq!(e.cursor(), TextPosition::new(1, 2));
}

#[test]
fn test_insert_newline_between_parens() {
    let mut e = ed("()");
    e.set_cursor(TextPosition::new(0, 1));
    e.insert_newline();
    assert_eq!(e.test_text(), "(\n  \n)");
    assert_eq!(e.cursor(), TextPosition::new(1, 2));
}

#[test]
fn test_insert_newline_between_brackets() {
    let mut e = ed("[]");
    e.set_cursor(TextPosition::new(0, 1));
    e.insert_newline();
    assert_eq!(e.test_text(), "[\n  \n]");
    assert_eq!(e.cursor(), TextPosition::new(1, 2));
}

#[test]
fn test_insert_newline_between_braces_preserves_indent() {
    let mut e = ed("  {}");
    e.set_cursor(TextPosition::new(0, 3));
    e.insert_newline();
    assert_eq!(e.test_text(), "  {\n    \n  }");
    assert_eq!(e.cursor(), TextPosition::new(1, 4));
}

#[test]
fn test_insert_newline_not_between_pair_no_split() {
    let mut e = ed("{hello}");
    e.set_cursor(TextPosition::new(0, 1));
    e.insert_newline();
    assert_eq!(e.test_text(), "{\nhello}");
    assert_eq!(e.cursor(), TextPosition::new(1, 0));
}

#[test]
fn test_backspace_basic() {
    let mut e = ed("hello");
    e.set_cursor(TextPosition::new(0, 5));
    e.backspace();
    assert_eq!(e.test_text(), "hell");
}

#[test]
fn test_backspace_joins_lines() {
    let mut e = ed("hello\nworld");
    e.set_cursor(TextPosition::new(1, 0));
    e.backspace();
    assert_eq!(e.test_text(), "helloworld");
}

#[test]
fn test_backspace_indent_snap() {
    let mut e = ed("    x");
    e.set_cursor(TextPosition::new(0, 4));
    e.backspace();
    assert_eq!(e.test_text(), "  x");
}

#[test]
fn test_backspace_deletes_selection() {
    let mut e = ed("hello world");
    set_sel(
        &mut e,
        Selection {
            anchor: TextPosition::new(0, 5),
            cursor: TextPosition::new(0, 11),
        },
    );
    e.backspace();
    assert_eq!(e.test_text(), "hello");
}

#[test]
fn test_backspace_at_origin_noop() {
    let mut e = ed("hello");
    e.backspace();
    assert_eq!(e.test_text(), "hello");
}

#[test]
fn test_delete_forward() {
    let mut e = ed("hello");
    e.set_cursor(TextPosition::new(0, 0));
    e.delete_forward();
    assert_eq!(e.test_text(), "ello");
}

#[test]
fn test_delete_forward_joins_lines() {
    let mut e = ed("hello\nworld");
    e.set_cursor(TextPosition::new(0, 5));
    e.delete_forward();
    assert_eq!(e.test_text(), "helloworld");
}

#[test]
fn test_delete_forward_with_selection() {
    let mut e = ed("hello world");
    set_sel(
        &mut e,
        Selection {
            anchor: TextPosition::new(0, 0),
            cursor: TextPosition::new(0, 5),
        },
    );
    e.delete_forward();
    assert_eq!(e.test_text(), " world");
}

#[test]
fn test_ctrl_backspace_word_delete() {
    let mut e = ed("hello world");
    e.set_cursor(TextPosition::new(0, 11));
    e.ctrl_backspace();
    assert_eq!(e.test_text(), "hello ");
}

#[test]
fn test_ctrl_backspace_at_line_start() {
    let mut e = ed("hello\nworld");
    e.set_cursor(TextPosition::new(1, 0));
    e.ctrl_backspace();
    assert_eq!(e.test_text(), "helloworld");
}

#[test]
fn test_ctrl_backspace_at_origin() {
    let mut e = ed("hello");
    e.ctrl_backspace();
    assert_eq!(e.test_text(), "hello");
}

#[test]
fn test_ctrl_backspace_with_selection() {
    let mut e = ed("hello world");
    set_sel(
        &mut e,
        Selection {
            anchor: TextPosition::new(0, 0),
            cursor: TextPosition::new(0, 5),
        },
    );
    e.ctrl_backspace();
    assert_eq!(e.test_text(), " world");
}

#[test]
fn test_duplicate_line() {
    let mut e = ed("hello\nworld");
    e.set_cursor(TextPosition::new(0, 2));
    e.duplicate_line();
    assert_eq!(e.test_text(), "hello\nhello\nworld");
    assert_eq!(e.cursor(), TextPosition::new(1, 2));
}


#[test]
fn test_find_highlights_smart_case() {
    let mut e = ed("Hello hello HELLO");
    e.update_find_highlights("hello");
    assert_eq!(e.find.matches.len(), 3);
}

#[test]
fn test_find_case_sensitive() {
    let mut e = ed("Hello hello HELLO");
    e.update_find_highlights("Hello");
    assert_eq!(e.find.matches.len(), 1);
}

#[test]
fn test_find_invalid_regex() {
    let mut e = ed("hello [world");
    e.update_find_highlights("[invalid");
    assert!(e.find.matches.is_empty());
}

#[test]
fn test_find_empty_pattern() {
    let mut e = ed("hello");
    e.update_find_highlights("");
    assert!(e.find.matches.is_empty());
}

#[test]
fn test_find_next_wraps_around() {
    let mut e = ed("aa bb aa");
    e.update_find_highlights("aa");
    assert_eq!(e.find.matches.len(), 2);
    e.find.active = true;
    e.set_cursor(TextPosition::new(0, 8));
    e.find_next();
    assert_eq!(
        e.find.current,
        Some((TextPosition::new(0, 0), TextPosition::new(0, 2)))
    );
}

#[test]
fn test_find_prev_wraps_around() {
    let mut e = ed("aa bb aa");
    e.update_find_highlights("aa");
    e.find.active = true;
    e.set_cursor(TextPosition::new(0, 0));
    e.find_prev();
    assert_eq!(
        e.find.current,
        Some((TextPosition::new(0, 6), TextPosition::new(0, 8)))
    );
}

#[test]
fn test_find_next_from_submit() {
    let mut e = ed("hello world hello");
    e.find_next_from_submit("hello");
    assert!(e.find.active);
    assert_eq!(e.find.matches.len(), 2);
    assert!(e.find.current.is_some());
}

#[test]
fn test_find_next_from_submit_no_matches() {
    let mut e = ed("hello world");
    e.find_next_from_submit("xyz");
    assert!(!e.find.active);
    assert!(e.status_message.contains("no matches"));
}

#[test]
fn test_exit_find_mode_selects_match() {
    let mut e = ed("hello world hello");
    e.find_next_from_submit("hello");
    e.exit_find_mode();
    assert!(!e.find.active);
    assert!(e.find.matches.is_empty());
    assert!(!selection(&e).is_empty());
}

#[test]
fn test_replace_all_whole_file() {
    let mut e = ed("foo bar foo");
    e.replace_all("foo", "baz");
    assert_eq!(e.test_text(), "baz bar baz");
    assert!(e.status_message.contains("2"));
}

#[test]
fn test_replace_all_in_selection() {
    let mut e = ed("foo bar foo");
    set_sel(
        &mut e,
        Selection {
            anchor: TextPosition::new(0, 0),
            cursor: TextPosition::new(0, 3),
        },
    );
    e.replace_all("foo", "baz");
    assert_eq!(e.test_text(), "baz bar foo");
}

#[test]
fn test_replace_all_no_matches() {
    let mut e = ed("hello world");
    e.replace_all("xyz", "abc");
    assert!(e.status_message.contains("0"));
}

#[test]
fn test_replace_all_invalid_regex() {
    let mut e = ed("hello");
    e.replace_all("[invalid", "x");
    assert!(e.status_message.contains("Invalid regex"));
}


#[test]
fn test_goto_line_in_range() {
    let text = (0..50)
        .map(|i| format!("line {}", i))
        .collect::<Vec<_>>()
        .join("\n");
    let mut e = ed(&text);
    e.goto_line(25);
    assert_eq!(e.cursor().line, 24);
}

#[test]
fn test_goto_line_zero() {
    let mut e = ed("hello\nworld");
    e.goto_line(0);
    assert_eq!(e.cursor().line, 0);
}

#[test]
fn test_goto_line_beyond_end() {
    let mut e = ed("hello\nworld");
    e.goto_line(999);
    assert_eq!(e.cursor().line, 1);
}

#[test]
fn test_goto_top_end() {
    let mut e = ed("hello\nworld\nfoo");
    e.set_cursor(TextPosition::new(1, 2));
    e.goto_top();
    assert_eq!(e.cursor(), TextPosition::new(0, 0));
    e.goto_end();
    assert_eq!(e.cursor(), TextPosition::new(2, 3));
}

#[test]
fn test_kill_line_middle() {
    let mut e = ed("aaa\nbbb\nccc");
    e.set_cursor(TextPosition::new(1, 1));
    e.kill_line();
    assert_eq!(e.test_text(), "aaa\nccc");
}

#[test]
fn test_kill_line_last() {
    let mut e = ed("aaa\nbbb");
    e.set_cursor(TextPosition::new(1, 0));
    e.kill_line();
    assert_eq!(e.test_text(), "aaa\n");
}

#[test]
fn test_execute_command_goto() {
    let mut e = ed("aaa\nbbb\nccc");
    e.execute_command("goto 2");
    assert_eq!(e.cursor().line, 1);
}

#[test]
fn test_execute_command_ruler_toggle() {
    let mut e = ed("hello");
    assert!(e.line_numbers_visible);
    e.execute_command("ruler");
    assert!(!e.line_numbers_visible);
    e.execute_command("ruler");
    assert!(e.line_numbers_visible);
}

#[test]
fn test_execute_command_quit() {
    let mut e = ed("hello");
    e.execute_command("quit");
    assert!(!e.running);
}

#[test]
fn test_execute_command_unknown() {
    let mut e = ed("hello");
    e.execute_command("foobar");
    assert!(e.status_message.contains("Unknown"));
}

#[test]
fn test_execute_command_replaceall() {
    let mut e = ed("foo bar foo");
    e.execute_command("replaceall foo baz");
    assert_eq!(e.test_text(), "baz bar baz");
}

#[test]
fn test_execute_command_comment() {
    let mut e = ed_named("hello", "test.rs");
    e.execute_command("comment");
    assert_eq!(e.test_text(), "// hello");
}

#[test]
fn test_execute_command_comment_on() {
    let mut e = ed_named("// hello", "test.rs");
    e.execute_command("comment on");
    assert_eq!(e.test_text(), "// hello");
}

#[test]
fn test_execute_command_comment_on_uncommented() {
    let mut e = ed_named("hello", "test.rs");
    e.execute_command("comment on");
    assert_eq!(e.test_text(), "// hello");
}

#[test]
fn test_execute_command_comment_off() {
    let mut e = ed_named("// hello", "test.rs");
    e.execute_command("comment off");
    assert_eq!(e.test_text(), "hello");
}

#[test]
fn test_execute_command_comment_off_uncommented() {
    let mut e = ed_named("hello", "test.rs");
    e.execute_command("comment off");
    assert_eq!(e.test_text(), "hello");
}

#[test]
fn test_execute_command_selectall() {
    let mut e = ed("hello\nworld");
    e.execute_command("selectall");
    assert!(!selection(&e).is_empty());
    let (start, end) = selection(&e).ordered();
    assert_eq!(start, TextPosition::zero());
    assert_eq!(end.line, 1);
}

#[test]
fn test_complete_command_single_match() {
    let mut e = ed("hello");
    e.command_buffer
        .open(CommandBufferMode::Command, "> ", "rul");
    e.complete_command();
    assert_eq!(e.command_buffer.input, "ruler");
    assert!(e.command_buffer.completions.is_empty());
}

#[test]
fn test_complete_command_multiple_matches() {
    let mut e = ed("hello");
    e.command_buffer.open(CommandBufferMode::Command, "> ", "q");
    e.complete_command();
    assert_eq!(e.command_buffer.completions.len(), 2);
}

#[test]
fn test_complete_command_no_matches() {
    let mut e = ed("hello");
    e.command_buffer
        .open(CommandBufferMode::Command, "> ", "xyz");
    e.complete_command();
    assert!(e.command_buffer.completions.is_empty());
}

#[test]
fn test_complete_command_empty_shows_all() {
    let mut e = ed("hello");
    e.command_buffer.open(CommandBufferMode::Command, "> ", "");
    e.complete_command();
    assert!(!e.command_buffer.completions.is_empty());
}


#[test]
fn test_handle_cmd_result_submit_find() {
    let mut e = ed("hello world hello");
    e.handle_cmd_result(
        CommandBufferMode::Find,
        CommandBufferResult::Submit("hello".to_string()),
    );
    assert!(e.find.active);
}

#[test]
fn test_handle_cmd_result_submit_goto() {
    let mut e = ed("aaa\nbbb\nccc");
    e.handle_cmd_result(
        CommandBufferMode::Goto,
        CommandBufferResult::Submit("2".to_string()),
    );
    assert_eq!(e.cursor().line, 1);
}

#[test]
fn test_handle_cmd_result_submit_prompt() {
    let dir = std::env::temp_dir().join("e_test_cmd_prompt");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("test.txt");
    let mut e = ed("hello");
    e.handle_cmd_result(
        CommandBufferMode::Prompt,
        CommandBufferResult::Submit(path.to_str().unwrap().to_string()),
    );
    assert_eq!(
        e.document.file_path.as_deref(),
        Some(path.to_str().unwrap())
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_handle_cmd_result_cancel_find() {
    let mut e = ed("hello");
    e.find.matches = vec![(TextPosition::new(0, 0), TextPosition::new(0, 5))];
    e.handle_cmd_result(CommandBufferMode::Find, CommandBufferResult::Cancel);
    assert!(e.find.matches.is_empty());
}

#[test]
fn test_handle_cmd_result_cancel_sudo() {
    let mut e = ed("hello");
    e.sudo_save = Some(SudoSave {
        path: "/tmp/nonexistent_test_file".to_string(),
        cleaned: b"secret".to_vec(),
    });
    e.handle_cmd_result(CommandBufferMode::SudoSave, CommandBufferResult::Cancel);
    assert!(e.sudo_save.is_none());
    assert!(e.status_message.contains("cancelled"));
}

#[test]
fn test_handle_cmd_result_changed_find() {
    let mut e = ed("hello world hello");
    e.handle_cmd_result(
        CommandBufferMode::Find,
        CommandBufferResult::Changed("hello".to_string()),
    );
    assert_eq!(e.find.matches.len(), 2);
}

#[test]
fn test_handle_cmd_result_tab_complete() {
    let mut e = ed("hello");
    e.command_buffer
        .open(CommandBufferMode::Command, "> ", "rul");
    e.handle_cmd_result(CommandBufferMode::Command, CommandBufferResult::TabComplete);
    assert_eq!(e.command_buffer.input, "ruler");
}

#[test]
fn test_handle_cmd_result_continue_noop() {
    let mut e = ed("hello");
    e.handle_cmd_result(CommandBufferMode::Command, CommandBufferResult::Continue);
    assert_eq!(e.test_text(), "hello");
}


#[test]
fn test_handle_event_dispatches_key() {
    let mut e = ed("hello");
    e.dispatch_event(EditorEvent::Key(Key::Char('x')));
    assert_eq!(e.test_text(), "xhello");
}

#[test]
fn test_handle_event_mouse_ignored_when_cmd_active() {
    let mut e = ed("hello");
    e.command_buffer.open(CommandBufferMode::Command, "> ", "");
    e.dispatch_event(EditorEvent::Mouse(
        MouseEvent::Press(MouseButton::Left, 1, 1),
        MouseMods::default(),
    ));
    assert!(e.command_buffer.active);
}

#[test]
fn test_handle_event_unsupported_ctrl_shift_up() {
    let mut e = ed("hello\nworld");
    e.set_cursor(TextPosition::new(1, 3));
    e.dispatch_event(EditorEvent::Key(Key::CtrlShiftUp));
    assert_eq!(selection(&e).cursor, TextPosition::new(0, 0));
}

#[test]
fn test_handle_event_unsupported_ctrl_shift_down() {
    let mut e = ed("hello\nworld");
    e.set_cursor(TextPosition::new(0, 2));
    e.dispatch_event(EditorEvent::Key(Key::CtrlShiftDown));
    assert_eq!(selection(&e).cursor, TextPosition::new(1, 5));
}

#[test]
fn test_quit_scratch_dirty_y_then_save_as() {
    let dir = std::env::temp_dir().join("e_test_quit_scratch");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("out.txt");
    let mut e = ed("hello");
    e.document.is_dirty = true;
    e.try_quit();
    e.handle_key(Key::Char('y'));
    assert!(e.running, "editor must not quit before filename is given");
    assert!(e.command_buffer.active, "save-as prompt must be open");
    assert_eq!(
        e.modal,
        Modal::QuitAfterSave,
        "quit must stay pending until the save completes"
    );
    let path_str = path.to_str().unwrap().to_string();
    for ch in path_str.chars() {
        e.handle_cmd_key(Key::Char(ch));
    }
    e.handle_cmd_key(Key::Char('\n'));
    assert!(!e.running, "editor must quit after filename confirmed");
    assert!(path.exists(), "file must have been written to disk");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_find_nav_up_down() {
    let mut e = ed("aa bb aa");
    e.find_next_from_submit("aa");
    assert!(e.find.active);
    let first = e.find.current;
    e.handle_key(Key::Down);
    let second = e.find.current;
    assert_ne!(first, second);
    e.handle_key(Key::Up);
    assert_ne!(e.find.current, second);
}

#[test]
fn test_find_nav_esc_clears() {
    let mut e = ed("aa bb aa");
    e.find_next_from_submit("aa");
    e.handle_key(Key::Esc);
    assert!(!e.find.active);
    assert!(selection(&e).is_empty());
}

#[test]
fn test_find_nav_other_key_exits_and_processes() {
    let mut e = ed("aa bb");
    e.find_next_from_submit("aa");
    assert!(e.find.active);
    e.handle_key(Key::Char('x'));
    assert!(!e.find.active);
    assert!(e.test_text().contains('x'));
}

#[test]
fn test_esc_clears_selection_and_matches() {
    let mut e = ed("hello");
    set_sel(
        &mut e,
        Selection {
            anchor: TextPosition::new(0, 0),
            cursor: TextPosition::new(0, 5),
        },
    );
    e.find.matches = vec![(TextPosition::new(0, 0), TextPosition::new(0, 5))];
    e.handle_key(Key::Esc);
    assert!(selection(&e).is_empty());
    assert!(e.find.matches.is_empty());
}

#[test]
fn test_keybinding_action_dispatch() {
    let mut e = ed("hello");
    e.handle_key(Key::Ctrl('a'));
    assert!(!selection(&e).is_empty());
}

#[test]
fn test_handle_cmd_key_dispatches() {
    let mut e = ed("hello");
    e.command_buffer.open(CommandBufferMode::Command, "> ", "");
    e.handle_cmd_key(Key::Char('a'));
    assert_eq!(e.command_buffer.input, "a");
}


#[test]
fn test_mouse_single_click() {
    let mut e = ed("hello\nworld");
    e.mouse_press(6, 2, MouseMods::default());
    assert_eq!(e.cursor().line, 1);
}

#[test]
fn test_mouse_drag() {
    let mut e = ed("hello world");
    e.mouse_press(3, 1, MouseMods::default());
    assert!(e.mouse.dragging);
    e.mouse_drag(8, 1);
    assert_ne!(selection(&e).anchor, selection(&e).cursor);
}

#[test]
fn test_mouse_release() {
    let mut e = ed("hello");
    e.mouse.dragging = true;
    e.handle_mouse(MouseEvent::Release(0, 0), MouseMods::default());
    assert!(!e.mouse.dragging);
}

#[test]
fn test_ctrl_click_adds_caret_and_inserts_at_all_carets() {
    let mut e = ed("abc\ndef");
    e.line_numbers_visible = false;

    e.mouse_press(2, 2, MouseMods { ctrl: true });

    assert_eq!(e.carets.len(), 2);
    assert_eq!(e.cursor(), TextPosition::new(1, 1));

    e.insert_char('X');
    assert_eq!(e.test_text(), "Xabc\ndXef");
    assert_eq!(e.carets.len(), 2);
}

#[test]
fn test_multicursor_vertical_movement_preserves_all_carets() {
    let mut e = ed("abc\n123456\nz");
    e.carets = CaretSet::from_carets(vec![
        Caret::new(TextPosition::new(1, 5)),
        Caret::new(TextPosition::new(1, 2)),
    ], 0).unwrap();
    e.carets.normalize();

    e.move_up();
    assert_eq!(e.carets.len(), 2);
    assert_eq!(e.cursor(), TextPosition::new(0, 3));
    assert_eq!(
        e.carets
            .iter()
            .map(|caret| caret.selection.cursor)
            .collect::<Vec<_>>(),
        vec![TextPosition::new(0, 2), TextPosition::new(0, 3)]
    );

    e.move_down();
    assert_eq!(e.carets.len(), 2);
    assert_eq!(e.cursor(), TextPosition::new(1, 5));
    assert_eq!(
        e.carets
            .iter()
            .map(|caret| caret.selection.cursor)
            .collect::<Vec<_>>(),
        vec![TextPosition::new(1, 2), TextPosition::new(1, 5)]
    );
}

#[test]
fn test_multicursor_word_right_moves_all_carets() {
    let mut e = ed("hello world\nfoo bar");
    e.carets = CaretSet::from_carets(vec![
        Caret::new(TextPosition::new(0, 0)),
        Caret::new(TextPosition::new(1, 0)),
    ], 1).unwrap();
    e.carets.normalize();

    e.word_right();

    assert_eq!(e.carets.len(), 2);
    assert_eq!(e.cursor(), TextPosition::new(1, 4));
    assert_eq!(
        e.carets
            .iter()
            .map(|caret| caret.selection.cursor)
            .collect::<Vec<_>>(),
        vec![TextPosition::new(0, 6), TextPosition::new(1, 4)]
    );
}

#[test]
fn test_mouse_scroll_up_down() {
    let text = (0..50)
        .map(|i| format!("line {}", i))
        .collect::<Vec<_>>()
        .join("\n");
    let mut e = ed(&text);
    e.viewport.scroll_line = 10;
    e.set_cursor(TextPosition::new(15, 0));
    e.scroll_down();
    assert!(e.viewport.scroll_line > 10);
    let prev = e.viewport.scroll_line;
    e.scroll_up();
    assert!(e.viewport.scroll_line < prev);
}

#[test]
fn test_scroll_up_at_top() {
    let mut e = ed("hello\nworld");
    e.scroll_up();
    assert_eq!(e.viewport.scroll_line, 0);
}

#[test]
fn test_scroll_down_at_bottom() {
    let mut e = ed("hello");
    e.scroll_down();
    assert_eq!(e.viewport.scroll_line, 0);
}

#[test]
fn test_screen_to_buffer_position_normal() {
    let e = ed("hello\nworld");
    let pos = e.screen_to_buffer_position(5, 1);
    assert_eq!(pos.line, 0);
}

#[test]
fn test_screen_to_buffer_position_below_content() {
    let e = ed("hello");
    let pos = e.screen_to_buffer_position(1, 20);
    assert_eq!(pos.line, 0);
    assert_eq!(pos.column, 5);
}

#[test]
fn test_clamp_cursor_to_viewport() {
    let text = (0..50)
        .map(|i| format!("line {}", i))
        .collect::<Vec<_>>()
        .join("\n");
    let mut e = ed(&text);
    e.set_cursor(TextPosition::new(0, 0));
    e.viewport.scroll_line = 10;
    let gw = gutter_width(e.document.buffer.line_count());
    let tc = e.viewport.text_cols(gw);
    e.clamp_cursor_to_viewport(gw, tc);
    assert!(e.cursor().line >= e.viewport.scroll_line);
}

#[test]
fn test_handle_event_mouse_exits_find_active() {
    let mut e = ed("hello world");
    e.find.active = true;
    e.find.matches = vec![(TextPosition::new(0, 0), TextPosition::new(0, 5))];
    e.dispatch_event(EditorEvent::Mouse(
        MouseEvent::Press(MouseButton::Left, 1, 1),
        MouseMods::default(),
    ));
    assert!(!e.find.active);
}


#[test]
fn test_copy_paste_workflow() {
    let mut e = ed("hello world");
    set_sel(
        &mut e,
        Selection {
            anchor: TextPosition::new(0, 0),
            cursor: TextPosition::new(0, 5),
        },
    );
    e.copy();
    e.set_cursor(TextPosition::new(0, 11));
    e.paste();
    assert_eq!(e.test_text(), "hello worldhello");
}

#[test]
fn test_cut() {
    let mut e = ed("hello world");
    set_sel(
        &mut e,
        Selection {
            anchor: TextPosition::new(0, 0),
            cursor: TextPosition::new(0, 5),
        },
    );
    e.cut();
    assert_eq!(e.test_text(), " world");
    e.paste();
    assert_eq!(e.test_text(), "hello world");
}

#[test]
fn test_multi_cursor_internal_clipboard_pastes_fragments_per_caret() {
    let mut e = ed("hello\nworld");
    e.carets = CaretSet::from_carets(vec![
        Caret {
            selection: Selection {
                anchor: TextPosition::new(0, 0),
                cursor: TextPosition::new(0, 5),
            },
            desired_column: None,
        },
        Caret {
            selection: Selection {
                anchor: TextPosition::new(1, 0),
                cursor: TextPosition::new(1, 5),
            },
            desired_column: None,
        },
    ], 0).unwrap();
    e.carets.normalize();
    e.copy();

    e.carets = CaretSet::from_carets(vec![
        Caret::new(TextPosition::new(0, 5)),
        Caret::new(TextPosition::new(1, 5)),
    ], 1).unwrap();
    e.carets.normalize();
    e.paste();

    assert_eq!(e.test_text(), "hellohello\nworldworld");
}

#[test]
fn test_multicursor_tab_with_selections_indents_lines() {
    let mut e = ed_named("aaa\nbbb\nccc", "test.rs");
    e.carets = CaretSet::from_carets(vec![
        Caret {
            selection: Selection {
                anchor: TextPosition::new(0, 0),
                cursor: TextPosition::new(0, 3),
            },
            desired_column: None,
        },
        Caret {
            selection: Selection {
                anchor: TextPosition::new(2, 0),
                cursor: TextPosition::new(2, 3),
            },
            desired_column: None,
        },
    ], 0).unwrap();
    e.carets.normalize();

    e.insert_tab();

    assert_eq!(e.test_text(), "  aaa\nbbb\n  ccc");
    assert_eq!(e.carets.len(), 2);
    assert_eq!(
        e.carets.as_slice()[0].selection.ordered(),
        (TextPosition::new(0, 2), TextPosition::new(0, 5))
    );
    assert_eq!(
        e.carets.as_slice()[1].selection.ordered(),
        (TextPosition::new(2, 2), TextPosition::new(2, 5))
    );
}

#[test]
fn test_multicursor_linewise_indent_handles_more_lines_than_carets() {
    let mut e = ed_named("aaa\nbbb\nccc\nddd", "test.rs");
    e.carets = CaretSet::from_carets(vec![
        Caret {
            selection: Selection {
                anchor: TextPosition::new(0, 0),
                cursor: TextPosition::new(1, 3),
            },
            desired_column: None,
        },
        Caret {
            selection: Selection {
                anchor: TextPosition::new(3, 0),
                cursor: TextPosition::new(3, 3),
            },
            desired_column: None,
        },
    ], 1).unwrap();
    e.carets.normalize();

    e.insert_tab();

    assert_eq!(e.test_text(), "  aaa\n  bbb\nccc\n  ddd");
    assert_eq!(e.cursor(), TextPosition::new(3, 5));
    assert_eq!(
        e.carets.as_slice()[0].selection.ordered(),
        (TextPosition::new(0, 2), TextPosition::new(1, 5))
    );
    assert_eq!(
        e.carets.as_slice()[1].selection.ordered(),
        (TextPosition::new(3, 2), TextPosition::new(3, 5))
    );
}

#[test]
fn test_multicursor_dedent_comment_undo_redo_preserve_carets() {
    let mut e = ed_named("  aaa\n  bbb\nccc", "test.rs");
    e.carets = CaretSet::from_carets(vec![
        Caret::new(TextPosition::new(0, 2)),
        Caret::new(TextPosition::new(1, 2)),
    ], 1).unwrap();
    e.carets.normalize();

    e.dedent();
    assert_eq!(e.test_text(), "aaa\nbbb\nccc");
    assert_eq!(e.cursor(), TextPosition::new(1, 0));
    assert_eq!(
        e.carets
            .iter()
            .map(|caret| caret.selection.cursor)
            .collect::<Vec<_>>(),
        vec![TextPosition::new(0, 0), TextPosition::new(1, 0)]
    );

    e.toggle_comment();
    assert_eq!(e.test_text(), "// aaa\n// bbb\nccc");
    assert_eq!(e.cursor(), TextPosition::new(1, 3));
    assert_eq!(
        e.carets
            .iter()
            .map(|caret| caret.selection.cursor)
            .collect::<Vec<_>>(),
        vec![TextPosition::new(0, 3), TextPosition::new(1, 3)]
    );

    e.undo();
    assert_eq!(e.test_text(), "aaa\nbbb\nccc");
    assert_eq!(e.cursor(), TextPosition::new(1, 0));
    assert_eq!(
        e.carets
            .iter()
            .map(|caret| caret.selection.cursor)
            .collect::<Vec<_>>(),
        vec![TextPosition::new(0, 0), TextPosition::new(1, 0)]
    );

    e.undo();
    assert_eq!(e.test_text(), "  aaa\n  bbb\nccc");
    assert_eq!(e.cursor(), TextPosition::new(1, 2));
    assert_eq!(
        e.carets
            .iter()
            .map(|caret| caret.selection.cursor)
            .collect::<Vec<_>>(),
        vec![TextPosition::new(0, 2), TextPosition::new(1, 2)]
    );

    e.redo();
    assert_eq!(e.test_text(), "aaa\nbbb\nccc");
    assert_eq!(e.cursor(), TextPosition::new(1, 0));
    assert_eq!(
        e.carets
            .iter()
            .map(|caret| caret.selection.cursor)
            .collect::<Vec<_>>(),
        vec![TextPosition::new(0, 0), TextPosition::new(1, 0)]
    );
}

#[test]
fn test_paste_text_replaces_selection() {
    let mut e = ed("hello world");
    set_sel(
        &mut e,
        Selection {
            anchor: TextPosition::new(0, 6),
            cursor: TextPosition::new(0, 11),
        },
    );
    e.paste_text("earth");
    assert_eq!(e.test_text(), "hello earth");
}

#[test]
fn test_paste_empty_noop() {
    let mut e = ed("hello");
    e.paste_text("");
    assert_eq!(e.test_text(), "hello");
}

#[test]
fn test_copy_empty_selection_noop() {
    let mut e = ed("hello");
    e.copy();
    assert_eq!(e.clipboard.paste().unwrap(), "");
}

#[test]
fn test_undo_redo_chain() {
    let mut e = ed("hello");
    e.set_cursor(TextPosition::new(0, 5));
    e.document.seal_undo();
    e.insert_char('!');
    e.document.seal_undo();
    assert_eq!(e.test_text(), "hello!");
    e.undo();
    assert_eq!(e.test_text(), "hello");
    e.redo();
    assert_eq!(e.test_text(), "hello!");
}


#[test]
fn test_toggle_comment_on_rs_file() {
    let mut e = ed_named("hello\nworld", "test.rs");
    e.set_cursor(TextPosition::new(0, 0));
    e.toggle_comment();
    assert_eq!(e.test_text(), "// hello\nworld");
}

#[test]
fn test_toggle_comment_off_rs_file() {
    let mut e = ed_named("// hello\nworld", "test.rs");
    e.set_cursor(TextPosition::new(0, 0));
    e.toggle_comment();
    assert_eq!(e.test_text(), "hello\nworld");
}

#[test]
fn test_toggle_comment_no_language() {
    let mut e = ed("hello");
    e.toggle_comment();
    assert!(e.status_message.contains("No language"));
}

#[test]
fn test_toggle_comment_selection() {
    let mut e = ed_named("aaa\nbbb\nccc", "test.rs");
    set_sel(
        &mut e,
        Selection {
            anchor: TextPosition::new(0, 0),
            cursor: TextPosition::new(2, 3),
        },
    );
    e.toggle_comment();
    assert_eq!(e.test_text(), "// aaa\n// bbb\n// ccc");
}

#[test]
fn test_dedent_spaces() {
    let mut e = ed("  hello");
    e.set_cursor(TextPosition::new(0, 2));
    e.dedent();
    assert_eq!(e.test_text(), "hello");
}

#[test]
fn test_dedent_tab() {
    let mut e = ed("\thello");
    e.set_cursor(TextPosition::new(0, 1));
    e.dedent();
    assert_eq!(e.test_text(), "hello");
}

#[test]
fn test_dedent_no_indent() {
    let mut e = ed("hello");
    e.set_cursor(TextPosition::new(0, 0));
    e.dedent();
    assert_eq!(e.test_text(), "hello");
}

#[test]
fn test_indent_selection_skips_blank_lines() {
    let mut e = ed_named("aaa\n\nbbb", "test.rs");
    set_sel(
        &mut e,
        Selection {
            anchor: TextPosition::new(0, 0),
            cursor: TextPosition::new(2, 3),
        },
    );
    e.indent_selection();
    assert_eq!(e.test_text(), "  aaa\n\n  bbb");
}


#[test]
fn test_strip_trailing_whitespace() {
    let mut e = ed("hello   \nworld  ");
    e.set_cursor(TextPosition::new(0, 8));
    e.strip_trailing_whitespace();
    assert_eq!(e.test_text(), "hello\nworld");
    assert!(e.cursor().column <= 5);
}

#[test]
fn test_tabs_to_spaces() {
    let mut e = ed("\thello\n\t\tworld");
    e.tabs_to_spaces();
    assert_eq!(e.test_text(), "  hello\n    world");
}

#[test]
fn test_tabs_to_spaces_mid_line() {
    let mut e = ed("a\tb");
    e.tabs_to_spaces();
    assert_eq!(e.test_text(), "a  b");
}

#[test]
fn test_spaces_to_tabs_leading_only() {
    let mut e = ed("    hello  world");
    e.spaces_to_tabs();
    assert_eq!(e.test_text(), "\t\thello  world");
}

#[test]
fn test_spaces_to_tabs_odd_spaces() {
    let mut e = ed("   hello");
    e.spaces_to_tabs();
    assert_eq!(e.test_text(), "\t hello");
}

#[test]
fn test_status_left_scratch() {
    let e = ed("hello");
    let left = e.status_left("Text");
    assert!(left.contains("[scratch]"));
}

#[test]
fn test_status_left_named_clean() {
    let e = ed_named("hello", "test.rs");
    let lang_name = e
        .document
        .detect_language()
        .map(|l| l.name())
        .unwrap_or("Text");
    let left = e.status_left(lang_name);
    assert!(left.contains("test.rs"));
    assert!(left.contains("Rust"));
    assert!(!left.contains('*'));
}

#[test]
fn test_status_left_named_dirty() {
    let mut e = ed_named("hello", "test.rs");
    e.document.is_dirty = true;
    let lang_name = e
        .document
        .detect_language()
        .map(|l| l.name())
        .unwrap_or("Text");
    let left = e.status_left(lang_name);
    assert!(left.contains("test.rs*"));
}

#[test]
fn test_status_right() {
    let right = Editor::status_right();
    assert!(right.contains("e v"));
}


#[test]
fn test_common_prefix_basic() {
    assert_eq!(common_prefix(&["abc", "abd", "abe"]), "ab");
}

#[test]
fn test_common_prefix_empty() {
    assert_eq!(common_prefix(&[]), "");
}

#[test]
fn test_common_prefix_single() {
    assert_eq!(common_prefix(&["hello"]), "hello");
}

#[test]
fn test_common_prefix_no_common() {
    assert_eq!(common_prefix(&["abc", "xyz"]), "");
}

#[test]
fn test_cursor_display_col_with_tabs() {
    let mut e = ed("\thello");
    e.set_cursor(TextPosition::new(0, 1));
    assert_eq!(e.cursor_display_col(), 2);
}

#[test]
fn test_cursor_display_col_no_tabs() {
    let mut e = ed("hello");
    e.set_cursor(TextPosition::new(0, 3));
    assert_eq!(e.cursor_display_col(), 3);
}

#[test]
fn test_find_matching_bracket_none() {
    let mut e = ed("hello");
    e.set_cursor(TextPosition::new(0, 0));
    assert!(e.find_matching_bracket().is_none());
}

#[test]
fn test_center_view_on_line() {
    let text = (0..100)
        .map(|i| format!("line {}", i))
        .collect::<Vec<_>>()
        .join("\n");
    let mut e = ed(&text);
    e.center_view_on_line(50);
    assert!(e.viewport.scroll_line <= 50);
    assert!(e.viewport.scroll_line + e.viewport.text_rows() > 50);
}


#[test]
fn test_draw_does_not_panic() {
    let mut e = ed_named("hello\nworld", "test.rs");
    set_sel(
        &mut e,
        Selection {
            anchor: TextPosition::new(0, 0),
            cursor: TextPosition::new(0, 5),
        },
    );
    e.find.matches = vec![(TextPosition::new(0, 0), TextPosition::new(0, 5))];
    e.find.active = true;
    let mut output = Vec::new();
    e.draw(&mut output).unwrap();
    assert!(!output.is_empty());
}

#[test]
fn test_draw_with_cmd_buf_active() {
    let mut e = ed("hello");
    e.command_buffer
        .open(CommandBufferMode::Find, "find: ", "test");
    e.command_buffer.completions = vec!["comp1".to_string()];
    let mut output = Vec::new();
    e.draw(&mut output).unwrap();
    let s = String::from_utf8_lossy(&output);
    assert!(s.contains("find: test"));
}

#[test]
fn test_draw_ruler_off() {
    let mut e = ed("hello");
    e.line_numbers_visible = false;
    let mut output = Vec::new();
    e.draw(&mut output).unwrap();
    assert!(!output.is_empty());
}


#[test]
fn test_handle_key_delete() {
    let mut e = ed("hello");
    e.handle_key(Key::Delete);
    assert_eq!(e.test_text(), "ello");
}

#[test]
fn test_handle_key_backtab() {
    let mut e = ed("  hello");
    e.set_cursor(TextPosition::new(0, 2));
    e.handle_key(Key::BackTab);
    assert_eq!(e.test_text(), "hello");
}

#[test]
fn test_handle_key_newline() {
    let mut e = ed("hello");
    e.set_cursor(TextPosition::new(0, 5));
    e.handle_key(Key::Char('\n'));
    assert_eq!(e.test_text(), "hello\n");
    assert_eq!(e.cursor(), TextPosition::new(1, 0));
}

#[test]
fn test_newline_empty_buffer() {
    let mut e = ed("");
    assert_eq!(e.cursor(), TextPosition::new(0, 0));
    e.handle_key(Key::Char('\n'));
    assert_eq!(e.test_text(), "\n");
    assert_eq!(
        e.cursor(),
        TextPosition::new(1, 0),
        "cursor should move to line 1"
    );
}

#[test]
fn test_handle_key_char() {
    let mut e = ed("");
    e.handle_key(Key::Char('a'));
    e.handle_key(Key::Char('b'));
    assert_eq!(e.test_text(), "ab");
}

#[test]
fn test_handle_key_unknown_does_nothing() {
    let mut e = ed("hello");
    e.handle_key(Key::F(12));
    assert_eq!(e.test_text(), "hello");
}


#[test]
fn test_keybinding_save() {
    let dir = std::env::temp_dir().join("e_test_kb_save");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("test.txt");
    std::fs::write(&path, b"hello").unwrap();
    let mut e = ed_named("hello", path.to_str().unwrap());
    e.document.is_dirty = true;
    e.handle_key(Key::Ctrl('s'));
    assert!(!e.document.is_dirty);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_keybinding_undo_redo() {
    let mut e = ed("hello");
    e.set_cursor(TextPosition::new(0, 5));
    e.insert_char('!');
    e.document.seal_undo();
    e.handle_key(Key::Ctrl('z'));
    assert_eq!(e.test_text(), "hello");
    e.handle_key(Key::Ctrl('y'));
    assert_eq!(e.test_text(), "hello!");
}

#[test]
fn test_keybinding_copy_paste() {
    let mut e = ed("hello");
    e.handle_key(Key::Ctrl('a'));
    e.handle_key(Key::Ctrl('c'));
    e.set_cursor(TextPosition::new(0, 5));
    e.handle_key(Key::Ctrl('v'));
    assert_eq!(e.test_text(), "hellohello");
}

#[test]
fn test_keybinding_cut() {
    let mut e = ed("hello");
    e.handle_key(Key::Ctrl('a'));
    e.handle_key(Key::Ctrl('x'));
    assert_eq!(e.test_text(), "");
}

#[test]
fn test_keybinding_kill_line() {
    let mut e = ed("hello\nworld");
    e.handle_key(Key::Ctrl('k'));
    assert_eq!(e.test_text(), "world");
}

#[test]
fn test_keybinding_goto_top_end() {
    let mut e = ed("aaa\nbbb\nccc");
    e.set_cursor(TextPosition::new(1, 1));
    e.handle_key(Key::Ctrl('t'));
    assert_eq!(e.cursor(), TextPosition::new(0, 0));
    e.handle_key(Key::Ctrl('g'));
    assert_eq!(e.cursor(), TextPosition::new(2, 3));
}

#[test]
fn test_keybinding_toggle_ruler() {
    let mut e = ed("hello");
    assert!(e.line_numbers_visible);
    e.handle_key(Key::Ctrl('r'));
    assert!(!e.line_numbers_visible);
}

#[test]
fn test_keybinding_command_palette() {
    let mut e = ed("hello");
    e.handle_key(Key::Ctrl('p'));
    assert!(e.command_buffer.active);
    assert_eq!(e.command_buffer.mode, CommandBufferMode::Command);
}

#[test]
fn test_keybinding_goto_line() {
    let mut e = ed("hello");
    e.handle_key(Key::Ctrl('l'));
    assert!(e.command_buffer.active);
    assert_eq!(e.command_buffer.mode, CommandBufferMode::Goto);
}

#[test]
fn test_keybinding_find() {
    let mut e = ed("hello");
    e.handle_key(Key::Ctrl('f'));
    assert!(e.command_buffer.active);
    assert_eq!(e.command_buffer.mode, CommandBufferMode::Find);
}

#[test]
fn test_keybinding_find_prefills_selection() {
    let mut e = ed("hello world");
    set_sel(
        &mut e,
        Selection {
            anchor: TextPosition::new(0, 6),
            cursor: TextPosition::new(0, 11),
        },
    );
    e.handle_key(Key::Ctrl('f'));
    assert_eq!(e.command_buffer.input, "world");
}

#[test]
fn test_keybinding_ctrl_backspace() {
    let mut e = ed("hello world");
    e.set_cursor(TextPosition::new(0, 11));
    e.handle_key(Key::Ctrl('h'));
    assert_eq!(e.test_text(), "hello ");
}

#[test]
fn test_keybinding_toggle_comment() {
    let mut e = ed_named("hello", "test.rs");
    e.handle_key(Key::Ctrl('d'));
    assert_eq!(e.test_text(), "// hello");
}

#[test]
fn test_keybinding_duplicate_line() {
    let mut e = ed("hello");
    e.handle_key(Key::Ctrl('j'));
    assert_eq!(e.test_text(), "hello\nhello");
}

#[test]
fn test_keybinding_select_word() {
    let mut e = ed("hello world");
    e.set_cursor(TextPosition::new(0, 7));
    e.handle_key(Key::Ctrl('w'));
    assert!(!selection(&e).is_empty());
}


#[test]
fn test_desired_col_reset_on_non_vertical_movement() {
    let mut e = ed("hello\nworld");
    e.set_cursor(TextPosition::new(0, 3));
    e.handle_key(Key::Down);
    assert!(e.desired_column().is_some());
    e.handle_key(Key::Char('x'));
    assert!(e.desired_column().is_none());
}


#[test]
fn test_select_word_at_empty_line() {
    let mut e = ed("hello\n\nworld");
    e.select_word_at(TextPosition::new(1, 0));
    assert!(selection(&e).is_empty());
}

#[test]
fn test_select_line_at_out_of_bounds() {
    let mut e = ed("hello");
    e.select_line_at(999);
    assert!(selection(&e).is_empty());
}


#[test]
fn test_set_status() {
    let mut e = ed("hello");
    e.set_status("test message".to_string());
    assert_eq!(e.status_message, "test message");
    assert!(e.status_time.is_some());
}


#[test]
fn test_handle_mouse_wheel_up() {
    let text = (0..50)
        .map(|i| format!("line {}", i))
        .collect::<Vec<_>>()
        .join("\n");
    let mut e = ed(&text);
    e.viewport.scroll_line = 10;
    e.set_cursor(TextPosition::new(15, 0));
    e.handle_mouse(
        MouseEvent::Press(MouseButton::WheelUp, 1, 1),
        MouseMods::default(),
    );
    assert!(e.viewport.scroll_line < 10);
}

#[test]
fn test_handle_mouse_wheel_down() {
    let text = (0..50)
        .map(|i| format!("line {}", i))
        .collect::<Vec<_>>()
        .join("\n");
    let mut e = ed(&text);
    e.set_cursor(TextPosition::new(5, 0));
    e.handle_mouse(
        MouseEvent::Press(MouseButton::WheelDown, 1, 1),
        MouseMods::default(),
    );
    assert!(e.viewport.scroll_line > 0);
}

#[test]
fn test_handle_mouse_other_button_noop() {
    let mut e = ed("hello");
    e.handle_mouse(
        MouseEvent::Press(MouseButton::Middle, 1, 1),
        MouseMods::default(),
    );
    assert_eq!(e.test_text(), "hello");
}


#[test]
fn test_save_undo_if_named_no_file() {
    let mut e = ed("hello");
    e.remember_session();
}


#[test]
fn test_handle_event_dispatches_cmd_key() {
    let mut e = ed("hello");
    e.command_buffer.open(CommandBufferMode::Command, "> ", "");
    e.dispatch_event(EditorEvent::Key(Key::Char('x')));
    assert_eq!(e.command_buffer.input, "x");
}

#[test]
fn test_unsupported_ignored_when_cmd_active() {
    let mut e = ed("hello\nworld");
    e.command_buffer.open(CommandBufferMode::Command, "> ", "");
    e.dispatch_event(EditorEvent::Key(Key::CtrlShiftUp));
    assert_eq!(e.cursor(), TextPosition::new(0, 0));
}


#[test]
fn test_find_next_empty_matches() {
    let mut e = ed("hello");
    e.find_next();
}

#[test]
fn test_find_prev_empty_matches() {
    let mut e = ed("hello");
    e.find_prev();
}


#[test]
fn test_kill_line_single_line() {
    let mut e = ed("hello");
    e.kill_line();
    assert_eq!(e.test_text(), "");
}


#[test]
fn test_shift_arrows_dispatch() {
    let mut e = ed("hello\nworld");
    e.set_cursor(TextPosition::new(0, 2));
    e.handle_key(Key::ShiftRight);
    assert_eq!(selection(&e).cursor, TextPosition::new(0, 3));
    e.handle_key(Key::ShiftLeft);
    assert_eq!(selection(&e).cursor, TextPosition::new(0, 2));
    e.handle_key(Key::ShiftDown);
    assert_eq!(selection(&e).cursor, TextPosition::new(1, 2));
    e.handle_key(Key::ShiftUp);
    assert_eq!(selection(&e).cursor, TextPosition::new(0, 2));
}


#[test]
fn test_page_up_down_dispatch() {
    let text = (0..50)
        .map(|i| format!("line {}", i))
        .collect::<Vec<_>>()
        .join("\n");
    let mut e = ed(&text);
    e.set_cursor(TextPosition::new(25, 0));
    e.handle_key(Key::PageUp);
    assert!(e.cursor().line < 25);
    e.handle_key(Key::PageDown);
    assert!(e.cursor().line > 3);
}


#[test]
fn test_arrow_keys_dispatch() {
    let mut e = ed("hello\nworld");
    e.handle_key(Key::Down);
    assert_eq!(e.cursor().line, 1);
    e.handle_key(Key::Up);
    assert_eq!(e.cursor().line, 0);
    e.handle_key(Key::Right);
    assert_eq!(e.cursor().column, 1);
    e.handle_key(Key::Left);
    assert_eq!(e.cursor().column, 0);
    e.handle_key(Key::End);
    assert_eq!(e.cursor().column, 5);
    e.handle_key(Key::Home);
    assert_eq!(e.cursor().column, 0);
}


#[test]
fn test_tab_key_dispatch() {
    let mut e = ed_named("hello", "test.rs");
    e.handle_key(Key::Char('\t'));
    assert_eq!(e.test_text(), "  hello");
}


#[test]
fn test_scroll_up_through_wrapped_prev_line() {
    let long_line = "a".repeat(300);
    let text = format!("{}\nshort", long_line);
    let mut e = ed(&text);
    e.line_numbers_visible = false;
    e.viewport.scroll_line = 1;
    e.viewport.scroll_wrap = 0;
    e.set_cursor(TextPosition::new(1, 0));
    e.scroll_up();
    assert_eq!(e.viewport.scroll_line, 0);
    assert!(e.viewport.scroll_wrap > 0);
}


#[test]
fn test_scroll_down_partial_wrap_advance() {
    let long_line = "a".repeat(500);
    let text = format!("{}\nend", long_line);
    let mut e = ed(&text);
    e.line_numbers_visible = false;
    e.viewport.scroll_line = 0;
    e.viewport.scroll_wrap = 0;
    e.set_cursor(TextPosition::new(0, 0));
    e.scroll_down();
    assert_eq!(e.viewport.scroll_line, 0);
    assert_eq!(e.viewport.scroll_wrap, 3);
}


#[test]
fn test_save_keybinding_no_filename() {
    let mut e = ed("hello");
    e.handle_key(Key::Ctrl('s'));
    assert!(e.command_buffer.active);
}


#[test]
fn test_backspace_key_dispatch() {
    let mut e = ed("ab");
    e.set_cursor(TextPosition::new(0, 2));
    e.handle_key(Key::Backspace);
    assert_eq!(e.test_text(), "a");
}


#[test]
fn test_cmd_submit_executes_command() {
    let mut e = ed("hello");
    e.handle_cmd_result(
        crate::command_buffer::CommandBufferMode::Command,
        crate::command_buffer::CommandBufferResult::Submit("ruler".to_string()),
    );
    assert!(!e.line_numbers_visible);
}


#[test]
fn test_execute_unknown_command() {
    let mut e = ed("hello");
    e.execute_command("nonexistent_command");
    assert!(e.status_message.contains("Unknown"));
}


#[test]
fn test_kill_line_on_last_line() {
    let mut e = ed("hello");
    e.kill_line();
    assert_eq!(e.test_text(), "");
}


#[test]
fn test_draw_with_status_msg() {
    let mut e = ed("hello\nworld");
    e.set_status("Test status".to_string());
    assert!(!e.status_message.is_empty());
    let mut buf = Vec::new();
    let _ = e.draw(&mut buf);
    let output = String::from_utf8_lossy(&buf);
    assert!(output.contains("Test status"));
}


#[test]
fn test_center_view_ruler_off() {
    let mut e =
        ed("a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\nl\nm\nn\no\np\nq\nr\ns\nt\nu\nv\nw\nx\ny\nz");
    e.line_numbers_visible = false;
    e.center_view_on_line(20);
    assert!(e.viewport.scroll_line > 0);
}


#[test]
fn test_find_matching_quote() {
    let mut e = ed("let s = \"hello\";\n");
    e.set_cursor(TextPosition::new(0, 8));
    let pair = e.find_matching_bracket();
    assert!(pair.is_some());
    let (_, match_pos) = pair.unwrap();
    assert_eq!(match_pos.column, 14);
}


#[test]
fn test_replace_all_case_sensitive() {
    let mut e = ed("Hello hello HELLO");
    e.replace_all("Hello", "Bye");
    assert_eq!(e.test_text(), "Bye hello HELLO");
}


#[test]
fn test_mouse_hold_drag() {
    let mut e = ed("hello world");
    e.line_numbers_visible = false;
    e.handle_mouse(
        MouseEvent::Press(MouseButton::Left, 1, 1),
        MouseMods::default(),
    );
    assert!(e.mouse.dragging);
    e.handle_mouse(MouseEvent::Hold(6, 1), MouseMods::default());
    assert!(!selection(&e).is_empty());
}


#[test]
fn test_mouse_release_stops_drag() {
    let mut e = ed("hello");
    e.mouse.dragging = true;
    e.handle_mouse(MouseEvent::Release(1, 1), MouseMods::default());
    assert!(!e.mouse.dragging);
}


#[test]
fn test_screen_to_buffer_position_ruler_off() {
    let mut e = ed("hello\nworld");
    e.line_numbers_visible = false;
    let pos = e.screen_to_buffer_position(1, 1);
    assert_eq!(pos, TextPosition::new(0, 0));
    let pos2 = e.screen_to_buffer_position(1, 2);
    assert_eq!(pos2, TextPosition::new(1, 0));
}


#[test]
fn test_screen_to_buffer_position_zero_cols() {
    let mut e = ed("hello");
    e.viewport = crate::viewport::Viewport::new(1, 3);
    e.line_numbers_visible = true;
    let pos = e.screen_to_buffer_position(1, 1);
    assert_eq!(pos, TextPosition::zero());
}


#[test]
fn test_double_click_selects_word() {
    let mut e = ed("hello world");
    e.line_numbers_visible = false;
    e.mouse_press(1, 1, MouseMods::default());
    e.mouse_press(1, 1, MouseMods::default());
    assert!(!selection(&e).is_empty());
}

#[test]
fn test_triple_click_selects_line() {
    let mut e = ed("hello world\nsecond");
    e.line_numbers_visible = false;
    e.mouse_press(1, 1, MouseMods::default());
    e.mouse_press(1, 1, MouseMods::default());
    e.mouse_press(1, 1, MouseMods::default());
    assert!(!selection(&e).is_empty());
}


#[test]
fn test_mouse_drag_not_dragging_noop() {
    let mut e = ed("hello");
    e.mouse.dragging = false;
    let cursor_before = e.cursor();
    e.mouse_drag(5, 1);
    assert_eq!(e.cursor(), cursor_before);
}


#[test]
fn test_scroll_up_ruler_off() {
    let text = (0..50)
        .map(|i| format!("line{}", i))
        .collect::<Vec<_>>()
        .join("\n");
    let mut e = ed(&text);
    e.line_numbers_visible = false;
    e.viewport.scroll_line = 20;
    e.set_cursor(TextPosition::new(20, 0));
    e.scroll_up();
    assert!(e.viewport.scroll_line < 20);
}

#[test]
fn test_scroll_down_ruler_off() {
    let text = (0..50)
        .map(|i| format!("line{}", i))
        .collect::<Vec<_>>()
        .join("\n");
    let mut e = ed(&text);
    e.line_numbers_visible = false;
    e.set_cursor(TextPosition::new(0, 0));
    e.scroll_down();
    assert!(e.viewport.scroll_line > 0);
}


#[test]
fn test_scroll_up_with_wrap() {
    let long_line = "a".repeat(200);
    let mut e = ed(&long_line);
    e.line_numbers_visible = false;
    e.viewport.scroll_wrap = 3;
    e.set_cursor(TextPosition::new(0, 0));
    e.scroll_up();
    assert!(e.viewport.scroll_wrap < 3);
}


#[test]
fn test_scroll_down_with_wrap() {
    let long_line = "a".repeat(200);
    let text = format!("{}\nshort", long_line);
    let mut e = ed(&text);
    e.line_numbers_visible = false;
    e.set_cursor(TextPosition::new(0, 0));
    e.scroll_down();
    assert!(e.viewport.scroll_wrap > 0 || e.viewport.scroll_line > 0);
}


#[test]
fn test_clamp_cursor_zero_rows() {
    let mut e = ed("hello");
    e.viewport = crate::viewport::Viewport::new(80, 2);
    let cursor_before = e.cursor();
    e.clamp_cursor_to_viewport(0, 80);
    assert_eq!(e.cursor(), cursor_before);
}


#[test]
fn test_clamp_cursor_below_viewport() {
    let text = (0..50)
        .map(|i| format!("line{}", i))
        .collect::<Vec<_>>()
        .join("\n");
    let mut e = ed(&text);
    e.line_numbers_visible = false;
    e.carets.primary_mut().selection.cursor = TextPosition::new(45, 0);
    e.carets.primary_mut().selection.anchor = TextPosition::new(45, 0);
    e.viewport.scroll_line = 0;
    e.clamp_cursor_to_viewport(0, 80);
    assert!(e.cursor().line < 45);
}


#[test]
fn test_move_left_extend_wraps_to_prev_line() {
    let mut e = ed("hello\nworld");
    e.set_cursor(TextPosition::new(1, 0));
    e.move_left_extend();
    assert_eq!(selection(&e).cursor, TextPosition::new(0, 5));
}


#[test]
fn test_move_right_extend_wraps_to_next_line() {
    let mut e = ed("hello\nworld");
    e.set_cursor(TextPosition::new(0, 5));
    e.move_right_extend();
    assert_eq!(selection(&e).cursor, TextPosition::new(1, 0));
}


#[test]
fn test_indent_selection_skips_trailing_empty_line() {
    let mut e = ed_named("aaa\nbbb\nccc\n", "test.rs");
    set_sel(
        &mut e,
        Selection {
            anchor: TextPosition::new(0, 0),
            cursor: TextPosition::new(3, 0),
        },
    );
    e.indent_selection();
    assert!(e.test_text().starts_with("  aaa\n  bbb\n  ccc\n"));
}


#[test]
fn test_toggle_comment_selection_end_adj() {
    let mut e = ed_named("aaa\nbbb\nccc\n", "test.rs");
    set_sel(
        &mut e,
        Selection {
            anchor: TextPosition::new(0, 0),
            cursor: TextPosition::new(2, 0),
        },
    );
    e.toggle_comment();
    let text = e.test_text();
    assert!(text.starts_with("// aaa\n// bbb\n"));
}


#[test]
fn test_toggle_comment_with_blank_lines() {
    let mut e = ed_named("aaa\n\nbbb\n", "test.rs");
    set_sel(
        &mut e,
        Selection {
            anchor: TextPosition::new(0, 0),
            cursor: TextPosition::new(2, 3),
        },
    );
    e.toggle_comment();
    let text = e.test_text();
    assert!(text.contains("// aaa"));
    assert!(text.contains("// bbb"));
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[1], "");
}

#[test]
fn test_toggle_comment_skips_already_commented() {
    let mut e = ed_named("aaa\n// bbb\nccc\n", "test.rs");
    set_sel(
        &mut e,
        Selection {
            anchor: TextPosition::new(0, 0),
            cursor: TextPosition::new(2, 3),
        },
    );
    e.toggle_comment();
    let text = e.test_text();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "// aaa");
    assert_eq!(lines[1], "// bbb");
    assert_eq!(lines[2], "// ccc");
}

#[test]
fn test_comment_performance_3000_lines() {
    let text: String = (0..3000)
        .map(|i| format!("let x{} = {};", i, i))
        .collect::<Vec<_>>()
        .join("\n");
    let mut e = ed_named(&text, "test.rs");
    e.select_all();
    let start = std::time::Instant::now();
    e.toggle_comment();
    let elapsed = start.elapsed();
    assert!(e.test_text().starts_with("// let x0"));
    assert!(
        elapsed.as_millis() < 500,
        "comment on 3000 lines took {:?}",
        elapsed
    );
}


#[test]
fn test_dedent_selection_end_adj() {
    let mut e = ed_named("  aaa\n  bbb\n  ccc\n", "test.rs");
    set_sel(
        &mut e,
        Selection {
            anchor: TextPosition::new(0, 0),
            cursor: TextPosition::new(2, 0),
        },
    );
    e.dedent();
    let text = e.test_text();
    assert!(text.starts_with("aaa\nbbb\n"));
}

#[test]
fn test_dedent_selection_preserves_selection() {
    let mut e = ed_named("  aaa\n  bbb\n  ccc", "test.rs");
    set_sel(
        &mut e,
        Selection {
            anchor: TextPosition::new(0, 2),
            cursor: TextPosition::new(2, 5),
        },
    );
    e.dedent();
    assert_eq!(e.test_text(), "aaa\nbbb\nccc");
    assert!(!selection(&e).is_empty());
    assert_eq!(selection(&e).anchor, TextPosition::new(0, 0));
    assert_eq!(selection(&e).cursor, TextPosition::new(2, 3));
}


#[test]
fn test_cut_no_selection_noop() {
    let mut e = ed("hello");
    e.cut();
    assert_eq!(e.test_text(), "hello");
}


#[test]
fn test_execute_command_save_as_file() {
    let dir = std::env::temp_dir().join("e_test_save_as_cmd");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("new.txt");
    let mut e = ed("hello");
    let cmd = format!("save {}", path.to_str().unwrap());
    e.execute_command(&cmd);
    assert_eq!(
        e.document.file_path.as_deref(),
        Some(path.to_str().unwrap())
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_execute_command_quit_via_cmd() {
    let mut e = ed("hello");
    e.execute_command("quit");
    assert!(!e.running);
}


#[test]
fn test_handle_cmd_result_sudo_cancel_cleans_tmp() {
    let mut e = ed("hello");
    e.sudo_save = Some(SudoSave {
        path: "/tmp/e_test_sudo_fake".to_string(),
        cleaned: Vec::new(),
    });
    e.handle_cmd_result(
        crate::command_buffer::CommandBufferMode::SudoSave,
        crate::command_buffer::CommandBufferResult::Cancel,
    );
    assert!(e.sudo_save.is_none());
}


#[test]
fn test_delete_selection_empty_noop() {
    let mut e = ed("hello");
    e.delete_selection();
    assert_eq!(e.test_text(), "hello");
}



#[test]
fn test_save_undo_if_named_with_existing_file() {
    let dir = std::env::temp_dir().join("e_test_save_undo");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("test.txt");
    std::fs::write(&path, b"hello").unwrap();
    let mut e = ed("hello");
    e.document.file_path = Some(path.to_str().unwrap().to_string());
    e.remember_session();
    let _ = std::fs::remove_dir_all(&dir);
}


#[test]
fn test_find_prefills_from_selection() {
    let mut e = ed("hello world");
    set_sel(
        &mut e,
        Selection {
            anchor: TextPosition::new(0, 0),
            cursor: TextPosition::new(0, 5),
        },
    );
    e.handle_key(Key::Ctrl('f'));
    assert!(e.command_buffer.active);
    assert_eq!(e.command_buffer.input, "hello");
}


#[test]
fn test_command_completion_common_prefix() {
    let mut e = ed("hello");
    e.command_buffer
        .open(crate::command_buffer::CommandBufferMode::Command, "> ", "");
    e.command_buffer.input = "go".to_string();
    e.command_buffer.cursor = 2;
    let result = e.command_buffer.handle_key(Key::Char('\t'));
    let mode = e.command_buffer.mode;
    e.handle_cmd_result(mode, result);
}


#[test]
fn test_dismiss_reload_updates_mtime() {
    let dir = std::env::temp_dir().join("e_test_dismiss");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("test.txt");
    std::fs::write(&path, b"original").unwrap();
    let mut e = ed_named("original", path.to_str().unwrap());

    e.file_modification_time = crate::file_io::file_modification_time(&path);

    std::thread::sleep(std::time::Duration::from_millis(50));
    std::fs::write(&path, b"changed").unwrap();
    let new_mtime = crate::file_io::file_modification_time(&path);

    e.modal = Modal::ConfirmReload;
    e.status_message = "test.txt changed on disk. Reload? (y/n)".to_string();
    e.dismiss_reload();
    assert_eq!(e.modal, Modal::None);
    assert!(e.status_message.is_empty());
    assert_eq!(e.file_modification_time, new_mtime);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_reload_clamps_cursor() {
    let dir = std::env::temp_dir().join("e_test_reload_clamp");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("test.txt");
    std::fs::write(&path, b"line1\nline2\nline3\n").unwrap();

    let mut e = ed_named("line1\nline2\nline3\n", path.to_str().unwrap());
    e.file_modification_time = crate::file_io::file_modification_time(&path);
    set_sel(&mut e, Selection::caret(TextPosition::new(2, 3)));

    std::thread::sleep(std::time::Duration::from_millis(50));
    std::fs::write(&path, b"short\n").unwrap();

    e.modal = Modal::ConfirmReload;
    e.reload_file();
    assert!(selection(&e).cursor.line < e.document.buffer.line_count());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_no_reload_for_unnamed() {
    let mut e = ed("hello");
    e.check_external_modification();
    assert_eq!(e.modal, Modal::None);
    assert!(e.status_message.is_empty());
}

#[test]
fn test_focus_in_event() {
    let dir = std::env::temp_dir().join("e_test_focus_in");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("test.txt");
    std::fs::write(&path, b"original").unwrap();

    let mut e = ed_named("original", path.to_str().unwrap());
    e.file_modification_time = crate::file_io::file_modification_time(&path);

    std::thread::sleep(std::time::Duration::from_millis(50));
    std::fs::write(&path, b"changed").unwrap();

    e.dispatch_event(EditorEvent::FocusIn);
    assert_eq!(e.modal, Modal::ConfirmReload);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_save_updates_mtime() {
    let dir = std::env::temp_dir().join("e_test_save_mtime");
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("test.txt");

    let mut e = ed_named("hello\n", path.to_str().unwrap());
    assert!(e.file_modification_time.is_none());

    e.save_file();
    assert!(e.file_modification_time.is_some());

    let _ = std::fs::remove_dir_all(&dir);
}


#[test]
fn test_word_left_middle_of_line() {
    let mut e = ed("hello world foo");
    e.set_cursor(TextPosition::new(0, 15));
    e.word_left();
    assert_eq!(e.cursor(), TextPosition::new(0, 12));
    e.word_left();
    assert_eq!(e.cursor(), TextPosition::new(0, 6));
    e.word_left();
    assert_eq!(e.cursor(), TextPosition::new(0, 0));
}

#[test]
fn test_word_right_middle_of_line() {
    let mut e = ed("hello world foo");
    e.word_right();
    assert_eq!(e.cursor(), TextPosition::new(0, 6));
    e.word_right();
    assert_eq!(e.cursor(), TextPosition::new(0, 12));
    e.word_right();
    assert_eq!(e.cursor(), TextPosition::new(0, 15));
}

#[test]
fn test_word_left_wraps_to_prev_line() {
    let mut e = ed("hello\nworld");
    e.set_cursor(TextPosition::new(1, 0));
    e.word_left();
    assert_eq!(e.cursor(), TextPosition::new(0, 5));
}

#[test]
fn test_word_right_wraps_to_next_line() {
    let mut e = ed("hello\nworld");
    e.set_cursor(TextPosition::new(0, 5));
    e.word_right();
    assert_eq!(e.cursor(), TextPosition::new(1, 0));
}

#[test]
fn test_word_left_collapses_selection() {
    let mut e = ed("hello world");
    set_sel(
        &mut e,
        Selection {
            anchor: TextPosition::new(0, 2),
            cursor: TextPosition::new(0, 8),
        },
    );
    e.word_left();
    assert_eq!(e.cursor(), TextPosition::new(0, 2));
    assert!(selection(&e).is_empty());
}

#[test]
fn test_word_right_collapses_selection() {
    let mut e = ed("hello world");
    set_sel(
        &mut e,
        Selection {
            anchor: TextPosition::new(0, 2),
            cursor: TextPosition::new(0, 8),
        },
    );
    e.word_right();
    assert_eq!(e.cursor(), TextPosition::new(0, 8));
    assert!(selection(&e).is_empty());
}

#[test]
fn test_word_left_at_origin() {
    let mut e = ed("hello");
    e.word_left();
    assert_eq!(e.cursor(), TextPosition::new(0, 0));
}

#[test]
fn test_word_right_at_end() {
    let mut e = ed("hello");
    e.set_cursor(TextPosition::new(0, 5));
    e.word_right();
    assert_eq!(e.cursor(), TextPosition::new(0, 5));
}

#[test]
fn test_word_left_skips_punctuation() {
    let mut e = ed("foo.bar");
    e.set_cursor(TextPosition::new(0, 7));
    e.word_left();
    assert_eq!(e.cursor(), TextPosition::new(0, 4));
    e.word_left();
    assert_eq!(e.cursor(), TextPosition::new(0, 0));
}

#[test]
fn test_word_right_skips_punctuation() {
    let mut e = ed("foo.bar");
    e.word_right();
    assert_eq!(e.cursor(), TextPosition::new(0, 4));
    e.word_right();
    assert_eq!(e.cursor(), TextPosition::new(0, 7));
}

#[test]
fn test_word_left_multiple_spaces() {
    let mut e = ed("foo   bar");
    e.set_cursor(TextPosition::new(0, 9));
    e.word_left();
    assert_eq!(e.cursor(), TextPosition::new(0, 6));
    e.word_left();
    assert_eq!(e.cursor(), TextPosition::new(0, 0));
}

#[test]
fn test_word_right_multiple_spaces() {
    let mut e = ed("foo   bar");
    e.word_right();
    assert_eq!(e.cursor(), TextPosition::new(0, 6));
}

#[test]
fn test_word_left_empty_line() {
    let mut e = ed("hello\n\nworld");
    e.set_cursor(TextPosition::new(2, 0));
    e.word_left();
    assert_eq!(e.cursor(), TextPosition::new(1, 0));
    e.word_left();
    assert_eq!(e.cursor(), TextPosition::new(0, 5));
}

#[test]
fn test_word_right_empty_line() {
    let mut e = ed("hello\n\nworld");
    e.set_cursor(TextPosition::new(0, 5));
    e.word_right();
    assert_eq!(e.cursor(), TextPosition::new(1, 0));
    e.word_right();
    assert_eq!(e.cursor(), TextPosition::new(2, 0));
}

#[test]
fn test_word_left_from_middle_of_word() {
    let mut e = ed("hello");
    e.set_cursor(TextPosition::new(0, 3));
    e.word_left();
    assert_eq!(e.cursor(), TextPosition::new(0, 0));
}

#[test]
fn test_word_right_from_middle_of_word() {
    let mut e = ed("hello world");
    e.set_cursor(TextPosition::new(0, 3));
    e.word_right();
    assert_eq!(e.cursor(), TextPosition::new(0, 6));
}

#[test]
fn test_word_left_underscores() {
    let mut e = ed("foo_bar baz");
    e.set_cursor(TextPosition::new(0, 11));
    e.word_left();
    assert_eq!(e.cursor(), TextPosition::new(0, 8));
    e.word_left();
    assert_eq!(e.cursor(), TextPosition::new(0, 0));
}

#[test]
fn test_word_right_underscores() {
    let mut e = ed("foo_bar baz");
    e.word_right();
    assert_eq!(e.cursor(), TextPosition::new(0, 8));
}

#[test]
fn test_word_left_at_last_line_end() {
    let mut e = ed("one\ntwo");
    e.set_cursor(TextPosition::new(1, 3));
    e.word_left();
    assert_eq!(e.cursor(), TextPosition::new(1, 0));
}

#[test]
fn test_word_right_at_last_line_end() {
    let mut e = ed("hello");
    e.set_cursor(TextPosition::new(0, 5));
    e.word_right();
    assert_eq!(e.cursor(), TextPosition::new(0, 5));
}

#[test]
fn test_word_nav_roundtrip() {
    let mut e = ed("  fn main() {");
    e.word_right();
    assert_eq!(e.cursor(), TextPosition::new(0, 2));
    e.word_right();
    assert_eq!(e.cursor(), TextPosition::new(0, 5));
    e.word_left();
    assert_eq!(e.cursor(), TextPosition::new(0, 2));
    e.word_left();
    assert_eq!(e.cursor(), TextPosition::new(0, 0));
}


#[test]
fn test_word_right_jumps_to_matching_bracket() {
    let mut e = ed("fn foo() { bar }");
    e.set_cursor(TextPosition::new(0, 6));
    e.word_right();
    assert_eq!(e.cursor(), TextPosition::new(0, 7));
}

#[test]
fn test_word_left_jumps_to_matching_bracket() {
    let mut e = ed("fn foo() { bar }");
    e.set_cursor(TextPosition::new(0, 7));
    e.word_left();
    assert_eq!(e.cursor(), TextPosition::new(0, 6));
}

#[test]
fn test_bracket_jump_multiline() {
    let mut e = ed("if x {\n  y\n}");
    e.set_cursor(TextPosition::new(0, 5));
    e.word_right();
    assert_eq!(e.cursor(), TextPosition::new(2, 0));
    e.word_left();
    assert_eq!(e.cursor(), TextPosition::new(0, 5));
}

#[test]
fn test_bracket_jump_nested() {
    let mut e = ed("((inner))");
    e.set_cursor(TextPosition::new(0, 0));
    e.word_right();
    assert_eq!(e.cursor(), TextPosition::new(0, 8));
    e.set_cursor(TextPosition::new(0, 1));
    e.word_left();
    assert_eq!(e.cursor(), TextPosition::new(0, 7));
}

#[test]
fn test_bracket_jump_square() {
    let mut e = ed("a[b[c]]");
    e.set_cursor(TextPosition::new(0, 1));
    e.word_right();
    assert_eq!(e.cursor(), TextPosition::new(0, 6));
}

#[test]
fn test_bracket_jump_extend_selection() {
    let mut e = ed("fn foo() {}");
    e.set_cursor(TextPosition::new(0, 6));
    e.word_right_extend();
    assert_eq!(selection(&e).cursor, TextPosition::new(0, 7));
    assert_eq!(selection(&e).anchor, TextPosition::new(0, 6));
}

#[test]
fn test_non_bracket_does_normal_word_jump() {
    let mut e = ed("hello world");
    e.set_cursor(TextPosition::new(0, 0));
    e.word_right();
    assert_eq!(e.cursor(), TextPosition::new(0, 6));
}

#[test]
fn test_word_left_selection_collapses_before_bracket_check() {
    let mut e = ed("(hello)");
    e.set_cursor(TextPosition::new(0, 0));
    e.carets.primary_mut().selection.cursor = TextPosition::new(0, 5);
    e.word_left();
    assert_eq!(e.cursor(), TextPosition::new(0, 0));
}


#[test]
fn test_autoclose_paren() {
    let mut e = ed("");
    e.insert_char('(');
    assert_eq!(e.test_text(), "()");
    assert_eq!(e.cursor(), TextPosition::new(0, 1));
}

#[test]
fn test_autoclose_bracket() {
    let mut e = ed("");
    e.insert_char('[');
    assert_eq!(e.test_text(), "[]");
    assert_eq!(e.cursor(), TextPosition::new(0, 1));
}

#[test]
fn test_autoclose_brace() {
    let mut e = ed("");
    e.insert_char('{');
    assert_eq!(e.test_text(), "{}");
    assert_eq!(e.cursor(), TextPosition::new(0, 1));
}

#[test]
fn test_autoclose_double_quote() {
    let mut e = ed("");
    e.insert_char('"');
    assert_eq!(e.test_text(), "\"\"");
    assert_eq!(e.cursor(), TextPosition::new(0, 1));
}

#[test]
fn test_autoclose_single_quote() {
    let mut e = ed("");
    e.insert_char('\'');
    assert_eq!(e.test_text(), "'");
    assert_eq!(e.cursor(), TextPosition::new(0, 1));
}

#[test]
fn test_autoclose_single_quote_in_rust() {
    let mut e = ed_named("", "/tmp/test.rs");
    e.insert_char('\'');
    assert_eq!(e.test_text(), "''");
    assert_eq!(e.cursor(), TextPosition::new(0, 1));
}

#[test]
fn test_autoclose_skip_closing_paren() {
    let mut e = ed("");
    e.insert_char('(');
    assert_eq!(e.test_text(), "()");
    assert_eq!(e.cursor(), TextPosition::new(0, 1));
    e.insert_char(')');
    assert_eq!(e.test_text(), "()");
    assert_eq!(e.cursor(), TextPosition::new(0, 2));
}

#[test]
fn test_autoclose_skip_closing_bracket() {
    let mut e = ed("");
    e.insert_char('[');
    e.insert_char(']');
    assert_eq!(e.test_text(), "[]");
    assert_eq!(e.cursor(), TextPosition::new(0, 2));
}

#[test]
fn test_autoclose_skip_closing_brace() {
    let mut e = ed("");
    e.insert_char('{');
    e.insert_char('}');
    assert_eq!(e.test_text(), "{}");
    assert_eq!(e.cursor(), TextPosition::new(0, 2));
}

#[test]
fn test_autoclose_skip_closing_double_quote() {
    let mut e = ed("");
    e.insert_char('"');
    e.insert_char('"');
    assert_eq!(e.test_text(), "\"\"");
    assert_eq!(e.cursor(), TextPosition::new(0, 2));
}

#[test]
fn test_autoclose_skip_closing_single_quote() {
    let mut e = ed("");
    e.insert_char('\'');
    e.insert_char('\'');
    assert_eq!(e.test_text(), "''");
    assert_eq!(e.cursor(), TextPosition::new(0, 2));
}

#[test]
fn test_autoclose_no_pair_when_next_is_word() {
    let mut e = ed("hello");
    e.insert_char('(');
    assert_eq!(e.test_text(), "(hello");
}

#[test]
fn test_autoclose_pair_when_next_is_space() {
    let mut e = ed(" hello");
    e.insert_char('(');
    assert_eq!(e.test_text(), "() hello");
    assert_eq!(e.cursor(), TextPosition::new(0, 1));
}

#[test]
fn test_autoclose_pair_when_next_is_close_char() {
    let mut e = ed(")");
    e.insert_char('(');
    assert_eq!(e.test_text(), "())");
    assert_eq!(e.cursor(), TextPosition::new(0, 1));
}

#[test]
fn test_autoclose_backspace_deletes_paren_pair() {
    let mut e = ed("");
    e.insert_char('(');
    assert_eq!(e.test_text(), "()");
    e.backspace();
    assert_eq!(e.test_text(), "");
}

#[test]
fn test_autoclose_backspace_deletes_bracket_pair() {
    let mut e = ed("");
    e.insert_char('[');
    e.backspace();
    assert_eq!(e.test_text(), "");
}

#[test]
fn test_autoclose_backspace_deletes_brace_pair() {
    let mut e = ed("");
    e.insert_char('{');
    e.backspace();
    assert_eq!(e.test_text(), "");
}

#[test]
fn test_autoclose_backspace_deletes_double_quote_pair() {
    let mut e = ed("");
    e.insert_char('"');
    e.backspace();
    assert_eq!(e.test_text(), "");
}

#[test]
fn test_autoclose_backspace_deletes_single_quote_pair() {
    let mut e = ed("");
    e.insert_char('\'');
    e.backspace();
    assert_eq!(e.test_text(), "");
}

#[test]
fn test_autoclose_backtick() {
    let mut e = ed("");
    e.insert_char('`');
    assert_eq!(e.test_text(), "``");
    assert_eq!(e.cursor(), TextPosition::new(0, 1));
}

#[test]
fn test_autoclose_skip_closing_backtick() {
    let mut e = ed("");
    e.insert_char('`');
    e.insert_char('`');
    assert_eq!(e.test_text(), "``");
    assert_eq!(e.cursor(), TextPosition::new(0, 2));
}

#[test]
fn test_autoclose_backtick_fence() {
    let mut e = ed("");
    e.insert_char('`');
    e.insert_char('`');
    e.insert_char('`');
    assert_eq!(e.test_text(), "``````");
    assert_eq!(e.cursor(), TextPosition::new(0, 3));
}

#[test]
fn test_autoclose_triple_double_quote() {
    let mut e = ed("");
    e.insert_char('"');
    e.insert_char('"');
    e.insert_char('"');
    assert_eq!(e.test_text(), "\"\"\"\"\"\"");
    assert_eq!(e.cursor(), TextPosition::new(0, 3));
}

#[test]
fn test_autoclose_triple_single_quote() {
    let mut e = ed_named("", "/tmp/test.py");
    e.insert_char('\'');
    e.insert_char('\'');
    e.insert_char('\'');
    assert_eq!(e.test_text(), "''''''");
    assert_eq!(e.cursor(), TextPosition::new(0, 3));
}

#[test]
fn test_autoclose_triple_paren_no_fence() {
    let mut e = ed("");
    e.insert_char('(');
    e.insert_char('(');
    e.insert_char('(');
    assert_eq!(e.test_text(), "((()))");
    assert_eq!(e.cursor(), TextPosition::new(0, 3));
}

#[test]
fn test_autoclose_backspace_deletes_backtick_pair() {
    let mut e = ed("");
    e.insert_char('`');
    e.backspace();
    assert_eq!(e.test_text(), "");
}

#[test]
fn test_autoclose_backspace_only_deletes_pair_when_matched() {
    let mut e = ed("(x");
    e.set_cursor(TextPosition::new(0, 1));
    e.backspace();
    assert_eq!(e.test_text(), "x");
}

#[test]
fn test_autoclose_wraps_selection_paren() {
    let mut e = ed("hello");
    set_sel(
        &mut e,
        Selection {
            anchor: TextPosition::new(0, 0),
            cursor: TextPosition::new(0, 5),
        },
    );
    e.insert_char('(');
    assert_eq!(e.test_text(), "(hello)");
    let (s, end) = selection(&e).ordered();
    assert_eq!(s, TextPosition::new(0, 1));
    assert_eq!(end, TextPosition::new(0, 6));
}

#[test]
fn test_autoclose_wraps_selection_bracket() {
    let mut e = ed("world");
    set_sel(
        &mut e,
        Selection {
            anchor: TextPosition::new(0, 0),
            cursor: TextPosition::new(0, 5),
        },
    );
    e.insert_char('[');
    assert_eq!(e.test_text(), "[world]");
}

#[test]
fn test_autoclose_wraps_selection_brace() {
    let mut e = ed("abc");
    set_sel(
        &mut e,
        Selection {
            anchor: TextPosition::new(0, 0),
            cursor: TextPosition::new(0, 3),
        },
    );
    e.insert_char('{');
    assert_eq!(e.test_text(), "{abc}");
}

#[test]
fn test_autoclose_wraps_selection_double_quote() {
    let mut e = ed("text");
    set_sel(
        &mut e,
        Selection {
            anchor: TextPosition::new(0, 0),
            cursor: TextPosition::new(0, 4),
        },
    );
    e.insert_char('"');
    assert_eq!(e.test_text(), "\"text\"");
}

#[test]
fn test_autoclose_wraps_selection_single_quote() {
    let mut e = ed("text");
    set_sel(
        &mut e,
        Selection {
            anchor: TextPosition::new(0, 0),
            cursor: TextPosition::new(0, 4),
        },
    );
    e.insert_char('\'');
    assert_eq!(e.test_text(), "'");
}

#[test]
fn test_autoclose_wraps_selection_single_quote_in_rust() {
    let mut e = ed_named("text", "/tmp/test.rs");
    set_sel(
        &mut e,
        Selection {
            anchor: TextPosition::new(0, 0),
            cursor: TextPosition::new(0, 4),
        },
    );
    e.insert_char('\'');
    assert_eq!(e.test_text(), "'text'");
}

#[test]
fn test_autoclose_wraps_partial_selection() {
    let mut e = ed("hello world");
    set_sel(
        &mut e,
        Selection {
            anchor: TextPosition::new(0, 6),
            cursor: TextPosition::new(0, 11),
        },
    );
    e.insert_char('(');
    assert_eq!(e.test_text(), "hello (world)");
}

#[test]
fn test_autoclose_wraps_multiline_selection() {
    let mut e = ed("foo\nbar");
    set_sel(
        &mut e,
        Selection {
            anchor: TextPosition::new(0, 0),
            cursor: TextPosition::new(1, 3),
        },
    );
    e.insert_char('{');
    assert_eq!(e.test_text(), "{foo\nbar}");
}

#[test]
fn test_autoclose_non_pair_char_replaces_selection() {
    let mut e = ed("hello");
    set_sel(
        &mut e,
        Selection {
            anchor: TextPosition::new(0, 0),
            cursor: TextPosition::new(0, 5),
        },
    );
    e.insert_char('x');
    assert_eq!(e.test_text(), "x");
}

#[test]
fn test_autoclose_type_inside_pair() {
    let mut e = ed("");
    e.insert_char('(');
    e.insert_char('x');
    assert_eq!(e.test_text(), "(x)");
    assert_eq!(e.cursor(), TextPosition::new(0, 2));
}

#[test]
fn test_autoclose_nested_pairs() {
    let mut e = ed("");
    e.insert_char('(');
    e.insert_char('[');
    assert_eq!(e.test_text(), "([])");
    assert_eq!(e.cursor(), TextPosition::new(0, 2));
    e.insert_char(']');
    assert_eq!(e.test_text(), "([])");
    assert_eq!(e.cursor(), TextPosition::new(0, 3));
    e.insert_char(')');
    assert_eq!(e.test_text(), "([])");
    assert_eq!(e.cursor(), TextPosition::new(0, 4));
}

#[test]
fn test_autoclose_at_end_of_line() {
    let mut e = ed("hello");
    e.set_cursor(TextPosition::new(0, 5));
    e.insert_char('(');
    assert_eq!(e.test_text(), "hello()");
    assert_eq!(e.cursor(), TextPosition::new(0, 6));
}

#[test]
fn test_autoclose_no_pair_before_digit() {
    let mut e = ed("42");
    e.insert_char('(');
    assert_eq!(e.test_text(), "(42");
}

#[test]
fn test_autoclose_wraps_backward_selection() {
    let mut e = ed("hello");
    set_sel(
        &mut e,
        Selection {
            anchor: TextPosition::new(0, 5),
            cursor: TextPosition::new(0, 0),
        },
    );
    e.insert_char('(');
    assert_eq!(e.test_text(), "(hello)");
}


#[test]
fn test_smart_paste_on_blank_line_clears_autoindent() {
    let mut e = ed("    fn main() {\n        ");
    e.set_cursor(TextPosition::new(1, 8));
    e.paste_text("if true {\n    println!(\"hi\");\n}");
    assert_eq!(
        e.test_text(),
        "    fn main() {\nif true {\n    println!(\"hi\");\n}"
    );
}

#[test]
fn test_smart_paste_single_line_no_change() {
    let mut e = ed("    ");
    e.set_cursor(TextPosition::new(0, 4));
    e.paste_text("hello");
    assert_eq!(e.test_text(), "    hello");
}

#[test]
fn test_smart_paste_blank_line_cleared() {
    let mut e = ed("  ");
    e.set_cursor(TextPosition::new(0, 2));
    e.paste_text("a\n  b\n  c");
    assert_eq!(e.test_text(), "a\n  b\n  c");
}

#[test]
fn test_smart_paste_preserves_relative_indent() {
    let mut e = ed("    ");
    e.set_cursor(TextPosition::new(0, 4));
    e.paste_text("if true {\n  nested\n}");
    assert_eq!(e.test_text(), "if true {\n  nested\n}");
}

#[test]
fn test_smart_paste_empty_lines_preserved() {
    let mut e = ed("    ");
    e.set_cursor(TextPosition::new(0, 4));
    e.paste_text("a\n\nb");
    assert_eq!(e.test_text(), "a\n\nb");
}

#[test]
fn test_smart_paste_zero_indent_no_change() {
    let mut e = ed("");
    e.paste_text("a\nb\nc");
    assert_eq!(e.test_text(), "a\nb\nc");
}

#[test]
fn test_smart_paste_with_selection_replaces() {
    let mut e = ed("    old stuff\n    more old");
    set_sel(
        &mut e,
        Selection {
            anchor: TextPosition::new(0, 4),
            cursor: TextPosition::new(0, 13),
        },
    );
    e.paste_text("new\n    thing");
    assert_eq!(e.test_text(), "new\n    thing\n    more old");
}

#[test]
fn test_smart_paste_cursor_position() {
    let mut e = ed("");
    e.paste_text("hello\nworld");
    assert_eq!(e.cursor(), TextPosition::new(1, 5));
}

#[test]
fn test_smart_paste_empty_string() {
    let mut e = ed("hello");
    e.set_cursor(TextPosition::new(0, 5));
    e.paste_text("");
    assert_eq!(e.test_text(), "hello");
    assert_eq!(e.cursor(), TextPosition::new(0, 5));
}

#[test]
fn test_smart_paste_all_empty_continuation_lines() {
    let mut e = ed("    ");
    e.set_cursor(TextPosition::new(0, 4));
    e.paste_text("a\n\n\n");
    assert_eq!(e.test_text(), "a\n\n\n");
}

#[test]
fn test_smart_paste_tabs_in_pasted_text() {
    let mut e = ed("  ");
    e.set_cursor(TextPosition::new(0, 2));
    e.paste_text("a\n\tb");
    assert_eq!(e.test_text(), "a\n\tb");
}

#[test]
fn test_smart_paste_yaml_structure_preserved() {
    let mut e = ed("  ");
    e.set_cursor(TextPosition::new(0, 2));
    e.paste_text("root:\n  child:\n    grandchild: value");
    assert_eq!(e.test_text(), "root:\n  child:\n    grandchild: value");
}

#[test]
fn test_smart_paste_yaml_at_col_zero_unchanged() {
    let mut e = ed("");
    e.paste_text("root:\n  child:\n    grandchild: value");
    assert_eq!(e.test_text(), "root:\n  child:\n    grandchild: value");
}

#[test]
fn test_smart_paste_non_blank_line_uses_cursor_col() {
    let mut e = ed("    prefix");
    e.set_cursor(TextPosition::new(0, 10));
    e.paste_text("key:\n  val");
    assert_eq!(e.test_text(), "    prefixkey:\n            val");
}

#[test]
fn test_cancelled_save_as_during_quit_returns_to_editing() {
    let mut e = ed("hello");
    e.document.is_dirty = true;
    e.try_quit();
    e.handle_key(Key::Char('y'));
    assert_eq!(e.modal, Modal::QuitAfterSave);

    e.dispatch_event(EditorEvent::Key(Key::Esc));

    assert_eq!(e.modal, Modal::None);
    assert!(e.running);
    e.dispatch_event(EditorEvent::Key(Key::Char('n')));
    assert!(e.running);
    assert_eq!(e.test_text(), "nhello");
}

#[test]
fn test_failed_save_as_keeps_previous_file_path() {
    let dir = std::env::temp_dir().join("e_test_save_as_failure");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let blocker = dir.join("not_a_dir");
    std::fs::write(&blocker, b"x").unwrap();
    let mut e = ed("hello");

    let outcome = e.save_to(blocker.join("sub/out.txt").to_str().unwrap());

    assert_eq!(outcome, file_session::SaveOutcome::Failed);
    assert_eq!(e.document.file_path, None);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_quit_with_failed_save_keeps_editor_open() {
    let dir = std::env::temp_dir().join("e_test_quit_failed_save");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let blocker = dir.join("not_a_dir");
    std::fs::write(&blocker, b"x").unwrap();
    let mut e = ed_named("hello", blocker.join("sub/out.txt").to_str().unwrap());
    e.document.is_dirty = true;
    e.try_quit();

    e.handle_key(Key::Char('y'));

    assert!(e.running, "unsaved text must not be discarded after a failed save");
    assert_eq!(e.modal, Modal::None);
    assert!(e.document.is_dirty);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_save_refuses_file_changed_on_disk() {
    let dir = std::env::temp_dir().join("e_test_save_conflict");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("test.txt");
    std::fs::write(&path, b"original\n").unwrap();
    let mut e = ed_named("original\n", path.to_str().unwrap());
    std::fs::write(&path, b"theirs\n").unwrap();
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(std::time::SystemTime::UNIX_EPOCH)
        .unwrap();

    let outcome = e.save_file();

    assert_eq!(outcome, file_session::SaveOutcome::Failed);
    assert!(e.status_message.contains("changed on disk"));
    assert_eq!(std::fs::read(&path).unwrap(), b"theirs\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_save_preserves_invalid_utf8_and_crlf_bytes() {
    let dir = std::env::temp_dir().join("e_test_save_bytes");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("latin1.txt");
    let bytes = b"caf\xe9\r\nline2\xc2\xa0\r\n";
    std::fs::write(&path, bytes).unwrap();
    let mut e = ed_named("", path.to_str().unwrap());
    e.document = Document::new(bytes.to_vec(), Some(path.to_str().unwrap().to_string()));
    e.document.is_dirty = true;

    e.save_file();

    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_replace_all_refuses_invalid_utf8() {
    let mut e = ed("");
    e.document = Document::new(b"foo \xff bar foo".to_vec(), None);

    e.replace_all("foo", "baz");

    assert_eq!(e.document.buffer.contents(), b"foo \xff bar foo");
    assert!(e.status_message.contains("invalid UTF-8"));
}

#[test]
fn test_find_reports_lines_skipped_for_invalid_utf8() {
    let mut e = ed("");
    e.document = Document::new(b"foo\n\xff foo\nbar\n".to_vec(), None);

    e.find_next_from_submit("zzz");

    assert!(e.status_message.contains("no matches"));
    assert!(e.status_message.contains("1 lines with invalid UTF-8 skipped"));
}
