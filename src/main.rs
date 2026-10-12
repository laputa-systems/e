use std::fs;
use std::io::{self, BufRead, IsTerminal, Read, Write};
use std::path::Path;
use std::process;

use e::file_io::{self, LockError};
use e::editor;

/// Piped stdin is read in full before the editor starts, so it is capped to
/// keep a runaway producer from exhausting memory.
const MAX_STDIN_BYTES: u64 = 256 * 1024 * 1024;

/// Ask a y/n question on the alternate screen. Answers come from the
/// terminal even when stdin is a pipe carrying the document; with no
/// terminal to ask, the caller gets an error instead of a silent "no".
fn confirm(prompt: &str) -> Result<bool, String> {
    let mut answers: Box<dyn BufRead> = if io::stdin().is_terminal() {
        Box::new(io::stdin().lock())
    } else {
        let tty = fs::File::open("/dev/tty")
            .map_err(|e| format!("cannot ask \"{prompt}\": no terminal available ({e})"))?;
        Box::new(io::BufReader::new(tty))
    };
    // Use alternate screen so the prompt doesn't pollute terminal history
    eprint!("\x1b[?1049h\x1b[2J\x1b[H{} (y/n) ", prompt);
    let _ = io::stderr().flush();
    let mut buf = String::new();
    let read = answers.read_line(&mut buf);
    eprint!("\x1b[?1049l");
    let _ = io::stderr().flush();
    read.map_err(|e| format!("cannot read answer: {e}"))?;
    Ok(buf.trim().eq_ignore_ascii_case("y"))
}

type LoadResult = Result<Option<(Vec<u8>, Option<String>)>, String>;

/// Load file data from command-line args. Returns Ok(Some((text, filename))) on success,
/// Ok(None) if user declined a confirmation prompt, or Err on failure.
fn load(
    args: &[String],
    piped_stdin: bool,
    stdin_data: Option<Vec<u8>>,
    confirm_fn: impl Fn(&str) -> Result<bool, String>,
) -> LoadResult {
    if args.len() > 2 {
        return Err("Usage: e [file]".to_string());
    }

    if args.len() > 1 {
        let path = Path::new(&args[1]);
        match fs::metadata(path) {
            Ok(meta) => {
                // Reading a FIFO or device (`/dev/zero`) would block or never end.
                if !meta.is_file() {
                    return Err(format!("{}: not a regular file", args[1]));
                }
                // File safety checks
                let size = meta.len();
                if size > 5_000_000
                    && !confirm_fn(&format!(
                        "e: {} is {}MB. Open anyway?",
                        args[1],
                        size / 1_000_000
                    ))?
                {
                    return Ok(None);
                }

                let data = file_io::read_file(path).map_err(|e| format!("{}: {}", args[1], e))?;

                if file_io::is_likely_binary(&data)
                    && !confirm_fn(&format!(
                        "e: {} appears to be binary. Open anyway?",
                        args[1]
                    ))?
                {
                    return Ok(None);
                }

                Ok(Some((data, Some(args[1].clone()))))
            }
            // New file — empty buffer with name set
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                Ok(Some((Vec::new(), Some(args[1].clone()))))
            }
            Err(e) => Err(format!("{}: {}", args[1], e)),
        }
    } else if piped_stdin {
        Ok(Some((stdin_data.unwrap_or_default(), None)))
    } else {
        Ok(Some((Vec::new(), None)))
    }
}

fn read_stdin() -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    io::stdin()
        .lock()
        .take(MAX_STDIN_BYTES + 1)
        .read_to_end(&mut buf)
        .map_err(|e| format!("failed to read stdin: {e}"))?;
    if buf.len() as u64 > MAX_STDIN_BYTES {
        return Err(format!(
            "stdin is larger than {}MB",
            MAX_STDIN_BYTES / 1_000_000
        ));
    }
    Ok(buf)
}

/// Claim `name` for this process. `Ok(None)` means the user declined to
/// override another instance's lock.
fn lock_file(name: &str) -> Result<Option<file_io::FileLock>, String> {
    let path = Path::new(name);
    match file_io::acquire_lock(path) {
        Ok(lock) => Ok(Some(lock)),
        Err(LockError::Held { holder_pid }) => {
            let holder = holder_pid.map_or(String::new(), |pid| format!(" (pid {pid})"));
            if !confirm(&format!(
                "e: {name} is already open in another instance{holder}. Take over the lock and open anyway?"
            ))? {
                return Ok(None);
            }
            file_io::break_lock(path).map_err(|e| format!("cannot remove lock: {e}"))?;
            file_io::acquire_lock(path)
                .map(Some)
                .map_err(|e| format!("{name}: {e}"))
        }
        Err(e @ LockError::Io(_)) => Err(format!("{name}: {e}")),
    }
}

fn main() {
    // `process::exit` skips destructors, so everything that must unwind (the
    // lock file) lives inside `run`.
    process::exit(run());
}

fn run() -> i32 {
    let args: Vec<String> = std::env::args().collect();

    if args.len() == 2 && (args[1] == "-V" || args[1] == "--version") {
        println!("e {}", env!("CARGO_PKG_VERSION"));
        return 0;
    }

    let piped_stdin = !io::stdin().is_terminal();

    // Lock before reading so a file another instance is editing is never
    // loaded from under it.
    let _lock = if args.len() == 2 {
        match lock_file(&args[1]) {
            Ok(Some(lock)) => Some(lock),
            Ok(None) => return 0,
            Err(e) => {
                eprintln!("e: {}", e);
                return 1;
            }
        }
    } else {
        None
    };

    // With a file argument stdin is not the document, so leave it unread; the
    // editor takes keys from the terminal instead.
    let stdin_data = if piped_stdin && args.len() < 2 {
        match read_stdin() {
            Ok(data) => Some(data),
            Err(e) => {
                eprintln!("e: {}", e);
                return 1;
            }
        }
    } else {
        None
    };

    let (text, filename) = match load(&args, piped_stdin, stdin_data, confirm) {
        Ok(Some(t)) => t,
        Ok(None) => return 0,
        Err(e) => {
            eprintln!("e: {}", e);
            return 1;
        }
    };

    let mut ed = editor::Editor::new(text, filename, piped_stdin);
    if let Err(e) = ed.run() {
        eprintln!("e: {}", e);
        return 1;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_load_too_many_args() {
        let result = load(&args(&["e", "a", "b"]), false, None, |_| Ok(true));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Usage"));
    }

    #[test]
    fn test_load_no_args_no_pipe() {
        let result = load(&args(&["e"]), false, None, |_| Ok(true));
        let (text, filename) = result.unwrap().unwrap();
        assert!(text.is_empty());
        assert!(filename.is_none());
    }

    #[test]
    fn test_load_binary_declined() {
        let dir = std::env::temp_dir().join("e_test_main_binary");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("data.bin");
        // Write enough null bytes to trigger binary detection
        let mut data = vec![0u8; 512];
        data[0] = 0x7f; // ELF-like header
        std::fs::write(&path, &data).unwrap();

        let result = load(
            &args(&["e", path.to_str().unwrap()]),
            false,
            None,
            |_| Ok(false), // decline
        );
        // User declined — should return Ok(None)
        assert!(result.unwrap().is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_load_binary_accepted() {
        let dir = std::env::temp_dir().join("e_test_main_binary_ok");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("data.bin");
        let mut data = vec![0u8; 512];
        data[0] = 0x7f;
        std::fs::write(&path, &data).unwrap();

        let result = load(
            &args(&["e", path.to_str().unwrap()]),
            false,
            None,
            |_| Ok(true), // accept
        );
        let (text, filename) = result.unwrap().unwrap();
        assert_eq!(text, data);
        assert!(filename.is_some());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_load_reports_failed_confirmation() {
        let dir = std::env::temp_dir().join("e_test_main_confirm_error");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("data.bin");
        std::fs::write(&path, vec![0u8; 512]).unwrap();

        let result = load(&args(&["e", path.to_str().unwrap()]), false, None, |_| {
            Err("no terminal".to_string())
        });
        assert_eq!(result.unwrap_err(), "no terminal");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
