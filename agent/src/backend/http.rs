//! Minimal HTTP/1.0 GET client for talking to local player interfaces (no extra crates).

use anyhow::{anyhow, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::time::Duration;

/// GET `path` on 127.0.0.1:port; optional HTTP basic auth with an empty user name
/// (what VLC expects). Returns the body of a 2xx response.
pub fn get(port: u16, path: &str, password: Option<&str>) -> Result<String> {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_millis(200))?;
    s.set_read_timeout(Some(Duration::from_millis(500)))?;
    s.set_write_timeout(Some(Duration::from_millis(500)))?;
    let auth = password
        .map(|p| format!("Authorization: Basic {}\r\n", STANDARD.encode(format!(":{p}"))))
        .unwrap_or_default();
    // One write: a single segment, and servers that read the request once see all of it.
    s.write_all(format!("GET {path} HTTP/1.0\r\nHost: 127.0.0.1\r\n{auth}Connection: close\r\n\r\n").as_bytes())?;
    let mut buf = Vec::new();
    // A timeout after data arrived still leaves a usable response.
    let _ = s.read_to_end(&mut buf);
    let text = String::from_utf8_lossy(&buf);
    let (head, body) = text.split_once("\r\n\r\n").ok_or_else(|| anyhow!("malformed response"))?;
    let code: u16 = head.split_whitespace().nth(1).and_then(|c| c.parse().ok()).ok_or_else(|| anyhow!("no status"))?;
    if !(200..300).contains(&code) {
        return Err(anyhow!("HTTP {code}"));
    }
    Ok(body.to_string())
}
