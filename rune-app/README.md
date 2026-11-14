# ChatGPT (Rune + WebView2)

The desktop app rebuilt with [Rune](https://github.com/anomalyco/rune) hosting
the window and Edge WebView2 showing the page.

## How it fits together

Rune has no webview, and the chat surface is the real chatgpt.com page, so the
two are combined rather than replaced:

- The window is a **Rune window** (winit + wgpu). Rune owns the window, its
  event loop, its input and the tray icon.
- The page is a **WebView2 child HWND** parented to that window and sized to
  the whole client area, so it is the real chatgpt.com page, not a
  reimplementation of it.

Rune draws no chrome: no rail, no top bar, no backdrop, no frame. The page is
the window. Clicks and keys go straight to WebView2, and the page keeps its own
session, cookies and streaming.

## Tray, startup and session

- Closing the window hides it; the app keeps running in the tray. The tray icon
  reopens it.
- The tray menu has **Open ChatGPT**, **Launch at system startup: On/Off**, and
  **Quit**. The startup item toggles the per-user Run key
  (HKCU\Software\Microsoft\Windows\CurrentVersion\Run), so it needs no
  elevation, and the label shows the state it is in.
- Started from that Run key the app passes `--autostart` and opens in the tray
  with no window; the page loads the first time the window is opened.
- The page uses the **same WebView2 profile as the previous (Tauri) build**,
  %LOCALAPPDATA%\com.chatgpt.desktop, so the sign-in and conversation list carry
  over. WebView2 appends EBWebView to that folder itself. If another process
  holds the profile, the app falls back to the default one rather than showing
  a blank window.
- Icons all come from the project's own assets, and all are embedded, so the
  installer ships one file:
  - build.rs embeds src-tauri/icons/icon.ico as the executable's icon resource
    (plus ProductName, FileDescription and FileVersion). Without it the exe
    carries no icon, and Explorer, the Start Menu shortcut and Add/Remove
    Programs all fall back to the generic Windows application icon.
  - rune::set_app_icon draws the window, taskbar and Alt+Tab icon from
    src-tauri/icons/icon.png (512 px).
  - The tray icon is src-tauri/icons/32x32.png.

## Links

Every link leaves the app for the operating system's default browser. Two
WebView2 events cover it, in src/webview.rs:

- `NewWindowRequested` is a link with `target=_blank` or a `window.open`. Left
  alone, WebView2 opens its own window for it — the in-app browser window this
  app must never show — so it is always marked handled and the URL goes to the
  browser.
- `NavigationStarting` is a plain click that would replace the page with a
  foreign site. That navigation is cancelled and the URL goes to the browser, so
  the page stays where it was.

src/links.rs decides what counts as foreign: chatgpt.com and the identity
providers its sign-in flow uses stay in the app, and every other http(s) host
plus mailto leaves. `about:blank`, `blob:`, `data:` and `javascript:` are left
to the webview so the page keeps working. Each hand-off is logged.

## Wiring

1. `App::on_ready` runs once the first frame is ready. It hands over a
   `WindowHandle`, and `with_raw_handle` gives the native handle; the app
   takes the `HWND` from `RawWindowHandle::Win32`.
2. `webview::start` begins WebView2 setup and returns immediately. WebView2
   completes through the message loop, so nothing blocks the frame and no
   message pump is re-entered from inside a Rune callback.
3. `poll` calls `webview::sync_bounds` every frame.
4. `sync_bounds` sizes the page to the client area in **physical** pixels
   (`GetClientRect`), and caches the size so a still window is not
   repositioned every frame. The cache is only written once the rect was
   actually applied: caching it before the controller exists would make every
   later call skip `SetBounds` and leave the page at its default zero size.

## Run it

Rune is pre-release and is consumed from a checkout. Clone it beside this
repository:

~~~powershell
git clone https://github.com/anomalyco/rune.git ..\..\rune-src
~~~

Cargo.toml points at ../../rune-src for the `rune` crate and for the four
crates Rune patches (`wgpu-hal`, `winit`, `parley_engine`,
`accesskit_windows`). A dependency's `[patch]` does not apply to the
workspace that depends on it, so an app that builds Rune has to patch them too.
Pin a `dev` rev rather than tracking `main`.

~~~powershell
cargo run
cargo build --release
~~~

## Command line

- `--autostart` opens in the tray with no window. The Run key uses it.
- `--startup on` / `--startup off` sets the Run value and exits, for a
  script or an installer that should not go through the tray.

## Installer

~~~powershell
cargo build --release
cd installer
& "$env:LOCALAPPDATA\tauri\NSIS\makensis.exe" chatgpt.nsi
~~~

That writes dist\ChatGPT_1.0.15_x64-setup.exe. It installs per-user into
%LOCALAPPDATA%\ChatGPT, so it needs no elevation, and adds a Desktop
shortcut, a Start Menu entry and an Add/Remove Programs entry with an
uninstaller. Run it with /S for a silent install.

## Diagnostics

`webview::log` appends to `%TEMP%\rune-webview.log`: the startup state, the
tray result, when the controller comes up, and each applied rect. The file is
truncated at 64 KB.

## Verification

~~~powershell
cargo test
~~~

Two tests cover the placement math: the page fills the client area, and a
degenerate client area clamps to an empty rect instead of inverting.

Checked by hand on the installed build: the window opens with the project icon,
the page loads chatgpt.com edge to edge signed in, closing hides it while the
process stays alive, the tray icon is created, `--startup on` writes
`"<installed exe>" --autostart` and the next launch reports the state as on,
`--startup off` removes it, and `--autostart` starts with no visible window.

## Not done yet

- Single instance: launching the app again starts a second process.
- No notification watcher.
- No navigation controls: the page is loaded once at startup.
