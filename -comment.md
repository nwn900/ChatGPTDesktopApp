## v1.0.16 — P0 done, P1 partly done

Shipped in [v1.0.16](https://github.com/nwn900/ChatGPTDesktopApp/releases/tag/v1.0.16) (commit 6e39c6c). The issue stays open: the performance work and the remaining parity items are not done.

### Done

- **P0.1 single instance.** `single_instance.rs` takes a per-session named mutex before Rune, the tray or WebView2 exist; a second launch signals a named activation event and exits. `--quit` lets the installer replace a running copy. Measured: four launches gave one process, and a launch while the window was hidden to the tray restored it.
- **P0.2 link policy.** The bug was real and worse than "swallowed": every `NewWindowRequested` was marked handled, so `target=_blank` to chatgpt.com, the sign-in providers and `about:blank` was dropped — exactly where OAuth popups start. `links.rs` is now a `LinkPolicy` trait over a `LinkKind`, with separate popup and navigation decisions. chatgpt.com popups navigate the one window; sign-in and `about:blank`/`blob:`/`data:` popups stay in-profile for their session; external hosts and mailto go to the default browser; only `javascript:`, `file:` and unknown schemes are dropped. The hand-off runs on its own thread. Verified over CDP.
- **P1.3 lifecycle and failure surface.** `webview.rs` is an `Idle`/`Starting`/`Ready`/`Failed` state machine; every failure path records a reason, synchronous creation failures are handled separately, and a second start for a live window does nothing. The window shows a native failure view with an **Open in browser** button instead of staying blank. Hide-to-tray is only enabled when the tray exists.
- **P1.4 autostart and diagnostics.** The startup state compares the stored Run value with this executable's command, so a stale entry reads as disabled; `--startup` exits non-zero on failure; logged URLs keep only scheme and host.
- **P1.6 CI.** The workflow now checks out Rune at the pinned revision, runs fmt/clippy/test/build in `rune-app`, builds the Rune installer, asserts `ProductName`/`FileVersion`, and uploads it. Still `workflow_dispatch`, never publishes.
- **P1.7 installer.** Quits a running copy, quotes `UninstallString`, GUID uninstall key, removes its own Run value on uninstall, warns when the Evergreen Runtime is missing, leaves the profile alone. Verified upgrade-over-running and uninstall.
- **P2.8 (partial).** Tray **Reload**.

### Not done

- **P1.5 performance.** No benchmark and no `put_IsVisible(false)` / `MemoryUsageTargetLevel` controls yet. No performance claim is made.
- **P1.3 crash recovery.** `ProcessFailed` is not wired; a renderer crash ends at the failure surface rather than recovering.
- **P2.8 remainder.** Notifications, download UX, accessibility, manual test matrix.

10 unit tests cover the policy, redaction, placement and the autostart command.
