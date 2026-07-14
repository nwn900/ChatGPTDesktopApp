// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent,
};
use tauri_plugin_autostart::ManagerExt;

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
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .setup(|app| {
            let target_url: url::Url = TARGET_URL.parse().unwrap();

            let main_window = WebviewWindowBuilder::new(
                app,
                "main",
                WebviewUrl::External(target_url),
            )
            .title("ChatGPT")
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36 Edg/126.0.0.0")
            .initialization_script(r#"
                Object.defineProperty(navigator, 'webdriver', { get: () => false });
                if (window.chrome && window.chrome.webview) {
                    delete window.chrome.webview;
                }
                window.open = function(url, name, features) {
                    if (url) { window.location.assign(url); }
                    return { close: function(){}, focus: function(){} };
                };
                document.addEventListener('click', function(e) {
                    let target = e.target.closest('a');
                    if (target && target.getAttribute('target') === '_blank') {
                        target.setAttribute('target', '_self');
                    }
                }, true);
                window.addEventListener('load', function() {
                    var path = window.location.pathname;
                    if (path.includes('/callback') || path.includes('/authorize') || path === '/lo' || path.startsWith('/u/login')) {
                        setTimeout(function() {
                            window.location.assign('/');
                        }, 2000);
                    }
                });
            "#)
            .inner_size(1200.0, 900.0)
            .resizable(true)
            .on_navigation(|url| is_allowed_url(url))
            .build()?;

            // If autostart is enabled, launch minimized to tray
            if app.autolaunch().is_enabled().unwrap_or(false) {
                let _ = main_window.hide();
            }

            // Hide to tray on close
            let win_clone = main_window.clone();
            main_window.on_window_event(move |event| {
                match event {
                    WindowEvent::CloseRequested { api, .. } => {
                        if !IS_QUITTING.load(Ordering::SeqCst) {
                            api.prevent_close();
                            let _ = win_clone.hide();
                        }
                    }
                    _ => {}
                }
            });

            // Build system tray menu
            let is_enabled = app.autolaunch().is_enabled().unwrap_or(false);

            let separator = PredefinedMenuItem::separator(app)?;
            let open_item = MenuItem::with_id(app, "open", "Open ChatGPT", true, None::<&str>)?;
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
                &[&open_item, &login_item, &separator, &startup_item, &separator, &close_item],
            )?;

            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().cloned().unwrap())
                .tooltip("ChatGPT")
                .menu(&menu)
                .on_menu_event(move |app_handle, event| {
                    match event.id().as_ref() {
                        "open" => {
                            if let Some(window) = app_handle.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                        "login" => {
                            if let Some(window) = app_handle.get_webview_window("main") {
                                let _ = window.show();
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
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    match event {
                        tauri::tray::TrayIconEvent::Click {
                            button: tauri::tray::MouseButton::Left,
                            button_state: tauri::tray::MouseButtonState::Up,
                            ..
                        } => {
                            let app = tray.app_handle();
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                        _ => {}
                    }
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
