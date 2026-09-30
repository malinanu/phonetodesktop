//! Windows backend: GSMTC (Windows.Media.Control) for sessions, SendInput for media keys,
//! and arrow-key injection into the foreground window as the last-resort seek.

use super::{keys, mpc, mpv, vlc, Backend, ButtonAction, Input, Key, MouseButton, Transport};
use crate::protocol::PlayerInfo;
use anyhow::{anyhow, Result};
use std::time::{SystemTime, UNIX_EPOCH};
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session,
    GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};
use windows::Win32::Foundation::{CloseHandle, HWND};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_EXTENDEDKEY,
    KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, MOUSEEVENTF_HWHEEL, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN,
    MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_WHEEL, MOUSE_EVENT_FLAGS, MOUSEINPUT,
    INPUT_MOUSE, VIRTUAL_KEY, VK_LEFT, VK_MEDIA_NEXT_TRACK, VK_MEDIA_PLAY_PAUSE,
    VK_MEDIA_PREV_TRACK, VK_RIGHT, VK_VOLUME_DOWN, VK_VOLUME_MUTE, VK_VOLUME_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

/// 100 ns ticks between 1601-01-01 (WinRT DateTime epoch) and 1970-01-01.
const UNIX_TO_WINRT_TICKS: i64 = 116_444_736_000_000_000;
const TICKS_PER_MS: i64 = 10_000;

/// Foreground executables we are willing to send arrow keys to, and the seconds one
/// Left/Right press skips. Defaults from each player's documentation; adjust in one place.
const KEY_SEEK_TABLE: &[(&str, i64)] = &[
    ("vlc.exe", 10),
    ("mpv.exe", 5),
    ("mpc-hc64.exe", 5),
    ("mpc-hc.exe", 5),
    ("mpc-be64.exe", 5),
    ("potplayermini64.exe", 5),
    ("chrome.exe", 5),
    ("msedge.exe", 5),
    ("firefox.exe", 5),
    ("brave.exe", 5),
    ("opera.exe", 5),
    ("vivaldi.exe", 5),
];

pub struct WindowsBackend {
    pub vlc_password: String,
}

thread_local! {
    /// COM/WinRT is set up once per thread (pool threads live long), never per call.
    static COM_READY: () = {
        // S_FALSE / RPC_E_CHANGED_MODE just mean the thread is already set up.
        let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
    };
}

fn init_winrt() {
    COM_READY.with(|_| {});
}

fn manager() -> Result<Manager> {
    init_winrt();
    Ok(Manager::RequestAsync()?.get()?)
}

fn find_session(id: &str) -> Result<Session> {
    let mgr = manager()?;
    for s in mgr.GetSessions()? {
        if s.SourceAppUserModelId()?.to_string_lossy() == id {
            return Ok(s);
        }
    }
    Err(anyhow!("session {id} is gone"))
}

fn friendly_app(id: &str) -> String {
    let base = id.rsplit('!').next().unwrap_or(id);
    base.trim_end_matches(".exe").to_string()
}

fn now_winrt_ticks() -> i64 {
    let d = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    d.as_nanos() as i64 / 100 + UNIX_TO_WINRT_TICKS
}

/// Read everything we can about a session. A failing property degrades that field only;
/// the session itself is never dropped (errors are collected for /debug).
fn describe(s: &Session, errs: &mut Vec<String>) -> Option<PlayerInfo> {
    let id = match s.SourceAppUserModelId() {
        Ok(h) => h.to_string_lossy(),
        Err(e) => {
            errs.push(format!("SourceAppUserModelId: {e}"));
            return None;
        }
    };
    let mut note = |what: &str, e: windows::core::Error| errs.push(format!("{id}: {what}: {e}"));

    let (mut playing, mut rate, mut pos_enabled) = (false, 1.0f64, false);
    match s.GetPlaybackInfo() {
        Ok(info) => {
            match info.PlaybackStatus() {
                Ok(st) => playing = st == Status::Playing,
                Err(e) => note("PlaybackStatus", e),
            }
            rate = info.PlaybackRate().and_then(|r| r.Value()).unwrap_or(1.0).clamp(0.0, 8.0);
            pos_enabled = info.Controls().and_then(|c| c.IsPlaybackPositionEnabled()).unwrap_or(false);
        }
        Err(e) => note("GetPlaybackInfo", e),
    }

    let (mut title, mut artist) = (String::new(), String::new());
    match s.TryGetMediaPropertiesAsync().and_then(|op| op.get()) {
        Ok(p) => {
            title = p.Title().map(|t| t.to_string_lossy()).unwrap_or_default();
            artist = p.Artist().map(|t| t.to_string_lossy()).unwrap_or_default();
        }
        Err(e) => note("MediaProperties", e),
    }

    let (mut pos_ms, mut dur_ms, mut can_seek) = (0, 0, false);
    let timeline = s.GetTimelineProperties().and_then(|t| {
        Ok((t.StartTime()?.Duration, t.EndTime()?.Duration, t.Position()?.Duration, t.LastUpdatedTime()?.UniversalTime))
    });
    match timeline {
        Ok((start, end, mut pos, updated)) => {
            if playing && updated > 0 {
                let age = now_winrt_ticks() - updated;
                // Ignore absurd ages (clock skew, apps that never refresh the stamp).
                if (0..86_400 * 10_000_000i64).contains(&age) {
                    pos += (age as f64 * rate) as i64;
                }
            }
            dur_ms = (end - start).max(0) / TICKS_PER_MS;
            pos_ms = (pos - start).clamp(0, (end - start).max(0)) / TICKS_PER_MS;
            can_seek = end > start && pos_enabled;
        }
        Err(e) => note("Timeline", e),
    }
    Some(PlayerInfo { app: friendly_app(&id), id, title, artist, playing, pos_ms, dur_ms, can_seek })
}

/// Apps currently producing audio on the default output, by executable stem (lowercase).
/// Catches players that register no media session (VLC 3, mpv, many games).
fn active_audio_apps(errs: &mut Vec<String>) -> Vec<(String, u32)> {
    match audio_apps() {
        Ok(v) => v,
        Err(e) => {
            errs.push(format!("audio sessions: {e}"));
            vec![]
        }
    }
}

fn audio_apps() -> Result<Vec<(String, u32)>> {
    use windows::core::Interface;
    use windows::Win32::Media::Audio::{
        eMultimedia, eRender, AudioSessionStateActive, IAudioSessionControl2, IAudioSessionManager2, IMMDeviceEnumerator,
        MMDeviceEnumerator,
    };
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
    let mut out = vec![];
    unsafe {
        init_winrt();
        let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let dev = en.GetDefaultAudioEndpoint(eRender, eMultimedia)?;
        let mgr: IAudioSessionManager2 = dev.Activate(CLSCTX_ALL, None)?;
        let list = mgr.GetSessionEnumerator()?;
        for i in 0..list.GetCount()? {
            let ctl = list.GetSession(i)?;
            if ctl.GetState()? != AudioSessionStateActive {
                continue;
            }
            let pid = ctl.cast::<IAudioSessionControl2>()?.GetProcessId()?;
            if pid == 0 {
                continue; // system sounds
            }
            if let Some(exe) = exe_of_pid(pid) {
                let stem = exe.trim_end_matches(".exe").to_lowercase();
                if !out.iter().any(|(s, _)| *s == stem) {
                    out.push((stem, pid));
                }
            }
        }
    }
    Ok(out)
}

/// Longest visible top-level window title of a process (players put the file name there).
fn window_title(pid: u32) -> Option<String> {
    use windows::core::BOOL;
    use windows::Win32::Foundation::LPARAM;
    use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowTextW, IsWindowVisible};
    unsafe extern "system" fn cb(hwnd: HWND, lp: LPARAM) -> BOOL {
        // A panic must not unwind through the Windows callback boundary.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe { visit(hwnd, lp) }));
        BOOL(1)
    }
    unsafe fn visit(hwnd: HWND, lp: LPARAM) {
        let data = unsafe { &mut *(lp.0 as *mut (u32, String)) };
        let mut wpid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut wpid)) };
        if wpid == data.0 && unsafe { IsWindowVisible(hwnd) }.as_bool() {
            let mut buf = [0u16; 512];
            let n = unsafe { GetWindowTextW(hwnd, &mut buf) };
            if n > 0 {
                let t = String::from_utf16_lossy(&buf[..n as usize]);
                if t.len() > data.1.len() {
                    data.1 = t;
                }
            }
        }
    }
    let mut data = (pid, String::new());
    let _ = unsafe { EnumWindows(Some(cb), LPARAM(&mut data as *mut _ as isize)) };
    (!data.1.is_empty()).then_some(data.1)
}

/// "Movie.mkv - VLC media player" -> "Movie.mkv"
fn clean_title(t: &str) -> String {
    const PLAYERS: &[&str] = &["vlc", "mpc", "mpv", "potplayer", "media player", "kmplayer", "winamp", "foobar"];
    match t.rsplit_once(" - ") {
        Some((head, tail)) if PLAYERS.iter().any(|p| tail.to_lowercase().contains(p)) => head.to_string(),
        _ => t.to_string(),
    }
}

const MPC_ID: &str = "mpc:webif";
const VLC_ID: &str = "vlc:http";
const MPV_ID: &str = "mpv:ipc";

fn state_label(playing: bool, stopped: bool) -> &'static str {
    if playing {
        "Playing"
    } else if stopped {
        "Stopped"
    } else {
        "Paused"
    }
}

/// Master volume of the default output device.
fn endpoint_volume() -> Result<windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume> {
    use windows::Win32::Media::Audio::{eMultimedia, eRender, Endpoints::IAudioEndpointVolume, IMMDeviceEnumerator, MMDeviceEnumerator};
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
    init_winrt();
    unsafe {
        let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let dev = en.GetDefaultAudioEndpoint(eRender, eMultimedia)?;
        Ok(dev.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None)?)
    }
}

const IGNORED_AUDIO: &[&str] = &["phone-remote", "audiodg", "system", "svchost", "explorer", "applicationframehost"];

fn collect(errs: &mut Vec<String>, vlc_pw: &str) -> Result<Vec<PlayerInfo>> {
    let mgr = manager()?;
    let current = mgr
        .GetCurrentSession()
        .ok()
        .and_then(|s| s.SourceAppUserModelId().ok())
        .map(|h| h.to_string_lossy());
    let mut out: Vec<PlayerInfo> = mgr.GetSessions()?.into_iter().filter_map(|s| describe(&s, errs)).collect();
    if let Some(cur) = current {
        if let Some(i) = out.iter().position(|p| p.id == cur) {
            let p = out.remove(i);
            out.insert(0, p);
        }
    }
    // Nothing with a media session is playing: surface apps that are making sound anyway.
    if !out.iter().any(|p| p.playing) {
        for (app, pid) in active_audio_apps(errs) {
            if IGNORED_AUDIO.contains(&app.as_str()) {
                continue;
            }
            let title = window_title(pid).map(|t| clean_title(&t)).unwrap_or_else(|| "Audio is playing".into());
            let hint = if app.starts_with("mpc-") || app == "vlc" || app == "mpv" {
                "Tray icon > Set up video players to get progress and seek"
            } else {
                "This app shares no progress information"
            };
            out.push(PlayerInfo {
                id: format!("audio:{app}"),
                app: app.clone(),
                title,
                artist: hint.into(),
                playing: true,
                pos_ms: 0,
                dur_ms: 0,
                can_seek: false,
            });
        }
    }
    // Players with a remote-control interface switched on: exact timeline, visible even while paused.
    let mut enhanced: Vec<PlayerInfo> = Vec::new();
    if let Some(st) = mpc::status(mpc::DEFAULT_PORT) {
        out.retain(|p| !p.id.starts_with("audio:mpc-"));
        enhanced.push(PlayerInfo {
            id: MPC_ID.into(),
            app: "MPC-HC".into(),
            title: if st.file.is_empty() { "MPC-HC".into() } else { st.file },
            artist: state_label(st.state == 2, st.state == 0).into(),
            playing: st.state == 2,
            pos_ms: st.pos_ms,
            dur_ms: st.dur_ms,
            can_seek: st.dur_ms > 0,
        });
    }
    if let Some(st) = vlc::status(vlc_pw) {
        out.retain(|p| p.id != "audio:vlc");
        enhanced.push(PlayerInfo {
            id: VLC_ID.into(),
            app: "VLC".into(),
            title: st.title,
            artist: state_label(st.playing, st.stopped).into(),
            playing: st.playing,
            pos_ms: st.pos_ms,
            dur_ms: st.dur_ms,
            can_seek: st.dur_ms > 0,
        });
    }
    if let Some(st) = mpv::status() {
        out.retain(|p| p.id != "audio:mpv");
        enhanced.push(PlayerInfo {
            id: MPV_ID.into(),
            app: "mpv".into(),
            title: st.title,
            artist: state_label(st.playing, false).into(),
            playing: st.playing,
            pos_ms: st.pos_ms,
            dur_ms: st.dur_ms,
            can_seek: st.dur_ms > 0,
        });
    }
    // Playing ones lead; paused ones follow the OS sessions.
    let (playing, paused): (Vec<_>, Vec<_>) = enhanced.into_iter().partition(|p| p.playing);
    out.splice(0..0, playing);
    out.extend(paused);
    Ok(out)
}

impl Backend for WindowsBackend {
    fn name(&self) -> &'static str {
        "windows-gsmtc"
    }

    fn snapshot(&self) -> Result<Vec<PlayerInfo>> {
        collect(&mut Vec::new(), &self.vlc_password)
    }

    fn debug(&self) -> String {
        let mut errs = Vec::new();
        let snap = collect(&mut errs, &self.vlc_password);
        let mut s = String::from("Phone Remote diagnostics (windows-gsmtc)\n\n");
        match snap {
            Ok(p) if p.is_empty() => s.push_str("No sessions found. Windows reports nothing playing and no app is making sound.\n"),
            Ok(p) => p.iter().for_each(|p| s.push_str(&format!("{p:#?}\n"))),
            Err(e) => s.push_str(&format!("snapshot failed: {e:#}\n")),
        }
        s.push_str("\nErrors:\n");
        if errs.is_empty() {
            s.push_str("  none\n");
        }
        errs.iter().for_each(|e| s.push_str(&format!("  {e}\n")));
        s
    }

    fn transport(&self, id: &str, what: Transport) -> Result<bool> {
        if id == MPC_ID {
            return match what {
                Transport::PlayPause => mpc::play_pause(mpc::DEFAULT_PORT).map(|_| true),
                _ => Ok(false), // media key fallback
            };
        }
        if id == VLC_ID {
            let pw = &self.vlc_password;
            return match what {
                Transport::PlayPause => vlc::play_pause(pw, vlc::status(pw).is_some_and(|s| s.stopped)),
                Transport::Next => vlc::next(pw),
                Transport::Prev => vlc::prev(pw),
            }
            .map(|_| true);
        }
        if id == MPV_ID {
            return match what {
                Transport::PlayPause => mpv::play_pause(),
                Transport::Next => mpv::next(),
                Transport::Prev => mpv::prev(),
            }
            .map(|_| true);
        }
        if id.starts_with("audio:") {
            return Ok(false); // no session: the controller falls back to the system media key
        }
        let s = find_session(id)?;
        Ok(match what {
            Transport::PlayPause => s.TryTogglePlayPauseAsync()?.get()?,
            Transport::Next => s.TrySkipNextAsync()?.get()?,
            Transport::Prev => s.TrySkipPreviousAsync()?.get()?,
        })
    }

    fn seek_abs(&self, id: &str, pos_ms: i64) -> Result<bool> {
        if id == MPC_ID {
            return mpc::seek(mpc::DEFAULT_PORT, pos_ms).map(|_| true);
        }
        if id == VLC_ID {
            return vlc::seek(&self.vlc_password, pos_ms).map(|_| true);
        }
        if id == MPV_ID {
            return mpv::seek(pos_ms).map(|_| true);
        }
        if id.starts_with("audio:") {
            return Ok(false);
        }
        let s = find_session(id)?;
        let start = s.GetTimelineProperties()?.StartTime()?.Duration;
        // The API takes 100 ns ticks, not ms or seconds.
        Ok(s.TryChangePlaybackPositionAsync(start + pos_ms * TICKS_PER_MS)?.get()?)
    }

    fn media_key(&self, key: Key) -> Result<()> {
        let vk = match key {
            Key::PlayPause => VK_MEDIA_PLAY_PAUSE,
            Key::Next => VK_MEDIA_NEXT_TRACK,
            Key::Prev => VK_MEDIA_PREV_TRACK,
            Key::VolUp => VK_VOLUME_UP,
            Key::VolDown => VK_VOLUME_DOWN,
            Key::Mute => VK_VOLUME_MUTE,
        };
        tap(vk, true)
    }

    fn volume(&self) -> Option<(u8, bool)> {
        let ep = endpoint_volume().ok()?;
        unsafe {
            let level = ep.GetMasterVolumeLevelScalar().ok()?;
            let muted = ep.GetMute().ok()?.as_bool();
            Some(((level * 100.0).round().clamp(0.0, 100.0) as u8, muted))
        }
    }

    fn set_volume(&self, level: u8) -> Result<()> {
        let ep = endpoint_volume()?;
        unsafe {
            ep.SetMasterVolumeLevelScalar(level.min(100) as f32 / 100.0, std::ptr::null())?;
            // Raising the volume while muted should be audible.
            if level > 0 {
                ep.SetMute(false, std::ptr::null())?;
            }
        }
        Ok(())
    }

    fn set_mute(&self, muted: bool) -> Result<()> {
        unsafe { endpoint_volume()?.SetMute(muted, std::ptr::null())? };
        Ok(())
    }

    fn input(&self, input: Input) -> Result<()> {
        match input {
            Input::Move(dx, dy) => send(&[mouse(MOUSEEVENTF_MOVE, dx, dy, 0)]),
            Input::Button(b, action) => {
                let (down, up) = match b {
                    MouseButton::Left => (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP),
                    MouseButton::Right => (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP),
                    MouseButton::Middle => (MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP),
                };
                match action {
                    ButtonAction::Click => send(&[mouse(down, 0, 0, 0), mouse(up, 0, 0, 0)]),
                    ButtonAction::Down => send(&[mouse(down, 0, 0, 0)]),
                    ButtonAction::Up => send(&[mouse(up, 0, 0, 0)]),
                }
            }
            Input::Scroll(dx, dy) => {
                let mut ev = vec![];
                if dy != 0 {
                    ev.push(mouse(MOUSEEVENTF_WHEEL, 0, 0, dy));
                }
                if dx != 0 {
                    ev.push(mouse(MOUSEEVENTF_HWHEEL, 0, 0, dx));
                }
                if ev.is_empty() { Ok(()) } else { send(&ev) }
            }
            Input::Text(text) => type_text(&text),
            Input::Key { name, mods } => chord(&name, &mods),
        }
    }

    fn focused_seek(&self, secs: i64) -> Result<i64> {
        let Some(exe) = foreground_exe() else { return Ok(0) };
        let Some(&(_, step)) = KEY_SEEK_TABLE.iter().find(|(n, _)| exe.eq_ignore_ascii_case(n)) else {
            return Ok(0);
        };
        let presses = (secs.abs() + step - 1) / step;
        let vk = if secs >= 0 { VK_RIGHT } else { VK_LEFT };
        for _ in 0..presses {
            tap(vk, false)?;
            std::thread::sleep(std::time::Duration::from_millis(30));
        }
        Ok(secs.signum() * presses * step)
    }
}

fn mouse(flags: MOUSE_EVENT_FLAGS, dx: i32, dy: i32, data: i32) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 { mi: MOUSEINPUT { dx, dy, mouseData: data as u32, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
    }
}

fn send(inputs: &[INPUT]) -> Result<()> {
    let sent = unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent as usize != inputs.len() {
        return Err(anyhow!("Windows blocked the input (an elevated window or the lock screen has focus)"));
    }
    Ok(())
}

/// Type any Unicode text: each UTF-16 unit is sent as a Unicode key event, so no keyboard layout is involved.
fn type_text(text: &str) -> Result<()> {
    let mut ev = Vec::new();
    for unit in text.encode_utf16() {
        match unit {
            0x000A | 0x000D => {
                // Enter is a real key, not a Unicode character.
                ev.push(key_event(VIRTUAL_KEY(0x0D), KEYBD_EVENT_FLAGS(0)));
                ev.push(key_event(VIRTUAL_KEY(0x0D), KEYEVENTF_KEYUP));
            }
            0x0009 => {
                ev.push(key_event(VIRTUAL_KEY(0x09), KEYBD_EVENT_FLAGS(0)));
                ev.push(key_event(VIRTUAL_KEY(0x09), KEYEVENTF_KEYUP));
            }
            u => {
                for up in [false, true] {
                    ev.push(INPUT {
                        r#type: INPUT_KEYBOARD,
                        Anonymous: INPUT_0 {
                            ki: KEYBDINPUT {
                                wVk: VIRTUAL_KEY(0),
                                wScan: u,
                                dwFlags: KEYEVENTF_UNICODE | if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
                                time: 0,
                                dwExtraInfo: 0,
                            },
                        },
                    });
                }
            }
        }
    }
    if ev.is_empty() { Ok(()) } else { send(&ev) }
}

/// Press the modifiers, tap the key, release the modifiers in reverse order.
fn chord(name: &str, mods: &[String]) -> Result<()> {
    let (vk, ext) = keys::key(name).ok_or_else(|| anyhow!("unknown key {name}"))?;
    let ext_flag = if ext { KEYEVENTF_EXTENDEDKEY } else { KEYBD_EVENT_FLAGS(0) };
    let mvk: Vec<VIRTUAL_KEY> = mods.iter().filter_map(|m| keys::modifier(m)).map(VIRTUAL_KEY).collect();
    let mut ev: Vec<INPUT> = mvk.iter().map(|m| key_event(*m, KEYBD_EVENT_FLAGS(0))).collect();
    ev.push(key_event(VIRTUAL_KEY(vk), ext_flag));
    ev.push(key_event(VIRTUAL_KEY(vk), ext_flag | KEYEVENTF_KEYUP));
    ev.extend(mvk.iter().rev().map(|m| key_event(*m, KEYEVENTF_KEYUP)));
    send(&ev)
}

fn key_event(vk: VIRTUAL_KEY, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, wScan: 0, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
    }
}

/// Press and release one key. Fails when UIPI blocks injection into an elevated window.
fn tap(vk: VIRTUAL_KEY, extended: bool) -> Result<()> {
    let ext = if extended { KEYEVENTF_EXTENDEDKEY } else { KEYBD_EVENT_FLAGS(0) };
    let inputs = [key_event(vk, ext), key_event(vk, ext | KEYEVENTF_KEYUP)];
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent as usize != inputs.len() {
        return Err(anyhow!("SendInput injected {sent}/2 events (target may be elevated)"));
    }
    Ok(())
}

fn foreground_exe() -> Option<String> {
    unsafe {
        let hwnd: HWND = GetForegroundWindow();
        if hwnd.0.is_null() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        exe_of_pid(pid)
    }
}

fn exe_of_pid(pid: u32) -> Option<String> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, windows::core::PWSTR(buf.as_mut_ptr()), &mut len);
        let _ = CloseHandle(h);
        ok.ok()?;
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        path.rsplit(['\\', '/']).next().map(str::to_string)
    }
}
