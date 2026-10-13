//! "Send files", built in. The computer serves the Send files web app (the FileSync page) and its tiny
//! signaling relay on the local network, so nothing has to be hosted anywhere: open it on this computer and
//! on the phone, share the room link or QR code, and files go straight between the two devices over WebRTC.
//! The relay only passes connection set-up messages; file bytes never touch this program.
//!
//! Same rules as the hosted FileSync server (`filesync/api/signaling.py`), minus TURN relaying, which only
//! matters between different networks.

use crate::net::is_lan;
use crate::server::App;
use axum::{
    extract::{
        ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade},
        ConnectInfo, Request, State,
    },
    http::{header, StatusCode, Uri},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{atomic::Ordering, Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::mpsc;

include!(concat!(env!("OUT_DIR"), "/files_assets.rs"));

const MAX_PAYLOAD: usize = 32 * 1024;
const MAX_MSG_PER_SECOND: usize = 100;
const PAIR_WINDOW: Duration = Duration::from_secs(10);
const PAIR_MAX: usize = 50;
const REGISTER_TIMEOUT: Duration = Duration::from_secs(10);
const IDLE_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_CONNECTIONS: usize = 200;

const CLOSE_INVALID_REGISTER: u16 = 4400;
const CLOSE_UNAVAILABLE_ID: u16 = 4409;
const CLOSE_RATE_LIMITED: u16 = 4429;
const CLOSE_IDLE: u16 = 4408;

const CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; connect-src 'self' ws: wss: stun: turn: turns:; worker-src 'self'; frame-src 'self'; object-src 'none'; base-uri 'self'; form-action 'self'; frame-ancestors 'self'";

/// What the relay asks a connection to do.
#[derive(Debug, PartialEq)]
pub enum Out {
    Text(String),
    Close(u16, &'static str),
}

/// peer id -> its outgoing queue. The newest registration for an id wins (a stale socket after a network
/// blip must not lock the device out), exactly as in the hosted server.
#[derive(Default)]
pub struct Relay {
    peers: Mutex<HashMap<String, (u64, mpsc::UnboundedSender<Out>)>>,
    next: Mutex<u64>,
}

pub fn valid_peer_id(s: &str) -> bool {
    (1..=64).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

impl Relay {
    pub fn len(&self) -> usize {
        self.peers.lock().unwrap().len()
    }

    /// Claim `id`. Returns this registration's token; any earlier holder is told to close.
    pub fn register(&self, id: &str, tx: mpsc::UnboundedSender<Out>) -> u64 {
        let token = {
            let mut n = self.next.lock().unwrap();
            *n += 1;
            *n
        };
        if let Some((_, old)) = self.peers.lock().unwrap().insert(id.to_string(), (token, tx)) {
            let _ = old.send(Out::Close(CLOSE_UNAVAILABLE_ID, "Replaced by a new registration."));
        }
        token
    }

    /// Remove `id` only if it still belongs to this registration.
    pub fn unregister(&self, id: &str, token: u64) {
        let mut p = self.peers.lock().unwrap();
        if p.get(id).is_some_and(|(t, _)| *t == token) {
            p.remove(id);
        }
    }

    /// Pass a signal to `to`. False when that peer is not connected.
    pub fn forward(&self, from: &str, to: &str, payload: Value) -> bool {
        let msg = json!({"type": "signal", "from": from, "payload": payload}).to_string();
        self.peers.lock().unwrap().get(to).is_some_and(|(_, tx)| tx.send(Out::Text(msg)).is_ok())
    }
}

/// One connection's rate-limit state.
#[derive(Default)]
pub struct Limits {
    recent: Vec<Instant>,
    pair: HashMap<String, Vec<Instant>>,
}

impl Limits {
    /// Count a message. False when the connection sends more than `MAX_MSG_PER_SECOND`.
    pub fn message(&mut self, now: Instant) -> bool {
        self.recent.push(now);
        self.recent.retain(|t| now.duration_since(*t) < Duration::from_secs(1));
        self.recent.len() <= MAX_MSG_PER_SECOND
    }

    /// Count a signal to `target`. False when this source is flooding that target.
    pub fn signal(&mut self, target: &str, now: Instant) -> bool {
        let v = self.pair.entry(target.to_string()).or_default();
        v.retain(|t| now.duration_since(*t) < PAIR_WINDOW);
        if v.len() >= PAIR_MAX {
            return false;
        }
        v.push(now);
        if self.pair.len() > 256 {
            self.pair.retain(|_, v| !v.is_empty());
        }
        true
    }
}

fn err(code: &str, message: &str) -> Out {
    Out::Text(json!({"type": "error", "code": code, "message": message}).to_string())
}

/// What to do with one message from a registered peer: replies to send back to it, and whether it must hang up.
pub fn handle_message(relay: &Relay, me: &str, raw: &str, limits: &mut Limits, now: Instant) -> (Vec<Out>, bool) {
    if !limits.message(now) {
        return (vec![err("rate-limited", "Too many messages."), Out::Close(CLOSE_RATE_LIMITED, "Rate limited.")], true);
    }
    if raw.len() > MAX_PAYLOAD {
        return (vec![err("invalid-message", "Message too large.")], false);
    }
    let Ok(msg) = serde_json::from_str::<Value>(raw) else { return (vec![err("invalid-message", "Malformed JSON.")], false) };
    let Some(obj) = msg.as_object() else { return (vec![err("invalid-message", "Message must be an object.")], false) };
    match obj.get("type").and_then(Value::as_str) {
        Some("ping") => (vec![Out::Text(json!({"type": "pong"}).to_string())], false),
        Some("signal") => {
            let Some(to) = obj.get("to").and_then(Value::as_str).filter(|t| valid_peer_id(t)) else {
                return (vec![err("invalid-message", "'to' must be a valid peer id.")], false);
            };
            if !limits.signal(to, now) {
                return (vec![err("rate-limited", &format!("Too many signals to peer {to:?}."))], false);
            }
            let payload = obj.get("payload").cloned().unwrap_or(Value::Null);
            if relay.forward(me, to, payload) {
                (vec![], false)
            } else {
                (vec![Out::Text(json!({"type": "peer-unavailable", "id": to}).to_string())], false)
            }
        }
        other => (vec![err("invalid-message", &format!("Unknown type: {other:?}."))], false),
    }
}

// ---- HTTP ----

#[derive(Clone)]
pub struct Files {
    app: Arc<App>,
    relay: Arc<Relay>,
}

pub fn router(app: Arc<App>) -> Router {
    let st = Files { app, relay: Arc::new(Relay::default()) };
    Router::new()
        .route("/ws", get(ws_upgrade))
        .route("/api/uuid", get(uuid))
        .route("/api/share-origin", get(share_origin))
        .route("/api/credentials", get(credentials))
        .route("/api/health", get(|| async { Json(json!({"message": "Send files is running"})) }))
        .fallback(asset)
        .layer(middleware::from_fn_with_state(st.clone(), gate))
        .with_state(st)
}

/// Only the home network, and only while the owner has Send files switched on.
async fn gate(State(st): State<Files>, ConnectInfo(peer): ConnectInfo<SocketAddr>, req: Request, next: Next) -> Response {
    if !is_lan(peer.ip()) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if !st.app.files_enabled.load(Ordering::Relaxed) {
        return (StatusCode::SERVICE_UNAVAILABLE, "Send files is switched off on this computer.").into_response();
    }
    next.run(req).await
}

fn headers(content_type: &'static str, path: &str) -> [(header::HeaderName, &'static str); 7] {
    let cache = if path.ends_with(".woff2") || path.ends_with(".png") { "public, max-age=604800" } else { "no-cache" };
    [
        (header::CONTENT_TYPE, content_type),
        (header::CACHE_CONTROL, cache),
        (header::CONTENT_SECURITY_POLICY, CSP),
        (header::X_FRAME_OPTIONS, "SAMEORIGIN"),
        (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        (header::REFERRER_POLICY, "same-origin"),
        (header::HeaderName::from_static("permissions-policy"), "camera=(), microphone=(), geolocation=(), payment=(), usb=()"),
    ]
}

async fn asset(uri: Uri) -> Response {
    let path = if uri.path() == "/" { "/index.html" } else { uri.path() };
    let found = ASSETS.iter().find(|(p, _, _)| *p == path).or_else(|| {
        // A room link such as /abc-defg-hij is the app itself; only things that look like files are 404s.
        let last = path.rsplit('/').next().unwrap_or("");
        (!last.contains('.') && !path.starts_with("/api/") && !path.starts_with("/__download/")).then(|| ASSETS.iter().find(|(p, _, _)| *p == "/index.html")).flatten()
    });
    match found {
        Some((p, ct, body)) => (headers(ct, p), *body).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// Which address goes into the room link and QR code. Opened on this computer the page says "localhost",
/// which a phone cannot reach, so give the address of this computer on the home network instead.
pub fn share_origin_for(host_header: Option<&str>, ips: &[std::net::Ipv4Addr], port: u16) -> String {
    let host = host_header.unwrap_or("").rsplit_once(':').map(|(h, _)| h).unwrap_or(host_header.unwrap_or(""));
    let loopback = host.is_empty() || host == "localhost" || host.starts_with("127.") || host == "[::1]" || host == "::1";
    if loopback {
        let ip = ips.iter().find(|i| !i.is_loopback()).copied().unwrap_or(std::net::Ipv4Addr::LOCALHOST);
        format!("http://{ip}:{port}")
    } else {
        format!("http://{host}:{port}")
    }
}

async fn share_origin(State(st): State<Files>, headers: axum::http::HeaderMap) -> Json<Value> {
    let host = headers.get(header::HOST).and_then(|v| v.to_str().ok());
    let port = st.app.files_port.load(Ordering::Relaxed);
    Json(json!({"origin": share_origin_for(host, &st.app.ips, port)}))
}

async fn uuid() -> Json<Value> {
    use rand::RngCore;
    let mut b = [0u8; 16];
    rand::rng().fill_bytes(&mut b);
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    Json(json!({"uuid": format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])}))
}

/// The page asks for TURN credentials. On the home network there is no TURN server (devices connect
/// directly), so this hands back a token of the right shape that simply leads nowhere.
async fn credentials() -> Json<Value> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    let exp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) + 300;
    let head = URL_SAFE_NO_PAD.encode(br#"{"alg":"none","typ":"JWT"}"#);
    let body = URL_SAFE_NO_PAD.encode(json!({"username": "local", "credential": "local", "exp": exp}).to_string());
    Json(json!({"token": format!("{head}.{body}.")}))
}

async fn ws_upgrade(State(st): State<Files>, ws: WebSocketUpgrade) -> Response {
    if st.relay.len() >= MAX_CONNECTIONS {
        return (StatusCode::SERVICE_UNAVAILABLE, "Too many connections").into_response();
    }
    ws.max_message_size(MAX_PAYLOAD * 2).on_upgrade(move |sock| signaling(sock, st.relay))
}

async fn close(sock: &mut WebSocket, code: u16, reason: &'static str) {
    let _ = sock.send(Message::Close(Some(CloseFrame { code, reason: reason.into() }))).await;
}

async fn text_within(sock: &mut WebSocket, wait: Duration) -> Option<String> {
    let deadline = tokio::time::Instant::now() + wait;
    loop {
        match tokio::time::timeout_at(deadline, sock.recv()).await {
            Ok(Some(Ok(Message::Text(t)))) => return Some(t.as_str().to_owned()),
            Ok(Some(Ok(Message::Ping(_) | Message::Pong(_)))) => continue,
            _ => return None,
        }
    }
}

async fn signaling(mut sock: WebSocket, relay: Arc<Relay>) {
    // 1. register
    let Some(raw) = text_within(&mut sock, REGISTER_TIMEOUT).await else { return close(&mut sock, CLOSE_INVALID_REGISTER, "Register timeout.").await };
    let id = serde_json::from_str::<Value>(&raw)
        .ok()
        .filter(|v| v.get("type").and_then(Value::as_str) == Some("register"))
        .and_then(|v| v.get("id").and_then(Value::as_str).map(str::to_owned))
        .filter(|id| valid_peer_id(id));
    let Some(id) = id else {
        let _ = sock.send(Message::Text(json!({"type": "error", "code": "invalid-id", "message": "Peer id format is invalid."}).to_string().into())).await;
        return close(&mut sock, CLOSE_INVALID_REGISTER, "Invalid register.").await;
    };
    let (tx, mut rx) = mpsc::unbounded_channel();
    let token = relay.register(&id, tx);
    let _ = sock.send(Message::Text(json!({"type": "registered", "id": id}).to_string().into())).await;

    // 2. relay
    let mut limits = Limits::default();
    'conn: loop {
        tokio::select! {
            out = rx.recv() => match out {
                Some(Out::Text(t)) => { if sock.send(Message::Text(t.into())).await.is_err() { break; } }
                Some(Out::Close(code, why)) => { close(&mut sock, code, why).await; break; }
                None => break,
            },
            incoming = text_within(&mut sock, IDLE_TIMEOUT) => match incoming {
                None => { close(&mut sock, CLOSE_IDLE, "Idle timeout.").await; break; }
                Some(raw) => {
                    let (replies, hang_up) = handle_message(&relay, &id, &raw, &mut limits, Instant::now());
                    for r in replies {
                        match r {
                            Out::Text(t) => { if sock.send(Message::Text(t.into())).await.is_err() { break 'conn; } }
                            Out::Close(code, why) => { close(&mut sock, code, why).await; break 'conn; }
                        }
                    }
                    if hang_up { break; }
                }
            },
        }
    }
    relay.unregister(&id, token);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer(relay: &Relay, id: &str) -> (u64, mpsc::UnboundedReceiver<Out>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (relay.register(id, tx), rx)
    }

    fn send(relay: &Relay, from: &str, raw: &str, l: &mut Limits) -> (Vec<Out>, bool) {
        handle_message(relay, from, raw, l, Instant::now())
    }

    #[test]
    fn signals_reach_only_the_named_peer_and_unknown_peers_are_reported() {
        let r = Relay::default();
        let (_a, mut rx_a) = peer(&r, "alice");
        let (_b, mut rx_b) = peer(&r, "bob");
        let mut l = Limits::default();
        let (replies, hang_up) = send(&r, "alice", r#"{"type":"signal","to":"bob","payload":{"kind":"offer","sdp":"x"}}"#, &mut l);
        assert!(replies.is_empty() && !hang_up);
        let Some(Out::Text(t)) = rx_b.try_recv().ok() else { panic!("bob got nothing") };
        let v: Value = serde_json::from_str(&t).unwrap();
        assert_eq!((v["type"].as_str(), v["from"].as_str(), v["payload"]["kind"].as_str()), (Some("signal"), Some("alice"), Some("offer")));
        assert!(rx_a.try_recv().is_err(), "the sender does not get its own signal back");
        let (replies, _) = send(&r, "alice", r#"{"type":"signal","to":"nobody","payload":1}"#, &mut l);
        assert_eq!(replies, vec![Out::Text(json!({"type":"peer-unavailable","id":"nobody"}).to_string())]);
    }

    #[test]
    fn a_new_registration_takes_over_and_the_old_one_cannot_remove_it() {
        let r = Relay::default();
        let (old_token, mut old_rx) = peer(&r, "phone");
        let (new_token, mut new_rx) = peer(&r, "phone");
        assert_eq!(old_rx.try_recv().ok(), Some(Out::Close(CLOSE_UNAVAILABLE_ID, "Replaced by a new registration.")));
        r.unregister("phone", old_token);
        assert_eq!(r.len(), 1, "a displaced socket closing must not evict its replacement");
        assert!(r.forward("x", "phone", json!(1)));
        assert!(new_rx.try_recv().is_ok());
        r.unregister("phone", new_token);
        assert_eq!(r.len(), 0);
    }

    #[test]
    fn ping_pong_and_bad_input_never_panic_or_hang_up() {
        let r = Relay::default();
        let mut l = Limits::default();
        assert_eq!(send(&r, "a", r#"{"type":"ping"}"#, &mut l).0, vec![Out::Text(r#"{"type":"pong"}"#.into())]);
        for bad in ["", "not json", "[]", "42", r#"{"type":"nope"}"#, r#"{"type":"signal"}"#, r#"{"type":"signal","to":"has space","payload":1}"#, r#"{"type":"signal","to":"../etc","payload":1}"#] {
            let (replies, hang_up) = send(&r, "a", bad, &mut l);
            assert_eq!(replies.len(), 1, "{bad}");
            assert!(!hang_up, "{bad}");
        }
        let big = format!(r#"{{"type":"signal","to":"b","payload":"{}"}}"#, "x".repeat(MAX_PAYLOAD));
        assert!(matches!(send(&r, "a", &big, &mut l).0.first(), Some(Out::Text(t)) if t.contains("too large")));
    }

    #[test]
    fn flooding_is_cut_off() {
        let r = Relay::default();
        let (_b, _rx) = peer(&r, "bob");
        let now = Instant::now();
        let mut l = Limits::default();
        // Many signals to one target: refused after the per-target cap, connection stays up.
        let mut refused = 0;
        for _ in 0..PAIR_MAX + 5 {
            let (replies, hang_up) = handle_message(&r, "a", r#"{"type":"signal","to":"bob","payload":1}"#, &mut l, now);
            assert!(!hang_up);
            if !replies.is_empty() {
                refused += 1;
            }
        }
        assert_eq!(refused, 5);
        // Too many messages in a second: the connection is closed.
        let mut l = Limits::default();
        let mut closed = false;
        for _ in 0..MAX_MSG_PER_SECOND + 2 {
            closed |= handle_message(&r, "a", r#"{"type":"ping"}"#, &mut l, now).1;
        }
        assert!(closed);
    }

    #[test]
    fn peer_ids_are_the_same_shape_the_hosted_server_accepts() {
        for ok in ["a", "d2f1c3e4-0b7a-4c1d-9e8f-123456789abc", "abc_DEF-123", &"x".repeat(64)] {
            assert!(valid_peer_id(ok), "{ok}");
        }
        for bad in ["", "has space", "a/b", "é", &"x".repeat(65), "a.b"] {
            assert!(!valid_peer_id(bad), "{bad}");
        }
    }

    #[test]
    fn the_app_is_embedded_and_room_links_load_it() {
        assert!(ASSETS.iter().any(|(p, ct, b)| *p == "/index.html" && ct.starts_with("text/html") && !b.is_empty()));
        assert!(ASSETS.iter().any(|(p, _, _)| *p == "/sw.js"), "the service worker is what streams big downloads");
        assert!(ASSETS.iter().all(|(p, _, _)| *p != "/test.html"), "developer pages stay out of the product");
    }

    #[test]
    fn the_share_link_never_says_localhost() {
        let ips = [std::net::Ipv4Addr::new(192, 168, 1, 8), std::net::Ipv4Addr::new(172, 29, 208, 1)];
        assert_eq!(share_origin_for(Some("localhost:8766"), &ips, 8766), "http://192.168.1.8:8766");
        assert_eq!(share_origin_for(Some("127.0.0.1:8766"), &ips, 8766), "http://192.168.1.8:8766");
        assert_eq!(share_origin_for(None, &ips, 8766), "http://192.168.1.8:8766");
        assert_eq!(share_origin_for(Some("192.168.1.8:8766"), &ips, 8766), "http://192.168.1.8:8766");
        assert_eq!(share_origin_for(Some("malin-pc.local:8766"), &ips, 8766), "http://malin-pc.local:8766");
        assert_eq!(share_origin_for(Some("localhost:8766"), &[], 8766), "http://127.0.0.1:8766", "no network: still a valid link");
    }
}
