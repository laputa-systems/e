use std::time::Duration;

use super::harness::*;

/// Whitespace-separated words of the shell's `stty -a` output, so a flag such
/// as `icanon` can be told apart from `-icanon`.
fn stty_flags(epilogue: &str) -> Vec<&str> {
    epilogue
        .split(|c: char| c.is_whitespace() || c == ';')
        .collect()
}

fn assert_cooked_terminal(epilogue: &str) {
    let flags = stty_flags(epilogue);
    assert!(
        flags.contains(&"icanon") && flags.contains(&"echo"),
        "terminal should be back in cooked mode, got: {epilogue}"
    );
    assert!(
        !flags.contains(&"-icanon") && !flags.contains(&"-echo"),
        "terminal left raw, got: {epilogue}"
    );
}

const LONG_LINE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaabbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\nsecond\n";

#[test]
fn resizing_while_running_relays_out_the_content() {
    let dir = TempDir::new();
    let path = create_file(dir.path(), "wide.txt", LONG_LINE);
    let mut e = TestEditor::new(&[path.to_str().unwrap()]);

    assert!(e.row(1).contains('b'), "narrow terminal should wrap the line");
    assert!(!e.row(1).contains("second"));

    e.resize(24, 140);
    e.wait_until(Duration::from_secs(5), |e| e.row(1).contains("second"));
    assert!(e.row(0).contains("bbbbbbbbbb"), "got: {}", e.row(0));

    e.resize(10, 80);
    e.wait_until(Duration::from_secs(5), |e| e.status_bar().contains("wide.txt"));
    assert_eq!(e.rows, 10);
}

#[test]
fn quitting_leaves_the_alternate_screen_and_cooked_terminal() {
    let dir = TempDir::new();
    let path = create_file(dir.path(), "plain.txt", "text\n");
    let mut e = TestEditor::in_shell(&[path.to_str().unwrap()]);

    e.ctrl('q');
    let epilogue = e.shell_epilogue();

    assert!(epilogue.contains("editor-exit=0"), "got: {epilogue}");
    e.assert_terminal_restored();
    assert_cooked_terminal(&epilogue);
}

fn assert_signal_restores_terminal(signal: i32) {
    let dir = TempDir::new();
    let path = create_file(dir.path(), "plain.txt", "text\n");
    let arg = path.to_str().unwrap();
    let mut e = TestEditor::in_shell(&[arg]);

    e.signal(signal);
    let epilogue = e.shell_epilogue();

    assert!(
        epilogue.contains("editor-exit=") && !epilogue.contains("editor-exit=0"),
        "signal {signal} should end the editor with a failure status, got: {epilogue}"
    );
    e.assert_terminal_restored();
    assert_cooked_terminal(&epilogue);

    // The lock must not outlive the editor: reopening shows the file, not a
    // take-over prompt.
    let mut again = e.next_session(&[arg]);
    assert!(again.row(0).contains("text"), "got: {}", again.row(0));
}

#[test]
fn sigterm_restores_the_terminal_and_releases_the_lock() {
    assert_signal_restores_terminal(15);
}

#[test]
fn sighup_restores_the_terminal_and_releases_the_lock() {
    assert_signal_restores_terminal(1);
}

#[test]
fn sigint_restores_the_terminal_and_releases_the_lock() {
    assert_signal_restores_terminal(2);
}

#[test]
fn sigquit_restores_the_terminal_and_releases_the_lock() {
    assert_signal_restores_terminal(3);
}
