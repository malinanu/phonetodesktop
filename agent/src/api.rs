//! JSON API behind the desktop dashboard. Every route is loopback-only (see `server::is_local`);
//! state-changing routes also need a custom header, which a foreign web page cannot send
//! cross-origin without a CORS preflight that we never grant.

use crate::backend::{mpc, mpv, setup, vlc};
use crate::server::{is_local, qr_svg, App};
use axum::{
    extract::{ConnectInfo, Path, State},
    http::{HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use serde_json::{json, Value};
use std::{net::SocketAddr, sync::Arc};

type Ctx = (ConnectInfo<SocketAddr>, HeaderMap, State<Arc<App>>);

pub fn routes() -> Router<Arc<App>> {
    Router::new()
        .route("/api/overview", get(overview))
        .route("/api/qr", get(qr))
        .route("/api/qr/refresh", post(qr_refresh))
        .route("/api/pending/{id}/{verb}", post(decide))
        .route("/api/devices/{id}", delete(remove_device))
        .route("/api/devices/{id}/input", post(set_input))
        .route("/api/setup-done", post(setup_done))
        .route("/api/legacy", post(set_legacy))
        .route("/api/allow-v1", post(set_allow_v1))
        .route("/api/unpair-all", post(unpair_all))
        .route("/api/players", get(players))
        .route("/api/players/setup", post(players_setup))
        .route("/api/autostart", post(set_autostart))
        .route("/api/open-files", post(open_files))
        .route("/api/log", get(log_tail))
}

/// Gate shared by every handler. `write` routes additionally require the custom header.
fn guard(ctx: &Ctx, method: &Method) -> Result<Arc<App>, Response> {
    let (ConnectInfo(peer), headers, State(app)) = ctx;
    if !is_local(peer, headers, app.port) {
        return Err(StatusCode::FORBIDDEN.into_response());
    }
    let write = method != Method::GET;
    if write && headers.get("x-requested-with").and_then(|v| v.to_str().ok()) != Some("phone-remote") {
        return Err(StatusCode::FORBIDDEN.into_response());
    }
    Ok(app.clone())
}

fn ok() -> Response {
    Json(json!({"ok": true})).into_response()
}

async fn overview(c: ConnectInfo<SocketAddr>, h: HeaderMap, s: State<Arc<App>>) -> Response {
    let app = match guard(&(c, h, s), &Method::GET) {
        Ok(a) => a,
        Err(r) => return r,
    };
    let ctl = app.controller.clone();
    let now = tokio::task::spawn_blocking(move || ctl.state().ok()).await.ok().flatten();
    let devices: Vec<Value> = app
        .auth
        .devices()
        .into_iter()
        .map(|(d, online)| json!({"id": d.id, "name": d.name, "created": d.created, "last_seen": d.last_seen, "online": online, "input": d.input_allowed, "platform": d.platform, "v": if d.pubkey.is_empty() { 1 } else { 2 }}))
        .collect();
    Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "host": app.host,
        "port": app.port,
        "ips": app.ips.iter().map(|i| i.to_string()).collect::<Vec<_>>(),
        "legacy": app.auth.legacy_enabled(),
        "allow_v1": app.auth.v1_allowed(),
        "setup_done": app.auth.setup_done(),
        "autostart": autostart_enabled(),
        "files_url": app.files_url,
        "phones_online": app.auth.online_count(),
        "restarts": std::env::var("PR_RESTARTS").ok().and_then(|v| v.parse::<u32>().ok()).unwrap_or(0),
        "last_exit": std::env::var("PR_LAST_EXIT").unwrap_or_default(),
        "devices": devices,
        "pending": app.auth.pending(),
        "now": now,
    }))
    .into_response()
}

async fn qr(c: ConnectInfo<SocketAddr>, h: HeaderMap, s: State<Arc<App>>) -> Response {
    let app = match guard(&(c, h, s), &Method::GET) {
        Ok(a) => a,
        Err(r) => return r,
    };
    let urls = app.pair_urls();
    let items: Vec<Value> = urls.iter().map(|u| json!({"url": u, "svg": qr_svg(u, 320)})).collect();
    Json(json!({"codes": items, "age_s": app.auth.code_age()})).into_response()
}

async fn qr_refresh(c: ConnectInfo<SocketAddr>, h: HeaderMap, s: State<Arc<App>>) -> Response {
    let app = match guard(&(c, h, s), &Method::POST) {
        Ok(a) => a,
        Err(r) => return r,
    };
    app.auth.refresh_code();
    ok()
}

async fn decide(c: ConnectInfo<SocketAddr>, h: HeaderMap, s: State<Arc<App>>, Path((id, verb)): Path<(String, String)>) -> Response {
    let app = match guard(&(c, h, s), &Method::POST) {
        Ok(a) => a,
        Err(r) => return r,
    };
    if app.auth.decide(&id, verb == "approve") {
        ok()
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}

async fn remove_device(c: ConnectInfo<SocketAddr>, h: HeaderMap, s: State<Arc<App>>, Path(id): Path<String>) -> Response {
    let app = match guard(&(c, h, s), &Method::DELETE) {
        Ok(a) => a,
        Err(r) => return r,
    };
    if app.auth.revoke(&id) {
        crate::log::log(&format!("device removed: {id}"));
        ok()
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}

async fn set_input(c: ConnectInfo<SocketAddr>, h: HeaderMap, s: State<Arc<App>>, Path(id): Path<String>, Json(body): Json<Value>) -> Response {
    let app = match guard(&(c, h, s), &Method::POST) {
        Ok(a) => a,
        Err(r) => return r,
    };
    if app.auth.set_input_allowed(&id, body["enabled"].as_bool().unwrap_or(false)) {
        ok()
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}

async fn setup_done(c: ConnectInfo<SocketAddr>, h: HeaderMap, s: State<Arc<App>>) -> Response {
    let app = match guard(&(c, h, s), &Method::POST) {
        Ok(a) => a,
        Err(r) => return r,
    };
    app.auth.mark_setup_done();
    ok()
}

async fn set_legacy(c: ConnectInfo<SocketAddr>, h: HeaderMap, s: State<Arc<App>>, Json(body): Json<Value>) -> Response {
    let app = match guard(&(c, h, s), &Method::POST) {
        Ok(a) => a,
        Err(r) => return r,
    };
    app.auth.set_legacy(body["enabled"].as_bool().unwrap_or(false));
    ok()
}

/// Turn the older bearer-token login (protocol v1, plain HTTP) on or off. Off = only key-based phones may connect.
async fn set_allow_v1(c: ConnectInfo<SocketAddr>, h: HeaderMap, s: State<Arc<App>>, Json(body): Json<Value>) -> Response {
    let app = match guard(&(c, h, s), &Method::POST) {
        Ok(a) => a,
        Err(r) => return r,
    };
    app.auth.set_v1_allowed(body["enabled"].as_bool().unwrap_or(true));
    ok()
}

async fn unpair_all(c: ConnectInfo<SocketAddr>, h: HeaderMap, s: State<Arc<App>>) -> Response {
    let app = match guard(&(c, h, s), &Method::POST) {
        Ok(a) => a,
        Err(r) => return r,
    };
    app.auth.revoke_all();
    crate::log::log("all phones unpaired");
    ok()
}

async fn players(c: ConnectInfo<SocketAddr>, h: HeaderMap, s: State<Arc<App>>) -> Response {
    let app = match guard(&(c, h, s), &Method::GET) {
        Ok(a) => a,
        Err(r) => return r,
    };
    let pw = app.vlc_password.clone();
    let rows = tokio::task::spawn_blocking(move || {
        let cfg = setup::configured();
        let status = |running: bool, configured: bool| if running { "ready" } else if configured { "restart" } else { "off" };
        json!([
            {"name": "VLC", "status": status(vlc::status(&pw).is_some(), cfg.vlc), "note": "Web interface"},
            {"name": "mpv", "status": status(mpv::status().is_some(), cfg.mpv), "note": "JSON IPC"},
            {"name": "MPC-HC / MPC-BE", "status": status(mpc::status(mpc::DEFAULT_PORT).is_some(), cfg.mpc), "note": "Web interface"},
        ])
    })
    .await
    .unwrap_or(json!([]));
    Json(rows).into_response()
}

async fn players_setup(c: ConnectInfo<SocketAddr>, h: HeaderMap, s: State<Arc<App>>) -> Response {
    let app = match guard(&(c, h, s), &Method::POST) {
        Ok(a) => a,
        Err(r) => return r,
    };
    let pw = app.vlc_password.clone();
    let lines = tokio::task::spawn_blocking(move || setup::apply(&pw)).await.unwrap_or_default();
    Json(json!({"lines": lines})).into_response()
}

async fn set_autostart(c: ConnectInfo<SocketAddr>, h: HeaderMap, s: State<Arc<App>>, Json(body): Json<Value>) -> Response {
    let app = match guard(&(c, h, s), &Method::POST) {
        Ok(a) => a,
        Err(r) => return r,
    };
    let _ = app;
    let on = body["enabled"].as_bool().unwrap_or(false);
    #[cfg(windows)]
    crate::tray::set_autostart(on);
    #[cfg(unix)]
    if let Err(e) = crate::platform::set_autostart(on) {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"ok": false, "error": e.to_string()}))).into_response();
    }
    let _ = on;
    ok()
}

/// Open the configured "Send files" page in the default browser. Takes no input: only the address
/// from the config is ever opened, and `config::clean_files_url` has already limited it to https plus
/// characters that are inert for `cmd /c start`.
async fn open_files(c: ConnectInfo<SocketAddr>, h: HeaderMap, s: State<Arc<App>>) -> Response {
    let app = match guard(&(c, h, s), &Method::POST) {
        Ok(a) => a,
        Err(r) => return r,
    };
    match &app.files_url {
        Some(url) => {
            crate::open_url(url);
            ok()
        }
        None => (StatusCode::NOT_FOUND, Json(json!({"ok": false, "error": "no file server configured"}))).into_response(),
    }
}

async fn log_tail(c: ConnectInfo<SocketAddr>, h: HeaderMap, s: State<Arc<App>>) -> Response {
    if let Err(r) = guard(&(c, h, s), &Method::GET) {
        return r;
    }
    let text = std::fs::read_to_string(crate::log::path()).unwrap_or_default();
    let lines: Vec<&str> = text.lines().rev().take(200).collect();
    Json(json!({"lines": lines})).into_response()
}

fn autostart_enabled() -> bool {
    #[cfg(windows)]
    return crate::tray::autostart_enabled();
    #[cfg(unix)]
    return crate::platform::autostart_enabled();
    #[cfg(not(any(windows, unix)))]
    false
}
