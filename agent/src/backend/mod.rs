//! OS backends. Every backend is blocking; the server calls them from `spawn_blocking`.

use crate::protocol::PlayerInfo;
use anyhow::Result;

pub mod keys;
pub mod mock;
#[cfg_attr(not(windows), allow(dead_code))]
pub mod http;
#[cfg_attr(not(windows), allow(dead_code))]
pub mod mpc;
#[cfg_attr(not(windows), allow(dead_code))]
pub mod mpv;
#[cfg_attr(not(windows), allow(dead_code))]
pub mod setup;
#[cfg_attr(not(windows), allow(dead_code))]
pub mod vlc;
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
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ButtonAction {
    Click,
    Down,
    Up,
}

/// Validated mouse/keyboard input, already clamped by the controller.
#[derive(Clone, Debug, PartialEq)]
pub enum Input {
    Move(i32, i32),
    Button(MouseButton, ButtonAction),
    Scroll(i32, i32),
    Text(String),
    Key { name: String, mods: Vec<String> },
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
    /// Master volume (0-100) and mute state, if this system can report them.
    fn volume(&self) -> Option<(u8, bool)> {
        None
    }
    fn set_volume(&self, _level: u8) -> Result<()> {
        Err(anyhow::anyhow!("volume level is not supported here"))
    }
    fn set_mute(&self, _muted: bool) -> Result<()> {
        Err(anyhow::anyhow!("mute is not supported here"))
    }
    /// Mouse and keyboard input. Backends without support refuse it.
    fn input(&self, _input: Input) -> Result<()> {
        Err(anyhow::anyhow!("mouse and keyboard control is not supported on this system"))
    }
    /// Human-readable dump for the /debug page.
    fn debug(&self) -> String {
        format!("backend: {}\n{:#?}", self.name(), self.snapshot())
    }
}

/// `local_secret` is a per-install secret shared with player interfaces (VLC's HTTP password).
pub fn default_backend(force_mock: bool, local_secret: &str) -> Box<dyn Backend> {
    #[cfg(windows)]
    if !force_mock {
        return Box::new(windows::WindowsBackend { vlc_password: vlc_password(local_secret) });
    }
    let _ = (force_mock, local_secret);
    Box::new(mock::MockBackend::new())
}

pub fn vlc_password(local_secret: &str) -> String {
    local_secret.chars().take(20).collect()
}
