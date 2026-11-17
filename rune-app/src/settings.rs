//! Small persisted preferences, in the per-user registry.
//!
//! The only one today is the folder the last Save As dialog used, so the next
//! download starts where the last one went.

use std::path::{Path, PathBuf};

use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegGetValueW, RegSetKeyValueW,
};
use windows::core::PCWSTR;

const KEY: &str = r"Software\nwn900\ChatGPT";
const LAST_DOWNLOAD_DIR: &str = "LastDownloadDir";

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Read one string value, if it is there.
fn read(name: &str) -> Option<String> {
    let (key, value) = (wide(KEY), wide(name));
    let mut size = 0u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
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
            PCWSTR(key.as_ptr()),
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

/// Write one string value.
fn write(name: &str, text: &str) -> Result<(), String> {
    let (key, value) = (wide(KEY), wide(name));
    let data = wide(text);
    let status = unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            PCWSTR(value.as_ptr()),
            REG_SZ.0,
            Some(data.as_ptr() as *const _),
            (data.len() * 2) as u32,
        )
    };
    status.ok().map_err(|error| error.to_string())
}

/// The folder the last Save As dialog used, if it still exists.
pub fn last_download_dir() -> Option<PathBuf> {
    let path = PathBuf::from(read(LAST_DOWNLOAD_DIR)?);
    path.is_dir().then_some(path)
}

/// Remember the folder a file was saved into.
pub fn set_last_download_dir(path: &Path) -> Result<(), String> {
    write(LAST_DOWNLOAD_DIR, &path.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_folder_is_not_offered() {
        // Whatever the registry holds, a path that is not a directory is not
        // returned, so the dialog never opens somewhere that does not exist.
        let missing = PathBuf::from(r"Z:\definitely\not\here");
        assert!(!missing.is_dir());
        assert!(last_download_dir().is_none_or(|path| path.is_dir()));
    }
}
