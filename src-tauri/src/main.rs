// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    webview::{DownloadEvent, NewWindowResponse},
    Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent,
};
use tauri_plugin_autostart::ManagerExt;

mod links;
mod logging;
mod notifications;

static IS_QUITTING: AtomicBool = AtomicBool::new(false);

const TARGET_URL: &str = "https://chatgpt.com";

pub(crate) fn build_main_window(
    app_handle: &tauri::AppHandle,
) -> tauri::Result<tauri::WebviewWindow> {
    let popup_app = app_handle.clone();
    let browser_app = app_handle.clone();
    let navigation_app = app_handle.clone();
    let target_url: url::Url = TARGET_URL.parse().unwrap();
    // The page itself decides which clicks leave the app. The host list is
    // injected here so the policy has a single source of truth in links.rs.
    let in_app_hosts = format!(
        "window.__CHATGPT_IN_APP_HOSTS = {};",
        serde_json::to_string(links::IN_APP_HOSTS).unwrap_or_else(|_| "[]".to_string())
    );

    WebviewWindowBuilder::new(app_handle, "main", WebviewUrl::External(target_url))
        .title("ChatGPT")
        .initialization_script(&in_app_hosts)
        .initialization_script(include_str!("links.js"))
        .initialization_script(include_str!("notifications.js"))
        .on_new_window(move |url, features| {
            if !links::is_allowed_url(&url) {
                // ponytail: a link the app does not own belongs to the user
                // default browser, never to an in-app window.
                if matches!(url.scheme(), "http" | "https" | "mailto") {
                    let _ = links::open_in_default_browser(&browser_app, url.as_str());
                }
                return NewWindowResponse::Deny;
            }
            static POPUP_ID: std::sync::atomic::AtomicUsize =
                std::sync::atomic::AtomicUsize::new(1);
            let label = format!("login-{}", POPUP_ID.fetch_add(1, Ordering::Relaxed));
            match WebviewWindowBuilder::new(
                &popup_app,
                label,
                WebviewUrl::External("about:blank".parse().unwrap()),
            )
            .window_features(features)
            .on_navigation(|url| links::is_allowed_url(url))
            .build()
            {
                Ok(window) => NewWindowResponse::Create { window },
                Err(_) => NewWindowResponse::Deny,
            }
        })
        .inner_size(1200.0, 900.0)
        .resizable(true)
        .visible(true)
        .center()
        .focused(true)
        // ponytail: Tauri enables wry drag-drop handler by default, which
        // registers IDropTarget on the WebView2 HWND and calls
        // SetAllowExternalDrop(false). This intercepts OS file drops before
        // they reach the page. Disabling it lets WebView2 forward drops to
        // the page as native HTML5 drag-drop events (dragenter/dragover/drop
        // with dataTransfer.files) which ChatGPT already handles.
        .disable_drag_drop_handler()
        // ponytail: keep the scroll path and the page timers alive. When the
        // window is minimized or hidden, Chromium clamps timers to about one
        // tick per minute, which starved the answer-completion watcher.
        .additional_browser_args("--disable-background-timer-throttling")
        .on_navigation(move |url| {
            if links::is_allowed_url(url) {
                return true;
            }
            // ponytail: a click that somehow skipped the page interceptor must
            // still not replace the ChatGPT window with a foreign page.
            if matches!(url.scheme(), "https" | "http" | "mailto") {
                let _ = links::open_in_default_browser(&navigation_app, url.as_str());
            }
            false
        })
        .on_download(|_webview, event| {
            match event {
                DownloadEvent::Requested { destination, .. } => {
                    // ponytail: wry pre-fills destination with WebView2
                    // suggested name+ext (from the blob download attr or
                    // Content-Disposition) via ResultFilePath(). Use that as
                    // the save-dialog default instead of the blob URL, which
                    // carries no filename. Avoids a second raw CoreWebView2
                    // DownloadStarting handler (would clash with wry).
                    let filename = destination
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("download")
                        .to_string();
                    let mut dlg = rfd::FileDialog::new().set_file_name(&filename);
                    if let Some(dir) = destination.parent() {
                        dlg = dlg.set_directory(dir);
                    }
                    if let Some(path) = dlg.save_file() {
                        *destination = path;
                        true
                    } else {
                        false
                    }
                }
                DownloadEvent::Finished { .. } => true,
                _ => true,
            }
        })
        .build()
}

pub(crate) fn show_main_window(app_handle: &tauri::AppHandle) {
    if let Some(window) = app_handle.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
        return;
    }
    // ponytail: tray-only trap — main window gone, rebuild it
    if let Ok(window) = build_main_window(app_handle) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--autostart"]),
        ))
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            show_main_window(app);
        }))
        .invoke_handler(tauri::generate_handler![
            notifications::notify_answer,
            links::open_external
        ])
        .setup(|app| {
            let main_window = build_main_window(app.handle())?;

            // If autostart is enabled, launch minimized to tray
            if std::env::args().any(|arg| arg == "--autostart") {
                let _ = main_window.hide();
            } else {
                // ponytail: guarantee visible window on manual launch
                let _ = main_window.show();
                let _ = main_window.unminimize();
                let _ = main_window.set_focus();
            }

            // Hide to tray on close
            let win_clone = main_window.clone();
            main_window.on_window_event(move |event| match event {
                WindowEvent::CloseRequested { api, .. } => {
                    if !IS_QUITTING.load(Ordering::SeqCst) {
                        api.prevent_close();
                        let _ = win_clone.hide();
                    }
                }
                _ => {}
            });

            // Build system tray menu
            let is_enabled = app.autolaunch().is_enabled().unwrap_or(false);

            let separator = PredefinedMenuItem::separator(app)?;
            let open_item = MenuItem::with_id(app, "open", "Open ChatGPT", true, None::<&str>)?;
            let refresh_item =
                MenuItem::with_id(app, "refresh", "Refresh ChatGPT", true, None::<&str>)?;
            let login_item = MenuItem::with_id(app, "login", "Login...", true, None::<&str>)?;
            let startup_item = CheckMenuItem::with_id(
                app,
                "startup",
                "Launch at system startup",
                true,
                is_enabled,
                None::<&str>,
            )?;
            let close_item = MenuItem::with_id(app, "close", "Close ChatGPT", true, None::<&str>)?;

            let menu = Menu::with_items(
                app,
                &[
                    &open_item,
                    &refresh_item,
                    &login_item,
                    &separator,
                    &startup_item,
                    &separator,
                    &close_item,
                ],
            )?;

            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().cloned().unwrap())
                .tooltip("ChatGPT")
                .menu(&menu)
                .on_menu_event(move |app_handle, event| match event.id().as_ref() {
                    "open" => {
                        show_main_window(app_handle);
                    }
                    "refresh" => {
                        show_main_window(app_handle);
                        if let Some(window) = app_handle.get_webview_window("main") {
                            let _ = window.eval("window.location.reload();");
                        }
                    }
                    "login" => {
                        show_main_window(app_handle);
                        if let Some(window) = app_handle.get_webview_window("main") {
                            if let Ok(url) = url::Url::parse(TARGET_URL) {
                                let _ = window.navigate(url);
                            }
                        }
                    }
                    "startup" => {
                        let manager = app_handle.autolaunch();
                        let currently_enabled = manager.is_enabled().unwrap_or(false);
                        if currently_enabled {
                            let _ = manager.disable();
                        } else {
                            let _ = manager.enable();
                        }
                    }
                    "close" => {
                        IS_QUITTING.store(true, Ordering::SeqCst);
                        app_handle.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| match event {
                    tauri::tray::TrayIconEvent::Click {
                        button: tauri::tray::MouseButton::Left,
                        button_state: tauri::tray::MouseButtonState::Up,
                        ..
                    } => {
                        show_main_window(tray.app_handle());
                    }
                    _ => {}
                })
                .show_menu_on_left_click(false)
                .build(app)?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running ChatGPT");
}
