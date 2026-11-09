//! A Repository: its name, the user's standing in it, and its size on GitHub (ADR 0015).

use std::fmt;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::error::{Error, ErrorKind};

const BAD_NAME: &str = "Write the Repository as owner/name, such as python/cpython.";
const GITHUB_PREFIXES: [&str; 2] = ["https://github.com/", "github.com/"];
const MAX_OWNER_LENGTH: usize = 39;
const MAX_NAME_LENGTH: usize = 100;

/// A Repository's `owner/name`, in the only form that reaches GitHub. It reaches the UI as a plain string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(try_from = "String", into = "String")]
#[specta(type = String)]
pub struct RepositoryName {
    owner: String,
    name: String,
}

impl RepositoryName {
    /// Reads what a user typed or pasted: `owner/name`, or the Repository's address on GitHub.
    pub fn parse(text: &str) -> Result<Self, Error> {
        let bad = || Error::new(ErrorKind::NotFound, BAD_NAME);
        let text = text.trim().trim_end_matches('/');
        let text = GITHUB_PREFIXES
            .iter()
            .find_map(|prefix| text.strip_prefix(prefix))
            .unwrap_or(text);
        let text = text.strip_suffix(".git").unwrap_or(text);
        let (owner, name) = text.split_once('/').ok_or_else(bad)?;
        if is_owner(owner) && is_name(name) {
            Ok(Self {
                owner: owner.to_owned(),
                name: name.to_owned(),
            })
        } else {
            Err(bad())
        }
    }

    pub fn owner(&self) -> &str {
        &self.owner
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether both name one Repository. GitHub ignores case in names, and a hand-edited file may differ.
    pub fn is_same(&self, other: &Self) -> bool {
        self.to_string().eq_ignore_ascii_case(&other.to_string())
    }
}

impl fmt::Display for RepositoryName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.owner, self.name)
    }
}

impl TryFrom<String> for RepositoryName {
    type Error = String;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        Self::parse(&text).map_err(|problem| problem.message)
    }
}

impl From<RepositoryName> for String {
    fn from(name: RepositoryName) -> Self {
        name.to_string()
    }
}

fn is_owner(owner: &str) -> bool {
    !owner.is_empty()
        && owner.len() <= MAX_OWNER_LENGTH
        && owner.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

fn is_name(name: &str) -> bool {
    let allowed = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.');
    !name.is_empty()
        && name.len() <= MAX_NAME_LENGTH
        && !matches!(name, "." | "..")
        && name.chars().all(allowed)
}

/// The number `digits` spell. GitHub's numbers are GraphQL `Int`s, 32 bits and signed, and it refuses a whole
/// query over one that doesn't fit.
pub(crate) fn issue_number(digits: &str) -> Option<u32> {
    digits
        .chars()
        .all(|c| c.is_ascii_digit())
        .then(|| digits.parse::<i32>().ok())
        .flatten()
        .and_then(|number| u32::try_from(number).ok())
}

/// The user's standing in a Repository, as far as adding it needs to know.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RepositoryAccess {
    /// The user administers it on GitHub.
    Maintained,
    Other,
}

/// Up to this many open Issues and PRs loads everything open, and more loads a window (ADR 0015).
pub(crate) const SMALL_UP_TO: u32 = 500;
/// How far back the first sync looks, in days (ADR 0015).
pub const WINDOW_DAYS: u8 = 30;

/// Which first sync a Repository gets (ADR 0015).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RepositorySize {
    /// Everything open, plus what closed in the window.
    Small,
    /// What was updated in the window.
    Large,
}

/// What GitHub says about a Repository, from the one query adding it costs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RepositoryCounts {
    pub open_issues: u32,
    pub open_prs: u32,
    pub updated_in_window: u32,
    pub closed_in_window: u32,
    /// The Repository's size on GitHub, about what its mirror takes on the disk.
    pub disk_kb: u64,
}

#[cfg(test)]
mod tests;
