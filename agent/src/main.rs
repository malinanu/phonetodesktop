// On Windows the agent is a background app: no console window, a tray icon instead.
// CLI subcommands re-attach to the parent terminal so their output still shows.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod api;
mod auth;
mod backend;
mod config;
mod controller;
mod discovery;
mod log;
mod net;
mod protocol;
mod server;
#[cfg(windows)]
mod tray;

use anyhow::Result;
use std::{net::SocketAddr, sync::Arc};

fn usage() -> &'static str {
    "phone-remote [serve] [--console] [--background] [--port N] [--mock] [--no-mdns]\n\
     phone-remote pair      where to find the pairing QR\n\
     phone-remote rotate    unpair every phone\n\
     phone-remote setup-players   switch on VLC / mpv / MPC-HC remote interfaces (progress + seek)
     phone-remote install   (Windows) allow private-network firewall access (run as Administrator)\n\
     \n\
     Windows: without --console the agent runs in the system tray."
}

/// Print to the terminal even though the Windows build has no console of its own.
pub fn say(s: &str) {
    #[cfg(windows)]
    {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().write(true).open("CONOUT$") {
            let _ = writeln!(f, "{s}");
            return;
        }
    }
    println!("{s}");
}

#[cfg(windows)]
fn attach_console() {
    use windows::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
    let _ = unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
}

fn print_qr(url: &str) {
    if let Ok(code) = qrcode::QrCode::new(url.as_bytes()) {
        say(&code
            .render::<char>()
            .quiet_zone(true)
            .module_dimensions(2, 1)
            .dark_color('█')
            .light_color(' ')
            .build());
    }
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("serve");
    let flag = |f: &str| args.iter().any(|a| a == f);
    #[cfg(windows)]
    if flag("--console") || !matches!(cmd, "serve") && !cmd.starts_with("--") {
        attach_console();
    }
    let mut cfg = config::load_or_create()?;
    if let Some(i) = args.iter().position(|a| a == "--port") {
        if let Some(p) = args.get(i + 1).and_then(|p| p.parse().ok()) {
            cfg.port = p;
            config::save(&cfg)?;
        }
    }
    let ips = net::lan_addrs();


    match cmd {
        "-h" | "--help" | "help" => say(usage()),
        "rotate" => {
            // Unpair everything: remove all phones and invalidate the legacy shared secret.
            cfg.devices.clear();
            cfg.token = config::new_token();
            cfg.legacy_shared_auth = false;
            config::save(&cfg)?;
            say("All phones unpaired. Pair each one again from the dashboard (restart the agent first if it is running).");
        }
        "pair" => say(&format!("Open http://127.0.0.1:{}/pair on this PC (the agent must be running) and scan the code.", cfg.port)),
        "install" => install(&cfg)?,
        "setup-players" => backend::setup::apply(&backend::vlc_password(&cfg.local_secret)).iter().for_each(|l| say(l)),
        _ => run(cfg, ips, flag("--mock"), flag("--no-mdns"), flag("--background"), flag("--console") || cfg!(not(windows)))?,
    }
    Ok(())
}

fn run(cfg: config::Config, ips: Vec<std::net::Ipv4Addr>, mock: bool, no_mdns: bool, background: bool, console: bool) -> Result<()> {
    let pair_page = format!("http://127.0.0.1:{}/dashboard", cfg.port);
    // Bind here so a second launch can detect the running agent and just show the pairing page.
    let listener = match std::net::TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], cfg.port))) {
        Ok(l) => l,
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            say(&format!("Already running. Dashboard: {pair_page}"));
            if !background {
                open_url(&pair_page);
            }
            return Ok(());
        }
        Err(e) => return Err(e.into()),
    };
    listener.set_nonblocking(true)?;

    let (server_cfg, server_ips) = (cfg.clone(), ips.clone());

    #[cfg(windows)]
    if !console {
        log::install_panic_hook();
        server::set_pending_hook(Arc::new(|app, p| {
            // Ask on a separate thread so the tray keeps running while the dialog is open.
            std::thread::spawn(move || {
                let allow = tray::ask_allow(&p.name, &p.ip);
                app.auth.decide(&p.id, allow);
                app.poke();
            });
        }));
        log::log(&format!("agent {} starting (tray mode{})", env!("CARGO_PKG_VERSION"), if background { ", at login" } else { "" }));
        // The server thread never ends on its own: only the tray's Quit stops the process.
        std::thread::spawn(move || supervise(server_cfg, server_ips, listener, mock, no_mdns));
        if !background {
            tray::open_dashboard(&format!("{pair_page}#add"));
        }
        return tray::run(cfg, pair_page);
    }
    let _ = (background, &pair_page);
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(serve(server_cfg, server_ips, listener, mock, no_mdns, console))
}

/// Run the server forever, restarting it (and logging why) if it ever stops or panics.
#[cfg(windows)]
fn supervise(cfg: config::Config, ips: Vec<std::net::Ipv4Addr>, first: std::net::TcpListener, mock: bool, no_mdns: bool) {
    let mut listener = Some(first);
    loop {
        let l = match listener.take() {
            Some(l) => l,
            None => match std::net::TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], cfg.port))).and_then(|l| l.set_nonblocking(true).map(|_| l)) {
                Ok(l) => l,
                Err(e) => {
                    log::log(&format!("cannot bind port {}: {e}; retrying", cfg.port));
                    std::thread::sleep(std::time::Duration::from_secs(3));
                    continue;
                }
            },
        };
        log::log(&format!("server listening on port {}", cfg.port));
        let (c, i) = (cfg.clone(), ips.clone());
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .map_err(anyhow::Error::from)
                .and_then(|rt| rt.block_on(serve(c, i, l, mock, no_mdns, false)))
        }));
        log::log(&format!("server stopped unexpectedly: {res:?}; restarting in 2s"));
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
}

async fn serve(
    cfg: config::Config,
    ips: Vec<std::net::Ipv4Addr>,
    listener: std::net::TcpListener,
    mock: bool,
    no_mdns: bool,
    console: bool,
) -> Result<()> {
    let host = net::hostname();
    let backend: Arc<dyn backend::Backend> = Arc::from(backend::default_backend(mock, &cfg.local_secret));
    let controller = Arc::new(controller::Controller::new(backend.clone(), host.clone()));
    let app = server::App::new(controller, &cfg, host.clone(), ips.clone(), server::pending_hook());
    let urls = app.pair_urls();

    let _mdns = if no_mdns {
        None
    } else {
        match discovery::advertise(&host, cfg.port, &cfg.pc_id) {
            Ok(d) => Some(d),
            Err(e) => {
                eprintln!("mDNS advertising failed (QR pairing still works): {e}");
                None
            }
        }
    };

    if console {
        say(&format!("Backend: {}\n\nScan with your phone (same Wi-Fi):\n", backend.name()));
        print_qr(&urls[0]);
        say(&urls[0]);
        if urls.len() > 1 {
            say(&format!("(other addresses: {})", ips[1..].iter().map(|i| i.to_string()).collect::<Vec<_>>().join(", ")));
        }
        say(&format!("Dashboard (approve phones, settings): http://127.0.0.1:{}/dashboard\n", cfg.port));
    }

    let listener = tokio::net::TcpListener::from_std(listener)?;
    tokio::spawn(server::poll_state(app.clone()));
    let svc = server::router(app).into_make_service_with_connect_info::<SocketAddr>();
    axum::serve(listener, svc)
        .with_graceful_shutdown(async move {
            if console {
                let _ = tokio::signal::ctrl_c().await;
            } else {
                // Background mode: no signal, no window, no browser can stop the server.
                std::future::pending::<()>().await;
            }
        })
        .await?;
    Ok(())
}

/// Open a URL in the default browser without flashing a console window.
pub fn open_url(url: &str) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("cmd")
            .args(["/c", "start", "", url])
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .spawn();
    }
    #[cfg(not(windows))]
    let _ = url;
}

#[cfg(windows)]
fn install(cfg: &config::Config) -> Result<()> {
    let exe = std::env::current_exe()?;
    let ok = std::process::Command::new("netsh")
        .args([
            "advfirewall", "firewall", "add", "rule", "name=Phone Remote", "dir=in", "action=allow",
            "protocol=TCP", &format!("localport={}", cfg.port), "profile=private",
            &format!("program={}", exe.display()),
        ])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    say(&format!(
        "{} firewall rule (private networks){}",
        if ok { "OK  " } else { "FAIL" },
        if ok { "" } else { " - re-run from an Administrator prompt" }
    ));
    Ok(())
}

#[cfg(not(windows))]
fn install(_: &config::Config) -> Result<()> {
    say("`install` is only implemented for Windows.");
    Ok(())
}
