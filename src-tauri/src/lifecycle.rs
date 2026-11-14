//! Tray lifecycle: hide/show bookkeeping, orphaned WebView2 reaping, a
//! non-blocking window restore, and an event-loop hang watchdog.
//!
//! v1.0.14 froze the whole WebView2 process tree with `NtSuspendProcess`
//! while the window sat in the tray. That is gone and should not come back.
//! A suspended browser process cannot service the synchronous cross-process
//! messages the host UI thread sends it, cannot composite, and cannot notice
//! that its host died. The observed failures were a permanently hung app (the
//! tray handler never returned), a blank page after resume, and suspended
//! `msedgewebview2.exe` orphans that made the next launch slow and blank.
//! Background cost is handled by Chromium's own hidden-window throttling plus
//! the watcher cadence in `notifications.js`.
//!
//! The other half of the same failure class is blocking the event-loop thread
//! on the shell. `ShowWindow` and `SetForegroundWindow` both send
//! synchronous messages to explorer.exe, so a wedged shell freezes the whole
//! app. The restore path therefore runs off the event-loop thread.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Image name of every WebView2 child process. The Edge browser itself runs
/// as `msedge.exe`, so it never matches.
const WEBVIEW2_EXE: &str = "msedgewebview2.exe";

/// How long the event loop may stay unresponsive before it is logged.
const HANG_PROBE: Duration = Duration::from_secs(10);
const HANG_STRIKES: u32 = 3;

static HIDDEN: AtomicBool = AtomicBool::new(false);
static EVENT_LOOP_TICKS: AtomicU64 = AtomicU64::new(0);

fn log(app: &tauri::AppHandle, line: &str) {
    crate::logging::append(app, "lifecycle.log", line);
}

/// Called on every path that hides the main window (close-to-tray,
/// `--autostart` launch).
pub(crate) fn note_hidden(app: &tauri::AppHandle) {
    HIDDEN.store(true, Ordering::SeqCst);
    log(app, "hidden");
}

/// Called on every path that shows the main window.
pub(crate) fn note_shown(app: &tauri::AppHandle) {
    HIDDEN.store(false, Ordering::SeqCst);
    log(app, "shown");
}

/// Bring the main window back without ever blocking the event-loop thread.
///
/// `ShowWindow` and `SetForegroundWindow` send synchronous messages to
/// explorer.exe. Running them here means a wedged shell can stall this worker
/// instead of the whole app, and the tray keeps responding.
pub(crate) fn restore_window(app: &tauri::AppHandle, window: tauri::WebviewWindow) {
    let app = app.clone();
    std::thread::spawn(move || {
        let started = Instant::now();
        #[cfg(windows)]
        let restored = win::restore(&window);
        #[cfg(not(windows))]
        let restored = false;

        // Tauri's own calls queue onto the event loop, so they cannot block
        // this worker. When the raw restore already made the window visible
        // they are OS-level no-ops that only resync tao's bookkeeping.
        let _ = window.show();
        let _ = window.unminimize();
        if !restored {
            let _ = window.set_focus();
        }

        let elapsed = started.elapsed();
        if elapsed >= Duration::from_secs(2) {
            log(&app, &format!("restore slow: {} ms", elapsed.as_millis()));
        }
    });
}

pub(crate) fn is_webview2_process(exe_file_name: &str) -> bool {
    exe_file_name.eq_ignore_ascii_case(WEBVIEW2_EXE)
}

/// A WebView2 process is garbage when its parent is gone: the runtime is
/// supposed to exit with its host, so a survivor is stuck. Suspended survivors
/// are exactly what a killed app leaves behind, and they then fight the next
/// launch over the same `EBWebView` profile.
fn should_reap(exe: &str, pid: u32, ppid: u32, me: u32, live: &HashSet<u32>) -> bool {
    pid != me && is_webview2_process(exe) && !live.contains(&ppid)
}

/// Terminate WebView2 processes left behind by a previous run. Called once at
/// startup, before this process builds its own webview.
pub(crate) fn reap_orphans(app: &tauri::AppHandle) {
    #[cfg(windows)]
    {
        let mut reaped = 0usize;
        // Killing a browser process orphans its own children, so repeat until
        // a pass finds nothing new. Bounded so a pathological tree cannot spin.
        for _ in 0..4 {
            let procs = win::snapshot();
            let live: HashSet<u32> = procs.iter().map(|p| p.pid).collect();
            let me = win::current_pid();
            let mut killed = 0usize;
            for proc_entry in &procs {
                if !should_reap(&proc_entry.exe, proc_entry.pid, proc_entry.ppid, me, &live) {
                    continue;
                }
                if win::terminate(proc_entry.pid) {
                    killed += 1;
                }
            }
            reaped += killed;
            if killed == 0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(150));
        }
        if reaped > 0 {
            log(
                app,
                &format!("reaped {reaped} orphaned WebView2 process(es)"),
            );
        }
    }
    #[cfg(not(windows))]
    let _ = app;
}

/// Log when the event-loop thread stops pumping messages. A blocked UI thread
/// cannot be recovered in-process, so the value here is the evidence: the
/// timestamp plus the hidden/shown history tells the next investigation what
/// wedged it.
pub(crate) fn start_watchdog(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        let mut strikes = 0u32;
        loop {
            std::thread::sleep(HANG_PROBE);
            let before = EVENT_LOOP_TICKS.load(Ordering::SeqCst);
            if app
                .run_on_main_thread(|| {
                    EVENT_LOOP_TICKS.fetch_add(1, Ordering::SeqCst);
                })
                .is_err()
            {
                return;
            }
            std::thread::sleep(Duration::from_secs(2));
            if EVENT_LOOP_TICKS.load(Ordering::SeqCst) == before {
                strikes += 1;
                if strikes == HANG_STRIKES {
                    log(
                        &app,
                        &format!(
                            "event loop unresponsive for ~{}s (hidden={})",
                            HANG_PROBE.as_secs() * u64::from(HANG_STRIKES),
                            HIDDEN.load(Ordering::SeqCst)
                        ),
                    );
                }
            } else {
                if strikes >= HANG_STRIKES {
                    log(&app, "event loop recovered");
                }
                strikes = 0;
            }
        }
    });
}

#[cfg(windows)]
mod win {
    use std::ffi::c_void;

    type Dword = u32;
    type Bool = i32;
    type Handle = *mut c_void;

    const SNAP_PROCESS: Dword = 0x0000_0002;
    const PROCESS_TERMINATE: Dword = 0x0001;
    const INVALID_HANDLE: Handle = -1isize as Handle;
    const SW_SHOW: i32 = 5;
    const SW_RESTORE: i32 = 9;

    // Field order mirrors winapi's PROCESSENTRY32W; repr(C) pads identically,
    // and dw_size is set from our own size_of.
    #[allow(dead_code)]
    #[repr(C)]
    struct ProcessEntry {
        dw_size: Dword,
        cnt_usage: Dword,
        pid: Dword,
        heap_id: usize,
        module_id: Dword,
        threads: Dword,
        ppid: Dword,
        pri_base: i32,
        flags: Dword,
        exe: [u16; 260],
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn CreateToolhelp32Snapshot(flags: Dword, pid: Dword) -> Handle;
        fn Process32FirstW(snap: Handle, entry: *mut ProcessEntry) -> Bool;
        fn Process32NextW(snap: Handle, entry: *mut ProcessEntry) -> Bool;
        fn CloseHandle(obj: Handle) -> Bool;
        fn OpenProcess(access: Dword, inherit: Bool, pid: Dword) -> Handle;
        fn TerminateProcess(handle: Handle, code: u32) -> Bool;
        fn GetCurrentProcessId() -> Dword;
    }

    #[link(name = "user32")]
    extern "system" {
        fn ShowWindow(hwnd: Handle, cmd: i32) -> Bool;
        fn BringWindowToTop(hwnd: Handle) -> Bool;
        fn SetForegroundWindow(hwnd: Handle) -> Bool;
        fn IsWindowVisible(hwnd: Handle) -> Bool;
    }

    pub struct Proc {
        pub pid: u32,
        pub ppid: u32,
        pub exe: String,
    }

    pub fn current_pid() -> u32 {
        unsafe { GetCurrentProcessId() }
    }

    pub fn snapshot() -> Vec<Proc> {
        let mut out = Vec::new();
        unsafe {
            let snap = CreateToolhelp32Snapshot(SNAP_PROCESS, 0);
            if snap.is_null() || snap == INVALID_HANDLE {
                return out;
            }
            let mut entry: ProcessEntry = std::mem::zeroed();
            entry.dw_size = std::mem::size_of::<ProcessEntry>() as Dword;
            if Process32FirstW(snap, &mut entry) == 0 {
                CloseHandle(snap);
                return out;
            }
            loop {
                let len = entry
                    .exe
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(entry.exe.len());
                out.push(Proc {
                    pid: entry.pid,
                    ppid: entry.ppid,
                    exe: String::from_utf16_lossy(&entry.exe[..len]),
                });
                if Process32NextW(snap, &mut entry) == 0 {
                    break;
                }
            }
            CloseHandle(snap);
        }
        out
    }

    pub fn terminate(pid: u32) -> bool {
        unsafe {
            let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
            if handle.is_null() || handle == INVALID_HANDLE {
                return false;
            }
            let ok = TerminateProcess(handle, 1) != 0;
            CloseHandle(handle);
            ok
        }
    }

    /// Show, restore and focus the window from this thread. Returns whether
    /// the window ended up visible.
    pub fn restore(window: &tauri::WebviewWindow) -> bool {
        let Ok(hwnd) = window.hwnd() else {
            return false;
        };
        let hwnd = hwnd.0 as Handle;
        unsafe {
            ShowWindow(hwnd, SW_SHOW);
            ShowWindow(hwnd, SW_RESTORE);
            BringWindowToTop(hwnd);
            SetForegroundWindow(hwnd);
            IsWindowVisible(hwnd) != 0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn live(pids: &[u32]) -> HashSet<u32> {
        pids.iter().copied().collect()
    }

    #[test]
    fn webview2_name_matches_case_insensitively() {
        assert!(is_webview2_process("msedgewebview2.exe"));
        assert!(is_webview2_process("MSEdgeWebView2.EXE"));
        assert!(!is_webview2_process("msedge.exe"));
        assert!(!is_webview2_process("chatgpt-desktop-app.exe"));
        assert!(!is_webview2_process(""));
    }

    #[test]
    fn orphaned_webview2_is_reaped() {
        // Parent 900 is gone, so this WebView2 process is a leftover.
        assert!(should_reap(
            "msedgewebview2.exe",
            42,
            900,
            1,
            &live(&[1, 42])
        ));
    }

    #[test]
    fn webview2_with_live_parent_is_spared() {
        assert!(!should_reap(
            "msedgewebview2.exe",
            42,
            7,
            1,
            &live(&[1, 7, 42])
        ));
    }

    #[test]
    fn self_and_foreign_processes_are_spared() {
        // Our own pid is never a target, even with a dead parent.
        assert!(!should_reap("msedgewebview2.exe", 1, 900, 1, &live(&[1])));
        // Another app's WebView2 with a live parent stays untouched.
        assert!(!should_reap("msedgewebview2.exe", 42, 7, 1, &live(&[1, 7])));
        // Non-WebView2 images are never touched.
        assert!(!should_reap("explorer.exe", 42, 900, 1, &live(&[1, 42])));
    }
}
