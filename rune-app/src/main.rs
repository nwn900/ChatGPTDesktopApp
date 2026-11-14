//! ChatGPT desktop app: Rune hosts the window, Edge WebView2 shows the page.
//!
//! Rune has no webview, so the window is a Rune window (winit + wgpu) and the
//! page is a WebView2 child HWND parented to it and sized to the whole client
//! area. Rune draws no chrome: the page is the window.
//!
//! One instance runs per session. Closing the window hides it and the app keeps
//! running in the tray, so the session survives; the tray icon brings it back.
//! The tray menu also toggles launching at system startup.
//!
//! Run it with `cargo run`.

// No console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autostart;
mod links;
mod single_instance;
mod webview;

use std::cell::Cell;
use std::sync::{Arc, OnceLock};

use rune::raw_window_handle::RawWindowHandle;
use rune::tok::{self, radius, type_};
use rune::{AppSpec, Cx, El, Tray, TrayEvent, TrayIcon, TrayItem, TraySpec, Tx, col, el, text};
use windows::Win32::Foundation::HWND;

pub(crate) const TARGET_URL: &str = "https://chatgpt.com/";

/// The project's own icons, embedded so the installer ships one file.
const APP_ICON: &[u8] = include_bytes!("../../src-tauri/icons/icon.png");
const TRAY_ICON: &[u8] = include_bytes!("../../src-tauri/icons/32x32.png");

/// The live tray, so a menu pick can relabel the startup item.
static TRAY: OnceLock<Arc<Tray>> = OnceLock::new();

thread_local! {
    /// The state the view was last drawn for, so poll reports a change once.
    static DRAWN: Cell<webview::State> = const { Cell::new(webview::State::Idle) };
}

fn poll(_state: &mut (), _now: f64, _tx: &mut Tx) -> bool {
    // Cheap when the window has not moved: the module caches the rect.
    webview::sync_bounds();
    let current = webview::state();
    DRAWN.with(|drawn| {
        let previous = drawn.get();
        drawn.set(current);
        previous != current
    })
}

fn view(_state: &(), _cx: &mut Cx<()>) -> El {
    // The page is a native child window covering the whole client area, so
    // Rune draws nothing behind it — unless the page could not be created, in
    // which case the window says so instead of staying blank.
    if webview::state() == webview::State::Failed {
        return failure_view();
    }
    el().w_full().h_full()
}

/// What the window shows when WebView2 could not start. It needs no WebView2.
fn failure_view() -> El {
    let t = tok::theme(true);
    col()
        .w_full()
        .h_full()
        .center()
        .gap(14.0)
        .padding(32.0)
        .fill(t.bg)
        .child(
            text("ChatGPT could not start")
                .font(type_::TITLE)
                .color(t.text),
        )
        .child(
            text(
                "The page could not be created. Check that the WebView2 runtime is \
                 installed, then try again.",
            )
            .font(type_::BODY)
            .color(t.text_secondary),
        )
        .child(
            el().key("open-browser")
                .h(36.0)
                .px(16.0)
                .center()
                .radius(radius::SM)
                .fill(t.card)
                .on_press(|_: &mut ()| webview::open_in_browser())
                .child(text("Open in browser").font(type_::LABEL).color(t.text)),
        )
}

/// The tray menu, with the startup item showing its current state.
fn tray_menu(startup: bool) -> Vec<TrayItem> {
    vec![
        TrayItem::action("open", "Open ChatGPT"),
        TrayItem::action("reload", "Reload"),
        TrayItem::Separator,
        TrayItem::action(
            "startup",
            if startup {
                "Launch at system startup: On"
            } else {
                "Launch at system startup: Off"
            },
        ),
        TrayItem::Separator,
        TrayItem::action("quit", "Quit"),
    ]
}

/// The tray icon from the project's 32x32 PNG.
fn tray_icon() -> TrayIcon {
    if let Some(icon) = decode_icon(TRAY_ICON) {
        return icon;
    }
    // Fallback: a solid accent square, so the tray still has an icon.
    let mut rgba = Vec::with_capacity(32 * 32 * 4);
    for _ in 0..32 * 32 {
        rgba.extend_from_slice(&[59, 130, 246, 255]);
    }
    TrayIcon::rgba(32, 32, rgba)
}

fn decode_icon(bytes: &[u8]) -> Option<TrayIcon> {
    let mut decoder = png::Decoder::new(bytes);
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let mut buffer = vec![0; reader.output_buffer_size()];
    let frame = reader.next_frame(&mut buffer).ok()?;
    let rgba = match frame.color_type {
        png::ColorType::Rgba => buffer[..frame.buffer_size()].to_vec(),
        png::ColorType::Rgb => buffer[..frame.buffer_size()]
            .chunks_exact(3)
            .flat_map(|pixel| [pixel[0], pixel[1], pixel[2], 255])
            .collect(),
        _ => return None,
    };
    Some(TrayIcon::rgba(frame.width, frame.height, rgba))
}

/// `--startup on|off` sets the Run value and exits, so a script or an
/// installer can set it without going through the tray.
fn startup_flag() -> Option<bool> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--startup" {
            return match args.next().as_deref() {
                Some("on") => Some(true),
                Some("off") => Some(false),
                _ => None,
            };
        }
    }
    None
}

fn main() {
    if let Some(enabled) = startup_flag() {
        match autostart::set_enabled(enabled) {
            Ok(()) => webview::log(&format!(
                "startup {}",
                if enabled { "enabled" } else { "disabled" }
            )),
            Err(error) => {
                webview::log(&format!("startup toggle failed: {error}"));
                std::process::exit(1);
            }
        }
        return;
    }

    // An installer replacing the app asks the running one to quit first.
    if std::env::args().any(|arg| arg == "--quit") {
        single_instance::signal_quit();
        return;
    }

    // One app per session. A second launch asks the first to show itself and
    // leaves, so there is never a second tray icon or a second WebView2 tree.
    let guard = match single_instance::SingleInstance::acquire() {
        single_instance::Acquired::First(guard) => guard,
        single_instance::Acquired::AlreadyRunning => return,
    };

    // The title bar, taskbar and Alt+Tab icon, from the project's own icon.
    // Rune wants a square PNG of 256 px or more; this one is 512.
    if let Err(error) = rune::set_app_icon(APP_ICON.to_vec()) {
        webview::log(&format!("app icon rejected: {error}"));
    }

    // Started by the Run key: open in the tray, not on screen.
    let from_startup = std::env::args().any(|arg| arg == "--autostart");

    let app = rune::App::new(AppSpec {
        poll: Some(poll),
        ..AppSpec::new("ChatGPT", (1180.0, 760.0), || (), view)
    })
    .on_ready(|handle| {
        // The page is parented to the Rune window, so it needs that window's
        // native handle. WebView2 finishes setting up through the message
        // loop; nothing here waits for it.
        let _ = handle.with_raw_handle(|raw| {
            if let RawWindowHandle::Win32(win32) = raw.as_raw() {
                let hwnd = HWND(win32.hwnd.get() as *mut _);
                webview::start(hwnd, TARGET_URL);
            }
        });
    });

    // The tray runs on its own thread, so it answers while the window is
    // hidden. TRAY keeps it alive for the life of the process.
    let handle = app.handle();
    let startup = autostart::is_enabled();
    webview::log(&format!("startup at launch: {startup}"));
    let tray = match Tray::new(
        TraySpec::new("chatgpt", "ChatGPT", tray_icon())
            .tooltip("ChatGPT")
            .menu(tray_menu(startup)),
        {
            let handle = handle.clone();
            move |event| match event {
                TrayEvent::Activate { .. } => handle.open(),
                TrayEvent::Menu(id) if id == "open" => handle.open(),
                TrayEvent::Menu(id) if id == "reload" => {
                    if webview::with(|web| web.reload()).is_none() {
                        webview::log("reload ignored: no page");
                    }
                }
                TrayEvent::Menu(id) if id == "startup" => {
                    let enabled = !autostart::is_enabled();
                    match autostart::set_enabled(enabled) {
                        Ok(()) => {
                            webview::log(&format!(
                                "startup {}",
                                if enabled { "enabled" } else { "disabled" }
                            ));
                            if let Some(tray) = TRAY.get() {
                                tray.set_menu(tray_menu(enabled));
                            }
                        }
                        Err(error) => webview::log(&format!("startup toggle failed: {error}")),
                    }
                }
                TrayEvent::Menu(id) if id == "quit" => handle.quit(),
                _ => {}
            }
        },
    ) {
        Ok(tray) => {
            let tray = Arc::new(tray);
            let _ = TRAY.set(Arc::clone(&tray));
            webview::log("tray icon created");
            Some(tray)
        }
        Err(error) => {
            webview::log(&format!("tray unavailable: {error}"));
            None
        }
    };

    // Without a tray there is no way back from a hidden window, so the app
    // only hides to the tray when the tray exists.
    let app = if tray.is_some() {
        app.keep_running()
    } else {
        app
    };
    // A startup launch with no tray would be invisible, so it opens normally.
    let app = if from_startup && tray.is_some() {
        app.windowless()
    } else {
        app
    };

    // A second launch signals this event; show the window that already exists.
    let activation = handle.clone();
    single_instance::listen_activation(move || activation.open());
    // An installer replacing the app signals this one.
    let quitting = handle.clone();
    single_instance::listen_quit(move || quitting.quit());

    app.run();
    drop(guard);
}
