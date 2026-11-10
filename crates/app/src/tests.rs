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

pub(crate) fn temp_folders(name: &str) -> (Folders, std::path::PathBuf) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../.temp/app-{name}"));
    drop(std::fs::remove_dir_all(&root));
    std::fs::create_dir_all(&root).unwrap();
    let folders = Folders {
        data: root.clone(),
        settings: root.clone(),
        logs: root.clone(),
    };
    (folders, root)
}

pub(crate) fn cpython() -> Home {
    Home::Repository {
        repository: RepositorySettings {
            name: RepositoryName::parse("python/cpython").unwrap(),
            conventions: Conventions::Cpython,
            filters: Filter::starters(RepositoryAccess::Other),
        },
    }
}

#[test]
fn about_answers_with_what_the_core_says() {
    assert_eq!(about(), tsugiki_core::about(&version()));
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
    let (folders, root) = temp_folders("problems");
    std::fs::write(
        folders.settings_file(),
        "[status_bar
",
    )
    .unwrap();
    let app = mock_app();
    assert!(app.manage(SettingsOwner::start(Ok(SettingsFiles::load(folders)))));
    std::fs::remove_dir_all(root).unwrap();

    assert_eq!(settings(app.state()).problems.len(), 1);
}

pub(crate) fn no_folders() -> Error {
    Error::new(ErrorKind::Internal, "no folders in this test")
}
