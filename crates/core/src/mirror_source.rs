//! The user's answer of where a Repository's mirror gets its objects (#172): all from GitHub, or borrowed
//! from a clone the user has. `settings` keeps the answers in `machine.toml`, and `mirror` clones by them.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::repository::RepositoryName;

/// The word `machine.toml` keeps for a mirror that downloads everything. Any other answer is a folder.
const DOWNLOAD: &str = "download";

/// The user's answer of where a Repository's mirror gets its objects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MirrorSource {
    /// All from GitHub.
    Download,
    /// Objects from the clone in `folder` where it has them, and the rest from GitHub.
    Borrow { folder: PathBuf },
}

impl MirrorSource {
    /// The answer `machine.toml` keeps as `text`.
    pub(crate) fn from_text(text: &str) -> Self {
        if text == DOWNLOAD {
            Self::Download
        } else {
            Self::Borrow {
                folder: PathBuf::from(text),
            }
        }
    }

    /// This answer as `machine.toml` keeps it.
    pub(crate) fn to_text(&self) -> String {
        match self {
            Self::Download => DOWNLOAD.to_owned(),
            Self::Borrow { folder } => folder.display().to_string(),
        }
    }
}

/// Each Repository's answer on this machine. A Repository left out hasn't been asked.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MirrorSources(Vec<(RepositoryName, MirrorSource)>);

impl MirrorSources {
    pub(crate) fn new(answers: Vec<(RepositoryName, MirrorSource)>) -> Self {
        Self(answers)
    }

    /// Keeps `source` as the answer for `repository`, in place of any before.
    pub(crate) fn set(&mut self, repository: RepositoryName, source: MirrorSource) {
        self.0.retain(|(one, _)| !one.is_same(&repository));
        self.0.push((repository, source));
    }

    /// The user's answer for `repository`, if they gave one.
    pub fn get(&self, repository: &RepositoryName) -> Option<&MirrorSource> {
        self.0
            .iter()
            .find(|(one, _)| one.is_same(repository))
            .map(|(_, source)| source)
    }
}
