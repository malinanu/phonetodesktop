use anyhow::{Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const DEFAULT_PORT: u16 = 8765;

#[derive(Serialize, Deserialize, Clone)]
pub struct Config {
    /// Public, stable identity of this PC (not secret). Phones use it to match mDNS sightings to saved pairings.
    #[serde(default)]
    pub pc_id: String,
    /// Legacy shared secret from before per-phone tokens. Only honoured while `legacy_shared_auth` is on.
    pub token: String,
    /// Phones approved on this PC, each with its own revocable token.
    #[serde(default)]
    pub devices: Vec<Device>,
    /// Old configs had no devices: phones paired with the shared secret keep working until the
    /// user switches this off. New installs start with it off.
    #[serde(default = "yes")]
    pub legacy_shared_auth: bool,
    pub port: u16,
    /// Per-install secret for local player interfaces (VLC HTTP password). Not shown to phones.
    #[serde(default)]
    pub local_secret: String,
    /// The first-run wizard has been completed (or skipped). Older installs count as done.
    #[serde(default = "yes")]
    pub setup_done: bool,
    /// Windows: start-at-login was switched on once by default; afterwards the tray checkbox rules.
    #[serde(default)]
    pub autostart_initialized: bool,
}

fn yes() -> bool {
    true
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Device {
    /// Chosen by the phone (stable across its IP changes).
    pub id: String,
    pub name: String,
    pub token: String,
    pub created: u64,
    #[serde(default)]
    pub last_seen: u64,
    /// May this phone move the mouse and type? Switchable per phone in the dashboard.
    #[serde(default = "yes")]
    pub input_allowed: bool,
}

fn path() -> Result<PathBuf> {
    let dir = dirs::config_dir().context("no config dir")?.join("phone-remote");
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("config.json"))
}

pub fn new_token() -> String {
    let mut b = [0u8; 32];
    rand::rng().fill_bytes(&mut b);
    URL_SAFE_NO_PAD.encode(b)
}

pub fn new_id() -> String {
    let mut b = [0u8; 9];
    rand::rng().fill_bytes(&mut b);
    URL_SAFE_NO_PAD.encode(b)
}

pub fn load_or_create() -> Result<Config> {
    let p = path()?;
    if let Ok(s) = std::fs::read_to_string(&p) {
        match serde_json::from_str::<Config>(&s) {
            Ok(mut c) => {
                // Fill fields added in later versions, keeping the existing pairing untouched.
                let mut dirty = false;
                if c.local_secret.is_empty() {
                    c.local_secret = new_token();
                    dirty = true;
                }
                if c.pc_id.is_empty() {
                    c.pc_id = new_id();
                    dirty = true;
                }
                if dirty {
                    save(&c)?;
                }
                return Ok(c);
            }
            // Never silently replace a damaged file: that would unpair every phone.
            Err(_) => {
                let _ = std::fs::rename(&p, p.with_extension("json.bad"));
            }
        }
    }
    let c = Config { pc_id: new_id(), token: new_token(), devices: vec![], legacy_shared_auth: false, setup_done: false, port: DEFAULT_PORT, autostart_initialized: false, local_secret: new_token() };
    save(&c)?;
    Ok(c)
}

/// Write to a temp file and rename, so a crash mid-write cannot corrupt the pairing secret.
pub fn save(c: &Config) -> Result<()> {
    let p = path()?;
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(c)?)?;
    std::fs::rename(&tmp, &p)?;
    Ok(())
}
