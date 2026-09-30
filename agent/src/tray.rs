//! System tray icon and the few native dialogs. Everything else lives in the dashboard window.

use crate::config::{self, Config};
use anyhow::Result;
use std::sync::Mutex;
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, TrayIconBuilder, TrayIconEvent};
use windows::Win32::UI::WindowsAndMessaging::{DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE, WM_QUIT};

const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_NAME: &str = "PhoneRemote";

static TOOLTIP: Mutex<String> = Mutex::new(String::new());

/// Latest tooltip text; the tray thread applies it on its next tick.
pub fn set_tooltip(text: &str) {
    if let Ok(mut t) = TOOLTIP.lock() {
        if *t != text {
            *t = text.to_string();
        }
    }
}

fn reg(args: &[&str]) -> bool {
    use std::os::windows::process::CommandExt;
    std::process::Command::new("reg")
        .args(args)
        .creation_flags(0x0800_0000)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn autostart_enabled() -> bool {
    reg(&["query", RUN_KEY, "/v", RUN_NAME])
}

pub fn set_autostart(on: bool) {
    if on {
        if let Ok(exe) = std::env::current_exe() {
            let value = format!("\"{}\" --background", exe.display());
            reg(&["add", RUN_KEY, "/v", RUN_NAME, "/t", "REG_SZ", "/d", &value, "/f"]);
        }
    } else {
        reg(&["delete", RUN_KEY, "/v", RUN_NAME, "/f"]);
    }
}

/// 32x32 amber disc with a dark play triangle (matches the app's accent).
fn icon() -> Icon {
    const N: i32 = 32;
    let mut px = Vec::with_capacity((N * N * 4) as usize);
    for y in 0..N {
        for x in 0..N {
            let (dx, dy) = (x as f32 - 15.5, y as f32 - 15.5);
            let inside_disc = dx * dx + dy * dy <= 15.0 * 15.0;
            // Triangle with vertices (11,8) (11,24) (24,16).
            let t = x >= 11 && (y as f32 - 16.0).abs() <= (24.0 - x as f32) * (8.0 / 13.0);
            let c = if !inside_disc { [0, 0, 0, 0] } else if t { [27, 16, 6, 255] } else { [255, 154, 60, 255] };
            px.extend_from_slice(&c);
        }
    }
    Icon::from_rgba(px, N as u32, N as u32).expect("valid icon")
}

pub fn info(title: &str, msg: &str) {
    use windows::core::HSTRING;
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONINFORMATION, MB_OK};
    unsafe { MessageBoxW(None, &HSTRING::from(msg), &HSTRING::from(title), MB_OK | MB_ICONINFORMATION) };
}

/// Keep the background agent out of Windows 11 "efficiency mode": throttled timers would make
/// it look frozen to the phone.
pub fn opt_out_of_throttling() {
    use windows::Win32::System::Threading::{
        GetCurrentProcess, ProcessPowerThrottling, SetProcessInformation, PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        PROCESS_POWER_THROTTLING_EXECUTION_SPEED, PROCESS_POWER_THROTTLING_STATE,
    };
    let state = PROCESS_POWER_THROTTLING_STATE {
        Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
        StateMask: 0, // 0 = do not throttle
    };
    let _ = unsafe {
        SetProcessInformation(
            GetCurrentProcess(),
            ProcessPowerThrottling,
            &state as *const _ as *const core::ffi::c_void,
            std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
        )
    };
}

/// Modal Allow/Deny question for a phone that scanned the QR. Call from a worker thread.
pub fn ask_allow(name: &str, ip: &str) -> bool {
    use windows::core::HSTRING;
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, IDYES, MB_ICONQUESTION, MB_SETFOREGROUND, MB_TOPMOST, MB_YESNO};
    let msg = format!("\"{name}\" ({ip}) wants to control this PC's media, mouse and keyboard.\n\nAllow it?");
    unsafe {
        MessageBoxW(None, &HSTRING::from(msg), &HSTRING::from("Phone Remote: new phone"), MB_YESNO | MB_ICONQUESTION | MB_TOPMOST | MB_SETFOREGROUND) == IDYES
    }
}

/// Open the dashboard as a frameless app window (Edge, then Chrome), else the default browser.
pub fn open_dashboard(url: &str) {
    use std::os::windows::process::CommandExt;
    let pf = std::env::var("ProgramFiles").unwrap_or_default();
    let pf86 = std::env::var("ProgramFiles(x86)").unwrap_or_default();
    let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
    let candidates = [
        format!(r"{pf86}\Microsoft\Edge\Application\msedge.exe"),
        format!(r"{pf}\Microsoft\Edge\Application\msedge.exe"),
        format!(r"{pf}\Google\Chrome\Application\chrome.exe"),
        format!(r"{pf86}\Google\Chrome\Application\chrome.exe"),
        format!(r"{local}\Google\Chrome\Application\chrome.exe"),
    ];
    for exe in candidates.iter().filter(|p| std::path::Path::new(p).exists()) {
        if std::process::Command::new(exe)
            .arg(format!("--app={url}"))
            .arg("--window-size=1040,760")
            .creation_flags(0x0800_0000)
            .spawn()
            .is_ok()
        {
            return;
        }
    }
    crate::open_url(url);
}

pub fn run(mut cfg: Config, dashboard: String) -> Result<()> {
    opt_out_of_throttling();
    // First run: start with Windows by default; the tray checkbox decides from then on.
    if !cfg.autostart_initialized {
        set_autostart(true);
        cfg.autostart_initialized = true;
        let _ = config::save(&cfg);
    }

    let menu = Menu::new();
    let open = MenuItem::new("Open Phone Remote", true, None);
    let add = MenuItem::new("Add a phone", true, None);
    let logm = MenuItem::new("Open log folder", true, None);
    let auto = CheckMenuItem::new("Start with Windows", true, autostart_enabled(), None);
    let quit = MenuItem::new("Quit Phone Remote", true, None);
    menu.append_items(&[&open, &add, &logm, &PredefinedMenuItem::separator(), &auto, &PredefinedMenuItem::separator(), &quit])?;
    let (open_id, add_id, log_id, auto_id, quit_id) = (open.id().clone(), add.id().clone(), logm.id().clone(), auto.id().clone(), quit.id().clone());
    let _keep = (&open, &add, &logm, &auto, &quit); // menu items must outlive the tray on this thread

    let (d1, d2, d3) = (dashboard.clone(), dashboard.clone(), dashboard.clone());
    // These closures run inside the tray window's procedure: a panic must never unwind out of it.
    MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if e.id == open_id {
                open_dashboard(&d1);
            } else if e.id == add_id {
                open_dashboard(&format!("{d2}#add"));
            } else if e.id == log_id {
                crate::open_url(&crate::log::dir().display().to_string());
            } else if e.id == auto_id {
                // The checkbox flips itself natively; make the registry match the new state.
                set_autostart(!autostart_enabled());
            } else if e.id == quit_id {
                crate::log::log("quit requested from tray");
                crate::log::clear_marker();
                std::process::exit(crate::guardian::EXIT_QUIT);
            }
        }));
    }));
    TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if let TrayIconEvent::DoubleClick { button: MouseButton::Left, .. } = e {
                open_dashboard(&d3);
            }
        }));
    }));

    // Right after sign-in Explorer's notification area may not exist yet. Keep retrying rather than
    // giving up: the server must stay up even if the icon takes a while to appear.
    let tray = loop {
        match TrayIconBuilder::new().with_menu(Box::new(menu.clone())).with_tooltip("Phone Remote").with_icon(icon()).build() {
            Ok(t) => break t,
            Err(e) => {
                crate::log::log(&format!("tray icon not ready ({e}); retrying"));
                std::thread::sleep(std::time::Duration::from_secs(3));
            }
        }
    };

    // The tray's hidden window needs this thread's message loop; menu callbacks fire from it.
    // Poll (rather than block) so the tooltip can follow the server's state.
    let mut shown = String::new();
    'pump: loop {
        unsafe {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT {
                    break 'pump;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        let want = TOOLTIP.lock().map(|t| t.clone()).unwrap_or_default();
        if !want.is_empty() && want != shown {
            let _ = tray.set_tooltip(Some(want.clone()));
            shown = want;
        }
        std::thread::sleep(std::time::Duration::from_millis(40));
    }
    Ok(())
}
