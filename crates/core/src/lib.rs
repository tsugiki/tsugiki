//! Tsugiki's core: everything the app knows and decides, with no UI (ADR 0007).

pub mod error;
pub mod folders;
pub mod github;
pub mod logfile;
pub mod session;
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

#[cfg(test)]
mod tests {
    use super::*;

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
