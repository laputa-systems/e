use std::io;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

static SIGWINCH_RECEIVED: AtomicBool = AtomicBool::new(false);
static TERMINATION_REQUESTED: AtomicBool = AtomicBool::new(false);

// The handlers only store to an `AtomicBool`, which is async-signal-safe. Linux
// takes an `unsafe extern "C" fn` in `KernelSigaction`; Darwin's local
// `sigaction` binding takes a safe one, hence the two spellings.
macro_rules! signal_handler {
    ($name:ident, $body:block) => {
        #[cfg(target_os = "macos")]
        extern "C" fn $name(_: core::ffi::c_int) $body

        #[cfg(target_os = "linux")]
        unsafe extern "C" fn $name(_: core::ffi::c_int) $body
    };
}

signal_handler!(sigwinch_handler, {
    SIGWINCH_RECEIVED.store(true, Ordering::Relaxed);
});
signal_handler!(termination_handler, {
    TERMINATION_REQUESTED.store(true, Ordering::Relaxed);
});
#[cfg(target_os = "linux")]
#[cfg(target_arch = "x86_64")]
unsafe extern "C" fn sigreturn_restorer() {
    unsafe {
        core::arch::asm!("mov rax, 15", "syscall", options(noreturn, nostack));
    }
}
#[cfg(target_os = "linux")]
#[cfg(target_arch = "aarch64")]
unsafe extern "C" fn sigreturn_restorer() {
    unsafe {
        core::arch::asm!("mov x8, #139", "svc #0", options(noreturn, nostack));
    }
}
#[cfg(target_os = "linux")]
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
compile_error!("sigreturn_restorer needs the rt_sigreturn syscall number for this architecture");
// Runs on the way to a crash. The cleanup must be async-signal-safe; the handler
// is one-shot, so returning from it re-runs the faulting instruction (or lets
// `abort` finish) under the default action and the process still dies.
signal_handler!(fatal_handler, {
    if let Some(cleanup) = FATAL_CLEANUP.get() {
        cleanup();
    }
});

static FATAL_CLEANUP: OnceLock<fn()> = OnceLock::new();

#[cfg(target_os = "linux")]
type Handler = unsafe extern "C" fn(core::ffi::c_int);
#[cfg(target_os = "macos")]
type Handler = extern "C" fn(core::ffi::c_int);

/// Signal numbers are the same on Linux and Darwin except SIGWINCH (28 on
/// Darwin).
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
    #[cfg(target_os = "linux")]
    {
        use rustix::runtime::Signal;
        use rustix::runtime::{
            How, KernelSigSet, KernelSigaction, KernelSigactionFlags, kernel_sigaction,
            kernel_sigprocmask,
        };

        let signal = match notify {
            Notify::Winch => Signal::WINCH,
            Notify::Hangup => Signal::HUP,
            Notify::Terminate => Signal::TERM,
            Notify::Interrupt => Signal::INT,
            Notify::Quit => Signal::QUIT,
            Notify::Abort => Signal::ABORT,
            Notify::IllegalInstruction => Signal::ILL,
            Notify::Trap => Signal::TRAP,
            Notify::SegmentationFault => Signal::SEGV,
            Notify::BusError => Signal::BUS,
            Notify::FloatingPointError => Signal::FPE,
        };
        let flags = if one_shot {
            KernelSigactionFlags::RESETHAND
        } else {
            KernelSigactionFlags::RESTART
        };
        // SAFETY: The handler only writes to an `AtomicBool`, which is async-signal-safe.
        // The action uses an empty signal mask and requests SA_RESTART, matching the previous
        // libc implementation. `kernel_sigaction` is unsafe because signal handlers are
        // inherently unsafe and its caller must uphold the platform ABI requirements.
        // `sigreturn_restorer` invokes only `rt_sigreturn`, which restores the
        // pre-handler context instead of returning. The signal is unblocked after
        // the handler is installed so a pending signal meets the new handler,
        // never the default disposition.
        unsafe {
            let action = KernelSigaction {
                sa_handler_kernel: Some(handler),
                sa_flags: flags | KernelSigactionFlags::RESTORER,
                sa_restorer: Some(sigreturn_restorer),
                sa_mask: KernelSigSet::empty(),
            };
            kernel_sigaction(signal, Some(action))
                .map(|_| ())
                .map_err(io::Error::from)?;
            let mut unblock = KernelSigSet::empty();
            unblock.insert(signal);
            kernel_sigprocmask(How::UNBLOCK, Some(&unblock))
                .map(|_| ())
                .map_err(io::Error::from)
        }
    }

    #[cfg(target_os = "macos")]
    {
        #[repr(C)]
        struct MacSigaction {
            sa_sigaction: usize,
            sa_mask: u32,
            sa_flags: core::ffi::c_int,
        }

        unsafe extern "C" {
            fn sigaction(
                signal: core::ffi::c_int,
                action: *const MacSigaction,
                old_action: *mut MacSigaction,
            ) -> core::ffi::c_int;
            fn pthread_sigmask(
                how: core::ffi::c_int,
                set: *const u32,
                oldset: *mut u32,
            ) -> core::ffi::c_int;
        }

        let signal = match notify {
            Notify::Winch => 28,
            Notify::Hangup => 1,
            Notify::Terminate => 15,
            Notify::Interrupt => 2,
            Notify::Quit => 3,
            Notify::Abort => 6,
            Notify::IllegalInstruction => 4,
            Notify::Trap => 5,
            Notify::SegmentationFault => 11,
            Notify::BusError => 10,
            Notify::FloatingPointError => 8,
        };
        // SA_RESETHAND is 0x4 and SA_RESTART 0x2 on Darwin.
        let flags = if one_shot { 0x0004 } else { 0x0002 };
        const SIG_UNBLOCK: core::ffi::c_int = 2;
        // SAFETY: The Darwin `sigaction` ABI uses a function pointer, 32-bit signal mask, and
        // integer flags in this order. The handler is a valid C ABI function, the mask is empty,
        // and the old action is not needed. These definitions are kept local because rustix's
        // signal-installation runtime API is Linux-only and libc is intentionally not a
        // dependency of this crate. `pthread_sigmask` takes the same narrowing
        // bitmask layout; the signal is unblocked after the handler is installed
        // so a pending signal meets the new handler, never the default action.
        let status = unsafe {
            let action = MacSigaction {
                sa_sigaction: handler as *const () as usize,
                sa_mask: 0,
                sa_flags: flags,
            };
            sigaction(signal, &action, core::ptr::null_mut())
        };
        if status != 0 {
            return Err(io::Error::last_os_error());
        }
        let bit: u32 = 1u32 << (signal - 1);
        if unsafe { pthread_sigmask(SIG_UNBLOCK, &bit, core::ptr::null_mut()) } == 0 {
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
