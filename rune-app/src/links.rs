//! Which URLs stay in the app and which belong to the system browser.

/// Hosts that stay in the app: ChatGPT and the identity providers its sign-in
/// flow uses. Everything else opens in the default browser.
pub const IN_APP_HOSTS: &[&str] = &[
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

fn host_in(host: &str, list: &[&str]) -> bool {
    list.iter()
        .any(|suffix| host == *suffix || host.ends_with(&format!(".{suffix}")))
}

pub fn is_in_app_host(host: &str) -> bool {
    host_in(host, IN_APP_HOSTS)
}

/// Whether a URL belongs in the system browser rather than the app.
///
/// An http(s) link to any host the app does not own leaves the app, and so does
/// mailto. Everything else (about:blank, blob:, data:, javascript:) is left
/// alone so the page keeps working.
pub fn opens_in_browser(url: &url::Url) -> bool {
    match url.scheme() {
        "http" | "https" => url.host_str().is_some_and(|host| !is_in_app_host(host)),
        "mailto" => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::opens_in_browser;

    fn url(value: &str) -> url::Url {
        value.parse().unwrap()
    }

    #[test]
    fn content_links_leave_the_app() {
        assert!(opens_in_browser(&url("https://en.wikipedia.org/wiki/Rust")));
        assert!(opens_in_browser(&url(
            "https://github.com/nwn900/ChatGPTDesktopApp"
        )));
        assert!(opens_in_browser(&url(
            "https://openai.com/policies/terms-of-use"
        )));
        assert!(opens_in_browser(&url(
            "https://help.openai.com/en/articles/1"
        )));
        assert!(opens_in_browser(&url(
            "https://docs.google.com/document/d/1"
        )));
        assert!(opens_in_browser(&url("mailto:support@example.com")));
    }

    #[test]
    fn app_and_sign_in_hosts_stay_in_app() {
        assert!(!opens_in_browser(&url("https://chatgpt.com/c/abc")));
        assert!(!opens_in_browser(&url("https://auth.openai.com/authorize")));
        assert!(!opens_in_browser(&url(
            "https://accounts.google.com/o/oauth2/v2/auth"
        )));
        assert!(!opens_in_browser(&url(
            "https://login.microsoftonline.com/common"
        )));
        assert!(!opens_in_browser(&url(
            "https://appleid.apple.com/auth/authorize"
        )));
    }

    #[test]
    fn other_schemes_are_left_to_the_webview() {
        assert!(!opens_in_browser(&url("about:blank")));
        assert!(!opens_in_browser(&url("blob:https://chatgpt.com/9f2b")));
        assert!(!opens_in_browser(&url("javascript:alert(1)")));
    }

    #[test]
    fn a_lookalike_host_is_not_in_app() {
        assert!(opens_in_browser(&url("https://chatgpt.com.example.com/")));
        assert!(opens_in_browser(&url("https://notchatgpt.com/")));
    }
}
