use tauri::Manager;

/// Append one line to a bounded diagnostic log in the app log directory. The
/// file is truncated once it reaches the cap, so a long-running install cannot
/// grow it without limit. Conversation text is never written here.
pub(crate) fn append(app: &tauri::AppHandle, file: &str, line: &str) {
    use std::io::Write;

    let Ok(dir) = app.path().app_log_dir() else {
        return;
    };
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(file);
    let append = std::fs::metadata(&path).map_or(true, |meta| meta.len() < 65_536);
    if let Ok(mut handle) = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .truncate(!append)
        .open(path)
    {
        let _ = writeln!(handle, "{line}");
    }
}
