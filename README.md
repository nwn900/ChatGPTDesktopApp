# ChatGPT Desktop

A native Windows desktop app for ChatGPT. [Rune](https://github.com/anomalyco/rune)
draws the window and the system tray; Microsoft Edge WebView2 shows the
chatgpt.com page.

![ChatGPT](icon.png)

## Downloads

[Download v1.0.17](https://github.com/nwn900/ChatGPTDesktopApp/releases/tag/v1.0.17)
— Windows x64 NSIS installer, compiled locally.

## What it is

Rune has no webview, and the chat surface should be ChatGPT itself rather than a
reimplementation, so the two are combined:

- The window is a **Rune window** (winit + wgpu). Rune owns the event loop, the
  input and the tray icon.
- The page is a **WebView2 child HWND** parented to that window and sized to the
  whole client area.

Rune draws no chrome: no rail, no top bar, no frame. The page is the window.
Clicks and keys go straight to WebView2, and the page keeps its own session,
cookies and streaming.

## Features

- Closing the window hides it; the app keeps running in the tray. The tray icon
  reopens it.
- Tray menu: **Open ChatGPT**, **Launch at system startup: On/Off**, **Quit**.
  The startup item writes the per-user Run key, so it needs no elevation.
- Started from that Run key the app passes `--autostart` and opens in the tray
  with no window.
- Every link opens in the Windows default browser. A `target=_blank` link or a
  `window.open` is marked handled so WebView2 never opens a second window, and
  a plain click that would replace the page is cancelled; both hand the URL to
  the operating system. Sign-in hosts stay inside the app so authentication
  keeps working.
- The page uses the same WebView2 profile as the previous build
  (`%LOCALAPPDATA%\com.chatgpt.desktop`), so the sign-in and conversation list
  carry over. If another process holds the profile the app falls back to the
  default one rather than showing a blank window.
- The project icon is embedded in the executable, so the title bar, taskbar,
  Start Menu shortcut, Desktop shortcut and Add/Remove Programs all show it.

## Layout

- `rune-app/` — the current app: Rune for the UI, Edge WebView2 for the page.
- `src-tauri/` — the previous Tauri build, kept for reference.

## Build locally

Rune is pre-release and is consumed from a checkout. Clone it beside this
repository:

~~~powershell
git clone https://github.com/anomalyco/rune.git ..\..\rune-src
cd rune-app
cargo test
cargo build --release
cd installer
& "$env:LOCALAPPDATA\tauri\NSIS\makensis.exe" chatgpt.nsi
~~~

That writes `rune-app\dist\ChatGPT_1.0.17_x64-setup.exe`. It installs per-user
into %LOCALAPPDATA%\ChatGPT, so it needs no elevation, and adds a Desktop
shortcut, a Start Menu entry and an Add/Remove Programs entry with an
uninstaller. Run it with /S for a silent install.

Rune ships patched copies of four crates.io packages, so an app that builds it
has to republish those patches — that is the `[patch.crates-io]` block in
`rune-app/Cargo.toml`. Pin a Rune `dev` rev rather than tracking `main`.

Build with `CC`/`CXX` unset (or set to `cl.exe`). A global `CXX=g++`
makes `vswhom-sys` emit a MinGW object that MSVC `link.exe` rejects with
`LNK1143`.

The JavaScript regression tests for the previous Tauri wrapper run with
`node --test tests/*.test.cjs`.

## Verification limits

Automated tests cover the link policy, the placement math and the page signals.
Signed-in provider flows, real response completion, rendering timings and the
system tray need live verification. Windows notification settings and Do Not
Disturb can suppress banners.

The current build has no answer-completion notifications — that watcher lives in
the previous Tauri build under `src-tauri/` — and no single-instance guard, so
launching the app twice starts two processes.
