//! The dashboard and approval windows: native windows around the system WebView2, so the taskbar shows the
//! Lanyard icon (not Edge's) and the pages can't be inspected with F12. `open` is false when WebView2 is missing,
//! and the caller falls back to an Edge/Chrome app window.
//!
//! All windows live on one UI thread with one event loop and one WebView2 environment. (A second event loop on a
//! second thread, for the approval window while the dashboard was open, could leave that window blank.)

use std::collections::HashMap;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy, EventLoopWindowTarget};
use tao::platform::run_return::EventLoopExtRunReturn;
use tao::platform::windows::{EventLoopBuilderExtWindows, IconExtWindows};
use tao::window::{Icon, Window, WindowBuilder, WindowId};
use wry::{NewWindowResponse, WebContext, WebView, WebViewBuilder, WebViewBuilderExtWindows};

/// Right-click menus are for developers; and `window.close()` from a page (the approval window closes itself after
/// the answer) is passed to the app, since WebView2 does not close a top-level window by itself.
const PAGE_SCRIPT: &str = "addEventListener('contextmenu', e => e.preventDefault());\
    window.close = () => window.ipc.postMessage('close');";

/// The page background while it loads (#15110e), instead of a white flash.
const BACKGROUND: (u8, u8, u8, u8) = (21, 17, 14, 255);

enum Ui {
    Open { url: String, title: String, width: u32, height: u32, on_top: bool, reply: Sender<bool> },
    Close(WindowId),
}

/// The UI thread's mailbox, once it runs; None when it could not start (no WebView2).
static UI: OnceLock<Option<Mutex<EventLoopProxy<Ui>>>> = OnceLock::new();

fn ui() -> Option<&'static Mutex<EventLoopProxy<Ui>>> {
    UI.get_or_init(|| {
        let (tx, rx) = channel();
        std::thread::Builder::new().name("lanyard-ui".into()).spawn(move || ui_thread(tx)).ok()?;
        rx.recv().ok().flatten().map(Mutex::new)
    })
    .as_ref()
}

/// Open `url` in its own window (or bring forward the one already showing that page). False when no window can be
/// made, e.g. WebView2 is missing.
pub fn open(url: &str, title: &str, width: u32, height: u32, on_top: bool) -> bool {
    let Some(proxy) = ui() else { return false };
    let (reply, answer) = channel();
    let ev = Ui::Open { url: url.into(), title: title.into(), width, height, on_top, reply };
    if proxy.lock().map(|p| p.send_event(ev).is_err()).unwrap_or(true) {
        return false;
    }
    answer.recv_timeout(Duration::from_secs(20)).unwrap_or(false)
}

struct Open {
    url: String,
    window: Window,
    _webview: WebView,
}

fn ui_thread(ready: Sender<Option<EventLoopProxy<Ui>>>) {
    let mut event_loop = EventLoopBuilder::<Ui>::with_user_event().with_any_thread(true).build();
    let proxy = event_loop.create_proxy();
    // WebView2 keeps its profile in a folder; the default (beside the exe) is not writable under Program Files.
    let data_dir = dirs::data_local_dir().unwrap_or_else(std::env::temp_dir).join("Lanyard").join("webview");
    let mut context = WebContext::new(Some(data_dir));
    let mut windows: HashMap<WindowId, Open> = HashMap::new();
    let _ = ready.send(Some(proxy.clone()));

    event_loop.run_return(move |event, target, flow| {
        *flow = ControlFlow::Wait;
        match event {
            Event::UserEvent(Ui::Open { url, title, width, height, on_top, reply }) => {
                // The same page again (the dashboard clicked twice): show the window that has it.
                if let Some(w) = windows.values().find(|w| w.url == url) {
                    w.window.set_minimized(false);
                    w.window.set_focus();
                    let _ = reply.send(true);
                    return;
                }
                let made = make(target, &mut context, &proxy, &url, &title, width, height, on_top);
                let _ = reply.send(made.is_some());
                if let Some(w) = made {
                    windows.insert(w.window.id(), w);
                }
            }
            Event::UserEvent(Ui::Close(id)) | Event::WindowEvent { window_id: id, event: WindowEvent::CloseRequested, .. } => {
                windows.remove(&id); // drops the webview, then the window
            }
            _ => {}
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn make(
    target: &EventLoopWindowTarget<Ui>,
    context: &mut WebContext,
    proxy: &EventLoopProxy<Ui>,
    url: &str,
    title: &str,
    width: u32,
    height: u32,
    on_top: bool,
) -> Option<Open> {
    let icon = Icon::from_resource(1, None).ok(); // the exe's embedded Lanyard icon
    let window = WindowBuilder::new()
        .with_title(title)
        .with_inner_size(LogicalSize::new(width as f64, height as f64))
        .with_window_icon(icon)
        .with_always_on_top(on_top)
        .build(target)
        .ok()?;
    let id = window.id();
    let closer = proxy.clone();
    let webview = WebViewBuilder::new_with_web_context(context)
        .with_url(url)
        .with_background_color(BACKGROUND)
        .with_devtools(false)
        .with_browser_accelerator_keys(false)
        .with_default_context_menus(false)
        .with_initialization_script(PAGE_SCRIPT)
        .with_ipc_handler(move |req| {
            if req.body() == "close" {
                let _ = closer.send_event(Ui::Close(id));
            }
        })
        .with_new_window_req_handler(|target, _| {
            crate::open_url(&target);
            NewWindowResponse::Deny
        })
        .build(&window)
        .ok()?;
    if on_top {
        window.set_focus();
    }
    Some(Open { url: url.to_string(), window, _webview: webview })
}
