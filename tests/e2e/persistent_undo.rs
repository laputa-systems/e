use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

use super::harness::*;

fn mode(path: &std::path::Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[test]
fn undo_history_survives_save_quit_and_reopen() {
    let dir = TempDir::new();
    let path = create_file(dir.path(), "notes.txt", "hello\n");
    let arg = path.to_str().unwrap();

    let mut first = TestEditor::new(&[arg]);
    first.key(Key::End);
    first.type_text(" world");
    first.ctrl('s');
    first.wait_until(Duration::from_secs(2), |_| {
        std::fs::read_to_string(&path).is_ok_and(|text| text == "hello world\n")
    });
    first.ctrl('q');
    assert_eq!(first.wait_for_exit(), ExitStatus::Code(0));

    let mut second = first.next_session(&[arg]);
    assert!(second.row(0).contains("hello world"));
    second.ctrl('z');
    let row = second.row(0);
    assert!(
        row.contains("hello") && !row.contains("world"),
        "undo in a new session should remove the saved edit, got: {row}"
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello world\n");
}

#[test]
fn undo_after_reopening_a_file_saved_with_trailing_spaces_stays_safe() {
    let dir = TempDir::new();
    let path = create_file(dir.path(), "spaces.txt", "abc   \nxyz");
    let arg = path.to_str().unwrap();

    let mut first = TestEditor::new(&[arg]);
    first.key(Key::Down);
    first.key(Key::End);
    first.type_text("Q");
    first.ctrl('s');
    first.wait_until(Duration::from_secs(2), |_| {
        std::fs::read_to_string(&path).is_ok_and(|text| text == "abc\nxyzQ\n")
    });
    first.ctrl('q');
    assert_eq!(first.wait_for_exit(), ExitStatus::Code(0));

    let mut second = first.next_session(&[arg]);
    second.ctrl('z');
    second.ctrl('z');
    assert!(!second.has_exited(), "undo must not crash the editor");
    assert!(second.row(0).contains("abc"), "got: {}", second.row(0));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "abc\nxyzQ\n");
    second.ctrl('q');
    // A restored history would make the undo dirty the buffer and prompt;
    // an untrusted one leaves it clean, so the editor exits at once.
    second.wait_until(Duration::from_secs(2), |e| e.has_exited());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "abc\nxyzQ\n");
}

#[test]
fn discarded_edits_leave_no_stale_history_for_the_next_session() {
    let dir = TempDir::new();
    let path = create_file(dir.path(), "discard.txt", "keep\n");
    let arg = path.to_str().unwrap();

    let mut first = TestEditor::new(&[arg]);
    first.key(Key::End);
    first.type_text(" scratch");
    first.quit_no_save();
    assert_eq!(first.wait_for_exit(), ExitStatus::Code(0));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "keep\n");

    let mut second = first.next_session(&[arg]);
    second.ctrl('z');
    second.ctrl('y');
    assert!(!second.has_exited());
    let row = second.row(0);
    assert!(
        row.contains("keep") && !row.contains("scratch"),
        "no history should be applied, got: {row}"
    );
}

#[test]
fn state_files_are_owner_only_after_a_save_session() {
    let dir = TempDir::new();
    let path = create_file(dir.path(), "private.txt", "secret\n");
    let mut e = TestEditor::new(&[path.to_str().unwrap()]);
    e.key(Key::End);
    e.type_text("!");
    e.ctrl('s');
    e.wait_until(Duration::from_secs(2), |_| {
        std::fs::read_to_string(&path).is_ok_and(|text| text == "secret!\n")
    });
    e.ctrl('q');
    assert_eq!(e.wait_for_exit(), ExitStatus::Code(0));

    let state = e.home.path().join(".config/e");
    assert_eq!(mode(&state), 0o700, "state directory");
    assert_eq!(mode(&state.join("undo.bin")), 0o600, "undo.bin");
    assert_eq!(mode(&state.join("locks")), 0o700, "locks directory");
}
