//! Everything that ties the buffer to the file on disk: saving (including the
//! sudo fallback), quitting with unsaved changes, external-change reload, and
//! the cursor and undo state remembered between sessions.

use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use crate::command_buffer::CommandBufferMode;
use crate::document::Document;
use crate::file_io;
use crate::selection::TextPosition;

use super::{Editor, Modal, SudoSave};

/// How a save request ended, so callers that chain work after saving (quit)
/// can tell a finished save from one still waiting on the user.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum SaveOutcome {
    Saved,
    Failed,
    /// A prompt is open (save-as name or sudo password); the save continues
    /// from its submit handler.
    AwaitingInput,
}

impl Editor {
    pub(super) fn try_quit(&mut self) {
        if self.document.is_dirty {
            let name = self.document.file_path.as_deref().unwrap_or("[scratch]");
            self.status_message = format!("Save changes to {}? (y/n)", name);
            self.status_time = None; // don't expire this message
            self.modal = Modal::ConfirmQuit;
        } else {
            self.quit();
        }
    }

    /// Remember the session and stop the event loop.
    pub(super) fn quit(&mut self) {
        self.remember_session();
        self.running = false;
    }

    /// Answer to the quit confirmation: save, then quit once the save has
    /// completed. A failed save keeps the editor open rather than discarding.
    pub(super) fn save_and_quit(&mut self) {
        self.modal = Modal::None;
        match self.save_file() {
            SaveOutcome::Saved => self.quit(),
            SaveOutcome::Failed => {}
            SaveOutcome::AwaitingInput => self.modal = Modal::QuitAfterSave,
        }
    }

    /// Continue a quit that was waiting on a save-as or sudo prompt.
    pub(super) fn finish_quit_after_save(&mut self, outcome: SaveOutcome) {
        if self.modal != Modal::QuitAfterSave {
            return;
        }
        match outcome {
            SaveOutcome::Saved => {
                self.modal = Modal::None;
                self.quit();
            }
            SaveOutcome::Failed => self.modal = Modal::None,
            SaveOutcome::AwaitingInput => {}
        }
    }

    /// Store the cursor for next time, and the undo history when it still
    /// describes the file. Edits that were discarded, or a save that rewrote
    /// the bytes (trailing whitespace), leave the buffer different from the
    /// file, and offsets recorded against one would corrupt the other.
    pub(super) fn remember_session(&mut self) {
        let Some(name) = self.document.file_path.clone() else {
            return;
        };
        let path = Path::new(&name);
        if let Err(e) = file_io::save_cursor_position(path, self.cursor()) {
            self.shutdown_warnings
                .push(format!("cursor position not saved: {e}"));
        }
        if path.exists() {
            self.document.seal_undo();
            if let Err(e) = self.persist_undo_history(path) {
                self.shutdown_warnings
                    .push(format!("undo history not saved: {e}"));
            }
        }
    }

    fn persist_undo_history(&self, path: &Path) -> io::Result<()> {
        let contents = self.document.buffer.contents();
        if fs::read(path)? != contents {
            return Ok(());
        }
        file_io::save_undo_history(path, &self.document.undo_stack, contents.len() as u64)
    }

    pub(super) fn check_external_modification(&mut self) {
        if self.modal != Modal::None || self.command_buffer.active {
            return;
        }
        let Some(ref name) = self.document.file_path else {
            return;
        };
        let path = Path::new(name);
        let disk_mtime = file_io::file_modification_time(path);
        if disk_mtime != self.file_modification_time && disk_mtime.is_some() {
            let short = path
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_else(|| name.clone());
            self.status_message = format!("{} changed on disk. Reload? (y/n)", short);
            self.status_time = None;
            self.modal = Modal::ConfirmReload;
        }
    }

    pub(super) fn reload_file(&mut self) {
        self.modal = Modal::None;
        let Some(name) = self.document.file_path.clone() else {
            return;
        };
        let read_result = file_io::read_file(Path::new(&name));
        match read_result {
            Ok(data) => {
                let path = Path::new(&name);
                self.file_modification_time = file_io::file_modification_time(path);
                self.document = Document::new(data, Some(name));
                // Clamp cursor to valid position in new buffer
                let lc = self.document.buffer.line_count();
                if self.cursor().line >= lc {
                    let last = lc.saturating_sub(1);
                    self.set_cursor(TextPosition::new(
                        last,
                        self.document.buffer.line_character_count(last),
                    ));
                } else {
                    let cursor = self.cursor();
                    let len = self.document.buffer.line_character_count(cursor.line);
                    if cursor.column > len {
                        self.set_cursor(TextPosition::new(cursor.line, len));
                    }
                }
                self.set_desired_col(None);
                self.find.clear();
                self.renderer.force_full_redraw();
                self.set_status("Reloaded".to_string());
            }
            Err(e) => self.set_status(format!("Reload failed: {}", e)),
        }
    }

    pub(super) fn dismiss_reload(&mut self) {
        if let Some(ref name) = self.document.file_path {
            self.file_modification_time = file_io::file_modification_time(Path::new(name));
        }
        self.modal = Modal::None;
        self.status_message.clear();
        self.status_time = None;
    }

    /// Save to the document's file, or ask for a name when it has none.
    pub(super) fn save_file(&mut self) -> SaveOutcome {
        match self.document.file_path.clone() {
            Some(path) => self.save_to(&path),
            None => {
                self.command_buffer
                    .open(CommandBufferMode::Prompt, "Save as: ", "");
                SaveOutcome::AwaitingInput
            }
        }
    }

    /// Write the buffer to `path`. The document adopts `path` as its file only
    /// once the bytes are on disk, so a failed save-as leaves it unchanged.
    pub(super) fn save_to(&mut self, path: &str) -> SaveOutcome {
        let path_ref = Path::new(path);

        // mkdir -p for parent directory
        if let Some(parent) = path_ref.parent()
            && !parent.as_os_str().is_empty()
            && !parent.exists()
        {
            match fs::create_dir_all(parent) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
                    return self.start_sudo_save(path);
                }
                Err(e) => {
                    self.set_status(format!("Error creating dirs: {}", e));
                    return SaveOutcome::Failed;
                }
            }
        }

        // Saving under a different name overwrites whatever is there on
        // purpose; only the document's own file is protected against changes
        // made behind the editor's back.
        let expected_mtime = if self.document.file_path.as_deref() == Some(path) {
            self.file_modification_time
        } else {
            file_io::file_modification_time(path_ref)
        };
        let cleaned = file_io::clean_for_write(&self.document.buffer.contents());
        match file_io::write_file(path_ref, &cleaned, expected_mtime) {
            Ok(()) => {
                self.finish_save(path, "");
                SaveOutcome::Saved
            }
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => self.start_sudo_save(path),
            Err(e) => {
                self.set_status(format!("Error saving: {}", e));
                SaveOutcome::Failed
            }
        }
    }

    fn finish_save(&mut self, path: &str, via: &str) {
        let path_ref = Path::new(path);
        self.document.file_path = Some(path.to_string());
        self.document.is_dirty = false;
        self.document.seal_undo();
        self.file_modification_time = file_io::file_modification_time(path_ref);
        match self.persist_undo_history(path_ref) {
            Ok(()) => self.set_status(format!("Saved {}{}", path, via)),
            Err(e) => self.set_status(format!(
                "Saved {}{} (undo history not saved: {})",
                path, via, e
            )),
        }
    }

    /// Keep the cleaned bytes in memory while the sudo password is asked for,
    /// instead of staging them in a world-readable temporary file.
    fn start_sudo_save(&mut self, path: &str) -> SaveOutcome {
        let contents = self.document.buffer.contents();
        self.sudo_save = Some(SudoSave {
            path: path.to_string(),
            cleaned: file_io::clean_for_write(&contents),
        });
        let prompt = format!("sudo password (to save {}): ", path);
        self.command_buffer
            .open(CommandBufferMode::SudoSave, &prompt, "");
        SaveOutcome::AwaitingInput
    }

    pub(super) fn save_file_sudo(&mut self, password: &str) -> SaveOutcome {
        let Some(SudoSave { path, cleaned }) = self.sudo_save.take() else {
            return SaveOutcome::Failed;
        };
        let path_ref = Path::new(&path);

        // Authenticate on its own, then run the real commands with `-n`. If
        // credentials were already cached, `sudo -S` would not consume the
        // password line, and a command reading stdin would write it into the
        // file.
        if !run_sudo(&["-S", "-v"], format!("{password}\n").as_bytes()) {
            self.set_status("sudo authentication failed".to_string());
            return SaveOutcome::Failed;
        }

        // mkdir -p via sudo if needed
        if let Some(parent) = path_ref.parent()
            && !parent.as_os_str().is_empty()
            && !parent.exists()
        {
            let parent = parent.to_string_lossy();
            if !run_sudo(&["-n", "mkdir", "-p", &parent], &[]) {
                self.set_status("sudo mkdir failed".to_string());
                return SaveOutcome::Failed;
            }
        }

        // `tee` truncates in place, so the file keeps its owner and mode, and
        // taking the content on stdin avoids a staging file other users could read.
        if run_sudo(&["-n", "tee", "--", &path], &cleaned) {
            self.finish_save(&path, " (sudo)");
            SaveOutcome::Saved
        } else {
            self.set_status("sudo save failed".to_string());
            SaveOutcome::Failed
        }
    }

    pub(super) fn cancel_sudo_save(&mut self) {
        self.sudo_save = None;
        self.set_status("sudo save cancelled".to_string());
    }
}

/// Run `sudo <args>` with `input` on stdin; true when it exits successfully.
fn run_sudo(args: &[&str], input: &[u8]) -> bool {
    let spawned = Command::new("sudo")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    let Ok(mut child) = spawned else {
        return false;
    };
    if let Some(mut stdin) = child.stdin.take() {
        // A command that exits early closes the pipe; the exit status below
        // reports that failure.
        let _ = stdin.write_all(input);
    }
    child.wait().is_ok_and(|status| status.success())
}
