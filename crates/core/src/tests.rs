use super::*;

/// `diff --exit-code` fails on files that differ, and says how on stdout, where git says some failures.
#[test]
#[should_panic(expected = "diff --git a/Cargo.toml b/src/lib.rs")]
fn a_test_git_that_fails_ends_the_test_with_what_it_printed() {
    let _ = tsugiki_test_git::git(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")),
        &[
            "diff",
            "--no-index",
            "--exit-code",
            "Cargo.toml",
            "src/lib.rs",
        ],
    );
}

#[test]
fn a_file_a_failed_run_left_at_a_test_path_is_gone() {
    let leftover = testing::temp_path("leftover-file");
    std::fs::write(&leftover, "what a test that panicked left").unwrap();
    assert!(!testing::temp_path("leftover-file").exists());
}

#[test]
fn a_folder_a_failed_run_left_at_a_test_path_is_gone() {
    let leftover = testing::temp_path("leftover-folder");
    std::fs::create_dir_all(leftover.join("below")).unwrap();
    std::fs::write(
        leftover.join("below/file"),
        "what a test that panicked left",
    )
    .unwrap();
    assert!(!testing::temp_path("leftover-folder").exists());
}

#[test]
fn a_backtrace_keeps_the_file_and_line() {
    // `[profile.dev]` keeps only line tables, and this is what they're for. Only this test's own frame
    // counts: the line after its name says where it stands, and libtest's frames keep their lines too.
    // macOS adds a column after the line, and Windows doesn't.
    let (backtrace, line) = (std::backtrace::Backtrace::force_capture(), line!());
    let backtrace = backtrace.to_string();
    let frame = backtrace
        .split("a_backtrace_keeps_the_file_and_line")
        .nth(1)
        .unwrap();
    let place = frame.lines().nth(1).unwrap();
    let at = place
        .split_once("tests.rs:")
        .and_then(|(_, at)| at.split(':').next());
    assert_eq!(at, Some(line.to_string().as_str()), "{backtrace}");
}

#[test]
fn about_names_the_app_and_its_version() {
    let commit = github::CommitSha::parse("3a6dc89f0e1d2c3b4a5968778695a4b3c2d1e0f9");
    assert_eq!(
        about(&version::Version::new(commit)),
        About {
            name: "Tsugiki".to_owned(),
            version: format!("{} (3a6dc89)", env!("CARGO_PKG_VERSION")),
        }
    );
}
