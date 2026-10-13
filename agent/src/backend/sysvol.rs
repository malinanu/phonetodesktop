//! Master volume on macOS and Linux through the system's own tools (no shell, fixed arguments).
//! The output parsers are pure functions so they are tested on any OS.

use anyhow::{anyhow, Result};
use std::process::Command;

/// `wpctl get-volume @DEFAULT_AUDIO_SINK@` -> "Volume: 0.45" or "Volume: 0.45 [MUTED]".
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn parse_wpctl(s: &str) -> Option<(u8, bool)> {
    let rest = s.trim().strip_prefix("Volume:")?.trim();
    let level: f64 = rest.split_whitespace().next()?.parse().ok()?;
    Some(((level * 100.0).round().clamp(0.0, 100.0) as u8, rest.contains("[MUTED]")))
}

/// `pactl get-sink-volume @DEFAULT_SINK@` -> "Volume: front-left: 29491 /  45% / -20.00 dB, ..." (first percentage).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn parse_pactl_volume(s: &str) -> Option<u8> {
    let pct = s.split('%').next()?.rsplit(|c: char| !c.is_ascii_digit()).next()?;
    pct.parse::<u32>().ok().map(|p| p.min(100) as u8)
}

/// `pactl get-sink-mute @DEFAULT_SINK@` -> "Mute: yes".
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn parse_pactl_mute(s: &str) -> Option<bool> {
    match s.trim().strip_prefix("Mute:")?.trim() {
        "yes" => Some(true),
        "no" => Some(false),
        _ => None,
    }
}

/// `osascript -e 'get volume settings'` -> "output volume:50, input volume:75, alert volume:100, output muted:false".
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn parse_osascript(s: &str) -> Option<(u8, bool)> {
    let field = |name: &str| s.split(',').map(str::trim).find_map(|p| p.strip_prefix(name)).map(str::trim);
    let level: u32 = field("output volume:")?.parse().ok()?;
    let muted = field("output muted:").map(|m| m == "true").unwrap_or(false);
    Some((level.min(100) as u8, muted))
}

fn run(program: &str, args: &[&str]) -> Result<String> {
    let out = Command::new(program).args(args).output().map_err(|e| anyhow!("{program}: {e}"))?;
    if !out.status.success() {
        return Err(anyhow!("{program} failed: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(target_os = "linux")]
pub fn get() -> Option<(u8, bool)> {
    if let Ok(o) = run("wpctl", &["get-volume", "@DEFAULT_AUDIO_SINK@"]) {
        if let Some(v) = parse_wpctl(&o) {
            return Some(v);
        }
    }
    let level = parse_pactl_volume(&run("pactl", &["get-sink-volume", "@DEFAULT_SINK@"]).ok()?)?;
    let muted = parse_pactl_mute(&run("pactl", &["get-sink-mute", "@DEFAULT_SINK@"]).ok()?).unwrap_or(false);
    Some((level, muted))
}

#[cfg(target_os = "linux")]
pub fn set_level(level: u8) -> Result<()> {
    let level = level.min(100);
    if run("wpctl", &["set-volume", "@DEFAULT_AUDIO_SINK@", &format!("{:.2}", level as f64 / 100.0)]).is_ok() {
        return Ok(());
    }
    run("pactl", &["set-sink-volume", "@DEFAULT_SINK@", &format!("{level}%")]).map(|_| ())
}

#[cfg(target_os = "linux")]
pub fn set_mute(muted: bool) -> Result<()> {
    let flag = if muted { "1" } else { "0" };
    if run("wpctl", &["set-mute", "@DEFAULT_AUDIO_SINK@", flag]).is_ok() {
        return Ok(());
    }
    run("pactl", &["set-sink-mute", "@DEFAULT_SINK@", flag]).map(|_| ())
}

#[cfg(target_os = "macos")]
pub fn get() -> Option<(u8, bool)> {
    parse_osascript(&run("osascript", &["-e", "get volume settings"]).ok()?)
}

#[cfg(target_os = "macos")]
pub fn set_level(level: u8) -> Result<()> {
    run("osascript", &["-e", &format!("set volume output volume {}", level.min(100))]).map(|_| ())
}

#[cfg(target_os = "macos")]
pub fn set_mute(muted: bool) -> Result<()> {
    run("osascript", &["-e", &format!("set volume output muted {muted}")]).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wpctl_output() {
        assert_eq!(parse_wpctl("Volume: 0.45\n"), Some((45, false)));
        assert_eq!(parse_wpctl("Volume: 1.00 [MUTED]"), Some((100, true)));
        assert_eq!(parse_wpctl("Volume: 1.50"), Some((100, false)));
        assert_eq!(parse_wpctl("garbage"), None);
    }

    #[test]
    fn pactl_output() {
        assert_eq!(parse_pactl_volume("Volume: front-left: 29491 /  45% / -20.00 dB,   front-right: 29491 /  45% / -20.00 dB"), Some(45));
        assert_eq!(parse_pactl_volume("Volume: mono: 65536 / 100% / 0.00 dB"), Some(100));
        assert_eq!(parse_pactl_volume("no percent here"), None);
        assert_eq!(parse_pactl_mute("Mute: yes"), Some(true));
        assert_eq!(parse_pactl_mute("Mute: no\n"), Some(false));
        assert_eq!(parse_pactl_mute("Mute: maybe"), None);
    }

    #[test]
    fn osascript_output() {
        assert_eq!(parse_osascript("output volume:50, input volume:75, alert volume:100, output muted:false"), Some((50, false)));
        assert_eq!(parse_osascript("output volume:0, input volume:missing value, alert volume:100, output muted:true\n"), Some((0, true)));
        assert_eq!(parse_osascript("nope"), None);
    }
}
