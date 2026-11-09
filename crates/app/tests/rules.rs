//! Rules about the rules: each one says why it is here, so a reader can judge a violation or an exception
//! without guessing. Also the one layering rule Clippy can't see, since it lives in a manifest (ADR 0007),
//! and the rule that keeps each test module in a file of its own.

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

/// The 1-based lines of `source` that open an inline test module: a `mod name {` after a `#[cfg(...)]` that
/// needs `test`, past any other attributes and line comments between the two.
fn inline_test_modules(source: &str) -> Vec<usize> {
    let mut found = Vec::new();
    for (start, _) in source.match_indices("#[cfg(") {
        let (condition, mut rest) = attribute(source.split_at(start).1);
        if !needs_test(condition) {
            continue;
        }
        loop {
            rest = rest.trim_start();
            if rest.starts_with("//") {
                rest = rest.split_once('\n').map_or("", |(_, after)| after);
            } else if rest.starts_with("#[") {
                rest = attribute(rest).1;
            } else {
                break;
            }
        }
        if opens_inline_module(rest) {
            let at = source.len() - rest.len();
            found.push(source.split_at(at).0.matches('\n').count() + 1);
        }
    }
    found
}

/// The attribute that `text` starts with, and the text after it. A `]` inside a string doesn't close it.
fn attribute(text: &str) -> (&str, &str) {
    let (mut depth, mut in_string, mut escaped) = (0, false, false);
    for (index, c) in text.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return text.split_at(index + 1);
                }
            }
            _ => {}
        }
    }
    (text, "")
}

/// Whether a `#[cfg(...)]` keeps its item to test builds. One under `not(...)` keeps it out of them instead.
fn needs_test(condition: &str) -> bool {
    let mut words = condition.split(|c: char| !(c.is_alphanumeric() || c == '_'));
    words.any(|word| word == "test") && !condition.contains("not(")
}

/// Whether `item` opens a module whose body follows in braces, after any `pub` or `pub(...)`.
fn opens_inline_module(item: &str) -> bool {
    let item = match item.strip_prefix("pub") {
        Some(after) if after.starts_with('(') => after.split_once(')').map_or("", |(_, rest)| rest),
        Some(after) => after,
        None => item,
    };
    let Some(after) = item.trim_start().strip_prefix("mod ") else {
        return false;
    };
    let name = after.trim_start();
    let body = name.trim_start_matches(|c: char| c.is_alphanumeric() || c == '_');
    body.trim_start().starts_with('{')
}

/// The `.rs` files under `dir`, at any depth.
#[expect(
    clippy::expect_used,
    reason = "a test fails by panicking, and this helper sits outside any #[test] function"
)]
fn rust_sources(dir: &Path) -> Vec<PathBuf> {
    let mut sources = Vec::new();
    for entry in fs::read_dir(dir).expect("list a source folder") {
        let path = entry.expect("read a source folder entry").path();
        if path.is_dir() {
            sources.extend(rust_sources(&path));
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            sources.push(path);
        }
    }
    sources
}

#[test]
fn an_inline_test_module_is_found() {
    let source = "#[cfg(test)]\nmod tests;\n\
                  #[cfg(test)]\nmod tests {\n}\n\
                  #[cfg(test)]\x20pub(crate) mod testing { use super::*; }\n\
                  #[cfg(all(test, windows))]\n\
                  /// What the Windows tests share.\n\
                  // A comment.\n\
                  #[expect(\n    dead_code,\n    reason = \"a \\\"quoted\\\" ] word\"\n)]\n\
                  pub mod windows { // Why.\n}\n\
                  #[cfg(not(test))]\nmod production {\n}\n\
                  #[cfg(test)]\nfn helper() {}\n\
                  #[cfg(test)]\n// The end.\n\
                  #[cfg(test";
    assert_eq!(inline_test_modules(source), [4, 6, 14]);
}

#[test]
fn each_test_module_lives_in_a_file_of_its_own() {
    let crates = fs::read_dir(repo().join("crates")).unwrap();
    let crates = crates
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_dir());
    let sources: Vec<_> = crates.flat_map(|path| rust_sources(&path)).collect();
    assert!(
        sources.len() > 100,
        "the scan found {} files",
        sources.len()
    );
    let inline: Vec<String> = sources
        .into_iter()
        .flat_map(|path| {
            let lines = inline_test_modules(&fs::read_to_string(&path).unwrap());
            lines
                .into_iter()
                .map(move |line| format!("{}:{line}", path.display()))
        })
        .collect();
    assert!(
        inline.is_empty(),
        "move each of these into a file, so a reviewer reads code and tests apart: {inline:#?}"
    );
}
