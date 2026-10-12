use super::harness::*;

#[test]
fn external_change_prompts_and_yes_reloads_the_file() {
    let dir = TempDir::new();
    let path = create_file(dir.path(), "shared.txt", "one\ntwo\n");
    let mut e = TestEditor::new(&[path.to_str().unwrap()]);

    modify_externally(&path, b"changed outside\n");
    e.focus_in();
    assert!(
        e.screen_text().contains("changed on disk. Reload? (y/n)"),
        "got: {}",
        e.screen_text()
    );

    e.send_raw(b"y");
    e.wait();
    assert!(e.row(0).contains("changed outside"), "got: {}", e.row(0));
}

#[test]
fn declining_the_reload_keeps_the_buffer() {
    let dir = TempDir::new();
    let path = create_file(dir.path(), "shared.txt", "one\ntwo\n");
    let mut e = TestEditor::new(&[path.to_str().unwrap()]);

    modify_externally(&path, b"changed outside\n");
    e.focus_in();
    e.send_raw(b"n");
    e.wait();

    assert!(e.row(0).contains("one"), "got: {}", e.row(0));
    assert!(!e.screen_text().contains("changed outside"));
}

#[test]
fn saving_after_an_external_change_is_refused() {
    let dir = TempDir::new();
    let path = create_file(dir.path(), "shared.txt", "one\n");
    let mut e = TestEditor::new(&[path.to_str().unwrap()]);

    modify_externally(&path, b"changed outside\n");
    e.type_text("X");
    e.ctrl('s');

    assert!(
        e.screen_text().contains("changed on disk"),
        "got: {}",
        e.screen_text()
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"changed outside\n");
}
