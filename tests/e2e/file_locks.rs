use super::harness::*;

#[test]
fn second_instance_declining_the_takeover_exits_cleanly() {
    let dir = TempDir::new();
    let path = create_file(dir.path(), "locked.txt", "content\n");
    let arg = path.to_str().unwrap();
    let mut first = TestEditor::new(&[arg]);

    let mut second = first.next_session_before_ui(&[arg]);
    second.wait_until(std::time::Duration::from_secs(5), |e| {
        e.screen_text().contains("already open")
    });
    second.send_raw(b"n\r");

    assert_eq!(second.wait_for_exit(), ExitStatus::Code(0));
    assert!(!first.has_exited());
    assert_eq!(std::fs::read(&path).unwrap(), b"content\n");
}

#[test]
fn second_instance_accepting_the_takeover_opens_the_file() {
    let dir = TempDir::new();
    let path = create_file(dir.path(), "locked.txt", "content\n");
    let arg = path.to_str().unwrap();
    let first = TestEditor::new(&[arg]);

    let mut second = first.next_session_before_ui(&[arg]);
    second.wait_until(std::time::Duration::from_secs(5), |e| {
        e.screen_text().contains("already open")
    });
    second.send_raw(b"y\r");
    second.wait_until(std::time::Duration::from_secs(5), |e| {
        e.row(0).contains("content")
    });
}

#[test]
fn lock_left_by_a_killed_instance_does_not_prompt() {
    let dir = TempDir::new();
    let path = create_file(dir.path(), "locked.txt", "content\n");
    let arg = path.to_str().unwrap();
    let mut first = TestEditor::new(&[arg]);

    first.signal(9);
    assert_eq!(first.wait_for_exit(), ExitStatus::Signal(9));

    let mut second = first.next_session(&[arg]);
    assert!(second.row(0).contains("content"), "got: {}", second.row(0));
}
