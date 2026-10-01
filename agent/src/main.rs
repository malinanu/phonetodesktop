// On Windows the agent is a background app: no console window, a tray icon instead.
// CLI subcommands re-attach to the parent terminal so their output still shows.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod account;
mod api;
mod auth;
mod backend;
mod config;
mod controller;
mod discovery;
mod files;
mod guardian;
mod log;
mod net;
mod platform;
mod protocol;
mod server;
mod tls;
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
     phone-remote open      (macOS/Linux) start the agent if needed and show the dashboard\n\
     phone-remote autostart on|off|status   (macOS/Linux) start at login\n\
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

fn main() {
    if let Err(e) = real_main() {
        log::log(&format!("fatal: {e:#}"));
        say(&format!("error: {e:#}"));
        std::process::exit(1);
    }
}

fn real_main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("serve");
    let flag = |f: &str| args.iter().any(|a| a == f);
    let worker = flag("--worker");
    let cli = !matches!(cmd, "serve") && !cmd.starts_with("--");
    #[cfg(windows)]
    if flag("--console") || cli {
        attach_console();
    }
    let mut cfg = config::load_or_create()?;
    if let Some(i) = args.iter().position(|a| a == "--port") {
        if let Some(p) = args.get(i + 1).and_then(|p| p.parse().ok()) {
            cfg.port = p;
            config::save(&cfg)?;
        }
    }

    // A normal launch on Windows is the guardian, which keeps the real agent ("worker") alive.
    // `--guardian` forces this on other systems (used to test it).
    let console = flag("--console") || cfg!(not(windows));
    if !worker && !cli && !flag("--console") && (cfg!(windows) || flag("--guardian")) {
        let port = cfg.port;
        if !guardian::acquire_single_instance() {
            // Already running: just bring the dashboard forward.
            #[cfg(windows)]
            if !flag("--background") {
                tray::open_dashboard(&format!("http://127.0.0.1:{port}/dashboard"));
            }
            return Ok(());
        }
        let forward: Vec<String> = args
            .iter()
            .enumerate()
            .filter(|(i, a)| {
                matches!(a.as_str(), "--mock" | "--no-mdns" | "--port" | "--simulate-crash" | "--background" | "--console")
                    || (*i > 0 && matches!(args[i - 1].as_str(), "--port" | "--simulate-crash"))
            })
            .map(|(_, a)| a.clone())
            .collect();
        std::process::exit(guardian::run(port, forward));
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
        "open" => open_cmd(cfg.port)?,
        "autostart" => autostart_cmd(args.get(1).map(String::as_str))?,
        "setup-players" => backend::setup::apply(&backend::vlc_password(&cfg.local_secret)).iter().for_each(|l| say(l)),
        _ => {
            log::install_hooks();
            log::mark_running();
            let simulate = args.iter().position(|a| a == "--simulate-crash").and_then(|i| args.get(i + 1)).cloned();
            run(cfg, ips, flag("--mock"), flag("--no-mdns"), flag("--background"), console, simulate)?
        }
    }
    Ok(())
}

/// Test hook: make the worker die (or hang) on purpose so the guardian can be verified.
/// `exit:CODE`, `abort` or `hang`, optionally followed by `@SECONDS` (default 2).
fn simulate_crash(spec: String) {
    let (kind, secs) = spec.split_once('@').map(|(k, s)| (k.to_string(), s.parse().unwrap_or(2))).unwrap_or((spec, 2));
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(secs));
        log::log(&format!("simulating: {kind}"));
        match kind.as_str() {
            k if k.starts_with("exit:") => std::process::exit(k[5..].parse().unwrap_or(1)),
            "hang" => server::HANG.store(true, std::sync::atomic::Ordering::SeqCst),
            _ => std::process::abort(),
        }
    });
}

fn run(cfg: config::Config, ips: Vec<std::net::Ipv4Addr>, mock: bool, no_mdns: bool, background: bool, console: bool, simulate: Option<String>) -> Result<()> {
    if let Some(spec) = simulate {
        simulate_crash(spec);
    }
    let pair_page = format!("http://127.0.0.1:{}/dashboard", cfg.port);
    // Bind here so a second launch can detect the running agent and just show the pairing page.
    let listener = match std::net::TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], cfg.port))) {
        Ok(l) => l,
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            // Another program owns our port (the guardian already made sure we are not a second copy).
            log::log(&format!("port {} is already in use by another program", cfg.port));
            say(&format!("Port {} is already in use. Start with --port N to use another.", cfg.port));
            #[cfg(windows)]
            tray::info("Phone Remote", &format!("Port {} is used by another program, so Phone Remote cannot start.\n\nRun it once from a terminal with: phone-remote --port 8766", cfg.port));
            std::process::exit(guardian::EXIT_PORT_BUSY);
        }
        Err(e) => return Err(e.into()),
    };
    listener.set_nonblocking(true)?;

    let (server_cfg, server_ips) = (cfg.clone(), ips.clone());

    #[cfg(windows)]
    if !console {
        server::set_pending_hook(Arc::new(|app, p| tray::request_approval(app, p)));
        log::log(&format!("agent {} starting (pid {}{})", env!("CARGO_PKG_VERSION"), std::process::id(), if background { ", background" } else { "" }));
        // The server thread never ends on its own: only the tray's Quit stops the process.
        std::thread::spawn(move || supervise(server_cfg, server_ips, listener, mock, no_mdns));
        if !background {
            tray::open_dashboard(&format!("{pair_page}#add"));
        }
        // The tray loop only returns if something went wrong: exit non-zero so the guardian restarts us.
        let res = tray::run(cfg, pair_page);
        log::log(&format!("tray loop ended unexpectedly: {res:?}"));
        std::process::exit(guardian::EXIT_TRAY_LOST);
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
    // This computer's TLS identity (made once). If it cannot be made, fall back to plain HTTP so the agent still runs.
    let identity = match config::dir().and_then(|d| tls::load_or_create(&d, &host)) {
        Ok(i) => Some(i),
        Err(e) => {
            log::log(&format!("TLS identity unavailable ({e:#}); serving plain HTTP only"));
            None
        }
    };
    let app = server::App::new(controller, &cfg, host.clone(), ips.clone(), server::pending_hook(), identity.as_ref().map(|i| i.fingerprint.clone()));
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
        if let Some(i) = &identity {
            say(&format!("Secure connection: phones pin this key fingerprint: {}", i.fingerprint));
        }
        say(&format!("Dashboard (approve phones, settings): http://127.0.0.1:{}/dashboard\n", cfg.port));
    }

    let listener = tokio::net::TcpListener::from_std(listener)?;
    // The state poller feeds every phone; if it ever panics, start it again.
    let poller_app = app.clone();
    tokio::spawn(async move {
        loop {
            let r = tokio::spawn(server::poll_state(poller_app.clone())).await;
            log::log(&format!("state poller stopped ({r:?}); restarting"));
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    });
    // Send files: the same page FileSync serves, built in, on the next port up. Failure to bind is not fatal.
    if let Some(port) = cfg.port.checked_add(1) {
        match tokio::net::TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], port))).await {
            Ok(l) => {
                app.files_port.store(port, std::sync::atomic::Ordering::Relaxed);
                let svc = files::router(app.clone()).into_make_service_with_connect_info::<SocketAddr>();
                log::log(&format!("Send files on port {port}"));
                tokio::spawn(async move {
                    if let Err(e) = axum::serve(l, svc).await {
                        log::log(&format!("Send files stopped: {e}"));
                    }
                });
            }
            Err(e) => log::log(&format!("Send files unavailable (port {port}): {e}")),
        }
    }
    let svc = server::router(app.clone()).into_make_service_with_connect_info::<SocketAddr>();
    match identity.as_ref().map(tls::acceptor) {
        Some(Ok(acceptor)) => {
            let app = app.clone();
            // `tap_io` (a no-op here) is what lets axum hand handlers the peer address for a custom listener.
            use axum::serve::ListenerExt;
            let listener = tls::SniffListener::new(listener, acceptor, Arc::new(move || app.auth.v1_allowed()))?.tap_io(|_| {});
            axum::serve(listener, svc).with_graceful_shutdown(wait_for_shutdown(console)).await?;
        }
        other => {
            if let Some(Err(e)) = other {
                log::log(&format!("TLS unavailable ({e:#}); serving plain HTTP only"));
            }
            axum::serve(listener, svc).with_graceful_shutdown(wait_for_shutdown(console)).await?;
        }
    }
    Ok(())
}

async fn wait_for_shutdown(console: bool) {
    if console {
        let _ = tokio::signal::ctrl_c().await;
    } else {
        // Background mode: no signal, no window, no browser can stop the server.
        std::future::pending::<()>().await;
    }
}

/// Open a URL in the default browser without flashing a console window.
pub fn open_url(url: &str) {
    platform::open_url(url);
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

#[cfg(unix)]
fn open_cmd(port: u16) -> Result<()> {
    platform::open_dashboard(port)
}

#[cfg(not(unix))]
fn open_cmd(port: u16) -> Result<()> {
    // Windows: the tray app is the launcher; just show the dashboard if it is running.
    open_url(&format!("http://127.0.0.1:{port}/dashboard"));
    Ok(())
}

#[cfg(unix)]
fn autostart_cmd(what: Option<&str>) -> Result<()> {
    match what {
        Some("on") => platform::set_autostart(true)?,
        Some("off") => platform::set_autostart(false)?,
        Some("status") | None => {}
        Some(other) => anyhow::bail!("unknown option {other:?}; use on, off or status"),
    }
    say(if platform::autostart_enabled() { "Start at login: on" } else { "Start at login: off" });
    Ok(())
}

#[cfg(not(unix))]
fn autostart_cmd(_: Option<&str>) -> Result<()> {
    say("On Windows, use the tray menu: Start with Windows.");
    Ok(())
}

#[cfg(not(windows))]
fn install(_: &config::Config) -> Result<()> {
    say("`install` is only implemented for Windows.");
    Ok(())
}
