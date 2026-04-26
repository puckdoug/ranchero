//! Process liveness probe. Trait-based so the lifecycle logic can be tested
//! without actually invoking the OS.

pub trait ProcessProbe: Send + Sync {
    /// Return `true` if the given PID names a live process owned by the
    /// caller (or, on systems where signal-0 is permitted across users, any
    /// live process).
    fn is_alive(&self, pid: u32) -> bool;
}

/// Real probe: uses `kill(pid, 0)` on Unix, no-op on Windows for now.
pub struct OsProcessProbe;

impl ProcessProbe for OsProcessProbe {
    #[cfg(unix)]
    fn is_alive(&self, pid: u32) -> bool {
        use nix::errno::Errno;
        use nix::sys::signal::kill;
        use nix::unistd::Pid;
        // signal 0 doesn't deliver anything; it's a permission/existence probe.
        match kill(Pid::from_raw(pid as i32), None) {
            Ok(()) => true,
            // EPERM means the PID exists but is owned by another user; that
            // still counts as "alive" for our refusal-to-double-start logic.
            Err(Errno::EPERM) => true,
            Err(_) => false,
        }
    }

    #[cfg(windows)]
    fn is_alive(&self, _pid: u32) -> bool {
        // Windows backgrounding is not supported in STEP 03; tests on
        // Windows shouldn't hit this path in practice.
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// In-memory probe that records whether is_alive was queried, and
    /// returns a configurable answer. Used by lifecycle logic tests to
    /// confirm the lifecycle module consults the probe rather than calling
    /// `kill(2)` directly.
    pub struct StubProbe {
        pub alive: bool,
        pub queried: AtomicBool,
    }
    impl StubProbe {
        pub fn new(alive: bool) -> Self {
            Self { alive, queried: AtomicBool::new(false) }
        }
    }
    impl ProcessProbe for StubProbe {
        fn is_alive(&self, _pid: u32) -> bool {
            self.queried.store(true, Ordering::SeqCst);
            self.alive
        }
    }

    #[test]
    fn stub_probe_records_query() {
        let p = StubProbe::new(true);
        assert!(p.is_alive(1));
        assert!(p.queried.load(Ordering::SeqCst));
    }

    #[test]
    fn os_probe_self_is_alive() {
        // The current process is always alive.
        assert!(OsProcessProbe.is_alive(std::process::id()));
    }

    #[test]
    fn os_probe_unlikely_pid_is_not_alive() {
        // PID 999_999 is overwhelmingly unlikely to exist.
        assert!(!OsProcessProbe.is_alive(999_999));
    }
}
