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
