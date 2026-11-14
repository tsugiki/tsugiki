use std::fs;

use super::*;
use crate::testing::temp_path;

/// A folder laid out as a toolchain, with `bin` and `lib/rustlib`, and a program in its `bin`.
fn toolchain(name: &str) -> (PathBuf, PathBuf) {
    let root = temp_path(name);
    fs::create_dir_all(root.join("lib").join("rustlib")).unwrap();
    fs::create_dir_all(root.join("bin")).unwrap();
    let program = root.join("bin").join("rust-analyzer.exe");
    (root, program)
}

#[test]
fn a_rust_analyzer_in_a_toolchain_runs_with_that_toolchain() {
    let (root, program) = toolchain("rust-analyzer-toolchain");

    let setup = setup(&program);
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
    assert_eq!(named("RUSTC_WRAPPER"), Some(PathBuf::new()));
    let sysroot = root.display().to_string();
    assert_eq!(setup.options["cargo"]["sysroot"], sysroot);
    assert_eq!(setup.settings["rust-analyzer"], setup.options);
    assert_eq!(setup.language, Language::Rust);
}

#[test]
fn a_rust_analyzer_outside_a_toolchain_loads_no_standard_library_and_still_installs_nothing() {
    let root = temp_path("rust-analyzer-alone");
    fs::create_dir_all(root.join("bin")).unwrap();

    let setup = setup(&root.join("bin").join("rust-analyzer.exe"));
    let at_root = sysroot(Path::new("rust-analyzer.exe"));
    let _removed = fs::remove_dir_all(&root);

    let names: Vec<_> = setup.env.iter().map(|(name, _)| name.clone()).collect();
    let expected = [
        "RUSTUP_AUTO_INSTALL",
        "CARGO_NET_OFFLINE",
        "RUSTC_WRAPPER",
        "RUSTC_WORKSPACE_WRAPPER",
    ];
    assert_eq!(names, expected.map(OsString::from));
    assert_eq!(setup.options["cargo"]["sysroot"], Value::Null);
    assert_eq!(at_root, None);
}

#[test]
fn rust_analyzer_runs_no_build_script_proc_macro_or_check() {
    let config = config(None);

    assert_eq!(config["cargo"]["buildScripts"]["enable"], false);
    assert_eq!(config["procMacro"]["enable"], false);
    assert_eq!(config["checkOnSave"], false);
}
