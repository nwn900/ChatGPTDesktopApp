fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&["notify_answer", "open_external"]),
    ))
    .expect("Tauri build configuration");
}
