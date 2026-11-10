#![expect(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a build script reports a failure by panicking, which stops the build"
)]

use std::path::Path;

fn main() {
    hand_over_the_commit();
    let mut attributes = tauri_build::Attributes::new();
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        // Tauri embeds its Windows manifest only in the app's binary, and test binaries then fail to start
        // (STATUS_ENTRYPOINT_NOT_FOUND, no Common Controls v6). Embed the manifest through MSVC's linker
        // for every binary instead, as Tauri's testing guide shows.
        attributes = attributes
            .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest());
        let manifest = Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap())
            .join("windows-app-manifest.xml");
        println!("cargo:rerun-if-changed={}", manifest.display());
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
        println!("cargo:rustc-link-arg=/WX");
    }
    tauri_build::try_build(attributes).expect("tauri-build");
}

/// Hands the app `HEAD`'s SHA as `TSUGIKI_COMMIT`, so its version names the commit it was built from (#238).
/// A build with no git, or outside a checkout, hands over nothing and shows the version alone.
///
/// The script runs again when `HEAD` moves: a checkout rewrites `HEAD`, and a commit rewrites its branch's
/// file under `refs/heads`, or adds one there if `git pack-refs` had moved the branch into `packed-refs`.
/// Cargo then rebuilds the app alone, as the core never sees the SHA.
fn hand_over_the_commit() {
    let Some(answer) = git(&[
        "rev-parse",
        "HEAD",
        "--git-path",
        "HEAD",
        "--git-path",
        "refs/heads",
        "--git-path",
        "packed-refs",
    ]) else {
        return;
    };
    let mut lines = answer.lines();
    let Some(sha) = lines.next() else { return };
    println!("cargo:rustc-env=TSUGIKI_COMMIT={sha}");
    let here = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    // Cargo runs the script on every build while a path it watches is missing, and a repository may have
    // no `packed-refs`.
    for watched in lines.map(|path| Path::new(&here).join(path)) {
        if watched.exists() {
            println!("cargo:rerun-if-changed={}", watched.display());
        }
    }
}

/// What git printed, or nothing if git is missing or fails, as it does outside a checkout.
#[expect(
    clippy::disallowed_types,
    reason = "the build runs git on the build machine, not the app on the user's"
)]
fn git(arguments: &[&str]) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(arguments)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}
