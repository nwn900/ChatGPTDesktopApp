# ChatGPT Desktop

A Windows desktop app for ChatGPT, built with Rust, Tauri 2 and Microsoft Edge WebView2.

![ChatGPT](icon.png)

## Downloads

[Download v1.0.12](https://github.com/nwn900/ChatGPTDesktopApp/releases/tag/v1.0.12) — Windows x64 NSIS installer, compiled locally.

## Features

- System tray, single instance, and hide on close.
- Manual launch opens the window; Windows startup uses the explicit `--autostart` argument.
- Answer-completion notifications while the main window is inactive. Click a notification to restore the app.
- Links open in the Windows default browser. Sign-in pages stay in the app, and file-download dialogs are native.
- Transparent application icon.

Completion detection observes page mutations and reads text without forcing layout. It does not continually scan an idle page. Selectors live in `src-tauri/src/notifications.js`; service redesigns can require selector updates. Native delivery status is recorded in a bounded `notifications.log` under the application's log directory.

A click on a link the app does not own is routed explicitly to the operating system instead of being left to the webview's new-window handling, so it cannot end up in an in-app window or in the ChatGPT window itself. The keep-in-app host list lives in `src-tauri/src/links.rs` and is injected into the page; every external launch is recorded in a bounded `links.log` next to `notifications.log`.

## Build locally

Install Rust's MSVC toolchain, Visual Studio C++ Build Tools, WebView2, and the Tauri CLI (`cargo install tauri-cli --locked`). Use a Visual Studio developer shell with `CC=cl.exe` and `CXX=cl.exe`.

```powershell
cd src-tauri
cargo test --locked
cargo tauri build -- --locked
```

The installer is generated in `target/release/bundle/nsis/`. To limit disk usage, set `CARGO_TARGET_DIR` to a dedicated directory on a drive with free space, set `CARGO_INCREMENTAL=0`, and build one app at a time. Copy completed installers outside the target directory before running `cargo clean`.

Run JavaScript regression tests with `node --test tests/*.test.cjs` (17 tests: page watcher and link routing).

Releases are uploaded from local builds. Tag pushes do not run a GitHub installer build.

## Verification limits

Automated tests cover wrapper logic and simulated page signals. Signed-in provider flows, real response completion, Windows notification visibility, and rendering timings require live verification. Windows notification settings and Do Not Disturb can suppress visible banners.
