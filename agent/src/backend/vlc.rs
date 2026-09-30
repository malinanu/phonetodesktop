//! VLC has no Windows media session (VLC 3). Its HTTP interface (extraintf=http + a password)
//! reports exact position and accepts commands. `setup` turns it on.

use anyhow::Result;
use serde_json::Value;

pub const PORT: u16 = 8080;

#[derive(Debug, PartialEq)]
pub struct Status {
    pub title: String,
    pub playing: bool,
    pub stopped: bool,
    pub pos_ms: i64,
    pub dur_ms: i64,
}

pub fn parse(json: &str) -> Option<Status> {
    let v: Value = serde_json::from_str(json).ok()?;
    let state = v["state"].as_str()?;
    let meta = &v["information"]["category"]["meta"];
    let title = meta["title"].as_str().or_else(|| meta["filename"].as_str()).unwrap_or("VLC").to_string();
    Some(Status {
        title,
        playing: state == "playing",
        stopped: state == "stopped",
        pos_ms: v["time"].as_i64().unwrap_or(0) * 1000,
        dur_ms: v["length"].as_i64().unwrap_or(0) * 1000,
    })
}

/// None when the interface is off, the password is wrong, or VLC is not running.
pub fn status(password: &str) -> Option<Status> {
    parse(&super::http::get(PORT, "/requests/status.json", Some(password)).ok()?)
}

fn command(password: &str, query: &str) -> Result<()> {
    super::http::get(PORT, &format!("/requests/status.json?command={query}"), Some(password)).map(|_| ())
}

pub fn play_pause(password: &str, stopped: bool) -> Result<()> {
    command(password, if stopped { "pl_play" } else { "pl_pause" })
}
pub fn next(password: &str) -> Result<()> {
    command(password, "pl_next")
}
pub fn prev(password: &str) -> Result<()> {
    command(password, "pl_previous")
}
pub fn seek(password: &str, pos_ms: i64) -> Result<()> {
    command(password, &format!("seek&val={}", (pos_ms.max(0)) / 1000))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    const SAMPLE: &str = r#"{"state":"playing","time":83,"length":5400,"position":0.015,
        "information":{"category":{"meta":{"filename":"Movie.mkv"}}}}"#;

    #[test]
    fn parses_status() {
        let s = parse(SAMPLE).unwrap();
        assert_eq!((s.title.as_str(), s.playing, s.pos_ms, s.dur_ms), ("Movie.mkv", true, 83_000, 5_400_000));
        let idle = parse(r#"{"state":"stopped","time":0,"length":0}"#).unwrap();
        assert!(idle.stopped && !idle.playing);
        assert!(parse("not json").is_none());
    }

    #[test]
    fn sends_basic_auth_and_bad_password_is_none() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let t = std::thread::spawn(move || {
            let (mut c, _) = l.accept().unwrap();
            let mut b = [0u8; 1024];
            let n = c.read(&mut b).unwrap();
            write!(c, "HTTP/1.0 200 OK\r\n\r\n{SAMPLE}").unwrap();
            String::from_utf8_lossy(&b[..n]).to_string()
        });
        let body = crate::backend::http::get(port, "/requests/status.json", Some("secret")).unwrap();
        assert!(parse(&body).is_some());
        let req = t.join().unwrap();
        // base64(":secret") = OnNlY3JldA==
        assert!(req.contains("Authorization: Basic OnNlY3JldA=="), "{req}");
    }
}
