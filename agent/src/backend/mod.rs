//! OS backends. Every backend is blocking; the server calls them from `spawn_blocking`.

use crate::protocol::PlayerInfo;
use anyhow::Result;

pub mod mock;
#[cfg_attr(not(windows), allow(dead_code))]
pub mod mpc;
#[cfg(windows)]
pub mod windows;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Key {
    PlayPause,
    Next,
    Prev,
    VolUp,
    VolDown,
    Mute,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Transport {
    PlayPause,
    Next,
    Prev,
}

pub trait Backend: Send + Sync {
    fn name(&self) -> &'static str;
    /// All known sessions, the OS's "current" session first.
    fn snapshot(&self) -> Result<Vec<PlayerInfo>>;
    /// Transport command on one session. Returns false if the app refused it.
    fn transport(&self, id: &str, what: Transport) -> Result<bool>;
    /// Absolute seek. May return true without seeking (some apps lie); the caller verifies.
    fn seek_abs(&self, id: &str, pos_ms: i64) -> Result<bool>;
    /// Synthetic system media key (works for apps without a session).
    fn media_key(&self, key: Key) -> Result<()>;
    /// Send Left/Right arrows to the foreground window if it is a known player.
    /// Returns the number of seconds actually skipped (0 = nothing sent).
    fn focused_seek(&self, secs: i64) -> Result<i64>;
    /// Human-readable dump for the /debug page.
    fn debug(&self) -> String {
        format!("backend: {}\n{:#?}", self.name(), self.snapshot())
    }
}

pub fn default_backend(force_mock: bool) -> Box<dyn Backend> {
    #[cfg(windows)]
    if !force_mock {
        return Box::new(windows::WindowsBackend);
    }
    let _ = force_mock;
    Box::new(mock::MockBackend::new())
}
