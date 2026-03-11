#[path = "src/icon_pixels.rs"]
mod icon_pixels;
fn main() {
    println!("cargo:rerun-if-changed=src/icon_pixels.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let path =
        std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("StartupManager.ico");
    std::fs::write(&path, icon_pixels::ico()).expect("Cannot write application icon");
    let version = std::env::var("CARGO_PKG_VERSION").unwrap();
    winresource::WindowsResource::new()
        .set_icon(path.to_str().unwrap())
        .set("ProductName", "StartupManager")
        .set("FileDescription", "StartupManager")
        .set("FileVersion", &version)
        .set("ProductVersion", &version)
        .set(
            "OriginalFilename",
            &format!("StartupManager-v{version}.exe"),
        )
        .compile()
        .expect("Cannot embed application icon and version resources");
}
