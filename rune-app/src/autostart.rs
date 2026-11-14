//! Launch at system startup, through the per-user Run key.
//!
//! HKCU\Software\Microsoft\Windows\CurrentVersion\Run needs no elevation, so the
//! toggle works from the tray without a UAC prompt. The value starts the app
//! with `--autostart`, which opens it in the tray instead of on screen.

use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
};
use windows::core::PCWSTR;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE: &str = "ChatGPT";

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// The command Windows runs at sign-in: this executable, started in the tray.
fn command() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    Some(format!("\"{}\" --autostart", exe.display()))
}

/// Whether the Run value is present.
pub fn is_enabled() -> bool {
    let (subkey, value) = (wide(RUN_KEY), wide(VALUE));
    let mut size = 0u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            PCWSTR(value.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut size),
        )
    };
    status.is_ok() && size > 0
}

/// Add or remove the Run value.
pub fn set_enabled(enabled: bool) -> Result<(), String> {
    let (subkey, value) = (wide(RUN_KEY), wide(VALUE));
    if enabled {
        let command = command().ok_or("cannot resolve the executable path")?;
        let data = wide(&command);
        let status = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                PCWSTR(subkey.as_ptr()),
                PCWSTR(value.as_ptr()),
                REG_SZ.0,
                Some(data.as_ptr() as *const _),
                (data.len() * 2) as u32,
            )
        };
        return status.ok().map_err(|error| error.to_string());
    }
    let status = unsafe {
        RegDeleteKeyValueW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            PCWSTR(value.as_ptr()),
        )
    };
    // Removing a value that is not there is not a failure.
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(());
    }
    status.ok().map_err(|error| error.to_string())
}
