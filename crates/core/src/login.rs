//! A GitHub username, which `github` reads and the modules above it compare.

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};
use specta::Type;

/// A GitHub username. It reaches the UI as a plain string, in GitHub's casing.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct Login(String);

/// GitHub logins ignore case, so `Octocat` and `octocat` are one user, and "is this me?" has one answer.
impl PartialEq for Login {
    fn eq(&self, other: &Self) -> bool {
        self.0.eq_ignore_ascii_case(&other.0)
    }
}

impl Eq for Login {}

/// The order ignores case as `eq` does, so a map keyed by login holds one entry for each user.
impl Ord for Login {
    fn cmp(&self, other: &Self) -> Ordering {
        let (ours, theirs) = (self.0.bytes(), other.0.bytes());
        ours.map(|byte| byte.to_ascii_lowercase())
            .cmp(theirs.map(|byte| byte.to_ascii_lowercase()))
    }
}

impl PartialOrd for Login {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Login {
    pub fn new(login: &str) -> Self {
        Self(login.to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests;
