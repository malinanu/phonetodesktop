use anyhow::{Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const DEFAULT_PORT: u16 = 8765;

#[derive(Serialize, Deserialize, Clone)]
pub struct Config {
    /// Pairing secret (256 bit, base64url). Rotating it revokes every paired phone.
    pub token: String,
    pub port: u16,
    /// Windows: start-at-login was switched on once by default; afterwards the tray checkbox rules.
    #[serde(default)]
    pub autostart_initialized: bool,
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

pub fn load_or_create() -> Result<Config> {
    let p = path()?;
    if let Ok(s) = std::fs::read_to_string(&p) {
        if let Ok(c) = serde_json::from_str::<Config>(&s) {
            return Ok(c);
        }
    }
    let c = Config { token: new_token(), port: DEFAULT_PORT, autostart_initialized: false };
    save(&c)?;
    Ok(c)
}

pub fn save(c: &Config) -> Result<()> {
    std::fs::write(path()?, serde_json::to_string_pretty(c)?)?;
    Ok(())
}
