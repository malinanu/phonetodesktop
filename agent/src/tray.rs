//! System tray icon: keeps the agent alive after the terminal is closed.

use crate::config::{self, Config};
use anyhow::Result;
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, TrayIconBuilder, TrayIconEvent};
use windows::Win32::UI::WindowsAndMessaging::{DispatchMessageW, GetMessageW, TranslateMessage, MSG};

const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_NAME: &str = "PhoneRemote";

fn reg(args: &[&str]) -> bool {
    use std::os::windows::process::CommandExt;
    std::process::Command::new("reg")
        .args(args)
        .creation_flags(0x0800_0000)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn autostart_enabled() -> bool {
    reg(&["query", RUN_KEY, "/v", RUN_NAME])
}

fn set_autostart(on: bool) {
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

pub fn run(mut cfg: Config, pair_page: String) -> Result<()> {
    // First run: start with Windows by default; the tray checkbox decides from then on.
    if !cfg.autostart_initialized {
        set_autostart(true);
        cfg.autostart_initialized = true;
        let _ = config::save(&cfg);
    }

    let menu = Menu::new();
    let pair = MenuItem::new("Pair a phone (show QR code)", true, None);
    let guide = MenuItem::new("Guide: how it works and connecting", true, None);
    let setup = MenuItem::new("Set up video players (VLC, mpv, MPC-HC)", true, None);
    let logm = MenuItem::new("Open log folder", true, None);
    let diag = MenuItem::new("Diagnostics (what is detected)", true, None);
    let auto = CheckMenuItem::new("Start with Windows", true, autostart_enabled(), None);
    let quit = MenuItem::new("Quit Phone Remote", true, None);
    menu.append_items(&[&pair, &setup, &guide, &diag, &logm, &PredefinedMenuItem::separator(), &auto, &PredefinedMenuItem::separator(), &quit])?;
    let (pair_id, setup_id, guide_id, diag_id, log_id, auto_id, quit_id) = (pair.id().clone(), setup.id().clone(), guide.id().clone(), diag.id().clone(), logm.id().clone(), auto.id().clone(), quit.id().clone());
    let _keep = (&pair, &setup, &guide, &diag, &logm, &auto, &quit); // menu items must outlive the tray on this thread
    let vlc_pw = crate::backend::vlc_password(&cfg.local_secret);

    let page = pair_page.clone();
    MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
        if e.id == pair_id {
            crate::open_url(&page);
        } else if e.id == setup_id {
            let pw = vlc_pw.clone();
            std::thread::spawn(move || {
                let report = crate::backend::setup::apply(&pw).join("\n\n");
                info("Set up video players", &format!("{report}\n\nRestart each player once, then play something."));
            });
        } else if e.id == guide_id {
            crate::open_url(&page.replace("/pair", "/guide"));
        } else if e.id == diag_id {
            crate::open_url(&page.replace("/pair", "/debug"));
        } else if e.id == log_id {
            crate::open_url(&crate::log::dir().display().to_string());
        } else if e.id == auto_id {
            // The checkbox flips itself natively; make the registry match the new state.
            set_autostart(!autostart_enabled());
        } else if e.id == quit_id {
            crate::log::log("quit requested from tray");
            std::process::exit(0);
        }
    }));
    TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
        if let TrayIconEvent::DoubleClick { button: MouseButton::Left, .. } = e {
            crate::open_url(&pair_page);
        }
    }));

    let _tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("Phone Remote — running")
        .with_icon(icon())
        .build()?;

    // The tray's hidden window needs this thread's message loop; menu callbacks fire from it.
    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}
