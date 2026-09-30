//! Tiny append-only log so a background (windowless) agent can explain itself.
//! Windows: %LOCALAPPDATA%\PhoneRemote\agent.log

use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn dir() -> PathBuf {
    dirs::data_local_dir().unwrap_or_else(std::env::temp_dir).join("PhoneRemote")
}

pub fn path() -> PathBuf {
    dir().join("agent.log")
}

/// "2026-09-30 14:05:09Z" from unix seconds (civil-from-days algorithm).
fn stamp(secs: u64) -> String {
    let (days, rem) = ((secs / 86_400) as i64, secs % 86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}Z", rem / 3600, rem / 60 % 60, rem % 60)
}

pub fn log(msg: &str) {
    let p = path();
    let _ = std::fs::create_dir_all(dir());
    if std::fs::metadata(&p).map(|m| m.len() > 512_000).unwrap_or(false) {
        let _ = std::fs::write(&p, "");
    }
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
        let _ = writeln!(f, "[{}] {msg}", stamp(now));
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| log(&format!("PANIC: {info}"))));
}

#[cfg(test)]
mod tests {
    #[test]
    fn formats_known_dates() {
        assert_eq!(super::stamp(0), "1970-01-01 00:00:00Z");
        assert_eq!(super::stamp(1782830709), "2026-06-30 14:45:09Z");
        assert_eq!(super::stamp(951_782_400), "2000-02-29 00:00:00Z"); // leap day
    }
}
