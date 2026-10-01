//! Per-OS glue that is not media control: opening a URL, start-at-login, and the `open` launcher that
//! starts the agent if needed and shows the dashboard (what a Start-menu / Applications icon runs).
//! Windows keeps its tray and registry autostart (see `tray.rs`); macOS and Linux use the files below.

#[cfg(unix)]
use anyhow::{anyhow, Result};
#[cfg(unix)]
use std::path::{Path, PathBuf};

/// Open a URL in the default browser without waiting for it and without a console window.
pub fn open_url(url: &str) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("cmd")
            .args(["/c", "start", "", url])
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .spawn();
    }
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).spawn();
    #[cfg(target_os = "linux")]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    let _ = url;
}

/// True when something answers `GET /health` with 200 on this port (our agent, or at least a server speaking HTTP).
#[cfg(unix)]
pub fn agent_running(port: u16) -> bool {
    use std::io::{Read, Write};
    use std::net::{SocketAddr, TcpStream};
    use std::time::Duration;
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let Ok(mut s) = TcpStream::connect_timeout(&addr, Duration::from_millis(300)) else { return false };
    let _ = s.set_read_timeout(Some(Duration::from_millis(800)));
    if s.write_all(b"GET /health HTTP/1.0\r\nHost: 127.0.0.1\r\n\r\n").is_err() {
        return false;
    }
    let mut buf = [0u8; 32];
    let n = s.read(&mut buf).unwrap_or(0);
    std::str::from_utf8(&buf[..n]).map(|h| h.starts_with("HTTP/1.") && h.contains(" 200")).unwrap_or(false)
}

/// Start the agent in the background, detached from this terminal (it keeps running after `open` exits).
#[cfg(unix)]
pub fn spawn_agent() -> Result<()> {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};
    Command::new(std::env::current_exe()?)
        .args(["serve", "--background"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(|e| anyhow!("cannot start the agent: {e}"))?;
    Ok(())
}

/// `phone-remote open`: make sure the agent is up, then show its dashboard in the browser.
#[cfg(unix)]
pub fn open_dashboard(port: u16) -> Result<()> {
    if !agent_running(port) {
        spawn_agent()?;
        for _ in 0..40 {
            if agent_running(port) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(150));
        }
    }
    open_url(&format!("http://127.0.0.1:{port}/dashboard"));
    Ok(())
}

// ---- start at login (macOS LaunchAgent, Linux XDG autostart) ---------------------------------------

#[cfg(unix)]
const MAC_LABEL: &str = "app.phoneremote.agent";

#[cfg(unix)]
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// launchd job: start at login, and restart after a crash but not after a clean quit.
#[cfg(unix)]
pub fn launch_agent_plist(exe: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{MAC_LABEL}</string>
  <key>ProgramArguments</key>
  <array><string>{}</string><string>serve</string><string>--background</string></array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict>
  <key>ProcessType</key><string>Background</string>
</dict>
</plist>
"#,
        xml_escape(exe)
    )
}

/// Quote a path for the Exec= line of a .desktop file (spaces and the characters the spec reserves).
#[cfg(unix)]
fn desktop_exec_quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            out.push('\\');
        }
        // A literal % must be doubled in Exec.
        if c == '%' {
            out.push('%');
        }
        out.push(c);
    }
    out.push('"');
    out
}

/// XDG autostart entry (works in GNOME, KDE, XFCE, ...).
#[cfg(unix)]
pub fn xdg_autostart_entry(exe: &str) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=Phone Remote\nComment=Control this computer's media from your phone\nExec={} serve --background\nTerminal=false\nX-GNOME-Autostart-enabled=true\n",
        desktop_exec_quote(exe)
    )
}

#[cfg(unix)]
fn autostart_path() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        dirs::home_dir().map(|h| h.join("Library/LaunchAgents").join(format!("{MAC_LABEL}.plist")))
    } else {
        dirs::config_dir().map(|c| c.join("autostart/phone-remote.desktop"))
    }
}

#[cfg(unix)]
fn autostart_contents(exe: &str) -> String {
    if cfg!(target_os = "macos") {
        launch_agent_plist(exe)
    } else {
        xdg_autostart_entry(exe)
    }
}

#[cfg(unix)]
fn set_autostart_at(path: &Path, contents: &str, on: bool) -> Result<()> {
    if on {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        // Write then rename so a crash never leaves a half-written file the OS would choke on.
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, contents)?;
        std::fs::rename(&tmp, path)?;
    } else if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

#[cfg(unix)]
pub fn autostart_enabled() -> bool {
    autostart_path().map(|p| p.exists()).unwrap_or(false)
}

#[cfg(unix)]
pub fn set_autostart(on: bool) -> Result<()> {
    let path = autostart_path().ok_or_else(|| anyhow!("cannot find the login-items folder"))?;
    let exe = std::env::current_exe()?;
    let exe = exe.to_str().ok_or_else(|| anyhow!("the program path is not valid UTF-8"))?;
    set_autostart_at(&path, &autostart_contents(exe), on)?;
    #[cfg(target_os = "macos")]
    if !on {
        // Stop the running job too; the file is already gone so it will not come back at login.
        let _ = std::process::Command::new("launchctl").args(["remove", MAC_LABEL]).status();
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    #[test]
    fn plist_has_the_job_and_escapes_the_path() {
        let p = launch_agent_plist("/Applications/Phone & Remote/phone-remote");
        assert!(p.contains("<string>/Applications/Phone &amp; Remote/phone-remote</string>"));
        assert!(p.contains("<string>serve</string><string>--background</string>"));
        assert!(p.contains("<key>RunAtLoad</key><true/>"));
        assert!(p.contains("<key>SuccessfulExit</key><false/>"));
        assert!(!p.contains("Phone & Remote"), "raw ampersand would break the XML");
    }

    #[test]
    fn desktop_entry_quotes_the_path() {
        let e = xdg_autostart_entry("/home/me/My Apps/phone-remote");
        assert!(e.contains("Exec=\"/home/me/My Apps/phone-remote\" serve --background\n"));
        assert!(e.starts_with("[Desktop Entry]\nType=Application\n"));
        assert_eq!(desktop_exec_quote("a$b`c\"d\\e%f"), "\"a\\$b\\`c\\\"d\\\\e%%f\"");
    }

    #[test]
    fn autostart_file_is_written_and_removed() {
        let dir = std::env::temp_dir().join(format!("pr-autostart-{}", std::process::id()));
        let path = dir.join("nested/phone-remote.desktop");
        set_autostart_at(&path, "x", true).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "x");
        assert!(!path.with_extension("tmp").exists());
        set_autostart_at(&path, "x", false).unwrap();
        assert!(!path.exists());
        set_autostart_at(&path, "x", false).unwrap(); // removing twice is fine
        let _ = std::fs::remove_dir_all(dir);
    }

    fn serve_once(reply: &'static str) -> u16 {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = l.accept() {
                let mut b = [0u8; 256];
                let _ = s.read(&mut b);
                let _ = s.write_all(reply.as_bytes());
            }
        });
        port
    }

    #[test]
    fn agent_detection() {
        assert!(agent_running(serve_once("HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\nok")));
        assert!(!agent_running(serve_once("HTTP/1.1 500 Internal Server Error\r\n\r\n")));
        let closed = { TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port() };
        assert!(!agent_running(closed));
    }
}
