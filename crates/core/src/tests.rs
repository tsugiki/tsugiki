use super::*;

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
    let (backtrace, line) = (std::backtrace::Backtrace::force_capture(), line!());
    let backtrace = backtrace.to_string();
    let frame = backtrace
        .split("a_backtrace_keeps_the_file_and_line")
        .nth(1)
        .unwrap();
    let place = frame.lines().nth(1).unwrap();
    assert!(place.ends_with(&format!("tests.rs:{line}")), "{backtrace}");
}

#[test]
fn about_names_the_app_and_its_version() {
    assert_eq!(
        about(),
        About {
            name: "Tsugiki".to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
        }
    );
}
