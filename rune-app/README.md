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

## One instance

A named mutex (`Local\ChatGPT.SingleInstance`) decides who is first. A second
launch signals a named event and exits, so there is never a second tray icon or
a second WebView2 tree; the first instance shows its window. `--quit` asks the
running instance to exit, which is how the installer replaces a running copy.

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

## Notifications

A finished answer raises a Windows toast, and clicking it restores the window.

- `src/notifications.js` is injected into every document the page creates. A
  MutationObserver marks the page dirty, a debounced scan reads the last
  assistant message's `textContent` (which does not force layout), and the scan
  backs off from 1 s to 3 s while the page is hidden. Only the last 300
  characters are sent, once per completion.
- The page posts `notify\n<body>` over `window.chrome.webview`, and
  `WebMessageReceived` turns it into a toast.
- The toast is suppressed while the window is visible and in front, so it only
  fires when the answer finished while you were elsewhere.
- The toast uses the same app id the previous build used, so the Windows
  notification registration carries over.

## Links

src/links.rs is an explicit policy rather than ad-hoc host checks. A
`LinkPolicy` classifies a URL into a `LinkKind`, and the two decisions the app
needs are separate methods, so the rules are testable with no window and no
WebView2:

| `LinkKind` | a `NewWindowRequested` popup | a `NavigationStarting` |
| --- | --- | --- |
| `Main` (chatgpt.com) | navigate the one window there is | allow |
| `Auth` (sign-in providers) | let WebView2 open it in-profile | allow |
| `Internal` (about:blank, blob:, data:) | let WebView2 open it | allow |
| `External` (any other host, mailto) | open in the default browser | cancel, open in the browser |
| `Blocked` (javascript:, file:, …) | drop | cancel |

Nothing the page opens is silently dropped: the only things that are dropped
are the schemes that must never be followed. A ChatGPT `target=_blank` link keeps
one window instead of opening a second ChatGPT surface, and sign-in popups stay
in-profile so the flow keeps the session it is establishing.

The external-browser hand-off runs on its own thread, so a slow process launch
never happens inside a WebView2 callback. Logged URLs are reduced to scheme and
host: no path, no query string, no token.

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

That writes dist\ChatGPT_1.0.18_x64-setup.exe. It installs per-user into
%LOCALAPPDATA%\ChatGPT, so it needs no elevation, and adds a Desktop
shortcut, a Start Menu entry and an Add/Remove Programs entry with an
uninstaller. Run it with /S for a silent install.

On upgrade it asks a running copy to quit with %%--quit%% before replacing the
executable. On uninstall it removes its own Run value, shortcuts and registry
key, and it leaves the WebView2 profile alone because that holds the sign-in.
The uninstall entry lives under a GUID key, not a generic "ChatGPT" name, and
%%UninstallString%% is quoted. If the Evergreen WebView2 Runtime is missing the
installer says so instead of leaving the app with a blank window.

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
