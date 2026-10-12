use std::process::Command;
use std::time::Duration;

use super::harness::*;

/// Error messages wrap at the terminal width, even inside a word, so compare
/// with all whitespace removed.
fn unwrapped(text: &str) -> String {
    text.split_whitespace().collect()
}

#[test]
fn piped_stdin_is_shown_and_editable_from_the_terminal() {
    let mut e = TestEditor::piped(b"hello from pipe\nsecond line\n", &[]);

    let first = e.row(0);
    assert!(first.contains("hello from pipe"), "got: {first}");
    assert!(e.row(1).contains("second line"));

    e.key(Key::Home);
    e.type_text("X");
    let edited = e.row(0);
    assert!(edited.contains("Xhello from pipe"), "got: {edited}");
}

#[test]
fn piped_stdin_can_be_saved_under_a_new_name() {
    let dir = TempDir::new();
    let target = dir.path().join("from-pipe.txt");
    let mut e = TestEditor::piped(b"piped text\n", &[]);

    e.ctrl('s');
    e.type_text(target.to_str().unwrap());
    e.enter();
    e.wait_until(Duration::from_secs(2), |_| target.exists());

    assert_eq!(std::fs::read_to_string(&target).unwrap(), "piped text\n");
}

#[test]
fn opening_a_device_exits_with_an_error_message() {
    let mut e = TestEditor::before_ui(&["/dev/zero"]);

    assert_eq!(e.wait_for_exit(), ExitStatus::Code(1));
    assert!(
        unwrapped(&e.screen_text()).contains("notaregularfile"),
        "got: {}",
        e.screen_text()
    );
}

#[test]
fn opening_a_fifo_exits_with_an_error_message() {
    let dir = TempDir::new();
    let fifo = dir.path().join("pipe");
    let made = Command::new("/usr/bin/mkfifo").arg(&fifo).status().unwrap();
    assert!(made.success());

    let mut e = TestEditor::before_ui(&[fifo.to_str().unwrap()]);

    assert_eq!(e.wait_for_exit(), ExitStatus::Code(1));
    assert!(
        unwrapped(&e.screen_text()).contains("notaregularfile"),
        "got: {}",
        e.screen_text()
    );
}
