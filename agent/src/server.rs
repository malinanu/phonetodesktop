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
use std::{net::SocketAddr, sync::{Arc, Mutex}, time::{Duration, Instant}};
use tokio::sync::{watch, Notify};

const INDEX: &str = include_str!("../web/index.html");
// Shared with the Android app (bundled there as assets).
const GUIDE: &str = include_str!("../../shared/guide.html");
const BASE_CSS: &str = include_str!("../../shared/base.css");
const FONT: &[u8] = include_bytes!("../../shared/font.woff2");

pub struct App {
    pub controller: Arc<Controller>,
    pub token: String,
    pub pair_urls: Vec<String>,
    tx: watch::Sender<String>,
    notify: Notify,
    fails: Mutex<(u32, Instant)>,
}

impl App {
    pub fn new(controller: Arc<Controller>, token: String, pair_urls: Vec<String>) -> Arc<Self> {
        Arc::new(App {
            controller,
            token,
            pair_urls,
            tx: watch::channel(String::new()).0,
            notify: Notify::new(),
            fails: Mutex::new((0, Instant::now())),
        })
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
}

pub fn router(app: Arc<App>) -> Router {
    Router::new()
        .route("/", get(|| async { Html(INDEX) }))
        .route("/ws", get(ws_upgrade))
        .route("/pair", get(pair_page))
        .route("/debug", get(debug_page))
        .route("/guide", get(|| async { Html(GUIDE) }))
        .route("/base.css", get(|| async { ([(header::CONTENT_TYPE, "text/css; charset=utf-8"), (header::CACHE_CONTROL, "max-age=3600")], BASE_CSS) }))
        .route("/font.woff2", get(|| async { ([(header::CONTENT_TYPE, "font/woff2"), (header::CACHE_CONTROL, "max-age=31536000, immutable")], FONT) }))
        .route("/health", get(|| async { "ok" }))
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

/// Shows the pairing QR; only reachable from the desktop itself so the token never
/// travels over the network in cleartext except inside the QR the user scans.
async fn pair_page(ConnectInfo(peer): ConnectInfo<SocketAddr>, State(app): State<Arc<App>>) -> Response {
    if !peer.ip().is_loopback() {
        return StatusCode::FORBIDDEN.into_response();
    }
    let qr = |url: &str| {
        qrcode::QrCode::new(url.as_bytes())
            .map(|c| c.render::<qrcode::render::svg::Color>().min_dimensions(340, 340).quiet_zone(true).build())
            .unwrap_or_default()
    };
    let main = app.pair_urls.first().cloned().unwrap_or_default();
    let others: String = app
        .pair_urls
        .iter()
        .skip(1)
        .map(|u| format!("<div class=qr>{}</div><code>{u}</code>", qr(u)))
        .collect();
    let more = if others.is_empty() {
        String::new()
    } else {
        format!("<details><summary>Phone can't connect? Try another network address</summary>{others}</details>")
    };
    let body = PAIR_PAGE
        .replace("{{QR}}", &qr(&main))
        .replace("{{URL}}", &main)
        .replace("{{MORE}}", &more);
    ([(header::CACHE_CONTROL, "no-store")], Html(body)).into_response()
}

const PAIR_PAGE: &str = include_str!("../web/pair.html");

/// Loopback-only diagnostics: what the agent can see right now, and why sessions may be missing.
async fn debug_page(ConnectInfo(peer): ConnectInfo<SocketAddr>, State(app): State<Arc<App>>) -> Response {
    if !peer.ip().is_loopback() {
        return StatusCode::FORBIDDEN.into_response();
    }
    let c = app.controller.clone();
    let text = tokio::task::spawn_blocking(move || c.debug()).await.unwrap_or_default();
    ([(header::CONTENT_TYPE, "text/plain; charset=utf-8"), (header::CACHE_CONTROL, "no-store")], text).into_response()
}

async fn ws_upgrade(ws: WebSocketUpgrade, headers: HeaderMap, State(app): State<Arc<App>>) -> Response {
    // Block cross-site WebSocket hijacking: a browser Origin must match our own Host.
    if let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
        let host = headers.get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or("");
        let auth = origin.split("://").nth(1).unwrap_or("");
        if auth != host {
            return StatusCode::FORBIDDEN.into_response();
        }
    }
    ws.on_upgrade(move |s| session(s, app))
}

fn ct_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut diff = (a.len() ^ b.len()) as u8;
    for i in 0..a.len().max(b.len()) {
        diff |= a.get(i).copied().unwrap_or(0) ^ b.get(i).copied().unwrap_or(0);
    }
    diff == 0
}

fn text(s: impl Into<String>) -> Message {
    Message::Text(s.into().into())
}

async fn session(mut sock: WebSocket, app: Arc<App>) {
    if !authenticate(&mut sock, &app).await {
        return;
    }
    let mut rx = app.tx.subscribe();
    crate::log::log("phone connected");
    let _ = sock.send(text(r#"{"t":"auth","ok":true}"#)).await;
    app.notify.notify_one();
    let initial = rx.borrow().clone();
    if !initial.is_empty() {
        let _ = sock.send(text(initial)).await;
    }
    loop {
        tokio::select! {
            changed = rx.changed() => {
                if changed.is_err() { break; }
                let s = rx.borrow_and_update().clone();
                if sock.send(text(s)).await.is_err() { break; }
            }
            msg = sock.recv() => {
                let Some(Ok(msg)) = msg else { break };
                let Message::Text(t) = msg else { if matches!(msg, Message::Close(_)) { break } else { continue } };
                match serde_json::from_str::<ClientMsg>(t.as_str()) {
                    Ok(ClientMsg::Cmd(cmd)) => {
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
    crate::log::log("phone disconnected");
}

async fn authenticate(sock: &mut WebSocket, app: &App) -> bool {
    let first = tokio::time::timeout(Duration::from_secs(5), sock.recv()).await;
    let token = match first {
        Ok(Some(Ok(Message::Text(t)))) => match serde_json::from_str::<ClientMsg>(t.as_str()) {
            Ok(ClientMsg::Auth { token }) => token,
            _ => return false,
        },
        _ => return false,
    };
    if app.locked_out() {
        let _ = sock.send(text(r#"{"t":"auth","ok":false,"err":"locked"}"#)).await;
        return false;
    }
    if !ct_eq(&token, &app.token) {
        app.record_fail();
        let _ = sock.send(text(r#"{"t":"auth","ok":false,"err":"bad token"}"#)).await;
        return false;
    }
    true
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
