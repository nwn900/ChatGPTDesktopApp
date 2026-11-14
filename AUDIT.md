# Desktop wrapper audit — 2026-09-11

## Scope

Reviewed Rust startup, navigation, native commands, notification integration, injected JavaScript, icons, installer configuration and release automation. This is a bounded wrapper audit, not proof that every bug in the application or upstream service has been found.

## Corrections

- Replaced completion polling with a debounced page observer and textContent reads that do not force layout.
- Added origin-checked, bounded, rate-limited native notifications with click-to-restore and bounded delivery diagnostics.
- Kept native WebView2 IPC intact.
- Restricted remote pages to the one completion command through generated Tauri command permissions.
- Preserved native authentication popup handling, removing forced callback redirects and obsolete fixed browser user agents.
- Distinguished actual Windows autostart from manually opening the application.
- Restored minimized windows when opening from the tray or another instance.
- Restricted external URL launching to web/mail schemes.
- Preserved the existing transparent ChatGPT icon; no new matching Desktop source was available.
- Updated Tauri and committed reproducible dependency resolution.
- Documented sequential local builds on a dedicated drive; release assets must be preserved before cleaning generated output.
- Disabled tag-triggered installer builds and automated release publication.

## Validation and limits

Rust unit tests and Node simulated-DOM regression tests are run locally, followed by an optimized Windows x64 NSIS build. These checks cover wrapper logic and packaging, not signed-in end-to-end behavior.

No measured page-rendering speedup is claimed. The changes remove wrapper scanning/layout overhead; provider latency and WebView2 rendering still require before/after profiling. DOM selectors can drift with upstream redesigns.

Real provider authentication, streaming responses, notification banner visibility/click activation, media keys and installation upgrades still need live acceptance testing. Windows notification settings and Do Not Disturb can suppress banners. A complete dependency vulnerability audit was not performed.

## Rollback

Previous GitHub release installers remain available. Close the application through its tray menu before reinstalling the prior version if authentication, playback or notification regressions occur. Do not delete the WebView2 user profile. No profile migration or user-data cleanup is part of this release.

## v1.0.12 follow-up

A later follow-up fixed two user-visible defects in the wrapper. It did not repeat the full audit above.

- Link clicks now open the Windows default browser. The page intercepts a click on any link the app does not own and calls a native command; the webview new-window and navigation handlers stay as the fallback for scripted navigation. The keep-in-app host list (ChatGPT origins plus the sign-in providers) lives in `src-tauri/src/links.rs` and is injected into the page, and the native command re-checks the calling origin before launching anything.
- The Test Notification entry and its toast were removed from the tray menu. Delivery diagnostics are unchanged in `notifications.log`.
- External launches are appended to a bounded `links.log` beside `notifications.log`, through one shared writer.

Verification for the follow-up: Rust unit tests, simulated-DOM JavaScript tests and a local optimized installer build. A live click inside a signed-in conversation remains a manual check, because provider authentication cannot be automated here.

## v1.0.15 follow-up — tray resume hang

### What was wrong

Two independent defects combined into one unrecoverable state.

1. **The WebView2 freeze could not be undone.** `suspend.rs` (v1.0.14) suspended every `msedgewebview2.exe` process under the app with `NtSuspendProcess` 120 s after the window hid. `NtSuspendProcess` increments each thread's suspend count and `NtResumeProcess` decrements it by exactly one, so any imbalance — a second freeze, a thread Chromium had already suspended, a thread created after the suspend — left the tree suspended with no owner. `OpenProcess` failures were swallowed, the recorded PID list was never refreshed, and a `note_shown` that landed between the freeze thread's epoch check and its `freeze_tree` call resumed an empty list and then let the freeze complete. Nothing was verified, nothing was retried, and the subsystem wrote nothing to any log.

2. **The restore path blocked the event-loop thread on the shell.** `show_main_window` called `window.show()`, `unminimize()` and `set_focus()` directly from the tray handler, which runs on the event-loop thread. `ShowWindow` and `SetForegroundWindow` send synchronous messages to explorer.exe. A live capture of the wedged process showed the main thread blocked in `WctSendMessage` to `explorer.exe` tid 16156, which had itself been blocked for about 92 minutes; every window in the process reported `IsHungAppWindow = true`, so the tray icon could not deliver another click and the app could never recover.

### Observed state of the wedged instance

- `chatgpt-desktop-app.exe` alive with 4 threads; main thread blocked about 90 minutes in `SendMessage` to explorer.
- All six top-level windows hung. The `Tauri Window` existed but was hidden, so "resume" had nothing to show.
- Every `msedgewebview2.exe` thread under the app suspended (52/52, 73/73, 30/30, 28/28, 21/21, 11/11).
- Killing the app left 7 suspended `msedgewebview2.exe` orphans with a dead parent. They then fight the next launch over the same `EBWebView` profile, which is the "lagging heavily, then blank content" symptom.

### Fix

- `suspend.rs` is replaced by `lifecycle.rs`. Process suspension is removed entirely. Background cost is left to Chromium's own hidden-window throttling plus the watcher cadence in `notifications.js`.
- `restore_window` performs `ShowWindow`/`BringWindowToTop`/`SetForegroundWindow` on a worker thread, so a wedged shell can stall that worker instead of the whole app. Tauri's own `show()`/`unminimize()` still run afterwards to resync tao's bookkeeping; they are OS-level no-ops once the window is visible.
- `reap_orphans` runs at startup and terminates `msedgewebview2.exe` processes whose parent is gone, repeating until a pass finds nothing so cascading orphans (killing a browser process orphans its own children) are caught too.
- `start_watchdog` probes the event loop every 10 s and logs when it stops pumping, so the next occurrence is diagnosable.
- `lifecycle.log` records hidden/shown/reap/hang events beside the existing logs.
- Removed the unused `shell:allow-open` capability from `default.json`. It let the remote page open arbitrary URLs through the shell plugin, bypassing the origin-checked `open_external` command.

### Verification

- 8 Rust unit tests pass, including the orphan predicate: dead parent reaped, live parent spared, self and foreign images spared.
- 17 page tests pass (`node --test tests/*.test.cjs`).
- End-to-end on the built release binary: `WM_CLOSE` hides the window (hidden, not hung); launching a second instance restores it through the single-instance callback (visible, not hung); every WebView2 process reports 0 suspended threads; `lifecycle.log` shows `hidden` then `shown`; a window capture after restore is pixel-identical to the pre-hide capture, so the page is not blank.
- Orphan reaping observed live: killing the wedged v1.0.14 instance left 7 suspended orphans, and the next launch reaped them, including the crashpad handler that only became an orphan after its parent was killed.

### Build note

`vswhom-sys` is compiled by the `cc` crate. A global `CXX=g++` makes it emit a MinGW object that MSVC `link.exe` rejects with `LNK1143`. Build with `CC`/`CXX` unset or set to `cl.exe`, as the README already says.

