//! Freeze the WebView2 child-process tree while the main window sits hidden
//! in the tray, and thaw it on restore. Zero new dependencies: raw
//! kernel32/ntdll FFI. No-ops on non-Windows builds.
//!
//! Why freeze the whole tree instead of only renderers: a live browser
//! process next to a frozen renderer trips Chromium's hang monitor and
//! kills the page. With everything frozen, nobody watches and resume is
//! consistent. CPU drops to ~zero while RAM stays resident; session,
//! navigation and in-page state survive, unlike destroy/recreate.
//!
//! The grace timeout keeps brief tray dips instant. Completion detection
//! freezes with the page and re-scans on `visibilitychange` after resume;
//! a download in flight pauses and continues on thaw. The quit path thaws
//! first so no frozen orphans are left behind.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

/// Grace period after hiding before the tree is frozen. Brief tray dips
/// (< 2 min) keep instant resume; real idle gets ~zero renderer CPU.
pub(crate) const SUSPEND_AFTER: Duration = Duration::from_secs(120);

/// Image name of every WebView2 child process (never the Edge browser,
/// which runs as `msedge.exe`).
const WEBVIEW2_EXE: &str = "msedgewebview2.exe";

static EPOCH: AtomicU64 = AtomicU64::new(0);
static HIDDEN: AtomicBool = AtomicBool::new(false);
static FROZEN: Mutex<Vec<u32>> = Mutex::new(Vec::new());

/// Called on every path that hides the main window (close-to-tray,
/// `--autostart` launch). Starts the grace timer; a later `note_shown`
/// cancels it via the epoch counter.
pub(crate) fn note_hidden() {
    HIDDEN.store(true, Ordering::SeqCst);
    let epoch = EPOCH.fetch_add(1, Ordering::SeqCst) + 1;
    std::thread::spawn(move || {
        std::thread::sleep(SUSPEND_AFTER);
        if EPOCH.load(Ordering::SeqCst) != epoch || !HIDDEN.load(Ordering::SeqCst) {
            return;
        }
        freeze_tree();
    });
}

/// Called on every path that shows the main window, and on the quit path.
/// Cancels a pending freeze first, then thaws anything already frozen.
pub(crate) fn note_shown() {
    EPOCH.fetch_add(1, Ordering::SeqCst);
    HIDDEN.store(false, Ordering::SeqCst);
    thaw_recorded();
}

fn freeze_tree() {
    let pids = webview2_tree_pids();
    if pids.is_empty() {
        return;
    }
    set_suspended(&pids, true);
    if let Ok(mut frozen) = FROZEN.lock() {
        *frozen = pids;
    }
}

fn thaw_recorded() {
    let pids = FROZEN
        .lock()
        .map(|mut frozen| std::mem::take(&mut *frozen))
        .unwrap_or_default();
    if !pids.is_empty() {
        set_suspended(&pids, false);
    }
}

pub(crate) fn is_webview2_process(exe_file_name: &str) -> bool {
    exe_file_name.eq_ignore_ascii_case(WEBVIEW2_EXE)
}

/// True when `pid` (whose own parent is `ppid`) descends from `root`
/// through the `child -> parent` map. Pure logic, unit-tested.
fn descends_from(parents: &HashMap<u32, u32>, pid: u32, ppid: u32, root: u32) -> bool {
    if pid == root {
        return true;
    }
    let mut current = ppid;
    for _ in 0..64 {
        if current == root {
            return true;
        }
        match parents.get(&current) {
            Some(&next) if next != current => current = next,
            _ => return false,
        }
    }
    false
}

#[cfg(windows)]
fn webview2_tree_pids() -> Vec<u32> {
    let procs = win::snapshot();
    let parents: HashMap<u32, u32> = procs.iter().map(|p| (p.pid, p.ppid)).collect();
    let root = win::current_pid();
    procs
        .iter()
        .filter(|p| is_webview2_process(&p.exe) && descends_from(&parents, p.pid, p.ppid, root))
        .map(|p| p.pid)
        .collect()
}

#[cfg(not(windows))]
fn webview2_tree_pids() -> Vec<u32> {
    Vec::new()
}

#[cfg(windows)]
fn set_suspended(pids: &[u32], suspend: bool) {
    win::set_suspended(pids, suspend);
}

#[cfg(not(windows))]
fn set_suspended(_pids: &[u32], _suspend: bool) {}

#[cfg(windows)]
mod win {
    use std::ffi::c_void;

    type Dword = u32;
    type Bool = i32;
    type Handle = *mut c_void;

    const SNAP_PROCESS: Dword = 0x0000_0002;
    const PROCESS_SUSPEND_RESUME: Dword = 0x0800;
    const INVALID_HANDLE: Handle = -1isize as Handle;

    // Field order mirrors winapi's PROCESSENTRY32W; repr(C)ABI pads
    // identically, and dw_size is set from our own size_of.
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
        fn GetCurrentProcessId() -> Dword;
    }

    #[link(name = "ntdll")]
    extern "system" {
        fn NtSuspendProcess(handle: Handle) -> i32;
        fn NtResumeProcess(handle: Handle) -> i32;
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

    pub fn set_suspended(pids: &[u32], suspend: bool) {
        for &pid in pids {
            unsafe {
                let handle = OpenProcess(PROCESS_SUSPEND_RESUME, 0, pid);
                if handle.is_null() || handle == INVALID_HANDLE {
                    continue;
                }
                // PID-reuse race is ~ms wide and the targets are
                // long-lived WebView2 processes; a stray suspend is
                // contained to one process and undone by the thaw.
                if suspend {
                    NtSuspendProcess(handle);
                } else {
                    NtResumeProcess(handle);
                }
                CloseHandle(handle);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parents(pairs: &[(u32, u32)]) -> HashMap<u32, u32> {
        pairs.iter().copied().collect()
    }

    #[test]
    fn direct_and_nested_children_descend() {
        let map = parents(&[(10, 1), (11, 10), (1, 0)]);
        assert!(descends_from(&map, 10, 1, 1));
        assert!(descends_from(&map, 11, 10, 1));
    }

    #[test]
    fn unrelated_and_orphaned_pids_rejected() {
        let map = parents(&[(10, 1), (20, 7), (1, 0)]);
        assert!(!descends_from(&map, 20, 7, 1));
        assert!(!descends_from(&map, 99, 98, 1));
    }

    #[test]
    fn parent_cycles_terminate() {
        let map = parents(&[(10, 10)]);
        assert!(!descends_from(&map, 10, 10, 1));
    }

    #[test]
    fn webview2_name_matches_case_insensitively() {
        assert!(is_webview2_process("msedgewebview2.exe"));
        assert!(is_webview2_process("MSEdgeWebView2.EXE"));
        assert!(!is_webview2_process("msedge.exe"));
        assert!(!is_webview2_process(""));
    }

    #[test]
    fn grace_timeout_is_minutes_scale() {
        assert_eq!(SUSPEND_AFTER, Duration::from_secs(120));
    }
}
