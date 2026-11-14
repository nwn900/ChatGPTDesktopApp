//! ChatGPT desktop app: Rune hosts the window, Edge WebView2 shows the page.
//!
//! Rune has no webview, so the window is a Rune window (winit + wgpu) and the
//! page is a WebView2 child HWND parented to it and sized to the whole client
//! area. Rune draws no chrome: the page is the window.
//!
//! Closing the window hides it and the app keeps running in the tray, so the
//! session survives; the tray icon brings it back. The tray menu also toggles
//! launching at system startup.
//!
//! Run it with `cargo run`.

// No console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autostart;
mod links;
mod webview;

use std::sync::{Arc, OnceLock};

use rune::raw_window_handle::RawWindowHandle;
use rune::{AppSpec, Cx, El, Tray, TrayEvent, TrayIcon, TrayItem, TraySpec, Tx, el};
use windows::Win32::Foundation::HWND;

const TARGET_URL: &str = "https://chatgpt.com/";

/// The project's own icons, embedded so the installer ships one file.
const APP_ICON: &[u8] = include_bytes!("../../src-tauri/icons/icon.png");
const TRAY_ICON: &[u8] = include_bytes!("../../src-tauri/icons/32x32.png");

/// The live tray, so a menu pick can relabel the startup item.
static TRAY: OnceLock<Arc<Tray>> = OnceLock::new();

fn poll(_state: &mut (), _now: f64, _tx: &mut Tx) -> bool {
    // Cheap when the window has not moved: the module caches the rect.
    webview::sync_bounds();
    false
}

fn view(_state: &(), _cx: &mut Cx<()>) -> El {
    // The page is a native child window covering the whole client area, so
    // Rune draws nothing behind it.
    el().w_full().h_full()
}

/// The tray menu, with the startup item showing its current state.
fn tray_menu(startup: bool) -> Vec<TrayItem> {
    vec![
        TrayItem::action("open", "Open ChatGPT"),
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
            Err(error) => webview::log(&format!("startup toggle failed: {error}")),
        }
        return;
    }

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
    // Closing the window hides it instead of ending the app.
    .keep_running()
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
    let app = if from_startup { app.windowless() } else { app };

    // The tray runs on its own thread, so it answers while the window is
    // hidden. TRAY keeps it alive for the life of the process.
    let handle = app.handle();
    let startup = autostart::is_enabled();
    webview::log(&format!("startup at launch: {startup}"));
    let _tray = match Tray::new(
        TraySpec::new("chatgpt", "ChatGPT", tray_icon())
            .tooltip("ChatGPT")
            .menu(tray_menu(startup)),
        move |event| match event {
            TrayEvent::Activate { .. } => handle.open(),
            TrayEvent::Menu(id) if id == "open" => handle.open(),
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

    app.run();
}
