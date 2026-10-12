use std::fs::File;
use std::io::{self, Read, Write};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::clipboard::Clipboard;
use crate::command::{CommandAction, CommandRegistry};
use crate::command_buffer::{CommandBuffer, CommandBufferMode, CommandBufferResult};
use crate::document::{Document, TextEdit};
use crate::find::FindState;
use crate::input::{self, EditorEvent, InputParser, Key, MouseButton, MouseEvent, MouseMods};
use crate::keybind::{EditorAction, KeybindingTable};
use crate::mouse::MouseState;
use crate::render::{Renderer, gutter_width};
use crate::selection::{
    CaretSet, CaretSnapshot, Selection, TextPosition, is_word_char, next_word_boundary,
    prev_word_boundary,
};
use crate::terminal::TerminalGuard;
use crate::viewport::Viewport;

mod caret_edits;
mod clipboard_ops;
mod editing;
mod file_session;
mod movement;

use caret_edits::PlannedCaretEdit;
const SCROLL_LINES: usize = 3;

fn auto_close_char(c: char, lang_name: Option<&str>) -> Option<char> {
    match c {
        '(' => Some(')'),
        '[' => Some(']'),
        '{' => Some('}'),
        '"' => Some('"'),
        // Skip single-quote autoclose for plain Text and Markdown: apostrophes
        // and contractions make it far too noisy there.
        '\'' if lang_name.is_some_and(|n| n != "Markdown") => Some('\''),
        '`' => Some('`'),
        _ => None,
    }
}

fn is_close_char(c: char) -> bool {
    matches!(c, ')' | ']' | '}' | '"' | '\'' | '`')
}

fn common_prefix(strings: &[&str]) -> String {
    if strings.is_empty() {
        return String::new();
    }
    let first = strings[0];
    let mut len = first.len();
    for s in &strings[1..] {
        len = len.min(s.len());
        for (i, (a, b)) in first.bytes().zip(s.bytes()).enumerate() {
            if a != b {
                len = len.min(i);
                break;
            }
        }
    }
    first[..len].to_string()
}

// The editor keeps character positions in its own selection model. Convert
// only at this storage-agnostic matching boundary; the reusable highlighter
// remains byte-oriented and does not depend on editor types.
fn find_bracket_match(
    pos: TextPosition,
    get_line: &mut impl FnMut(usize, &mut Vec<u8>),
    scratch: &mut Vec<u8>,
    line_count: usize,
) -> Option<TextPosition> {
    hi_lite::find_bracket_match(
        hi_lite::TextPosition::new(pos.line, pos.column),
        get_line,
        scratch,
        line_count,
    )
    .map(|position| TextPosition::new(position.line, position.column))
}

fn find_quote_match(
    pos: TextPosition,
    get_line: &mut impl FnMut(usize, &mut Vec<u8>),
    scratch: &mut Vec<u8>,
    line_count: usize,
) -> Option<TextPosition> {
    hi_lite::find_quote_match(
        hi_lite::TextPosition::new(pos.line, pos.column),
        get_line,
        scratch,
        line_count,
    )
    .map(|position| TextPosition::new(position.line, position.column))
}

pub struct Editor {
    document: Document,
    carets: CaretSet,
    viewport: Viewport,
    renderer: Renderer,
    clipboard: Clipboard,
    commands: CommandRegistry,
    keybindings: KeybindingTable,
    command_buffer: CommandBuffer,
    line_numbers_visible: bool,
    status_message: String,
    status_time: Option<Instant>,
    running: bool,
    /// The confirmation the next key answers, if any.
    modal: Modal,
    mouse: MouseState,
    find: FindState,
    /// Bytes held while the sudo password prompt is open.
    sudo_save: Option<SudoSave>,
    /// True when stdin was a pipe (e.g. `git show | e`).
    piped_stdin: bool,
    /// Cached file mtime for external modification detection.
    file_modification_time: Option<std::time::SystemTime>,
    /// Problems found while quitting (state that could not be stored). They
    /// are printed after the terminal is restored, where the user can see them.
    shutdown_warnings: Vec<String>,
    /// Cached status-left string; reused each frame to avoid per-draw allocation.
    status_left_text_cache: String,
    /// Scratch buffer for line_text_into; avoids per-call allocation.
    line_text_scratch: Vec<u8>,
}


/// Editor-level confirmation that captures the next key. Command-buffer prompts
/// and find navigation are not listed here: they own their own state next to the
/// text and matches they edit, and the key routing checks them separately.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Modal {
    None,
    /// "Save changes? (y/n)" for a dirty quit.
    ConfirmQuit,
    /// A save-as or sudo-password prompt opened by answering yes to the quit
    /// confirmation; the editor exits once the save completes and goes back to
    /// normal editing if the prompt is cancelled or the save fails.
    QuitAfterSave,
    /// "Changed on disk. Reload? (y/n)"
    ConfirmReload,
}

/// A save that hit a permission error and waits for the sudo password.
struct SudoSave {
    path: String,
    cleaned: Vec<u8>,
}

impl Editor {
    pub fn new(text: Vec<u8>, file_path: Option<String>, piped_stdin: bool) -> Self {
        let (w, h) = input::terminal_size().unwrap_or((80, 24));
        let mut keybindings = KeybindingTable::with_defaults();
        let mut startup_notes = Vec::new();
        if let Err(e) = keybindings.load_config() {
            startup_notes.push(e);
        }
        let loaded_len = text.len() as u64;
        let mut document = Document::new(text, file_path);
        let file_modification_time = document
            .file_path
            .as_ref()
            .and_then(|name| crate::file_io::file_modification_time(std::path::Path::new(name)));
        let mut restored_cursor = None;
        if let Some(ref name) = document.file_path {
            let path = std::path::Path::new(name);
            if path.exists()
                && let Err(e) =
                    crate::file_io::load_undo_history(path, &mut document.undo_stack, loaded_len)
            {
                startup_notes.push(format!("undo history not restored: {e}"));
            }
            restored_cursor = crate::file_io::load_cursor_position(path);
        }
        // Clamp restored cursor to buffer bounds
        let initial_cursor = if let Some(pos) = restored_cursor {
            let line_count = document.buffer.line_count();
            let line = pos.line.min(line_count.saturating_sub(1));
            let column = pos.column.min(document.buffer.line_character_count(line));
            TextPosition::new(line, column)
        } else {
            TextPosition::zero()
        };
        let mut editor = Self::from_parts(
            document,
            initial_cursor,
            Viewport::new(w, h),
            Clipboard::detect(),
            keybindings,
            piped_stdin,
            file_modification_time,
        );
        if !startup_notes.is_empty() {
            editor.set_status(startup_notes.join("; "));
        }
        editor
    }

    /// Assemble an editor from already-prepared collaborators. `new` supplies
    /// the real terminal, clipboard and config; tests supply hermetic ones.
    fn from_parts(
        document: Document,
        initial_cursor: TextPosition,
        viewport: Viewport,
        clipboard: Clipboard,
        keybindings: KeybindingTable,
        piped_stdin: bool,
        file_modification_time: Option<std::time::SystemTime>,
    ) -> Self {
        Self {
            document,
            carets: CaretSet::new(initial_cursor),
            viewport,
            renderer: Renderer::new(),
            clipboard,
            commands: CommandRegistry::new(),
            keybindings,
            command_buffer: CommandBuffer::new(),
            line_numbers_visible: true,
            status_message: String::new(),
            status_time: None,
            running: true,
            modal: Modal::None,
            mouse: MouseState::new(),
            find: FindState::new(),
            sudo_save: None,
            piped_stdin,
            file_modification_time,
            shutdown_warnings: Vec::new(),
            status_left_text_cache: String::new(),
            line_text_scratch: Vec::new(),
        }
    }

    pub fn run(&mut self) -> io::Result<()> {
        // Center viewport on restored cursor position
        if self.cursor() != TextPosition::zero() {
            self.center_view_on_line(self.cursor().line);
        }

        let result = {
            let _terminal = TerminalGuard::enter()?;
            self.event_loop()
        };
        // The terminal is back to normal, so these reach the user's shell.
        for warning in self.shutdown_warnings.drain(..) {
            eprintln!("e: {warning}");
        }
        result
    }

    fn event_loop(&mut self) -> io::Result<()> {
        let mut stdout = io::stdout();

        let (tx, rx) = mpsc::channel::<EditorEvent>();
        let tx_input = tx.clone();
        let use_tty = self.piped_stdin;
        std::thread::spawn(move || {
            let mut reader: Box<dyn Read> = if use_tty {
                match File::open("/dev/tty") {
                    Ok(f) => Box::new(f),
                    Err(_) => return,
                }
            } else {
                Box::new(io::stdin())
            };
            let mut parser = InputParser::new();
            let mut buf = [0u8; 256];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        for &b in &buf[..n] {
                            if let Some(ev) = parser.advance(b)
                                && tx_input.send(ev).is_err()
                            {
                                return;
                            }
                        }
                        // After each read burst, flush pending bare ESC.
                        // Terminal emulators send escape sequences atomically,
                        // so a pending ESC means the user pressed Escape alone.
                        if let Some(ev) = parser.flush()
                            && tx_input.send(ev).is_err()
                        {
                            return;
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        crate::signal::register_sigwinch()?;
        crate::signal::register_termination()?;

        while self.running {
            if crate::signal::termination_requested() {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "terminated by signal",
                ));
            }

            // Expire status messages after 3 seconds
            if let Some(t) = self.status_time
                && t.elapsed().as_secs() >= 3
            {
                self.status_message.clear();
                self.status_time = None;
            }

            self.draw(&mut stdout)?;

            match rx.recv_timeout(Duration::from_millis(500)) {
                Ok(ev) => {
                    self.dispatch_event(ev);
                    // Drain all pending events before re-rendering so bursts
                    // (e.g. rapid scroll wheel) coalesce into a single frame.
                    while self.running {
                        match rx.try_recv() {
                            Ok(ev) => self.dispatch_event(ev),
                            Err(_) => break,
                        }
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if crate::signal::take_sigwinch()
                        && let Ok((w, h)) = input::terminal_size()
                    {
                        self.resize_view(w, h);
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }

        Ok(())
    }

    /// Resize the terminal while keeping the logical center near the same content.
    fn resize_view(&mut self, width: u16, height: u16) {
        let line_count = self.document.buffer.line_count();
        let gutter = if self.line_numbers_visible {
            gutter_width(line_count)
        } else {
            0
        };
        let anchor = {
            let buf = &self.document.buffer;
            let mut widths = |line: usize| buf.display_column_at(line, usize::MAX);
            self.viewport.center_anchor(line_count, gutter, &mut widths)
        };

        self.viewport.width = width;
        self.viewport.height = height;

        if let Some(anchor) = anchor {
            let buf = &self.document.buffer;
            let mut widths = |line: usize| buf.display_column_at(line, usize::MAX);
            self.viewport
                .center_on_anchor(anchor, line_count, gutter, &mut widths);
        }
        self.renderer.force_full_redraw();
    }

    fn dispatch_event(&mut self, ev: EditorEvent) {
        match ev {
            EditorEvent::Key(key) => {
                if self.command_buffer.active {
                    self.handle_cmd_key(key);
                } else {
                    self.handle_key(key);
                }
            }
            EditorEvent::Mouse(mouse, mods) => {
                if !self.command_buffer.active {
                    if self.find.active {
                        self.exit_find_mode();
                    }
                    self.handle_mouse(mouse, mods);
                }
            }
            EditorEvent::Paste(text) => {
                if self.command_buffer.active {
                    let result = self.command_buffer.insert_str(&text);
                    let mode = self.command_buffer.mode;
                    self.handle_cmd_result(mode, result);
                } else {
                    self.paste_text(&text);
                }
            }
            EditorEvent::FocusIn => {
                self.check_external_modification();
            }
        }
    }

    fn set_status(&mut self, msg: String) {
        self.status_message = msg;
        self.status_time = Some(Instant::now());
    }

    fn cursor(&self) -> TextPosition {
        self.carets.cursor()
    }

    fn set_cursor(&mut self, pos: TextPosition) {
        self.carets.set_single_selection(Selection::caret(pos));
    }

    fn set_cursor_preserving_desired_col(&mut self, pos: TextPosition) {
        let desired_column = self.desired_column();
        self.carets.set_single_selection(Selection::caret(pos));
        self.set_desired_col(desired_column);
    }

    fn set_selection(&mut self, selection: Selection) {
        self.carets.set_single_selection(selection);
    }

    fn restore_carets(&mut self, snapshot: CaretSnapshot) {
        self.carets.restore(snapshot);
    }

    fn has_selection(&self) -> bool {
        !self.carets.selection().is_empty()
    }

    fn selection(&self) -> Selection {
        self.carets.selection()
    }

    fn desired_column(&self) -> Option<usize> {
        self.carets.primary().desired_column
    }

    fn set_desired_col(&mut self, desired_column: Option<usize>) {
        self.carets.primary_mut().desired_column = desired_column;
    }

    fn move_cursor(&mut self, pos: TextPosition, extend: bool) {
        if extend {
            self.carets.primary_mut().selection.cursor = pos;
        } else {
            self.set_cursor_preserving_desired_col(pos);
        }
    }

    fn mutate_carets(
        &mut self,
        normalize: bool,
        mut f: impl FnMut(&Document, &mut Vec<u8>, &mut crate::selection::Caret),
    ) {
        let document = &self.document;
        let mut scratch = std::mem::take(&mut self.line_text_scratch);
        for caret in self.carets.iter_mut() {
            f(document, &mut scratch, caret);
        }
        self.line_text_scratch = scratch;
        if normalize {
            self.carets.normalize();
        }
    }


    fn indent_snap_left_for(document: &Document, line: usize, column: usize) -> usize {
        let ls = document.buffer.line_start(line);
        let le = document.buffer.line_end(line);
        let mut leading_ws = 0;
        while ls + leading_ws < le {
            match document.buffer.byte_at(ls + leading_ws) {
                b' ' | b'\t' => leading_ws += 1,
                _ => break,
            }
        }
        if column <= leading_ws
            && column >= 1
            && (0..column).all(|i| document.buffer.byte_at(ls + i) == b' ')
        {
            return (column - 1) / 2 * 2;
        }
        column - 1
    }

    fn indent_snap_right_for(document: &Document, line: usize, column: usize) -> usize {
        let ls = document.buffer.line_start(line);
        let le = document.buffer.line_end(line);
        let mut leading_ws = 0;
        let mut all_spaces = true;
        while ls + leading_ws < le {
            match document.buffer.byte_at(ls + leading_ws) {
                b' ' => leading_ws += 1,
                b'\t' => {
                    all_spaces = false;
                    leading_ws += 1;
                }
                _ => break,
            }
        }
        if column < leading_ws && all_spaces {
            return ((column / 2 + 1) * 2).min(leading_ws);
        }
        column + 1
    }

    fn bracket_jump_target_at(
        document: &Document,
        scratch: &mut Vec<u8>,
        pos: TextPosition,
    ) -> Option<TextPosition> {
        find_bracket_match(
            pos,
            &mut |line_idx, buf| document.buffer.line_text_into(line_idx, buf),
            scratch,
            document.buffer.line_count(),
        )
    }

    fn use_tab_indent(&self) -> bool {
        self.document.file_path.as_ref().is_some_and(|f| {
            f.ends_with(".c") || f.ends_with(".h") || f.ends_with(".go") || f.contains("Makefile")
        })
    }

    fn draw(&mut self, out: &mut impl Write) -> io::Result<()> {
        let line_count = self.document.buffer.line_count();
        let gw = if self.line_numbers_visible {
            gutter_width(line_count)
        } else {
            0
        };

        let display_col = self.cursor_display_col();
        let cursor_line = self.cursor().line;
        let mut line_display_width =
            |line: usize| -> usize { self.document.buffer.display_column_at(line, usize::MAX) };
        self.viewport
            .ensure_cursor_visible(cursor_line, display_col, gw, &mut line_display_width);

        let lang = self.document.detect_language();
        let lang_name = lang.map(|l| l.name()).unwrap_or("Text");
        let selection = if self.selection().is_empty() {
            None
        } else {
            Some(self.selection())
        };
        let secondary_selections: Vec<Selection> = self
            .carets
            .iter()
            .enumerate()
            .filter_map(|(idx, caret)| {
                if idx == self.carets.primary_index() || caret.selection.is_empty() {
                    None
                } else {
                    Some(caret.selection)
                }
            })
            .collect();
        let secondary_cursors: Vec<TextPosition> = self
            .carets
            .iter()
            .enumerate()
            .filter_map(|(idx, caret)| {
                if idx == self.carets.primary_index() || !caret.selection.is_empty() {
                    None
                } else {
                    Some(caret.selection.cursor)
                }
            })
            .collect();
        let line_numbers_visible = self.line_numbers_visible;

        // All &mut self calls must happen before we borrow status_left_text_cache.
        let bracket_pair = self.find_matching_bracket();

        // Refresh viewport matches on every draw (cheap — only scans visible lines).
        if self.find.re.is_some() {
            self.find
                .refresh_viewport_matches(&self.document.buffer, &self.viewport);
        }

        let language = lang;
        self.renderer.set_language(language);
        if self.carets.is_multicursor() {
            self.renderer.force_full_redraw();
        }

        // Pure reads — no more &mut self after this point.
        let find_matches = if !self.find.matches.is_empty() {
            Some(self.find.matches.as_slice())
        } else {
            None
        };
        let find_current = if self.find.active {
            self.find.current
        } else {
            None
        };

        let completions = &self.command_buffer.completions;

        let cmd_cursor = if self.command_buffer.active {
            Some(self.command_buffer.prompt.len() + self.command_buffer.cursor)
        } else {
            None
        };

        // Avoid cloning status_message: borrow it directly as &str.
        let display_line_owned;
        let cmd_ref: Option<&str> = if self.command_buffer.active {
            display_line_owned = self.command_buffer.display_line();
            Some(&display_line_owned)
        } else if !self.status_message.is_empty() {
            Some(&self.status_message)
        } else {
            None
        };

        // Rebuild status_left into the reused cache buffer (no allocation after warm-up).
        let name = self.document.file_path.as_deref().unwrap_or("[scratch]");
        Self::build_status_left(
            name,
            self.document.is_dirty,
            lang_name,
            &mut self.status_left_text_cache,
        );
        let status_left = &self.status_left_text_cache;
        let status_right = Self::status_right();

        self.renderer.render(
            out,
            &mut self.document.buffer,
            &self.viewport,
            cursor_line,
            display_col,
            line_numbers_visible,
            status_left,
            status_right,
            cmd_ref,
            selection,
            &secondary_selections,
            &secondary_cursors,
            find_matches,
            find_current,
            completions,
            cmd_cursor,
            self.find.active,
            bracket_pair,
        )
    }

    fn build_status_left(name: &str, dirty: bool, lang_name: &str, out: &mut String) {
        out.clear();
        out.push(' ');
        out.push_str(name);
        if dirty {
            out.push('*');
        }
        out.push_str(" [");
        out.push_str(lang_name);
        out.push(']');
    }

    #[cfg(test)]
    fn status_left(&self, lang_name: &str) -> String {
        let name = self.document.file_path.as_deref().unwrap_or("[scratch]");
        let mut s = String::new();
        Self::build_status_left(name, self.document.is_dirty, lang_name, &mut s);
        s
    }

    fn status_right() -> &'static str {
        concat!(" e v", env!("CARGO_PKG_VERSION"), " ")
    }

    fn center_view_on_line(&mut self, line: usize) {
        let gw = if self.line_numbers_visible {
            gutter_width(self.document.buffer.line_count())
        } else {
            0
        };
        let mut ldw = |l: usize| -> usize { self.document.buffer.display_column_at(l, usize::MAX) };
        self.viewport.center_on_line(line, &mut ldw, gw);
    }

    fn cursor_display_col(&self) -> usize {
        self.document
            .buffer
            .display_column_at(self.cursor().line, self.cursor().column)
    }

    fn find_matching_bracket(&mut self) -> Option<(TextPosition, TextPosition)> {
        let cursor = self.cursor();
        let line_count = self.document.buffer.line_count();
        // Reuse the editor's scratch buffer to avoid a per-frame allocation.
        let mut scratch = std::mem::take(&mut self.line_text_scratch);
        if let Some(match_pos) = find_bracket_match(
            cursor,
            &mut |line_idx, buf| self.document.buffer.line_text_into(line_idx, buf),
            &mut scratch,
            line_count,
        ) {
            self.line_text_scratch = scratch;
            return Some((cursor, match_pos));
        }
        let match_pos = find_quote_match(
            cursor,
            &mut |line_idx, buf| self.document.buffer.line_text_into(line_idx, buf),
            &mut scratch,
            line_count,
        );
        self.line_text_scratch = scratch;
        match_pos.map(|p| (cursor, p))
    }

    fn handle_key(&mut self, key: Key) {
        match self.modal {
            Modal::ConfirmQuit => {
                match key {
                    Key::Char('y') | Key::Char('Y') => self.save_and_quit(),
                    Key::Char('n') | Key::Char('N') => self.quit(),
                    _ => {
                        self.modal = Modal::None;
                        self.status_message.clear();
                        self.status_time = None;
                    }
                }
                return;
            }
            Modal::ConfirmReload => {
                match key {
                    Key::Char('y') | Key::Char('Y') => self.reload_file(),
                    _ => self.dismiss_reload(),
                }
                return;
            }
            // The save-as or password prompt is a command buffer, which
            // `dispatch_event` routes before this point.
            Modal::QuitAfterSave | Modal::None => {}
        }

        // Find navigation mode: up/down browse matches, anything else exits
        if self.find.active {
            match key {
                Key::Up => {
                    self.find_prev();
                    return;
                }
                Key::Down => {
                    self.find_next();
                    return;
                }
                Key::Esc => {
                    self.exit_find_mode();
                    self.clear_selection();
                    return;
                }
                _ => {
                    self.exit_find_mode();
                    // Fall through to process the key normally
                }
            }
        }

        let desired_column = match key {
            Key::Up | Key::Down | Key::PageUp | Key::PageDown => self.desired_column(),
            _ => None,
        };
        self.set_desired_col(desired_column);

        // Check keybinding table first
        if let Some(action) = self.keybindings.lookup(key).cloned() {
            match action {
                EditorAction::Save => {
                    self.save_file();
                }
                EditorAction::Quit => self.try_quit(),
                EditorAction::Undo => self.undo(),
                EditorAction::Redo => self.redo(),
                EditorAction::SelectAll => self.select_all(),
                EditorAction::Copy => {
                    self.copy();
                }
                EditorAction::Cut => self.cut(),
                EditorAction::Paste => self.paste(),
                EditorAction::KillLine => self.kill_line(),
                EditorAction::GotoTop => self.goto_top(),
                EditorAction::GotoEnd => self.goto_end(),
                EditorAction::ToggleRuler => {
                    self.line_numbers_visible = !self.line_numbers_visible;
                    self.renderer.force_full_redraw();
                }
                EditorAction::CommandPalette => {
                    self.command_buffer
                        .open(CommandBufferMode::Command, "> ", "");
                }
                EditorAction::GotoLine => {
                    self.command_buffer
                        .open(CommandBufferMode::Goto, "goto: ", "");
                }
                EditorAction::Find => {
                    let prefill = if self.has_selection() {
                        let (start, end) = self.selection().ordered();
                        let text = self.document.text_in_range(start, end);
                        let s = String::from_utf8_lossy(&text).to_string();
                        if s.len() <= 100 { s } else { String::new() }
                    } else {
                        String::new()
                    };
                    self.command_buffer
                        .open(CommandBufferMode::Find, "find: ", &prefill);
                    self.find.clear();
                    self.find.search_start = self.cursor();
                }
                EditorAction::CtrlBackspace => self.ctrl_backspace(),
                EditorAction::ToggleComment => self.toggle_comment(),
                EditorAction::DuplicateLine => self.duplicate_line(),
                EditorAction::SelectWord => self.select_word_at(self.cursor()),
            }
            return;
        }

        // Non-configurable keys
        match key {
            // Shift+Arrow
            Key::ShiftUp => self.move_up_extend(),
            Key::ShiftDown => self.move_down_extend(),
            Key::ShiftLeft => self.move_left_extend(),
            Key::ShiftRight => self.move_right_extend(),

            // Movement
            Key::Up => self.move_up(),
            Key::Down => self.move_down(),
            Key::Left => self.move_left(),
            Key::Right => self.move_right(),
            Key::Home => self.move_home(),
            Key::End => self.move_end(),
            Key::CtrlLeft => self.word_left(),
            Key::CtrlRight => self.word_right(),
            Key::CtrlUp => self.move_up(),
            Key::CtrlDown => self.move_down(),
            Key::CtrlShiftUp => self.select_above(),
            Key::CtrlShiftDown => self.select_below(),
            Key::CtrlShiftLeft => self.word_left_extend(),
            Key::CtrlShiftRight => self.word_right_extend(),
            Key::PageUp => self.page_up(),
            Key::PageDown => self.page_down(),

            Key::Esc => {
                self.clear_selection();
                self.find.clear();
            }

            // Editing
            Key::Delete => self.delete_forward(),
            Key::Backspace => self.backspace(),
            Key::Char('\t') => self.insert_tab(),
            Key::BackTab => self.dedent(),
            Key::Char('\n') => self.insert_newline(),
            // Ctrl+J (0x0A) arrives as Key::Null via CtrlJReader (0x0A → 0x00).
            Key::Null => self.duplicate_line(),
            Key::Char(c) => self.insert_char(c),
            _ => {}
        }
    }


    // -- command buffer key handling ----------------------------------------

    fn handle_cmd_key(&mut self, key: Key) {
        // Key::Null = Ctrl+J (0x0A via CtrlJReader); treat as Enter in command buffer.
        let key = if key == Key::Null {
            Key::Char('\n')
        } else {
            key
        };
        let mode = self.command_buffer.mode;
        let result = self.command_buffer.handle_key(key);
        self.handle_cmd_result(mode, result);
    }

    fn handle_cmd_result(&mut self, mode: CommandBufferMode, result: CommandBufferResult) {
        match result {
            CommandBufferResult::Submit(val) => {
                self.command_buffer.close();
                match mode {
                    CommandBufferMode::Command => self.execute_command(&val),
                    CommandBufferMode::Find => self.find_next_from_submit(&val),
                    CommandBufferMode::Goto => {
                        let cmd = format!("goto {}", val);
                        self.execute_command(&cmd);
                    }
                    CommandBufferMode::Prompt => {
                        // save-as prompt
                        let outcome = self.save_to(&val);
                        self.finish_quit_after_save(outcome);
                    }
                    CommandBufferMode::SudoSave => {
                        let outcome = self.save_file_sudo(&val);
                        self.finish_quit_after_save(outcome);
                    }
                }
            }
            CommandBufferResult::Cancel => {
                self.command_buffer.close();
                if mode == CommandBufferMode::Find {
                    self.find.clear();
                    self.status_message.clear();
                    self.status_time = None;
                }
                if mode == CommandBufferMode::SudoSave {
                    self.cancel_sudo_save();
                }
                if self.modal == Modal::QuitAfterSave {
                    self.modal = Modal::None;
                    self.set_status("Quit cancelled".to_string());
                }
            }
            CommandBufferResult::Changed(val) => {
                if mode == CommandBufferMode::Find {
                    self.update_find_highlights(&val);
                    if let Some((_, end)) = self.find.current {
                        self.set_cursor(end);
                        self.center_view_on_line(end.line);
                        self.set_find_status();
                    }
                }
            }
            CommandBufferResult::TabComplete => {
                if mode == CommandBufferMode::Command {
                    self.complete_command();
                }
            }
            CommandBufferResult::Continue => {}
        }
    }

    fn complete_command(&mut self) {
        let input = self.command_buffer.input.trim().to_string();
        let names = self.commands.command_names();

        if input.is_empty() {
            // Show all commands
            self.command_buffer.completions = names.iter().map(|s| s.to_string()).collect();
        } else {
            let matches: Vec<&str> = names
                .iter()
                .filter(|n| n.starts_with(&input))
                .copied()
                .collect();

            match matches.len() {
                0 => {
                    self.command_buffer.completions.clear();
                }
                1 => {
                    // Single match — autocomplete
                    self.command_buffer.input = matches[0].to_string();
                    self.command_buffer.cursor = self.command_buffer.input.len();
                    self.command_buffer.completions.clear();
                }
                _ => {
                    // Multiple matches — show them and complete common prefix
                    self.command_buffer.completions =
                        matches.iter().map(|s| s.to_string()).collect();
                    let common = common_prefix(&matches);
                    if common.len() > input.len() {
                        self.command_buffer.input = common;
                        self.command_buffer.cursor = self.command_buffer.input.len();
                    }
                }
            }
        }
    }

    // -- commands -----------------------------------------------------------

    fn execute_command(&mut self, input: &str) {
        let action = self.commands.execute(input);
        match action {
            CommandAction::None => {}
            CommandAction::Save => {
                self.save_file();
            }
            CommandAction::SaveAs(name) => {
                self.save_to(&name);
            }
            CommandAction::Quit => self.quit(),
            CommandAction::Goto(n) => self.goto_line(n),
            CommandAction::ToggleRuler => {
                self.line_numbers_visible = !self.line_numbers_visible;
                self.renderer.force_full_redraw();
            }
            CommandAction::ReplaceAll {
                pattern,
                replacement,
            } => {
                self.replace_all(&pattern, &replacement);
            }
            CommandAction::Find(pattern) => {
                self.find.search_start = self.cursor();
                self.find_next_from_submit(&pattern);
            }
            CommandAction::ToggleComment => self.toggle_comment(),
            CommandAction::CommentOn => self.set_comment(true),
            CommandAction::CommentOff => self.set_comment(false),
            CommandAction::SelectAll => self.select_all(),
            CommandAction::Trim => self.strip_trailing_whitespace(),
            CommandAction::TabsToSpaces => self.tabs_to_spaces(),
            CommandAction::SpacesToTabs => self.spaces_to_tabs(),
            CommandAction::StatusMsg(msg) => self.set_status(msg),
        }
    }

    fn goto_line(&mut self, n: usize) {
        let line_count = self.document.buffer.line_count();
        let target = if n == 0 {
            0
        } else {
            (n - 1).min(line_count.saturating_sub(1))
        };
        self.set_cursor(TextPosition::new(target, 0));
        self.center_view_on_line(target);
        self.renderer.force_full_redraw();
    }

    fn goto_top(&mut self) {
        self.set_cursor(TextPosition::zero());
        self.renderer.force_full_redraw();
    }

    fn goto_end(&mut self) {
        let line_count = self.document.buffer.line_count();
        let last_line = line_count.saturating_sub(1);
        let last_col = self.document.buffer.line_character_count(last_line);
        self.set_cursor(TextPosition::new(last_line, last_col));
        self.renderer.force_full_redraw();
    }

    fn kill_line(&mut self) {
        let c = self.cursor();
        let line_count = self.document.buffer.line_count();
        if line_count == 0 {
            return;
        }
        self.document.seal_undo();
        let start = TextPosition::new(c.line, 0);
        let end = if c.line + 1 < line_count {
            TextPosition::new(c.line + 1, 0)
        } else {
            let len = self.document.buffer.line_character_count(c.line);
            TextPosition::new(c.line, len)
        };
        self.document.delete_range(start, end);
        self.document.seal_undo();
        // Clamp cursor
        let new_line_count = self.document.buffer.line_count();
        let new_line = c.line.min(new_line_count.saturating_sub(1));
        let new_col = self
            .document
            .buffer
            .line_character_count(new_line)
            .min(c.column);
        self.set_cursor(TextPosition::new(new_line, new_col));
    }

    // -- find ---------------------------------------------------------------

    fn update_find_highlights(&mut self, pattern: &str) {
        self.find
            .update_highlights_lazy(pattern, &self.document.buffer, &self.viewport);
    }

    fn find_next_from_submit(&mut self, pattern: &str) {
        self.find
            .update_highlights(pattern, &self.document.buffer, &self.viewport);
        if self.find.current.is_none() {
            let note = self.skipped_lines_note();
            self.set_status(format!("Find: no matches{note}"));
            return;
        }
        self.find.active = true;
        if let Some((_, end)) = self.find.current {
            self.set_cursor(end);
            self.center_view_on_line(end.line);
            self.set_find_status();
        }
    }

    fn find_next(&mut self) {
        let cursor = self.cursor();
        if let Some(m) = self.find.find_next(&self.document.buffer, cursor) {
            let (_, end) = m;
            self.set_cursor(end);
            self.center_view_on_line(end.line);
            self.set_find_status();
        }
    }

    fn find_prev(&mut self) {
        let cursor = self.cursor();
        if let Some(m) = self.find.find_prev(&self.document.buffer, cursor) {
            let (_, end) = m;
            self.set_cursor(end);
            self.center_view_on_line(end.line);
            self.set_find_status();
        }
    }

    fn set_find_status(&mut self) {
        self.status_message = format!("{}{}", self.find.status_text(), self.skipped_lines_note());
        self.status_time = None; // don't auto-expire while browsing
    }

    /// Find cannot match text that is not valid UTF-8; say so rather than let a
    /// missing match look like an absent one.
    fn skipped_lines_note(&self) -> String {
        match self.find.skipped_invalid_lines() {
            0 => String::new(),
            n => format!(" ({n} lines with invalid UTF-8 skipped)"),
        }
    }

    fn exit_find_mode(&mut self) {
        if let Some((start, end)) = self.find.exit() {
            self.set_selection(Selection {
                anchor: start,
                cursor: end,
            });
        }
        self.status_message.clear();
        self.status_time = None;
    }

    // -- replace all --------------------------------------------------------

    fn replace_all(&mut self, pattern: &str, replacement: &str) {
        let case_insensitive = pattern.chars().all(|c| !c.is_uppercase());
        let re = if case_insensitive {
            regex_lite::RegexBuilder::new(pattern)
                .case_insensitive(true)
                .build()
        } else {
            regex_lite::Regex::new(pattern)
        };
        let re = match re {
            Ok(r) => r,
            Err(e) => {
                self.set_status(format!("Invalid regex: {}", e));
                return;
            }
        };

        // Determine the range to operate on
        let (range_start, range_end) = if self.has_selection() {
            self.selection().ordered()
        } else {
            let line_count = self.document.buffer.line_count();
            let last_line = line_count.saturating_sub(1);
            let last_col = self.document.buffer.line_character_count(last_line);
            (TextPosition::zero(), TextPosition::new(last_line, last_col))
        };

        let text_bytes = self.document.text_in_range(range_start, range_end);
        // Rewriting through a lossy decode would turn every invalid byte in the
        // range into U+FFFD, far from any match, so refuse instead.
        let Ok(text) = std::str::from_utf8(&text_bytes) else {
            self.set_status("Replace all: text contains invalid UTF-8; nothing changed".to_string());
            return;
        };

        let count = re.find_iter(text).count();
        if count == 0 {
            self.set_status("Replaced 0 occurrences".to_string());
            return;
        }

        let new_text = re.replace_all(text, replacement).into_owned();

        self.document.seal_undo();
        self.document.replace_range_with_deleted(
            range_start,
            range_end,
            new_text.as_bytes(),
            text_bytes,
        );
        self.document.seal_undo();

        self.clear_selection();
        self.set_status(format!("Replaced {} occurrences", count));
    }

    // -- mouse handling -----------------------------------------------------

    fn handle_mouse(&mut self, mouse: MouseEvent, mods: MouseMods) {
        match mouse {
            MouseEvent::Press(MouseButton::Left, x, y) => self.mouse_press(x, y, mods),
            MouseEvent::Press(MouseButton::Right, x, y) if mods.ctrl => {
                self.mouse_press(x, y, mods)
            }
            MouseEvent::Press(MouseButton::Right, x, y)
                if cfg!(target_os = "macos") && !mods.ctrl =>
            {
                self.mouse_press(x, y, MouseMods { ctrl: true });
            }
            MouseEvent::Hold(x, y) => self.mouse_drag(x, y),
            MouseEvent::Release(_, _) => {
                self.mouse.release();
            }
            MouseEvent::Press(MouseButton::WheelUp, _, _) => self.scroll_up(),
            MouseEvent::Press(MouseButton::WheelDown, _, _) => self.scroll_down(),
            _ => {}
        }
    }

    fn screen_to_buffer_position(&self, x: u16, y: u16) -> TextPosition {
        crate::mouse::screen_to_buffer_position(
            x,
            y,
            &self.document.buffer,
            &self.viewport,
            self.line_numbers_visible,
        )
    }

    fn mouse_press(&mut self, x: u16, y: u16, mods: MouseMods) {
        let pos = self.screen_to_buffer_position(x, y);
        if mods.ctrl {
            self.carets.add_caret(pos);
            self.mouse.dragging = false;
            return;
        }
        let click_count = self.mouse.press(x, y);

        match click_count {
            1 => {
                self.set_cursor(pos);
                self.mouse.dragging = true;
            }
            2 => self.select_word_at(pos),
            3 => self.select_line_at(pos.line),
            _ => {}
        }
    }

    fn mouse_drag(&mut self, x: u16, y: u16) {
        if !self.mouse.dragging {
            return;
        }
        let pos = self.screen_to_buffer_position(x, y);
        self.carets.primary_mut().selection.cursor = pos;
    }

    fn select_word_at(&mut self, pos: TextPosition) {
        self.document
            .buffer
            .line_text_into(pos.line, &mut self.line_text_scratch);
        let line_text = &self.line_text_scratch;
        if line_text.is_empty() {
            return;
        }
        let column = pos.column.min(line_text.len().saturating_sub(1));
        if column < line_text.len() && is_word_char(line_text[column]) {
            let mut start = column;
            while start > 0 && is_word_char(line_text[start - 1]) {
                start -= 1;
            }
            let mut end = column;
            while end < line_text.len() && is_word_char(line_text[end]) {
                end += 1;
            }
            self.set_selection(Selection {
                anchor: TextPosition::new(pos.line, end),
                cursor: TextPosition::new(pos.line, start),
            });
        }
    }

    fn select_line_at(&mut self, line: usize) {
        let line_count = self.document.buffer.line_count();
        if line >= line_count {
            return;
        }
        let end = if line + 1 < line_count {
            TextPosition::new(line + 1, 0)
        } else {
            let len = self.document.buffer.line_character_count(line);
            TextPosition::new(line, len)
        };
        self.set_selection(Selection {
            anchor: TextPosition::new(line, 0),
            cursor: end,
        });
    }

}

#[cfg(test)]
mod tests;
