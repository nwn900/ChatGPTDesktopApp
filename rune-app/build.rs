//! Embed the project's icon and version info in the executable.
//!
//! Without this the exe carries no icon resource, so Explorer, the Start Menu
//! shortcut and Add/Remove Programs all fall back to the generic Windows
//! application icon. Rune's runtime set_app_icon only affects the window and
//! taskbar, not the file.

fn main() {
    println!("cargo:rerun-if-changed=../src-tauri/icons/icon.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let icon = std::path::Path::new(&manifest).join("../src-tauri/icons/icon.ico");

    let mut resource = winresource::WindowsResource::new();
    resource.set_icon(icon.to_str().expect("icon path"));
    resource.set("ProductName", "ChatGPT");
    resource.set("FileDescription", "ChatGPT");
    resource.set("LegalCopyright", "ISC");
    resource
        .compile()
        .expect("embed the application icon and version info");
}
