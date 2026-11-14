//! One running app per user session.
//!
//! A named mutex decides who is first; named events carry activation and quit
//! requests to the first instance. All of them live in the `Local\` namespace,
//! so they are per logon session and another user's session cannot reach them.

use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, WAIT_OBJECT_0,
};
use windows::Win32::System::Threading::{
    CreateEventW, CreateMutexW, INFINITE, SetEvent, WaitForSingleObject,
};
use windows::core::PCWSTR;

const MUTEX: &str = "Local\\ChatGPT.SingleInstance";
const ACTIVATE: &str = "Local\\ChatGPT.Activate";
const QUIT: &str = "Local\\ChatGPT.Quit";

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// The first instance's claim on the app. Dropping it lets the next launch in.
pub struct SingleInstance {
    mutex: HANDLE,
}

/// What [SingleInstance::acquire] found.
pub enum Acquired {
    /// This process is the app; keep the guard alive for as long as it runs.
    First(SingleInstance),
    /// Another process is already the app and has been asked to show itself.
    AlreadyRunning,
}

impl SingleInstance {
    pub fn acquire() -> Acquired {
        let name = wide(MUTEX);
        let Ok(mutex) = (unsafe { CreateMutexW(None, false, PCWSTR(name.as_ptr())) }) else {
            // Without a mutex the app still runs; it just cannot deduplicate.
            return Acquired::First(SingleInstance {
                mutex: HANDLE::default(),
            });
        };
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            unsafe {
                let _ = CloseHandle(mutex);
            }
            signal(ACTIVATE);
            return Acquired::AlreadyRunning;
        }
        Acquired::First(SingleInstance { mutex })
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        if !self.mutex.is_invalid() {
            unsafe {
                let _ = CloseHandle(self.mutex);
            }
        }
    }
}

/// Set a named event, creating it if the app is not listening yet.
fn signal(name: &str) {
    let name = wide(name);
    if let Ok(event) = unsafe { CreateEventW(None, false, false, PCWSTR(name.as_ptr())) } {
        unsafe {
            let _ = SetEvent(event);
            let _ = CloseHandle(event);
        }
    }
}

/// Ask the running instance to show itself.
pub fn signal_activation() {
    signal(ACTIVATE);
}

/// Ask the running instance to quit, for an installer that must replace it.
pub fn signal_quit() {
    signal(QUIT);
}

/// Run `on_signal` on a worker thread for every request on that event. The
/// thread waits on the event, so it works whether or not the window is drawing.
fn listen(name: &'static str, on_signal: impl Fn() + Send + 'static) {
    std::thread::spawn(move || {
        let wide_name = wide(name);
        let Ok(event) = (unsafe { CreateEventW(None, false, false, PCWSTR(wide_name.as_ptr())) })
        else {
            return;
        };
        loop {
            if unsafe { WaitForSingleObject(event, INFINITE) } != WAIT_OBJECT_0 {
                break;
            }
            on_signal();
        }
        unsafe {
            let _ = CloseHandle(event);
        }
    });
}

/// Show the window when another launch asks for it.
pub fn listen_activation(on_activate: impl Fn() + Send + 'static) {
    listen(ACTIVATE, on_activate);
}

/// Quit when another process asks for it.
pub fn listen_quit(on_quit: impl Fn() + Send + 'static) {
    listen(QUIT, on_quit);
}
