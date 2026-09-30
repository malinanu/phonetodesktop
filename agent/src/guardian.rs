//! The guardian: a tiny hidden process that keeps the real agent ("worker") alive.
//! It starts the worker, waits, and relaunches it after any crash, kill or hang. It stops only
//! when the worker exits with `EXIT_QUIT` (the user chose Quit in the tray). Every exit is logged
//! with its code, which names the cause when the worker dies without a trace.

use crate::backend::http;
use crate::log::log;
use std::collections::VecDeque;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

/// Worker exit codes the guardian understands.
pub const EXIT_QUIT: i32 = 0;
pub const EXIT_PORT_BUSY: i32 = 10;
/// The worker's tray loop ended for an unknown reason (restart it).
#[cfg_attr(not(windows), allow(dead_code))]
pub const EXIT_TRAY_LOST: i32 = 3;

#[derive(Debug, PartialEq)]
pub enum Verdict {
    /// The user quit: stop everything.
    Stop,
    /// Something else owns our port: restarting cannot help.
    PortBusy,
    /// Anything else: bring it back.
    Restart,
}

pub fn classify(code: Option<i32>) -> Verdict {
    match code {
        Some(EXIT_QUIT) => Verdict::Stop,
        Some(EXIT_PORT_BUSY) => Verdict::PortBusy,
        _ => Verdict::Restart,
    }
}

/// How long to wait before relaunching, given how many crashes happened in the last minute.
/// Quick at first so the user barely notices; slower in a crash loop so we never spin.
pub fn restart_delay(recent_crashes: usize) -> Duration {
    if recent_crashes >= 5 {
        Duration::from_secs(30)
    } else {
        Duration::from_secs(2)
    }
}

/// "0xC0000005" for NTSTATUS-style exit codes, plain decimal otherwise.
pub fn describe_exit(code: Option<i32>) -> String {
    match code {
        None => "no exit code (terminated)".into(),
        Some(c) if c < 0 || c > 0xFFFF => format!("{:#010X}", c as u32),
        Some(c) => c.to_string(),
    }
}

struct Health {
    fails: u32,
    next_probe: Instant,
}

impl Health {
    fn new() -> Self {
        Health { fails: 0, next_probe: Instant::now() + Duration::from_secs(20) }
    }

    /// Probe every 10 s; three misses in a row mean the server is hung.
    fn tick(&mut self, port: u16) -> bool {
        if Instant::now() < self.next_probe {
            return false;
        }
        self.next_probe = Instant::now() + Duration::from_secs(10);
        self.fails = if http::get(port, "/health", None).is_ok() { 0 } else { self.fails + 1 };
        self.fails >= 3
    }
}

fn spawn_worker(forward: &[String], restarts: u32, last_exit: &str) -> std::io::Result<Child> {
    let exe = std::env::current_exe()?;
    let mut cmd = Command::new(exe);
    cmd.arg("--worker").args(forward);
    if restarts > 0 {
        // After a restart nothing should pop up in front of the user.
        cmd.arg("--background");
    }
    cmd.env("PR_RESTARTS", restarts.to_string()).env("PR_LAST_EXIT", last_exit);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd.spawn()
}

/// Returns the process exit code for the guardian itself.
pub fn run(port: u16, forward: Vec<String>) -> i32 {
    log(&format!("guardian {} started (pid {})", env!("CARGO_PKG_VERSION"), std::process::id()));
    let mut crashes: VecDeque<Instant> = VecDeque::new();
    let mut restarts = 0u32;
    let mut last_exit = String::new();
    loop {
        let mut child = match spawn_worker(&forward, restarts, &last_exit) {
            Ok(c) => c,
            Err(e) => {
                log(&format!("guardian: cannot start the agent: {e}; retrying in 10 s"));
                std::thread::sleep(Duration::from_secs(10));
                continue;
            }
        };
        let started = Instant::now();
        let mut health = Health::new();
        let mut hung = false;
        let code = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status.code(),
                Ok(None) => {}
                Err(e) => {
                    log(&format!("guardian: lost track of the agent: {e}"));
                    break None;
                }
            }
            std::thread::sleep(Duration::from_millis(250));
            if health.tick(port) {
                log("guardian: the agent stopped answering on /health; restarting it");
                let _ = child.kill();
                let _ = child.wait();
                hung = true;
                break None;
            }
        };
        let uptime = started.elapsed().as_secs();
        let what = if hung { "hung (killed by guardian)".to_string() } else { describe_exit(code) };
        log(&format!("guardian: agent exited: {what} after {uptime}s"));
        match classify(code) {
            Verdict::Stop if !hung => {
                log("guardian: quit requested, stopping");
                return 0;
            }
            Verdict::PortBusy if !hung => {
                log(&format!("guardian: port {port} is used by another program; not restarting"));
                return EXIT_PORT_BUSY;
            }
            _ => {}
        }
        let now = Instant::now();
        crashes.push_back(now);
        while crashes.front().is_some_and(|t| now.duration_since(*t) > Duration::from_secs(60)) {
            crashes.pop_front();
        }
        restarts += 1;
        last_exit = what;
        let wait = restart_delay(crashes.len());
        log(&format!("guardian: restarting the agent in {}s (restart #{restarts})", wait.as_secs()));
        std::thread::sleep(wait);
    }
}

/// Only one guardian per user session. Returns false if another one is already running.
#[cfg(windows)]
pub fn acquire_single_instance() -> bool {
    use windows::core::w;
    use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
    use windows::Win32::System::Threading::CreateMutexW;
    // The handle is never closed (HANDLE has no Drop): the mutex lives as long as this process.
    match unsafe { CreateMutexW(None, false, w!("Local\\PhoneRemoteGuardian")) } {
        Ok(_) => unsafe { GetLastError() != ERROR_ALREADY_EXISTS },
        Err(_) => true,
    }
}

#[cfg(not(windows))]
pub fn acquire_single_instance() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes_are_classified() {
        assert_eq!(classify(Some(0)), Verdict::Stop);
        assert_eq!(classify(Some(10)), Verdict::PortBusy);
        for c in [Some(1), Some(3), Some(-1073741819), Some(101), None] {
            assert_eq!(classify(c), Verdict::Restart, "{c:?}");
        }
    }

    #[test]
    fn backoff_only_in_a_crash_loop() {
        assert_eq!(restart_delay(0), Duration::from_secs(2));
        assert_eq!(restart_delay(4), Duration::from_secs(2));
        assert_eq!(restart_delay(5), Duration::from_secs(30));
        assert_eq!(restart_delay(50), Duration::from_secs(30));
    }

    #[test]
    fn exit_codes_are_readable() {
        assert_eq!(describe_exit(Some(-1073741819)), "0xC0000005");
        assert_eq!(describe_exit(Some(7)), "7");
        assert_eq!(describe_exit(None), "no exit code (terminated)");
    }
}
