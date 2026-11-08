//! Tsugiki's core: everything the app knows and decides, with no UI (ADR 0007).

#![forbid(unsafe_code)]

pub mod activity;
pub mod build;
pub mod conventions;
pub mod diff;
pub mod error;
pub mod filters;
pub mod folders;
pub mod github;
pub mod highlight;
pub mod latest;
pub mod launch;
pub mod links;
pub mod logfile;
pub mod measurement;
pub mod mirror;
pub mod misses;
pub mod repository;
pub mod session;
pub mod settings;
pub mod status;
pub mod store;
pub mod sync;
pub mod token;
pub mod web;
pub mod workbench;

use serde::Serialize;
use specta::Type;

/// What Tsugiki says about itself: the first value that travels from the core to the window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct About {
    pub name: String,
    pub version: String,
}

pub fn about() -> About {
    About {
        name: "Tsugiki".to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
    }
}

/// What the core's tests share.
#[cfg(test)]
pub(crate) mod testing {
    use std::path::{Path, PathBuf};

    /// A path for this test run under the repo's ignored `.temp` folder. `.temp` exists by the time this
    /// returns; the path itself doesn't. A test that panics leaves its folder or file behind, and Windows
    /// reuses process IDs, so a later run can get the same path: this removes what it finds there.
    pub(crate) fn temp_path(name: &str) -> PathBuf {
        let temp = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.temp");
        std::fs::create_dir_all(&temp).unwrap();
        let path = temp.join(format!("{name}-{}", std::process::id()));
        match std::fs::symlink_metadata(&path) {
            Ok(found) if found.is_dir() => std::fs::remove_dir_all(&path).unwrap(),
            Ok(_) => std::fs::remove_file(&path).unwrap(),
            Err(error) => assert_eq!(error.kind(), std::io::ErrorKind::NotFound, "{error}"),
        }
        path
    }
}

#[cfg(test)]
mod tests {
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
        std::fs::write(leftover.join("below/file"), "what a test that panicked left").unwrap();
        assert!(!testing::temp_path("leftover-folder").exists());
    }

    #[test]
    fn a_backtrace_keeps_the_file_and_line() {
        // `[profile.dev]` keeps only line tables, and this is what they're for. Only this test's own frame
        // counts, because libtest's frames have a `lib.rs` of their own, and they keep their lines.
        let (backtrace, line) = (std::backtrace::Backtrace::force_capture(), line!());
        let backtrace = backtrace.to_string();
        let frame = backtrace
            .split("a_backtrace_keeps_the_file_and_line")
            .nth(1)
            .unwrap();
        assert!(frame.contains(&format!("lib.rs:{line}")), "{backtrace}");
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
}
