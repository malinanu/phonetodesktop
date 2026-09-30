//! MPC-HC / MPC-BE expose no media session, but their optional Web Interface
//! (Options > Player > Web Interface > "Listen on port", default 13579) reports the exact
//! position and accepts seek/play commands. Plain HTTP/1.0 over a std socket; no extra crates.

use anyhow::Result;

pub const DEFAULT_PORT: u16 = 13579;

#[derive(Debug, PartialEq)]
pub struct Status {
    pub file: String,
    /// 0 stopped, 1 paused, 2 playing
    pub state: i32,
    pub pos_ms: i64,
    pub dur_ms: i64,
}

fn get(port: u16, path: &str) -> Result<String> {
    super::http::get(port, path, None)
}

fn field(html: &str, id: &str) -> Option<String> {
    let start = html.find(&format!("id=\"{id}\">"))? + id.len() + 6;
    let end = html[start..].find("</p>")? + start;
    Some(html[start..end].trim().to_string())
}

pub fn parse(html: &str) -> Option<Status> {
    Some(Status {
        file: field(html, "file").unwrap_or_default(),
        state: field(html, "state")?.parse().ok()?,
        pos_ms: field(html, "position")?.parse().ok()?,
        dur_ms: field(html, "duration")?.parse().ok()?,
    })
}

/// None when the Web Interface is not enabled (connection refused) or the player is idle.
pub fn status(port: u16) -> Option<Status> {
    parse(&get(port, "/variables.html").ok()?)
}

pub fn play_pause(port: u16) -> Result<()> {
    get(port, "/command.html?wm_command=889").map(|_| ())
}

pub fn seek(port: u16, pos_ms: i64) -> Result<()> {
    let s = (pos_ms.max(0)) / 1000;
    get(port, &format!("/command.html?wm_command=-1&position={:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    const SAMPLE: &str = "<html><body><p id=\"file\">Movie.mkv</p><p id=\"state\">2</p>\
        <p id=\"statestring\">Playing</p><p id=\"position\">39372</p><p id=\"positionstring\">00:00:39</p>\
        <p id=\"duration\">5400000</p></body></html>";

    #[test]
    fn parses_variables() {
        assert_eq!(parse(SAMPLE), Some(Status { file: "Movie.mkv".into(), state: 2, pos_ms: 39372, dur_ms: 5_400_000 }));
        assert_eq!(parse("<html>nothing</html>"), None);
    }

    #[test]
    fn talks_to_a_server_and_formats_seek() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let t = std::thread::spawn(move || {
            let mut seen = vec![];
            for _ in 0..2 {
                let (mut c, _) = l.accept().unwrap();
                let mut b = [0u8; 512];
                let n = c.read(&mut b).unwrap();
                let req = String::from_utf8_lossy(&b[..n]).to_string();
                seen.push(req.lines().next().unwrap().to_string());
                write!(c, "HTTP/1.0 200 OK\r\n\r\n{SAMPLE}").unwrap();
            }
            seen
        });
        assert_eq!(status(port).unwrap().pos_ms, 39372);
        seek(port, 3_725_000).unwrap();
        let seen = t.join().unwrap();
        assert_eq!(seen[0], "GET /variables.html HTTP/1.0");
        assert_eq!(seen[1], "GET /command.html?wm_command=-1&position=01:02:05 HTTP/1.0");
    }

    #[test]
    fn refused_is_none() {
        assert!(status(1).is_none());
    }
}
