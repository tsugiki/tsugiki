use tsugiki_test_git::{Generated, GeneratedFile, check_generated};

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
fn a_test_path_goes_when_the_test_is_done_with_it() {
    let folder = testing::temp_path("done-folder");
    std::fs::create_dir_all(folder.join("below")).unwrap();
    let file = testing::temp_path("done-file");
    std::fs::write(&file, "what the test wrote").unwrap();
    let paths = [folder.to_path_buf(), file.to_path_buf()];

    drop((folder, file));

    assert!(paths.iter().all(|path| !path.exists()), "{paths:?}");
}

#[test]
fn a_test_that_fails_leaves_its_path_for_reading() {
    let mut left = None;
    let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let folder = testing::temp_path("failed");
        std::fs::create_dir_all(folder.join("below")).unwrap();
        left = Some(folder.to_path_buf());
        panic!("the test failed");
    }));

    assert!(failed.is_err());
    let left = left.unwrap();
    assert!(left.join("below").exists(), "{left:?}");
    drop(testing::temp_path("failed"));
}

#[test]
fn a_generated_file_updated_says_what_generated_it_whatever_its_line_ends() {
    let path = testing::temp_path("generated");
    let file = generated(&path);

    check_generated(&file, "fresh\n", Generated::Update);
    let updated = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, "fresh\r\n").unwrap();
    check_generated(&file, "fresh\n", Generated::Compare);

    assert_eq!(updated, "fresh\n");
}

#[test]
fn a_generated_file_that_differs_says_how_to_bring_it_up_to_date() {
    let path = testing::temp_path("generated-stale");
    std::fs::write(&path, "stale\n").unwrap();

    // Caught, so the file goes when the test passes.
    let failed = std::panic::catch_unwind(|| {
        check_generated(&generated(&path), "fresh\n", Generated::Compare);
    });

    let message = *failed.unwrap_err().downcast::<String>().unwrap();
    let says = "is out of date: run `just generate` and commit the result";
    assert!(message.contains(says), "{message}");
}

#[test]
#[should_panic(expected = "is out of date (it can't be read: ")]
fn a_generated_file_that_cant_be_read_says_why() {
    let path = testing::temp_path("generated-missing");
    check_generated(&generated(&path), "fresh\n", Generated::Compare);
}

/// The generated file at `path`, which `just generate` would write.
fn generated(path: &std::path::Path) -> GeneratedFile<'_> {
    GeneratedFile {
        path: path.to_str().unwrap(),
        update_with: "TSUGIKI_UPDATE_NOTHING",
        fix: "just generate",
    }
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
fn about_names_the_app_its_version_and_its_build() {
    let version = version::Version::new(Some("3a6dc89f0e1d2c3b4a5968778695a4b3c2d1e0f9"));
    assert_eq!(
        about(&version, build::Build::Release),
        About {
            name: "Tsugiki".to_owned(),
            version: format!("{} (3a6dc89)", env!("CARGO_PKG_VERSION")),
            build: build::Build::Release,
            source_ref: format!("v{}", env!("CARGO_PKG_VERSION")),
        }
    );
}

#[test]
fn a_development_build_links_its_source_on_main() {
    let about = about(&version::Version::new(None), build::Build::Development);
    assert_eq!(about.source_ref, "main");
}

#[test]
fn the_window_reads_the_build_in_lower_case() {
    let json = |build| serde_json::to_value(build).unwrap();
    assert_eq!(json(build::Build::Release), "release");
    assert_eq!(json(build::Build::Development), "development");
}
