use std::fmt::Write as FmtWrite;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

pub use ptytest::ExitStatus;
use ptytest::{
    Color, CommandSpec, Deadline, ProtocolProfile, PtyTest, Scenario, Size,
    TerminalBaseline, TestEnv,
};

// ---------------------------------------------------------------------------
// Key — editor-specific names for exact input sequences
// ---------------------------------------------------------------------------

pub enum Key {
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Delete,
    ShiftUp,
    ShiftDown,
    ShiftLeft,
    ShiftRight,
    CtrlLeft,
    CtrlRight,
    CtrlShiftUp,
    CtrlShiftDown,
    CtrlShiftLeft,
    CtrlShiftRight,
    ShiftTab,
}

impl Key {
    fn as_bytes(&self) -> &'static [u8] {
        match self {
            Self::Up => b"\x1b[A",
            Self::Down => b"\x1b[B",
            Self::Right => b"\x1b[C",
            Self::Left => b"\x1b[D",
            Self::Home => b"\x1b[H",
            Self::End => b"\x1b[F",
            Self::PageUp => b"\x1b[5~",
            Self::PageDown => b"\x1b[6~",
            Self::Delete => b"\x1b[3~",
            Self::ShiftUp => b"\x1b[1;2A",
            Self::ShiftDown => b"\x1b[1;2B",
            Self::ShiftRight => b"\x1b[1;2C",
            Self::ShiftLeft => b"\x1b[1;2D",
            Self::CtrlLeft => b"\x1b[1;5D",
            Self::CtrlRight => b"\x1b[1;5C",
            Self::CtrlShiftUp => b"\x1b[1;6A",
            Self::CtrlShiftDown => b"\x1b[1;6B",
            Self::CtrlShiftRight => b"\x1b[1;6C",
            Self::CtrlShiftLeft => b"\x1b[1;6D",
            Self::ShiftTab => b"\x1b[Z",
        }
    }
}

// ---------------------------------------------------------------------------
// TempDir — editor fixtures remain application-specific
// ---------------------------------------------------------------------------

pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new() -> Self {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        loop {
            let id = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!("e_test_{}_{}", std::process::id(), id));
            match std::fs::create_dir(&path) {
                Ok(()) => return TempDir(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("could not create temporary e2e directory: {error}"),
            }
        }
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// ---------------------------------------------------------------------------
// Recording — an opt-in e-owned asciicast policy
// ---------------------------------------------------------------------------

struct Recording {
    start: std::time::Instant,
    events: Vec<(f64, char, Vec<u8>)>,
    rows: u16,
    cols: u16,
}

impl Recording {
    fn new(rows: u16, cols: u16) -> Self {
        Self {
            start: std::time::Instant::now(),
            events: Vec::new(),
            rows,
            cols,
        }
    }

    fn push(&mut self, kind: char, data: &[u8]) {
        self.events
            .push((self.start.elapsed().as_secs_f64(), kind, data.to_vec()));
    }

    fn save(&self, path: &Path) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        let mut output = std::fs::File::create(path).unwrap();
        use std::io::Write;
        writeln!(
            output,
            r#"{{"version": 2, "width": {}, "height": {}}}"#,
            self.cols, self.rows
        )
        .unwrap();
        let mut groups: Vec<(f64, Vec<u8>)> = Vec::new();
        for (time, kind, data) in &self.events {
            if *kind != 'o' {
                continue;
            }
            if let Some(last) = groups.last_mut()
                && time - last.0 < 0.05
            {
                last.1.extend_from_slice(data);
            } else {
                groups.push((*time, data.clone()));
            }
        }
        let mut adjusted = 0.0;
        for (index, (_, data)) in groups.iter().enumerate() {
            if index > 0 {
                adjusted += (groups[index].0 - groups[index - 1].0).max(0.4);
            }
            writeln!(
                output,
                r#"[{adjusted:.3}, "o", "{}"]"#,
                json_escape_bytes(data)
            )
            .unwrap();
        }
    }
}

fn json_escape_bytes(data: &[u8]) -> String {
    let mut output = String::new();
    for character in String::from_utf8_lossy(data).chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            character if (character as u32) < 0x20 || character == '\x7f' => {
                write!(output, "\\u{:04x}", character as u32).unwrap()
            }
            character => output.push(character),
        }
    }
    output
}

fn recording_path() -> Option<PathBuf> {
    std::env::var("E2E_RECORD").ok()?;
    let name = std::thread::current()
        .name()
        .unwrap_or("unknown")
        .replace("::", "__");
    Some(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/e2e/recordings")
            .join(format!("{name}.cast")),
    )
}

// ---------------------------------------------------------------------------
// TestEditor — thin e-specific adapter over ptytest
// ---------------------------------------------------------------------------

/// Upper bound for one editor reaction. Output normally arrives within
/// milliseconds; the bound only matters on a loaded host, where a short one
/// made `wait` give up before the editor had drawn.
const OUTPUT_WAIT: Duration = Duration::from_secs(2);

pub struct TestEditor {
    terminal: PtyTest,
    terminal_baseline: TerminalBaseline,
    restoration_checked: bool,
    /// Shared so a later session can reuse the same isolated HOME.
    pub home: Arc<TempDir>,
    pub rows: u16,
    pub cols: u16,
    recording: Option<Recording>,
    recorded_output: usize,
}

impl TestEditor {
    pub fn new(args: &[&str]) -> Self {
        Self::with_size(args, 24, 80)
    }

    pub fn with_size(args: &[&str], rows: u16, cols: u16) -> Self {
        let home = Arc::new(TempDir::new());
        let command = CommandSpec::new(selected_binary()).args(args.iter().copied());
        Self::spawn(home, command, rows, cols)
    }

    /// Start another session that sees the state files an earlier one left
    /// in the same isolated HOME. The earlier session must already be done.
    pub fn next_session(&self, args: &[&str]) -> Self {
        let command = CommandSpec::new(selected_binary()).args(args.iter().copied());
        Self::spawn(Arc::clone(&self.home), command, self.rows, self.cols)
    }

    /// Start a session that may stop before the editor screen appears (a
    /// startup prompt, or an error exit), so no editor UI is awaited.
    pub fn next_session_before_ui(&self, args: &[&str]) -> Self {
        let command = CommandSpec::new(selected_binary()).args(args.iter().copied());
        Self::spawn_inner(Arc::clone(&self.home), command, self.rows, self.cols, false)
    }

    /// Like `next_session_before_ui`, in a fresh HOME.
    pub fn before_ui(args: &[&str]) -> Self {
        let command = CommandSpec::new(selected_binary()).args(args.iter().copied());
        Self::spawn_inner(Arc::new(TempDir::new()), command, 24, 80, false)
    }

    /// Start the editor with `input` piped to its stdin, as in
    /// `printf ... | e`. The terminal stays on the PTY, so keys still arrive.
    pub fn piped(input: &[u8], args: &[&str]) -> Self {
        let home = Arc::new(TempDir::new());
        let input_path = home.path().join("stdin.bin");
        std::fs::write(&input_path, input).unwrap();
        let command = CommandSpec::new("/bin/sh")
            .arg("-c")
            .arg(r#"input=$1; editor=$2; shift 2; /bin/cat "$input" | "$editor" "$@""#)
            .arg("sh")
            .arg(&input_path)
            .arg(selected_binary())
            .args(args.iter().copied());
        Self::spawn(home, command, 24, 80)
    }

    /// Start the editor from a shell that ignores SIGTERM, SIGHUP, SIGINT and
    /// SIGQUIT, so the editor can be signalled without taking the shell down.
    /// Ignored dispositions are inherited, so the editor must install its own
    /// handlers to observe these signals. After the editor ends the shell
    /// prints its exit status and the terminal's line discipline flags
    /// (`stty -a`), which `shell_epilogue` reads back.
    pub fn in_shell(args: &[&str]) -> Self {
        let home = Arc::new(TempDir::new());
        let command = CommandSpec::new("/bin/sh")
            .arg("-c")
            .arg(
                r#"trap "" TERM HUP INT QUIT; editor=$1; shift; "$editor" "$@"; echo "editor-exit=$?"; /bin/stty -a; echo shell-done"#,
            )
            .arg("sh")
            .arg(selected_binary())
            .args(args.iter().copied());
        Self::spawn(home, command, 24, 80)
    }

    fn spawn(home: Arc<TempDir>, command: CommandSpec, rows: u16, cols: u16) -> Self {
        Self::spawn_inner(home, command, rows, cols, true)
    }

    fn spawn_inner(
        home: Arc<TempDir>,
        command: CommandSpec,
        rows: u16,
        cols: u16,
        wait_for_editor_ui: bool,
    ) -> Self {
        let command = command
            .current_dir(home.path())
            // A HOME-only PATH forces the editor's internal clipboard, so
            // parallel E2E tests never race through the system clipboard.
            .env("PATH", home.path())
            .remove_env("WAYLAND_DISPLAY")
            .remove_env("DISPLAY");
        let environment = if cfg!(target_os = "linux") {
            TestEnv::hermetic_ascii()
        } else {
            TestEnv::hermetic_utf8("C.UTF-8")
        }
        .expect("a supported hermetic locale must be available on E2E platforms")
            .env("HOME", home.path())
            .env("PATH", home.path());
        let scenario = Scenario::new("e editor E2E")
            .expect("valid scenario label")
            .command(command)
            .size(Size::new(cols, rows).expect("non-zero editor size"))
            .environment(environment)
            .protocol_profile(ProtocolProfile::xterm_minimal_v1());
        let terminal = PtyTest::spawn(scenario).expect("spawn editor through ptytest");
        let terminal_baseline = terminal.terminal_baseline();
        let mut editor = Self {
            terminal,
            terminal_baseline,
            restoration_checked: false,
            home,
            rows,
            cols,
            recording: recording_path().map(|_| Recording::new(rows, cols)),
            recorded_output: 0,
        };
        if wait_for_editor_ui {
            editor.wait_for_startup();
        }
        editor
    }

    fn deadline(&self) -> Deadline {
        self.terminal.deadline(Duration::from_secs(5))
    }

    fn capture_output(&mut self) {
        let new_output = self
            .terminal
            .raw_output()
            .get(self.recorded_output..)
            .unwrap_or_default()
            .to_vec();
        self.recorded_output += new_output.len();
        if let Some(recording) = &mut self.recording
            && !new_output.is_empty()
        {
            recording.push('o', &new_output);
        }
    }

    fn wait_for_startup(&mut self) {
        let status_row = self.rows.saturating_sub(2) as usize;
        let deadline = self.terminal.deadline(Duration::from_secs(5));
        self.terminal
            .wait_for_screen(deadline, "editor status row", move |screen| {
                screen
                    .row(status_row)
                    .is_some_and(|row| !row.trim().is_empty())
            })
            .expect("editor did not become ready during startup");
        self.capture_output();
    }

    /// Wait for one editor output event. The public tests assert the resulting
    /// semantic row/cell state; a legitimate no-op input is allowed to remain
    /// silent instead of being turned into a timing failure.
    pub fn wait(&mut self) {
        let deadline = self.terminal.deadline(OUTPUT_WAIT);
        // A timeout only means the input was a no-op: the output arrives
        // within milliseconds when there is any, so a generous deadline costs
        // time only for inputs that legitimately stay silent.
        let _ = self.terminal.wait_for_output(deadline);
        self.capture_output();
    }

    pub fn wait_until<F>(&mut self, timeout: Duration, mut ready: F)
    where
        F: FnMut(&mut Self) -> bool,
    {
        let deadline = self.terminal.deadline(timeout);
        loop {
            self.terminal.drain(deadline).expect("drain editor output");
            self.capture_output();
            if ready(self) {
                return;
            }
            let saw_output = self
                .terminal
                .wait_for_output(deadline)
                .expect("wait for editor output");
            self.capture_output();
            if ready(self) {
                return;
            }
            if !saw_output {
                // `wait_for_output` returns false only at the caller's
                // deadline. The final semantic check above distinguishes a
                // quiet valid exit from a real state-transition failure.
                panic!("editor did not reach the expected state within {timeout:?}");
            }
        }
    }

    pub fn send_raw(&mut self, bytes: &[u8]) {
        if let Some(recording) = &mut self.recording {
            recording.push('i', bytes);
        }
        let deadline = self.deadline();
        self.terminal
            .send_bytes(deadline, bytes)
            .expect("PTY write failed");
    }

    pub fn type_text(&mut self, text: &str) {
        self.send_raw(text.as_bytes());
        self.wait();
    }
    pub fn ctrl(&mut self, character: char) {
        self.send_raw(&[(character as u8) & 0x1f]);
        self.wait();
    }
    pub fn key(&mut self, key: Key) {
        self.send_raw(key.as_bytes());
        self.wait();
    }
    pub fn enter(&mut self) {
        self.send_raw(b"\r");
        self.wait();
    }
    pub fn backspace(&mut self) {
        self.send_raw(b"\x7f");
        self.wait();
    }
    pub fn tab(&mut self) {
        self.send_raw(b"\t");
        self.wait();
    }
    pub fn escape(&mut self) {
        self.send_raw(b"\x1b");
        self.wait();
    }

    pub fn paste(&mut self, text: &str) {
        self.send_raw(b"\x1b[200~");
        self.send_raw(text.as_bytes());
        self.send_raw(b"\x1b[201~");
        self.wait();
    }

    // --- mouse events (SGR mode, 1-indexed) ---------------------------------

    pub fn click(&mut self, row: u16, column: u16) {
        let (row, column) = (row + 1, column + 1);
        self.send_raw(format!("\x1b[<0;{column};{row}M\x1b[<0;{column};{row}m").as_bytes());
        self.wait();
    }
    pub fn double_click(&mut self, row: u16, column: u16) {
        self.click(row, column);
        self.click(row, column);
    }
    pub fn triple_click(&mut self, row: u16, column: u16) {
        self.click(row, column);
        self.click(row, column);
        self.click(row, column);
    }
    pub fn drag(&mut self, from: (u16, u16), to: (u16, u16)) {
        let (from_row, from_column) = (from.0 + 1, from.1 + 1);
        let (to_row, to_column) = (to.0 + 1, to.1 + 1);
        self.send_raw(format!("\x1b[<0;{from_column};{from_row}M\x1b[<32;{to_column};{to_row}M\x1b[<0;{to_column};{to_row}m").as_bytes());
        self.wait();
    }
    pub fn scroll_up(&mut self) {
        self.send_raw(b"\x1b[<64;1;1M");
        self.wait();
    }
    pub fn scroll_down(&mut self) {
        self.send_raw(b"\x1b[<65;1;1M");
        self.wait();
    }
    pub fn resize(&mut self, rows: u16, cols: u16) {
        self.terminal
            .resize(Size::new(cols, rows).expect("non-zero editor size"))
            .expect("resize PTY");
        self.rows = rows;
        self.cols = cols;
        self.wait();
    }

    /// Signal the editor's whole process group.
    pub fn signal(&mut self, signal: i32) {
        self.terminal.signal(signal).expect("signal editor");
    }

    /// Fail unless the terminal modes (alternate screen, mouse, paste, cursor)
    /// are back to what they were before the editor started.
    pub fn assert_terminal_restored(&mut self) {
        self.terminal
            .assert_terminal_restored(&self.terminal_baseline)
            .expect("editor restored the terminal modes");
    }

    /// Wait for the shell started by `in_shell` to finish and return what it
    /// printed after the editor ended.
    pub fn shell_epilogue(&mut self) -> String {
        self.wait_until(Duration::from_secs(5), |e| {
            e.screen_text().contains("shell-done")
        });
        self.screen_text()
    }

    pub fn focus_in(&mut self) {
        self.send_raw(b"\x1b[I");
        self.wait();
    }

    pub fn row(&mut self, row: u16) -> String {
        self.terminal
            .drain(self.deadline())
            .expect("drain editor output");
        self.capture_output();
        self.terminal
            .screen()
            .row(row as usize)
            .unwrap_or_default()
            .trim_end()
            .to_owned()
    }

    pub fn screen_text(&mut self) -> String {
        (0..self.rows)
            .map(|row| self.row(row))
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn cursor(&mut self) -> (u16, u16) {
        self.terminal
            .drain(self.deadline())
            .expect("drain editor output");
        self.capture_output();
        let cursor = self.terminal.screen().cursor();
        (cursor.row, cursor.column)
    }

    /// The editor draws its software cursor as bold inverse text.
    pub fn cursor_visible(&mut self) -> bool {
        let (row, column) = self.cursor();
        self.terminal
            .screen()
            .cell(row as usize, column as usize)
            .is_some_and(|cell| cell.attributes().bold && cell.attributes().inverse)
    }

    pub fn status_bar(&mut self) -> String {
        self.row(self.rows - 2)
    }
    pub fn command_line(&mut self) -> String {
        self.row(self.rows - 1)
    }
    pub fn cell_fg(&mut self, row: u16, column: u16) -> Color {
        self.terminal
            .drain(self.deadline())
            .expect("drain editor output");
        self.capture_output();
        self.terminal
            .screen()
            .cell(row as usize, column as usize)
            .map_or(Color::Default, |cell| cell.attributes().foreground.clone())
    }
    pub fn cell_bg(&mut self, row: u16, column: u16) -> Color {
        self.terminal
            .drain(self.deadline())
            .expect("drain editor output");
        self.capture_output();
        self.terminal
            .screen()
            .cell(row as usize, column as usize)
            .map_or(Color::Default, |cell| cell.attributes().background.clone())
    }
    pub fn cell_inverse(&mut self, row: u16, column: u16) -> bool {
        self.terminal
            .drain(self.deadline())
            .expect("drain editor output");
        self.capture_output();
        self.terminal
            .screen()
            .cell(row as usize, column as usize)
            .is_some_and(|cell| cell.attributes().inverse)
    }

    pub fn quit_no_save(&mut self) {
        self.ctrl('q');
        self.send_raw(b"n");
        self.wait();
    }
    pub fn quit_saving(&mut self) {
        self.ctrl('q');
        self.send_raw(b"y");
        self.wait();
    }
    pub fn wait_for_exit(&mut self) -> ExitStatus {
        let deadline = self.deadline();
        let status = self
            .terminal
            .wait_for_exit(deadline)
            .expect("wait for editor exit");
        self.assert_normal_exit_restoration(status);
        status
    }
    pub fn has_exited(&mut self) -> bool {
        let status = self.terminal.observe_exit().expect("observe editor exit");
        if let Some(status) = status {
            self.assert_normal_exit_restoration(status);
            true
        } else {
            false
        }
    }

    fn assert_normal_exit_restoration(&mut self, status: ExitStatus) {
        if status == ExitStatus::Code(0) && !self.restoration_checked {
            self.terminal
                .assert_terminal_restored(&self.terminal_baseline)
                .expect("normal editor exit restores applicable terminal modes");
            self.restoration_checked = true;
        }
    }
}

impl Drop for TestEditor {
    fn drop(&mut self) {
        self.terminal.drain(self.deadline()).ok();
        self.capture_output();
        if let Some(recording) = self.recording.take()
            && let Some(path) = recording_path()
        {
            recording.save(&path);
        }
        let deadline = self.terminal.deadline(Duration::from_secs(1));
        let _ = self.terminal.finish(deadline);
    }
}

fn selected_binary() -> PathBuf {
    if let Some(path) = std::env::var_os("E_TEST_BINARY") {
        let path = PathBuf::from(path);
        assert!(
            path.is_file(),
            "selected editor binary is not a file: {}",
            path.display()
        );
        return path;
    }
    PathBuf::from(env!("CARGO_BIN_EXE_e"))
}

/// Replace a file's content behind the editor's back and give it a fixed,
/// distinct modification time, so the change is detectable whatever the
/// filesystem's timestamp granularity.
pub fn modify_externally(path: &Path, content: &[u8]) {
    std::fs::write(path, content).unwrap();
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(std::time::UNIX_EPOCH + Duration::from_secs(1_000_000_000))
        .unwrap();
}

/// Create a file in an application fixture directory.
pub fn create_file(dir: &Path, name: &str, content: &str) -> PathBuf {
    let path = dir.join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&path, content).unwrap();
    path
}
