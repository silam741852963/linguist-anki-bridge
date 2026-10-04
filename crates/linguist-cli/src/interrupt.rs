//! Interruption exits (exit code 130 for SIGINT, 143 for SIGTERM).
//!
//! Every durable record is written in a SQLite transaction or published by
//! atomic rename, and every collection effect is journaled before it is sent.
//! An interruption therefore stops the process at once: committed records are
//! kept, an open transaction is rolled back by SQLite on the next open, and
//! interrupted jobs or operations are found by `recover inspect --pending` and
//! `jobs list`. The handler only calls async-signal-safe `write` and `_exit`.

#[cfg(unix)]
mod imp {
    const MESSAGE: &[u8] = b"{\"error\":\"INTERRUPTED: stopped by a signal; committed local records are kept and uncommitted work was discarded\",\"next\":\"Run `linguist-anki-bridge recover inspect --pending` and `linguist-anki-bridge jobs list` to find interrupted work; a job left running needs `linguist-anki-bridge jobs recover JOB`.\"}\n";

    extern "C" fn on_signal(signal: libc::c_int) {
        // SAFETY: write(2) and _exit(2) are async-signal-safe; MESSAGE is static.
        unsafe {
            libc::write(2, MESSAGE.as_ptr().cast(), MESSAGE.len());
            libc::_exit(128 + signal);
        }
    }

    pub fn install() {
        for signal in [libc::SIGINT, libc::SIGTERM] {
            // SAFETY: a zeroed sigaction with an empty mask and a plain handler
            // is valid; the handler is async-signal-safe.
            unsafe {
                let mut action: libc::sigaction = std::mem::zeroed();
                action.sa_sigaction = on_signal as *const () as libc::sighandler_t;
                libc::sigemptyset(&mut action.sa_mask);
                // Respect a signal the parent deliberately ignores (nohup, `&`).
                let mut previous: libc::sigaction = std::mem::zeroed();
                if libc::sigaction(signal, std::ptr::null(), &mut previous) == 0
                    && previous.sa_sigaction == libc::SIG_IGN
                {
                    continue;
                }
                libc::sigaction(signal, &action, std::ptr::null_mut());
            }
        }
    }
}

#[cfg(unix)]
pub use imp::install;

#[cfg(not(unix))]
pub fn install() {}
