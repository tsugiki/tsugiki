//! Filters: the saved GitHub searches each Repository keeps in `settings.toml` (ADR 0014, ADR 0015).

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::repository::RepositoryAccess;

/// Text in `settings.toml` that means nothing when blank, so a blank one stops at the file's boundary.
macro_rules! never_blank {
    ($name:ident, $what:literal) => {
        impl TryFrom<String> for $name {
            type Error = String;

            fn try_from(text: String) -> Result<Self, Self::Error> {
                let text = text.trim();
                if text.is_empty() {
                    Err(format!("{} can't be blank.", $what))
                } else {
                    Ok(Self(text.to_owned()))
                }
            }
        }

        impl From<$name> for String {
            fn from(text: $name) -> Self {
                text.0
            }
        }
    };
}

/// A Filter's name in the Sidebar. It reaches the UI as a plain string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(try_from = "String", into = "String")]
#[specta(type = String)]
pub struct FilterName(String);

never_blank!(FilterName, "A Filter's name");

/// A Filter's search as the user wrote it, without the Repository. It also names the Filter's results, so
/// renaming a Filter keeps them and editing its search replaces them at the next sync. It reaches the UI as a
/// plain string.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Type)]
#[serde(try_from = "String", into = "String")]
#[specta(type = String)]
pub struct FilterQuery(String);

never_blank!(FilterQuery, "A Filter's query");

impl FilterQuery {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A saved GitHub search, as `settings.toml` holds it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Filter {
    pub name: FilterName,
    pub query: FilterQuery,
}

impl Filter {
    fn starter(name: &str, query: &str) -> Self {
        Self {
            name: FilterName(name.to_owned()),
            query: FilterQuery(query.to_owned()),
        }
    }

    /// The Filters a new Repository starts with, which the user can edit or delete (docs/devel/workbench.md).
    pub fn starters(access: RepositoryAccess) -> Vec<Self> {
        match access {
            RepositoryAccess::Maintained => vec![
                Self::starter("Open Issues", "is:issue is:open"),
                Self::starter("Open PRs", "is:pr is:open"),
            ],
            RepositoryAccess::Other => vec![
                Self::starter("Mine", "is:open author:@me"),
                Self::starter("Involved", "is:open involves:@me -author:@me"),
            ],
        }
    }
}

#[cfg(test)]
mod tests;
