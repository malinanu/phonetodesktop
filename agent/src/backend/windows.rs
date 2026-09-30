//! Windows backend: GSMTC (Windows.Media.Control) for sessions, SendInput for media keys,
//! and arrow-key injection into the foreground window as the last-resort seek.

use super::{Backend, Key, Transport};
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
    KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_LEFT, VK_MEDIA_NEXT_TRACK, VK_MEDIA_PLAY_PAUSE,
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

pub struct WindowsBackend;

fn init_winrt() {
    // Idempotent; S_FALSE / RPC_E_CHANGED_MODE just mean the thread is already set up.
    let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
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

fn describe(s: &Session) -> Result<PlayerInfo> {
    let id = s.SourceAppUserModelId()?.to_string_lossy();
    let info = s.GetPlaybackInfo()?;
    let playing = info.PlaybackStatus()? == Status::Playing;
    let rate = info
        .PlaybackRate()
        .and_then(|r| r.Value())
        .unwrap_or(1.0)
        .clamp(0.0, 8.0);
    let (title, artist) = match s.TryGetMediaPropertiesAsync().and_then(|op| op.get()) {
        Ok(p) => (
            p.Title().map(|t| t.to_string_lossy()).unwrap_or_default(),
            p.Artist().map(|t| t.to_string_lossy()).unwrap_or_default(),
        ),
        Err(_) => Default::default(),
    };

    let (mut pos_ms, mut dur_ms, mut can_seek) = (0, 0, false);
    if let Ok(t) = s.GetTimelineProperties() {
        let start = t.StartTime()?.Duration;
        let end = t.EndTime()?.Duration;
        let mut pos = t.Position()?.Duration;
        let updated = t.LastUpdatedTime()?.UniversalTime;
        if playing && updated > 0 {
            let age = now_winrt_ticks() - updated;
            // Ignore absurd ages (clock skew, apps that never refresh the stamp).
            if (0..86_400 * 10_000_000i64).contains(&age) {
                pos += (age as f64 * rate) as i64;
            }
        }
        dur_ms = (end - start) / TICKS_PER_MS;
        pos_ms = (pos - start).clamp(0, (end - start).max(0)) / TICKS_PER_MS;
        can_seek = end > start && info.Controls().and_then(|c| c.IsPlaybackPositionEnabled()).unwrap_or(false);
    }
    Ok(PlayerInfo { app: friendly_app(&id), id, title, artist, playing, pos_ms, dur_ms, can_seek })
}

impl Backend for WindowsBackend {
    fn name(&self) -> &'static str {
        "windows-gsmtc"
    }

    fn snapshot(&self) -> Result<Vec<PlayerInfo>> {
        let mgr = manager()?;
        let current = mgr
            .GetCurrentSession()
            .ok()
            .and_then(|s| s.SourceAppUserModelId().ok())
            .map(|h| h.to_string_lossy());
        let mut out: Vec<PlayerInfo> = mgr.GetSessions()?.into_iter().filter_map(|s| describe(&s).ok()).collect();
        if let Some(cur) = current {
            if let Some(i) = out.iter().position(|p| p.id == cur) {
                let p = out.remove(i);
                out.insert(0, p);
            }
        }
        Ok(out)
    }

    fn transport(&self, id: &str, what: Transport) -> Result<bool> {
        let s = find_session(id)?;
        Ok(match what {
            Transport::PlayPause => s.TryTogglePlayPauseAsync()?.get()?,
            Transport::Next => s.TrySkipNextAsync()?.get()?,
            Transport::Prev => s.TrySkipPreviousAsync()?.get()?,
        })
    }

    fn seek_abs(&self, id: &str, pos_ms: i64) -> Result<bool> {
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
