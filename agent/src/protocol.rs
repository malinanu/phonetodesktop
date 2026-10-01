//! Wire types shared by the phone clients and the agent.
//!
//! Phone -> agent: `{"t":"auth","token":"..."}` first, then `{"t":"cmd","c":"seek_rel","d":10}` etc.
//! Agent -> phone: `{"t":"state", ...}` on change and about once a second.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct PlayerInfo {
    /// Stable id of the session (GSMTC source app id, MPRIS bus name, ...).
    pub id: String,
    pub app: String,
    pub title: String,
    pub artist: String,
    pub playing: bool,
    /// Position at the moment the snapshot was taken, already extrapolated.
    pub pos_ms: i64,
    pub dur_ms: i64,
    pub can_seek: bool,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct State {
    pub t: &'static str,
    pub host: String,
    pub backend: &'static str,
    pub version: &'static str,
    /// Id of the player the commands go to.
    pub current: Option<String>,
    pub players: Vec<PlayerInfo>,
    /// Master volume 0-100 and mute state, when the PC reports them.
    pub volume: Option<u8>,
    pub muted: Option<bool>,
}

#[derive(Deserialize, Debug)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum ClientMsg {
    /// Legacy: `device` absent, `token` is the shared secret. Otherwise `device` + its own token.
    Auth { token: String, #[serde(default)] device: Option<String> },
    /// First contact with a pairing code from the QR. The PC owner must approve.
    /// `pk` (base64url Ed25519 public key) makes it a protocol v2 pairing: the phone then logs in with
    /// `challenge` + `auth_sig` and the PC never hands it a token.
    Pair { code: String, device: String, name: String, #[serde(default)] pk: Option<String>, #[serde(default)] platform: Option<String> },
    /// Protocol v2 login, step 1: the PC answers with `{"t":"challenge","nonce":...}`.
    Challenge { device: String },
    /// Protocol v2 login, step 2: base64url Ed25519 signature over `auth::auth_message(pc_id, device, nonce)`.
    AuthSig { device: String, sig: String },
    Cmd(Command),
    Ping,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "c", rename_all = "snake_case")]
pub enum Command {
    PlayPause,
    Next,
    Prev,
    /// Relative seek in seconds (negative = back).
    SeekRel { d: i64 },
    /// Absolute seek in milliseconds.
    SeekAbs { pos_ms: i64 },
    /// Master volume in steps of 2%.
    Volume { d: i32 },
    /// Master volume to an exact level, 0-100.
    VolumeSet { level: u8 },
    Mute,
    Select { id: String },
    // ---- mouse and keyboard (need the phone's "mouse & keyboard" permission) ----
    /// Relative cursor move in pixels.
    MouseMove { dx: i32, dy: i32 },
    /// `button`: left|right|middle. `action`: click|down|up.
    MouseButton { button: String, action: String },
    /// Wheel units: 120 is one notch. Positive `dy` scrolls up.
    Scroll { dx: i32, dy: i32 },
    /// Typed text (any Unicode).
    Text { s: String },
    /// A named key with optional modifiers: ctrl, alt, shift, win.
    Key { name: String, #[serde(default)] mods: Vec<String> },
}

impl Command {
    pub fn is_input(&self) -> bool {
        matches!(self, Command::MouseMove { .. } | Command::MouseButton { .. } | Command::Scroll { .. } | Command::Text { .. } | Command::Key { .. })
    }
}
