//! One-click configuration that switches on each player's remote-control interface.
//! The INI editing is pure (and unit-tested); `apply` touches the real config files.

/// Set `key=value` inside `[section]` of an INI-style file, uncommenting a `#key=` default if present.
pub fn set_ini_key(text: &str, section: &str, key: &str, value: &str) -> String {
    let crlf = text.contains("\r\n");
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let header = format!("[{section}]");
    let new_line = format!("{key}={value}");
    let prefix = format!("{key}=");

    if let Some(h) = lines.iter().position(|l| l.trim() == header) {
        let end = lines[h + 1..].iter().position(|l| l.trim_start().starts_with('[')).map_or(lines.len(), |i| h + 1 + i);
        let live = (h + 1..end).find(|&i| lines[i].trim_start().starts_with(&prefix));
        let commented = (h + 1..end).find(|&i| lines[i].trim_start().trim_start_matches('#').trim_start().starts_with(&prefix));
        match live.or(commented) {
            Some(i) => lines[i] = new_line,
            None => lines.insert(h + 1, new_line),
        }
    } else {
        if lines.last().is_some_and(|l| !l.trim().is_empty()) {
            lines.push(String::new());
        }
        lines.push(header);
        lines.push(new_line);
    }
    let sep = if crlf { "\r\n" } else { "\n" };
    lines.join(sep) + sep
}

pub fn patch_vlcrc(text: &str, password: &str) -> String {
    let t = set_ini_key(text, "core", "extraintf", "http");
    let t = set_ini_key(&t, "lua", "http-password", password);
    let t = set_ini_key(&t, "lua", "http-host", "127.0.0.1");
    set_ini_key(&t, "lua", "http-port", "8080")
}

/// Append a line to a conf file unless a line with the same key is already there.
pub fn ensure_conf_line(text: &str, key: &str, value: &str) -> String {
    let prefix = format!("{key}=");
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    match lines.iter().position(|l| l.trim_start().starts_with(&prefix)) {
        Some(i) => lines[i] = format!("{key}={value}"),
        None => lines.push(format!("{key}={value}")),
    }
    lines.join("\n") + "\n"
}

#[cfg(windows)]
pub fn apply(vlc_password: &str) -> Vec<String> {
    use std::os::windows::process::CommandExt;
    use std::path::PathBuf;
    let mut report = vec![];
    let running = |exe: &str| {
        std::process::Command::new("tasklist")
            .args(["/FI", &format!("IMAGENAME eq {exe}"), "/NH"])
            .creation_flags(0x0800_0000)
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_lowercase().contains(exe))
            .unwrap_or(false)
    };
    let appdata = std::env::var_os("APPDATA").map(PathBuf::from);

    // VLC
    match &appdata {
        Some(dir) => {
            let rc = dir.join("vlc").join("vlcrc");
            let old = std::fs::read_to_string(&rc).unwrap_or_default();
            let res = std::fs::create_dir_all(rc.parent().unwrap()).and_then(|_| std::fs::write(&rc, patch_vlcrc(&old, vlc_password)));
            report.push(match res {
                Ok(()) if running("vlc.exe") => "VLC: configured. Close VLC completely and open it again.".to_string(),
                Ok(()) => "VLC: configured. It takes effect the next time you open VLC.".to_string(),
                Err(e) => format!("VLC: could not write settings ({e})."),
            });
        }
        None => report.push("VLC: could not find your settings folder.".into()),
    }
    // mpv
    if let Some(dir) = &appdata {
        let conf = dir.join("mpv").join("mpv.conf");
        let old = std::fs::read_to_string(&conf).unwrap_or_default();
        let res = std::fs::create_dir_all(conf.parent().unwrap())
            .and_then(|_| std::fs::write(&conf, ensure_conf_line(&old, "input-ipc-server", super::mpv::PIPE_NAME)));
        report.push(match res {
            Ok(()) if running("mpv.exe") => "mpv: configured. Close mpv and open it again.".to_string(),
            Ok(()) => "mpv: configured. It takes effect the next time you open mpv.".to_string(),
            Err(e) => format!("mpv: could not write settings ({e})."),
        });
    }
    // MPC-HC / MPC-BE (best effort: settings live in the registry unless the player is portable)
    let mut mpc_ok = false;
    for key in [r"HKCU\Software\MPC-HC\MPC-HC\Settings", r"HKCU\Software\MPC-BE\Settings"] {
        let exists = std::process::Command::new("reg").args(["query", key]).creation_flags(0x0800_0000).output().map(|o| o.status.success()).unwrap_or(false);
        if exists {
            for (name, val) in [("EnableWebServer", "1"), ("WebServerPort", "13579")] {
                let _ = std::process::Command::new("reg")
                    .args(["add", key, "/v", name, "/t", "REG_DWORD", "/d", val, "/f"])
                    .creation_flags(0x0800_0000)
                    .status();
            }
            mpc_ok = true;
        }
    }
    report.push(if mpc_ok {
        "MPC-HC/BE: web interface switched on. Close and reopen the player. If there is no progress, enable it in Options > Player > Web Interface.".into()
    } else {
        "MPC-HC/BE: not found. If you use it, enable Options > Player > Web Interface.".to_string()
    });
    report
}

#[cfg(not(windows))]
pub fn apply(_: &str) -> Vec<String> {
    vec!["Player setup is only available on Windows.".into()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uncomments_default_and_keeps_others() {
        let rc = "[core]\n# Extra interface modules (string)\n#extraintf=\nfoo=1\n[lua]\n#http-password=\n";
        let out = patch_vlcrc(rc, "pw");
        assert!(out.contains("\nextraintf=http\n"));
        assert!(out.contains("http-password=pw"));
        assert!(out.contains("foo=1"));
        assert!(!out.contains("#extraintf"));
    }

    #[test]
    fn creates_missing_sections_and_is_idempotent() {
        let once = patch_vlcrc("", "pw");
        assert!(once.contains("[core]\nextraintf=http"));
        assert!(once.contains("[lua]"));
        assert_eq!(patch_vlcrc(&once, "pw"), once);
    }

    #[test]
    fn replaces_existing_value_and_preserves_crlf() {
        let out = set_ini_key("[lua]\r\nhttp-password=old\r\n", "lua", "http-password", "new");
        assert_eq!(out, "[lua]\r\nhttp-password=new\r\n");
    }

    #[test]
    fn conf_line_added_once() {
        let a = ensure_conf_line("volume=50\n", "input-ipc-server", "x");
        assert_eq!(ensure_conf_line(&a, "input-ipc-server", "x"), a);
        assert!(a.contains("volume=50") && a.ends_with("input-ipc-server=x\n"));
    }
}
