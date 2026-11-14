//! Edge WebView2 hosted as a child window of the Rune window.
//!
//! The page is a real WebView2 child HWND parented to the Rune window and
//! sized to the whole client area, so it is the real chatgpt.com page rather
//! than a reimplementation of it. Creation is asynchronous on purpose:
//! WebView2 completes through the message loop, so nothing here blocks the
//! frame that starts it, and no message pump is re-entered from inside a Rune
//! callback.

use std::cell::{Cell, RefCell};
use std::os::windows::ffi::OsStrExt;

use webview2_com::Microsoft::Web::WebView2::Win32::{
    CreateCoreWebView2EnvironmentWithOptions, ICoreWebView2, ICoreWebView2Controller,
    ICoreWebView2Environment, ICoreWebView2EnvironmentOptions,
};
use webview2_com::{
    CreateCoreWebView2ControllerCompletedHandler, CreateCoreWebView2EnvironmentCompletedHandler,
    NavigationStartingEventHandler, NewWindowRequestedEventHandler, take_pwstr,
};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::UI::WindowsAndMessaging::GetClientRect;
use windows::core::{HSTRING, PCWSTR, PWSTR};

thread_local! {
    /// The live controller. WebView2 is apartment-bound to the thread that
    /// created it, so it lives with that thread rather than in app state.
    static LIVE: RefCell<Option<WebView>> = const { RefCell::new(None) };
    /// The Rune window the page is parented to.
    static PARENT: Cell<isize> = const { Cell::new(0) };
    /// The client size last handed to the controller, so a still window is not
    /// repositioned every frame.
    static LAST: Cell<(i32, i32)> = const { Cell::new((-1, -1)) };
}

pub struct WebView {
    controller: ICoreWebView2Controller,
}

impl WebView {
    pub fn set_bounds(&self, rect: RECT) {
        unsafe {
            let _ = self.controller.SetBounds(rect);
        }
    }
}

impl Drop for WebView {
    fn drop(&mut self) {
        unsafe {
            let _ = self.controller.Close();
        }
    }
}

/// Append one line to a bounded diagnostic log in the temp directory. The
/// file is truncated once it reaches the cap, so a long session cannot grow it
/// without limit.
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

/// Start creating the WebView2 under the Rune window. Returns at once; the
/// controller lands in the slot when WebView2 finishes its setup.
pub fn start(parent: HWND, url: &'static str) {
    set_parent(parent);
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
/// process holds it, so a failure falls back to the default profile rather
/// than leaving a blank window.
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
                log(&format!("webview2 environment failed: {error:?}"));
            }
            Ok(())
        },
    ));
    let folder = match &folder {
        Some(wide) => PCWSTR(wide.as_ptr()),
        None => PCWSTR::null(),
    };
    unsafe {
        let _ = CreateCoreWebView2EnvironmentWithOptions(
            PCWSTR::null(),
            folder,
            None::<&ICoreWebView2EnvironmentOptions>,
            &handler,
        );
    }
}

fn create_controller(parent: HWND, environment: ICoreWebView2Environment, url: &'static str) {
    let handler =
        CreateCoreWebView2ControllerCompletedHandler::create(Box::new(move |error, controller| {
            if error.is_err() {
                return Ok(());
            }
            let Some(controller) = controller else {
                return Ok(());
            };
            let core = unsafe { controller.CoreWebView2() }?;
            install_link_handlers(&core);
            unsafe {
                let _ = core.Navigate(&HSTRING::from(url));
                let _ = controller.SetIsVisible(true);
            }
            log("controller ready");
            LIVE.with(|live| {
                if let Ok(mut live) = live.try_borrow_mut() {
                    *live = Some(WebView { controller });
                }
            });
            Ok(())
        }));
    unsafe {
        let _ = environment.CreateCoreWebView2Controller(parent, &handler);
    }
}

/// Send every link the app does not own to the system browser.
///
/// Two events cover it. `NewWindowRequested` is a link with `target=_blank` or
/// a `window.open`; left alone WebView2 opens its own window for it, which is
/// the in-app browser window this app must never show, so it is always marked
/// handled. `NavigationStarting` is a plain click that would replace the page
/// with a foreign site; that navigation is cancelled. Both then hand the URL to
/// the operating system's default browser.
fn install_link_handlers(core: &ICoreWebView2) {
    let new_window = NewWindowRequestedEventHandler::create(Box::new(|_sender, args| {
        if let Some(args) = args {
            let mut uri = PWSTR::null();
            unsafe { args.Uri(&mut uri)? };
            let uri = take_pwstr(uri);
            // Handled either way: WebView2 must not open a window of its own.
            unsafe { args.SetHandled(true)? };
            open_externally(&uri);
        }
        Ok(())
    }));
    let mut token = 0;
    unsafe {
        let _ = core.add_NewWindowRequested(&new_window, &mut token);
    }

    let navigating = NavigationStartingEventHandler::create(Box::new(|_sender, args| {
        if let Some(args) = args {
            let mut uri = PWSTR::null();
            unsafe { args.Uri(&mut uri)? };
            let uri = take_pwstr(uri);
            if let Ok(url) = url::Url::parse(&uri) {
                if crate::links::opens_in_browser(&url) {
                    // Keep the page where it is, then hand the link out.
                    unsafe { args.SetCancel(true)? };
                    open_externally(&uri);
                }
            }
        }
        Ok(())
    }));
    let mut token = 0;
    unsafe {
        let _ = core.add_NavigationStarting(&navigating, &mut token);
    }
}

/// Hand a URL to the default browser, logging the outcome.
fn open_externally(uri: &str) {
    let Ok(url) = url::Url::parse(uri) else {
        return;
    };
    if !crate::links::opens_in_browser(&url) {
        return;
    }
    let result = open::that_detached(url.as_str());
    log(&format!("open {url} -> {result:?}"));
}

/// Run `f` against the live controller, if it has finished setting up.
pub fn with<R>(f: impl FnOnce(&WebView) -> R) -> Option<R> {
    LIVE.with(|live| live.try_borrow().ok().and_then(|live| live.as_ref().map(f)))
}

/// The page's rect in physical pixels: the whole client area. Pure, so the
/// math is testable without a window.
fn page_rect(client: (i32, i32)) -> (i32, i32, i32, i32) {
    (0, 0, client.0.max(0), client.1.max(0))
}

/// Size the page to the client area. Returns whether a controller was there to
/// move, so the caller can tell "not ready yet" from "already placed".
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
    .is_some();
    // Only remember the size once it was actually applied. Caching it before
    // the controller exists would make every later call skip SetBounds and
    // leave the page at its default zero size.
    if applied {
        LAST.with(|last| last.set(key));
        log(&format!(
            "bounds {left},{top},{right},{bottom} client {right}x{bottom}"
        ));
    }
    applied
}

#[cfg(test)]
mod tests {
    use super::page_rect;

    #[test]
    fn the_page_fills_the_client_area() {
        assert_eq!(page_rect((1475, 950)), (0, 0, 1475, 950));
        assert_eq!(page_rect((982, 653)), (0, 0, 982, 653));
    }

    #[test]
    fn a_degenerate_client_area_never_inverts() {
        // A window with no client area yet must not produce a negative rect.
        assert_eq!(page_rect((0, 0)), (0, 0, 0, 0));
        assert_eq!(page_rect((-5, -5)), (0, 0, 0, 0));
    }
}
