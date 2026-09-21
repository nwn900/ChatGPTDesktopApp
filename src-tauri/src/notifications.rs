use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::Manager;
#[cfg(not(windows))]
use tauri_plugin_notification::NotificationExt;

static LAST: Mutex<Option<Instant>> = Mutex::new(None);

pub fn send(app: &tauri::AppHandle, body: &str) -> Result<(), String> {
    #[cfg(windows)]
    let result = {
        let handle = app.clone();
        tauri_winrt_notification::Toast::new(&app.config().identifier)
            .title("ChatGPT")
            .text1(body)
            .on_activated(move |_| {
                crate::show_main_window(&handle);
                Ok(())
            })
            .show()
            .map_err(|error| error.to_string())
    };
    #[cfg(not(windows))]
    let result = app
        .notification()
        .builder()
        .title("ChatGPT")
        .body(body)
        .show()
        .map_err(|error| error.to_string());
    // Bounded diagnostic log; never store conversation text.
    crate::logging::append(
        app,
        "notifications.log",
        &format!("Windows notification: {result:?}"),
    );
    result
}

#[tauri::command]
pub fn notify_answer(window: tauri::WebviewWindow, body: String) -> Result<(), String> {
    let url = window.url().map_err(|error| error.to_string())?;
    if window.label() != "main" || url.scheme() != "https" || url.host_str() != Some("chatgpt.com")
    {
        return Err("Notification origin rejected".into());
    }
    if window.is_focused().unwrap_or(false)
        && window.is_visible().unwrap_or(false)
        && !window.is_minimized().unwrap_or(false)
    {
        return Ok(());
    }
    let mut last = LAST.lock().map_err(|error| error.to_string())?;
    if last.is_some_and(|time| time.elapsed() < Duration::from_secs(3)) {
        return Ok(());
    }
    let body: String = body.chars().take(300).collect();
    if body.trim().is_empty() {
        return Err("Empty notification".into());
    }
    send(window.app_handle(), &body)?;
    *last = Some(Instant::now());
    Ok(())
}
