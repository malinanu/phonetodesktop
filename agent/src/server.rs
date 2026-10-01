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
const APPROVE: &str = include_str!("../web/approve.html");
// Shared with the Android app (bundled there as assets).
const GUIDE: &str = include_str!("../../shared/guide.html");
const BASE_CSS: &str = include_str!("../../shared/base.css");
const FONT: &[u8] = include_bytes!("../../shared/font.woff2");
const PAD_JS: &str = include_str!("../../shared/pad.js");
const PAD_CSS: &str = include_str!("../../shared/pad.css");
const SETTINGS_JS: &str = include_str!("../../shared/settings.js");
const SETTINGS_PAGE: &str = include_str!("../../shared/settings.html");

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
    /// Validated address of the "Send files" server, if one is configured.
    pub files_url: Option<String>,
    /// SHA-256 of this computer's TLS public key (base64url). Goes into the QR so phones can pin it.
    pub tls_fp: Option<String>,
    pub on_pending: Option<PendingHook>,
    tx: watch::Sender<String>,
    notify: Notify,
    fails: Mutex<(u32, Instant)>,
}

impl App {
    pub fn new(controller: Arc<Controller>, cfg: &Config, host: String, ips: Vec<Ipv4Addr>, on_pending: Option<PendingHook>, tls_fp: Option<String>) -> Arc<Self> {
        Self::with_auth(Auth::new(cfg.clone()), controller, cfg, host, ips, on_pending, tls_fp)
    }

    fn with_auth(auth: Auth, controller: Arc<Controller>, cfg: &Config, host: String, ips: Vec<Ipv4Addr>, on_pending: Option<PendingHook>, tls_fp: Option<String>) -> Arc<Self> {
        Arc::new(App {
            controller,
            auth,
            host,
            pc_id: cfg.pc_id.clone(),
            port: cfg.port,
            ips,
            vlc_password: crate::backend::vlc_password(&cfg.local_secret),
            files_url: crate::config::files_url(cfg),
            tls_fp,
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

    /// What the QR carries: where the PC is, a short-lived pairing code, who the PC is and, for secure phones,
    /// the fingerprint (`fp`) of its TLS key. It stays an `http://` link so a plain camera app still opens the
    /// browser remote (while older phones are allowed); the phone app connects with `https://` and pins `fp`.
    pub fn pair_urls(&self) -> Vec<String> {
        let code = self.auth.code();
        let ips: Vec<Ipv4Addr> = if self.ips.is_empty() { vec![Ipv4Addr::LOCALHOST] } else { self.ips.clone() };
        let fp = self.tls_fp.as_deref().map(|f| format!("&fp={f}")).unwrap_or_default();
        ips.iter()
            .map(|ip| format!("http://{ip}:{}/#k={code}&id={}&n={}{fp}", self.port, self.pc_id, Self::enc(&self.host)))
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
        .route("/settings.js", get(|| async { ([(header::CONTENT_TYPE, "application/javascript; charset=utf-8"), (header::CACHE_CONTROL, "no-cache")], SETTINGS_JS) }))
        .route("/settings", get(|| async { Html(SETTINGS_PAGE) }))
        .route("/pad.css", get(|| async { ([(header::CONTENT_TYPE, "text/css; charset=utf-8"), (header::CACHE_CONTROL, "no-cache")], PAD_CSS) }))
        .route("/font.woff2", get(|| async { ([(header::CONTENT_TYPE, "font/woff2"), (header::CACHE_CONTROL, "max-age=31536000, immutable")], FONT) }))
        .route("/dashboard", get(dashboard_page))
        .route("/approve", get(approve_page))
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

async fn approve_page(ConnectInfo(peer): ConnectInfo<SocketAddr>, headers: HeaderMap, State(app): State<Arc<App>>) -> Response {
    if !is_local(&peer, &headers, app.port) {
        return StatusCode::FORBIDDEN.into_response();
    }
    ([(header::CACHE_CONTROL, "no-store")], Html(APPROVE)).into_response()
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
        ClientMsg::Auth { .. } | ClientMsg::Pair { pk: None, .. } if !app.auth.v1_allowed() => {
            // Older phones are switched off on this PC: they must update to the key-based login.
            fail(sock, serde_json::json!({"t":"auth","ok":false,"err":"v2_required"})).await
        }
        ClientMsg::Challenge { device } => challenge_login(sock, app, device).await,
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
        ClientMsg::Pair { code, device, name, pk, platform } => {
            let key = pk.map(|pk| (pk, platform.unwrap_or_default()));
            pair(sock, app, peer, code, device, name, key).await
        }
        _ => None,
    }
}

/// Protocol v2 login: send a fresh random nonce, then check the phone's signature over it.
/// The nonce belongs to this connection only, so a recorded login cannot be replayed.
async fn challenge_login(sock: &mut WebSocket, app: &Arc<App>, device: String) -> Option<Login> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use rand::RngCore;
    let mut nonce = [0u8; 32];
    rand::rng().fill_bytes(&mut nonce);
    let _ = sock.send(text(serde_json::json!({"t":"challenge","nonce":URL_SAFE_NO_PAD.encode(nonce)}).to_string())).await;
    let reply = tokio::time::timeout(Duration::from_secs(5), sock.recv()).await;
    let sig = match reply {
        Ok(Some(Ok(Message::Text(t)))) => match serde_json::from_str::<ClientMsg>(t.as_str()) {
            Ok(ClientMsg::AuthSig { device: d, sig }) if d == device => sig,
            _ => return None,
        },
        _ => return None,
    };
    match app.auth.verify_signature(&device, &nonce, &sig) {
        Ok(name) => Some(Login { device: Some(device), name }),
        Err(AuthErr::Revoked) => fail(sock, serde_json::json!({"t":"auth","ok":false,"err":"revoked"})).await,
        Err(_) => {
            app.record_fail();
            fail(sock, serde_json::json!({"t":"auth","ok":false,"err":"bad token"})).await
        }
    }
}

/// The "approved" message. A key-based (v2) phone gets no token.
fn approved(token: &str) -> String {
    if token.is_empty() {
        serde_json::json!({"t":"pair","status":"approved","v":2}).to_string()
    } else {
        serde_json::json!({"t":"pair","status":"approved","device_token":token}).to_string()
    }
}

/// First contact from a QR scan. The phone waits here until the PC's owner answers.
async fn pair(sock: &mut WebSocket, app: &Arc<App>, peer: SocketAddr, code: String, device: String, name: String, key: Option<(String, String)>) -> Option<Login> {
    if device.is_empty() || device.len() > 64 {
        return None;
    }
    let result = match &key {
        Some((pk, platform)) => app.auth.request_pairing_v2(&code, &device, &name, &peer.ip().to_string(), pk, platform),
        None => app.auth.request_pairing(&code, &device, &name, &peer.ip().to_string()),
    };
    match result {
        Err(PairErr::BadCode) => {
            app.record_fail();
            return fail(sock, serde_json::json!({"t":"pair","status":"bad_code"})).await;
        }
        Err(PairErr::Expired) => return fail(sock, serde_json::json!({"t":"pair","status":"expired"})).await,
        Err(PairErr::BadKey) => return fail(sock, serde_json::json!({"t":"pair","status":"bad_key"})).await,
        Ok(Some(token)) => {
            let _ = sock.send(text(approved(&token))).await;
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
                let _ = sock.send(text(approved(&token))).await;
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

    // ---- protocol v2 over a real WebSocket ----

    use crate::backend::mock::MockBackend;
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use ed25519_dalek::{Signer, SigningKey};
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message as WsMsg;

    type Client = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

    fn test_config() -> Config {
        Config {
            pc_id: "pc-test".into(),
            token: "legacy".into(),
            devices: vec![],
            legacy_shared_auth: false,
            setup_done: true,
            port: 1,
            local_secret: String::new(),
            autostart_initialized: true,
            files_url: String::new(),
            allow_v1: true,
        }
    }

    async fn start() -> (Arc<App>, u16) {
        let cfg = test_config();
        let backend: Arc<dyn crate::backend::Backend> = Arc::new(MockBackend::new());
        let controller = Arc::new(Controller::new(backend, "host".into()));
        let app = App::with_auth(Auth::in_memory(cfg.clone()), controller, &cfg, "host".into(), vec![], None, None);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let svc = router(app.clone()).into_make_service_with_connect_info::<SocketAddr>();
        tokio::spawn(async move { axum::serve(listener, svc).await });
        (app, port)
    }

    async fn connect(port: u16) -> Client {
        tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/ws")).await.unwrap().0
    }

    async fn send(c: &mut Client, v: serde_json::Value) {
        c.send(WsMsg::text(v.to_string())).await.unwrap();
    }

    /// The next JSON message of type `t` (state broadcasts are skipped).
    async fn next(c: &mut Client, t: &str) -> serde_json::Value {
        loop {
            let m = tokio::time::timeout(Duration::from_secs(5), c.next()).await.expect("timed out").expect("closed").unwrap();
            if let WsMsg::Text(txt) = m {
                let v: serde_json::Value = serde_json::from_str(txt.as_str()).unwrap();
                if v["t"] == t {
                    return v;
                }
            }
        }
    }

    fn sign(sk: &SigningKey, pc: &str, device: &str, nonce_b64: &str) -> String {
        let nonce = URL_SAFE_NO_PAD.decode(nonce_b64).unwrap();
        URL_SAFE_NO_PAD.encode(sk.sign(&crate::auth::auth_message(pc, device, &nonce)).to_bytes())
    }

    /// Pair a key-based phone and have the owner approve it.
    async fn pair_v2(app: &Arc<App>, port: u16, sk: &SigningKey, device: &str) {
        let mut c = connect(port).await;
        let pk = URL_SAFE_NO_PAD.encode(sk.verifying_key().to_bytes());
        send(&mut c, serde_json::json!({"t":"pair","code":app.auth.code(),"device":device,"name":"Test phone","pk":pk,"platform":"android"})).await;
        assert_eq!(next(&mut c, "pair").await["status"], "pending");
        let owner = app.clone();
        let dev = device.to_string();
        tokio::spawn(async move {
            loop {
                if owner.auth.decide(&dev, true) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        });
        let done = next(&mut c, "pair").await;
        assert_eq!(done["status"], "approved");
        assert_eq!(done["v"], 2);
        assert!(done.get("device_token").is_none(), "no token for a key-based phone");
    }

    async fn login(port: u16, sk: &SigningKey, device: &str) -> serde_json::Value {
        let mut c = connect(port).await;
        send(&mut c, serde_json::json!({"t":"challenge","device":device})).await;
        let nonce = next(&mut c, "challenge").await["nonce"].as_str().unwrap().to_string();
        send(&mut c, serde_json::json!({"t":"auth_sig","device":device,"sig":sign(sk, "pc-test", device, &nonce)})).await;
        next(&mut c, "auth").await
    }

    #[tokio::test]
    async fn v2_pair_then_login_with_a_signature() {
        let (app, port) = start().await;
        let sk = SigningKey::from_bytes(&[5u8; 32]);
        pair_v2(&app, port, &sk, "dev1").await;
        let ok = login(port, &sk, "dev1").await;
        assert_eq!(ok["ok"], true);
        assert_eq!(ok["input"], true);
    }

    #[tokio::test]
    async fn a_recorded_login_cannot_be_replayed() {
        let (app, port) = start().await;
        let sk = SigningKey::from_bytes(&[5u8; 32]);
        pair_v2(&app, port, &sk, "dev1").await;

        // Record a valid login...
        let mut a = connect(port).await;
        send(&mut a, serde_json::json!({"t":"challenge","device":"dev1"})).await;
        let nonce_a = next(&mut a, "challenge").await["nonce"].as_str().unwrap().to_string();
        let sig_a = sign(&sk, "pc-test", "dev1", &nonce_a);
        send(&mut a, serde_json::json!({"t":"auth_sig","device":"dev1","sig":sig_a.clone()})).await;
        assert_eq!(next(&mut a, "auth").await["ok"], true);

        // ...and replay its signature on a new connection (which has a new nonce).
        let mut b = connect(port).await;
        send(&mut b, serde_json::json!({"t":"challenge","device":"dev1"})).await;
        let nonce_b = next(&mut b, "challenge").await["nonce"].as_str().unwrap().to_string();
        assert_ne!(nonce_a, nonce_b);
        send(&mut b, serde_json::json!({"t":"auth_sig","device":"dev1","sig":sig_a})).await;
        let r = next(&mut b, "auth").await;
        assert_eq!((r["ok"].clone(), r["err"].clone()), (false.into(), "bad token".into()));
    }

    #[tokio::test]
    async fn wrong_key_and_unknown_or_revoked_devices_are_refused() {
        let (app, port) = start().await;
        let sk = SigningKey::from_bytes(&[5u8; 32]);
        pair_v2(&app, port, &sk, "dev1").await;
        let stranger = SigningKey::from_bytes(&[6u8; 32]);
        assert_eq!(login(port, &stranger, "dev1").await["err"], "bad token");
        assert_eq!(login(port, &sk, "nobody").await["err"], "revoked");
        assert!(app.auth.revoke("dev1"));
        assert_eq!(login(port, &sk, "dev1").await["err"], "revoked");
    }

    #[tokio::test]
    async fn turning_v1_off_refuses_old_phones_but_not_key_based_ones() {
        let (app, port) = start().await;
        let sk = SigningKey::from_bytes(&[5u8; 32]);
        pair_v2(&app, port, &sk, "dev1").await;
        app.auth.set_v1_allowed(false);

        let mut old = connect(port).await;
        send(&mut old, serde_json::json!({"t":"auth","token":"anything","device":"dev1"})).await;
        assert_eq!(next(&mut old, "auth").await["err"], "v2_required");

        let mut old_pair = connect(port).await;
        send(&mut old_pair, serde_json::json!({"t":"pair","code":app.auth.code(),"device":"old1","name":"Old"})).await;
        assert_eq!(next(&mut old_pair, "auth").await["err"], "v2_required");

        assert_eq!(login(port, &sk, "dev1").await["ok"], true);
    }

    // ---- the same protocol over TLS with a pinned key ----

    /// A client that trusts exactly one public key (by fingerprint) and nothing else, like the phone app.
    #[derive(Debug)]
    struct PinVerifier {
        fingerprint: String,
        provider: Arc<tokio_rustls::rustls::crypto::CryptoProvider>,
    }

    impl tokio_rustls::rustls::client::danger::ServerCertVerifier for PinVerifier {
        fn verify_server_cert(
            &self,
            end_entity: &tokio_rustls::rustls::pki_types::CertificateDer<'_>,
            _intermediates: &[tokio_rustls::rustls::pki_types::CertificateDer<'_>],
            _server_name: &tokio_rustls::rustls::pki_types::ServerName<'_>,
            _ocsp: &[u8],
            _now: tokio_rustls::rustls::pki_types::UnixTime,
        ) -> Result<tokio_rustls::rustls::client::danger::ServerCertVerified, tokio_rustls::rustls::Error> {
            match crate::tls::spki_from_cert(end_entity.as_ref()) {
                Some(spki) if crate::tls::fingerprint_of_spki(spki) == self.fingerprint => Ok(tokio_rustls::rustls::client::danger::ServerCertVerified::assertion()),
                _ => Err(tokio_rustls::rustls::Error::General("public key does not match the pinned fingerprint".into())),
            }
        }
        fn verify_tls12_signature(&self, m: &[u8], c: &tokio_rustls::rustls::pki_types::CertificateDer<'_>, d: &tokio_rustls::rustls::DigitallySignedStruct) -> Result<tokio_rustls::rustls::client::danger::HandshakeSignatureValid, tokio_rustls::rustls::Error> {
            tokio_rustls::rustls::crypto::verify_tls12_signature(m, c, d, &self.provider.signature_verification_algorithms)
        }
        fn verify_tls13_signature(&self, m: &[u8], c: &tokio_rustls::rustls::pki_types::CertificateDer<'_>, d: &tokio_rustls::rustls::DigitallySignedStruct) -> Result<tokio_rustls::rustls::client::danger::HandshakeSignatureValid, tokio_rustls::rustls::Error> {
            tokio_rustls::rustls::crypto::verify_tls13_signature(m, c, d, &self.provider.signature_verification_algorithms)
        }
        fn supported_verify_schemes(&self) -> Vec<tokio_rustls::rustls::SignatureScheme> {
            self.provider.signature_verification_algorithms.supported_schemes()
        }
    }

    /// Start the real listener (TLS and plain HTTP on one port). Returns the app, the port, and the pin a phone
    /// would read from the QR.
    async fn start_tls() -> (Arc<App>, u16, String) {
        use axum::serve::ListenerExt;
        let dir = std::env::temp_dir().join(format!("pr-srv-tls-{}-{}", std::process::id(), rand::random::<u32>()));
        let identity = crate::tls::load_or_create(&dir, "test-pc").unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        let cfg = test_config();
        let backend: Arc<dyn crate::backend::Backend> = Arc::new(MockBackend::new());
        let controller = Arc::new(Controller::new(backend, "host".into()));
        let app = App::with_auth(Auth::in_memory(cfg.clone()), controller, &cfg, "host".into(), vec![], None, Some(identity.fingerprint.clone()));
        let tcp = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = tcp.local_addr().unwrap().port();
        let a = app.clone();
        let listener = crate::tls::SniffListener::new(tcp, crate::tls::acceptor(&identity).unwrap(), Arc::new(move || a.auth.v1_allowed())).unwrap().tap_io(|_| {});
        let svc = router(app.clone()).into_make_service_with_connect_info::<SocketAddr>();
        tokio::spawn(async move { axum::serve(listener, svc).await });
        (app, port, identity.fingerprint)
    }

    async fn tls_connect(port: u16, pin: &str) -> Result<tokio_rustls::client::TlsStream<tokio::net::TcpStream>, std::io::Error> {
        let provider = Arc::new(tokio_rustls::rustls::crypto::ring::default_provider());
        let cfg = tokio_rustls::rustls::ClientConfig::builder_with_provider(provider.clone())
            .with_safe_default_protocol_versions()
            .unwrap()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(PinVerifier { fingerprint: pin.to_string(), provider }))
            .with_no_client_auth();
        let tcp = tokio::net::TcpStream::connect(("127.0.0.1", port)).await?;
        let name = tokio_rustls::rustls::pki_types::ServerName::try_from("localhost").unwrap();
        tokio_rustls::TlsConnector::from(Arc::new(cfg)).connect(name, tcp).await
    }

    #[tokio::test]
    async fn the_pinned_key_logs_in_over_wss_and_a_wrong_pin_is_refused() {
        let (app, port, pin) = start_tls().await;
        let sk = SigningKey::from_bytes(&[5u8; 32]);

        // Pair over plain HTTP from this computer (always allowed), exactly as the dashboard-side flow would.
        pair_v2(&app, port, &sk, "dev1").await;

        // Log in over TLS, trusting only the fingerprint from the QR.
        let tls = tls_connect(port, &pin).await.expect("the right pin must be accepted");
        let (mut ws, _) = tokio_tungstenite::client_async(format!("wss://127.0.0.1:{port}/ws"), tls).await.unwrap();
        ws.send(WsMsg::text(serde_json::json!({"t":"challenge","device":"dev1"}).to_string())).await.unwrap();
        let nonce = loop {
            let m = ws.next().await.unwrap().unwrap();
            if let WsMsg::Text(t) = m {
                let v: serde_json::Value = serde_json::from_str(t.as_str()).unwrap();
                if v["t"] == "challenge" {
                    break v["nonce"].as_str().unwrap().to_string();
                }
            }
        };
        ws.send(WsMsg::text(serde_json::json!({"t":"auth_sig","device":"dev1","sig":sign(&sk, "pc-test", "dev1", &nonce)}).to_string())).await.unwrap();
        let auth = loop {
            if let WsMsg::Text(t) = ws.next().await.unwrap().unwrap() {
                let v: serde_json::Value = serde_json::from_str(t.as_str()).unwrap();
                if v["t"] == "auth" {
                    break v;
                }
            }
        };
        assert_eq!(auth["ok"], true);

        // Someone pretending to be this computer (a different key) is refused before any data is sent.
        let wrong = crate::tls::fingerprint_of_spki(b"some other key");
        assert!(tls_connect(port, &wrong).await.is_err(), "a different fingerprint must fail the handshake");
    }

    #[tokio::test]
    async fn the_qr_carries_the_fingerprint() {
        let (app, _port, pin) = start_tls().await;
        // no LAN addresses in the test app, so it falls back to 127.0.0.1
        let url = app.pair_urls().remove(0);
        assert!(url.starts_with("http://127.0.0.1:1/#k="), "{url}");
        assert!(url.ends_with(&format!("&fp={pin}")), "{url}");
    }

    #[tokio::test]
    async fn an_invalid_public_key_is_refused_at_pairing() {
        let (app, port) = start().await;
        let mut c = connect(port).await;
        send(&mut c, serde_json::json!({"t":"pair","code":app.auth.code(),"device":"d","name":"x","pk":"AAAA","platform":"web"})).await;
        assert_eq!(next(&mut c, "pair").await["status"], "bad_key");
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
