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

    /// The Filters a new Repository starts with, which the user can edit or delete (docs/scope.md).
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
mod tests {
    use super::*;

    fn from_toml(text: &str) -> Result<Filter, toml_edit::de::Error> {
        toml_edit::de::from_str(text)
    }

    #[test]
    fn a_maintained_repository_starts_with_everything_open() {
        let queries: Vec<(String, String)> = Filter::starters(RepositoryAccess::Maintained)
            .into_iter()
            .map(|filter| (filter.name.into(), filter.query.into()))
            .collect();
        assert_eq!(
            queries,
            [
                ("Open Issues".to_owned(), "is:issue is:open".to_owned()),
                ("Open PRs".to_owned(), "is:pr is:open".to_owned()),
            ]
        );
    }

    #[test]
    fn any_other_repository_starts_with_the_users_own_work() {
        let queries: Vec<(String, String)> = Filter::starters(RepositoryAccess::Other)
            .into_iter()
            .map(|filter| (filter.name.into(), filter.query.into()))
            .collect();
        assert_eq!(
            queries,
            [
                ("Mine".to_owned(), "is:open author:@me".to_owned()),
                (
                    "Involved".to_owned(),
                    "is:open involves:@me -author:@me".to_owned()
                ),
            ]
        );
    }

    #[test]
    fn a_filter_from_the_file_loses_the_spaces_around_it() {
        let filter = from_toml("name = ' Bugs '\nquery = ' label:bug '").unwrap();
        assert_eq!(filter, Filter::starter("Bugs", "label:bug"));
        assert_eq!(filter.query.as_str(), "label:bug");
    }

    #[test]
    fn a_blank_name_or_query_is_refused_and_says_which() {
        let no_name = from_toml("name = '  '\nquery = 'label:bug'").unwrap_err();
        assert!(
            no_name
                .to_string()
                .contains("A Filter's name can't be blank.")
        );
        let no_query = from_toml("name = 'Bugs'\nquery = ''").unwrap_err();
        assert!(
            no_query
                .to_string()
                .contains("A Filter's query can't be blank.")
        );
    }
}
