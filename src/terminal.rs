use std::io;
use std::os::fd::BorrowedFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Once, OnceLock};

use rustix::termios::Termios;

use crate::input;

/// Alternate screen, mouse reporting (button, drag, SGR), bracketed paste,
/// focus events, and a hidden cursor.
const ENTER: &[u8] = b"\x1b[?1049h\x1b[?1000h\x1b[?1002h\x1b[?1006h\x1b[?2004h\x1b[?1004h\x1b[?25l";
const LEAVE: &[u8] = b"\x1b[?1004l\x1b[?2004l\x1b[?1006l\x1b[?1002l\x1b[?1000l\x1b[?25h\x1b[?1049l";

/// Terminal settings to put back. Shared with the panic hook and the crash
/// signal handler, which cannot reach the guard: release builds abort on panic
/// without unwinding. A `OnceLock` read is lock-free, which the signal handler
/// needs; the settings are those of the shell, so the first capture stays valid.
static SAVED_TERMIOS: OnceLock<Termios> = OnceLock::new();
static ACTIVE: AtomicBool = AtomicBool::new(false);
static INSTALL_PANIC_HOOK: Once = Once::new();

/// Puts the terminal in raw, alternate-screen editing mode for its lifetime
/// and restores it on drop, so an early `?` return or a panic cannot leave
/// the user's shell raw.
pub struct TerminalGuard(());

impl TerminalGuard {
    pub fn enter() -> io::Result<Self> {
        let saved = input::enable_raw_mode()?;
        SAVED_TERMIOS.get_or_init(|| saved);
        ACTIVE.store(true, Ordering::SeqCst);
        // Constructed before the first write so a failed write still restores.
        let guard = Self(());
        INSTALL_PANIC_HOOK.call_once(|| {
            let previous = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |info| {
                // Restore first so the panic message lands on the normal
                // screen where the user can read it.
                restore();
                previous(info);
            }));
        });
        // Release builds skip the panic hook (`panic=immediate-abort`) but still
        // die by a signal, which this catches.
        crate::signal::register_fatal_cleanup(restore)?;
        write_all(ENTER)?;
        Ok(guard)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore();
    }
}

/// Undo `TerminalGuard::enter`; safe to call more than once, and from a signal
/// handler (atomics, `write`, and `tcsetattr` only).
fn restore() {
    if !ACTIVE.swap(false, Ordering::SeqCst) {
        return;
    }
    let _ = write_all(LEAVE);
    if let Some(termios) = SAVED_TERMIOS.get() {
        let _ = input::disable_raw_mode(termios);
    }
}

/// Write straight to the stdout descriptor, bypassing std's buffer and lock,
/// which may be held by the thread that is panicking or interrupted.
fn write_all(mut bytes: &[u8]) -> io::Result<()> {
    // SAFETY: descriptor 1 is open for the life of the process.
    let stdout = unsafe { BorrowedFd::borrow_raw(1) };
    while !bytes.is_empty() {
        match rustix::io::write(stdout, bytes) {
            Ok(n) => bytes = &bytes[n..],
            Err(rustix::io::Errno::INTR) => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}
