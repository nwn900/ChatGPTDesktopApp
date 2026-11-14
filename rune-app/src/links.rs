//! Link policy: what a URL is, and what the app does with it.
//!
//! The policy is an explicit abstraction rather than ad-hoc host checks: a
//! [LinkPolicy] classifies a URL into a [LinkKind], and the two decision
//! methods say what to do for a popup request and for a navigation on the main
//! surface. [ChatGptPolicy] is the only implementation, so the rules are
//! testable without a window or a WebView2.

use url::Url;

/// What a URL is, from the app's point of view.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LinkKind {
    /// A page of the app itself.
    Main,
    /// A trusted identity provider the sign-in flow needs, with its session.
    Auth,
    /// Any other http(s) or mailto target: belongs to the user's browser.
    External,
    /// A scheme the page uses for itself: about:blank, blob:, data:.
    Internal,
    /// A scheme that must never be followed.
    Blocked,
}

/// What to do when the page asks for a new window.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Popup {
    /// Let WebView2 open its own in-profile popup, so the session carries over.
    Allow,
    /// Load it in the main page instead; nothing is dropped, no second window.
    NavigateMain,
    /// Hand it to the operating system's default browser.
    OpenExternally,
    /// Drop it.
    Block,
}

/// What to do when the main surface is about to navigate.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Navigation {
    Allow,
    OpenExternally,
    Block,
}

/// The rules the app applies to every URL it meets.
pub trait LinkPolicy {
    fn classify(&self, url: &Url) -> LinkKind;
    fn popup(&self, kind: LinkKind) -> Popup;
    fn navigation(&self, kind: LinkKind) -> Navigation;
}

/// Hosts that are the app itself.
const MAIN_HOSTS: &[&str] = &["chatgpt.com", "chat.openai.com"];
/// Hosts the sign-in flow goes through and whose session must be kept.
const AUTH_HOSTS: &[&str] = &[
    "auth.openai.com",
    "auth0.openai.com",
    "accounts.google.com",
    "appleid.apple.com",
    "microsoftonline.com",
    "live.com",
    "recaptcha.net",
];

fn host_in(host: &str, list: &[&str]) -> bool {
    list.iter()
        .any(|suffix| host == *suffix || host.ends_with(&format!(".{suffix}")))
}

/// The app's policy: fall back to the real page, keep sign-in in the app, and
/// send everything else to the user's browser.
pub struct ChatGptPolicy;

impl LinkPolicy for ChatGptPolicy {
    fn classify(&self, url: &Url) -> LinkKind {
        match url.scheme() {
            "http" | "https" => match url.host_str() {
                Some(host) if host_in(host, MAIN_HOSTS) => LinkKind::Main,
                Some(host) if host_in(host, AUTH_HOSTS) => LinkKind::Auth,
                Some(_) => LinkKind::External,
                None => LinkKind::Blocked,
            },
            "mailto" => LinkKind::External,
            "about" if url.path() == "blank" => LinkKind::Internal,
            "blob" | "data" => LinkKind::Internal,
            _ => LinkKind::Blocked,
        }
    }

    fn popup(&self, kind: LinkKind) -> Popup {
        match kind {
            // A link to another page of the app: keep it in the one window the
            // app has rather than opening a second ChatGPT surface.
            LinkKind::Main => Popup::NavigateMain,
            // Sign-in popups need the embedded session, so WebView2 opens them
            // in-profile. Forcing them into the system browser would abandon
            // the session the flow is establishing.
            LinkKind::Auth => Popup::Allow,
            // about:blank is how an OAuth popup starts; blob: and data: belong
            // to the page. None of them should go to the browser.
            LinkKind::Internal => Popup::Allow,
            LinkKind::External => Popup::OpenExternally,
            LinkKind::Blocked => Popup::Block,
        }
    }

    fn navigation(&self, kind: LinkKind) -> Navigation {
        match kind {
            LinkKind::Main | LinkKind::Auth | LinkKind::Internal => Navigation::Allow,
            LinkKind::External => Navigation::OpenExternally,
            LinkKind::Blocked => Navigation::Block,
        }
    }
}

/// Whether a URL belongs in the system browser. Kept for the call sites that
/// only need the yes/no.
pub fn opens_in_browser(url: &Url) -> bool {
    matches!(ChatGptPolicy.classify(url), LinkKind::External)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(value: &str) -> Url {
        value.parse().unwrap()
    }

    fn kind(value: &str) -> LinkKind {
        ChatGptPolicy.classify(&url(value))
    }

    #[test]
    fn classification_covers_every_case() {
        assert_eq!(kind("https://chatgpt.com/c/abc"), LinkKind::Main);
        assert_eq!(kind("https://chat.openai.com/"), LinkKind::Main);
        assert_eq!(kind("https://auth.openai.com/authorize"), LinkKind::Auth);
        assert_eq!(
            kind("https://accounts.google.com/o/oauth2/v2/auth"),
            LinkKind::Auth
        );
        assert_eq!(
            kind("https://login.microsoftonline.com/common"),
            LinkKind::Auth
        );
        assert_eq!(
            kind("https://en.wikipedia.org/wiki/Rust"),
            LinkKind::External
        );
        assert_eq!(
            kind("https://openai.com/policies/terms-of-use"),
            LinkKind::External
        );
        assert_eq!(kind("mailto:support@example.com"), LinkKind::External);
        assert_eq!(kind("about:blank"), LinkKind::Internal);
        assert_eq!(kind("blob:https://chatgpt.com/9f2b"), LinkKind::Internal);
        assert_eq!(kind("data:text/plain,hi"), LinkKind::Internal);
        assert_eq!(kind("javascript:alert(1)"), LinkKind::Blocked);
        assert_eq!(kind("file:///C:/Windows/win.ini"), LinkKind::Blocked);
    }

    #[test]
    fn a_lookalike_host_is_external() {
        assert_eq!(kind("https://chatgpt.com.example.com/"), LinkKind::External);
        assert_eq!(kind("https://notchatgpt.com/"), LinkKind::External);
        assert_eq!(
            kind("https://accounts.google.com.evil.example/"),
            LinkKind::External
        );
    }

    #[test]
    fn nothing_the_page_opens_is_silently_dropped() {
        // The only things that may be dropped are the schemes that must never
        // be followed at all.
        for value in [
            "https://chatgpt.com/c/abc",
            "https://auth.openai.com/authorize",
            "https://en.wikipedia.org/wiki/Rust",
            "about:blank",
            "blob:https://chatgpt.com/9f2b",
            "mailto:support@example.com",
        ] {
            let popup = ChatGptPolicy.popup(kind(value));
            assert_ne!(popup, Popup::Block, "{value} must not be dropped");
            let navigation = ChatGptPolicy.navigation(kind(value));
            assert_ne!(navigation, Navigation::Block, "{value} must not be blocked");
        }
        // And the ones that must be dropped are.
        for value in ["javascript:alert(1)", "file:///C:/Windows/win.ini"] {
            assert_eq!(ChatGptPolicy.popup(kind(value)), Popup::Block);
            assert_eq!(ChatGptPolicy.navigation(kind(value)), Navigation::Block);
        }
    }

    #[test]
    fn a_chatgpt_link_stays_in_the_one_window() {
        assert_eq!(ChatGptPolicy.popup(LinkKind::Main), Popup::NavigateMain);
    }

    #[test]
    fn sign_in_keeps_its_session_in_the_app() {
        assert_eq!(ChatGptPolicy.popup(LinkKind::Auth), Popup::Allow);
        assert_eq!(ChatGptPolicy.navigation(LinkKind::Auth), Navigation::Allow);
        assert_eq!(ChatGptPolicy.popup(LinkKind::Internal), Popup::Allow);
    }

    #[test]
    fn content_links_leave_for_the_browser() {
        assert_eq!(
            ChatGptPolicy.popup(LinkKind::External),
            Popup::OpenExternally
        );
        assert_eq!(
            ChatGptPolicy.navigation(LinkKind::External),
            Navigation::OpenExternally
        );
        assert!(opens_in_browser(&url("https://github.com/x/y")));
        assert!(!opens_in_browser(&url("https://chatgpt.com/")));
    }
}
