use std::io;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

static SIGWINCH_RECEIVED: AtomicBool = AtomicBool::new(false);
static TERMINATION_REQUESTED: AtomicBool = AtomicBool::new(false);

// The handlers only store to an `AtomicBool`, which is async-signal-safe.
macro_rules! signal_handler {
    ($name:ident, $body:block) => {
        unsafe extern "C" fn $name(_: core::ffi::c_int) $body
    };
}

signal_handler!(sigwinch_handler, {
    SIGWINCH_RECEIVED.store(true, Ordering::Relaxed);
});
signal_handler!(termination_handler, {
    TERMINATION_REQUESTED.store(true, Ordering::Relaxed);
});
// Runs on the way to a crash. The cleanup must be async-signal-safe; the handler
// is one-shot, so returning from it re-runs the faulting instruction (or lets
// `abort` finish) under the default action and the process still dies.
signal_handler!(fatal_handler, {
    if let Some(cleanup) = FATAL_CLEANUP.get() {
        cleanup();
    }
});

static FATAL_CLEANUP: OnceLock<fn()> = OnceLock::new();

type Handler = unsafe extern "C" fn(core::ffi::c_int);

enum Notify {
    Winch,
    Hangup,
    Terminate,
    Interrupt,
    Quit,
    Abort,
    IllegalInstruction,
    Trap,
    SegmentationFault,
    BusError,
    FloatingPointError,
}

/// `one_shot` restores the default action before the handler runs.
fn install(notify: Notify, handler: Handler, one_shot: bool) -> io::Result<()> {
    let signal = match notify {
        Notify::Winch => libc::SIGWINCH,
        Notify::Hangup => libc::SIGHUP,
        Notify::Terminate => libc::SIGTERM,
        Notify::Interrupt => libc::SIGINT,
        Notify::Quit => libc::SIGQUIT,
        Notify::Abort => libc::SIGABRT,
        Notify::IllegalInstruction => libc::SIGILL,
        Notify::Trap => libc::SIGTRAP,
        Notify::SegmentationFault => libc::SIGSEGV,
        Notify::BusError => libc::SIGBUS,
        Notify::FloatingPointError => libc::SIGFPE,
    };
    let flags = if one_shot { libc::SA_RESETHAND } else { libc::SA_RESTART };
    // SA_RESETHAND restores the default action before the handler runs;
    // SA_RESTART restarts syscalls interrupted by the signal. The libc
    // `sigaction` wrapper supplies the kernel's restorer trampoline, so the
    // handler needs no arch-specific restorer of its own. The signal is
    // unblocked after the handler is installed so a pending signal meets the
    // new handler, never the default disposition.
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = handler as libc::sighandler_t;
        libc::sigemptyset(&mut action.sa_mask);
        action.sa_flags = flags;
        if libc::sigaction(signal, &action, std::ptr::null_mut()) != 0 {
            return Err(io::Error::last_os_error());
        }
        let mut unblock: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut unblock);
        libc::sigaddset(&mut unblock, signal);
        if libc::pthread_sigmask(libc::SIG_UNBLOCK, &unblock, std::ptr::null_mut()) == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
}

pub fn register_sigwinch() -> io::Result<()> {
    install(Notify::Winch, sigwinch_handler, false)
}

/// Ask for a clean shutdown on SIGTERM, SIGHUP, SIGINT, and SIGQUIT (kill,
/// closed ssh session or terminal window, kill -INT/-QUIT). The event loop
/// polls `termination_requested` so the terminal modes and lock are restored
/// on the way out.
pub fn register_termination() -> io::Result<()> {
    install(Notify::Hangup, termination_handler, false)?;
    install(Notify::Terminate, termination_handler, false)?;
    install(Notify::Interrupt, termination_handler, false)?;
    install(Notify::Quit, termination_handler, false)
}

/// Run `cleanup` when the process is about to die from an abort, illegal
/// instruction, trap, segmentation fault, bus error, or floating-point
/// exception: the ways a Rust panic or a hard crash ends the process when the
/// panic hook cannot run (`panic=immediate-abort` release builds skip it).
/// `cleanup` executes in a signal handler, so it may only use
/// async-signal-safe calls.
pub fn register_fatal_cleanup(cleanup: fn()) -> io::Result<()> {
    FATAL_CLEANUP.get_or_init(|| cleanup);
    install(Notify::Abort, fatal_handler, true)?;
    install(Notify::IllegalInstruction, fatal_handler, true)?;
    install(Notify::Trap, fatal_handler, true)?;
    install(Notify::SegmentationFault, fatal_handler, true)?;
    install(Notify::BusError, fatal_handler, true)?;
    install(Notify::FloatingPointError, fatal_handler, true)
}

pub fn take_sigwinch() -> bool {
    SIGWINCH_RECEIVED.swap(false, Ordering::Relaxed)
}

pub fn termination_requested() -> bool {
    TERMINATION_REQUESTED.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_and_take() {
        register_sigwinch().unwrap();
        // Initially false (or may have been set by previous test — clear it)
        let _ = take_sigwinch();
        assert!(!take_sigwinch());
    }

    #[test]
    fn test_signal_delivery() {
        register_sigwinch().unwrap();
        let _ = take_sigwinch(); // clear
        // Directly set the flag to test take_sigwinch
        SIGWINCH_RECEIVED.store(true, std::sync::atomic::Ordering::Relaxed);
        assert!(take_sigwinch());
        assert!(!take_sigwinch()); // consumed
    }

    #[test]
    fn test_termination_signal_sets_flag() {
        register_termination().unwrap();
        assert!(!termination_requested());
        // SAFETY: raising SIGTERM at ourselves after installing a handler that
        // only sets a flag; no other test in this binary depends on SIGTERM.
        unsafe extern "C" {
            fn raise(signal: core::ffi::c_int) -> core::ffi::c_int;
        }
        assert_eq!(unsafe { raise(15) }, 0);
        assert!(termination_requested());
    }
}
