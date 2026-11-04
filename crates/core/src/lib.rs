//! Tsugiki's core: everything the app knows and decides, with no UI (ADR 0007).

#![forbid(unsafe_code)]

pub mod build;
pub mod error;
pub mod folders;
pub mod github;
pub mod latest;
pub mod launch;
pub mod logfile;
pub mod repository;
pub mod session;
pub mod settings;
pub mod status;
pub mod store;
pub mod token;

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
    /// returns; the path itself doesn't.
    pub(crate) fn temp_path(name: &str) -> PathBuf {
        let temp = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.temp");
        std::fs::create_dir_all(&temp).unwrap();
        temp.join(format!("{name}-{}", std::process::id()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
