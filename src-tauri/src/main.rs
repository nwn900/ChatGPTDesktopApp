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

mod notifications;

static IS_QUITTING: AtomicBool = AtomicBool::new(false);

const TARGET_URL: &str = "https://chatgpt.com";

const ALLOWED_HOSTS: &[&str] = &[
    "chatgpt.com",
    "openai.com",
    "chat.openai.com",
    "auth.openai.com",
    "accounts.google.com",
    "google.com",
    "googleusercontent.com",
    "gstatic.com",
    "googleapis.com",
    "apple.com",
    "appleid.apple.com",
    "recaptcha.net",
    "microsoftonline.com",
    "live.com",
    "microsoft.com",
    "stripe.com",
];

fn is_allowed_host(hostname: &str) -> bool {
    ALLOWED_HOSTS
        .iter()
        .any(|suffix| hostname == *suffix || hostname.ends_with(&format!(".{}", suffix)))
}

fn is_allowed_url(url: &url::Url) -> bool {
    match url.scheme() {
        "http" | "https" => url.host_str().is_some_and(is_allowed_host),
        "about" => url.path() == "blank",
        "blob" => url
            .path()
            .split_once(':')
            .and_then(|(scheme, rest)| match scheme {
                "http" | "https" => url::Url::parse(&format!("{scheme}:{rest}")).ok(),
                _ => None,
            })
            .is_some_and(|inner_url| inner_url.host_str().is_some_and(is_allowed_host)),
        _ => false,
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
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .invoke_handler(tauri::generate_handler![notifications::notify_answer])
        .setup(|app| {
            let popup_app = app.handle().clone();
            let target_url: url::Url = TARGET_URL.parse().unwrap();

            let main_window =
                WebviewWindowBuilder::new(app, "main", WebviewUrl::External(target_url))
                    .title("ChatGPT")
                    .initialization_script(include_str!("notifications.js"))
                    .on_new_window(move |url, features| {
                        if !is_allowed_url(&url) {
                            if matches!(url.scheme(), "http" | "https") {
                                let _ = open::that_detached(url.as_str());
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
                        .on_navigation(|url| is_allowed_url(url))
                        .build()
                        {
                            Ok(window) => NewWindowResponse::Create { window },
                            Err(_) => NewWindowResponse::Deny,
                        }
                    })
                    .inner_size(1200.0, 900.0)
                    .resizable(true)
                    // ponytail: Tauri enables wry's drag-drop handler by default, which
                    // registers IDropTarget on the WebView2 HWND and calls
                    // SetAllowExternalDrop(false). This intercepts OS file drops before
                    // they reach the page. Disabling it lets WebView2 forward drops to
                    // the page as native HTML5 drag-drop events (dragenter/dragover/drop
                    // with dataTransfer.files) — which ChatGPT already handles.
                    .disable_drag_drop_handler()
                    // ponytail: disable overlay scrollbar flash timeout. When overlay
                    // scrollbars auto-hide, WebView2/Chromium can stop routing wheel
                    // events to the page, causing scroll to freeze until manual click.
                    // Disabling the timeout keeps the scroll path active.
                    // Also disable background timer throttling: when the window is
                    // minimized/hidden, Chromium clamps setInterval to ~1/min, which
                    // starved the answer-completion watcher below.
                    .additional_browser_args("--disable-background-timer-throttling")
                    .on_navigation(|url| {
                        if is_allowed_url(url) {
                            return true;
                        }
                        // ponytail: block navigation in webview for external links,
                        // open them in the default system browser instead.
                        if matches!(url.scheme(), "https" | "http" | "mailto") {
                            let _ = open::that_detached(url.as_str());
                        }
                        false
                    })
                    .on_download(|_webview, event| {
                        match event {
                            DownloadEvent::Requested { destination, .. } => {
                                // ponytail: wry pre-fills `destination` with WebView2's
                                // suggested name+ext (from the blob's `download` attr /
                                // Content-Disposition) via ResultFilePath(). Use that as
                                // the save-dialog default instead of the blob URL, which
                                // carries no filename. Avoids a second raw CoreWebView2
                                // DownloadStarting handler (would clash with wry's).
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
                    .build()?;

            // If autostart is enabled, launch minimized to tray
            if std::env::args().any(|arg| arg == "--autostart") {
                let _ = main_window.hide();
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
            let test_notif_item =
                MenuItem::with_id(app, "test-notif", "Test Notification", true, None::<&str>)?;
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
                    &test_notif_item,
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
                        if let Some(window) = app_handle.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.unminimize();
                            let _ = window.set_focus();
                        }
                    }
                    "refresh" => {
                        if let Some(window) = app_handle.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.unminimize();
                            let _ = window.set_focus();
                            let _ = window.eval("window.location.reload();");
                        }
                    }
                    "test-notif" => {
                        let _ =
                            notifications::send(app_handle, "Desktop notifications are enabled.");
                    }
                    "login" => {
                        if let Some(window) = app_handle.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.unminimize();
                            let _ = window.set_focus();
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
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.unminimize();
                            let _ = window.set_focus();
                        }
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

#[cfg(test)]
mod tests {
    use super::{is_allowed_host, is_allowed_url};

    #[test]
    fn allows_common_chatgpt_login_hosts() {
        assert!(is_allowed_host("chatgpt.com"));
        assert!(is_allowed_host("auth.openai.com"));
        assert!(is_allowed_host("chat.openai.com"));
        assert!(is_allowed_host("accounts.google.com"));
        assert!(is_allowed_host("auth.example.apple.com"));
        assert!(is_allowed_host("login.microsoftonline.com"));
    }

    #[test]
    fn rejects_unknown_hosts() {
        assert!(!is_allowed_host("example.com"));
        assert!(!is_allowed_host("chatgpt.com.example.com"));
    }

    #[test]
    fn allows_auth_safe_urls() {
        let blank: url::Url = "about:blank".parse().unwrap();
        let blob: url::Url = "blob:https://chatgpt.com/login/callback".parse().unwrap();
        let app: url::Url = "https://chatgpt.com/?os=app".parse().unwrap();
        let blocked: url::Url = "https://example.com/login".parse().unwrap();

        assert!(is_allowed_url(&blank));
        assert!(is_allowed_url(&blob));
        assert!(is_allowed_url(&app));
        assert!(!is_allowed_url(&blocked));
    }
}
