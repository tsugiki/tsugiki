use std::fs;

use super::*;
use crate::testing::temp_path;

#[test]
fn a_rust_analyzer_in_a_toolchain_runs_with_that_toolchain() {
    let root = temp_path("rust-analyzer-toolchain");
    fs::create_dir_all(root.join("lib").join("rustlib")).unwrap();
    fs::create_dir_all(root.join("bin")).unwrap();

    let setup = setup(&root.join("bin").join("rust-analyzer.exe")).unwrap();
    let _removed = fs::remove_dir_all(&root);

    let named = |name: &str| {
        (setup.env.iter())
            .find(|(key, _)| key == name)
            .map(|(_, value)| PathBuf::from(value))
    };
    let bin = root.join("bin");
    assert_eq!(named("RUSTUP_TOOLCHAIN"), Some(root.clone()));
    assert_eq!(named("CARGO"), Some(bin.join(format!("cargo{EXE_SUFFIX}"))));
    assert_eq!(named("RUSTC"), Some(bin.join(format!("rustc{EXE_SUFFIX}"))));
    assert_eq!(named("RUSTUP_AUTO_INSTALL"), Some(PathBuf::from("0")));
    assert_eq!(named("CARGO_NET_OFFLINE"), Some(PathBuf::from("true")));
    assert_eq!(named("RUSTC_WRAPPER"), Some(PathBuf::new()));
    let sysroot = root.display().to_string();
    assert_eq!(setup.options["cargo"]["sysroot"], sysroot);
    assert_eq!(setup.settings["rust-analyzer"], setup.options);
    assert_eq!(setup.language, Language::Rust);
}

/// A rustup proxy in `~/.cargo/bin`, or a rust-analyzer unpacked alone, would run the cargo and rustc that
/// the checkout's files pick.
#[test]
fn a_rust_analyzer_outside_a_toolchain_doesn_t_start() {
    let root = temp_path("rust-analyzer-alone");
    fs::create_dir_all(root.join("bin")).unwrap();
    let program = root.join("bin").join("rust-analyzer.exe");

    let refused = setup(&program).map(|_setup| ());
    let at_root = sysroot(Path::new("rust-analyzer.exe"));
    let _removed = fs::remove_dir_all(&root);

    let message = format!(
        "{} isn't inside a Rust toolchain. Pick the rust-analyzer that `rustup which rust-analyzer` names.",
        program.display()
    );
    assert_eq!(refused, Err(failed(&message)));
    assert_eq!(at_root, None);
}

#[test]
fn rust_analyzer_runs_no_build_script_proc_macro_or_check() {
    let config = config(Path::new("toolchain"));

    assert_eq!(config["cargo"]["buildScripts"]["enable"], false);
    assert_eq!(config["procMacro"]["enable"], false);
    assert_eq!(config["checkOnSave"], false);
}
