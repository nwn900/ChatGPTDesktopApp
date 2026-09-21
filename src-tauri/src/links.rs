use tauri::{Manager, WebviewWindow};

/// Hosts the app keeps inside its own webview. These are the ChatGPT origins
/// plus the identity providers its sign-in flow uses, so authentication keeps
/// working in-app instead of being pushed to the system browser.
pub(crate) const IN_APP_HOSTS: &[&str] = &[
    "chatgpt.com",
    "chat.openai.com",
    "auth.openai.com",
    "auth0.openai.com",
    "accounts.google.com",
    "appleid.apple.com",
    "microsoftonline.com",
    "live.com",
    "recaptcha.net",
    "stripe.com",
];

/// Hosts the webview may navigate to or open as an in-app popup. This is the
/// wider authentication-safe list: several providers bounce sign-in through
/// their main domain, so those stay allowed here even though a plain content
/// link to them is handed to the system browser by the link interceptor.
pub(crate) const ALLOWED_HOSTS: &[&str] = &[
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

/// Origins the app itself is served from. A page on any other origin may not
/// drive the native command below.
const APP_HOSTS: &[&str] = &["chatgpt.com", "chat.openai.com"];

fn host_in(host: &str, list: &[&str]) -> bool {
    list.iter()
        .any(|suffix| host == *suffix || host.ends_with(&format!(".{suffix}")))
}

pub(crate) fn is_allowed_host(hostname: &str) -> bool {
    host_in(hostname, ALLOWED_HOSTS)
}

pub(crate) fn is_in_app_host(hostname: &str) -> bool {
    host_in(hostname, IN_APP_HOSTS)
}

pub(crate) fn is_app_host(hostname: &str) -> bool {
    host_in(hostname, APP_HOSTS)
}

pub(crate) fn is_allowed_url(url: &url::Url) -> bool {
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

/// Whether a clicked link belongs in the operating system default browser
/// rather than in the app webview.
pub(crate) fn should_open_in_browser(url: &url::Url) -> bool {
    match url.scheme() {
        "http" | "https" => url.host_str().is_some_and(|host| !is_in_app_host(host)),
        "mailto" => true,
        _ => false,
    }
}

/// Hand the URL to the operating system default handler and record the
/// outcome. The error string is returned so the page can fall back to its own
/// new-window handling instead of leaving the click dead.
pub(crate) fn open_in_default_browser(app: &tauri::AppHandle, url: &str) -> Result<(), String> {
    let result = open::that_detached(url).map_err(|error| error.to_string());
    crate::logging::append(app, "links.log", &format!("open {url} -> {result:?}"));
    result
}

/// Native command behind the page link interceptor. The page is a remote
/// origin, so the caller is checked again here: only the main ChatGPT window
/// may ask, and only for links that have to leave the app.
#[tauri::command]
pub fn open_external(window: WebviewWindow, url: String) -> Result<(), String> {
    let origin = window.url().map_err(|error| error.to_string())?;
    let origin_host = origin.host_str().unwrap_or_default();
    if window.label() != "main" || origin.scheme() != "https" || !is_app_host(origin_host) {
        return Err("Link origin rejected".into());
    }
    let parsed = url::Url::parse(&url).map_err(|error| error.to_string())?;
    if !should_open_in_browser(&parsed) {
        return Err("Link stays inside the app".into());
    }
    open_in_default_browser(window.app_handle(), parsed.as_str())
}

#[cfg(test)]
mod tests {
    use super::{is_allowed_url, should_open_in_browser};

    fn url(value: &str) -> url::Url {
        value.parse().unwrap()
    }

    #[test]
    fn content_links_open_in_the_browser() {
        assert!(should_open_in_browser(&url(
            "https://en.wikipedia.org/wiki/Rust"
        )));
        assert!(should_open_in_browser(&url(
            "https://github.com/nwn900/ChatGPTDesktopApp"
        )));
        assert!(should_open_in_browser(&url(
            "https://openai.com/policies/terms-of-use"
        )));
        assert!(should_open_in_browser(&url(
            "https://help.openai.com/en/articles/1"
        )));
        assert!(should_open_in_browser(&url(
            "https://docs.google.com/document/d/1"
        )));
        assert!(should_open_in_browser(&url("mailto:support@example.com")));
    }

    #[test]
    fn app_and_sign_in_hosts_stay_in_app() {
        assert!(!should_open_in_browser(&url("https://chatgpt.com/c/abc")));
        assert!(!should_open_in_browser(&url(
            "https://auth.openai.com/authorize"
        )));
        assert!(!should_open_in_browser(&url(
            "https://accounts.google.com/o/oauth2/v2/auth"
        )));
        assert!(!should_open_in_browser(&url(
            "https://login.microsoftonline.com/common"
        )));
        assert!(!should_open_in_browser(&url(
            "https://appleid.apple.com/auth/authorize"
        )));
    }

    #[test]
    fn other_schemes_are_left_to_the_webview() {
        assert!(!should_open_in_browser(&url("about:blank")));
        assert!(!should_open_in_browser(&url(
            "blob:https://chatgpt.com/9f2b"
        )));
        assert!(!should_open_in_browser(&url("javascript:alert(1)")));
    }

    #[test]
    fn navigation_policy_keeps_authentication_hosts() {
        assert!(is_allowed_url(&url("https://chatgpt.com/?os=app")));
        assert!(is_allowed_url(&url("https://auth.openai.com/authorize")));
        assert!(is_allowed_url(&url("https://accounts.google.com/signin")));
        assert!(is_allowed_url(&url("about:blank")));
        assert!(is_allowed_url(&url(
            "blob:https://chatgpt.com/login/callback"
        )));
        assert!(!is_allowed_url(&url("https://en.wikipedia.org/wiki/Rust")));
        assert!(!is_allowed_url(&url("https://chatgpt.com.example.com/")));
    }
}
