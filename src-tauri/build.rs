fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        let attributes = tauri_build::Attributes::new()
            .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest());
        tauri_build::try_build(attributes).expect("failed to run tauri-build");
        embed_windows_manifest();
    } else {
        tauri_build::build();
    }
}

fn embed_windows_manifest() {
    let manifest = std::path::PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("Cargo sets CARGO_MANIFEST_DIR"),
    )
    .join("windows-app.manifest");
    println!("cargo:rerun-if-changed={}", manifest.display());
    // Tauri APIs import Common Controls v6. The app manifest is normally
    // embedded by tauri-build, but Cargo's test harness does not inherit it.
    // Embed the same manifest via the linker for app and test executables.
    println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
}
