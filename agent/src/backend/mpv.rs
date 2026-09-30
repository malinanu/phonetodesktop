//! mpv exposes a JSON IPC on a named pipe when started with `input-ipc-server`.
//! `setup` adds that option to mpv.conf.

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::sync::mpsc;
use std::time::Duration;

pub const PIPE_NAME: &str = "phone-remote-mpv";

#[derive(Debug, PartialEq)]
pub struct Status {
    pub title: String,
    pub playing: bool,
    pub pos_ms: i64,
    pub dur_ms: i64,
}

/// Send all commands, then read lines until every request id is answered (events are skipped).
pub fn exchange<T: Read + Write>(io: &mut T, cmds: &[Value]) -> Result<Vec<Value>> {
    for (i, c) in cmds.iter().enumerate() {
        writeln!(io, "{}", json!({ "command": c, "request_id": i + 1 }))?;
    }
    io.flush()?;
    let mut out = vec![Value::Null; cmds.len()];
    let mut left = cmds.len();
    let mut lines = BufReader::new(io);
    let mut line = String::new();
    while left > 0 {
        line.clear();
        if lines.read_line(&mut line)? == 0 {
            return Err(anyhow!("mpv closed the pipe"));
        }
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        if let Some(id) = v["request_id"].as_u64() {
            if (1..=cmds.len() as u64).contains(&id) && out[id as usize - 1].is_null() {
                out[id as usize - 1] = v;
                left -= 1;
            }
        }
    }
    Ok(out)
}

pub fn parse_status(r: &[Value]) -> Option<Status> {
    let ok = |v: &Value| v["error"] == "success";
    let (pos, dur, pause, title) = (&r[0], &r[1], &r[2], &r[3]);
    if !ok(dur) || !ok(pos) {
        return None; // idle: no file loaded
    }
    Some(Status {
        title: title["data"].as_str().unwrap_or("mpv").to_string(),
        playing: ok(pause) && pause["data"] == false,
        pos_ms: (pos["data"].as_f64()? * 1000.0) as i64,
        dur_ms: (dur["data"].as_f64()? * 1000.0) as i64,
    })
}

#[cfg(windows)]
fn open() -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new().read(true).write(true).open(format!(r"\\.\pipe\{PIPE_NAME}"))
}
#[cfg(not(windows))]
fn open() -> std::io::Result<std::fs::File> {
    Err(std::io::ErrorKind::Unsupported.into())
}

/// Pipe reads block, so talk to mpv on a worker thread and give up after a moment.
fn with_timeout<R: Send + 'static>(f: impl FnOnce() -> R + Send + 'static) -> Option<R> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    rx.recv_timeout(Duration::from_millis(700)).ok()
}

pub fn status() -> Option<Status> {
    with_timeout(|| {
        let mut io = open().ok()?;
        let r = exchange(
            &mut io,
            &[
                json!(["get_property", "time-pos"]),
                json!(["get_property", "duration"]),
                json!(["get_property", "pause"]),
                json!(["get_property", "media-title"]),
            ],
        )
        .ok()?;
        parse_status(&r)
    })
    .flatten()
}

pub fn run(cmd: Value) -> Result<()> {
    with_timeout(move || -> Result<()> {
        let mut io = open()?;
        let r = exchange(&mut io, &[cmd])?;
        if r[0]["error"] != "success" {
            return Err(anyhow!("mpv: {}", r[0]["error"]));
        }
        Ok(())
    })
    .unwrap_or_else(|| Err(anyhow!("mpv did not answer")))
}

pub fn play_pause() -> Result<()> {
    run(json!(["cycle", "pause"]))
}
pub fn next() -> Result<()> {
    run(json!(["playlist-next"]))
}
pub fn prev() -> Result<()> {
    run(json!(["playlist-prev"]))
}
pub fn seek(pos_ms: i64) -> Result<()> {
    run(json!(["seek", pos_ms.max(0) as f64 / 1000.0, "absolute"]))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::net::UnixStream;

    #[test]
    fn exchange_skips_events_and_matches_ids() {
        let (mut client, server) = UnixStream::pair().unwrap();
        let t = std::thread::spawn(move || {
            let mut r = BufReader::new(server.try_clone().unwrap());
            let mut w = server;
            let mut n = 0;
            let mut l = String::new();
            while n < 4 {
                l.clear();
                r.read_line(&mut l).unwrap();
                n += 1;
            }
            // An event first, then responses out of order.
            writeln!(w, r#"{{"event":"playback-restart"}}"#).unwrap();
            writeln!(w, r#"{{"request_id":4,"error":"success","data":"Movie"}}"#).unwrap();
            writeln!(w, r#"{{"request_id":3,"error":"success","data":false}}"#).unwrap();
            writeln!(w, r#"{{"request_id":2,"error":"success","data":5400.0}}"#).unwrap();
            writeln!(w, r#"{{"request_id":1,"error":"success","data":83.5}}"#).unwrap();
        });
        let r = exchange(
            &mut client,
            &[json!(["get_property", "time-pos"]), json!(["get_property", "duration"]), json!(["get_property", "pause"]), json!(["get_property", "media-title"])],
        )
        .unwrap();
        t.join().unwrap();
        assert_eq!(parse_status(&r), Some(Status { title: "Movie".into(), playing: true, pos_ms: 83_500, dur_ms: 5_400_000 }));
    }

    #[test]
    fn idle_mpv_is_none() {
        let e = json!({"error":"property unavailable"});
        assert_eq!(parse_status(&[e.clone(), e.clone(), e.clone(), e]), None);
    }
}
