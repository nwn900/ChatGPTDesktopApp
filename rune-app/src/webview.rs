//! Edge WebView2 hosted as a child window of the Rune window.
//!
//! The page is a real WebView2 child HWND parented to the Rune window and
//! sized to the whole client area. Creation is asynchronous: WebView2 completes
//! through the message loop, so nothing here blocks the frame that starts it
//! and no message pump is re-entered from inside a Rune callback.
//!
//! The host is a small state machine. Every failure path records a reason and
//! moves to [State::Failed], so the app can show a native message instead of an
//! empty window, and repeated or out-of-order callbacks cannot start a second
//! environment.

use std::cell::{Cell, RefCell};
use std::os::windows::ffi::OsStrExt;
use std::sync::OnceLock;
use std::sync::mpsc::Sender;

use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_PROCESS_FAILED_KIND, COREWEBVIEW2_PROCESS_FAILED_KIND_FRAME_RENDER_PROCESS_EXITED,
    COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED,
    CreateCoreWebView2EnvironmentWithOptions, ICoreWebView2, ICoreWebView2Controller,
    ICoreWebView2Environment, ICoreWebView2EnvironmentOptions,
};
use webview2_com::{
    CreateCoreWebView2ControllerCompletedHandler, CreateCoreWebView2EnvironmentCompletedHandler,
    NavigationStartingEventHandler, NewWindowRequestedEventHandler, ProcessFailedEventHandler,
    take_pwstr,
};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::UI::WindowsAndMessaging::GetClientRect;
use windows::core::{HSTRING, PCWSTR, PWSTR};

use crate::links::{ChatGptPolicy, LinkPolicy, Navigation, Popup};

/// Where the page is in its life.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    /// Nothing has been asked for yet.
    Idle,
    /// WebView2 is creating its environment and controller.
    Starting,
    /// The controller is live.
    Ready,
    /// Setup failed; the app shows what it can instead of a blank window.
    Failed,
}

/// The page and its state, owned by the UI thread.
struct Host {
    state: State,
    view: Option<WebView>,
    /// Renderer restarts since the environment came up, so a crash loop stops.
    reloads: u32,
}

impl Host {
    const fn new() -> Self {
        Self {
            state: State::Idle,
            view: None,
            reloads: 0,
        }
    }

    /// Move to the next state, logging only real transitions.
    fn set(&mut self, next: State) {
        if self.state != next {
            self.state = next;
            log(&format!("webview2 state: {next:?}"));
        }
    }
}

thread_local! {
    /// WebView2 is apartment-bound to the thread that created it, so the host
    /// lives with that thread rather than in app state.
    static HOST: RefCell<Host> = const { RefCell::new(Host::new()) };
    /// The Rune window the page is parented to.
    static PARENT: Cell<isize> = const { Cell::new(0) };
    /// The client size last handed to the controller, so a still window is not
    /// repositioned every frame.
    static LAST: Cell<(i32, i32)> = const { Cell::new((-1, -1)) };
}

pub struct WebView {
    controller: ICoreWebView2Controller,
    core: ICoreWebView2,
}

impl WebView {
    pub fn set_bounds(&self, rect: RECT) -> bool {
        unsafe { self.controller.SetBounds(rect).is_ok() }
    }

    pub fn reload(&self) -> bool {
        unsafe { self.core.Reload().is_ok() }
    }
}

impl Drop for WebView {
    fn drop(&mut self) {
        unsafe {
            let _ = self.controller.Close();
        }
    }
}

/// Append one line to a bounded diagnostic log in the temp directory. The file
/// is truncated once it reaches the cap. URLs are never written: callers pass
/// [redacted] so a query string or token cannot reach the log.
pub fn log(line: &str) {
    use std::io::Write;
    let path = std::env::temp_dir().join("rune-webview.log");
    let append = std::fs::metadata(&path).map_or(true, |meta| meta.len() < 65_536);
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .truncate(!append)
        .open(path)
    {
        let _ = writeln!(file, "{line}");
    }
}

/// A URL's scheme and host only: enough to diagnose, nothing to leak.
fn redacted(url: &url::Url) -> String {
    match url.host_str() {
        Some(host) => format!("{}://{host}", url.scheme()),
        None => url.scheme().to_string(),
    }
}

/// Remember the window the page belongs to.
pub fn set_parent(hwnd: HWND) {
    PARENT.with(|parent| parent.set(hwnd.0 as isize));
}

fn parent() -> Option<HWND> {
    PARENT.with(|parent| {
        let raw = parent.get();
        (raw != 0).then(|| HWND(raw as *mut _))
    })
}

/// The page's current state, for the view to render.
pub fn state() -> State {
    HOST.with(|host| {
        host.try_borrow()
            .map(|host| host.state)
            .unwrap_or(State::Failed)
    })
}

fn transition(next: State) {
    HOST.with(|host| {
        if let Ok(mut host) = host.try_borrow_mut() {
            host.set(next);
        }
    });
}

/// Record a failure and move to [State::Failed], so the app stops waiting.
fn fail(why: &str) {
    log(&format!("webview2 failed: {why}"));
    transition(State::Failed);
}

/// Hand a URL to the default browser off the COM thread, so a slow process
/// launch never runs inside a WebView2 callback.
fn open_externally(url: url::Url) {
    static OPENER: OnceLock<Sender<url::Url>> = OnceLock::new();
    let sender = OPENER.get_or_init(|| {
        let (sender, receiver) = std::sync::mpsc::channel::<url::Url>();
        std::thread::spawn(move || {
            while let Ok(url) = receiver.recv() {
                let result = open::that_detached(url.as_str());
                log(&format!("open {} -> {result:?}", redacted(&url)));
            }
        });
        sender
    });
    let _ = sender.send(url);
}

/// Open the app's own address in the browser, for the failure surface.
pub fn open_in_browser() {
    if let Ok(url) = url::Url::parse(crate::TARGET_URL) {
        open_externally(url);
    }
}

/// Start creating the WebView2 under the Rune window. Returns at once; only the
/// first call for a live window does anything.
pub fn start(parent: HWND, url: &'static str) {
    set_parent(parent);
    let begun = HOST.with(|host| {
        let Ok(mut host) = host.try_borrow_mut() else {
            return false;
        };
        if host.state != State::Idle && host.state != State::Failed {
            return false;
        }
        host.set(State::Starting);
        true
    });
    if !begun {
        return;
    }
    create_environment(parent, url, profile_folder(), true);
}

/// The WebView2 profile the previous (Tauri) build used, so the session,
/// cookies and sign-in carry over instead of starting signed out.
fn profile_folder() -> Option<Vec<u16>> {
    let base = std::env::var_os("LOCALAPPDATA")?;
    // WebView2 appends "EBWebView" to the folder it is given, so this is the
    // parent the previous build passed, not the EBWebView folder itself.
    let path = std::path::PathBuf::from(base).join("com.chatgpt.desktop");
    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    wide.push(0);
    Some(wide)
}

/// Create the WebView2 environment. A shared profile is locked while another
/// process holds it, so a failure falls back to the default profile rather than
/// leaving a blank window.
fn create_environment(parent: HWND, url: &'static str, folder: Option<Vec<u16>>, fallback: bool) {
    let handler = CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(
        move |error, environment| {
            if error.is_ok() {
                if let Some(environment) = environment {
                    create_controller(parent, environment, url);
                    return Ok(());
                }
            }
            if fallback {
                log(&format!(
                    "shared profile unavailable ({error:?}); using the default profile"
                ));
                create_environment(parent, url, None, false);
            } else {
                fail(&format!("environment: {error:?}"));
            }
            Ok(())
        },
    ));
    let folder = match &folder {
        Some(wide) => PCWSTR(wide.as_ptr()),
        None => PCWSTR::null(),
    };
    let immediate = unsafe {
        CreateCoreWebView2EnvironmentWithOptions(
            PCWSTR::null(),
            folder,
            None::<&ICoreWebView2EnvironmentOptions>,
            &handler,
        )
    };
    // A synchronous failure never invokes the handler, so it is handled here.
    if let Err(error) = immediate {
        if fallback {
            log(&format!(
                "environment call failed ({error:?}); using the default profile"
            ));
            create_environment(parent, url, None, false);
        } else {
            fail(&format!("environment: {error:?}"));
        }
    }
}

fn create_controller(parent: HWND, environment: ICoreWebView2Environment, url: &'static str) {
    let handler =
        CreateCoreWebView2ControllerCompletedHandler::create(Box::new(move |error, controller| {
            if error.is_err() {
                fail(&format!("controller: {error:?}"));
                return Ok(());
            }
            let Some(controller) = controller else {
                fail("controller: no controller");
                return Ok(());
            };
            let core = match unsafe { controller.CoreWebView2() } {
                Ok(core) => core,
                Err(error) => {
                    fail(&format!("core webview: {error:?}"));
                    return Ok(());
                }
            };
            install_link_handlers(&core);
            install_process_handlers(&core);
            if let Err(error) = unsafe { controller.SetIsVisible(true) } {
                log(&format!("set visible failed: {error:?}"));
            }
            if let Err(error) = unsafe { core.Navigate(&HSTRING::from(url)) } {
                log(&format!("navigate failed: {error:?}"));
            }
            HOST.with(|host| {
                if let Ok(mut host) = host.try_borrow_mut() {
                    host.view = Some(WebView { controller, core });
                    host.set(State::Ready);
                }
            });
            // Size the page now that there is something to size.
            LAST.with(|last| last.set((-1, -1)));
            sync_bounds();
            Ok(())
        }));
    let immediate = unsafe { environment.CreateCoreWebView2Controller(parent, &handler) };
    if let Err(error) = immediate {
        fail(&format!("controller call: {error:?}"));
    }
}

/// Restart the page when a renderer dies, and stop before a crash loop.
fn install_process_handlers(core: &ICoreWebView2) {
    let handler = ProcessFailedEventHandler::create(Box::new(|_sender, args| {
        let Some(args) = args else {
            return Ok(());
        };
        let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND(0);
        if let Err(error) = unsafe { args.ProcessFailedKind(&mut kind) } {
            log(&format!("process failed, kind unreadable: {error:?}"));
            fail("process failed");
            return Ok(());
        }
        recover(kind);
        Ok(())
    }));
    let mut token = 0;
    if let Err(error) = unsafe { core.add_ProcessFailed(&handler, &mut token) } {
        log(&format!("add_ProcessFailed failed: {error:?}"));
    }
}

/// A dead renderer is reloadable; a dead browser process is not.
fn recover(kind: COREWEBVIEW2_PROCESS_FAILED_KIND) {
    let reloadable = kind.0 == COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED.0
        || kind.0 == COREWEBVIEW2_PROCESS_FAILED_KIND_FRAME_RENDER_PROCESS_EXITED.0;
    if !reloadable {
        log(&format!("process failed: kind {}", kind.0));
        fail(&format!("process failed: kind {}", kind.0));
        return;
    }
    let attempts = HOST.with(|host| {
        let Ok(mut host) = host.try_borrow_mut() else {
            return 0;
        };
        host.reloads += 1;
        host.reloads
    });
    if attempts > 3 {
        log("renderer failed repeatedly; not reloading again");
        fail("renderer failed repeatedly");
        return;
    }
    log(&format!("renderer failed; reload {attempts}/3"));
    with(|web| web.reload());
}

/// Apply the link policy to both WebView2 events that decide where a link goes.
fn install_link_handlers(core: &ICoreWebView2) {
    let new_window = NewWindowRequestedEventHandler::create(Box::new(|sender, args| {
        let Some(args) = args else {
            return Ok(());
        };
        let mut raw = PWSTR::null();
        if let Err(error) = unsafe { args.Uri(&mut raw) } {
            log(&format!("new window without a uri: {error:?}"));
            return Ok(());
        }
        let uri = take_pwstr(raw);
        let Ok(url) = url::Url::parse(&uri) else {
            unsafe {
                let _ = args.SetHandled(true);
            }
            return Ok(());
        };
        match ChatGptPolicy.popup(ChatGptPolicy.classify(&url)) {
            // Sign-in and the page's own schemes: WebView2 opens its in-profile
            // popup, which keeps the session the flow is establishing.
            Popup::Allow => {}
            // Another page of the app: keep it in the one window there is.
            Popup::NavigateMain => {
                unsafe {
                    let _ = args.SetHandled(true);
                }
                if let Some(sender) = sender {
                    let _ = unsafe { sender.Navigate(&HSTRING::from(url.as_str())) };
                }
            }
            Popup::OpenExternally => {
                unsafe {
                    let _ = args.SetHandled(true);
                }
                open_externally(url);
            }
            Popup::Block => {
                unsafe {
                    let _ = args.SetHandled(true);
                }
                log(&format!("blocked popup to {}", redacted(&url)));
            }
        }
        Ok(())
    }));
    let mut token = 0;
    if let Err(error) = unsafe { core.add_NewWindowRequested(&new_window, &mut token) } {
        log(&format!("add_NewWindowRequested failed: {error:?}"));
    }

    let navigating = NavigationStartingEventHandler::create(Box::new(|_sender, args| {
        let Some(args) = args else {
            return Ok(());
        };
        let mut raw = PWSTR::null();
        if let Err(error) = unsafe { args.Uri(&mut raw) } {
            log(&format!("navigation without a uri: {error:?}"));
            return Ok(());
        }
        let uri = take_pwstr(raw);
        let Ok(url) = url::Url::parse(&uri) else {
            return Ok(());
        };
        match ChatGptPolicy.navigation(ChatGptPolicy.classify(&url)) {
            Navigation::Allow => {}
            Navigation::OpenExternally => {
                unsafe {
                    let _ = args.SetCancel(true);
                }
                open_externally(url);
            }
            Navigation::Block => {
                unsafe {
                    let _ = args.SetCancel(true);
                }
                log(&format!("blocked navigation to {}", redacted(&url)));
            }
        }
        Ok(())
    }));
    let mut token = 0;
    if let Err(error) = unsafe { core.add_NavigationStarting(&navigating, &mut token) } {
        log(&format!("add_NavigationStarting failed: {error:?}"));
    }
}

/// Run a closure against the live controller, if there is one.
pub fn with<R>(f: impl FnOnce(&WebView) -> R) -> Option<R> {
    HOST.with(|host| {
        host.try_borrow()
            .ok()
            .and_then(|host| host.view.as_ref().map(f))
    })
}

/// The page's rect in physical pixels: the whole client area. Pure, so the
/// math is testable without a window.
fn page_rect(client: (i32, i32)) -> (i32, i32, i32, i32) {
    (0, 0, client.0.max(0), client.1.max(0))
}

/// Size the page to the client area. The cache is only updated once the bounds
/// were actually applied.
pub fn sync_bounds() -> bool {
    let Some(parent) = parent() else {
        return false;
    };
    let mut client = RECT::default();
    let _ = unsafe { GetClientRect(parent, &mut client) };
    if client.right <= 0 || client.bottom <= 0 {
        return false;
    }
    let key = (client.right, client.bottom);
    if LAST.with(|last| last.get()) == key {
        return true;
    }
    let (left, top, right, bottom) = page_rect(key);
    let applied = with(|web| {
        web.set_bounds(RECT {
            left,
            top,
            right,
            bottom,
        })
    })
    .unwrap_or(false);
    if applied {
        LAST.with(|last| last.set(key));
    }
    applied
}

#[cfg(test)]
mod tests {
    use super::{page_rect, redacted};

    #[test]
    fn the_page_fills_the_client_area() {
        assert_eq!(page_rect((1475, 950)), (0, 0, 1475, 950));
        assert_eq!(page_rect((982, 653)), (0, 0, 982, 653));
    }

    #[test]
    fn a_degenerate_client_area_never_inverts() {
        assert_eq!(page_rect((0, 0)), (0, 0, 0, 0));
        assert_eq!(page_rect((-5, -5)), (0, 0, 0, 0));
    }

    #[test]
    fn a_logged_url_keeps_no_path_or_query() {
        let url: url::Url = "https://chatgpt.com/c/6af7?token=secret&email=a@b.c"
            .parse()
            .unwrap();
        assert_eq!(redacted(&url), "https://chatgpt.com");
        let blob: url::Url = "blob:https://chatgpt.com/9f2b".parse().unwrap();
        assert_eq!(redacted(&blob), "blob");
    }
}
