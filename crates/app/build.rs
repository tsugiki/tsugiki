#![expect(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a build script reports a failure by panicking, which stops the build"
)]

use std::path::{Path, PathBuf};

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
/// file, or adds one under `refs/heads` if `git pack-refs` had moved the branch into `packed-refs`. A
/// repository that keeps its refs in a `reftable` folder rewrites that instead. Cargo then rebuilds the app
/// alone, as the core never sees the SHA. It watches the folder of branches only while the branch has no
/// file, as each branch deleted or made there would rebuild the app and every test binary that links it.
fn hand_over_the_commit() {
    let Some(answer) = git(&["rev-parse", "HEAD", "--symbolic-full-name", "HEAD"]) else {
        return;
    };
    // A detached `HEAD` names no branch, and git prints `HEAD` as its name.
    let (Some(sha), Some(branch)) = (answer.lines().next(), answer.lines().nth(1)) else {
        return;
    };
    println!("cargo:rustc-env=TSUGIKI_COMMIT={sha}");
    let Some(paths) = git(&[
        "rev-parse",
        "--git-path",
        "HEAD",
        "--git-path",
        branch,
        "--git-path",
        "refs/heads",
        "--git-path",
        "packed-refs",
        "--git-path",
        "reftable",
    ]) else {
        return;
    };
    let here = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let paths: Vec<PathBuf> = paths
        .lines()
        .map(|path| Path::new(&here).join(path))
        .collect();
    let Ok([head, branch, branches, packed, reftable]) = <[PathBuf; 5]>::try_from(paths) else {
        return;
    };
    let branch = if branch.exists() { branch } else { branches };
    let mut watched = vec![head, branch, packed, reftable];
    // A detached `HEAD` is its own branch.
    watched.dedup();
    // Cargo runs the script on every build while a path it watches is missing, and a repository may have
    // no `packed-refs` or `reftable`.
    for path in watched.iter().filter(|path| path.exists()) {
        println!("cargo:rerun-if-changed={}", path.display());
    }
}

/// What git printed, or nothing if git is missing or fails, as it does outside a checkout. It leaves out
/// `GIT_DIR` and its kin, as `tsugiki_test_git` does: a hook or `git rebase -x` that runs the build may set
/// them, and git would then ask another repository than the one the app's tests ask.
#[expect(
    clippy::disallowed_types,
    reason = "the build runs git on the build machine, not the app on the user's"
)]
fn git(arguments: &[&str]) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(arguments)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}
