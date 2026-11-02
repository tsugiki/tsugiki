//! Rules about the rules: each one says why it is here, so a reader can judge a violation or an exception
//! without guessing. Also the one layering rule Clippy can't see, since it lives in a manifest (ADR 0007).

use std::fs;
use std::path::{Path, PathBuf};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[expect(
    clippy::expect_used,
    reason = "a test fails by panicking, and this helper sits outside any #[test] function"
)]
fn read(path: &str) -> String {
    fs::read_to_string(repo().join(path)).expect("read a file from the repo")
}

/// The parts of a Cargo manifest under `[workspace.lints]`, which other tables don't need reasons.
fn workspace_lints(manifest: &str) -> String {
    let mut in_lints = false;
    let lines = manifest.lines().filter(|line| {
        if line.starts_with('[') {
            in_lints = line.starts_with("[workspace.lints");
        }
        in_lints
    });
    lines.collect::<Vec<_>>().join("\n")
}

/// The `name = value` lines of a TOML text, indented or not. The items of an array don't start with a
/// name, so an array needs its reason on the line that opens it.
fn rules(toml: &str) -> Vec<&str> {
    let is_name = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.');
    let is_rule = |line: &&str| {
        line.split_once('=')
            .is_some_and(|(name, _)| !name.trim().is_empty() && name.trim().chars().all(is_name))
    };
    toml.lines().filter(is_rule).collect()
}

/// The lines in `rules` with no comment after the value.
fn unexplained<'a>(rules: &[&'a str]) -> Vec<&'a str> {
    let explained = |line: &&&str| {
        line.split_once(" # ")
            .is_some_and(|(_, why)| !why.trim().is_empty())
    };
    rules
        .iter()
        .filter(|line| !explained(line))
        .copied()
        .collect()
}

#[test]
fn a_rule_without_a_reason_is_found() {
    let toml = "# A heading.\n\
                [table]\n\
                explained = 1 # Why.\n\
                bare = 2\n\
                empty = 3 # \n\
                list = [ # Why.\n    { item = 4 },\n]\n\
                silent = [\n    { item = 5 },\n]\n\
                \x20   indented = 6\n";
    assert_eq!(
        unexplained(&rules(toml)),
        ["bare = 2", "empty = 3 # ", "silent = [", "    indented = 6"]
    );
}

#[test]
fn only_the_workspace_lints_need_reasons() {
    let manifest = "[workspace.dependencies]\nserde = \"1\"\n\
                    [workspace.lints.rust]\nfirst = \"deny\"\n\
                    [workspace.lints.clippy]\nsecond = \"deny\"\n\
                    [profile.release]\nlto = true\n";
    assert_eq!(
        rules(&workspace_lints(manifest)),
        ["first = \"deny\"", "second = \"deny\""]
    );
}

#[test]
fn every_lint_says_why_it_is_on() {
    let lints = workspace_lints(&read("Cargo.toml"));
    let lints = rules(&lints);
    assert!(lints.iter().any(|line| line.starts_with("unsafe_code ")));
    assert!(lints.iter().any(|line| line.starts_with("unwrap_used ")));
    assert_eq!(unexplained(&lints), [""; 0]);
}

#[test]
fn every_clippy_setting_says_why_it_is_set() {
    let settings = read("clippy.toml");
    let settings = rules(&settings);
    assert!(
        settings
            .iter()
            .any(|line| line.starts_with("max-fn-params-bools "))
    );
    assert!(
        settings
            .iter()
            .any(|line| line.starts_with("disallowed-types "))
    );
    assert_eq!(unexplained(&settings), [""; 0]);
}

#[test]
fn every_crate_follows_the_workspace_lints() {
    let crates: Vec<_> = fs::read_dir(repo().join("crates"))
        .unwrap()
        .map(|entry| entry.unwrap().path().join("Cargo.toml"))
        .collect();
    assert!(crates.len() >= 2, "the scan found {crates:?}");
    for manifest in crates {
        let text = fs::read_to_string(&manifest).unwrap();
        assert!(
            text.contains("[lints]\nworkspace = true"),
            "{}: without this, Clippy holds the crate to none of our rules",
            manifest.display()
        );
    }
}

#[test]
fn the_core_doesnt_depend_on_tauri() {
    let manifest = read("crates/core/Cargo.toml");
    assert!(
        !manifest.to_lowercase().contains("tauri"),
        "the core stays free of the desktop shell, so it can run without a window (ADR 0007)"
    );
}
