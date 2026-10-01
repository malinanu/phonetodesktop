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
    /// Address of the "Send files" server (docs/DEPLOYING-SERVER.md). Empty = the address baked in at build
    /// time (`PHONE_REMOTE_FILES_URL`), if any. When neither is set the feature is hidden.
    #[serde(default)]
    pub files_url: String,
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
    let c = Config { pc_id: new_id(), token: new_token(), devices: vec![], legacy_shared_auth: false, setup_done: false, port: DEFAULT_PORT, autostart_initialized: false, local_secret: new_token(), files_url: String::new() };
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

/// Normalise a file-server address: https only (a bare host gets `https://`), no credentials, spaces or
/// other schemes. Mirrors `FilesUrl.kt` in the Android app.
pub fn clean_files_url(raw: &str) -> Option<String> {
    let s = raw.trim();
    if s.is_empty() || s.chars().any(|c| c.is_whitespace() || c == '\\') {
        return None;
    }
    let s = if s.contains("://") { s.to_string() } else { format!("https://{s}") };
    let rest = match s.get(..8) {
        Some(p) if p.eq_ignore_ascii_case("https://") => &s[8..],
        _ => return None,
    };
    let cut = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(cut);
    if authority.is_empty() || authority.contains('@') {
        return None;
    }
    let (host, port) = match authority.split_once(':') {
        Some((h, p)) => (h, Some(p)),
        None => (authority, None),
    };
    if let Some(p) = port {
        if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) || !matches!(p.parse::<u16>(), Ok(n) if n >= 1) {
            return None;
        }
    }
    let label_ok = |l: &str| {
        let b = l.as_bytes();
        !b.is_empty() && b[0].is_ascii_alphanumeric() && b[b.len() - 1].is_ascii_alphanumeric() && b.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'-')
    };
    if !host.split('.').all(label_ok) {
        return None;
    }
    Some(format!("https://{}{}", authority.to_ascii_lowercase(), tail))
}

/// The "Send files" address: the config value if valid, else the one baked in at build time.
pub fn files_url(cfg: &Config) -> Option<String> {
    clean_files_url(&cfg.files_url).or_else(|| option_env!("PHONE_REMOTE_FILES_URL").and_then(clean_files_url))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_https_and_normalises() {
        assert_eq!(clean_files_url("https://files.example.com").as_deref(), Some("https://files.example.com"));
        assert_eq!(clean_files_url("  FILES.example.com ").as_deref(), Some("https://files.example.com"));
        assert_eq!(clean_files_url("https://Files.Example.com:8443/app?x=1").as_deref(), Some("https://files.example.com:8443/app?x=1"));
    }

    #[test]
    fn rejects_anything_unsafe_or_empty() {
        for bad in [
            "", "   ", "http://files.example.com", "ftp://x.com", "javascript:alert(1)", "https://user:pw@files.example.com",
            "https://a b.com", "https://", "https://:443", "https://files.example.com:0", "https://files.example.com:99999",
            "https://files.example.com:x", "https://files.example.com:+80", "https://-bad.example.com", "https://exa_mple.com",
            "https://files.example.com\\evil", "https://[::1]/", "https://a..b.com",
        ] {
            assert_eq!(clean_files_url(bad), None, "should reject: {bad}");
        }
    }

    #[test]
    fn config_value_is_used_when_valid() {
        let mut c = Config { pc_id: String::new(), token: String::new(), devices: vec![], legacy_shared_auth: false, port: 1, local_secret: String::new(), setup_done: true, autostart_initialized: true, files_url: "files.example.com".into() };
        assert_eq!(files_url(&c).as_deref(), Some("https://files.example.com"));
        c.files_url = "http://insecure.example.com".into();
        assert_eq!(files_url(&c), option_env!("PHONE_REMOTE_FILES_URL").and_then(clean_files_url));
    }

    #[test]
    fn old_config_files_without_the_key_still_load() {
        let c: Config = serde_json::from_str(r#"{"token":"t","port":8765}"#).unwrap();
        assert_eq!(c.files_url, "");
    }
}
