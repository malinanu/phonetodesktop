use crate::auth::{Auth, AuthErr, Decision, PairErr, Pending};
use crate::config::Config;
use crate::controller::Controller;
use crate::net::is_lan;
use crate::protocol::{ClientMsg, Command};
use axum::{
    extract::{ws::{Message, WebSocket, WebSocketUpgrade}, ConnectInfo, Request, State},
    http::{header, HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::get,
    Router,
};
use std::{net::{Ipv4Addr, SocketAddr}, sync::{Arc, Mutex}, time::{Duration, Instant}};
use tokio::sync::{watch, Notify};

const INDEX: &str = include_str!("../web/index.html");
const DASHBOARD: &str = include_str!("../web/dashboard.html");
const PAIR_PAGE: &str = include_str!("../web/pair.html");
// Shared with the Android app (bundled there as assets).
const GUIDE: &str = include_str!("../../shared/guide.html");
const BASE_CSS: &str = include_str!("../../shared/base.css");
const FONT: &[u8] = include_bytes!("../../shared/font.woff2");
const PAD_JS: &str = include_str!("../../shared/pad.js");
const PAD_CSS: &str = include_str!("../../shared/pad.css");

/// Called when a new phone asks to pair (the tray shows an Allow/Deny prompt).
pub type PendingHook = Arc<dyn Fn(Arc<App>, Pending) + Send + Sync>;

/// Test hook (see `--simulate-crash hang`): make /health stop answering.
pub static HANG: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

static HOOK: std::sync::OnceLock<PendingHook> = std::sync::OnceLock::new();

#[cfg_attr(not(windows), allow(dead_code))]
pub fn set_pending_hook(h: PendingHook) {
    let _ = HOOK.set(h);
}

pub fn pending_hook() -> Option<PendingHook> {
    HOOK.get().cloned()
}

pub struct App {
    pub controller: Arc<Controller>,
    pub auth: Auth,
    pub host: String,
    pub pc_id: String,
    pub port: u16,
    pub ips: Vec<Ipv4Addr>,
    pub vlc_password: String,
    pub on_pending: Option<PendingHook>,
    tx: watch::Sender<String>,
    notify: Notify,
    fails: Mutex<(u32, Instant)>,
}

impl App {
    pub fn new(controller: Arc<Controller>, cfg: &Config, host: String, ips: Vec<Ipv4Addr>, on_pending: Option<PendingHook>) -> Arc<Self> {
        Arc::new(App {
            controller,
            auth: Auth::new(cfg.clone()),
            host,
            pc_id: cfg.pc_id.clone(),
            port: cfg.port,
            ips,
            vlc_password: crate::backend::vlc_password(&cfg.local_secret),
            on_pending,
            tx: watch::channel(String::new()).0,
            notify: Notify::new(),
            fails: Mutex::new((0, Instant::now())),
        })
    }

    /// Percent-encode everything except unreserved URL characters.
    fn enc(s: &str) -> String {
        s.bytes()
            .map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
            .collect()
    }

    /// What the QR carries: where the PC is, a short-lived pairing code, and who the PC is.
    pub fn pair_urls(&self) -> Vec<String> {
        let code = self.auth.code();
        let ips: Vec<Ipv4Addr> = if self.ips.is_empty() { vec![Ipv4Addr::LOCALHOST] } else { self.ips.clone() };
        ips.iter()
            .map(|ip| format!("http://{ip}:{}/#k={code}&id={}&n={}", self.port, self.pc_id, Self::enc(&self.host)))
            .collect()
    }

    /// 5 bad tokens inside a minute locks authentication for a minute.
    fn locked_out(&self) -> bool {
        let mut f = self.fails.lock().unwrap();
        if f.1.elapsed() > Duration::from_secs(60) {
            *f = (0, Instant::now());
        }
        f.0 >= 5
    }

    fn record_fail(&self) {
        let mut f = self.fails.lock().unwrap();
        if f.0 == 0 {
            f.1 = Instant::now();
        }
        f.0 += 1;
    }

    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn poke(&self) {
        self.notify.notify_one();
    }
}

/// Requests that may only come from this PC's own browser: loopback peer, and a Host header that
/// names loopback (blocks DNS-rebinding pages that resolve their own name to 127.0.0.1).
pub fn is_local(peer: &SocketAddr, headers: &HeaderMap, port: u16) -> bool {
    let host = headers.get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or("");
    peer.ip().is_loopback() && (host == format!("127.0.0.1:{port}") || host == format!("localhost:{port}"))
}

pub fn router(app: Arc<App>) -> Router {
    Router::new()
        .route("/", get(|| async { Html(INDEX) }))
        .route("/ws", get(ws_upgrade))
        .route("/pair", get(pair_page))
        .route("/debug", get(debug_page))
        .route("/guide", get(|| async { Html(GUIDE) }))
        .route("/base.css", get(|| async { ([(header::CONTENT_TYPE, "text/css; charset=utf-8"), (header::CACHE_CONTROL, "max-age=3600")], BASE_CSS) }))
        .route("/pad.js", get(|| async { ([(header::CONTENT_TYPE, "application/javascript; charset=utf-8"), (header::CACHE_CONTROL, "no-cache")], PAD_JS) }))
        .route("/pad.css", get(|| async { ([(header::CONTENT_TYPE, "text/css; charset=utf-8"), (header::CACHE_CONTROL, "no-cache")], PAD_CSS) }))
        .route("/font.woff2", get(|| async { ([(header::CONTENT_TYPE, "font/woff2"), (header::CACHE_CONTROL, "max-age=31536000, immutable")], FONT) }))
        .route("/dashboard", get(dashboard_page))
        .route("/health", get(|| async {
            if HANG.load(std::sync::atomic::Ordering::SeqCst) {
                std::future::pending::<()>().await;
            }
            "ok"
        }))
        .merge(crate::api::routes())
        .layer(middleware::from_fn(lan_only))
        .with_state(app)
}

async fn lan_only(ConnectInfo(peer): ConnectInfo<SocketAddr>, req: Request, next: Next) -> Response {
    if is_lan(peer.ip()) {
        next.run(req).await
    } else {
        StatusCode::FORBIDDEN.into_response()
    }
}

/// Shows the pairing QR; only reachable from the desktop itself.
async fn pair_page(ConnectInfo(peer): ConnectInfo<SocketAddr>, headers: HeaderMap, State(app): State<Arc<App>>) -> Response {
    if !is_local(&peer, &headers, app.port) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let urls = app.pair_urls();
    let main = urls.first().cloned().unwrap_or_default();
    let others: String = urls.iter().skip(1).map(|u| format!("<div class=qr>{}</div><code>{u}</code>", qr_svg(u, 340))).collect();
    let more = if others.is_empty() {
        String::new()
    } else {
        format!("<details><summary>Phone can't connect? Try another network address</summary>{others}</details>")
    };
    let body = PAIR_PAGE.replace("{{QR}}", &qr_svg(&main, 340)).replace("{{URL}}", &main).replace("{{MORE}}", &more);
    ([(header::CACHE_CONTROL, "no-store")], Html(body)).into_response()
}

pub fn qr_svg(url: &str, size: u32) -> String {
    qrcode::QrCode::new(url.as_bytes())
        .map(|c| c.render::<qrcode::render::svg::Color>().min_dimensions(size, size).quiet_zone(true).build())
        .unwrap_or_default()
}

async fn dashboard_page(ConnectInfo(peer): ConnectInfo<SocketAddr>, headers: HeaderMap, State(app): State<Arc<App>>) -> Response {
    if !is_local(&peer, &headers, app.port) {
        return StatusCode::FORBIDDEN.into_response();
    }
    ([(header::CACHE_CONTROL, "no-store")], Html(DASHBOARD)).into_response()
}

/// Loopback-only diagnostics: what the agent can see right now, and why sessions may be missing.
async fn debug_page(ConnectInfo(peer): ConnectInfo<SocketAddr>, headers: HeaderMap, State(app): State<Arc<App>>) -> Response {
    if !is_local(&peer, &headers, app.port) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let c = app.controller.clone();
    let text = tokio::task::spawn_blocking(move || c.debug()).await.unwrap_or_default();
    ([(header::CONTENT_TYPE, "text/plain; charset=utf-8"), (header::CACHE_CONTROL, "no-store")], text).into_response()
}

async fn ws_upgrade(ws: WebSocketUpgrade, ConnectInfo(peer): ConnectInfo<SocketAddr>, headers: HeaderMap, State(app): State<Arc<App>>) -> Response {
    // Block cross-site WebSocket hijacking: a browser Origin must match our own Host.
    if let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
        let host = headers.get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or("");
        let auth = origin.split("://").nth(1).unwrap_or("");
        if auth != host {
            return StatusCode::FORBIDDEN.into_response();
        }
    }
    ws.on_upgrade(move |s| session(s, app, peer))
}

/// Allows `per_sec` events per second; the excess is dropped.
struct Bucket {
    per_sec: u32,
    window: Instant,
    used: u32,
}

impl Bucket {
    fn new(per_sec: u32) -> Self {
        Bucket { per_sec, window: Instant::now(), used: 0 }
    }
    fn allow(&mut self) -> bool {
        if self.window.elapsed() >= Duration::from_secs(1) {
            self.window = Instant::now();
            self.used = 0;
        }
        self.used += 1;
        self.used <= self.per_sec
    }
}

fn text(s: impl Into<String>) -> Message {
    Message::Text(s.into().into())
}

/// Who logged in: a named device, or a legacy shared-secret phone.
struct Login {
    device: Option<String>,
    name: String,
}

async fn session(mut sock: WebSocket, app: Arc<App>, peer: SocketAddr) {
    let Some(login) = authenticate(&mut sock, &app, peer).await else { return };
    if let Some(d) = &login.device {
        app.auth.set_online(d, true);
    }
    let mut rx = app.tx.subscribe();
    let mut bucket = Bucket::new(400);
    let mut last_input_err = Instant::now() - Duration::from_secs(5);
    crate::log::log(&format!("phone connected: {}", login.name));
    let caps = login.device.as_deref().is_some_and(|d| app.auth.input_allowed(d));
    let _ = sock.send(text(serde_json::json!({"t":"auth","ok":true,"input":caps}).to_string())).await;
    app.notify.notify_one();
    let initial = rx.borrow().clone();
    if !initial.is_empty() {
        let _ = sock.send(text(initial)).await;
    }
    loop {
        tokio::select! {
            changed = rx.changed() => {
                if changed.is_err() { break; }
                // A phone removed from the dashboard loses access within a second.
                if let Some(d) = &login.device {
                    if !app.auth.devices().iter().any(|(x, _)| &x.id == d) {
                        let _ = sock.send(text(r#"{"t":"auth","ok":false,"err":"revoked"}"#)).await;
                        break;
                    }
                }
                let s = rx.borrow_and_update().clone();
                if sock.send(text(s)).await.is_err() { break; }
            }
            msg = sock.recv() => {
                let Some(Ok(msg)) = msg else { break };
                let Message::Text(t) = msg else { if matches!(msg, Message::Close(_)) { break } else { continue } };
                match serde_json::from_str::<ClientMsg>(t.as_str()) {
                    Ok(ClientMsg::Cmd(cmd)) if cmd.is_input() => {
                        // Mouse/keyboard: checked live, run inline, answer only on failure.
                        let allowed = login.device.as_deref().is_some_and(|d| app.auth.input_allowed(d));
                        let problem = if !allowed {
                            Some("Mouse and keyboard are turned off for this phone (PC dashboard → Phones).".to_string())
                        } else if !bucket.allow() {
                            None // over the rate limit: drop silently, the phone coalesces anyway
                        } else {
                            app.controller.execute_input(&cmd).err().map(|e| e.to_string())
                        };
                        if let Some(err) = problem {
                            // One error per second at most, so a blocked window cannot flood the phone.
                            if last_input_err.elapsed() > Duration::from_secs(1) {
                                last_input_err = Instant::now();
                                let m = serde_json::json!({"t":"ack","ok":false,"err":err}).to_string();
                                if sock.send(text(m)).await.is_err() { break; }
                            }
                        }
                    }
                    Ok(ClientMsg::Cmd(cmd)) => {
                        // A device removed from the PC loses control immediately.
                        if let Some(d) = &login.device {
                            if !app.auth.devices().iter().any(|(x, _)| &x.id == d) {
                                let _ = sock.send(text(r#"{"t":"auth","ok":false,"err":"revoked"}"#)).await;
                                break;
                            }
                        }
                        let reply = run(&app, cmd).await;
                        if sock.send(text(reply)).await.is_err() { break; }
                        app.notify.notify_one();
                    }
                    Ok(ClientMsg::Ping) => { let _ = sock.send(text(r#"{"t":"pong"}"#)).await; }
                    _ => {}
                }
            }
        }
    }
    if let Some(d) = &login.device {
        app.auth.set_online(d, false);
    }
    crate::log::log(&format!("phone disconnected: {}", login.name));
}

async fn fail(sock: &mut WebSocket, json: serde_json::Value) -> Option<Login> {
    let _ = sock.send(text(json.to_string())).await;
    None
}

async fn authenticate(sock: &mut WebSocket, app: &Arc<App>, peer: SocketAddr) -> Option<Login> {
    let first = tokio::time::timeout(Duration::from_secs(5), sock.recv()).await;
    let msg = match first {
        Ok(Some(Ok(Message::Text(t)))) => serde_json::from_str::<ClientMsg>(t.as_str()).ok()?,
        _ => return None,
    };
    if app.locked_out() {
        return fail(sock, serde_json::json!({"t":"auth","ok":false,"err":"locked"})).await;
    }
    match msg {
        ClientMsg::Auth { token, device } => {
            let res = match &device {
                Some(id) => app.auth.check_device(id, &token),
                None => app.auth.check_legacy(&token).map(|_| "Shared-secret phone".to_string()),
            };
            match res {
                Ok(name) => Some(Login { device, name }),
                Err(AuthErr::BadToken) => {
                    app.record_fail();
                    fail(sock, serde_json::json!({"t":"auth","ok":false,"err":"bad token"})).await
                }
                Err(AuthErr::Revoked) => fail(sock, serde_json::json!({"t":"auth","ok":false,"err":"revoked"})).await,
                Err(AuthErr::LegacyOff) => fail(sock, serde_json::json!({"t":"auth","ok":false,"err":"revoked"})).await,
            }
        }
        ClientMsg::Pair { code, device, name } => pair(sock, app, peer, code, device, name).await,
        _ => None,
    }
}

/// First contact from a QR scan. The phone waits here until the PC's owner answers.
async fn pair(sock: &mut WebSocket, app: &Arc<App>, peer: SocketAddr, code: String, device: String, name: String) -> Option<Login> {
    if device.is_empty() || device.len() > 64 {
        return None;
    }
    match app.auth.request_pairing(&code, &device, &name, &peer.ip().to_string()) {
        Err(PairErr::BadCode) => {
            app.record_fail();
            return fail(sock, serde_json::json!({"t":"pair","status":"bad_code"})).await;
        }
        Err(PairErr::Expired) => return fail(sock, serde_json::json!({"t":"pair","status":"expired"})).await,
        Ok(Some(token)) => {
            let _ = sock.send(text(serde_json::json!({"t":"pair","status":"approved","device_token":token}).to_string())).await;
            return Some(Login { device: Some(device), name });
        }
        Ok(None) => {}
    }
    crate::log::log(&format!("pairing request from {name} ({})", peer.ip()));
    let _ = sock.send(text(r#"{"t":"pair","status":"pending"}"#)).await;
    if let (Some(hook), Some(p)) = (&app.on_pending, app.auth.pending().into_iter().find(|p| p.id == device)) {
        hook(app.clone(), p);
    }
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        match app.auth.take_decision(&device) {
            Some(Decision::Approved(token)) => {
                crate::log::log(&format!("pairing approved: {name}"));
                let _ = sock.send(text(serde_json::json!({"t":"pair","status":"approved","device_token":token}).to_string())).await;
                return Some(Login { device: Some(device), name });
            }
            Some(Decision::Denied) => {
                crate::log::log(&format!("pairing denied: {name}"));
                return fail(sock, serde_json::json!({"t":"pair","status":"denied"})).await;
            }
            None => {}
        }
        if Instant::now() > deadline {
            app.auth.cancel_pending(&device);
            return fail(sock, serde_json::json!({"t":"pair","status":"expired"})).await;
        }
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(400)) => {}
            msg = sock.recv() => {
                // The phone closed the page while waiting.
                if !matches!(msg, Some(Ok(Message::Text(_))) | Some(Ok(Message::Ping(_))) | Some(Ok(Message::Pong(_)))) {
                    app.auth.cancel_pending(&device);
                    return None;
                }
            }
        }
    }
}

async fn run(app: &Arc<App>, cmd: Command) -> String {
    let c = app.controller.clone();
    let res = tokio::task::spawn_blocking(move || c.execute(cmd)).await;
    match res {
        Ok(Ok(())) => r#"{"t":"ack","ok":true}"#.to_string(),
        Ok(Err(e)) => serde_json::json!({"t":"ack","ok":false,"err":e.to_string()}).to_string(),
        Err(e) => serde_json::json!({"t":"ack","ok":false,"err":e.to_string()}).to_string(),
    }
}

/// Recompute the state about once a second (or right after a command) while phones are connected.
pub async fn poll_state(app: Arc<App>) {
    loop {
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(1)) => {}
            _ = app.notify.notified() => {}
        }
        #[cfg(windows)]
        crate::tray::set_tooltip(&tooltip(&app));
        if app.tx.receiver_count() == 0 {
            continue;
        }
        let c = app.controller.clone();
        if let Ok(Ok(state)) = tokio::task::spawn_blocking(move || c.state()).await {
            if let Ok(json) = serde_json::to_string(&state) {
                app.tx.send_replace(json);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_limits_per_second() {
        let mut b = Bucket::new(3);
        assert_eq!((0..5).filter(|_| b.allow()).count(), 3);
    }
}

#[cfg(windows)]
fn tooltip(app: &App) -> String {
    let n = app.auth.online_count();
    match n {
        0 => "Phone Remote: no phone connected".to_string(),
        1 => "Phone Remote: 1 phone connected".to_string(),
        n => format!("Phone Remote: {n} phones connected"),
    }
}
