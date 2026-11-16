//! The labels of an Issue or a PR, which a search of the cache matches with `label:` (#463).

use serde::{Deserialize, Serialize};
use specta::Type;

/// The name of a label, as GitHub spells it. It reaches the UI as a plain string.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct Label(String);

/// GitHub's label names ignore case, so a Repository can't hold both `docs` and `Docs`, and `label:Docs`
/// finds either.
impl PartialEq for Label {
    fn eq(&self, other: &Self) -> bool {
        self.0.eq_ignore_ascii_case(&other.0)
    }
}

impl Eq for Label {}

impl Label {
    pub fn new(name: &str) -> Self {
        Self(name.to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests;
