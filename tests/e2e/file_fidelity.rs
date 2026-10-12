use super::harness::*;

#[test]
fn invalid_utf8_and_crlf_file_survives_edit_and_save_byte_for_byte() {
    let dir = TempDir::new();
    let path = dir.path().join("legacy.txt");
    std::fs::write(&path, b"caf\xe9\r\nline2\xc2\xa0\r\n").unwrap();
    let mut e = TestEditor::new(&[path.to_str().unwrap()]);

    e.key(Key::Home);
    e.type_text("X");
    e.key(Key::Down);
    e.key(Key::Home);
    e.type_text("Y");
    e.ctrl('s');
    e.wait_until(std::time::Duration::from_secs(2), |_| {
        std::fs::read(&path).is_ok_and(|bytes| bytes.starts_with(b"X"))
    });

    assert_eq!(
        std::fs::read(&path).unwrap(),
        b"Xcaf\xe9\r\nYline2\xc2\xa0\r\n"
    );
}

#[test]
fn unedited_invalid_utf8_file_is_not_rewritten_differently_on_save() {
    let dir = TempDir::new();
    let path = dir.path().join("legacy.txt");
    let original = b"caf\xe9\r\nline2\xc2\xa0\r\n";
    std::fs::write(&path, original).unwrap();
    let mut e = TestEditor::new(&[path.to_str().unwrap()]);

    e.ctrl('s');
    e.wait_until(std::time::Duration::from_secs(2), |e| {
        e.status_bar().contains("Saved") || e.command_line().contains("Saved")
    });

    assert_eq!(std::fs::read(&path).unwrap(), original);
}
