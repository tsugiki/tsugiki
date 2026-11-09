//! A GitHub username, which `github` reads and the modules above it compare.

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
