use super::*;
use std::path::Path;
use std::time::Duration;
use tauri::Manager;
use tauri::test::mock_app;
use tsugiki_core::conventions::Conventions;
use tsugiki_core::error::ErrorKind;
use tsugiki_core::filters::Filter;
use tsugiki_core::folders::Folders;
use tsugiki_core::repository_settings::RepositorySettings;
use tsugiki_core::settings::SettingsFiles;
use tsugiki_test_git::TempPath;

/// Folders that are all one new folder under `.temp`, and that folder, which goes when the test passes.
pub(crate) fn temp_folders(name: &str) -> (Folders, TempPath) {
    let root = tsugiki_test_git::temp_path(&format!("app-{name}"));
    std::fs::create_dir_all(&root).unwrap();
    (tsugiki_core::testing::one_folder(&root), root)
}

pub(crate) fn cpython() -> Home {
    Home::Repository {
        repository: RepositorySettings {
            name: RepositoryName::parse("python/cpython").unwrap(),
            conventions: Conventions::Cpython,
            filters: Filter::starters(),
            read_only: false,
        },
        opened_at: None,
        others: Vec::new(),
    }
}

#[test]
fn about_answers_with_what_the_core_says() {
    assert_eq!(about(), tsugiki_core::about(&version(), Build::CURRENT));
}

#[test]
fn person_query_spells_the_search_a_clicked_name_runs() {
    assert_eq!(
        person_query("some-author").unwrap(),
        "is:open author:some-author"
    );
}

#[test]
fn person_query_refuses_a_name_that_names_no_one_person() {
    assert_eq!(person_query("  ").unwrap_err().kind, ErrorKind::Internal);
}

#[test]
fn a_release_refuses_to_open_devtools() {
    let app = mock_app();
    let window = tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
        .build()
        .unwrap();
    let refusal = open_devtools_in(Build::Release, &window).unwrap_err();
    assert_eq!(refusal.kind, ErrorKind::Internal);
    assert!(refusal.message.contains("DevTools"), "{}", refusal.message);
    open_devtools_in(Build::Development, &window).unwrap();
}

/// `build.rs` ran in this checkout, so the version names the commit it's on, whichever that is (#238).
#[test]
fn the_version_names_the_commit_checked_out() {
    let head = tsugiki_test_git::git(
        Path::new(env!("CARGO_MANIFEST_DIR")),
        &["rev-parse", "HEAD"],
    );
    let short = head.get(..7).unwrap();
    assert_eq!(
        version().to_string(),
        format!("{} ({short})", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn time_running_counts_from_the_start() {
    let app = mock_app();
    assert!(app.manage(Started::now()));
    std::thread::sleep(Duration::from_millis(20));
    let first = time_running(app.state());
    let second = time_running(app.state());
    assert!(first >= 20.0, "{first}");
    assert!(second >= first, "{first} then {second}");
}

#[test]
fn a_measurement_answers_with_the_line_it_wrote() {
    let measurement: Measurement = serde_json::from_str(
            r#"{"seconds": 2, "frames": null, "startup_ms": null, "keys": null, "view_queries": null,
                "refreshes": null, "issues": null, "long_frames": [{"at_s": 1, "ms": 30, "cause": null}]}"#,
        )
        .unwrap();
    let line = record_measurement(measurement.clone());
    assert_eq!(line, measurement.summary(&version()));
}

#[test]
fn a_miss_answers_with_the_line_it_wrote() {
    let miss: Miss = serde_json::from_str(r#"{"kind": "view_query", "ms": 73}"#).unwrap();
    assert_eq!(record_miss(miss.clone()), miss.line());
}

#[test]
fn settings_says_what_was_wrong_with_the_files() {
    let (folders, _root) = temp_folders("problems");
    std::fs::write(
        folders.settings_file(),
        "[status_bar
",
    )
    .unwrap();
    let app = mock_app();
    assert!(app.manage(SettingsOwner::start(Ok(SettingsFiles::load(folders)))));

    assert_eq!(settings(app.state()).problems.len(), 1);
}

pub(crate) fn no_folders() -> Error {
    Error::new(ErrorKind::Internal, "no folders in this test")
}
