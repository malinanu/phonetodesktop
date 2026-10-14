//! The dashboard and approval windows: a native window around the system WebView2, so the taskbar shows the
//! Lanyard icon (not Edge's) and the page can't be inspected with F12. `open` is false when WebView2 is missing,
//! and the caller falls back to an Edge/Chrome app window.

use std::sync::atomic::{AtomicBool, Ordering};
use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::platform::run_return::EventLoopExtRunReturn;
use tao::platform::windows::{EventLoopBuilderExtWindows, IconExtWindows};
use tao::window::{Icon, WindowBuilder};
use wry::{NewWindowResponse, WebContext, WebViewBuilder, WebViewBuilderExtWindows};

/// Right-click menus and F12 are for developers; the pages are ours.
const NO_INSPECT: &str = "addEventListener('contextmenu', e => e.preventDefault());";

/// True once WebView2 failed to start, so later windows go straight to the browser fallback.
static BROKEN: AtomicBool = AtomicBool::new(false);

/// Open `url` in its own window on its own thread. Returns false if the window cannot be made (no WebView2).
pub fn open(url: &str, title: &str, width: u32, height: u32, on_top: bool) -> bool {
    if BROKEN.load(Ordering::Relaxed) {
        return false;
    }
    let (url, title) = (url.to_string(), title.to_string());
    let (tx, rx) = std::sync::mpsc::channel::<bool>();
    let spawned = std::thread::Builder::new().name("lanyard-window".into()).spawn(move || {
        let ok = run(&url, &title, width, height, on_top, &tx);
        // Reached only if the window never came up, or after it closed (the send is then ignored).
        let _ = tx.send(ok);
    });
    if spawned.is_err() {
        return false;
    }
    // The first message says whether the window opened; the thread keeps running until it closes.
    let up = rx.recv().unwrap_or(false);
    if !up {
        BROKEN.store(true, Ordering::Relaxed);
    }
    up
}

fn run(url: &str, title: &str, width: u32, height: u32, on_top: bool, tx: &std::sync::mpsc::Sender<bool>) -> bool {
    let mut event_loop = EventLoopBuilder::new().with_any_thread(true).build();
    let icon = Icon::from_resource(1, None).ok(); // the exe's embedded Lanyard icon
    let Ok(window) = WindowBuilder::new()
        .with_title(title)
        .with_inner_size(LogicalSize::new(width as f64, height as f64))
        .with_window_icon(icon)
        .with_always_on_top(on_top)
        .build(&event_loop)
    else {
        return false;
    };
    // WebView2 keeps its profile in a folder; the default (beside the exe) is not writable under Program Files.
    let data_dir = dirs::data_local_dir().unwrap_or_else(std::env::temp_dir).join("Lanyard").join("webview");
    let mut context = WebContext::new(Some(data_dir));
    let webview = WebViewBuilder::new_with_web_context(&mut context)
        .with_url(url)
        .with_devtools(false)
        .with_browser_accelerator_keys(false)
        .with_default_context_menus(false)
        .with_initialization_script(NO_INSPECT)
        .with_new_window_req_handler(|target, _| {
            crate::open_url(&target);
            NewWindowResponse::Deny
        })
        .build(&window);
    let Ok(_webview) = webview else {
        return false;
    };
    let _ = tx.send(true);
    event_loop.run_return(|event, _, flow| {
        *flow = ControlFlow::Wait;
        if let Event::WindowEvent { event: WindowEvent::CloseRequested, .. } = event {
            *flow = ControlFlow::Exit;
        }
    });
    true
}
