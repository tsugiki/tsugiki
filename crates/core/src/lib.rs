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
pub mod login;
pub mod measurement;
pub mod mirror;
pub mod misses;
pub mod releases;
pub mod repository;
pub mod repository_settings;
pub mod session;
pub mod settings;
pub mod status;
pub mod prototype_definitions;
pub mod store;
pub mod sync;
pub mod token;
pub mod version;
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

pub fn about(version: &version::Version) -> About {
    About {
        name: "Tsugiki".to_owned(),
        version: version.to_string(),
    }
}

/// What the core's tests share.
#[cfg(test)]
pub(crate) mod testing;

#[cfg(test)]
mod tests;
