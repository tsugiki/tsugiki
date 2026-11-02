#![expect(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a build script reports a failure by panicking, which stops the build"
)]

fn main() {
    let mut attributes = tauri_build::Attributes::new();
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        // Tauri embeds its Windows manifest only in the app's binary, and test binaries then fail to start
        // (STATUS_ENTRYPOINT_NOT_FOUND, no Common Controls v6). Embed the manifest through MSVC's linker
        // for every binary instead, as Tauri's testing guide shows.
        attributes = attributes
            .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest());
        let manifest = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap())
            .join("windows-app-manifest.xml");
        println!("cargo:rerun-if-changed={}", manifest.display());
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
        println!("cargo:rustc-link-arg=/WX");
    }
    tauri_build::try_build(attributes).expect("tauri-build");
}
