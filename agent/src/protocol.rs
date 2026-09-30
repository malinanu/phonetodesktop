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
}

#[derive(Deserialize, Debug)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum ClientMsg {
    /// Legacy: `device` absent, `token` is the shared secret. Otherwise `device` + its own token.
    Auth { token: String, #[serde(default)] device: Option<String> },
    /// First contact with a pairing code from the QR. The PC owner must approve.
    Pair { code: String, device: String, name: String },
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
    Mute,
    Select { id: String },
}
