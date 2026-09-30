mod backend;
mod config;
mod controller;
mod discovery;
mod net;
mod protocol;
mod server;

use anyhow::Result;
use std::{net::SocketAddr, sync::Arc};

fn usage() {
    println!(
        "phone-remote [serve] [--port N] [--mock] [--no-mdns]\n\
         phone-remote pair      print the pairing QR\n\
         phone-remote rotate    new secret; unpairs every phone\n\
         phone-remote install   (Windows) start at login + allow private-network firewall access"
    );
}

fn pair_url(ip: std::net::Ipv4Addr, port: u16, token: &str) -> String {
    format!("http://{ip}:{port}/#k={token}")
}

fn print_qr(url: &str) {
    if let Ok(code) = qrcode::QrCode::new(url.as_bytes()) {
        let s = code
            .render::<char>()
            .quiet_zone(true)
            .module_dimensions(2, 1)
            .dark_color('█')
            .light_color(' ')
            .build();
        println!("{s}");
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("serve");
    let flag = |f: &str| args.iter().any(|a| a == f);
    let mut cfg = config::load_or_create()?;
    if let Some(i) = args.iter().position(|a| a == "--port") {
        if let Some(p) = args.get(i + 1).and_then(|p| p.parse().ok()) {
            cfg.port = p;
            config::save(&cfg)?;
        }
    }
    let ips = net::lan_addrs();
    let ip = ips.first().copied().unwrap_or(std::net::Ipv4Addr::LOCALHOST);

    match cmd {
        "-h" | "--help" | "help" => usage(),
        "rotate" => {
            cfg.token = config::new_token();
            config::save(&cfg)?;
            println!("New secret generated. All phones must pair again.");
        }
        "pair" => {
            let url = pair_url(ip, cfg.port, &cfg.token);
            print_qr(&url);
            println!("{url}");
        }
        "install" => install(&cfg)?,
        _ => serve(cfg, ips, flag("--mock"), flag("--no-mdns")).await?,
    }
    Ok(())
}

async fn serve(cfg: config::Config, ips: Vec<std::net::Ipv4Addr>, mock: bool, no_mdns: bool) -> Result<()> {
    let host = net::hostname();
    let backend: Arc<dyn backend::Backend> = Arc::from(backend::default_backend(mock));
    println!("Backend: {}", backend.name());
    let controller = Arc::new(controller::Controller::new(backend, host.clone()));
    let ip = ips.first().copied().unwrap_or(std::net::Ipv4Addr::LOCALHOST);
    let url = pair_url(ip, cfg.port, &cfg.token);
    let app = server::App::new(controller, cfg.token.clone(), url.clone());

    let _mdns = if no_mdns {
        None
    } else {
        match discovery::advertise(&host, cfg.port) {
            Ok(d) => Some(d),
            Err(e) => {
                eprintln!("mDNS advertising failed (QR pairing still works): {e}");
                None
            }
        }
    };

    let listener = tokio::net::TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], cfg.port))).await?;
    println!("\nScan with your phone (same Wi-Fi):\n");
    print_qr(&url);
    println!("{url}");
    if ips.len() > 1 {
        println!("(other addresses: {})", ips[1..].iter().map(|i| i.to_string()).collect::<Vec<_>>().join(", "));
    }
    println!("Pairing page on this PC: http://127.0.0.1:{}/pair\n", cfg.port);

    tokio::spawn(server::poll_state(app.clone()));
    let svc = server::router(app).into_make_service_with_connect_info::<SocketAddr>();
    axum::serve(listener, svc)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}

#[cfg(windows)]
fn install(cfg: &config::Config) -> Result<()> {
    use std::process::Command;
    let exe = std::env::current_exe()?;
    let run = |prog: &str, a: &[&str]| -> bool {
        Command::new(prog).args(a).status().map(|s| s.success()).unwrap_or(false)
    };
    let value = format!("\"{}\" serve", exe.display());
    let ok = run(
        "reg",
        &["add", r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run", "/v", "PhoneRemote", "/t", "REG_SZ", "/d", &value, "/f"],
    );
    println!("{} start at login", if ok { "OK  " } else { "FAIL" });
    let port = cfg.port.to_string();
    let program = exe.display().to_string();
    let ok = run(
        "netsh",
        &[
            "advfirewall", "firewall", "add", "rule", "name=Phone Remote", "dir=in", "action=allow",
            "protocol=TCP", &format!("localport={port}"), "profile=private", &format!("program={program}"),
        ],
    );
    println!(
        "{} firewall rule (private networks){}",
        if ok { "OK  " } else { "FAIL" },
        if ok { "" } else { " - re-run from an Administrator prompt" }
    );
    Ok(())
}

#[cfg(not(windows))]
fn install(_: &config::Config) -> Result<()> {
    println!("`install` is only implemented for Windows.");
    Ok(())
}
