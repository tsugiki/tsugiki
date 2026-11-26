//! Filters: the saved GitHub searches each Repository keeps in `settings.toml` (ADR 0014, ADR 0015).

use serde::{Deserialize, Serialize};
use specta::Type;

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

impl FilterName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

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
    /// A Filter the file writes without it is off the Workbench.
    #[serde(default)]
    pub on_workbench: OnWorkbench,
}

/// Whether a Filter's items go on the Workbench, and so into Reach (#819). One off it is a view the user
/// browses, whose items Tsugiki keeps no head of and reads ahead nothing of. `settings.toml` and the window
/// write it as a bool.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(from = "bool", into = "bool")]
#[specta(type = bool)]
pub enum OnWorkbench {
    On,
    #[default]
    Off,
}

impl From<bool> for OnWorkbench {
    fn from(on: bool) -> Self {
        if on { Self::On } else { Self::Off }
    }
}

impl From<OnWorkbench> for bool {
    fn from(switch: OnWorkbench) -> Self {
        switch == OnWorkbench::On
    }
}

/// Whether the search `token` names a state: `is:open`, `is:closed`, `is:merged`, `is:unmerged` or
/// `state:` (#464, #465), excluded with a leading `-` or not. A query that names none gets `is:open`. The
/// cache leaves an excluded token to GitHub whole; the typed GitHub query counts it, so `-is:open` never
/// gets an `is:open` that would contradict it.
pub(crate) fn names_a_state(token: &str) -> bool {
    let token = token.strip_prefix('-').unwrap_or(token);
    let Some((qualifier, value)) = token.split_once(':') else {
        return false;
    };
    match qualifier.to_ascii_lowercase().as_str() {
        "state" => true,
        "is" => matches!(
            value.to_ascii_lowercase().as_str(),
            "open" | "closed" | "merged" | "unmerged"
        ),
        _ => false,
    }
}

/// Whether `token` is the `sort:` qualifier, so Tsugiki adds no `sort:updated-desc` of its own (#465).
pub(crate) fn names_a_sort(token: &str) -> bool {
    if token.starts_with('-') {
        return false;
    }
    token
        .split_once(':')
        .is_some_and(|(qualifier, _)| qualifier.eq_ignore_ascii_case("sort"))
}

impl Filter {
    fn starter(name: &str, query: &str, on_workbench: OnWorkbench) -> Self {
        Self {
            name: FilterName(name.to_owned()),
            query: FilterQuery(query.to_owned()),
            on_workbench,
        }
    }

    /// The Filters a new Repository starts with, Maintained or not, which the user can edit or delete
    /// (docs/devel/workbench.md). Only the user's own work goes on the Workbench: the Inbox lists every open
    /// item.
    pub fn starters() -> Vec<Self> {
        vec![
            Self::starter("Mine", "is:open author:@me", OnWorkbench::On),
            Self::starter(
                "Involved",
                "is:open involves:@me -author:@me",
                OnWorkbench::Off,
            ),
        ]
    }

    /// The Filters of a Repository whose settings leave them out: the starters, each switched off. Before
    /// Filters had the switch (#819), such a Repository had these two, and a Filter it had then stays off.
    pub fn left_out() -> Vec<Self> {
        let mut filters = Self::starters();
        for filter in &mut filters {
            filter.on_workbench = OnWorkbench::Off;
        }
        filters
    }
}

#[cfg(test)]
mod tests;
