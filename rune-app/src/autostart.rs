//! Launch at system startup, through the per-user Run key.
//!
//! HKCU\Software\Microsoft\Windows\CurrentVersion\Run needs no elevation, so the
//! toggle works from the tray without a UAC prompt. The value starts the app
//! with `--autostart`, which opens it in the tray instead of on screen.
//!
//! The value is compared with the command this executable would write, so a
//! stale entry left by a moved or reinstalled app reads as disabled instead of
//! claiming a startup that no longer happens.

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

/// The Run value as stored, if there is one.
fn read_value() -> Option<String> {
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
    if status.is_err() || size == 0 {
        return None;
    }
    let mut buffer = vec![0u16; (size as usize + 1) / 2];
    let mut size = (buffer.len() * 2) as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            PCWSTR(value.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr() as *mut _),
            Some(&mut size),
        )
    };
    if status.is_err() {
        return None;
    }
    let len = buffer
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(buffer.len());
    Some(String::from_utf16_lossy(&buffer[..len]))
}

/// Whether the Run value is present *and* points at this executable.
pub fn is_enabled() -> bool {
    let (Some(stored), Some(expected)) = (read_value(), command()) else {
        return false;
    };
    stored.trim().eq_ignore_ascii_case(expected.trim())
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

#[cfg(test)]
mod tests {
    use super::command;

    #[test]
    fn the_command_quotes_the_path_and_asks_for_the_tray() {
        let command = command().expect("a running executable");
        assert!(command.starts_with('"'), "the path is quoted: {command}");
        assert!(
            command.ends_with("\" --autostart"),
            "and starts hidden: {command}"
        );
    }
}
